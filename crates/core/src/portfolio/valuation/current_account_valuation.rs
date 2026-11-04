use super::{
    CurrentResourceValuation, CurrentValuationIssue, CurrentValuationIssueCode,
    DetailedCurrentAccountValuation, DetailedCurrentValuationResponse, ValuationComponent,
};
use chrono::{DateTime, NaiveDate, Utc};
use log::warn;
use rust_decimal::Decimal;
use std::collections::{HashMap, HashSet};

use crate::{
    accounts::{account_supports_portfolio_scope, Account, AccountPurpose, AccountServiceTrait},
    assets::{Asset, AssetServiceTrait, InstrumentType},
    errors::Result,
    fx::{currency::normalize_amount, FxServiceTrait},
    portfolio::{
        snapshot::{AccountStateSnapshot, SnapshotServiceTrait},
        valuation::{
            CurrentAccountValuation, CurrentValuationResponse, CurrentValuationSplit,
            CurrentValuationSummary,
        },
    },
    quotes::{LatestQuotePair, QuoteServiceTrait},
    utils::occ_symbol::parse_occ_symbol,
};

const MISSING_ASSET_WARNING: &str =
    "Some asset details are missing, so this value may be incomplete.";
const MISSING_QUOTE_WARNING: &str =
    "Some market prices are missing, so this value may be incomplete.";
const MISSING_FX_WARNING: &str =
    "Some exchange rates are missing, so this value may be approximate.";

pub struct CurrentAccountValuationService<'a> {
    account_service: &'a dyn AccountServiceTrait,
    snapshot_service: &'a dyn SnapshotServiceTrait,
    asset_service: &'a dyn AssetServiceTrait,
    quote_service: &'a dyn QuoteServiceTrait,
    fx_service: &'a dyn FxServiceTrait,
}

impl<'a> CurrentAccountValuationService<'a> {
    pub fn new(
        account_service: &'a dyn AccountServiceTrait,
        snapshot_service: &'a dyn SnapshotServiceTrait,
        asset_service: &'a dyn AssetServiceTrait,
        quote_service: &'a dyn QuoteServiceTrait,
        fx_service: &'a dyn FxServiceTrait,
    ) -> Self {
        Self {
            account_service,
            snapshot_service,
            asset_service,
            quote_service,
            fx_service,
        }
    }

    pub async fn get_current_valuation_for_scope(
        &self,
        scope_id: &str,
        account_ids: &[String],
        base_currency: &str,
        latest_snapshot_cutoff: NaiveDate,
        include_accounts: bool,
    ) -> Result<CurrentValuationResponse> {
        self.get_current_valuation_for_scope_at(
            scope_id,
            account_ids,
            base_currency,
            latest_snapshot_cutoff,
            include_accounts,
            crate::utils::clock::now(),
        )
        .await
    }

    pub async fn get_current_valuation_for_scope_at(
        &self,
        scope_id: &str,
        account_ids: &[String],
        base_currency: &str,
        latest_snapshot_cutoff: NaiveDate,
        include_accounts: bool,
        calculated_at: DateTime<Utc>,
    ) -> Result<CurrentValuationResponse> {
        Ok(self
            .get_current_valuation_detailed_for_scope_at(
                scope_id,
                account_ids,
                base_currency,
                latest_snapshot_cutoff,
                include_accounts,
                calculated_at,
            )
            .await?
            .legacy)
    }

    pub async fn get_current_valuation_detailed_for_scope(
        &self,
        scope_id: &str,
        account_ids: &[String],
        base_currency: &str,
        day: NaiveDate,
    ) -> Result<DetailedCurrentValuationResponse> {
        self.get_current_valuation_detailed_for_scope_at(
            scope_id,
            account_ids,
            base_currency,
            day,
            true,
            crate::utils::clock::now(),
        )
        .await
    }

