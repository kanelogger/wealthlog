//! Purpose-owned valuation, separate from actual-account balances and loss budgets.
use super::super::model::invalid;
use super::super::{
    AllocationPreview, ExactDecimal, FundReconciliationItem, FundResource, JournalResult,
};
use super::accounting::AccountingContextReader;
use chrono::NaiveDate;
use num_bigint::BigInt;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use wealthfolio_core::fx::currency::normalize_amount_exact;

impl AccountingContextReader {
    pub(super) fn convert_observed(
        &self,
        amount: Decimal,
        from: &str,
        to: &str,
        day: NaiveDate,
        fx_ready: bool,
    ) -> Option<(Decimal, Option<NaiveDate>)> {
        let (amount, from) = normalize_amount_exact(amount, from)?;
        if from == to || amount.is_zero() {
            return Some((amount, None));
        }
        if !fx_ready {
            return None;
        }
        let read = self
            .fx
            .get_exchange_rate_observation(from, to, Some(day))
            .ok()?;
        if !read.verified || read.rate <= Decimal::ZERO {
            return None;
        }
        amount
            .checked_mul(read.rate)
            .filter(|value| !value.is_zero())
            .map(|value| (value, read.source_date))
    }
    pub(super) async fn purpose_risks(
        &self,
        purposes: &[Value],
        allocations: &BTreeMap<(String, FundResource), ExactDecimal>,
        preview: &AllocationPreview,
        reconciliation: &[FundReconciliationItem],
        day: NaiveDate,
        fx_ready: bool,
    ) -> JournalResult<Vec<Value>> {
        let asset_ids: Vec<_> = allocations
            .keys()
            .filter(|(id, _)| purposes.iter().any(|p| p["id"].as_str() == Some(id)))
            .filter_map(|(_, r)| match r {
                FundResource::Security { asset_id, .. } => Some(asset_id.clone()),
                _ => None,
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let mut quotes = self
            .quotes
            .get_latest_quotes_as_of(&asset_ids, day)
            .unwrap_or_default();
        let selected: Vec<_> = quotes.values().cloned().collect();
        let verified = self
            .quotes
            .verify_quote_sources(&selected)
            .unwrap_or_default();
        quotes.retain(|_, quote| {
            verified.get(&quote.id) == Some(&wealthfolio_core::quotes::QuoteSourceQuality::Verified)
        });
        let unverified_security: BTreeSet<_> = reconciliation
            .iter()
            .map(|item| &item.resource)
            .filter(|resource| matches!(resource, FundResource::Security { .. }))
            .collect();
        let mut result = Vec::new();
        for purpose in purposes {
            let id = purpose["id"]
                .as_str()
                .ok_or_else(|| invalid("purpose has no id"))?;
            let currency = purpose["currency"]
                .as_str()
                .ok_or_else(|| invalid("purpose has no currency"))?;
            let mut cash = Some(Decimal::ZERO);
            let mut total = Some(Decimal::ZERO);
            let mut assets: BTreeMap<String, Option<Decimal>> = BTreeMap::new();
            let mut problems = Vec::new();
            let mut fx_dates = BTreeSet::new();
            for ((owner, resource), amount) in allocations {
                if owner != id || *amount == ExactDecimal::ZERO {
                    continue;
                }
                let state = preview
                    .resources
                    .iter()
                    .find(|r| &r.observation.resource == resource);
                // A zero excess only proves the allocated count fits the account.
                // A pending split or other source change can invalidate the unit
                // basis of those allocated securities without creating an excess.
                let resource_known = !unverified_security.contains(resource)
                    && state.is_some_and(|r| {
                        r.balance.actual.is_some() && r.balance.excess == Some(ExactDecimal::ZERO)
                    });
                let value = if !resource_known {
                    problems.push(json!({"code":"RESOURCE_UNRECONCILED","resource":resource}));
                    None
                } else {
                    match resource {
                        FundResource::Cash {
                            currency: source, ..
                        } => self
                            .convert_observed(amount.value(), source, currency, day, fx_ready)
                            .map(|(value, date)| {
                                fx_dates.extend(date);
                                value
                            }),
                        FundResource::Security { asset_id, .. } => quotes
                            .get(asset_id)
                            .filter(|q| q.close >= Decimal::ZERO)
                            .and_then(|quote| {
                                let (price, source) =
                                    normalize_amount_exact(quote.close, &quote.currency)?;
                                amount.value().checked_mul(price).and_then(|local| {
                                    self.convert_observed(local, source, currency, day, fx_ready)
                                        .map(|(value, date)| {
                                            fx_dates.extend(date);
                                            value
                                        })
                                })
                            }),
                    }
                };
                if value.is_none() && resource_known {
                    problems.push(json!({"code":"MISSING_VALUATION_OR_FX","resource":resource}));
                }
                total = add_optional(total, value);
                match resource {
                    FundResource::Cash { .. } => cash = add_optional(cash, value),
                    FundResource::Security { asset_id, .. } => {
                        let accumulated = assets
                            .entry(asset_id.clone())
                            .or_insert(Some(Decimal::ZERO));
                        *accumulated = add_optional(*accumulated, value);
                    }
                }
            }
            if (cash.is_none() || total.is_none()) && problems.is_empty() {
                problems.push(json!({"code":"VALUATION_OUT_OF_RANGE"}));
            }
            let minimum = purpose["minimumCash"]
                .as_str()
                .map(ExactDecimal::parse)
                .transpose()?;
            let cash_state = match (minimum, cash) {
                (None, _) => "NOT_CONFIGURED",
                (Some(_), None) => "UNABLE_TO_DETERMINE",
                (Some(min), Some(value)) if value < min.value() => "BELOW_MINIMUM",
                _ => "AT_OR_ABOVE_MINIMUM",
            };
            let limit = purpose["singleAssetLimitBps"].as_i64().map(|v| v as u32);
            let asset_values:Vec<_>=assets.into_iter().map(|(asset,value)|{
                let status=match (limit,value,total) {(None,_,_)=>"NOT_CONFIGURED",(Some(limit),Some(value),Some(total)) if total>Decimal::ZERO=>if ratio_exceeds(value,total,limit){"ABOVE_LIMIT"}else{"WITHIN_LIMIT"},_=>"UNABLE_TO_DETERMINE"};
                let date=quotes.get(&asset).map(|q|q.timestamp.date_naive());
                json!({"assetId":asset,"value":value.map(ExactDecimal::new),"valuationDate":date,"stale":date.is_some_and(|d|d<day),"limitStatus":status})
            }).collect();
            let limit_state = if limit.is_none() {
                "NOT_CONFIGURED"
            } else if total.is_none() || total == Some(Decimal::ZERO) {
                "UNABLE_TO_DETERMINE"
            } else if asset_values
                .iter()
                .any(|v| v["limitStatus"] == "ABOVE_LIMIT")
            {
                "ABOVE_LIMIT"
            } else {
                "WITHIN_LIMIT"
            };
            let source_date = asset_values
                .iter()
                .filter_map(|v| {
                    v["valuationDate"]
                        .as_str()
                        .and_then(|raw| NaiveDate::parse_from_str(raw, "%Y-%m-%d").ok())
                })
                .chain(fx_dates.iter().copied())
                .min();
            let stale = asset_values.iter().any(|v| v["stale"] == true)
                || fx_dates.iter().any(|date| *date < day);
            result.push(json!({"sourceDataAsOf":source_date,"fxSourceDate":fx_dates.iter().next(),"stale":stale,"purposeId":id,"currency":currency,"cashValue":cash.map(ExactDecimal::new),"totalValue":total.map(ExactDecimal::new),"minimumCash":minimum,"minimumCashStatus":cash_state,"singleAssetLimitBps":limit,"singleAssetLimitStatus":limit_state,"assets":asset_values,"lossBudget":purpose["lossBudget"],"lossComparisonStatus":"NO_SCENARIO_PROVIDED","quality":if problems.is_empty(){"COMPLETE"}else{"PARTIAL"},"issues":problems}));
        }
        Ok(result)
    }
}
fn add_optional(a: Option<Decimal>, b: Option<Decimal>) -> Option<Decimal> {
    a.zip(b).and_then(|(a, b)| a.checked_add(b))
}
fn ratio_exceeds(value: Decimal, total: Decimal, bps: u32) -> bool {
    let scale = value.scale().max(total.scale());
    let numerator = BigInt::from(value.mantissa())
        * BigInt::from(10u8).pow(scale - value.scale())
        * BigInt::from(10000u32);
    let denominator = BigInt::from(total.mantissa())
        * BigInt::from(10u8).pow(scale - total.scale())
        * BigInt::from(bps);
    numerator > denominator
}
