//! Agent portfolio bookkeeping. All mutations, replay records and audits share
//! one writer transaction; this module never executes a brokerage order.
use crate::requests::hash_json;
use crate::store::{insert_audit_tx, upsert_completed_request_tx, RequestRow};
use crate::{BusinessAuditEntry, BusinessChannel, BusinessStore, RequestRecord, RequestReplay};
use chrono::NaiveDate;
use diesel::prelude::*;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, RwLock};
use wealthfolio_core::{
    accounts::{Account, AccountServiceTrait, TrackingMode},
    activities::{Activity, ActivityServiceTrait, InternalTransferPairRequest, NewActivity},
    assets::AssetServiceTrait,
    errors::Error,
    fx::FxServiceTrait,
    portfolio::snapshot::{AccountStateSnapshot, Position, SnapshotServiceTrait, SnapshotSource},
    utils::time_utils::{parse_user_timezone_or_default, user_today},
};
use wealthfolio_storage_sqlite::{
    accounts::AccountDB,
    activities::ActivityDB,
    db::{get_connection, DbWriteTx},
    errors::StorageError,
    portfolio::snapshot::{AccountStateSnapshotDB, SnapshotRepository},
    schema::{accounts, activities, business_requests, holdings_snapshots},
};

#[derive(Debug, thiserror::Error)]
pub enum InvestmentError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0}")]
    OutOfScope(String),
    #[error("account state changed; read it again before preparing a new request")]
    StateConflict,
    #[error("requestKey already identifies different content")]
    RequestConflict,
    #[error("an activity with this deduplication identity already exists")]
    DuplicateActivity,
    #[error(transparent)]
    Storage(Error),
}
impl From<Error> for InvestmentError {
    fn from(error: Error) -> Self {
        match &error {
            Error::Validation(_) => Self::InvalidInput(error.to_string()),
            Error::Activity(wealthfolio_core::activities::ActivityError::DuplicateActivity {
                ..
            }) => Self::DuplicateActivity,
            Error::Activity(_) => Self::InvalidInput(error.to_string()),
            _ => Self::Storage(error),
        }
    }
}