    pub async fn get_current_valuation_detailed_for_scope_at(
        &self,
        scope_id: &str,
        account_ids: &[String],
        base_currency: &str,
        latest_snapshot_cutoff: NaiveDate,
        include_accounts: bool,
        calculated_at: DateTime<Utc>,
    ) -> Result<DetailedCurrentValuationResponse> {
        let accounts = self.resolve_requested_accounts(account_ids)?;
        let selected: Vec<_> = accounts.iter().map(|a| a.id.clone()).collect();
        let snapshots = self.latest_snapshots_with_positions(&selected, latest_snapshot_cutoff)?;
        let assets_by_id = self.load_assets_by_id(&snapshots).await?;
        let latest_quote_pairs =
            self.load_latest_quotes(&snapshots, &assets_by_id, latest_snapshot_cutoff)?;
        let mut fx_cache = FxRateCache::new(self.fx_service);
        fx_cache.verify_sources = true;
        let mut response = calculate_current_valuation_detailed_from_snapshots(
            scope_id,
            &accounts,
            &snapshots,
            &assets_by_id,
            &latest_quote_pairs,
            base_currency,
            calculated_at,
            include_accounts,
            |from, to| fx_cache.get(from, to),
        );
        let quotes: Vec<_> = latest_quote_pairs
            .values()
            .map(|pair| pair.latest.clone())
            .collect();
        let checks = self
            .quote_service
            .verify_quote_sources(&quotes)
            .unwrap_or_default();
        let pending = self
            .snapshot_service
            .pending_projection_accounts(
                &snapshots,
                latest_snapshot_cutoff,
                crate::portfolio::projection::ProjectionReadScope::Holdings,
            )
            .ok()
            .flatten();
        let missing_final_cash = self
            .snapshot_service
            .snapshots_missing_final_cash(&snapshots)?;
        for account in &mut response.accounts {
            for resource in &mut account.resources {
                resource.fx_source_date = resource.valuation_currency.as_ref().and_then(|from| {
                    fx_cache
                        .source_dates
                        .get(&(from.clone(), base_currency.to_string()))
                        .copied()
                        .flatten()
                });
            }
            if snapshots.get(&account.account_id).is_some_and(|snapshot| {
                snapshot.source == super::super::snapshot::SnapshotSource::Calculated
            }) && pending
                .as_ref()
                .is_none_or(|pending| pending.contains(&account.account_id))
            {
                let issue = CurrentValuationIssue {
                    code: CurrentValuationIssueCode::ProjectionNotCurrent,
                    component: ValuationComponent::All,
                    account_id: account.account_id.clone(),
                    asset_id: None,
                    currency: None,
                };
                account.issues.push(issue.clone());
                response.issues.push(issue.clone());
                account.cash_balance_base = None;
                account.investment_market_value_base = None;
                account.total_value_base = None;
                response.cash_balance_base = None;
                response.investment_market_value_base = None;
                response.total_value_base = None;
                for resource in &mut account.resources {
                    resource.native_amount = None;
                    resource.local_value = None;
                    resource.value_base = None;
                    resource.issues.push(issue.clone());
                }
            }
            for resource in &mut account.resources {
                if resource.kind == "CASH"
                    && resource.currency.as_deref().is_some_and(|currency| {
                        resource.native_amount.is_some_and(|amount| {
                            crate::fx::currency::normalize_amount_exact(amount, currency).is_none()
                        })
                    })
                {
                    let issue = CurrentValuationIssue {
                        code: CurrentValuationIssueCode::ArithmeticOverflow,
                        component: ValuationComponent::Cash,
                        account_id: account.account_id.clone(),
                        asset_id: None,
                        currency: resource.currency.clone(),
                    };
                    resource.local_value = None;
                    resource.value_base = None;
                    resource.issues.push(issue.clone());
                    account.issues.push(issue.clone());
                    response.issues.push(issue);
                    account.cash_balance_base = None;
                    account.total_value_base = None;
                    response.cash_balance_base = None;
                    response.total_value_base = None;
                    continue;
                }
                let Some(asset) = &resource.asset_id else {
                    continue;
                };
                let Some(pair) = latest_quote_pairs.get(asset) else {
                    continue;
                };
                let origin_known = checks.get(&pair.latest.id)
                    == Some(&crate::quotes::QuoteSourceQuality::Verified);
                let numeric_known = crate::fx::currency::normalize_amount_exact(
                    pair.latest.close,
                    &pair.latest.currency,
                )
                .is_some();
                if origin_known && numeric_known {
                    continue;
                }
                let issue = CurrentValuationIssue {
                    code: if origin_known {
                        CurrentValuationIssueCode::ArithmeticOverflow
                    } else {
                        CurrentValuationIssueCode::InvalidQuoteSource
                    },
                    component: ValuationComponent::Investments,
                    account_id: account.account_id.clone(),
                    asset_id: Some(asset.clone()),
                    currency: resource.valuation_currency.clone(),
                };
                resource.unit_value = None;
                resource.local_value = None;
                resource.value_base = None;
                resource.valuation_date = None;
                resource.stale = false;
                resource.issues.push(issue.clone());
                account.issues.push(issue.clone());
                response.issues.push(issue);
                account.investment_market_value_base = None;
                account.total_value_base = None;
                response.investment_market_value_base = None;
                response.total_value_base = None;
            }
            if missing_final_cash.contains(&account.account_id) {
                let issue = CurrentValuationIssue {
                    code: CurrentValuationIssueCode::MissingFinalCash,
                    component: ValuationComponent::Cash,
                    account_id: account.account_id.clone(),
                    asset_id: None,
                    currency: None,
                };
                for cash in account
                    .resources
                    .iter_mut()
                    .filter(|row| row.kind == "CASH")
                {
                    cash.native_amount = None;
                    cash.local_value = None;
                    cash.value_base = None;
                    cash.issues.push(issue.clone());
                }
                account.issues.push(issue.clone());
                response.issues.push(issue);
                account.cash_balance_base = None;
                account.total_value_base = None;
                response.cash_balance_base = None;
                response.total_value_base = None;
            }
            account.source_data_as_of = account
                .resources
                .iter()
                .flat_map(|r| r.valuation_date.into_iter().chain(r.fx_source_date))
                .min()
                .or_else(|| {
                    snapshots
                        .get(&account.account_id)
                        .map(|snapshot| snapshot.snapshot_date)
                });
        }
        response.source_data_as_of = response
            .accounts
            .iter()
            .filter_map(|account| account.source_data_as_of)
            .min();
        for account in unique_account_ids(account_ids.iter().cloned())
            .into_iter()
            .filter(|id| !selected.contains(id))
        {
            let issue = CurrentValuationIssue {
                code: CurrentValuationIssueCode::AccountNotEligible,
                component: ValuationComponent::All,
                account_id: account.clone(),
                asset_id: None,
                currency: None,
            };
            response.issues.push(issue.clone());
            response.accounts.push(DetailedCurrentAccountValuation {
                account_id: account,
                account_currency: String::new(),
                base_currency: base_currency.into(),
                cash_balance_base: None,
                investment_market_value_base: None,
                total_value_base: None,
                source_data_as_of: None,
                calculated_at,
                warnings: Vec::new(),
                issues: vec![issue],
                resources: Vec::new(),
                stale: false,
            });
            response.cash_balance_base = None;
            response.investment_market_value_base = None;
            response.total_value_base = None;
        }
        // Actual data dates describe the selected business day, not when this query ran.
        for account in &mut response.accounts {
            for resource in &mut account.resources {
                resource.stale = (resource.kind == "SECURITY"
                    && resource
                        .valuation_date
                        .is_some_and(|date| date < latest_snapshot_cutoff))
                    || resource
                        .fx_source_date
                        .is_some_and(|date| date < latest_snapshot_cutoff);
            }
            account.stale = account.resources.iter().any(|r| r.stale);
        }
        response.stale = response.accounts.iter().any(|a| a.stale);
        response
            .accounts
            .sort_by(|a, b| a.account_id.cmp(&b.account_id));
        response.issues.sort();
        response.issues.dedup();
        Ok(response)
    }

