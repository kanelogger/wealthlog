//! Native context composition with a source guard spanning all three blocks.
use super::super::{model::invalid, service, FundResource, InvestmentResourceReader, JournalError};
use super::*;
use crate::BusinessStore;
use chrono::NaiveDate;
use diesel::prelude::*;
use futures::FutureExt;
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    sync::{Arc, RwLock},
};
use wealthfolio_core::{
    accounts::AccountServiceTrait,
    assets::AssetServiceTrait,
    fx::FxServiceTrait,
    portfolio::{
        net_worth::NetWorthServiceTrait, snapshot::SnapshotServiceTrait,
        valuation::CurrentAccountValuationService,
    },
    quotes::QuoteServiceTrait,
};
use wealthfolio_storage_sqlite::db::get_connection;

#[derive(Clone)]
pub struct AccountingContextReader {
    pub(super) store: BusinessStore,
    pub(super) resources: Arc<dyn InvestmentResourceReader>,
    pub(super) accounts: Arc<dyn AccountServiceTrait>,
    pub(super) snapshots: Arc<dyn SnapshotServiceTrait>,
    pub(super) assets: Arc<dyn AssetServiceTrait>,
    pub(super) quotes: Arc<dyn QuoteServiceTrait>,
    pub(super) fx: Arc<dyn FxServiceTrait>,
    pub(super) net_worth: Arc<dyn NetWorthServiceTrait>,
    pub(super) base_currency: Arc<RwLock<String>>,
    pub(super) timezone: Arc<RwLock<String>>,
}

