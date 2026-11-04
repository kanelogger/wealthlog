use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::{
    apportion_transfer,
    model::{apply_deltas, invalid, nonblank},
    AllocationDelta, ExactDecimal, FundResource, JournalError, JournalResult, ResourceBalance,
    ResourceObservation, TradeImpactPreview,
};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnershipTopUp {
    pub resource: FundResource,
    pub amount: ExactDecimal,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TradePurposeShare {
    pub purpose_id: String,
    /// Positive original trade units assigned to this purpose.
    pub quantity: ExactDecimal,
    /// Signed actual settlement cash, independently confirmed per purpose.
    pub net_cash: ExactDecimal,
    #[serde(default)]
    pub top_ups: Vec<OwnershipTopUp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PurposeTradeCharges {
    pub purpose_id: String,
    pub currency: String,
    pub fee: Option<ExactDecimal>,
    pub tax: Option<ExactDecimal>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PurposeResourceBalance {
    pub purpose_id: String,
    pub resource: FundResource,
    pub amount: ExactDecimal,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeOwnershipPreview {
    pub schema_version: u32,
    /// These two append-only legs remain distinct even when their net is zero.
    pub top_up_entries: Vec<AllocationDelta>,
    pub conversion_entries: Vec<AllocationDelta>,
    /// Detail only: charges already belong to final net cash, never a third debit.
    pub charges: Vec<PurposeTradeCharges>,
    pub balances_after: Vec<PurposeResourceBalance>,
}

/// Validate every purpose together against the *before-trade* resources. The
/// caller freezes top-ups in its request, then commits facts and both legs in
/// one transaction. Existing-source evidence links must not call this function.
pub fn preview_trade_ownership(
    trade: &TradeImpactPreview,
    shares: &[TradePurposeShare],
    balances: &BTreeMap<(String, FundResource), ExactDecimal>,
    observations: &[ResourceObservation],
) -> JournalResult<TradeOwnershipPreview> {
    if !trade.automatic_allocation_eligible {
        return Err(JournalError::ReconciliationRequired);
    }
    if shares.is_empty() {
        return Err(invalid("at least one purpose share is required"));
    }
    let securities: Vec<_> = trade
        .changes
        .iter()
        .filter(|c| matches!(c.resource, FundResource::Security { .. }))
        .collect();
    let [security] = securities.as_slice() else {
        return Err(invalid(
            "ownership conversion requires one affected security",
        ));
    };
    let total_quantity = ExactDecimal::new(security.delta.value().abs());
    if total_quantity == ExactDecimal::ZERO {
        return Err(invalid(
            "ownership conversion requires a nonzero security quantity",
        ));
    }
    let net_cash = trade.net_cash.ok_or(JournalError::ResourceUnavailable)?;
    let cash = FundResource::Cash {
        account_id: security.resource.account_id().to_string(),
        currency: trade
            .settlement_currency
            .clone()
            .ok_or(JournalError::ResourceUnavailable)?,
    };
    let cash_change = trade
        .changes
        .iter()
        .find(|c| c.resource == cash)
        .map(|c| c.delta)
        .unwrap_or_default();
    if cash_change != net_cash
        || trade
            .changes
            .iter()
            .any(|c| c.resource != cash && c.resource != security.resource)
    {
        return Err(invalid("trade cash/quantity impact is inconsistent"));
    }
    let mut ids = BTreeSet::new();
    let mut quantity_sum = ExactDecimal::ZERO;
    let mut cash_sum = ExactDecimal::ZERO;
    let mut top_up_entries = Vec::new();
    let mut conversion_entries = Vec::new();
    let mut ordered = shares.to_vec();
    ordered.sort_by(|a, b| a.purpose_id.cmp(&b.purpose_id));
    for share in &ordered {
        nonblank(&share.purpose_id, "purposeId")?;
        if !ids.insert(&share.purpose_id) || share.quantity <= ExactDecimal::ZERO {
            return Err(invalid(
                "purpose shares must have positive quantities and unique IDs",
            ));
        }
        if (net_cash > ExactDecimal::ZERO && share.net_cash < ExactDecimal::ZERO)
            || (net_cash < ExactDecimal::ZERO && share.net_cash > ExactDecimal::ZERO)
            || (net_cash == ExactDecimal::ZERO && share.net_cash != ExactDecimal::ZERO)
        {
            return Err(invalid(
                "purpose cash direction must match actual settlement cash",
            ));
        }
        quantity_sum = quantity_sum.checked_add(share.quantity)?;
        cash_sum = cash_sum.checked_add(share.net_cash)?;
        let mut resources = BTreeSet::new();
        for top_up in &share.top_ups {
            top_up.resource.validate()?;
            let consumed = trade
                .changes
                .iter()
                .any(|c| c.resource == top_up.resource && c.delta < ExactDecimal::ZERO);
            if !consumed
                || top_up.amount <= ExactDecimal::ZERO
                || !resources.insert(&top_up.resource)
            {
                return Err(invalid("top-ups must explicitly name a consumed resource once and use a positive amount"));
            }
            top_up_entries.push(AllocationDelta {
                purpose_id: share.purpose_id.clone(),
                resource: top_up.resource.clone(),
                delta: top_up.amount,
            });
        }
        let quantity = if security.delta > ExactDecimal::ZERO {
            share.quantity
        } else {
            ExactDecimal::new(-share.quantity.value())
        };
        conversion_entries.push(AllocationDelta {
            purpose_id: share.purpose_id.clone(),
            resource: security.resource.clone(),
            delta: quantity,
        });
        if share.net_cash != ExactDecimal::ZERO {
            conversion_entries.push(AllocationDelta {
                purpose_id: share.purpose_id.clone(),
                resource: cash.clone(),
                delta: share.net_cash,
            });
        }
    }
    if quantity_sum != total_quantity || cash_sum != net_cash {
        return Err(invalid(
            "all purpose shares must conserve actual net cash and security quantity",
        ));
    }
    let actual: BTreeMap<_, _> = observations
        .iter()
        .map(|o| (o.resource.clone(), o.actual))
        .collect();
    // A top-up is only the unallocated part of this same resource. Capacity is
    // checked across all purposes together by apply_deltas.
    let funded = apply_deltas(balances, &top_up_entries, &actual)?;
    let mut after_actual = actual.clone();
    for change in &trade.changes {
        let before = after_actual.get(&change.resource).copied().flatten();
        after_actual.insert(
            change.resource.clone(),
            before.map(|b| b.checked_add(change.delta)).transpose()?,
        );
    }
    // The before resource must be known even when all consumed units are
    // already assigned; unknown B cannot prove a safe conversion.
    for resource in [&cash, &security.resource] {
        if after_actual.get(resource).copied().flatten().is_none() {
            return Err(JournalError::ResourceUnavailable);
        }
    }
    let mut final_balances = funded;
    for entry in &conversion_entries {
        let key = (entry.purpose_id.clone(), entry.resource.clone());
        let next = final_balances
            .get(&key)
            .copied()
            .unwrap_or_default()
            .checked_add(entry.delta)?;
        if next < ExactDecimal::ZERO {
            return Err(JournalError::NegativeAllocation);
        }
        final_balances.insert(key, next);
    }
    // A conserving conversion may preserve an existing excess, never enlarge
    // it. Negative real balances still cannot become positive purpose money.
    for resource in [&cash, &security.resource] {
        let allocated = final_balances
            .iter()
            .filter(|((_, r), _)| r == resource)
            .try_fold(ExactDecimal::ZERO, |sum, (_, v)| sum.checked_add(*v))?;
        let old_allocated = balances
            .iter()
            .filter(|((_, r), _)| r == resource)
            .try_fold(ExactDecimal::ZERO, |sum, (_, v)| sum.checked_add(*v))?;
        let old_excess =
            ResourceBalance::calculate(actual.get(resource).copied().flatten(), old_allocated)?
                .excess
                .ok_or(JournalError::ResourceUnavailable)?;
        if ResourceBalance::calculate(after_actual.get(resource).copied().flatten(), allocated)?
            .excess
            .is_some_and(|e| e > old_excess)
        {
            return Err(JournalError::InsufficientResource);
        }
    }
    let weights: Vec<_> = ordered
        .iter()
        .map(|s| (s.purpose_id.clone(), s.quantity))
        .collect();
    let apportion = |total: Option<ExactDecimal>,
                     precision: Option<u32>|
     -> JournalResult<BTreeMap<String, Option<ExactDecimal>>> {
        match total {
            None => Ok(weights.iter().map(|(id, _)| (id.clone(), None)).collect()),
            Some(v) if v == ExactDecimal::ZERO => Ok(weights
                .iter()
                .map(|(id, _)| (id.clone(), Some(ExactDecimal::ZERO)))
                .collect()),
            Some(v) => Ok(apportion_transfer(
                total_quantity,
                v,
                &trade.activity_currency,
                precision.unwrap_or(v.value().scale()),
                &weights,
            )?
            .into_iter()
            .map(|s| (s.purpose_id, Some(s.destination_amount)))
            .collect()),
        }
    };
    let fees = apportion(trade.fee, trade.fee_precision)?;
    let taxes = apportion(trade.tax, trade.tax_precision)?;
    let charges = ordered
        .iter()
        .map(|s| PurposeTradeCharges {
            purpose_id: s.purpose_id.clone(),
            currency: trade.activity_currency.clone(),
            fee: fees.get(&s.purpose_id).copied().flatten(),
            tax: taxes.get(&s.purpose_id).copied().flatten(),
        })
        .collect();
    let changed: BTreeSet<_> = top_up_entries
        .iter()
        .chain(&conversion_entries)
        .map(|e| (e.purpose_id.clone(), e.resource.clone()))
        .collect();
    let balances_after = final_balances
        .into_iter()
        .filter(|(k, _)| changed.contains(k))
        .map(|((purpose_id, resource), amount)| PurposeResourceBalance {
            purpose_id,
            resource,
            amount,
        })
        .collect();
    Ok(TradeOwnershipPreview {
        schema_version: 1,
        top_up_entries,
        conversion_entries,
        charges,
        balances_after,
    })
}