    fn resolve_requested_accounts(&self, account_ids: &[String]) -> Result<Vec<Account>> {
        if account_ids.is_empty() {
            return Ok(Vec::new());
        }

        let requested_ids = unique_account_ids(account_ids.iter().cloned());
        let accounts = self.account_service.get_accounts_by_ids(&requested_ids)?;
        Ok(filter_current_valuation_accounts(
            Some(&requested_ids),
            accounts,
        ))
    }

    fn latest_snapshots_with_positions(
        &self,
        account_ids: &[String],
        latest_snapshot_cutoff: NaiveDate,
    ) -> Result<HashMap<String, AccountStateSnapshot>> {
        self.snapshot_service
            .get_latest_snapshots_as_of(account_ids, latest_snapshot_cutoff)
    }

    async fn load_assets_by_id(
        &self,
        snapshots: &HashMap<String, AccountStateSnapshot>,
    ) -> Result<HashMap<String, Asset>> {
        let asset_ids = snapshot_asset_ids(snapshots);
        if asset_ids.is_empty() {
            return Ok(HashMap::new());
        }

        Ok(self
            .asset_service
            .get_assets_by_asset_ids(&asset_ids)
            .await?
            .into_iter()
            .map(|asset| (asset.id.clone(), asset))
            .collect())
    }

    fn load_latest_quotes(
        &self,
        snapshots: &HashMap<String, AccountStateSnapshot>,
        assets_by_id: &HashMap<String, Asset>,
        latest_snapshot_cutoff: NaiveDate,
    ) -> Result<HashMap<String, LatestQuotePair>> {
        let asset_ids = quoted_asset_ids(snapshots, assets_by_id);
        if asset_ids.is_empty() {
            return Ok(HashMap::new());
        }

        Ok(self
            .quote_service
            .get_latest_quotes_as_of(&asset_ids, latest_snapshot_cutoff)?
            .into_iter()
            .map(|(asset_id, latest)| {
                (
                    asset_id,
                    LatestQuotePair {
                        latest,
                        previous: None,
                    },
                )
            })
            .collect())
    }
}

#[derive(Debug, Clone)]
pub struct CurrentValuationRate {
    pub rate: Decimal,
    pub warning: Option<String>,
}

