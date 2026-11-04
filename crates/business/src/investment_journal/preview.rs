use std::collections::{BTreeMap, BTreeSet};

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use wealthfolio_portfolio_engine::{self as engine, model::*};

use super::model::invalid;
use super::{ExactDecimal, FundResource, JournalResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceChange {
    pub resource: FundResource,
    pub delta: ExactDecimal,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TradeImpactPreview {
    pub schema_version: u32,
    pub source_id: String,
    pub changes: Vec<ResourceChange>,
    pub automatic_allocation_eligible: bool,
    pub net_cash: Option<ExactDecimal>,
    pub settlement_currency: Option<String>,
    /// Preserve the difference between unavailable fees and known zero.
    pub fee: Option<ExactDecimal>,
    pub tax: Option<ExactDecimal>,
    pub fee_precision: Option<u32>,
    pub tax_precision: Option<u32>,
    pub activity_currency: String,
    pub cash_adjustment: Option<ExactDecimal>,
    pub issues: Vec<String>,
    pub facts_hash: String,
}

/// Preview a ledger trade by folding the same facts with and without it.
/// The kernel owns signs, fee-inclusive final cash, settlement FX and rejects.
/// Cost, lots and realised gains are intentionally absent from comparison.
pub fn preview_ledger_trade(
    facts: RawFacts,
    trade: RawActivity,
) -> JournalResult<TradeImpactPreview> {
    preview_ledger_trade_with_gross(facts, trade).map(|(preview, _)| preview)
}

/// The execution index also needs gross cash from the compiled trade event.
/// Return it from the same kernel pass so the index need not compile the
/// account's full history again.
pub(super) fn preview_ledger_trade_with_gross(
    mut facts: RawFacts,
    trade: RawActivity,
) -> JournalResult<(TradeImpactPreview, Option<ExactDecimal>)> {
    let account = facts
        .accounts
        .iter()
        .find(|a| a.id == trade.account_id)
        .ok_or_else(|| invalid("trade account is missing"))?;
    if account.tracking_mode == "HOLDINGS" || account.is_archived {
        return Err(invalid(
            "ledger trade preview requires a non-archived transactions account",
        ));
    }
    let account_currency = account.currency.clone();
    let asset = facts
        .assets
        .iter()
        .find(|a| Some(a.id.as_str()) == trade.asset_id.as_deref());
    if asset.is_none_or(|a| {
        a.kind != "INVESTMENT"
            || !matches!(a.instrument_type.as_deref(), None | Some("EQUITY"))
            || a.contract_multiplier.is_some_and(|m| m != Decimal::ONE)
    }) {
        return Err(invalid(
            "journal trade preview supports stock/ETF/fund assets with a unit multiplier only",
        ));
    }
    let side = wealthfolio_core::activities::effective_activity_type(
        &trade.activity_type,
        trade.activity_type_override.as_deref(),
    );
    if !matches!(side, "BUY" | "SELL") || trade.status != "POSTED" {
        return Err(invalid("preview requires a posted BUY or SELL"));
    }
    if facts
        .activities
        .iter()
        .any(|a| a.id == trade.id && *a != trade)
    {
        return Err(invalid(
            "candidate identity already belongs to a different source",
        ));
    }
    facts.activities.retain(|a| a.id != trade.id);
    let before_engine = engine::Engine::new(facts.clone()).map_err(|e| invalid(e.to_string()))?;
    let before = before_engine
        .project()
        .map_err(|e| invalid(e.to_string()))?;
    facts.activities.push(trade.clone());
    let facts_hash = stable_facts_hash(&facts)?;
    let after_engine = engine::Engine::new(facts).map_err(|e| invalid(e.to_string()))?;
    let after = after_engine.project().map_err(|e| invalid(e.to_string()))?;
    let changes = resource_changes(&before.final_state, &after.final_state)?;
    let events: Vec<_> = after_engine
        .ledger()
        .events
        .iter()
        .filter(|e| e.source.as_str() == trade.id)
        .collect();
    let gross = if trade.fee.is_some() && trade.tax.is_some() {
        events
            .first()
            .and_then(|event| event.cash.as_ref())
            .and_then(|cash| cash.gross)
            .map(|value| ExactDecimal::new(value.abs()))
    } else {
        None
    };
    let mut issues: BTreeSet<String> = after_engine
        .normalize_diagnostics()
        .iter()
        .chain(after_engine.ledger().diagnostics.iter())
        .chain(after.diagnostics.iter())
        .filter(|d| d.source == trade.id || d.severity == engine::Severity::Error)
        .map(|d| format!("{:?}", d.code))
        .collect();
    let mut expected = BTreeMap::new();
    let mut net_cash = None;
    let mut settlement_currency = None;
    if let [event] = events.as_slice() {
        if let Action::Trade {
            asset,
            side,
            quantity,
            ..
        } = &event.action
        {
            let signed = if *side == Side::Buy {
                *quantity
            } else {
                -*quantity
            };
            expected.insert(
                FundResource::Security {
                    account_id: trade.account_id.clone(),
                    asset_id: asset.as_str().into(),
                },
                signed,
            );
            if let Some(cash) = &event.cash {
                let (currency, amount) = match cash.booking {
                    Booking::ActivityCurrency => {
                        (event.currency.as_str().to_string(), Some(cash.amount))
                    }
                    Booking::AccountCurrency { rate } => {
                        (account_currency, cash.amount.checked_mul(rate))
                    }
                };
                if let Some(amount) = amount {
                    expected.insert(
                        FundResource::Cash {
                            account_id: trade.account_id.clone(),
                            currency: currency.clone(),
                        },
                        amount,
                    );
                    net_cash = Some(ExactDecimal::new(amount));
                    settlement_currency = Some(currency);
                } else {
                    issues.insert("CashOutOfRange".into());
                }
            } else {
                issues.insert("MissingFinalCash".into());
            }
        } else {
            issues.insert("UnsupportedTradeAction".into());
        }
    } else {
        issues.insert("TradeNotAcceptedAsSingleEvent".into());
    }
    expected.retain(|_, v| !v.is_zero());
    let actual: BTreeMap<_, _> = changes
        .iter()
        .map(|r| (r.resource.clone(), r.delta.value()))
        .collect();
    if expected != actual {
        issues.insert("ResourceImpactMismatch".into());
    }
    let blocked = issues
        .iter()
        .any(|code| !matches!(code.as_str(), "NegativeCash" | "FxUnavailable"));
    // Missing fee/tax does not invalidate a reliable final net ledger amount.
    Ok((
        TradeImpactPreview {
            schema_version: 1,
            source_id: trade.id,
            changes,
            automatic_allocation_eligible: !blocked && net_cash.is_some(),
            net_cash,
            settlement_currency,
            fee: trade.fee.map(ExactDecimal::new),
            tax: trade.tax.map(ExactDecimal::new),
            fee_precision: trade.fee.map(|v| v.scale()),
            tax_precision: trade.tax.map(|v| v.scale()),
            activity_currency: trade.currency,
            cash_adjustment: None,
            issues: issues.into_iter().collect(),
            facts_hash,
        },
        gross,
    ))
}

impl crate::investments::InvestmentService {
    /// Same preparation used by record_manual_trade; read-only, today-only and
    /// guarded by the original expectedStateVersion contract.
    pub async fn preview_manual_trade(
        &self,
        input: &crate::investments::RecordManualTrade,
    ) -> std::result::Result<TradeImpactPreview, crate::investments::InvestmentError> {
        let prepared = self.prepare_manual_trade(input).await?;
        let old_quantity = prepared
            .baseline
            .positions
            .get(&input.asset_id)
            .map(|p| p.quantity)
            .unwrap_or_default();
        let new_quantity = prepared
            .snapshot
            .positions
            .get(&input.asset_id)
            .map(|p| p.quantity)
            .unwrap_or_default();
        let mut issues = Vec::new();
        if input.fee.is_none() {
            issues.push("FeeUnknown".into());
        }
        if input.tax.is_none() {
            issues.push("TaxUnknown".into());
        }
        if !prepared.cash_adjustment.is_zero() {
            issues.push("IndependentCashCalibration".into());
        }
        let fee = input
            .fee
            .as_deref()
            .map(ExactDecimal::parse)
            .transpose()
            .map_err(|e| crate::investments::InvestmentError::InvalidInput(e.to_string()))?;
        let tax = input
            .tax
            .as_deref()
            .map(ExactDecimal::parse)
            .transpose()
            .map_err(|e| crate::investments::InvestmentError::InvalidInput(e.to_string()))?;
        let actual_cash_change = prepared
            .trade_cash_change
            .checked_add(prepared.cash_adjustment)
            .ok_or_else(|| {
                crate::investments::InvestmentError::InvalidInput("cash change out of range".into())
            })?;
        let changes = vec![
            ResourceChange {
                resource: FundResource::Cash {
                    account_id: input.account_id.clone(),
                    currency: input.currency.clone(),
                },
                delta: ExactDecimal::new(actual_cash_change),
            },
            ResourceChange {
                resource: FundResource::Security {
                    account_id: input.account_id.clone(),
                    asset_id: input.asset_id.clone(),
                },
                delta: ExactDecimal::new(new_quantity - old_quantity),
            },
        ];
        let mut body = serde_json::to_value(input)
            .map_err(|e| crate::investments::InvestmentError::InvalidInput(e.to_string()))?;
        if let Some(object) = body.as_object_mut() {
            object.remove("requestKey");
        }
        let facts_hash = crate::requests::hash_json(&crate::requests::canonicalize(
            json!({"schemaVersion":1,"trade":body,"changes":changes,"fee":fee,"tax":tax,"cashAdjustment":ExactDecimal::new(prepared.cash_adjustment)}),
        ));
        Ok(TradeImpactPreview {
            schema_version: 1,
            source_id: String::new(),
            changes,
            automatic_allocation_eligible: issues.is_empty(),
            net_cash: Some(ExactDecimal::new(prepared.trade_cash_change)),
            settlement_currency: Some(input.currency.clone()),
            fee,
            tax,
            fee_precision: input.fee.as_deref().map(recorded_precision),
            tax_precision: input.tax.as_deref().map(recorded_precision),
            activity_currency: input.currency.clone(),
            cash_adjustment: Some(ExactDecimal::new(prepared.cash_adjustment)),
            issues,
            facts_hash,
        })
    }
}

fn recorded_precision(value: &str) -> u32 {
    value
        .split_once('.')
        .map(|(_, fraction)| fraction.len() as u32)
        .unwrap_or_default()
}

fn stable_facts_hash(facts: &RawFacts) -> JournalResult<String> {
    let mut value = serde_json::to_value(facts).map_err(|e| invalid(e.to_string()))?;
    // Quotes/FX may change book valuations, never these native resource deltas.
    if let Some(o) = value.as_object_mut() {
        o.remove("quotes");
        o.remove("fx_rates");
    }
    for field in ["accounts", "assets", "activities"] {
        if let Some(rows) = value[field].as_array_mut() {
            rows.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        }
    }
    if let Some(rows) = value["observed_snapshots"].as_array_mut() {
        for row in rows.iter_mut() {
            if let Some(cash) = row["cash"].as_array_mut() {
                cash.sort_by(|a, b| a[0].as_str().cmp(&b[0].as_str()));
            }
            if let Some(positions) = row["positions"].as_array_mut() {
                positions.sort_by(|a, b| a["asset_id"].as_str().cmp(&b["asset_id"].as_str()));
            }
        }
        rows.sort_by(|a, b| {
            a["account_id"]
                .as_str()
                .cmp(&b["account_id"].as_str())
                .then(a["date"].as_str().cmp(&b["date"].as_str()))
        });
    }
    Ok(crate::requests::hash_json(&crate::requests::canonicalize(
        json!({"schemaVersion":1,"facts":value}),
    )))
}

fn resource_changes(
    before: &ProjectionState,
    after: &ProjectionState,
) -> JournalResult<Vec<ResourceChange>> {
    let flatten = |state: &ProjectionState| {
        let mut result = BTreeMap::new();
        for (id, account) in &state.accounts {
            for (currency, amount) in &account.cash {
                result.insert(
                    FundResource::Cash {
                        account_id: id.as_str().into(),
                        currency: currency.as_str().into(),
                    },
                    *amount,
                );
            }
            for (asset, position) in &account.positions {
                result.insert(
                    FundResource::Security {
                        account_id: id.as_str().into(),
                        asset_id: asset.as_str().into(),
                    },
                    position.quantity,
                );
            }
        }
        result
    };
    let old = flatten(before);
    let new = flatten(after);
    let keys: BTreeSet<_> = old.keys().chain(new.keys()).cloned().collect();
    let mut changes = Vec::new();
    for resource in keys {
        let delta = new
            .get(&resource)
            .copied()
            .unwrap_or_default()
            .checked_sub(old.get(&resource).copied().unwrap_or_default())
            .ok_or_else(|| invalid("resource delta out of range"))?;
        if delta != Decimal::ZERO {
            changes.push(ResourceChange {
                resource,
                delta: ExactDecimal::new(delta),
            });
        }
    }
    Ok(changes)
}
