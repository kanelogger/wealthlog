use super::*;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct NetWorthDetails {
    pub issues: Vec<NetWorthIssue>,
    pub basis: BTreeMap<String, DetailedValuationBasis>,
    pub dates: BTreeMap<String, NaiveDate>,
    pub fx_dates: BTreeMap<String, NaiveDate>,
    pub fx_reads:
        BTreeMap<(String, String, NaiveDate, bool), Option<crate::fx::ExchangeRateObservation>>,
}
impl NetWorthDetails {
    #[allow(clippy::too_many_arguments)]
    pub fn issue(
        &mut self,
        code: NetWorthIssueCode,
        item: &str,
        account: Option<&str>,
        asset: Option<&str>,
        name: Option<&str>,
        category: &str,
        currency: Option<&str>,
    ) {
        let issue = NetWorthIssue {
            code,
            item_id: item.into(),
            account_id: account.map(str::to_string),
            asset_id: asset.map(str::to_string),
            name: name.map(str::to_string),
            category: category.into(),
            currency: currency.map(str::to_string),
        };
        if !self.issues.contains(&issue) {
            self.issues.push(issue);
        }
    }
    pub fn date(&mut self, item: &str, date: NaiveDate) {
        self.dates
            .entry(item.into())
            .and_modify(|old| *old = (*old).min(date))
            .or_insert(date);
    }
    pub fn fx_date(&mut self, item: &str, date: NaiveDate) {
        self.fx_dates
            .entry(item.into())
            .and_modify(|old| *old = (*old).min(date))
            .or_insert(date);
    }
    pub fn finish(
        self,
        legacy: NetWorthResponse,
        valuations: &[ValuationInfo],
    ) -> DetailedNetWorthResponse {
        let mut items: BTreeMap<String, DetailedNetWorthItem> = BTreeMap::new();
        for val in valuations {
            let issues: Vec<_> = self
                .issues
                .iter()
                .filter(|i| i.item_id == val.asset_id)
                .cloned()
                .collect();
            let basis = self
                .basis
                .get(&val.asset_id)
                .copied()
                .unwrap_or(if val.is_cash_like {
                    DetailedValuationBasis::Balance
                } else {
                    DetailedValuationBasis::MarketQuote
                });
            let date = self
                .dates
                .get(&val.asset_id)
                .copied()
                .unwrap_or(val.valuation_date);
            let invalid_source = issues.iter().any(|issue| {
                matches!(
                    issue.code,
                    NetWorthIssueCode::InvalidQuoteSource
                        | NetWorthIssueCode::StoredQuoteUnverified
                )
            });
            let mut item = DetailedNetWorthItem {
                id: val.asset_id.clone(),
                name: val.name.clone().unwrap_or_else(|| val.asset_id.clone()),
                category: super::net_worth_service::NetWorthService::category_key(val.category)
                    .into(),
                currency: legacy.currency.clone(),
                value: issues.is_empty().then_some(val.market_value_base),
                substitute_value: (!issues.is_empty()
                    && !issues.iter().any(|i| {
                        matches!(
                            i.code,
                            NetWorthIssueCode::MissingFx
                                | NetWorthIssueCode::ProjectionNotCurrent
                                | NetWorthIssueCode::MissingFinalCash
                                | NetWorthIssueCode::StoredCurrencyMismatch
                                | NetWorthIssueCode::InvalidQuoteSource
                                | NetWorthIssueCode::StoredQuoteUnverified
                                | NetWorthIssueCode::ArithmeticOverflow
                        )
                    }))
                .then_some(val.market_value_base),
                valuation_date: (!invalid_source).then_some(date),
                fx_source_date: self.fx_dates.get(&val.asset_id).copied(),
                stale: !invalid_source
                    && !val.is_cash_like
                    && !(val.category == AssetCategory::Liability
                        && val.market_value_base.is_zero())
                    && (legacy.date - date).num_days() > 90
                    || self
                        .fx_dates
                        .get(&val.asset_id)
                        .is_some_and(|fx_date| (legacy.date - *fx_date).num_days() > 90),
                basis,
                issues,
            };
            if let Some(previous) = items.remove(&val.asset_id) {
                item.value = previous
                    .value
                    .zip(item.value)
                    .and_then(|(a, b)| a.checked_add(b));
                item.substitute_value = previous
                    .substitute_value
                    .zip(item.substitute_value)
                    .and_then(|(a, b)| a.checked_add(b));
                item.valuation_date = previous
                    .valuation_date
                    .into_iter()
                    .chain(item.valuation_date)
                    .min();
                item.stale |= previous.stale;
                item.fx_source_date = item
                    .fx_source_date
                    .into_iter()
                    .chain(previous.fx_source_date)
                    .min();
            }
            items.insert(val.asset_id.clone(), item);
        }
        for issue in &self.issues {
            if !items.contains_key(&issue.item_id) {
                let related = self
                    .issues
                    .iter()
                    .filter(|i| i.item_id == issue.item_id)
                    .cloned()
                    .collect();
                let date = self.dates.get(&issue.item_id).copied();
                items.insert(
                    issue.item_id.clone(),
                    DetailedNetWorthItem {
                        id: issue.item_id.clone(),
                        name: issue.name.clone().unwrap_or_else(|| {
                            issue
                                .asset_id
                                .clone()
                                .unwrap_or_else(|| issue.item_id.clone())
                        }),
                        category: issue.category.clone(),
                        currency: legacy.currency.clone(),
                        value: None,
                        substitute_value: None,
                        valuation_date: date,
                        fx_source_date: self.fx_dates.get(&issue.item_id).copied(),
                        stale: date.is_some_and(|d| (legacy.date - d).num_days() > 90),
                        basis: DetailedValuationBasis::Unknown,
                        issues: related,
                    },
                );
            }
        }
        let assets_complete = !self.issues.iter().any(|i| i.category != "liabilities");
        let liabilities_complete = !self.issues.iter().any(|i| i.category == "liabilities");
        // Missing liability-account currency conversion can affect either side
        // of its signed net balance, so both totals must remain unknown.
        let unknown_liability_sign = self.issues.iter().any(|i| {
            matches!(
                i.code,
                NetWorthIssueCode::MissingFx | NetWorthIssueCode::MissingSnapshot
            ) && i.category == "liabilities"
        });
        let group_total = |financial: bool| {
            if unknown_liability_sign {
                return None;
            }
            items
                .values()
                .filter(|item| {
                    item.category != "liabilities"
                        && matches!(item.category.as_str(), "cash" | "investments") == financial
                })
                .try_fold(Decimal::ZERO, |total, item| total.checked_add(item.value?))
        };
        let financial_assets = group_total(true);
        let other_assets = group_total(false);
        DetailedNetWorthResponse {
            date: legacy.date,
            currency: legacy.currency.clone(),
            total_assets: (assets_complete && !unknown_liability_sign)
                .then_some(legacy.assets.total),
            financial_assets,
            other_assets,
            total_liabilities: liabilities_complete.then_some(legacy.liabilities.total),
            net_worth: (assets_complete && liabilities_complete).then_some(legacy.net_worth),
            items: items.into_values().collect(),
            issues: self.issues,
            legacy,
        }
    }
}