impl From<Decimal> for CurrentValuationRate {
    fn from(rate: Decimal) -> Self {
        Self {
            rate,
            warning: None,
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn calculate_current_valuation_response_from_snapshots<F, R>(
    scope_id: &str,
    accounts: &[Account],
    snapshots: &HashMap<String, AccountStateSnapshot>,
    assets_by_id: &HashMap<String, Asset>,
    latest_quote_pairs: &HashMap<String, LatestQuotePair>,
    base_currency: &str,
    calculated_at: DateTime<Utc>,
    include_accounts: bool,
    latest_rate: F,
) -> CurrentValuationResponse
where
    F: FnMut(&str, &str) -> R,
    R: Into<CurrentValuationRate>,
{
    calculate_current_valuation_detailed_from_snapshots(
        scope_id,
        accounts,
        snapshots,
        assets_by_id,
        latest_quote_pairs,
        base_currency,
        calculated_at,
        include_accounts,
        latest_rate,
    )
    .legacy
}

#[allow(clippy::too_many_arguments)]
pub fn calculate_current_valuation_detailed_from_snapshots<F, R>(
    scope_id: &str,
    accounts: &[Account],
    snapshots: &HashMap<String, AccountStateSnapshot>,
    assets_by_id: &HashMap<String, Asset>,
    latest_quote_pairs: &HashMap<String, LatestQuotePair>,
    base_currency: &str,
    calculated_at: DateTime<Utc>,
    include_accounts: bool,
    mut latest_rate: F,
) -> DetailedCurrentValuationResponse
where
    F: FnMut(&str, &str) -> R,
    R: Into<CurrentValuationRate>,
{
    let mut summary = CurrentValuationSummary {
        scope_id: scope_id.to_string(),
        base_currency: base_currency.to_string(),
        cash_balance_base: Decimal::ZERO,
        investment_market_value_base: Decimal::ZERO,
        total_value_base: Decimal::ZERO,
        holdings_count: 0,
        account_count: accounts.len(),
        currency_split: Vec::new(),
        cash_currency_split: Vec::new(),
        source_data_as_of: None,
        calculated_at,
        warnings: Vec::new(),
    };
    let mut currency_base_totals: HashMap<String, Decimal> = HashMap::new();
    let mut cash_base_totals: HashMap<String, Decimal> = HashMap::new();
    let mut cash_local_totals: HashMap<String, Decimal> = HashMap::new();
    let mut account_valuations = Vec::new();
    let mut detailed_accounts = Vec::new();

    for account in accounts {
        let computation = match snapshots.get(&account.id) {
            Some(snapshot) => calculate_account_snapshot_valuation_with_contributions(
                account,
                snapshot,
                assets_by_id,
                latest_quote_pairs,
                base_currency,
                calculated_at,
                &mut latest_rate,
            ),
            None => AccountValuationComputation {
                valuation: zero_current_account_valuation(account, base_currency, calculated_at),
                holdings_count: 0,
                currency_base_totals: HashMap::new(),
                cash_base_totals: HashMap::new(),
                cash_local_totals: HashMap::new(),
                summary_warnings: Vec::new(),
                resources: Vec::new(),
                issues: vec![CurrentValuationIssue {
                    code: CurrentValuationIssueCode::MissingSnapshot,
                    component: ValuationComponent::All,
                    account_id: account.id.clone(),
                    asset_id: None,
                    currency: None,
                }],
                source_date: None,
            },
        };

        let mut computation = computation;
        computation.resources.sort_by(|a, b| {
            (&a.kind, &a.asset_id, &a.currency).cmp(&(&b.kind, &b.asset_id, &b.currency))
        });
        computation.issues.sort();
        computation.issues.dedup();
        let cash_ok = !computation.issues.iter().any(|i| {
            matches!(
                i.component,
                ValuationComponent::Cash | ValuationComponent::All
            )
        });
        let investments_ok = !computation.issues.iter().any(|i| {
            matches!(
                i.component,
                ValuationComponent::Investments | ValuationComponent::All
            )
        });
        let stale = computation.resources.iter().any(|r| r.stale);
        detailed_accounts.push(DetailedCurrentAccountValuation {
            account_id: account.id.clone(),
            account_currency: account.currency.clone(),
            base_currency: base_currency.into(),
            cash_balance_base: cash_ok.then_some(computation.valuation.cash_balance_base),
            investment_market_value_base: investments_ok
                .then_some(computation.valuation.investment_market_value_base),
            total_value_base: (cash_ok && investments_ok)
                .then_some(computation.valuation.total_value_base),
            source_data_as_of: computation.source_date,
            calculated_at,
            warnings: computation.valuation.warnings.clone(),
            issues: computation.issues.clone(),
            resources: computation.resources.clone(),
            stale,
        });
        summary.cash_balance_base += computation.valuation.cash_balance_base;
        summary.investment_market_value_base += computation.valuation.investment_market_value_base;
        summary.total_value_base += computation.valuation.total_value_base;
        summary.holdings_count += computation.holdings_count;
        summary.source_data_as_of = latest_datetime(
            summary.source_data_as_of,
            computation.valuation.source_data_as_of,
        );
        merge_totals(&mut currency_base_totals, computation.currency_base_totals);
        merge_totals(&mut cash_base_totals, computation.cash_base_totals);
        merge_totals(&mut cash_local_totals, computation.cash_local_totals);
        summary.warnings.extend(computation.summary_warnings);

        if include_accounts {
            account_valuations.push(computation.valuation);
        }
    }

    summary.currency_split = build_currency_split(currency_base_totals, summary.total_value_base);
    summary.cash_currency_split = build_cash_currency_split(
        cash_local_totals,
        cash_base_totals,
        summary.cash_balance_base,
    );
    summary.warnings.sort();
    summary.warnings.dedup();

    detailed_accounts.sort_by(|a, b| a.account_id.cmp(&b.account_id));
    let mut issues: Vec<_> = detailed_accounts
        .iter()
        .flat_map(|a| a.issues.clone())
        .collect();
    issues.sort_by(|a, b| {
        (&a.account_id, &a.asset_id, &a.currency).cmp(&(&b.account_id, &b.asset_id, &b.currency))
    });
    let cash_ok = detailed_accounts
        .iter()
        .all(|a| a.cash_balance_base.is_some());
    let investments_ok = detailed_accounts
        .iter()
        .all(|a| a.investment_market_value_base.is_some());
    let source_data_as_of = detailed_accounts
        .iter()
        .filter_map(|a| a.source_data_as_of)
        .min();
    let stale = detailed_accounts.iter().any(|a| a.stale);
    DetailedCurrentValuationResponse {
        scope_id: scope_id.into(),
        base_currency: base_currency.into(),
        cash_balance_base: cash_ok.then_some(summary.cash_balance_base),
        investment_market_value_base: investments_ok
            .then_some(summary.investment_market_value_base),
        total_value_base: (cash_ok && investments_ok).then_some(summary.total_value_base),
        source_data_as_of,
        calculated_at,
        accounts: detailed_accounts,
        issues,
        stale,
        legacy: CurrentValuationResponse {
            summary,
            accounts: account_valuations,
        },
    }
}

pub fn filter_current_valuation_accounts(
    requested_ids: Option<&[String]>,
    accounts: Vec<Account>,
) -> Vec<Account> {
    match requested_ids {
        None => accounts
            .into_iter()
            .filter(|account| {
                account_supports_portfolio_scope(account, AccountPurpose::Performance)
                    && account_supports_portfolio_scope(account, AccountPurpose::Holdings)
            })
            .collect(),
        Some(ids) => {
            let accounts_by_id: HashMap<String, Account> = accounts
                .into_iter()
                .map(|account| (account.id.clone(), account))
                .collect();

            ids.iter()
                .filter_map(|account_id| accounts_by_id.get(account_id).cloned())
                .filter(|account| {
                    account_supports_portfolio_scope(account, AccountPurpose::Performance)
                        && account_supports_portfolio_scope(account, AccountPurpose::Holdings)
                })
                .collect()
        }
    }
}

pub fn unique_account_ids(account_ids: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    account_ids
        .into_iter()
        .filter(|account_id| seen.insert(account_id.clone()))
        .collect()
}

struct AccountValuationComputation {
    valuation: CurrentAccountValuation,
    holdings_count: usize,
    currency_base_totals: HashMap<String, Decimal>,
    cash_base_totals: HashMap<String, Decimal>,
    cash_local_totals: HashMap<String, Decimal>,
    summary_warnings: Vec<String>,
    resources: Vec<CurrentResourceValuation>,
    issues: Vec<CurrentValuationIssue>,
    source_date: Option<NaiveDate>,
}

fn calculate_account_snapshot_valuation_with_contributions<F, R>(
    account: &Account,
    snapshot: &AccountStateSnapshot,
    assets_by_id: &HashMap<String, Asset>,
    latest_quote_pairs: &HashMap<String, LatestQuotePair>,
    base_currency: &str,
    calculated_at: DateTime<Utc>,
    latest_rate: &mut F,
) -> AccountValuationComputation
where
    F: FnMut(&str, &str) -> R,
    R: Into<CurrentValuationRate>,
{
    let mut valuation = zero_current_account_valuation(account, base_currency, calculated_at);
    valuation.source_data_as_of = Some(snapshot_source_data_as_of(snapshot));
    let mut holdings_count = 0;
    let mut currency_base_totals: HashMap<String, Decimal> = HashMap::new();
    let mut cash_base_totals: HashMap<String, Decimal> = HashMap::new();
    let mut cash_local_totals: HashMap<String, Decimal> = HashMap::new();
    let mut summary_warnings = Vec::new();
    let mut account_warnings = Vec::new();
    let mut resources = Vec::new();
    let mut issues = Vec::new();
    let mut source_date = Some(snapshot.snapshot_date);

    let account_fx_rate = latest_rate(&account.currency, base_currency).into();
    valuation.fx_rate_to_base = account_fx_rate
        .warning
        .is_none()
        .then_some(account_fx_rate.rate)
        .filter(|rate| *rate > Decimal::ZERO);
    if let Some(warning) = account_fx_rate.warning {
        account_warnings.push(warning);
    }

    for (cash_currency, amount) in &snapshot.cash_balances {
        let (normalized_amount, normalized_cash_currency) =
            normalize_amount(*amount, cash_currency);
        let account_value = normalized_amount
            * rate_for(
                &mut account_warnings,
                latest_rate,
                normalized_cash_currency,
                &account.currency,
            );
        let (base_rate, base_reliable) = rate_for_checked(
            &mut summary_warnings,
            latest_rate,
            normalized_cash_currency,
            base_currency,
        );
        let base_reliable = base_reliable || normalized_amount.is_zero();
        let base_value = normalized_amount * base_rate;
        let resource_issues = if base_reliable {
            Vec::new()
        } else {
            vec![CurrentValuationIssue {
                code: CurrentValuationIssueCode::MissingFx,
                component: ValuationComponent::Cash,
                account_id: account.id.clone(),
                asset_id: None,
                currency: Some(normalized_cash_currency.into()),
            }]
        };
        issues.extend(resource_issues.clone());
        let (unit_value, _) = normalize_amount(Decimal::ONE, cash_currency);
        resources.push(CurrentResourceValuation {
            kind: "CASH".into(),
            account_id: account.id.clone(),
            asset_id: None,
            currency: Some(cash_currency.clone()),
            native_amount: Some(*amount),
            valuation_currency: Some(normalized_cash_currency.into()),
            unit_value: Some(unit_value),
            local_value: Some(normalized_amount),
            value_base: base_reliable.then_some(base_value),
            valuation_date: Some(snapshot.snapshot_date),
            fx_source_date: None,
            stale: false,
            issues: resource_issues,
        });
        valuation.cash_balance += account_value;
        valuation.cash_balance_base += base_value;

        if !normalized_amount.is_zero() {
            holdings_count += 1;
            add_total(
                &mut currency_base_totals,
                normalized_cash_currency,
                base_value,
            );
            add_total(&mut cash_base_totals, normalized_cash_currency, base_value);
            add_total(
                &mut cash_local_totals,
                normalized_cash_currency,
                normalized_amount,
            );
        }
    }

    for (asset_id, position) in &snapshot.positions {
        if position.quantity == Decimal::ZERO || position.is_alternative {
            continue;
        }

        let Some(asset) = assets_by_id.get(asset_id) else {
            warn!(
                "Skipping current valuation for position {} in account {} because asset metadata is missing",
                asset_id, account.id
            );
            summary_warnings.push(MISSING_ASSET_WARNING.to_string());
            account_warnings.push(MISSING_ASSET_WARNING.to_string());
            let issue = CurrentValuationIssue {
                code: CurrentValuationIssueCode::MissingAsset,
                component: ValuationComponent::Investments,
                account_id: account.id.clone(),
                asset_id: Some(asset_id.clone()),
                currency: None,
            };
            issues.push(issue.clone());
            resources.push(unpriced_resource(
                account,
                asset_id,
                position.quantity,
                issue,
            ));
            continue;
        };
        if asset.is_alternative() {
            continue;
        }
        if is_expired_option_asset(asset, calculated_at.date_naive()) {
            continue;
        }

        let Some(quote_pair) = latest_quote_pairs.get(asset_id) else {
            warn!(
                "Missing latest quote for asset {} in account {}. Current market value treated as zero.",
                asset_id, account.id
            );
            summary_warnings.push(MISSING_QUOTE_WARNING.to_string());
            account_warnings.push(MISSING_QUOTE_WARNING.to_string());
            let issue = CurrentValuationIssue {
                code: CurrentValuationIssueCode::MissingQuote,
                component: ValuationComponent::Investments,
                account_id: account.id.clone(),
                asset_id: Some(asset_id.clone()),
                currency: Some(asset.quote_ccy.clone()),
            };
            issues.push(issue.clone());
            resources.push(unpriced_resource(
                account,
                asset_id,
                position.quantity,
                issue,
            ));
            continue;
        };
        holdings_count += 1;
        valuation.source_data_as_of = latest_datetime(
            valuation.source_data_as_of,
            Some(quote_pair.latest.timestamp),
        );

        let (normalized_price, normalized_quote_currency) =
            normalize_amount(quote_pair.latest.close, &quote_pair.latest.currency);
        let market_value_quote =
            normalized_price * position.quantity * position.contract_multiplier;
        let (base_rate, base_reliable) = rate_for_checked(
            &mut summary_warnings,
            latest_rate,
            normalized_quote_currency,
            base_currency,
        );
        let market_value_base = market_value_quote * base_rate;
        let resource_issues = if base_reliable {
            Vec::new()
        } else {
            vec![CurrentValuationIssue {
                code: CurrentValuationIssueCode::MissingFx,
                component: ValuationComponent::Investments,
                account_id: account.id.clone(),
                asset_id: Some(asset_id.clone()),
                currency: Some(normalized_quote_currency.into()),
            }]
        };
        issues.extend(resource_issues.clone());
        let quote_date = quote_pair.latest.timestamp.date_naive();
        source_date = source_date.map(|d| d.min(quote_date));
        resources.push(CurrentResourceValuation {
            kind: "SECURITY".into(),
            account_id: account.id.clone(),
            asset_id: Some(asset_id.clone()),
            currency: None,
            native_amount: Some(position.quantity),
            valuation_currency: Some(normalized_quote_currency.into()),
            unit_value: Some(normalized_price * position.contract_multiplier),
            local_value: Some(market_value_quote),
            value_base: base_reliable.then_some(market_value_base),
            valuation_date: Some(quote_date),
            fx_source_date: None,
            stale: quote_date < calculated_at.date_naive(),
            issues: resource_issues,
        });

        valuation.investment_market_value += market_value_quote
            * rate_for(
                &mut account_warnings,
                latest_rate,
                normalized_quote_currency,
                &account.currency,
            );
        valuation.investment_market_value_base += market_value_base;
        add_total(
            &mut currency_base_totals,
            normalized_quote_currency,
            market_value_base,
        );
    }

    valuation.total_value = valuation.cash_balance + valuation.investment_market_value;
    valuation.total_value_base =
        valuation.cash_balance_base + valuation.investment_market_value_base;
    summary_warnings.sort();
    summary_warnings.dedup();
    account_warnings.sort();
    account_warnings.dedup();
    valuation.warnings = account_warnings;

    AccountValuationComputation {
        valuation,
        holdings_count,
        currency_base_totals,
        cash_base_totals,
        cash_local_totals,
        summary_warnings,
        resources,
        issues,
        source_date,
    }
}

fn unpriced_resource(
    account: &Account,
    asset: &str,
    quantity: Decimal,
    issue: CurrentValuationIssue,
) -> CurrentResourceValuation {
    CurrentResourceValuation {
        kind: "SECURITY".into(),
        account_id: account.id.clone(),
        asset_id: Some(asset.into()),
        currency: None,
        native_amount: Some(quantity),
        valuation_currency: None,
        unit_value: None,
        local_value: None,
        value_base: None,
        valuation_date: None,
        fx_source_date: None,
        stale: false,
        issues: vec![issue],
    }
}
fn rate_for_checked<F, R>(
    warnings: &mut Vec<String>,
    latest_rate: &mut F,
    from: &str,
    to: &str,
) -> (Decimal, bool)
where
    F: FnMut(&str, &str) -> R,
    R: Into<CurrentValuationRate>,
{
    let lookup = latest_rate(from, to).into();
    let reliable = lookup.warning.is_none() && lookup.rate > Decimal::ZERO;
    if let Some(warning) = lookup.warning {
        warnings.push(warning);
    }
    (lookup.rate, reliable)
}

fn rate_for<F, R>(warnings: &mut Vec<String>, latest_rate: &mut F, from: &str, to: &str) -> Decimal
where
    F: FnMut(&str, &str) -> R,
    R: Into<CurrentValuationRate>,
{
    let lookup = latest_rate(from, to).into();
    if let Some(warning) = lookup.warning {
        warnings.push(warning);
    }
    lookup.rate
}

fn zero_current_account_valuation(
    account: &Account,
    base_currency: &str,
    calculated_at: DateTime<Utc>,
) -> CurrentAccountValuation {
    CurrentAccountValuation {
        account_id: account.id.clone(),
        account_currency: account.currency.clone(),
        base_currency: base_currency.to_string(),
        fx_rate_to_base: (account.currency == base_currency).then_some(Decimal::ONE),
        cash_balance: Decimal::ZERO,
        investment_market_value: Decimal::ZERO,
        total_value: Decimal::ZERO,
        cash_balance_base: Decimal::ZERO,
        investment_market_value_base: Decimal::ZERO,
        total_value_base: Decimal::ZERO,
        source_data_as_of: None,
        calculated_at,
        warnings: Vec::new(),
    }
}

fn snapshot_asset_ids(snapshots: &HashMap<String, AccountStateSnapshot>) -> Vec<String> {
    unique_asset_ids(
        snapshots
            .values()
            .flat_map(|snapshot| snapshot.positions.keys().cloned()),
    )
}

fn quoted_asset_ids(
    snapshots: &HashMap<String, AccountStateSnapshot>,
    assets_by_id: &HashMap<String, Asset>,
) -> Vec<String> {
    unique_asset_ids(snapshots.values().flat_map(|snapshot| {
        snapshot
            .positions
            .iter()
            .filter_map(|(asset_id, position)| {
                if position.quantity == Decimal::ZERO || position.is_alternative {
                    return None;
                }
                let asset = assets_by_id.get(asset_id)?;
                (!asset.is_alternative()).then(|| asset_id.clone())
            })
    }))
}

fn unique_asset_ids(asset_ids: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    asset_ids
        .into_iter()
        .filter(|asset_id| seen.insert(asset_id.clone()))
        .collect()
}

fn add_total(totals: &mut HashMap<String, Decimal>, currency: &str, value: Decimal) {
    totals
        .entry(currency.to_string())
        .and_modify(|total| *total += value)
        .or_insert(value);
}

fn merge_totals(target: &mut HashMap<String, Decimal>, source: HashMap<String, Decimal>) {
    for (currency, value) in source {
        add_total(target, &currency, value);
    }
}

fn build_currency_split(
    totals: HashMap<String, Decimal>,
    total_value_base: Decimal,
) -> Vec<CurrentValuationSplit> {
    let mut rows: Vec<CurrentValuationSplit> = totals
        .into_iter()
        .filter(|(_, value)| !value.is_zero())
        .map(|(currency, value)| CurrentValuationSplit {
            currency,
            value_base: value,
            value_local: None,
            percentage: percentage(value, total_value_base),
        })
        .collect();
    rows.sort_by(|a, b| {
        b.value_base
            .cmp(&a.value_base)
            .then_with(|| a.currency.cmp(&b.currency))
    });
    rows
}

fn build_cash_currency_split(
    cash_local_totals: HashMap<String, Decimal>,
    cash_base_totals: HashMap<String, Decimal>,
    cash_balance_base: Decimal,
) -> Vec<CurrentValuationSplit> {
    let mut rows: Vec<CurrentValuationSplit> = cash_local_totals
        .into_iter()
        .filter(|(_, value)| !value.is_zero())
        .map(|(currency, value)| {
            let base_value = cash_base_totals
                .get(&currency)
                .copied()
                .unwrap_or(Decimal::ZERO);
            CurrentValuationSplit {
                currency,
                value_base: base_value,
                value_local: Some(value),
                percentage: percentage(base_value, cash_balance_base),
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        b.percentage
            .cmp(&a.percentage)
            .then_with(|| a.currency.cmp(&b.currency))
    });
    rows
}

fn percentage(value: Decimal, total: Decimal) -> Decimal {
    if total > Decimal::ZERO {
        ((value / total) * Decimal::from(100)).round_dp(4)
    } else {
        Decimal::ZERO
    }
}

fn latest_datetime(
    current: Option<DateTime<Utc>>,
    candidate: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    match (current, candidate) {
        (Some(current), Some(candidate)) => Some(current.max(candidate)),
        (Some(current), None) => Some(current),
        (None, Some(candidate)) => Some(candidate),
        (None, None) => None,
    }
}

fn snapshot_source_data_as_of(snapshot: &AccountStateSnapshot) -> DateTime<Utc> {
    DateTime::<Utc>::from_naive_utc_and_offset(snapshot.calculated_at, Utc)
}

fn is_expired_option_asset(asset: &Asset, today: NaiveDate) -> bool {
    if asset.instrument_type.as_ref() != Some(&InstrumentType::Option) {
        return false;
    }

    let expiration = asset
        .metadata
        .as_ref()
        .and_then(|m| m.get("option"))
        .and_then(|o| o.get("expiration"))
        .and_then(|v| v.as_str())
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        .or_else(|| {
            [
                asset.instrument_symbol.as_deref().unwrap_or_default(),
                asset.display_code.as_deref().unwrap_or_default(),
            ]
            .iter()
            .find_map(|symbol| {
                parse_occ_symbol(symbol)
                    .ok()
                    .map(|parsed| parsed.expiration)
            })
        });

    matches!(expiration, Some(expiration) if expiration < today)
}

struct FxRateCache<'a> {
    fx_service: &'a dyn FxServiceTrait,
    rates: HashMap<(String, String), CurrentValuationRate>,
    verify_sources: bool,
    source_dates: HashMap<(String, String), Option<NaiveDate>>,
}

impl<'a> FxRateCache<'a> {
    fn new(fx_service: &'a dyn FxServiceTrait) -> Self {
        Self {
            fx_service,
            rates: HashMap::new(),
            verify_sources: false,
            source_dates: HashMap::new(),
        }
    }

    fn get(&mut self, from: &str, to: &str) -> CurrentValuationRate {
        if from == to {
            return Decimal::ONE.into();
        }

        let key = (from.to_string(), to.to_string());
        if let Some(rate) = self.rates.get(&key) {
            return rate.clone();
        }

        let observed = if self.verify_sources {
            self.fx_service
                .get_exchange_rate_observation(from, to, None)
                .map(|read| {
                    (
                        read.rate,
                        (!read.verified).then(|| MISSING_FX_WARNING.to_string()),
                        read.verified.then_some(read.source_date).flatten(),
                    )
                })
        } else {
            self.fx_service
                .get_latest_exchange_rate(from, to)
                .map(|rate| (rate, None, None))
        };
        let (rate, warning, source_date) = match observed {
            Ok(read) => read,
            Err(error) => {
                warn!("Falling back to FX rate 1 while calculating current valuation from {} to {}: {}",from,to,error);
                (Decimal::ONE, Some(MISSING_FX_WARNING.to_string()), None)
            }
        };
        self.source_dates.insert(key.clone(), source_date);
        let latest_rate = CurrentValuationRate { rate, warning };
        self.rates.insert(key, latest_rate.clone());
        latest_rate
    }
}
