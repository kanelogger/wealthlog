#![allow(
    dead_code,
    reason = "shared integration harness; each test target uses a subset"
)]

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use tempfile::TempDir;
use wealthfolio_business::{investment_journal::*, investments::*, BusinessChannel, BusinessStore};
use wealthfolio_core::{
    accounts::AccountService,
    activities::ActivityService,
    assets::AssetService,
    events::NoOpDomainEventSink,
    fx::{FxService, FxServiceTrait},
    portfolio::{coordinator::FactSources, net_worth::NetWorthService, snapshot::SnapshotService},
    quotes::QuoteService,
    secrets::SecretStore,
    utils::time_utils::{parse_user_timezone, user_today},
};
use wealthfolio_storage_sqlite::{
    accounts::AccountRepository,
    activities::ActivityRepository,
    assets::{AlternativeAssetRepository, AssetRepository},
    db::{write_actor::spawn_writer, DbAccess, DbEncryptionKey},
    fx::FxRepository,
    market_data::{MarketDataRepository, QuoteSyncStateRepository},
    portfolio::{
        projection::ProjectionStore, snapshot::SnapshotRepository, valuation::ValuationRepository,
    },
};

struct NoSecrets;
impl SecretStore for NoSecrets {
    fn set_secret(&self, _: &str, _: &str) -> wealthfolio_core::Result<()> {
        panic!("test must not write secrets")
    }
    fn get_secret(&self, _: &str) -> wealthfolio_core::Result<Option<String>> {
        Ok(None)
    }
    fn delete_secret(&self, _: &str) -> wealthfolio_core::Result<()> {
        panic!("test must not delete secrets")
    }
}

pub struct Harness {
    _dir: TempDir,
    pub access: DbAccess,
    pub store: BusinessStore,
    pub ordinary: wealthfolio_business::BusinessService,
    pub investments: Arc<InvestmentService>,
    pub reader: Arc<AccountingResourceReader>,
    pub facts: FactSources,
    pub context: Arc<AccountingContextReader>,
    pub net_worth: Arc<NetWorthService>,
    pub day: String,
}

impl Harness {
    pub async fn new() -> Self {
        Self::with_encryption(false).await
    }

    pub async fn encrypted() -> Self {
        Self::with_encryption(true).await
    }