async fn guarded_core<T>(
    operation: impl std::future::Future<Output = wealthfolio_core::Result<T>>,
) -> wealthfolio_core::Result<T> {
    std::panic::AssertUnwindSafe(operation)
        .catch_unwind()
        .await
        .unwrap_or_else(|_| {
            Err(wealthfolio_core::errors::Error::Unexpected(
                "Native valuation arithmetic unavailable".into(),
            ))
        })
}
impl AccountingContextReader {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        store: BusinessStore,
        resources: Arc<dyn InvestmentResourceReader>,
        accounts: Arc<dyn AccountServiceTrait>,
        snapshots: Arc<dyn SnapshotServiceTrait>,
        assets: Arc<dyn AssetServiceTrait>,
        quotes: Arc<dyn QuoteServiceTrait>,
        fx: Arc<dyn FxServiceTrait>,
        net_worth: Arc<dyn NetWorthServiceTrait>,
        base_currency: Arc<RwLock<String>>,
        timezone: Arc<RwLock<String>>,
    ) -> Self {
        Self {
            store,
            resources,
            accounts,
            snapshots,
            assets,
            quotes,
            fx,
            net_worth,
            base_currency,
            timezone,
        }
    }
    pub fn with_net_worth(mut self, service: Arc<dyn NetWorthServiceTrait>) -> Self {
        self.net_worth = service;
        self
    }

    fn settings(&self) -> (String, String) {
        (
            self.base_currency
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .clone(),
            self.timezone
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .clone(),
        )
    }
    pub(super) fn source_version(
        &self,
        conn: &mut SqliteConnection,
        query: &InvestmentContextQuery,
        day: NaiveDate,
    ) -> JournalResult<String> {
        let (currency, timezone) = self.settings();
        super::state::source_version(conn, query, day, &currency, &timezone)
    }
    async fn funding(
        &self,
        query: &InvestmentContextQuery,
        day: NaiveDate,
        currency: &str,
        fx_ready: bool,
    ) -> JournalResult<InvestmentContextBlock> {
        let (purposes, allocations) = {
            let mut conn = get_connection(self.store.pool())?;
            conn.transaction::<_, JournalError, _>(|conn| {
                Ok((
                    service::purpose_rows(conn)?,
                    service::allocation_rows(conn)?,
                ))
            })?
        };
        let purposes: Vec<_> = purposes
            .into_iter()
            .filter(|p| {
                p["id"]
                    .as_str()
                    .is_some_and(|id| query.purpose_ids.iter().any(|p| p == id))
            })
            .collect();
        let mut issues = Vec::new();
        for id in &query.purpose_ids {
            if !purposes.iter().any(|p| p["id"].as_str() == Some(id)) {
                issues.push(issue("PURPOSE_MISSING", Some(id.clone()), None));
            }
        }
        let mut identities: BTreeSet<_> = allocations
            .iter()
            .filter(|((purpose, _), amount)| {
                query.purpose_ids.contains(purpose) && **amount > super::super::ExactDecimal::ZERO
            })
            .map(|((_, resource), _)| resource.clone())
            .collect();
        let selected_accounts = self.accounts.get_accounts_by_ids(&query.account_ids)?;
        for account in &selected_accounts {
            identities.insert(FundResource::Cash {
                account_id: account.id.clone(),
                currency: account.currency.clone(),
            });
        }
        let current = if fx_ready {
            guarded_core(
                CurrentAccountValuationService::new(
                    self.accounts.as_ref(),
                    self.snapshots.as_ref(),
                    self.assets.as_ref(),
                    self.quotes.as_ref(),
                    self.fx.as_ref(),
                )
                .get_current_valuation_detailed_for_scope(
                    "INVESTMENT_DECISION_ACCOUNTS",
                    &query.account_ids,
                    currency,
                    day,
                ),
            )
            .await
        } else {
            Err(wealthfolio_core::errors::Error::Unexpected(
                "FX initialization unavailable".into(),
            ))
        };
        let account_valuations = match current {
            Ok(valuation) => {
                for account in &valuation.accounts {
                    for resource in &account.resources {
                        let identity = match resource.kind.as_str() {
                            "CASH" => resource.currency.as_ref().map(|c| FundResource::Cash {
                                account_id: account.account_id.clone(),
                                currency: c.clone(),
                            }),
                            "SECURITY" => {
                                resource.asset_id.as_ref().map(|id| FundResource::Security {
                                    account_id: account.account_id.clone(),
                                    asset_id: id.clone(),
                                })
                            }
                            _ => None,
                        };
                        if let Some(resource) = identity {
                            identities.insert(resource);
                        }
                    }
                }
                for problem in &valuation.issues {
                    issues.push(issue(
                        serde_json::to_value(problem.code)
                            .map_err(|e| invalid(e.to_string()))?
                            .as_str()
                            .unwrap_or("CURRENT_VALUATION_ISSUE"),
                        Some(
                            problem
                                .asset_id
                                .clone()
                                .unwrap_or_else(|| problem.account_id.clone()),
                        ),
                        problem.currency.clone(),
                    ));
                }
                Some(valuation)
            }
            Err(_) => {
                issues.push(issue("ACCOUNT_VALUATION_UNAVAILABLE", None, None));
                None
            }
        };
        let identities: Vec<_> = identities.into_iter().collect();
        let observations = if identities.is_empty() {
            Vec::new()
        } else {
            self.resources.observe(&identities, day).await?
        };
        let (allocation, reconciliation) = {
            let mut conn = get_connection(self.store.pool())?;
            conn.transaction::<_, JournalError, _>(|conn| {
                if !observations.is_empty() {
                    self.resources.validate(conn, &observations)?;
                }
                let allocation = service::build_preview(conn, observations, day)?;
                let tasks = service::fund_reconciliation_items(conn, &allocation)?;
                Ok((allocation, tasks))
            })?
        };
        for row in &allocation.resources {
            if row.balance.actual.is_none() {
                issues.push(issue(
                    "RESOURCE_UNAVAILABLE",
                    Some(row.observation.resource.account_id().into()),
                    None,
                ));
            }
        }
        let risks = self
            .purpose_risks(
                &purposes,
                &allocations,
                &allocation,
                &reconciliation,
                day,
                fx_ready,
            )
            .await?;
        let risk_incomplete = risks.iter().any(|r| r["quality"] != "COMPLETE");
        if risk_incomplete {
            issues.push(issue("PURPOSE_VALUATION_INCOMPLETE", None, None));
        }
        let source_date = account_valuations
            .as_ref()
            .and_then(|v| v.source_data_as_of)
            .map(|d| d.to_string())
            .into_iter()
            .chain(
                risks
                    .iter()
                    .filter_map(|v| v["sourceDataAsOf"].as_str().map(str::to_string)),
            )
            .min();
        let stale = account_valuations.as_ref().is_some_and(|v| v.stale)
            || risks.iter().any(|r| r["stale"] == true);
        let available = allocation
            .resources
            .iter()
            .any(|r| r.balance.actual.is_some())
            || purposes.iter().any(|p| !p.is_null())
            || query.account_ids.is_empty();
        let account_ids: Vec<_> = identities
            .iter()
            .map(|resource| resource.account_id().to_string())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let reference_accounts: Vec<_> = self
            .accounts
            .get_accounts_by_ids(&account_ids)?
            .into_iter()
            .map(|account| json!({"id":account.id,"name":account.name,"currency":account.currency}))
            .collect();
        let reference_assets: Vec<_> = identities.iter().filter_map(|resource| match resource {
            FundResource::Security { asset_id, .. } => Some(asset_id.as_str()),
            _ => None,
        }).collect::<BTreeSet<_>>().into_iter().filter_map(|id| self.assets.get_asset_by_id(id).ok())
            .map(|asset| json!({"id":asset.id,"name":asset.name,"displayCode":asset.display_code,"currency":asset.quote_ccy})).collect();
        let data = json!({"purposes":purposes,"allocation":allocation,"accountValuations":account_valuations,"risks":risks,"reconciliation":reconciliation,"resourceReferences":{"accounts":reference_accounts,"assets":reference_assets}});
        Ok(InvestmentContextBlock {
            scope: json!({"purposeIds":query.purpose_ids,"accountIds":query.account_ids,"riskScope":"ALL_RESOURCES_OWNED_BY_SELECTED_PURPOSES","nativeAmounts":"AUTHORITATIVE_LEDGER_RESOURCES"}),
            as_of_date: day,
            source_data_as_of: source_date,
            currency: currency.into(),
            quality: quality(available, &issues),
            stale,
            issues,
            data,
        })
    }
    async fn household(
        &self,
        day: NaiveDate,
        currency: &str,
    ) -> JournalResult<InvestmentContextBlock> {
        let detailed = guarded_core(self.net_worth.get_net_worth_detailed(day)).await?;
        if detailed.currency != currency || detailed.date != day {
            return Err(JournalError::StateConflict);
        }
        let issues: Vec<_> = detailed
            .issues
            .iter()
            .map(|i| {
                issue(
                    serde_json::to_value(i.code)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_string))
                        .as_deref()
                        .unwrap_or("NET_WORTH_ISSUE"),
                    Some(i.asset_id.clone().unwrap_or_else(|| i.item_id.clone())),
                    i.currency.clone(),
                )
            })
            .collect();
        let available =
            detailed.items.iter().any(|i| i.value.is_some()) || detailed.issues.is_empty();
        let source_date = detailed
            .items
            .iter()
            .flat_map(|i| i.valuation_date.into_iter().chain(i.fx_source_date))
            .min()
            .map(|d| d.to_string());
        let stale = detailed.items.iter().any(|i| i.stale);
        let data = serde_json::to_value(detailed).map_err(|e| invalid(e.to_string()))?;
        Ok(InvestmentContextBlock {
            scope: json!({"accounts":"ALL_UNARCHIVED","standaloneAlternativeAssets":true,"pageAccountFilterApplied":false,"portfolioAddedAgain":false}),
            as_of_date: day,
            source_data_as_of: source_date,
            currency: currency.into(),
            quality: quality(available, &issues),
            stale,
            issues,
            data,
        })
    }
}
#[async_trait]
impl InvestmentContextReader for AccountingContextReader {
    async fn capture(
        &self,
        query: &InvestmentContextQuery,
        day: NaiveDate,
    ) -> JournalResult<InvestmentContextCapture> {
        let _window = self.store.investment_read_window().read().await;
        let mut anchor = get_connection(self.store.pool())?;
        for _ in 0..2 {
            let epoch = super::state::read_epoch(&mut anchor)?;
            let (currency, timezone) = self.settings();
            super::super::model::validate_currency(&currency)?;
            let before = {
                let mut conn = get_connection(self.store.pool())?;
                conn.transaction::<_, JournalError, _>(|conn| {
                    self.source_version(conn, query, day)
                })?
            };
            let fx_ready = self.fx.initialize().is_ok();
            let funding = match self.funding(query, day, &currency, fx_ready).await {
                Ok(block) => block,
                Err(_) => unavailable(
                    day,
                    &currency,
                    json!({"purposeIds":query.purpose_ids,"accountIds":query.account_ids}),
                    "FUNDING_UNAVAILABLE",
                ),
            };
            let recent_cash_flow =
                match self.recent_cash_flow(query, day, &currency, &timezone, fx_ready) {
                    Ok(block) => block,
                    Err(_) => unavailable(
                        day,
                        &currency,
                        json!({"days":30,"timezone":timezone}),
                        "CASH_FLOW_UNAVAILABLE",
                    ),
                };
            let household = if fx_ready {
                match self.household(day, &currency).await {
                    Ok(block) => block,
                    Err(_) => unavailable(
                        day,
                        &currency,
                        json!({"accounts":"ALL_UNARCHIVED","standaloneAlternativeAssets":true}),
                        "HOUSEHOLD_UNAVAILABLE",
                    ),
                }
            } else {
                unavailable(
                    day,
                    &currency,
                    json!({"accounts":"ALL_UNARCHIVED","standaloneAlternativeAssets":true}),
                    "FX_UNAVAILABLE",
                )
            };
            let after = {
                let mut conn = get_connection(self.store.pool())?;
                conn.transaction::<_, JournalError, _>(|conn| {
                    self.source_version(conn, query, day)
                })?
            };
            if before == after && epoch == super::state::read_epoch(&mut anchor)? {
                return Ok(InvestmentContextCapture {
                    schema_version: 1,
                    source_version: before,
                    response: InvestmentContextResponse {
                        schema_version: 1,
                        query: query.clone(),
                        as_of_date: day,
                        currency,
                        funding,
                        recent_cash_flow,
                        household,
                    },
                });
            }
        }
        Err(JournalError::StateConflict)
    }
    fn validate(
        &self,
        conn: &mut SqliteConnection,
        capture: &InvestmentContextCapture,
    ) -> JournalResult<()> {
        let (currency, timezone) = self.settings();
        let today = wealthfolio_core::utils::time_utils::user_today(
            wealthfolio_core::utils::time_utils::parse_user_timezone(&timezone)?,
        );
        if capture.response.currency != currency
            || capture.response.as_of_date != today
            || self.source_version(conn, &capture.response.query, today)? != capture.source_version
        {
            return Err(JournalError::StateConflict);
        }
        Ok(())
    }
}
pub(super) fn issue(
    code: &str,
    source_id: Option<String>,
    currency: Option<String>,
) -> InvestmentContextIssue {
    InvestmentContextIssue {
        code: code.into(),
        source_id,
        currency,
    }
}
pub(super) fn quality(available: bool, issues: &[InvestmentContextIssue]) -> ContextQuality {
    if issues.is_empty() {
        ContextQuality::Complete
    } else if available {
        ContextQuality::Partial
    } else {
        ContextQuality::Unavailable
    }
}
fn unavailable(day: NaiveDate, currency: &str, scope: Value, code: &str) -> InvestmentContextBlock {
    InvestmentContextBlock {
        scope,
        as_of_date: day,
        source_data_as_of: None,
        currency: currency.into(),
        quality: ContextQuality::Unavailable,
        stale: false,
        issues: vec![issue(code, None, None)],
        data: Value::Null,
    }
}