impl InvestmentError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "invalid_input",
            Self::OutOfScope(_) => "investment_out_of_scope",
            Self::StateConflict => "state_conflict",
            Self::RequestConflict => "request_conflict",
            Self::DuplicateActivity => "duplicate_activity",
            Self::Storage(_) => "storage_error",
        }
    }
    pub fn retryable(&self) -> bool {
        matches!(self, Self::Storage(_))
    }
}
type Result<T> = std::result::Result<T, InvestmentError>;

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateCashTransfer {
    pub from_account_id: String,
    pub to_account_id: String,
    pub activity_date: String,
    pub source_currency: String,
    pub destination_currency: String,
    pub source_amount: String,
    #[serde(default)]
    pub destination_amount: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    pub request_key: String,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PositionPatch {
    pub asset_id: String,
    pub quantity: String,
    pub average_cost: String,
    pub currency: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PatchHoldingsSnapshot {
    pub account_id: String,
    pub snapshot_date: String,
    pub expected_state_version: String,
    #[serde(default)]
    pub positions: Vec<PositionPatch>,
    #[serde(default)]
    pub cash_balances: BTreeMap<String, String>,
    pub notes: String,
    pub request_key: String,
}
#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordManualTrade {
    pub account_id: String,
    pub activity_date: String,
    pub expected_state_version: String,
    pub asset_id: String,
    pub side: String,
    pub quantity: String,
    pub price: String,
    pub currency: String,
    #[serde(default)]
    pub fee: Option<String>,
    #[serde(default)]
    pub tax: Option<String>,
    #[serde(default)]
    pub observed_cash_balance: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    pub request_key: String,
}

pub struct InvestmentService {
    pub(crate) store: BusinessStore,
    ledger_id: String,
    activities: Arc<dyn ActivityServiceTrait>,
    accounts: Arc<dyn AccountServiceTrait>,
    snapshots: Arc<dyn SnapshotServiceTrait>,
    assets: Arc<dyn AssetServiceTrait>,
    fx: Arc<dyn FxServiceTrait>,
    base_currency: Arc<RwLock<String>>,
    timezone: Arc<RwLock<String>>,
}
impl InvestmentService {
    pub(crate) async fn prepare_ledger_trade(
        &self,
        input: &crate::investment_journal::RecordLedgerTrade,
        id: String,
    ) -> Result<PreparedLedgerTrade> {
        self.prepare_ledger_trade_inner(input, id, true).await
    }

    pub(crate) async fn prepare_ledger_trade_fact(
        &self,
        input: &crate::investment_journal::RecordLedgerTrade,
        id: String,
    ) -> Result<PreparedLedgerTrade> {
        self.prepare_ledger_trade_inner(input, id, false).await
    }

    async fn prepare_ledger_trade_inner(
        &self,
        input: &crate::investment_journal::RecordLedgerTrade,
        id: String,
        ownership_required: bool,
    ) -> Result<PreparedLedgerTrade> {
        let account = self.account(&input.account_id, false).await?;
        if account.tracking_mode == TrackingMode::Holdings {
            return Err(InvestmentError::OutOfScope(
                "use manual snapshot recording for a holdings account".into(),
            ));
        }
        let asset = self.assets.get_asset_by_id(&input.asset_id)?;
        if !asset.is_unit_equity() {
            return Err(InvestmentError::OutOfScope(
                "ledger journal supports existing stock/ETF/fund assets only".into(),
            ));
        }
        if !matches!(input.side.as_str(), "BUY" | "SELL") || input.quantity.value() <= Decimal::ZERO
        {
            return Err(invalid(
                "trade requires a BUY/SELL side and positive quantity",
            ));
        }
        if input.fx_rate.is_some_and(|v| v.value() <= Decimal::ZERO) {
            return Err(invalid("fxRate must be positive"));
        }
        if wealthfolio_core::fx::currency::get_normalization_rule(&input.currency).is_none() {
            currency(&input.currency)?;
        }
        if ownership_required && input.amount.is_none() {
            return Err(invalid(
                "final cash amount is required for atomic ownership conversion",
            ));
        }
        for value in [input.amount, input.unit_price, input.fee, input.tax]
            .into_iter()
            .flatten()
        {
            if value.value() < Decimal::ZERO {
                return Err(invalid("trade amounts must be nonnegative magnitudes"));
            }
        }
        let guard = self.guard(&input.account_id, self.today())?;
        let precision = if NaiveDate::parse_from_str(input.activity_date.trim(), "%Y-%m-%d").is_ok()
        {
            "DATE"
        } else {
            "DATETIME"
        };
        let activity=NewActivity { id:Some(id),account_id:input.account_id.clone(),asset:Some(wealthfolio_core::activities::AssetResolutionInput { id:Some(input.asset_id.clone()),..Default::default() }),activity_type:input.side.clone(),subtype:None,activity_date:input.activity_date.clone(),quantity:Some(input.quantity.value()),unit_price:input.unit_price.map(|v| v.value()),currency:input.currency.clone(),fee:input.fee.map(|v| v.value()),tax:input.tax.map(|v| v.value()),amount:input.amount.map(|v| v.value()),status:Some(wealthfolio_core::activities::ActivityStatus::Posted),notes:input.notes.clone(),fx_rate:input.fx_rate.map(|v| v.value()),metadata:Some(json!({"investmentJournal":{"timePrecision":precision,"declaredDate":input.activity_date.trim()}}).to_string()),source_system:Some("MANUAL".into()),source_record_id:None,source_group_id:None,idempotency_key:input.idempotency_key.clone(),needs_review:Some(input.amount.is_none()) };
        let activity = if !ownership_required && input.amount.is_none() {
            self.activities
                .prepare_incomplete_trade_for_atomic_commit(activity)
                .await?
        } else {
            self.activities
                .prepare_activity_for_atomic_commit(activity)
                .await?
        };
        Ok(PreparedLedgerTrade { activity, guard })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        store: BusinessStore,
        ledger_id: String,
        activities: Arc<dyn ActivityServiceTrait>,
        accounts: Arc<dyn AccountServiceTrait>,
        snapshots: Arc<dyn SnapshotServiceTrait>,
        assets: Arc<dyn AssetServiceTrait>,
        fx: Arc<dyn FxServiceTrait>,
        base_currency: Arc<RwLock<String>>,
        timezone: Arc<RwLock<String>>,
    ) -> Self {
        Self {
            store,
            ledger_id,
            activities,
            accounts,
            snapshots,
            assets,
            fx,
            base_currency,
            timezone,
        }
    }

    fn today(&self) -> NaiveDate {
        user_today(parse_user_timezone_or_default(
            &self.timezone.read().unwrap_or_else(|e| e.into_inner()),
        ))
    }
    fn date(&self, text: &str, current_only: bool) -> Result<NaiveDate> {
        let day = NaiveDate::parse_from_str(text, "%Y-%m-%d")
            .map_err(|_| invalid("date must be YYYY-MM-DD"))?;
        if day.to_string() != text || day > self.today() || (current_only && day != self.today()) {
            return Err(invalid(if current_only {
                "manual holdings writes require today's snapshotDate/activityDate"
            } else {
                "future dates are not supported"
            }));
        }
        Ok(day)
    }
    async fn account(&self, id: &str, manual: bool) -> Result<Account> {
        let account = self.accounts.get_account(id)?;
        if !account.is_active
            || account.is_archived
            || !matches!(
                account.account_type.as_str(),
                "CASH" | "SECURITIES" | "CRYPTOCURRENCY"
            )
            || (manual
                && (account.tracking_mode != TrackingMode::Holdings
                    || account.account_type != "SECURITIES"))
        {
            return Err(InvestmentError::OutOfScope("requires an active, non-archived supported account; manual writes require a HOLDINGS securities account".into()));
        }
        Ok(account)
    }
    fn version(&self, id: &str, date: NaiveDate) -> Result<String> {
        let mut conn = get_connection(self.store.pool())?;
        Ok(state_version(&mut conn, id, date)?)
    }
    fn snapshot(&self, account: &Account, day: NaiveDate) -> Result<AccountStateSnapshot> {
        let mut snapshots = self
            .snapshots
            .get_latest_snapshots_as_of(std::slice::from_ref(&account.id), day)?;
        Ok(snapshots
            .remove(&account.id)
            .unwrap_or_else(|| AccountStateSnapshot {
                account_id: account.id.clone(),
                currency: account.currency.clone(),
                snapshot_date: day,
                source: SnapshotSource::ManualEntry,
                ..AccountStateSnapshot::default()
            }))
    }
    pub async fn get_state(&self, id: &str) -> Result<Value> {
        let account = self.account(id, false).await?;
        let day = self.today();
        let version = self.version(id, day)?;
        let snapshot = self.snapshot(&account, day)?;
        let ids: Vec<_> = snapshot.positions.keys().cloned().collect();
        let assets = self.assets.get_assets_by_asset_ids(&ids).await?;
        if version != self.version(id, day)? {
            return Err(InvestmentError::StateConflict);
        }
        let mut positions: Vec<_> = snapshot.positions.values().map(|p| {
            let asset = assets.iter().find(|a| a.id == p.asset_id);
            json!({"assetId":p.asset_id,"symbol":asset.map(|a| &a.display_code),
                "quantity":p.quantity.normalize().to_string(),"averageCost":p.average_cost.normalize().to_string(),
                "totalCostBasis":p.total_cost_basis.normalize().to_string(),"currency":p.currency})
        }).collect();
        positions.sort_by(|a, b| a["assetId"].as_str().cmp(&b["assetId"].as_str()));
        let cash: BTreeMap<_, _> = snapshot
            .cash_balances
            .iter()
            .map(|(c, a)| (c.clone(), a.normalize().to_string()))
            .collect();
        Ok(
            json!({"accountId":id,"accountName":account.name,"trackingMode":account.tracking_mode,
            "currency":account.currency,"asOfDate":day,"snapshotDate":if snapshot.id.is_empty(){None}else{Some(snapshot.snapshot_date)},"hasSnapshot":!snapshot.id.is_empty(),
            "stateVersion":version,"positions":positions,"cashBalances":cash,"manualCashCarryPolicy":"AFTER_SNAPSHOT_DATE"}),
        )
    }
    fn record<T: Serialize>(
        &self,
        operation: &str,
        key: &str,
        actor: &str,
        input: &T,
    ) -> Result<RequestRecord> {
        if key.trim().is_empty() || key.len() > 256 {
            return Err(invalid("requestKey must be nonblank and at most 256 bytes"));
        }
        Ok(RequestRecord::new(
            &self.ledger_id,
            actor,
            operation,
            key,
            serde_json::to_value(input).map_err(|e| invalid(e.to_string()))?,
        ))
    }
    fn replay(&self, record: &RequestRecord) -> Result<Option<Value>> {
        let existing = self.store.request(
            &record.ledger_id,
            &record.subject,
            &record.operation,
            &record.request_key,
        )?;
        replay_result(RequestReplay::decide(
            existing.as_ref(),
            &record.request_hash,
        ))
    }

    pub async fn create_transfer(
        &self,
        input: CreateCashTransfer,
        channel: BusinessChannel,
        actor: &str,
        authenticated: bool,
    ) -> Result<Value> {
        let record = self.record("cash_transfer_create", &input.request_key, actor, &input)?;
        if let Some(result) = self.replay(&record)? {
            return Ok(result);
        }
        let prepared = self.prepare_cash_transfer_inner(&input, false).await?;
        self.commit(
            record,
            prepared.activities,
            None,
            prepared.guards,
            prepared.result,
            channel,
            actor,
            authenticated,
        )
        .await
    }

    pub(crate) async fn prepare_cash_transfer(
        &self,
        input: &CreateCashTransfer,
    ) -> Result<PreparedCashTransfer> {
        self.prepare_cash_transfer_inner(input, true).await
    }

    async fn prepare_cash_transfer_inner(
        &self,
        input: &CreateCashTransfer,
        atomic: bool,
    ) -> Result<PreparedCashTransfer> {
        let day = self.date(&input.activity_date, false)?;
        let from_account = self.account(&input.from_account_id, false).await?;
        let to_account = self.account(&input.to_account_id, false).await?;
        if input.from_account_id == input.to_account_id {
            return Err(invalid("transfer accounts must differ"));
        }
        currency(&input.source_currency)?;
        currency(&input.destination_currency)?;
        let amount = positive(&input.source_amount, "sourceAmount")?;
        let destination = input
            .destination_amount
            .as_deref()
            .map(|s| positive(s, "destinationAmount"))
            .transpose()?;
        if input.source_currency != input.destination_currency && destination.is_none() {
            return Err(invalid(
                "cross-currency transfer requires destinationAmount",
            ));
        }
        if input.source_currency == input.destination_currency
            && destination.is_some_and(|v| v != amount)
        {
            return Err(invalid("same-currency amounts must match"));
        }
        let mut dedupe = serde_json::to_value(input).map_err(|e| invalid(e.to_string()))?;
        dedupe
            .as_object_mut()
            .ok_or_else(|| invalid("invalid transfer"))?
            .remove("requestKey");
        let identity = match input.idempotency_key.as_deref() {
            Some(key) if key.trim().is_empty() => {
                return Err(invalid("idempotencyKey must not be blank"))
            }
            Some(key) => hash_json(&json!({"key":key})),
            None => hash_json(&dedupe),
        };
        let guards = vec![
            self.guard(&input.from_account_id, day)?,
            self.guard(&input.to_account_id, day)?,
        ];
        // A manual observation is an end-of-day baseline. A backdated or
        // same-day transfer may already be included in it; expose that fact
        // rather than silently changing an observed balance twice.
        let mut cash_reconciliation_accounts = Vec::new();
        for account in [&from_account, &to_account] {
            if account.tracking_mode == TrackingMode::Holdings {
                let observed = self.snapshot(account, self.today())?;
                if observed.id.is_empty() || observed.snapshot_date >= day {
                    cash_reconciliation_accounts.push(account.id.clone());
                }
            }
        }
        let request = InternalTransferPairRequest {
            transfer_out_id: None,
            transfer_in_id: None,
            source_group_id: Some(format!("agent-transfer-{identity}")),
            from_account_id: input.from_account_id.clone(),
            to_account_id: input.to_account_id.clone(),
            activity_date: input.activity_date.clone(),
            source_amount: Some(amount),
            destination_amount: destination,
            source_currency: input.source_currency.clone(),
            destination_currency: input.destination_currency.clone(),
            fx_rate: None,
            notes: input.notes.clone(),
            transfer_mode: Some("cash".into()),
        };
        let mut prepared = if atomic {
            self.activities
                .prepare_internal_cash_transfer_for_atomic_commit(request)
                .await?
        } else {
            self.activities
                .prepare_internal_cash_transfer(request)
                .await?
        };
        for (index, a) in prepared.iter_mut().enumerate() {
            a.idempotency_key = Some(format!("agent-transfer-{identity}-{index}"));
        }
        let result = json!({"fromAccountId":input.from_account_id,"toAccountId":input.to_account_id,
            "activityDate":input.activity_date,"sourceAmount":amount.normalize().to_string(),
            "destinationAmount":destination.unwrap_or(amount).normalize().to_string(),
            "sourceCurrency":input.source_currency,"destinationCurrency":input.destination_currency,
            "cashReconciliationAccounts":cash_reconciliation_accounts});
        Ok(PreparedCashTransfer {
            activities: prepared,
            guards,
            result,
            cash_reconciliation_accounts,
        })
    }
    fn guard(&self, id: &str, date: NaiveDate) -> Result<StateGuard> {
        Ok(StateGuard {
            account_id: id.into(),
            date,
            version: self.version(id, date)?,
        })
    }
    async fn manual_baseline(
        &self,
        id: &str,
        date: &str,
        expected: &str,
    ) -> Result<(Account, AccountStateSnapshot, StateGuard)> {
        let day = self.date(date, true)?;
        let account = self.account(id, true).await?;
        let guard = self.guard(id, day)?;
        if expected.is_empty() || guard.version != expected {
            return Err(InvestmentError::StateConflict);
        }
        let snapshot = self.snapshot(&account, day)?;
        Ok((account, snapshot, guard))
    }
    pub async fn patch_snapshot(
        &self,
        input: PatchHoldingsSnapshot,
        channel: BusinessChannel,
        actor: &str,
        authenticated: bool,
    ) -> Result<Value> {
        let record = self.record("holdings_snapshot_patch", &input.request_key, actor, &input)?;
        if let Some(result) = self.replay(&record)? {
            return Ok(result);
        }
        if input.notes.trim().is_empty()
            || (input.positions.is_empty() && input.cash_balances.is_empty())
        {
            return Err(invalid(
                "notes and at least one position or cash patch are required",
            ));
        }
        let (_, mut snapshot, guard) = self
            .manual_baseline(
                &input.account_id,
                &input.snapshot_date,
                &input.expected_state_version,
            )
            .await?;
        let mut seen = HashSet::new();
        for patch in &input.positions {
            if !seen.insert(&patch.asset_id) {
                return Err(invalid("duplicate assetId in positions"));
            }
            self.apply_position(&mut snapshot, patch)?;
        }
        let mut adjustments = BTreeMap::new();
        for (ccy, amount) in &input.cash_balances {
            currency(ccy)?;
            let amount = decimal(amount, "cashBalances")?;
            let before = snapshot.cash_balances.get(ccy).copied().unwrap_or_default();
            adjustments.insert(ccy.clone(), (amount - before).normalize().to_string());
            snapshot.cash_balances.insert(ccy.clone(), amount);
        }
        self.finish_snapshot(&mut snapshot, guard.date)?;
        let result = json!({"accountId":input.account_id,"snapshotId":snapshot.id,"snapshotDate":guard.date,
            "recordingMode":"MANUAL_SNAPSHOT","cashAdjustments":adjustments,"notes":input.notes});
        self.commit(
            record,
            vec![],
            Some(snapshot),
            vec![guard],
            result,
            channel,
            actor,
            authenticated,
        )
        .await
    }
    fn apply_position(
        &self,
        snapshot: &mut AccountStateSnapshot,
        patch: &PositionPatch,
    ) -> Result<()> {
        currency(&patch.currency)?;
        let quantity = nonnegative(&patch.quantity, "quantity")?;
        let average = nonnegative(&patch.average_cost, "averageCost")?;
        let asset = self.assets.get_asset_by_id(&patch.asset_id)?;
        if !asset.is_unit_equity() || asset.quote_ccy != patch.currency {
            return Err(InvestmentError::OutOfScope(
                "position requires an existing stock/ETF/fund asset in the supplied quote currency"
                    .into(),
            ));
        }
        if quantity.is_zero() {
            snapshot.positions.remove(&patch.asset_id);
            return Ok(());
        }
        let mut p = snapshot
            .positions
            .remove(&patch.asset_id)
            .unwrap_or_else(|| Position {
                id: format!("POS-{}-{}", patch.asset_id, snapshot.account_id),
                account_id: snapshot.account_id.clone(),
                asset_id: patch.asset_id.clone(),
                ..Position::default()
            });
        if p.contract_multiplier != Decimal::ONE {
            return Err(InvestmentError::OutOfScope(
                "derivatives require their own trade workflow".into(),
            ));
        }
        p.quantity = quantity;
        p.average_cost = average;
        p.total_cost_basis = mul(quantity, average)?;
        p.currency = patch.currency.clone();
        p.cost_basis_account = None;
        p.cost_basis_base = None;
        p.last_updated = wealthfolio_core::utils::clock::now();
        snapshot.positions.insert(p.asset_id.clone(), p);
        Ok(())
    }
    pub async fn record_manual_trade(
        &self,
        input: RecordManualTrade,
        channel: BusinessChannel,
        actor: &str,
        authenticated: bool,
    ) -> Result<Value> {
        let record = self.record("manual_trade_record", &input.request_key, actor, &input)?;
        if let Some(result) = self.replay(&record)? {
            return Ok(result);
        }
        let prepared = self.prepare_manual_trade(&input).await?;
        self.commit(
            record,
            vec![],
            Some(prepared.snapshot),
            vec![prepared.guard],
            prepared.result,
            channel,
            actor,
            authenticated,
        )
        .await
    }

    /// Shared preparation for legacy and journal compound commands. This never
    /// writes activities or snapshots and preserves unknown fee/tax inputs.
    pub(crate) async fn prepare_manual_trade(
        &self,
        input: &RecordManualTrade,
    ) -> Result<PreparedManualTrade> {
        let (account, mut snapshot, guard) = self
            .manual_baseline(
                &input.account_id,
                &input.activity_date,
                &input.expected_state_version,
            )
            .await?;
        if snapshot.id.is_empty() {
            return Err(invalid(
                "initialize a manual holdings/cash snapshot before recording a trade",
            ));
        }
        let baseline = snapshot.clone();
        currency(&input.currency)?;
        if input.currency != account.currency {
            return Err(InvestmentError::OutOfScope(
                "manual trades currently require account-currency settlement".into(),
            ));
        }
        let amounts = ManualTradeAmounts::prepare(input)?;
        let quantity = amounts.quantity;
        let price = amounts.price;
        let gross = amounts.gross;
        let charges = amounts.charges;
        let old = snapshot.positions.get(&input.asset_id);
        if old
            .is_some_and(|p| p.currency != input.currency || p.contract_multiplier != Decimal::ONE)
        {
            return Err(InvestmentError::OutOfScope(
                "position currency/multiplier is incompatible".into(),
            ));
        }
        let old_quantity = old.map(|p| p.quantity).unwrap_or_default();
        if old_quantity < Decimal::ZERO {
            return Err(InvestmentError::OutOfScope(
                "manual short positions require their own trade workflow".into(),
            ));
        }
        let old_cost = old.map(|p| p.total_cost_basis).unwrap_or_default();
        let (new_quantity, new_cost, cash_change) = match input.side.as_str() {
            "BUY" => (
                add(old_quantity, quantity)?,
                add(old_cost, add(gross, charges)?)?,
                amounts.cash_change("BUY")?,
            ),
            "SELL" if old_quantity >= quantity => {
                let remaining = old_quantity - quantity;
                let cost = if remaining.is_zero() {
                    Decimal::ZERO
                } else {
                    mul(old_cost, remaining)?
                        .checked_div(old_quantity)
                        .ok_or_else(|| invalid("cost out of range"))?
                };
                (remaining, cost, amounts.cash_change("SELL")?)
            }
            "SELL" => return Err(invalid("sell quantity exceeds the current manual position")),
            _ => return Err(invalid("side must be BUY or SELL")),
        };
        let average = if new_quantity.is_zero() {
            Decimal::ZERO
        } else {
            new_cost
                .checked_div(new_quantity)
                .ok_or_else(|| invalid("average cost out of range"))?
        };
        self.apply_position(
            &mut snapshot,
            &PositionPatch {
                asset_id: input.asset_id.clone(),
                quantity: new_quantity.normalize().to_string(),
                average_cost: average.to_string(),
                currency: input.currency.clone(),
            },
        )?;
        if let Some(p) = snapshot.positions.get_mut(&input.asset_id) {
            p.total_cost_basis = new_cost;
        }
        let calculated = add(
            snapshot
                .cash_balances
                .get(&input.currency)
                .copied()
                .unwrap_or_default(),
            cash_change,
        )?;
        let observed = input
            .observed_cash_balance
            .as_deref()
            .map(|s| decimal(s, "observedCashBalance"))
            .transpose()?
            .unwrap_or(calculated);
        snapshot
            .cash_balances
            .insert(input.currency.clone(), observed);
        self.finish_snapshot(&mut snapshot, guard.date)?;
        let result = json!({"accountId":input.account_id,"assetId":input.asset_id,"side":input.side,
            "quantity":quantity.normalize().to_string(),"price":price.normalize().to_string(),"grossAmount":gross.normalize().to_string(),
            "feesProvided":input.fee.is_some(),"taxProvided":input.tax.is_some(),
            "positionQuantity":new_quantity.normalize().to_string(),"totalCostBasis":new_cost.normalize().to_string(),
            "calculatedCashBalance":calculated.normalize().to_string(),"cashBalance":observed.normalize().to_string(),
            "cashAdjustment":(observed-calculated).normalize().to_string(),"snapshotId":snapshot.id,
            "recordingMode":"MANUAL_SNAPSHOT","snapshotDate":guard.date});
        Ok(PreparedManualTrade {
            baseline,
            snapshot,
            guard,
            result,
            trade_cash_change: cash_change,
            cash_adjustment: observed - calculated,
        })
    }
    fn finish_snapshot(&self, snapshot: &mut AccountStateSnapshot, day: NaiveDate) -> Result<()> {
        snapshot.id = AccountStateSnapshot::stable_id(&snapshot.account_id, day);
        snapshot.snapshot_date = day;
        snapshot.source = SnapshotSource::ManualEntry;
        snapshot.calculated_at = wealthfolio_core::utils::clock::now().naive_utc();
        snapshot.cost_basis = snapshot
            .positions
            .values()
            .try_fold(Decimal::ZERO, |sum, p| add(sum, p.total_cost_basis))?;
        let base = self.base_currency.read().unwrap_or_else(|e| e.into_inner());
        let sum = |target: &str| -> Result<Decimal> {
            snapshot
                .cash_balances
                .iter()
                .try_fold(Decimal::ZERO, |total, (c, a)| {
                    let converted = if c == target || a.is_zero() {
                        *a
                    } else {
                        self.fx.convert_currency_for_date(*a, c, target, day)?
                    };
                    add(total, converted)
                })
        };
        snapshot.cash_total_account_currency = sum(&snapshot.currency)?;
        snapshot.cash_total_base_currency = sum(&base)?;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    async fn commit(
        &self,
        record: RequestRecord,
        prepared: Vec<NewActivity>,
        snapshot: Option<AccountStateSnapshot>,
        guards: Vec<StateGuard>,
        result: Value,
        channel: BusinessChannel,
        actor: &str,
        authenticated: bool,
    ) -> Result<Value> {
        let audit = BusinessAuditEntry::new(
            &self.ledger_id,
            channel,
            actor,
            authenticated,
            &record.operation,
            "portfolio_bookkeeping",
        )
        .detail(record.request_snapshot.clone())
        .to_row();
        let outcome = commit_mutation(
            &self.store,
            record,
            prepared,
            snapshot.clone(),
            guards,
            result,
            audit,
        )
        .await?;
        if !outcome.replayed {
            for activity in &outcome.activities {
                self.activities.notify_activity_committed(None, activity);
            }
            if let Some(snapshot) = snapshot {
                self.snapshots.notify_manual_snapshot_committed(&snapshot);
            }
        }
        Ok(outcome.result)
    }
}

fn invalid(message: impl Into<String>) -> InvestmentError {
    InvestmentError::InvalidInput(message.into())
}
fn decimal(value: &str, field: &str) -> Result<Decimal> {
    value
        .parse()
        .map_err(|_| invalid(format!("{field} must be a finite decimal string")))
}
fn nonnegative(value: &str, field: &str) -> Result<Decimal> {
    let v = decimal(value, field)?;
    if v < Decimal::ZERO {
        Err(invalid(format!("{field} must be nonnegative")))
    } else {
        Ok(v)
    }
}
fn positive(value: &str, field: &str) -> Result<Decimal> {
    let v = nonnegative(value, field)?;
    if v.is_zero() {
        Err(invalid(format!("{field} must be positive")))
    } else {
        Ok(v)
    }
}
fn currency(value: &str) -> Result<()> {
    if value.len() == 3 && value.bytes().all(|b| b.is_ascii_uppercase()) {
        Ok(())
    } else {
        Err(invalid(
            "currency must be an uppercase three-letter currency code",
        ))
    }
}
fn add(a: Decimal, b: Decimal) -> Result<Decimal> {
    a.checked_add(b)
        .ok_or_else(|| invalid("amount out of range"))
}
fn mul(a: Decimal, b: Decimal) -> Result<Decimal> {
    a.checked_mul(b)
        .ok_or_else(|| invalid("amount out of range"))
}

pub(crate) struct PreparedCashTransfer {
    pub(crate) activities: Vec<NewActivity>,
    guards: Vec<StateGuard>,
    pub(crate) result: Value,
    pub(crate) cash_reconciliation_accounts: Vec<String>,
}

pub(crate) struct PreparedLedgerTrade {
    pub(crate) activity: NewActivity,
    guard: StateGuard,
}
impl PreparedLedgerTrade {
    pub(crate) fn validate(&self, conn: &mut diesel::SqliteConnection) -> Result<()> {
        if state_version(conn, &self.guard.account_id, self.guard.date)? != self.guard.version {
            Err(InvestmentError::StateConflict)
        } else {
            Ok(())
        }
    }
}

impl PreparedCashTransfer {
    pub(crate) fn validate(&self, conn: &mut diesel::SqliteConnection) -> Result<()> {
        for guard in &self.guards {
            if state_version(conn, &guard.account_id, guard.date)? != guard.version {
                return Err(InvestmentError::StateConflict);
            }
        }
        Ok(())
    }
}

impl InvestmentService {
    pub(crate) fn get_existing_transfer(
        &self,
        id: &str,
    ) -> Result<Option<wealthfolio_core::activities::InternalTransferPairResponse>> {
        Ok(self.activities.get_transfer_pair_for_activity(id.into())?)
    }
    pub(crate) fn notify_compound_activities_committed(&self, activities: &[Activity]) {
        for activity in activities {
            self.activities.notify_activity_committed(None, activity);
        }
    }
}

pub(crate) struct PreparedManualTrade {
    pub(crate) baseline: AccountStateSnapshot,
    pub(crate) snapshot: AccountStateSnapshot,
    guard: StateGuard,
    pub(crate) result: Value,
    pub(crate) trade_cash_change: Decimal,
    pub(crate) cash_adjustment: Decimal,
}

impl PreparedManualTrade {
    pub(crate) fn validate(&self, conn: &mut diesel::SqliteConnection) -> Result<()> {
        if state_version(conn, &self.guard.account_id, self.guard.date)? != self.guard.version {
            Err(InvestmentError::StateConflict)
        } else {
            Ok(())
        }
    }
}

impl InvestmentService {
    pub(crate) fn notify_compound_snapshot_committed(&self, snapshot: &AccountStateSnapshot) {
        self.snapshots.notify_manual_snapshot_committed(snapshot);
    }
}

/// The numeric part of manual-trade preparation, also used to index completed
/// legacy requests. Unknown charges remain None; only arithmetic defaults them.
pub(crate) struct ManualTradeAmounts {
    pub(crate) quantity: Decimal,
    pub(crate) price: Decimal,
    pub(crate) gross: Decimal,
    pub(crate) fee: Option<Decimal>,
    pub(crate) tax: Option<Decimal>,
    pub(crate) charges: Decimal,
}

impl ManualTradeAmounts {
    pub(crate) fn prepare(input: &RecordManualTrade) -> Result<Self> {
        let quantity = positive(&input.quantity, "quantity")?;
        let price = positive(&input.price, "price")?;
        let fee = input
            .fee
            .as_deref()
            .map(|v| nonnegative(v, "fee"))
            .transpose()?;
        let tax = input
            .tax
            .as_deref()
            .map(|v| nonnegative(v, "tax"))
            .transpose()?;
        Ok(Self {
            quantity,
            price,
            gross: mul(quantity, price)?,
            charges: add(fee.unwrap_or_default(), tax.unwrap_or_default())?,
            fee,
            tax,
        })
    }

    pub(crate) fn cash_change(&self, side: &str) -> Result<Decimal> {
        match side {
            "BUY" => Ok(-add(self.gross, self.charges)?),
            "SELL" => self
                .gross
                .checked_sub(self.charges)
                .ok_or_else(|| invalid("cash change out of range")),
            _ => Err(invalid("side must be BUY or SELL")),
        }
    }
}

#[derive(Clone)]
struct StateGuard {
    account_id: String,
    date: NaiveDate,
    version: String,
}
struct CommitResult {
    result: Value,
    activities: Vec<Activity>,
    replayed: bool,
}
fn state_version(
    conn: &mut diesel::SqliteConnection,
    id: &str,
    date: NaiveDate,
) -> wealthfolio_core::errors::Result<String> {
    let account = accounts::table
        .find(id)
        .first::<AccountDB>(conn)
        .map_err(StorageError::from)?;
    // Raw observations and source activities only: background valuation never
    // invalidates a version; a real user/agent edit always does.
    let snapshots = holdings_snapshots::table
        .filter(holdings_snapshots::account_id.eq(id))
        .filter(holdings_snapshots::source.ne("CALCULATED"))
        .order(holdings_snapshots::id.asc())
        .load::<AccountStateSnapshotDB>(conn)
        .map_err(StorageError::from)?;
    let rows = activities::table
        .filter(activities::account_id.eq(id))
        .order(activities::id.asc())
        .load::<ActivityDB>(conn)
        .map_err(StorageError::from)?;
    Ok(hash_json(
        &json!({"account":account,"snapshots":snapshots,"activities":rows,"asOfDate":date}),
    ))
}
fn replay_result(replay: RequestReplay) -> Result<Option<Value>> {
    match replay {
        RequestReplay::Conflict => Err(InvestmentError::RequestConflict),
        RequestReplay::Completed(text) => {
            let mut v: Value = serde_json::from_str(&text)
                .map_err(|_| invalid("stored request result is invalid"))?;
            v["replayed"] = json!(true);
            Ok(Some(v))
        }
        _ => Ok(None),
    }
}
fn replay_tx(
    tx: &mut DbWriteTx<'_>,
    record: &RequestRecord,
) -> wealthfolio_core::errors::Result<RequestReplay> {
    let row = business_requests::table
        .filter(business_requests::ledger_id.eq(&record.ledger_id))
        .filter(business_requests::subject.eq(&record.subject))
        .filter(business_requests::operation.eq(&record.operation))
        .filter(business_requests::request_key.eq(&record.request_key))
        .first::<RequestRow>(tx.conn())
        .optional()
        .map_err(StorageError::from)?;
    Ok(RequestReplay::decide(row.as_ref(), &record.request_hash))
}
#[allow(clippy::too_many_arguments)]
async fn commit_mutation(
    store: &BusinessStore,
    record: RequestRecord,
    prepared: Vec<NewActivity>,
    snapshot: Option<AccountStateSnapshot>,
    guards: Vec<StateGuard>,
    mut result: Value,
    mut audit: crate::store::NewAuditRow,
) -> Result<CommitResult> {
    enum Outcome {
        Committed(CommitResult),
        Conflict,
        StateConflict,
    }
    let outcome = store
        .transaction(move |tx| {
            match replay_tx(tx, &record)? {
                RequestReplay::Conflict => return Ok(Outcome::Conflict),
                RequestReplay::Completed(text) => {
                    let mut result: Value = serde_json::from_str(&text)?;
                    result["replayed"] = json!(true);
                    return Ok(Outcome::Committed(CommitResult {
                        result,
                        activities: vec![],
                        replayed: true,
                    }));
                }
                _ => {}
            }
            for guard in guards {
                if state_version(tx.conn(), &guard.account_id, guard.date)? != guard.version {
                    return Ok(Outcome::StateConflict);
                }
            }
            let request_id = format!("req-{}", uuid::Uuid::new_v4());
            let mut committed = Vec::new();
            for activity in prepared {
                let mut db = ActivityDB::from(activity);
                if db.id.is_empty() {
                    db.id = uuid::Uuid::new_v4().to_string();
                }
                wealthfolio_storage_sqlite::activities::assert_final_cash_floor(&db)?;
                let saved = diesel::insert_into(activities::table)
                    .values(&db)
                    .get_result::<ActivityDB>(tx.conn())
                    .map_err(StorageError::from)?;
                committed.push(Activity::from(saved));
            }
            if let Some(snapshot) = snapshot {
                SnapshotRepository::save_snapshot_tx(tx.conn(), snapshot)?;
            }
            result["requestId"] = json!(request_id);
            result["replayed"] = json!(false);
            if !committed.is_empty() {
                result["activityIds"] = json!(committed.iter().map(|a| &a.id).collect::<Vec<_>>());
            }
            upsert_completed_request_tx(
                tx,
                &record.ledger_id,
                &record.subject,
                &record.operation,
                &record.request_key,
                &record.request_hash,
                &serde_json::to_string(&record.request_snapshot)?,
                &request_id,
                &serde_json::to_string(&result)?,
            )?;
            audit.request_id = Some(request_id);
            audit.entity_id = result
                .get("snapshotId")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| committed.first().map(|a| a.id.clone()));
            insert_audit_tx(tx, &audit)?;
            Ok(Outcome::Committed(CommitResult {
                result,
                activities: committed,
                replayed: false,
            }))
        })
        .await
        .map_err(|e| {
            let ordinary = crate::OrdinaryActivityError::from(e);
            match ordinary {
                crate::OrdinaryActivityError::DuplicateActivity { .. } => {
                    InvestmentError::DuplicateActivity
                }
                crate::OrdinaryActivityError::Storage(e) => InvestmentError::Storage(e),
                other => invalid(other.to_string()),
            }
        })?;
    match outcome {
        Outcome::Committed(v) => Ok(v),
        Outcome::Conflict => Err(InvestmentError::RequestConflict),
        Outcome::StateConflict => Err(InvestmentError::StateConflict),
    }
}