    async fn with_encryption(encrypted: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let access = DbAccess::new(
            dir.path().join("app.db").to_str().unwrap(),
            encrypted.then(|| Arc::new(DbEncryptionKey::generate())),
        );
        access.prepare().unwrap();
        access.run_migrations().unwrap();
        let day = user_today(parse_user_timezone("Asia/Shanghai").unwrap()).to_string();
        let db = access.connect_rusqlite().unwrap();
        db.execute_batch("UPDATE market_data_providers SET enabled=0;
            INSERT INTO accounts(id,name,account_type,currency,is_default,is_active,created_at,updated_at,is_archived,tracking_mode) VALUES('transactions','Transactions','SECURITIES','CNY',0,1,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,0,'TRANSACTIONS'),('manual','Manual','SECURITIES','CNY',0,1,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP,0,'HOLDINGS');
            INSERT INTO assets(id,kind,name,display_code,is_active,quote_mode,quote_ccy,instrument_type,created_at,updated_at) VALUES('stock','INVESTMENT','Stock','TEST',1,'MARKET','CNY','EQUITY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP);").unwrap();
        db.execute("INSERT INTO activities(id,account_id,activity_type,status,activity_date,currency,amount,created_at,updated_at) VALUES('deposit','transactions','DEPOSIT','POSTED',?1,'CNY','1000',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[&day]).unwrap();
        drop(db);
        let pool = access.create_pool().unwrap();
        let writer = spawn_writer(pool.as_ref().clone()).unwrap();
        let store = BusinessStore::new(pool.clone(), writer.clone());
        let accounts = Arc::new(AccountRepository::new(pool.clone(), writer.clone()));
        let activities = Arc::new(ActivityRepository::new(pool.clone(), writer.clone()));
        let assets = Arc::new(AssetRepository::new(pool.clone(), writer.clone()));
        let snapshot_repo = Arc::new(SnapshotRepository::new(pool.clone(), writer.clone()));
        let market = Arc::new(MarketDataRepository::new(pool.clone(), writer.clone()));
        let sync = Arc::new(QuoteSyncStateRepository::new(pool.clone(), writer.clone()));
        let fx_repo = Arc::new(FxRepository::new(pool.clone(), writer.clone()));
        let fx = Arc::new(FxService::new(fx_repo.clone()));
        fx.initialize().unwrap();
        let quotes = Arc::new(
            QuoteService::new(
                market.clone(),
                sync.clone(),
                market,
                assets.clone(),
                activities.clone(),
                Arc::new(NoSecrets),
            )
            .await
            .unwrap(),
        );
        let base = Arc::new(RwLock::new("CNY".into()));
        let timezone = Arc::new(RwLock::new("Asia/Shanghai".into()));
        let account_service = Arc::new(AccountService::new(
            accounts.clone(),
            fx.clone(),
            base.clone(),
            Arc::new(NoOpDomainEventSink),
            assets.clone(),
            sync,
        ));
        let asset_service = Arc::new(AssetService::new(assets.clone(), quotes.clone()).unwrap());
        let activity_service = Arc::new(
            ActivityService::new(
                activities.clone(),
                account_service.clone(),
                asset_service.clone(),
                fx.clone(),
                quotes.clone(),
            )
            .with_timezone(timezone.clone()),
        );
        let snapshots = Arc::new(SnapshotService::new(
            timezone.clone(),
            accounts.clone(),
            snapshot_repo.clone(),
            activities.clone(),
        ));
        let valuations = Arc::new(ValuationRepository::new(pool.clone(), writer.clone()));
        let alternatives = Arc::new(AlternativeAssetRepository::new(
            pool.clone(),
            writer.clone(),
        ));
        let projections = Arc::new(ProjectionStore::new(pool, writer));
        let facts = FactSources {
            accounts,
            activities,
            assets,
            quotes,
            fx_rates: fx_repo,
            snapshots: snapshot_repo,
            projections,
        };
        let reader = Arc::new(AccountingResourceReader::new(
            store.clone(),
            facts.clone(),
            snapshots.clone(),
            base.clone(),
            timezone.clone(),
        ));
        let net_worth = Arc::new(
            NetWorthService::new(
                base.clone(),
                facts.accounts.clone(),
                facts.assets.clone(),
                snapshots.clone(),
                facts.quotes.clone(),
                valuations,
                fx.clone(),
            )
            .with_loan_payments(alternatives, timezone.clone()),
        );
        let context = Arc::new(AccountingContextReader::new(
            store.clone(),
            reader.clone(),
            account_service.clone(),
            snapshots.clone(),
            asset_service.clone(),
            facts.quotes.clone(),
            fx.clone(),
            net_worth.clone(),
            base.clone(),
            timezone.clone(),
        ));
        let ordinary = wealthfolio_business::BusinessService::new(
            activity_service.clone(),
            account_service.clone(),
            store.clone(),
            "ledger",
        );
        let investments = Arc::new(InvestmentService::new(
            store.clone(),
            "ledger".into(),
            activity_service,
            account_service,
            snapshots,
            asset_service,
            fx,
            base,
            timezone,
        ));
        Self {
            _dir: dir,
            access,
            store,
            ordinary,
            investments,
            reader,
            facts,
            context,
            net_worth,
            day,
        }
    }

    pub async fn initialize_manual(&self) {
        let state = self.investments.get_state("manual").await.unwrap();
        self.investments
            .patch_snapshot(
                PatchHoldingsSnapshot {
                    account_id: "manual".into(),
                    snapshot_date: self.day.clone(),
                    expected_state_version: state["stateVersion"].as_str().unwrap().into(),
                    positions: vec![],
                    cash_balances: BTreeMap::from([("CNY".into(), "1000".into())]),
                    notes: "Observed opening resources".into(),
                    request_key: "opening".into(),
                },
                BusinessChannel::Gui,
                "session",
                true,
            )
            .await
            .unwrap();
    }
    pub async fn trade(&self) -> RecordManualTrade {
        let state = self.investments.get_state("manual").await.unwrap();
        RecordManualTrade {
            account_id: "manual".into(),
            activity_date: self.day.clone(),
            expected_state_version: state["stateVersion"].as_str().unwrap().into(),
            asset_id: "stock".into(),
            side: "BUY".into(),
            quantity: "10".into(),
            price: "10".into(),
            currency: "CNY".into(),
            fee: Some("1".into()),
            tax: Some("0.5".into()),
            observed_cash_balance: None,
            notes: None,
            request_key: "trade".into(),
        }
    }

    pub fn journal(&self) -> InvestmentJournalService {
        InvestmentJournalService::new(
            self.store.clone(),
            "ledger".into(),
            self.reader.clone(),
            Arc::new(RwLock::new("Asia/Shanghai".into())),
        )
        .with_bookkeeping(self.investments.clone())
    }

    pub fn seed_purpose(&self, id: &str) {
        self.access.connect_rusqlite().unwrap().execute("INSERT INTO fund_purposes(id,name,category,currency,created_at,updated_at) VALUES(?1,?1,'CUSTOM','CNY',CURRENT_TIMESTAMP,CURRENT_TIMESTAMP)",[id]).unwrap();
    }
}

pub fn journal_caller() -> JournalCaller {
    JournalCaller {
        channel: BusinessChannel::Cli,
        actor_id: "agent".into(),
        credential_id: Some("token".into()),
        actor_kind: JournalActorKind::Program,
        authenticated: true,
        can_read: true,
        can_write: true,
        can_confirm: false,
        fact_permissions: JournalFactPermissions {
            holdings_write: true,
            transfers_write: true,
            activities_write: false,
        },
    }
}

pub fn manual_cash() -> FundResource {
    FundResource::Cash {
        account_id: "manual".into(),
        currency: "CNY".into(),
    }
}
pub fn manual_security() -> FundResource {
    FundResource::Security {
        account_id: "manual".into(),
        asset_id: "stock".into(),
    }
}
pub fn exact(s: &str) -> ExactDecimal {
    ExactDecimal::parse(s).unwrap()
}
pub fn funded_share(id: &str, quantity: &str, cash: &str, top_up: &str) -> TradePurposeShare {
    TradePurposeShare {
        purpose_id: id.into(),
        quantity: exact(quantity),
        net_cash: exact(cash),
        top_ups: vec![OwnershipTopUp {
            resource: manual_cash(),
            amount: exact(top_up),
        }],
    }
}
