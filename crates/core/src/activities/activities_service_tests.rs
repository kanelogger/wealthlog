#[cfg(test)]
pub(crate) mod tests {
    use crate::accounts::{Account, AccountServiceTrait, AccountUpdate, NewAccount};
    use crate::activities::activities_model::*;
    use crate::activities::{
        ActivityError, ActivityRepositoryTrait, ActivityService, ActivityServiceTrait,
        TransferLinkState, ACTIVITY_TYPE_ADJUSTMENT,
    };
    use crate::assets::{
        Asset, AssetKind, AssetServiceTrait, InstrumentType, ProviderProfile, QuoteMode,
        UpdateAssetProfile,
    };
    use crate::errors::{DatabaseError, Error, Result};
    use crate::events::{DomainEvent, MockDomainEventSink};
    use crate::fx::{ExchangeRate, FxServiceTrait, NewExchangeRate};
    use crate::quotes::service::ProviderInfo;
    use crate::quotes::{
        LatestQuotePair, LatestQuoteSnapshot, Quote, QuoteServiceTrait, QuoteSyncState,
        ResolvedQuote, SymbolSearchResult, SymbolSyncPlan, SyncMode, SyncResult,
    };
    use async_trait::async_trait;
    use chrono::{DateTime, NaiveDate, Utc};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use serde_json::json;
    use std::collections::{HashMap, HashSet};
    use std::sync::{Arc, Mutex};

    // --- Mock AccountService ---
    #[derive(Clone)]
    struct MockAccountService {
        accounts: Arc<Mutex<Vec<Account>>>,
    }

    impl MockAccountService {
        fn new() -> Self {
            Self {
                accounts: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn add_account(&self, account: Account) {
            self.accounts.lock().unwrap().push(account);
        }
    }

    #[async_trait]
    impl AccountServiceTrait for MockAccountService {
        async fn create_account(&self, _new_account: NewAccount) -> Result<Account> {
            unimplemented!()
        }

        async fn update_account(&self, _account_update: AccountUpdate) -> Result<Account> {
            unimplemented!()
        }

        async fn delete_account(&self, _account_id: &str) -> Result<()> {
            unimplemented!()
        }

        fn get_account(&self, account_id: &str) -> Result<Account> {
            let accounts = self.accounts.lock().unwrap();
            accounts
                .iter()
                .find(|a| a.id == account_id)
                .cloned()
                .ok_or_else(|| crate::errors::Error::Unexpected("Account not found".to_string()))
        }

        fn list_accounts(
            &self,
            _active_only: Option<bool>,
            _is_archived_filter: Option<bool>,
            _account_ids: Option<&[String]>,
        ) -> Result<Vec<Account>> {
            Ok(self.accounts.lock().unwrap().clone())
        }

        fn get_all_accounts(&self) -> Result<Vec<Account>> {
            Ok(self.accounts.lock().unwrap().clone())
        }

        fn get_active_accounts(&self) -> Result<Vec<Account>> {
            Ok(self.accounts.lock().unwrap().clone())
        }

        fn get_accounts_by_ids(&self, _account_ids: &[String]) -> Result<Vec<Account>> {
            unimplemented!()
        }

        fn get_non_archived_accounts(&self) -> Result<Vec<Account>> {
            Ok(self.accounts.lock().unwrap().clone())
        }

        fn get_active_non_archived_accounts(&self) -> Result<Vec<Account>> {
            Ok(self.accounts.lock().unwrap().clone())
        }

        fn get_base_currency(&self) -> Option<String> {
            Some("USD".to_string())
        }
    }

    // --- Mock AssetService ---
    #[derive(Clone)]
    struct MockAssetService {
        assets: Arc<Mutex<Vec<Asset>>>,
        get_asset_by_id_error: Arc<Mutex<Option<String>>>,
    }

    impl MockAssetService {
        fn new() -> Self {
            Self {
                assets: Arc::new(Mutex::new(Vec::new())),
                get_asset_by_id_error: Arc::new(Mutex::new(None)),
            }
        }

        fn add_asset(&self, asset: Asset) {
            self.assets.lock().unwrap().push(asset);
        }

        fn set_get_asset_by_id_error(&self, message: &str) {
            *self.get_asset_by_id_error.lock().unwrap() = Some(message.to_string());
        }
    }

    #[async_trait]
    impl AssetServiceTrait for MockAssetService {
        fn get_assets(&self) -> Result<Vec<Asset>> {
            Ok(self.assets.lock().unwrap().clone())
        }

        fn get_asset_by_id(&self, asset_id: &str) -> Result<Asset> {
            if let Some(message) = self.get_asset_by_id_error.lock().unwrap().clone() {
                return Err(Error::Unexpected(message));
            }
            let assets = self.assets.lock().unwrap();
            assets
                .iter()
                .find(|a| a.id == asset_id)
                .cloned()
                .ok_or_else(|| {
                    Error::Database(DatabaseError::NotFound(format!(
                        "Asset not found: {asset_id}",
                    )))
                })
        }

        async fn delete_asset(&self, _asset_id: &str) -> Result<()> {
            unimplemented!()
        }

        async fn update_asset_profile(
            &self,
            _asset_id: &str,
            _payload: UpdateAssetProfile,
        ) -> Result<Asset> {
            unimplemented!()
        }

        async fn update_quote_mode(&self, _asset_id: &str, _quote_mode: &str) -> Result<Asset> {
            // Return a dummy asset
            Ok(Asset::default())
        }

        async fn get_assets_by_asset_ids(&self, _asset_ids: &[String]) -> Result<Vec<Asset>> {
            unimplemented!()
        }

        async fn create_asset(&self, _new_asset: crate::assets::NewAsset) -> Result<Asset> {
            unimplemented!()
        }

        async fn get_or_create_minimal_asset(
            &self,
            asset_id: &str,
            context_currency: Option<String>,
            _metadata: Option<crate::assets::AssetMetadata>,
            _quote_mode: Option<String>,
        ) -> Result<Asset> {
            if let Ok(existing) = self.get_asset_by_id(asset_id) {
                return Ok(existing);
            }
            // Mirror the real service: a missing asset is created minimally.
            let created = create_test_asset(asset_id, context_currency.as_deref().unwrap_or("USD"));
            self.assets.lock().unwrap().push(created.clone());
            Ok(created)
        }

        async fn enrich_asset_profile(&self, _asset_id: &str) -> Result<Asset> {
            unimplemented!()
        }

        async fn enrich_assets(&self, _asset_ids: Vec<String>) -> Result<(usize, usize, usize)> {
            Ok((0, 0, 0))
        }

        async fn cleanup_legacy_metadata(&self, _asset_id: &str) -> Result<()> {
            Ok(())
        }

        async fn merge_unknown_asset(
            &self,
            _resolved_asset_id: &str,
            _unknown_asset_id: &str,
            _activity_repository: &dyn crate::activities::ActivityRepositoryTrait,
        ) -> Result<u32> {
            Ok(0)
        }

        async fn ensure_assets(
            &self,
            mut specs: Vec<crate::assets::AssetSpec>,
            _activity_repository: &dyn crate::activities::ActivityRepositoryTrait,
        ) -> Result<crate::assets::EnsureAssetsResult> {
            let mut result = crate::assets::EnsureAssetsResult::default();
            let mut assets = self.assets.lock().unwrap();

            let input_keys: Vec<Option<String>> = specs
                .iter()
                .map(|spec| spec.id.clone().or_else(|| spec.instrument_key()))
                .collect();
            let resolutions = crate::assets::resolve_bond_aliases(&specs, &assets);
            for (spec, resolution) in specs.iter_mut().zip(resolutions) {
                resolution.apply_to_spec(spec);
            }

            for (spec, input_key) in specs.into_iter().zip(input_keys) {
                if let Some(ref id) = spec.id {
                    // Look up existing assets by spec ID
                    if let Some(asset) = assets.iter().find(|a| a.id == *id) {
                        result.assets.insert(id.clone(), asset.clone());
                        if let Some(input_key) = input_key {
                            result.input_to_asset_id.insert(input_key, id.clone());
                        }
                    }
                    continue;
                }
                // Natural-identity spec: create the asset, mirroring the real
                // service, so symbol-only import rows resolve in tests.
                let instrument_key = spec.instrument_key();
                if let Some(existing) = assets
                    .iter()
                    .find(|a| a.instrument_key.is_some() && a.instrument_key == instrument_key)
                {
                    result.assets.insert(existing.id.clone(), existing.clone());
                    if let Some(input_key) = input_key {
                        result
                            .input_to_asset_id
                            .insert(input_key, existing.id.clone());
                    }
                    continue;
                }
                let symbol = spec
                    .display_code
                    .clone()
                    .or_else(|| spec.instrument_symbol.clone())
                    .unwrap_or_else(|| "CREATED".to_string());
                let created = Asset {
                    id: format!("created-{}", symbol),
                    display_code: Some(symbol.clone()),
                    instrument_symbol: spec.instrument_symbol.clone(),
                    quote_ccy: spec.quote_ccy.clone(),
                    kind: spec.kind,
                    instrument_type: spec.instrument_type,
                    instrument_key,
                    metadata: spec.metadata,
                    ..Default::default()
                };
                result.created_ids.push(created.id.clone());
                result.assets.insert(created.id.clone(), created.clone());
                if let Some(input_key) = input_key {
                    result
                        .input_to_asset_id
                        .insert(input_key, created.id.clone());
                }
                assets.push(created);
            }

            Ok(result)
        }
    }

    // --- Mock FxService ---
    #[derive(Clone, Default)]
    struct MockFxService {
        registered_pairs: Arc<Mutex<HashSet<(String, String)>>>,
    }

    impl MockFxService {
        fn new() -> Self {
            Self {
                registered_pairs: Arc::new(Mutex::new(HashSet::new())),
            }
        }

        fn get_registered_pairs(&self) -> HashSet<(String, String)> {
            self.registered_pairs.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl FxServiceTrait for MockFxService {
        fn initialize(&self) -> Result<()> {
            Ok(())
        }

        async fn add_exchange_rate(&self, _new_rate: NewExchangeRate) -> Result<ExchangeRate> {
            unimplemented!()
        }

        fn get_historical_rates(
            &self,
            _from_currency: &str,
            _to_currency: &str,
            _days: i64,
        ) -> Result<Vec<ExchangeRate>> {
            unimplemented!()
        }

        async fn update_exchange_rate(
            &self,
            _from_currency: &str,
            _to_currency: &str,
            _rate: Decimal,
        ) -> Result<ExchangeRate> {
            unimplemented!()
        }

        fn get_latest_exchange_rate(
            &self,
            _from_currency: &str,
            _to_currency: &str,
        ) -> Result<Decimal> {
            Ok(Decimal::ONE)
        }

        fn get_exchange_rate_for_date(
            &self,
            _from_currency: &str,
            _to_currency: &str,
            _date: NaiveDate,
        ) -> Result<Decimal> {
            Ok(Decimal::ONE)
        }

        fn convert_currency(
            &self,
            amount: Decimal,
            _from_currency: &str,
            _to_currency: &str,
        ) -> Result<Decimal> {
            Ok(amount)
        }

        fn convert_currency_for_date(
            &self,
            amount: Decimal,
            _from_currency: &str,
            _to_currency: &str,
            _date: NaiveDate,
        ) -> Result<Decimal> {
            Ok(amount)
        }

        fn get_latest_exchange_rates(&self) -> Result<Vec<ExchangeRate>> {
            unimplemented!()
        }

        async fn delete_exchange_rate(&self, _rate_id: &str) -> Result<()> {
            unimplemented!()
        }

        async fn register_currency_pair(
            &self,
            from_currency: &str,
            to_currency: &str,
        ) -> Result<()> {
            let mut pairs = self.registered_pairs.lock().unwrap();
            pairs.insert((from_currency.to_string(), to_currency.to_string()));
            Ok(())
        }

        async fn register_currency_pair_manual(
            &self,
            _from_currency: &str,
            _to_currency: &str,
        ) -> Result<()> {
            unimplemented!()
        }

        async fn ensure_fx_pairs(&self, pairs: Vec<(String, String)>) -> Result<()> {
            let mut registered = self.registered_pairs.lock().unwrap();
            for (from, to) in pairs {
                registered.insert((from, to));
            }
            Ok(())
        }
    }

    // --- Mock QuoteService ---
    #[derive(Clone, Default)]
    struct MockQuoteService;

    #[async_trait]
    impl QuoteServiceTrait for MockQuoteService {
        fn get_latest_quote(&self, _symbol: &str) -> Result<Quote> {
            unimplemented!()
        }

        fn get_latest_quotes(&self, _symbols: &[String]) -> Result<HashMap<String, Quote>> {
            unimplemented!()
        }

        fn get_latest_quotes_as_of(
            &self,
            _symbols: &[String],
            _as_of: chrono::NaiveDate,
        ) -> Result<HashMap<String, Quote>> {
            Ok(HashMap::new())
        }

        fn get_sparse_asset_market_facts(
            &self,
            _requests: &[(String, NaiveDate)],
        ) -> Result<crate::quotes::SparseAssetMarketFacts> {
            Ok(crate::quotes::SparseAssetMarketFacts::default())
        }

        fn get_latest_quotes_snapshot(
            &self,
            asset_ids: &[String],
        ) -> Result<HashMap<String, LatestQuoteSnapshot>> {
            let today = Utc::now().date_naive();
            let quotes = self.get_latest_quotes(asset_ids)?;
            Ok(quotes
                .into_iter()
                .map(|(asset_id, quote)| {
                    let quote_day = quote.timestamp.date_naive();
                    (
                        asset_id,
                        LatestQuoteSnapshot {
                            quote: Some(quote),
                            is_stale: quote_day < today,
                            effective_market_date: today.to_string(),
                            quote_date: Some(quote_day.to_string()),
                            no_quote_reason: None,
                        },
                    )
                })
                .collect())
        }

        fn get_latest_quotes_pair(
            &self,
            _symbols: &[String],
        ) -> Result<HashMap<String, LatestQuotePair>> {
            unimplemented!()
        }

        fn get_historical_quotes(&self, _symbol: &str) -> Result<Vec<Quote>> {
            unimplemented!()
        }

        fn get_all_historical_quotes(&self) -> Result<HashMap<String, Vec<(NaiveDate, Quote)>>> {
            unimplemented!()
        }

        fn get_quotes_in_range(
            &self,
            _symbols: &HashSet<String>,
            _start: NaiveDate,
            _end: NaiveDate,
        ) -> Result<Vec<Quote>> {
            unimplemented!()
        }

        fn get_sparse_quotes_in_range(
            &self,
            symbols: &HashSet<String>,
            start: NaiveDate,
            end: NaiveDate,
        ) -> Result<Vec<Quote>> {
            if !symbols.contains("PARITY_ASSET") {
                return Ok(Vec::new());
            }
            Ok([
                ("2026-06-01", dec!(100)),
                ("2026-06-03", dec!(102)),
                ("2026-06-05", dec!(52)),
            ]
            .into_iter()
            .filter_map(|(date, close)| {
                let quote_date = NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap();
                (quote_date >= start && quote_date <= end).then(|| Quote {
                    id: format!("PARITY_ASSET-{date}"),
                    asset_id: "PARITY_ASSET".to_string(),
                    timestamp: quote_date.and_hms_opt(12, 0, 0).unwrap().and_utc(),
                    open: close,
                    high: close,
                    low: close,
                    close,
                    adjclose: close,
                    volume: Decimal::ZERO,
                    currency: "USD".to_string(),
                    data_source: "TEST".to_string(),
                    created_at: quote_date.and_hms_opt(12, 0, 0).unwrap().and_utc(),
                    notes: None,
                })
            })
            .collect())
        }

        fn get_quotes_in_range_filled(
            &self,
            _symbols: &HashSet<String>,
            _start: NaiveDate,
            _end: NaiveDate,
        ) -> Result<Vec<Quote>> {
            unimplemented!()
        }

        async fn get_daily_quotes(
            &self,
            _asset_ids: &HashSet<String>,
            _start: NaiveDate,
            _end: NaiveDate,
        ) -> Result<HashMap<NaiveDate, HashMap<String, Quote>>> {
            unimplemented!()
        }

        async fn add_quote(&self, _quote: &Quote) -> Result<Quote> {
            unimplemented!()
        }

        async fn update_quote(&self, quote: Quote) -> Result<Quote> {
            Ok(quote)
        }

        async fn delete_quote(&self, _quote_id: &str) -> Result<()> {
            unimplemented!()
        }

        async fn bulk_upsert_quotes(&self, _quotes: Vec<Quote>) -> Result<usize> {
            unimplemented!()
        }

        async fn search_symbol(&self, _query: &str) -> Result<Vec<SymbolSearchResult>> {
            unimplemented!()
        }

        async fn search_symbol_with_currency(
            &self,
            query: &str,
            _account_currency: Option<&str>,
        ) -> Result<Vec<SymbolSearchResult>> {
            if query.eq_ignore_ascii_case("VWRPL") {
                return Ok(vec![SymbolSearchResult {
                    symbol: "VWRPL".to_string(),
                    short_name: "Vanguard FTSE All-World".to_string(),
                    long_name: "Vanguard FTSE All-World UCITS ETF".to_string(),
                    exchange: "LSE".to_string(),
                    exchange_mic: Some("XLON".to_string()),
                    exchange_name: Some("London Stock Exchange".to_string()),
                    quote_type: "EQUITY".to_string(),
                    type_display: "ETF".to_string(),
                    currency: Some("GBP".to_string()),
                    currency_source: Some("provider".to_string()),
                    data_source: Some("YAHOO".to_string()),
                    is_existing: false,
                    existing_asset_id: None,
                    index: String::new(),
                    score: 1.0,
                    ..Default::default()
                }]);
            }

            if query.eq_ignore_ascii_case("MSF.DE") {
                return Ok(vec![SymbolSearchResult {
                    symbol: "MSF".to_string(),
                    short_name: "Microsoft Corporation".to_string(),
                    long_name: "Microsoft Corporation".to_string(),
                    exchange: "NMS".to_string(),
                    exchange_mic: Some("XNAS".to_string()),
                    exchange_name: Some("NASDAQ".to_string()),
                    quote_type: "EQUITY".to_string(),
                    type_display: "Equity".to_string(),
                    currency: Some("USD".to_string()),
                    currency_source: Some("provider".to_string()),
                    data_source: Some("YAHOO".to_string()),
                    is_existing: false,
                    existing_asset_id: None,
                    index: String::new(),
                    score: 1.0,
                    ..Default::default()
                }]);
            }

            if query.eq_ignore_ascii_case("BRK.B") {
                return Ok(vec![SymbolSearchResult {
                    symbol: "BRK.B".to_string(),
                    short_name: "Berkshire Hathaway Inc.".to_string(),
                    long_name: "Berkshire Hathaway Inc.".to_string(),
                    exchange: "NYQ".to_string(),
                    exchange_mic: Some("XNYS".to_string()),
                    exchange_name: Some("NYSE".to_string()),
                    quote_type: "EQUITY".to_string(),
                    type_display: "Equity".to_string(),
                    currency: Some("USD".to_string()),
                    currency_source: Some("provider".to_string()),
                    data_source: Some("YAHOO".to_string()),
                    is_existing: false,
                    existing_asset_id: None,
                    index: String::new(),
                    score: 1.0,
                    ..Default::default()
                }]);
            }

            Ok(vec![])
        }

        async fn resolve_symbol_quote(
            &self,
            symbol: &str,
            exchange_mic: Option<&str>,
            _instrument_type: Option<&InstrumentType>,
            _quote_ccy: Option<&str>,
            _preferred_provider: Option<&str>,
            _provider_symbol: Option<&str>,
        ) -> Result<ResolvedQuote> {
            let is_uk_vwrp = (exchange_mic == Some("XLON") || exchange_mic == Some("BCXE"))
                && (symbol.eq_ignore_ascii_case("VWRPL")
                    || symbol.eq_ignore_ascii_case("VWRPL.XC"));
            if is_uk_vwrp {
                return Ok(ResolvedQuote {
                    currency: Some("GBP".to_string()),
                    price: Some(dec!(131.60)),
                    resolved_provider_id: Some("YAHOO".to_string()),
                });
            }

            Ok(ResolvedQuote::default())
        }

        async fn get_asset_profile(&self, _asset: &Asset) -> Result<ProviderProfile> {
            unimplemented!()
        }

        async fn fetch_quotes_from_provider(
            &self,
            _asset_id: &str,
            _start: NaiveDate,
            _end: NaiveDate,
        ) -> Result<Vec<Quote>> {
            unimplemented!()
        }

        async fn fetch_quotes_for_symbol(
            &self,
            _symbol: &str,
            _currency: &str,
            _start: NaiveDate,
            _end: NaiveDate,
        ) -> Result<Vec<Quote>> {
            unimplemented!()
        }

        async fn sync(
            &self,
            _mode: SyncMode,
            _asset_ids: Option<Vec<String>>,
        ) -> Result<SyncResult> {
            unimplemented!()
        }

        async fn resync(&self, _asset_ids: Option<Vec<String>>) -> Result<SyncResult> {
            unimplemented!()
        }

        async fn refresh_sync_state(&self) -> Result<()> {
            unimplemented!()
        }

        fn get_sync_plan(&self) -> Result<Vec<SymbolSyncPlan>> {
            unimplemented!()
        }

        async fn handle_activity_created(
            &self,
            _symbol: &str,
            _activity_date: NaiveDate,
        ) -> Result<()> {
            Ok(())
        }

        async fn handle_activity_deleted(&self, _symbol: &str) -> Result<()> {
            Ok(())
        }

        async fn delete_sync_state(&self, _symbol: &str) -> Result<()> {
            Ok(())
        }

        fn get_symbols_needing_sync(&self) -> Result<Vec<QuoteSyncState>> {
            Ok(vec![])
        }

        fn get_sync_state(&self, _symbol: &str) -> Result<Option<QuoteSyncState>> {
            Ok(None)
        }

        async fn mark_profile_enriched(&self, _symbol: &str) -> Result<()> {
            Ok(())
        }

        fn get_assets_needing_profile_enrichment(&self) -> Result<Vec<QuoteSyncState>> {
            Ok(vec![])
        }

        async fn update_position_status_from_holdings(
            &self,
            _current_holdings: &HashMap<String, Decimal>,
        ) -> Result<()> {
            Ok(())
        }

        fn get_sync_states_with_errors(&self) -> Result<Vec<QuoteSyncState>> {
            Ok(vec![])
        }

        async fn reset_sync_errors(&self, _asset_ids: &[String]) -> Result<()> {
            Ok(())
        }

        async fn reset_sync_state_for_profile_change(&self, _asset_id: &str) -> Result<()> {
            Ok(())
        }

        async fn get_providers_info(&self) -> Result<Vec<ProviderInfo>> {
            Ok(vec![])
        }

        async fn update_provider_settings(
            &self,
            _provider_id: &str,
            _priority: i32,
            _enabled: bool,
        ) -> Result<()> {
            Ok(())
        }
    }

    #[derive(Clone, Default)]
    struct RecordingQuoteService {
        updated_quotes: Arc<Mutex<Vec<Quote>>>,
    }

    impl RecordingQuoteService {
        fn updated_quotes(&self) -> Vec<Quote> {
            self.updated_quotes.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl QuoteServiceTrait for RecordingQuoteService {
        fn get_latest_quote(&self, _symbol: &str) -> Result<Quote> {
            unimplemented!()
        }

        fn get_latest_quotes(&self, _symbols: &[String]) -> Result<HashMap<String, Quote>> {
            unimplemented!()
        }

        fn get_latest_quotes_as_of(
            &self,
            _symbols: &[String],
            _as_of: chrono::NaiveDate,
        ) -> Result<HashMap<String, Quote>> {
            Ok(HashMap::new())
        }

        fn get_sparse_asset_market_facts(
            &self,
            _requests: &[(String, NaiveDate)],
        ) -> Result<crate::quotes::SparseAssetMarketFacts> {
            Err(Error::Unexpected(
                "RecordingQuoteService::get_sparse_asset_market_facts should not be called"
                    .to_string(),
            ))
        }

        fn get_latest_quotes_snapshot(
            &self,
            _asset_ids: &[String],
        ) -> Result<HashMap<String, LatestQuoteSnapshot>> {
            Ok(HashMap::new())
        }

        fn get_latest_quotes_pair(
            &self,
            _symbols: &[String],
        ) -> Result<HashMap<String, LatestQuotePair>> {
            unimplemented!()
        }

        fn get_historical_quotes(&self, _symbol: &str) -> Result<Vec<Quote>> {
            unimplemented!()
        }

        fn get_all_historical_quotes(&self) -> Result<HashMap<String, Vec<(NaiveDate, Quote)>>> {
            unimplemented!()
        }

        fn get_quotes_in_range(
            &self,
            _symbols: &HashSet<String>,
            _start: NaiveDate,
            _end: NaiveDate,
        ) -> Result<Vec<Quote>> {
            unimplemented!()
        }

        fn get_sparse_quotes_in_range(
            &self,
            symbols: &HashSet<String>,
            _start: NaiveDate,
            end: NaiveDate,
        ) -> Result<Vec<Quote>> {
            // Include earlier quotes for carry-forward, with the latest update
            // winning for each asset/date as it does in the quote store.
            let mut quotes = std::collections::BTreeMap::new();
            for quote in self.updated_quotes.lock().unwrap().iter() {
                let date = quote.timestamp.date_naive();
                if symbols.contains(&quote.asset_id) && date <= end {
                    quotes.insert((quote.asset_id.clone(), date), quote.clone());
                }
            }
            Ok(quotes.into_values().collect())
        }

        fn get_quotes_in_range_filled(
            &self,
            _symbols: &HashSet<String>,
            _start: NaiveDate,
            _end: NaiveDate,
        ) -> Result<Vec<Quote>> {
            unimplemented!()
        }

        async fn get_daily_quotes(
            &self,
            _asset_ids: &HashSet<String>,
            _start: NaiveDate,
            _end: NaiveDate,
        ) -> Result<HashMap<NaiveDate, HashMap<String, Quote>>> {
            unimplemented!()
        }

        async fn add_quote(&self, _quote: &Quote) -> Result<Quote> {
            unimplemented!()
        }

        async fn update_quote(&self, quote: Quote) -> Result<Quote> {
            self.updated_quotes.lock().unwrap().push(quote.clone());
            Ok(quote)
        }

        async fn delete_quote(&self, _quote_id: &str) -> Result<()> {
            unimplemented!()
        }

        async fn bulk_upsert_quotes(&self, _quotes: Vec<Quote>) -> Result<usize> {
            unimplemented!()
        }

        async fn search_symbol(&self, _query: &str) -> Result<Vec<SymbolSearchResult>> {
            unimplemented!()
        }

        async fn search_symbol_with_currency(
            &self,
            _query: &str,
            _account_currency: Option<&str>,
        ) -> Result<Vec<SymbolSearchResult>> {
            unimplemented!()
        }

        async fn resolve_symbol_quote(
            &self,
            _symbol: &str,
            _exchange_mic: Option<&str>,
            _instrument_type: Option<&InstrumentType>,
            _quote_ccy: Option<&str>,
            _preferred_provider: Option<&str>,
            _provider_symbol: Option<&str>,
        ) -> Result<ResolvedQuote> {
            unimplemented!()
        }

        async fn get_asset_profile(&self, _asset: &Asset) -> Result<ProviderProfile> {
            unimplemented!()
        }

        async fn fetch_quotes_from_provider(
            &self,
            _asset_id: &str,
            _start: NaiveDate,
            _end: NaiveDate,
        ) -> Result<Vec<Quote>> {
            unimplemented!()
        }

        async fn fetch_quotes_for_symbol(
            &self,
            _symbol: &str,
            _currency: &str,
            _start: NaiveDate,
            _end: NaiveDate,
        ) -> Result<Vec<Quote>> {
            unimplemented!()
        }

        async fn sync(
            &self,
            _mode: SyncMode,
            _asset_ids: Option<Vec<String>>,
        ) -> Result<SyncResult> {
            unimplemented!()
        }

        async fn resync(&self, _asset_ids: Option<Vec<String>>) -> Result<SyncResult> {
            unimplemented!()
        }

        async fn refresh_sync_state(&self) -> Result<()> {
            unimplemented!()
        }

        fn get_sync_plan(&self) -> Result<Vec<SymbolSyncPlan>> {
            unimplemented!()
        }

        async fn handle_activity_created(
            &self,
            _symbol: &str,
            _activity_date: NaiveDate,
        ) -> Result<()> {
            Ok(())
        }

        async fn handle_activity_deleted(&self, _symbol: &str) -> Result<()> {
            Ok(())
        }

        async fn delete_sync_state(&self, _symbol: &str) -> Result<()> {
            Ok(())
        }

        fn get_symbols_needing_sync(&self) -> Result<Vec<QuoteSyncState>> {
            Ok(vec![])
        }

        fn get_sync_state(&self, _symbol: &str) -> Result<Option<QuoteSyncState>> {
            Ok(None)
        }

        async fn mark_profile_enriched(&self, _symbol: &str) -> Result<()> {
            Ok(())
        }

        fn get_assets_needing_profile_enrichment(&self) -> Result<Vec<QuoteSyncState>> {
            Ok(vec![])
        }

        async fn update_position_status_from_holdings(
            &self,
            _current_holdings: &HashMap<String, Decimal>,
        ) -> Result<()> {
            Ok(())
        }

        fn get_sync_states_with_errors(&self) -> Result<Vec<QuoteSyncState>> {
            Ok(vec![])
        }

        async fn reset_sync_errors(&self, _asset_ids: &[String]) -> Result<()> {
            Ok(())
        }

        async fn reset_sync_state_for_profile_change(&self, _asset_id: &str) -> Result<()> {
            Ok(())
        }

        async fn get_providers_info(&self) -> Result<Vec<ProviderInfo>> {
            Ok(vec![])
        }

        async fn update_provider_settings(
            &self,
            _provider_id: &str,
            _priority: i32,
            _enabled: bool,
        ) -> Result<()> {
            Ok(())
        }
    }

    // --- Mock ActivityRepository ---
    #[derive(Clone, Default)]
    pub(crate) struct MockActivityRepository {
        activities: Arc<Mutex<Vec<Activity>>>,
    }

    impl MockActivityRepository {
        fn new() -> Self {
            Self {
                activities: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn add_activity(&self, activity: Activity) {
            self.activities.lock().unwrap().push(activity);
        }
    }

    fn parse_test_activity_datetime(activity_date: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(activity_date)
            .map(|dt| dt.with_timezone(&Utc))
            .or_else(|_| {
                NaiveDate::parse_from_str(activity_date, "%Y-%m-%d")
                    .map(|date| date.and_hms_opt(0, 0, 0).unwrap().and_utc())
            })
            .unwrap_or_else(|_| Utc::now())
    }

    #[async_trait]
    impl ActivityRepositoryTrait for MockActivityRepository {
        fn get_activity(&self, activity_id: &str) -> Result<Activity> {
            let activities = self.activities.lock().unwrap();
            activities
                .iter()
                .find(|a| a.id == activity_id)
                .cloned()
                .ok_or_else(|| Error::Unexpected("Activity not found".to_string()))
        }

        fn get_activities(&self) -> Result<Vec<Activity>> {
            Ok(self.activities.lock().unwrap().clone())
        }

        fn get_activities_including_archived_accounts(&self) -> Result<Vec<Activity>> {
            Ok(self.activities.lock().unwrap().clone())
        }

        fn get_activities_by_account_id(&self, _account_id: &str) -> Result<Vec<Activity>> {
            unimplemented!()
        }

        fn get_activities_by_account_ids(&self, _account_ids: &[String]) -> Result<Vec<Activity>> {
            unimplemented!()
        }

        fn get_trading_activities(&self) -> Result<Vec<Activity>> {
            unimplemented!()
        }

        fn get_income_activities(&self) -> Result<Vec<Activity>> {
            unimplemented!()
        }

        fn search_activities(
            &self,
            _page: i64,
            _page_size: i64,
            _account_id_filter: Option<Vec<String>>,
            _activity_type_filter: Option<Vec<String>>,
            _asset_id_keyword: Option<String>,
            _sort: Option<Sort>,
            _is_draft_filter: Option<bool>,
            _date_from: Option<chrono::NaiveDate>,
            _date_to: Option<chrono::NaiveDate>,
            _instrument_type_filter: Option<Vec<String>>,
            _activity_id_filter: Option<Vec<String>>,
        ) -> Result<ActivitySearchResponse> {
            unimplemented!()
        }

        async fn create_activity(&self, new_activity: NewActivity) -> Result<Activity> {
            use crate::activities::ActivityStatus;
            // Extract asset_id before consuming other fields
            let asset_id = new_activity.get_symbol_id().map(|s| s.to_string());
            let metadata = new_activity
                .metadata
                .as_deref()
                .and_then(|metadata| serde_json::from_str(metadata).ok());
            let activity_date = parse_test_activity_datetime(&new_activity.activity_date);
            let generated_id = new_activity.id.unwrap_or_else(|| {
                format!("test-id-{}", self.activities.lock().unwrap().len() + 1)
            });
            let activity = Activity {
                id: generated_id,
                account_id: new_activity.account_id,
                asset_id,
                activity_type: new_activity.activity_type,
                activity_type_override: None,
                source_type: None,
                subtype: new_activity.subtype,
                status: new_activity.status.unwrap_or(ActivityStatus::Posted),
                activity_date,
                settlement_date: None,
                quantity: new_activity.quantity,
                unit_price: new_activity.unit_price,
                amount: new_activity.amount,
                fee: new_activity.fee,
                tax: new_activity.tax,
                currency: new_activity.currency,
                fx_rate: new_activity.fx_rate,
                notes: new_activity.notes,
                metadata,
                source_system: None,
                source_record_id: None,
                source_group_id: new_activity.source_group_id,
                idempotency_key: new_activity.idempotency_key,
                import_run_id: None,
                is_user_modified: false,
                // Mirrors the storage mapping: absent means not flagged.
                needs_review: new_activity.needs_review.unwrap_or(false),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };
            self.activities.lock().unwrap().push(activity.clone());
            Ok(activity)
        }

        async fn update_activity(&self, activity_update: ActivityUpdate) -> Result<Activity> {
            let mut activities = self.activities.lock().unwrap();
            let existing = activities
                .iter_mut()
                .find(|activity| activity.id == activity_update.id)
                .ok_or_else(|| Error::Unexpected("Activity not found".to_string()))?;
            let asset_id_patch = activity_update.asset.as_ref().map(|asset| asset.id.clone());

            existing.account_id = activity_update.account_id;
            if let Some(asset_id) = asset_id_patch {
                existing.asset_id = asset_id;
            }
            existing.activity_type = activity_update.activity_type;
            existing.activity_date = parse_test_activity_datetime(&activity_update.activity_date);
            existing.subtype = match activity_update.subtype {
                Some(subtype) if subtype.trim().is_empty() => None,
                Some(subtype) => Some(subtype),
                None => existing.subtype.clone(),
            };
            existing.activity_date = parse_test_activity_datetime(&activity_update.activity_date);
            existing.quantity = activity_update.quantity.unwrap_or(existing.quantity);
            existing.unit_price = activity_update.unit_price.unwrap_or(existing.unit_price);
            existing.amount = activity_update.amount.unwrap_or(existing.amount);
            existing.fee = activity_update.fee.unwrap_or(existing.fee);
            existing.tax = activity_update.tax.unwrap_or(existing.tax);
            existing.status = activity_update.status.unwrap_or(existing.status.clone());
            existing.needs_review = activity_update
                .needs_review
                .unwrap_or(existing.needs_review);
            existing.currency = activity_update.currency;
            existing.fx_rate = activity_update.fx_rate.unwrap_or(existing.fx_rate);
            existing.notes = activity_update.notes;
            // Mirror the storage repository: a patch without metadata keeps
            // the stored blob; a supplied blob replaces it.
            existing.metadata = match activity_update.metadata {
                Some(raw) => serde_json::from_str(&raw).ok(),
                None => existing.metadata.take(),
            };
            existing.updated_at = Utc::now();

            Ok(existing.clone())
        }

        async fn delete_activity(&self, activity_id: String) -> Result<Activity> {
            let mut activities = self.activities.lock().unwrap();
            let index = activities
                .iter()
                .position(|activity| activity.id == activity_id)
                .ok_or_else(|| Error::Unexpected("Activity not found".to_string()))?;
            Ok(activities.remove(index))
        }

        async fn link_transfer_activities(
            &self,
            _activity_a_id: String,
            _activity_b_id: String,
        ) -> Result<(Activity, Activity)> {
            unimplemented!()
        }

        async fn unlink_transfer_activities(
            &self,
            activity_a_id: String,
            activity_b_id: String,
        ) -> Result<(Activity, Activity)> {
            let mut activities = self.activities.lock().unwrap();
            let first_index = activities
                .iter()
                .position(|activity| activity.id == activity_a_id)
                .ok_or_else(|| {
                    crate::errors::Error::Unexpected("first activity not found".to_string())
                })?;
            let second_index = activities
                .iter()
                .position(|activity| activity.id == activity_b_id)
                .ok_or_else(|| {
                    crate::errors::Error::Unexpected("second activity not found".to_string())
                })?;

            let first = activities[first_index].clone();
            let second = activities[second_index].clone();
            let (transfer_in_index, transfer_out_index) =
                match (first.activity_type.as_str(), second.activity_type.as_str()) {
                    ("TRANSFER_IN", "TRANSFER_OUT") => (first_index, second_index),
                    ("TRANSFER_OUT", "TRANSFER_IN") => (second_index, first_index),
                    _ => {
                        return Err(crate::errors::Error::Unexpected(
                            "unlink requires transfer pair".to_string(),
                        ));
                    }
                };

            let transfer_in_group = activities[transfer_in_index].source_group_id.clone();
            let transfer_out_group = activities[transfer_out_index].source_group_id.clone();
            if transfer_in_group.is_none() || transfer_in_group != transfer_out_group {
                return Err(crate::errors::Error::Unexpected(
                    "transfer pair is not linked".to_string(),
                ));
            }

            let mut transfer_in = activities[transfer_in_index].clone();
            let mut transfer_out = activities[transfer_out_index].clone();
            transfer_in.source_group_id = None;
            transfer_in.metadata = Some(json!({ "flow": { "is_external": true } }));
            transfer_out.source_group_id = None;
            transfer_out.metadata = Some(json!({ "flow": { "is_external": true } }));
            transfer_in.updated_at = Utc::now();
            transfer_out.updated_at = Utc::now();

            activities[transfer_in_index] = transfer_in.clone();
            activities[transfer_out_index] = transfer_out.clone();

            Ok((transfer_in, transfer_out))
        }

        async fn bulk_mutate_activities(
            &self,
            creates: Vec<NewActivity>,
            updates: Vec<ActivityUpdate>,
            delete_ids: Vec<String>,
        ) -> Result<ActivityBulkMutationResult> {
            let mut deleted = Vec::new();
            for delete_id in delete_ids {
                deleted.push(self.delete_activity(delete_id).await?);
            }

            let mut created = Vec::new();
            for new_activity in creates {
                let activity = self.create_activity(new_activity).await?;
                created.push(activity);
            }
            let mut updated = Vec::new();
            for update in updates {
                let activity = self.update_activity(update).await?;
                updated.push(activity);
            }
            Ok(ActivityBulkMutationResult {
                created,
                updated,
                deleted,
                created_mappings: Vec::new(),
                errors: Vec::new(),
            })
        }

        async fn create_activities(&self, _activities: Vec<NewActivity>) -> Result<usize> {
            let mut stored = self.activities.lock().unwrap();
            let mut count = 0usize;
            for new_activity in _activities {
                let asset_id = new_activity.get_symbol_id().map(|s| s.to_string());
                let metadata = new_activity
                    .metadata
                    .as_deref()
                    .and_then(|metadata| serde_json::from_str(metadata).ok());
                let activity_date = parse_test_activity_datetime(&new_activity.activity_date);
                stored.push(Activity {
                    id: new_activity.id.unwrap_or_else(|| "test-id".to_string()),
                    account_id: new_activity.account_id,
                    asset_id,
                    activity_type: new_activity.activity_type,
                    activity_type_override: None,
                    source_type: None,
                    subtype: None,
                    status: new_activity
                        .status
                        .unwrap_or(crate::activities::ActivityStatus::Posted),
                    activity_date,
                    settlement_date: None,
                    quantity: new_activity.quantity,
                    unit_price: new_activity.unit_price,
                    amount: new_activity.amount,
                    fee: new_activity.fee,
                    tax: new_activity.tax,
                    currency: new_activity.currency,
                    fx_rate: new_activity.fx_rate,
                    notes: new_activity.notes,
                    metadata,
                    source_system: None,
                    source_record_id: None,
                    source_group_id: new_activity.source_group_id,
                    idempotency_key: new_activity.idempotency_key,
                    import_run_id: None,
                    is_user_modified: false,
                    needs_review: new_activity.needs_review.unwrap_or(false),
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                });
                count += 1;
            }
            Ok(count)
        }

        fn get_first_activity_date(
            &self,
            _account_ids: Option<&[String]>,
        ) -> Result<Option<DateTime<Utc>>> {
            unimplemented!()
        }

        fn get_income_activities_data(
            &self,
            _account_ids: Option<&[String]>,
        ) -> Result<Vec<IncomeData>> {
            unimplemented!()
        }

        fn get_first_activity_date_overall(&self) -> Result<DateTime<Utc>> {
            unimplemented!()
        }

        fn get_activity_bounds_for_assets(
            &self,
            _asset_ids: &[String],
        ) -> Result<
            std::collections::HashMap<
                String,
                (Option<chrono::NaiveDate>, Option<chrono::NaiveDate>),
            >,
        > {
            Ok(std::collections::HashMap::new())
        }

        fn get_holdings_snapshot_bounds_for_assets(
            &self,
            _asset_ids: &[String],
        ) -> Result<
            std::collections::HashMap<
                String,
                (Option<chrono::NaiveDate>, Option<chrono::NaiveDate>),
            >,
        > {
            Ok(std::collections::HashMap::new())
        }

        fn check_existing_duplicates(
            &self,
            idempotency_keys: &[String],
        ) -> Result<std::collections::HashMap<String, String>> {
            let stored = self.activities.lock().unwrap();
            let mut map = std::collections::HashMap::new();
            for requested_key in idempotency_keys {
                if let Some(existing) = stored
                    .iter()
                    .find(|a| a.idempotency_key.as_deref() == Some(requested_key.as_str()))
                {
                    map.insert(requested_key.clone(), existing.id.clone());
                }
            }
            Ok(map)
        }
        async fn bulk_upsert(
            &self,
            _activities: Vec<crate::activities::ActivityUpsert>,
        ) -> Result<crate::activities::BulkUpsertResult> {
            unimplemented!()
        }

        async fn reassign_asset(&self, _old_asset_id: &str, _new_asset_id: &str) -> Result<u32> {
            Ok(0)
        }

        async fn get_activity_accounts_and_currencies_by_asset_id(
            &self,
            _asset_id: &str,
        ) -> Result<(Vec<String>, Vec<String>)> {
            Ok((Vec::new(), Vec::new()))
        }
    }
    // Helper to create a test account
    fn create_test_account(id: &str, currency: &str) -> Account {
        Account {
            id: id.to_string(),
            name: format!("Test Account {}", id),
            account_type: "SECURITIES".to_string(),
            currency: currency.to_string(),
            is_default: false,
            is_active: true,
            created_at: Utc::now().naive_utc(),
            updated_at: Utc::now().naive_utc(),
            platform_id: None,
            group: None,
            account_number: None,
            meta: None,
            provider: None,
            provider_account_id: None,
            is_archived: false,
            tracking_mode: crate::accounts::TrackingMode::NotSet,
        }
    }

    // Helper to create a test asset
    fn create_test_asset(id: &str, currency: &str) -> Asset {
        Asset {
            id: id.to_string(),
            display_code: Some(id.to_string()),
            quote_ccy: currency.to_string(),
            kind: AssetKind::Investment,
            ..Default::default()
        }
    }

    /// Create a test asset with proper instrument fields for matching
    fn create_test_asset_with_instrument(
        id: &str,
        symbol: &str,
        exchange_mic: Option<&str>,
        instrument_type: Option<InstrumentType>,
        currency: &str,
    ) -> Asset {
        Asset {
            id: id.to_string(),
            display_code: Some(symbol.to_string()),
            instrument_symbol: Some(symbol.to_string()),
            instrument_exchange_mic: exchange_mic.map(|s| s.to_string()),
            instrument_type,
            quote_ccy: currency.to_string(),
            kind: AssetKind::Investment,
            ..Default::default()
        }
    }

    fn create_stored_activity(id: &str, account_id: &str, asset_id: Option<&str>) -> Activity {
        Activity {
            id: id.to_string(),
            account_id: account_id.to_string(),
            asset_id: asset_id.map(|s| s.to_string()),
            activity_type: "BUY".to_string(),
            activity_type_override: None,
            source_type: None,
            subtype: None,
            status: ActivityStatus::Posted,
            activity_date: Utc::now(),
            settlement_date: None,
            quantity: Some(dec!(1)),
            unit_price: Some(dec!(100)),
            amount: Some(dec!(100)),
            fee: Some(dec!(0)),
            tax: None,
            currency: "USD".to_string(),
            fx_rate: None,
            notes: None,
            metadata: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
            import_run_id: None,
            is_user_modified: false,
            needs_review: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    struct TransferActivitySeed<'a> {
        id: &'a str,
        account_id: &'a str,
        activity_type: &'a str,
        date: &'a str,
        amount: Option<Decimal>,
        currency: &'a str,
        asset_id: Option<&'a str>,
        quantity: Option<Decimal>,
        unit_price: Option<Decimal>,
    }

    fn create_transfer_activity(seed: TransferActivitySeed<'_>) -> Activity {
        Activity {
            id: seed.id.to_string(),
            account_id: seed.account_id.to_string(),
            asset_id: seed.asset_id.map(str::to_string),
            activity_type: seed.activity_type.to_string(),
            activity_type_override: None,
            source_type: None,
            subtype: None,
            status: ActivityStatus::Posted,
            activity_date: parse_test_activity_datetime(seed.date),
            settlement_date: None,
            quantity: seed.quantity,
            unit_price: seed.unit_price,
            amount: seed.amount,
            fee: Some(dec!(0)),
            tax: None,
            currency: seed.currency.to_string(),
            fx_rate: None,
            notes: None,
            metadata: Some(json!({ "flow": { "is_external": true } })),
            source_system: Some("MANUAL".to_string()),
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
            import_run_id: None,
            is_user_modified: false,
            needs_review: false,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn create_cash_transfer_activity(
        id: &str,
        account_id: &str,
        activity_type: &str,
        date: &str,
        amount: Decimal,
        currency: &str,
    ) -> Activity {
        create_transfer_activity(TransferActivitySeed {
            id,
            account_id,
            activity_type,
            date,
            amount: Some(amount),
            currency,
            asset_id: None,
            quantity: None,
            unit_price: None,
        })
    }

    fn create_security_transfer_activity(
        id: &str,
        account_id: &str,
        activity_type: &str,
        date: &str,
        asset_id: &str,
        quantity: Decimal,
        unit_price: Decimal,
    ) -> Activity {
        create_transfer_activity(TransferActivitySeed {
            id,
            account_id,
            activity_type,
            date,
            amount: None,
            currency: "USD",
            asset_id: Some(asset_id),
            quantity: Some(quantity),
            unit_price: Some(unit_price),
        })
    }

    fn create_test_activity_update(
        id: &str,
        account_id: &str,
        asset: Option<AssetResolutionInput>,
        currency: &str,
    ) -> ActivityUpdate {
        ActivityUpdate {
            id: id.to_string(),
            account_id: account_id.to_string(),
            asset,
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: Some(Some(dec!(1))),
            unit_price: Some(Some(dec!(100))),
            currency: currency.to_string(),
            fee: Some(Some(dec!(0))),
            tax: None,
            amount: Some(Some(dec!(100))),
            status: None,
            needs_review: None,
            notes: None,
            fx_rate: None,
            metadata: None,
        }
    }

    #[tokio::test]
    async fn test_create_security_transfer_preserves_preexisting_amount_behavior() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "USD"));
        let mut asset = create_test_asset_with_instrument(
            "asset-aapl",
            "AAPL",
            Some("XNAS"),
            Some(InstrumentType::Equity),
            "USD",
        );
        asset.quote_mode = QuoteMode::Manual;
        asset_service.add_asset(asset);

        let quote_service = Arc::new(RecordingQuoteService::default());
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            quote_service.clone(),
        );

        let created = activity_service
            .create_activity(NewActivity {
                id: Some("transfer-1".to_string()),
                account_id: "acc-1".to_string(),
                asset: Some(AssetResolutionInput {
                    id: Some("asset-aapl".to_string()),
                    ..Default::default()
                }),
                activity_type: "TRANSFER_IN".to_string(),
                subtype: None,
                activity_date: "2024-01-15".to_string(),
                quantity: Some(dec!(10)),
                unit_price: None,
                currency: "USD".to_string(),
                fee: Some(dec!(0)),
                tax: None,
                amount: Some(dec!(80)),
                status: None,
                notes: None,
                fx_rate: None,
                metadata: None,
                needs_review: None,
                source_system: None,
                source_record_id: None,
                source_group_id: None,
                idempotency_key: None,
            })
            .await
            .expect("security transfer should be created");

        assert_eq!(created.amount, Some(dec!(80)));
        assert_eq!(created.unit_price, None);
        assert!(
            quote_service.updated_quotes().is_empty(),
            "transfer unit_price is book basis and must not be written as a quote"
        );
    }

    #[tokio::test]
    async fn test_update_recalculates_trade_in_activity_currency_when_quote_currency_differs() {
        // The patch declares CAD, so its quantity and unit price recalculate a
        // CAD final. The asset's USD quote currency is valuation metadata.
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-usd", "USD"));
        account_service.add_account(create_test_account("acc-cad", "CAD"));
        asset_service.add_asset(create_test_asset_with_instrument(
            "asset-aapl",
            "AAPL",
            Some("XNAS"),
            Some(InstrumentType::Equity),
            "USD",
        ));

        let mut existing = create_stored_activity("activity-1", "acc-usd", Some("asset-aapl"));
        existing.amount = Some(dec!(100));
        existing.quantity = Some(dec!(1));
        existing.unit_price = Some(dec!(100));
        existing.currency = "USD".to_string();
        activity_repository
            .activities
            .lock()
            .unwrap()
            .push(existing);

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let updated = activity_service
            .update_activity(ActivityUpdate {
                id: "activity-1".to_string(),
                account_id: "acc-cad".to_string(),
                asset: Some(AssetResolutionInput {
                    id: Some("asset-aapl".to_string()),
                    ..Default::default()
                }),
                activity_type: "BUY".to_string(),
                subtype: None,
                activity_date: "2024-01-15".to_string(),
                quantity: Some(Some(dec!(2))),
                unit_price: Some(Some(dec!(70))),
                currency: "CAD".to_string(),
                fee: Some(Some(dec!(0))),
                tax: None,
                amount: None,
                status: None,
                needs_review: None,
                notes: None,
                fx_rate: None,
                metadata: None,
            })
            .await
            .expect("update should succeed");

        assert_eq!(updated.account_id, "acc-cad");
        assert_eq!(updated.amount, Some(dec!(140)));
        assert!(!updated.needs_review);
        assert_eq!(updated.quantity, Some(dec!(2)));
        assert_eq!(updated.unit_price, Some(dec!(70)));
    }

    #[tokio::test]
    async fn test_update_recalculates_trade_when_quote_currency_matches() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-usd", "USD"));
        asset_service.add_asset(create_test_asset_with_instrument(
            "asset-aapl",
            "AAPL",
            Some("XNAS"),
            Some(InstrumentType::Equity),
            "USD",
        ));
        let mut existing = create_stored_activity("activity-recalc", "acc-usd", Some("asset-aapl"));
        existing.amount = Some(dec!(100));
        existing.currency = "USD".to_string();
        activity_repository.add_activity(existing);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let updated = activity_service
            .update_activity(ActivityUpdate {
                id: "activity-recalc".to_string(),
                account_id: "acc-usd".to_string(),
                asset: Some(AssetResolutionInput {
                    id: Some("asset-aapl".to_string()),
                    ..Default::default()
                }),
                activity_type: "BUY".to_string(),
                subtype: None,
                activity_date: "2024-01-15".to_string(),
                quantity: Some(Some(dec!(2))),
                unit_price: Some(Some(dec!(70))),
                currency: "USD".to_string(),
                fee: Some(Some(dec!(0))),
                tax: None,
                amount: None,
                status: None,
                needs_review: None,
                notes: None,
                fx_rate: None,
                metadata: None,
            })
            .await
            .expect("update should succeed");

        assert_eq!(updated.amount, Some(dec!(140)));
        assert_eq!(updated.quantity, Some(dec!(2)));
        assert_eq!(updated.unit_price, Some(dec!(70)));
    }

    #[tokio::test]
    async fn test_update_cross_quote_bond_uses_cash_multiplier_when_amount_is_omitted() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-cad", "CAD"));
        // Percent-of-par pricing is opt-in via explicit metadata; a default
        // bond stays at multiplier 1 (provider quotes are fraction-of-par).
        let mut bond = create_test_asset_with_instrument(
            "asset-bond",
            "US912828ZT58",
            None,
            Some(InstrumentType::Bond),
            "USD",
        );
        bond.metadata = Some(serde_json::json!({ "contractMultiplier": "0.01" }));
        asset_service.add_asset(bond);

        let mut existing = create_stored_activity("activity-1", "acc-cad", Some("asset-bond"));
        existing.amount = Some(dec!(990));
        existing.quantity = Some(dec!(1000));
        existing.unit_price = Some(dec!(99));
        existing.currency = "CAD".to_string();
        activity_repository
            .activities
            .lock()
            .unwrap()
            .push(existing);

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let updated = activity_service
            .update_activity(ActivityUpdate {
                id: "activity-1".to_string(),
                account_id: "acc-cad".to_string(),
                asset: Some(AssetResolutionInput {
                    id: Some("asset-bond".to_string()),
                    ..Default::default()
                }),
                activity_type: "BUY".to_string(),
                subtype: None,
                activity_date: "2024-01-15".to_string(),
                quantity: Some(Some(dec!(1000))),
                unit_price: Some(Some(dec!(98))),
                currency: "CAD".to_string(),
                fee: Some(Some(dec!(0))),
                tax: None,
                amount: None,
                status: None,
                needs_review: None,
                notes: None,
                fx_rate: None,
                metadata: None,
            })
            .await
            .expect("update should succeed");

        assert_eq!(updated.amount, Some(dec!(980)));
        assert_eq!(updated.quantity, Some(dec!(1000)));
        assert_eq!(updated.unit_price, Some(dec!(98)));
    }

    #[tokio::test]
    async fn test_update_uses_the_asset_multiplier_not_activity_metadata() {
        // A row carrying `contract_multiplier: 10` against a standard
        // 100-multiplier option asset: the asset wins. The metadata is a
        // creation-time seed, and this asset already exists.
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-usd", "USD"));
        asset_service.add_asset(create_test_asset_with_instrument(
            "asset-opt",
            "XSP240119C00500000",
            Some("XCBO"),
            Some(InstrumentType::Option),
            "USD",
        ));
        let mut existing = create_stored_activity("activity-mini", "acc-usd", Some("asset-opt"));
        existing.quantity = Some(dec!(1));
        existing.unit_price = Some(dec!(100));
        existing.amount = Some(dec!(1000)); // 1 x 100 x 10
        existing.metadata = Some(serde_json::json!({ "contract_multiplier": 10 }));
        activity_repository.add_activity(existing);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let updated = activity_service
            .update_activity(ActivityUpdate {
                id: "activity-mini".to_string(),
                account_id: "acc-usd".to_string(),
                asset: Some(AssetResolutionInput {
                    id: Some("asset-opt".to_string()),
                    ..Default::default()
                }),
                activity_type: "BUY".to_string(),
                subtype: None,
                activity_date: "2024-01-15".to_string(),
                quantity: Some(Some(dec!(2))),
                unit_price: Some(Some(dec!(7))),
                currency: "USD".to_string(),
                fee: Some(Some(dec!(0))),
                tax: None,
                amount: None,
                status: None,
                needs_review: None,
                notes: None,
                fx_rate: None,
                metadata: None,
            })
            .await
            .expect("update should succeed");

        // The ASSET owns the multiplier: 2 x 7 x 100, not 2 x 7 x 10. A row
        // whose metadata disagrees with its asset is a misconfigured asset,
        // not a per-row override.
        assert_eq!(updated.amount, Some(dec!(1400)));
    }

    #[tokio::test]
    async fn test_create_cross_quote_mini_option_uses_the_assets_multiplier() {
        // The regression this ownership model exists for: a mini-option ASSET
        // carries multiplier 10, and a later trade entered without touching
        // the form's multiplier field must still price at 10 - never at the
        // form's 100 default. Nothing the row carries can outrank the asset.
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-cad", "CAD"));
        let mut mini = create_test_asset_with_instrument(
            "asset-mini",
            "XSP240119C00500000",
            Some("XCBO"),
            Some(InstrumentType::Option),
            "USD",
        );
        mini.metadata = Some(serde_json::json!({
            crate::assets::CONTRACT_MULTIPLIER_METADATA_KEY: 10
        }));
        asset_service.add_asset(mini);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let created = activity_service
            .create_activity(NewActivity {
                id: None,
                account_id: "acc-cad".to_string(),
                asset: Some(AssetResolutionInput {
                    id: Some("asset-mini".to_string()),
                    ..Default::default()
                }),
                activity_type: "BUY".to_string(),
                subtype: None,
                activity_date: "2024-01-15".to_string(),
                quantity: Some(dec!(2)),
                unit_price: Some(dec!(7)),
                currency: "CAD".to_string(),
                fee: Some(dec!(1)),
                tax: None,
                amount: None,
                status: None,
                notes: None,
                fx_rate: None,
                // No multiplier metadata: the form no longer sends its
                // untouched 100 default.
                metadata: None,
                needs_review: None,
                source_system: None,
                source_record_id: None,
                source_group_id: None,
                idempotency_key: None,
            })
            .await
            .expect("create should succeed");

        // 2 x 7 x 10 + 1 fee.
        assert_eq!(created.amount, Some(dec!(141)));
        assert!(!created.needs_review);
    }

    #[tokio::test]
    async fn test_create_uses_the_asset_multiplier_not_activity_metadata() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-usd", "USD"));
        asset_service.add_asset(create_test_asset_with_instrument(
            "asset-opt",
            "XSP240119C00500000",
            Some("XCBO"),
            Some(InstrumentType::Option),
            "USD",
        ));
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let created = activity_service
            .create_activity(NewActivity {
                id: None,
                account_id: "acc-usd".to_string(),
                asset: Some(AssetResolutionInput {
                    id: Some("asset-opt".to_string()),
                    ..Default::default()
                }),
                activity_type: "BUY".to_string(),
                subtype: None,
                activity_date: "2024-01-15".to_string(),
                quantity: Some(dec!(2)),
                unit_price: Some(dec!(7)),
                currency: "USD".to_string(),
                fee: Some(dec!(1)),
                tax: None,
                amount: None,
                status: None,
                notes: None,
                fx_rate: None,
                metadata: Some(r#"{"contract_multiplier": 10}"#.to_string()),
                needs_review: None,
                source_system: None,
                source_record_id: None,
                source_group_id: None,
                idempotency_key: None,
            })
            .await
            .expect("create should succeed");

        // The ASSET owns the multiplier: 2 x 7 x 100 + 1 fee. The row's own
        // `contract_multiplier` seeds a brand-new asset and nothing else, so
        // against this pre-existing 100-multiplier asset it is inert.
        assert_eq!(created.amount, Some(dec!(1401)));
    }

    #[tokio::test]
    async fn test_update_symbol_only_asset_replacement_recalculates_amount() {
        // Replacing the instrument with a not-yet-resolved symbol changes the
        // trade's economics even though no resolved asset id is in the patch;
        // the stored total must not survive onto a different asset unnoticed.
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-usd", "USD"));
        asset_service.add_asset(create_test_asset_with_instrument(
            "asset-aapl",
            "AAPL",
            Some("XNAS"),
            Some(InstrumentType::Equity),
            "USD",
        ));
        let mut existing = create_stored_activity("activity-swap", "acc-usd", Some("asset-aapl"));
        existing.quantity = Some(dec!(1));
        existing.unit_price = Some(dec!(100));
        existing.amount = Some(dec!(123)); // deliberate custom total
        activity_repository.add_activity(existing);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let updated = activity_service
            .update_activity(ActivityUpdate {
                id: "activity-swap".to_string(),
                account_id: "acc-usd".to_string(),
                asset: Some(AssetResolutionInput {
                    symbol: Some("NVDA".to_string()),
                    quote_ccy: Some("USD".to_string()),
                    ..Default::default()
                }),
                activity_type: "BUY".to_string(),
                subtype: None,
                activity_date: "2024-01-15".to_string(),
                quantity: None,
                unit_price: None,
                currency: "USD".to_string(),
                fee: None,
                tax: None,
                amount: None,
                status: None,
                needs_review: None,
                notes: None,
                fx_rate: None,
                metadata: None,
            })
            .await
            .expect("update should succeed");

        // Recalculated from qty x price for the replacement asset, and the
        // replaced custom total is surfaced for review.
        assert_eq!(updated.amount, Some(dec!(100)));
        assert!(updated.needs_review);
    }

    #[tokio::test]
    async fn test_update_never_changes_a_loan_payment_tag() {
        // An edit form sends back the metadata it loaded; a stale copy of the
        // loan tag must not undo a link made in the meantime.
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-usd", "USD"));
        asset_service.add_asset(create_test_asset_with_instrument(
            "asset-opt",
            "XSP240119C00500000",
            Some("XCBO"),
            Some(InstrumentType::Option),
            "USD",
        ));
        let mut existing = create_stored_activity("activity-tag", "acc-usd", Some("asset-opt"));
        existing.metadata = Some(serde_json::json!({ "loan_payment": { "loan_id": "current" } }));
        activity_repository.add_activity(existing);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let updated = activity_service
            .update_activity(ActivityUpdate {
                id: "activity-tag".to_string(),
                account_id: "acc-usd".to_string(),
                asset: Some(AssetResolutionInput {
                    id: Some("asset-opt".to_string()),
                    ..Default::default()
                }),
                activity_type: "BUY".to_string(),
                subtype: None,
                activity_date: "2024-01-15".to_string(),
                quantity: Some(Some(dec!(1))),
                unit_price: Some(Some(dec!(100))),
                currency: "USD".to_string(),
                fee: None,
                tax: None,
                amount: None,
                status: None,
                needs_review: None,
                notes: None,
                fx_rate: None,
                metadata: Some(
                    r#"{"loan_payment": {"loan_id": "stale"}, "contract_multiplier": 10}"#
                        .to_string(),
                ),
            })
            .await
            .expect("update should succeed");

        let metadata = updated.metadata.expect("metadata should be stored");
        assert_eq!(metadata["loan_payment"]["loan_id"], "current");
        assert_eq!(metadata["contract_multiplier"], 10);
    }

    #[tokio::test]
    async fn test_update_metadata_patch_merges_into_existing_metadata() {
        // A legacy client may still send the creation-time multiplier key;
        // the migration breadcrumb stored beside it must survive the patch.
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-usd", "USD"));
        asset_service.add_asset(create_test_asset_with_instrument(
            "asset-opt",
            "XSP240119C00500000",
            Some("XCBO"),
            Some(InstrumentType::Option),
            "USD",
        ));
        let mut existing = create_stored_activity("activity-meta", "acc-usd", Some("asset-opt"));
        existing.metadata = Some(serde_json::json!({
            "final_cash_migration": { "legacy_amount": "1000" }
        }));
        activity_repository.add_activity(existing);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let updated = activity_service
            .update_activity(ActivityUpdate {
                id: "activity-meta".to_string(),
                account_id: "acc-usd".to_string(),
                asset: Some(AssetResolutionInput {
                    id: Some("asset-opt".to_string()),
                    ..Default::default()
                }),
                activity_type: "BUY".to_string(),
                subtype: None,
                activity_date: "2024-01-15".to_string(),
                quantity: Some(Some(dec!(1))),
                unit_price: Some(Some(dec!(100))),
                currency: "USD".to_string(),
                fee: None,
                tax: None,
                amount: None,
                status: None,
                needs_review: None,
                notes: None,
                fx_rate: None,
                metadata: Some(r#"{"contract_multiplier": 10}"#.to_string()),
            })
            .await
            .expect("update should succeed");

        let metadata = updated.metadata.expect("metadata should be stored");
        assert_eq!(
            metadata.get("contract_multiplier").and_then(|v| v.as_f64()),
            Some(10.0),
            "patch key must land"
        );
        assert_eq!(
            metadata
                .get("final_cash_migration")
                .and_then(|v| v.get("legacy_amount"))
                .and_then(|v| v.as_str()),
            Some("1000"),
            "existing breadcrumb must survive the patch"
        );
    }

    #[tokio::test]
    async fn test_create_preserves_attested_custom_total_when_quote_currency_differs() {
        // The form attests this custom CAD total. Its disagreement with the
        // calculated CAD total is preserved regardless of USD asset quotes.
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-cad", "CAD"));
        asset_service.add_asset(create_test_asset_with_instrument(
            "asset-aapl",
            "AAPL",
            Some("XNAS"),
            Some(InstrumentType::Equity),
            "USD",
        ));
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let created = activity_service
            .create_activity(NewActivity {
                id: None,
                account_id: "acc-cad".to_string(),
                asset: Some(AssetResolutionInput {
                    id: Some("asset-aapl".to_string()),
                    ..Default::default()
                }),
                activity_type: "BUY".to_string(),
                subtype: None,
                activity_date: "2024-01-15".to_string(),
                quantity: Some(dec!(1)),
                unit_price: Some(dec!(140)),
                currency: "CAD".to_string(),
                fee: Some(dec!(0)),
                tax: None,
                amount: Some(dec!(150)),
                status: None,
                notes: None,
                fx_rate: None,
                metadata: None,
                needs_review: Some(false),
                source_system: None,
                source_record_id: None,
                source_group_id: None,
                idempotency_key: None,
            })
            .await
            .expect("create should succeed");

        assert_eq!(created.amount, Some(dec!(150)));
        assert!(!created.needs_review);
    }

    #[tokio::test]
    async fn test_create_derives_missing_trade_amount_when_quote_currency_differs() {
        // The activity declares CAD, so quantity and unit price derive a CAD
        // final even though the asset is quoted in USD.
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-cad", "CAD"));
        asset_service.add_asset(create_test_asset_with_instrument(
            "asset-aapl",
            "AAPL",
            Some("XNAS"),
            Some(InstrumentType::Equity),
            "USD",
        ));
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let created = activity_service
            .create_activity(NewActivity {
                id: None,
                account_id: "acc-cad".to_string(),
                asset: Some(AssetResolutionInput {
                    id: Some("asset-aapl".to_string()),
                    ..Default::default()
                }),
                activity_type: "BUY".to_string(),
                subtype: None,
                activity_date: "2024-01-15".to_string(),
                quantity: Some(dec!(1)),
                unit_price: Some(dec!(140)),
                currency: "CAD".to_string(),
                fee: Some(dec!(0)),
                tax: None,
                amount: None,
                status: None,
                notes: None,
                fx_rate: None,
                metadata: None,
                needs_review: None,
                source_system: None,
                source_record_id: None,
                source_group_id: None,
                idempotency_key: None,
            })
            .await
            .expect("activity-currency derivation should succeed");

        assert_eq!(created.amount, Some(dec!(140)));
        assert!(!created.needs_review);
    }

    #[tokio::test]
    async fn test_update_explicit_amount_clear_is_recanonicalized_when_derivable() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-usd", "USD"));
        asset_service.add_asset(create_test_asset_with_instrument(
            "asset-aapl",
            "AAPL",
            Some("XNAS"),
            Some(InstrumentType::Equity),
            "USD",
        ));
        activity_repository.add_activity(create_stored_activity(
            "activity-1",
            "acc-usd",
            Some("asset-aapl"),
        ));

        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            Arc::new(MockQuoteService),
        );

        let updated = activity_service
            .update_activity(ActivityUpdate {
                id: "activity-1".to_string(),
                account_id: "acc-usd".to_string(),
                asset: Some(AssetResolutionInput {
                    id: Some("asset-aapl".to_string()),
                    ..Default::default()
                }),
                activity_type: "BUY".to_string(),
                subtype: None,
                activity_date: "2024-01-15".to_string(),
                quantity: Some(Some(dec!(2))),
                unit_price: Some(Some(dec!(70))),
                currency: "USD".to_string(),
                fee: Some(Some(dec!(5))),
                tax: None,
                amount: Some(None),
                status: None,
                needs_review: Some(false),
                notes: None,
                fx_rate: None,
                metadata: None,
            })
            .await
            .expect("derivable cash should be recanonicalized");

        assert_eq!(updated.amount, Some(dec!(145)));
    }

    #[tokio::test]
    async fn test_create_cash_activity_rejects_missing_non_derivable_final_amount() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-usd", "USD"));
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            Arc::new(MockQuoteService),
        );
        let mut activity = create_test_cash_create("deposit-1", "acc-usd", "DEPOSIT", "USD");
        activity.amount = None;

        let error = activity_service
            .create_activity(activity)
            .await
            .expect_err("missing final cash must not reach persistence");

        assert!(error.to_string().contains("require a final cash amount"));

        let mut reviewed = create_test_cash_create("deposit-reviewed", "acc-usd", "DEPOSIT", "USD");
        reviewed.amount = None;
        reviewed.needs_review = Some(true);
        let error = activity_service
            .create_activity(reviewed)
            .await
            .expect_err("an omitted status defaults to Posted and cannot bypass validation");
        assert!(error.to_string().contains("require a final cash amount"));

        let mut draft = create_test_cash_create("deposit-draft", "acc-usd", "DEPOSIT", "USD");
        draft.amount = None;
        draft.status = Some(ActivityStatus::Draft);
        draft.needs_review = Some(true);
        let error = activity_service
            .create_activity(draft)
            .await
            .expect_err("manual Draft creation cannot bypass final-cash validation");
        assert!(error.to_string().contains("require a final cash amount"));
    }

    #[tokio::test]
    async fn test_create_standalone_fee_copies_explicit_fee_when_amount_is_missing() {
        let account_service = Arc::new(MockAccountService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-usd", "USD"));
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            Arc::new(MockAssetService::new()),
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );
        let mut activity = create_test_cash_create("fee-1", "acc-usd", "FEE", "USD");
        activity.amount = None;
        activity.fee = Some(dec!(4));

        let created = activity_service
            .create_activity(activity)
            .await
            .expect("standalone fee should copy its explicit charge");

        assert_eq!(created.amount, Some(dec!(4)));
    }

    #[tokio::test]
    async fn test_explicit_zero_cash_amount_remains_zero() {
        let account_service = Arc::new(MockAccountService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-usd", "USD"));
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            Arc::new(MockAssetService::new()),
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );
        let mut activity = create_test_cash_create("deposit-zero", "acc-usd", "DEPOSIT", "USD");
        activity.amount = Some(Decimal::ZERO);

        let created = activity_service
            .create_activity(activity)
            .await
            .expect("explicit zero is a valid final amount");

        assert_eq!(created.amount, Some(Decimal::ZERO));
    }

    #[tokio::test]
    async fn draft_review_cannot_be_cleared_without_explicit_post() {
        let account_service = Arc::new(MockAccountService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-usd", "USD"));
        let mut existing = create_stored_activity("deposit-draft", "acc-usd", None);
        existing.activity_type = "DEPOSIT".to_string();
        existing.status = ActivityStatus::Draft;
        existing.needs_review = true;
        existing.amount = Some(dec!(100));
        activity_repository.add_activity(existing);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            Arc::new(MockAssetService::new()),
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let update = ActivityUpdate {
            id: "deposit-draft".to_string(),
            account_id: "acc-usd".to_string(),
            asset: None,
            activity_type: "DEPOSIT".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: None,
            unit_price: None,
            currency: "USD".to_string(),
            fee: None,
            tax: None,
            amount: None,
            status: None,
            needs_review: Some(false),
            notes: None,
            fx_rate: None,
            metadata: None,
        };

        let error = activity_service
            .update_activity(update)
            .await
            .expect_err("Draft approval requires an explicit post transition");
        assert!(error.to_string().contains("explicitly approved and posted"));
    }

    #[tokio::test]
    async fn valid_draft_review_is_posted_only_by_explicit_approval() {
        let account_service = Arc::new(MockAccountService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-usd", "USD"));
        let mut existing = create_stored_activity("deposit-draft", "acc-usd", None);
        existing.activity_type = "DEPOSIT".to_string();
        existing.status = ActivityStatus::Draft;
        existing.needs_review = true;
        existing.amount = Some(dec!(100));
        activity_repository.add_activity(existing);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            Arc::new(MockAssetService::new()),
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let approved = activity_service
            .update_activity(ActivityUpdate {
                id: "deposit-draft".to_string(),
                account_id: "acc-usd".to_string(),
                asset: None,
                activity_type: "DEPOSIT".to_string(),
                subtype: None,
                activity_date: "2024-01-15".to_string(),
                quantity: None,
                unit_price: None,
                currency: "USD".to_string(),
                fee: None,
                tax: None,
                amount: None,
                status: Some(ActivityStatus::Posted),
                needs_review: Some(false),
                notes: None,
                fx_rate: None,
                metadata: None,
            })
            .await
            .expect("valid Draft should post on explicit approval");

        assert_eq!(approved.status, ActivityStatus::Posted);
        assert!(!approved.needs_review);
    }

    #[tokio::test]
    async fn approving_flagged_custom_total_trade_clears_review() {
        // The grid approve action re-sends the stored custom amount alongside
        // `needs_review: Some(false)`. The recomputed confidence flag must not
        // override that explicit human approval.
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-usd", "USD"));
        asset_service.add_asset(create_test_asset("AAPL", "USD"));
        let mut existing = create_stored_activity("buy-custom", "acc-usd", Some("AAPL"));
        existing.quantity = Some(dec!(2));
        existing.unit_price = Some(dec!(10));
        existing.fee = Some(dec!(1));
        existing.amount = Some(dec!(25)); // matches neither gross (20) nor final (21)
        existing.needs_review = true;
        activity_repository.add_activity(existing);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let approved = activity_service
            .update_activity(ActivityUpdate {
                id: "buy-custom".to_string(),
                account_id: "acc-usd".to_string(),
                asset: Some(AssetResolutionInput {
                    id: Some("AAPL".to_string()),
                    ..Default::default()
                }),
                activity_type: "BUY".to_string(),
                subtype: None,
                activity_date: "2024-01-15".to_string(),
                quantity: Some(Some(dec!(2))),
                unit_price: Some(Some(dec!(10))),
                currency: "USD".to_string(),
                fee: Some(Some(dec!(1))),
                tax: None,
                amount: Some(Some(dec!(25))),
                status: None,
                needs_review: Some(false),
                notes: None,
                fx_rate: None,
                metadata: None,
            })
            .await
            .expect("approving a flagged Posted trade must succeed");

        assert!(!approved.needs_review);
        assert_eq!(approved.amount, Some(dec!(25)));
    }

    #[tokio::test]
    async fn editing_trade_to_custom_total_still_flags_review() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-usd", "USD"));
        asset_service.add_asset(create_test_asset("AAPL", "USD"));
        let mut existing = create_stored_activity("buy-edited", "acc-usd", Some("AAPL"));
        existing.quantity = Some(dec!(2));
        existing.unit_price = Some(dec!(10));
        existing.fee = Some(dec!(1));
        existing.amount = Some(dec!(21));
        activity_repository.add_activity(existing);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let updated = activity_service
            .update_activity(ActivityUpdate {
                id: "buy-edited".to_string(),
                account_id: "acc-usd".to_string(),
                asset: Some(AssetResolutionInput {
                    id: Some("AAPL".to_string()),
                    ..Default::default()
                }),
                activity_type: "BUY".to_string(),
                subtype: None,
                activity_date: "2024-01-15".to_string(),
                quantity: Some(Some(dec!(2))),
                unit_price: Some(Some(dec!(10))),
                currency: "USD".to_string(),
                fee: Some(Some(dec!(1))),
                tax: None,
                amount: Some(Some(dec!(30))),
                status: None,
                needs_review: None,
                notes: None,
                fx_rate: None,
                metadata: None,
            })
            .await
            .expect("editing to a custom total is a valid update");

        assert!(updated.needs_review);
        assert_eq!(updated.amount, Some(dec!(30)));
    }

    #[tokio::test]
    async fn creating_trade_with_attested_custom_total_is_not_flagged() {
        // The form shows the calculated total next to the custom one; keeping
        // the custom value and submitting `needs_review: Some(false)` is the
        // user's confirmation, so the manual save path must not re-queue it.
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-usd", "USD"));
        asset_service.add_asset(create_test_asset("AAPL", "USD"));
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let mut new_activity = NewActivity {
            id: Some("buy-attested".to_string()),
            account_id: "acc-usd".to_string(),
            asset: Some(AssetResolutionInput {
                id: Some("AAPL".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: Some(dec!(2)),
            unit_price: Some(dec!(10)),
            currency: "USD".to_string(),
            fee: Some(dec!(1)),
            tax: None,
            amount: Some(dec!(30)), // matches neither gross (20) nor final (21)
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: Some(false),
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let created = activity_service
            .create_activity(new_activity.clone())
            .await
            .expect("attested custom total should be created");
        assert!(!created.needs_review);
        assert_eq!(created.amount, Some(dec!(30)));

        // Without the attestation the same mismatch still flags.
        new_activity.id = Some("buy-unattested".to_string());
        new_activity.idempotency_key = Some("buy-unattested-key".to_string());
        new_activity.needs_review = None;
        let created = activity_service
            .create_activity(new_activity)
            .await
            .expect("unattested custom total should be created");
        assert!(created.needs_review);
        assert_eq!(created.amount, Some(dec!(30)));
    }

    #[tokio::test]
    async fn attested_gross_matching_total_is_preserved_verbatim() {
        // An attested total is the user's number even when it equals the
        // derived gross (fee waived, or the fee booked as a separate FEE
        // activity); only unattested callers get the gross-to-final
        // conversion.
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-usd", "USD"));
        asset_service.add_asset(create_test_asset("AAPL", "USD"));
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let mut new_activity = NewActivity {
            id: Some("buy-gross-attested".to_string()),
            account_id: "acc-usd".to_string(),
            asset: Some(AssetResolutionInput {
                id: Some("AAPL".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: Some(dec!(10)),
            unit_price: Some(dec!(100)),
            currency: "USD".to_string(),
            fee: Some(dec!(5)),
            tax: None,
            amount: Some(dec!(1000)), // exactly qty x price
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: Some(false),
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let created = activity_service
            .create_activity(new_activity.clone())
            .await
            .expect("attested gross-equal total should be created");
        assert_eq!(created.amount, Some(dec!(1000)));
        assert!(!created.needs_review);

        // Without the attestation the same total converts to final.
        new_activity.id = Some("buy-gross-unattested".to_string());
        new_activity.idempotency_key = Some("buy-gross-unattested-key".to_string());
        new_activity.needs_review = None;
        let created = activity_service
            .create_activity(new_activity)
            .await
            .expect("unattested gross-equal total should be created");
        assert_eq!(created.amount, Some(dec!(1005)));
        assert!(!created.needs_review);
    }

    #[tokio::test]
    async fn manual_bond_activity_reuses_existing_cusip_by_isin() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        account_service.add_account(create_test_account("acc-bonds", "USD"));
        let cusip = "037833EZ9";
        let isin = crate::utils::cusip::cusip_to_isin(cusip, "US");
        let asset = create_test_asset_with_instrument(
            "existing-bond",
            cusip,
            None,
            Some(InstrumentType::Bond),
            "USD",
        );
        asset_service.add_asset(asset);
        let activity_service = ActivityService::new(
            Arc::new(MockActivityRepository::new()),
            account_service,
            asset_service.clone(),
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let created = activity_service
            .create_activity(NewActivity {
                id: Some("bond-buy".to_string()),
                account_id: "acc-bonds".to_string(),
                asset: Some(AssetResolutionInput {
                    symbol: Some(isin),
                    instrument_type: Some("BOND".to_string()),
                    quote_ccy: Some("USD".to_string()),
                    ..Default::default()
                }),
                activity_type: "BUY".to_string(),
                subtype: None,
                activity_date: "2024-01-15".to_string(),
                quantity: Some(dec!(100)),
                unit_price: Some(dec!(0.98)),
                amount: Some(dec!(98)),
                currency: "USD".to_string(),
                fee: None,
                tax: None,
                status: None,
                notes: None,
                fx_rate: None,
                metadata: None,
                needs_review: None,
                source_system: None,
                source_record_id: None,
                source_group_id: None,
                idempotency_key: None,
            })
            .await
            .unwrap();

        assert_eq!(created.asset_id.as_deref(), Some("existing-bond"));
        assert_eq!(asset_service.get_assets().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn creating_charge_with_zero_amount_derives_from_charge_column() {
        // Legacy import shape: FEE rows carry amount=0 with the value in fee.
        let account_service = Arc::new(MockAccountService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-usd", "USD"));
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            Arc::new(MockAssetService::new()),
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let mut fee = create_test_cash_create("fee-zero-amount", "acc-usd", "FEE", "USD");
        fee.amount = Some(Decimal::ZERO);
        fee.fee = Some(dec!(15));

        let created = activity_service
            .create_activity(fee)
            .await
            .expect("zero-amount charge should be created");
        assert_eq!(created.amount, Some(dec!(15)));
    }

    #[tokio::test]
    async fn economics_edit_replacing_custom_total_flags_review() {
        // A fee-only patch that omits the amount recalculates the total; when
        // that replaces a deliberate custom total, the change must surface.
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-usd", "USD"));
        asset_service.add_asset(create_test_asset("AAPL", "USD"));
        let mut existing = create_stored_activity("buy-custom-econ", "acc-usd", Some("AAPL"));
        existing.quantity = Some(dec!(2));
        existing.unit_price = Some(dec!(10));
        existing.fee = Some(dec!(1));
        existing.amount = Some(dec!(25)); // custom: matches neither gross nor final
        existing.needs_review = false;
        activity_repository.add_activity(existing);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let updated = activity_service
            .update_activity(ActivityUpdate {
                id: "buy-custom-econ".to_string(),
                account_id: "acc-usd".to_string(),
                asset: Some(AssetResolutionInput {
                    id: Some("AAPL".to_string()),
                    ..Default::default()
                }),
                activity_type: "BUY".to_string(),
                subtype: None,
                activity_date: "2024-01-15".to_string(),
                quantity: None,
                unit_price: None,
                currency: "USD".to_string(),
                fee: Some(Some(dec!(2))),
                tax: None,
                amount: None,
                status: None,
                needs_review: None,
                notes: None,
                fx_rate: None,
                metadata: None,
            })
            .await
            .expect("economics edit must succeed");

        assert_eq!(updated.amount, Some(dec!(22)));
        assert!(updated.needs_review);
    }

    #[tokio::test]
    async fn economics_edit_replacing_calculated_total_stays_unflagged() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-usd", "USD"));
        asset_service.add_asset(create_test_asset("AAPL", "USD"));
        let mut existing = create_stored_activity("buy-calc-econ", "acc-usd", Some("AAPL"));
        existing.quantity = Some(dec!(2));
        existing.unit_price = Some(dec!(10));
        existing.fee = Some(dec!(1));
        existing.amount = Some(dec!(21)); // exactly the calculated final
        existing.needs_review = false;
        activity_repository.add_activity(existing);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let updated = activity_service
            .update_activity(ActivityUpdate {
                id: "buy-calc-econ".to_string(),
                account_id: "acc-usd".to_string(),
                asset: Some(AssetResolutionInput {
                    id: Some("AAPL".to_string()),
                    ..Default::default()
                }),
                activity_type: "BUY".to_string(),
                subtype: None,
                activity_date: "2024-01-15".to_string(),
                quantity: None,
                unit_price: None,
                currency: "USD".to_string(),
                fee: Some(Some(dec!(2))),
                tax: None,
                amount: None,
                status: None,
                needs_review: None,
                notes: None,
                fx_rate: None,
                metadata: None,
            })
            .await
            .expect("economics edit must succeed");

        assert_eq!(updated.amount, Some(dec!(22)));
        assert!(!updated.needs_review);
    }

    #[tokio::test]
    async fn attested_update_preserves_gross_matching_total_verbatim() {
        // Form saves always attest; a re-sent fee-waived total that equals
        // the derived gross must never be gross-converted on an update.
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-usd", "USD"));
        asset_service.add_asset(create_test_asset("AAPL", "USD"));
        let mut existing = create_stored_activity("buy-fee-waived", "acc-usd", Some("AAPL"));
        existing.quantity = Some(dec!(10));
        existing.unit_price = Some(dec!(50));
        existing.fee = Some(dec!(5));
        existing.amount = Some(dec!(500)); // == gross; fee billed separately
        existing.needs_review = false;
        activity_repository.add_activity(existing);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let updated = activity_service
            .update_activity(ActivityUpdate {
                id: "buy-fee-waived".to_string(),
                account_id: "acc-usd".to_string(),
                asset: Some(AssetResolutionInput {
                    id: Some("AAPL".to_string()),
                    ..Default::default()
                }),
                activity_type: "BUY".to_string(),
                subtype: None,
                activity_date: "2024-02-20".to_string(), // date-only change
                quantity: Some(Some(dec!(10))),
                unit_price: Some(Some(dec!(50))),
                currency: "USD".to_string(),
                fee: Some(Some(dec!(5))),
                tax: None,
                amount: Some(Some(dec!(500))),
                status: None,
                needs_review: Some(false),
                notes: None,
                fx_rate: None,
                metadata: None,
            })
            .await
            .expect("date-only edit must succeed");

        assert_eq!(updated.amount, Some(dec!(500)));
        assert!(!updated.needs_review);
    }

    #[tokio::test]
    async fn unrelated_edit_keeps_reviewed_custom_total_cleared() {
        // A patch that touches neither the total nor its economics must not
        // resurrect the review flag on an already-reviewed custom total.
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-usd", "USD"));
        asset_service.add_asset(create_test_asset("AAPL", "USD"));
        let mut existing = create_stored_activity("buy-reviewed", "acc-usd", Some("AAPL"));
        existing.quantity = Some(dec!(2));
        existing.unit_price = Some(dec!(10));
        existing.fee = Some(dec!(1));
        existing.amount = Some(dec!(25)); // custom: matches neither gross nor final
        existing.needs_review = false;
        activity_repository.add_activity(existing);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let updated = activity_service
            .update_activity(ActivityUpdate {
                id: "buy-reviewed".to_string(),
                account_id: "acc-usd".to_string(),
                asset: Some(AssetResolutionInput {
                    id: Some("AAPL".to_string()),
                    ..Default::default()
                }),
                activity_type: "BUY".to_string(),
                subtype: None,
                activity_date: "2024-01-15".to_string(),
                quantity: Some(Some(dec!(2))),
                unit_price: Some(Some(dec!(10))),
                currency: "USD".to_string(),
                fee: Some(Some(dec!(1))),
                tax: None,
                amount: Some(Some(dec!(25))),
                status: None,
                needs_review: None,
                notes: Some("corrected note".to_string()),
                fx_rate: None,
                metadata: None,
            })
            .await
            .expect("notes-only edit must succeed");

        assert!(!updated.needs_review);
        assert_eq!(updated.amount, Some(dec!(25)));
    }

    #[tokio::test]
    async fn editing_trade_to_unresolved_option_symbol_uses_patch_multiplier() {
        // Replacing an equity trade's asset with a not-yet-resolved option
        // symbol must derive the total from the patch's instrument facts, not
        // from the equity being replaced.
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-usd", "USD"));
        asset_service.add_asset(create_test_asset("AAPL", "USD"));
        let mut existing = create_stored_activity("buy-to-option", "acc-usd", Some("AAPL"));
        existing.quantity = Some(dec!(2));
        existing.unit_price = Some(dec!(10));
        existing.fee = Some(dec!(0));
        existing.amount = Some(dec!(20));
        activity_repository.add_activity(existing);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let updated = activity_service
            .update_activity(ActivityUpdate {
                id: "buy-to-option".to_string(),
                account_id: "acc-usd".to_string(),
                asset: Some(AssetResolutionInput {
                    symbol: Some("AAPL260116C00200000".to_string()),
                    instrument_type: Some("OPTION".to_string()),
                    quote_ccy: Some("USD".to_string()),
                    ..Default::default()
                }),
                activity_type: "BUY".to_string(),
                subtype: None,
                activity_date: "2024-01-15".to_string(),
                quantity: Some(Some(dec!(2))),
                unit_price: Some(Some(dec!(10))),
                currency: "USD".to_string(),
                fee: Some(Some(dec!(0))),
                tax: None,
                amount: Some(None),
                status: None,
                needs_review: None,
                notes: None,
                fx_rate: None,
                metadata: None,
            })
            .await
            .expect("asset replacement with recalculated amount");

        // 2 contracts x 10 x 100 multiplier, not 2 x 10 x 1.
        assert_eq!(updated.amount, Some(dec!(2000)));
        assert!(!updated.needs_review);
    }

    #[tokio::test]
    async fn test_create_split_rejects_missing_amount_ratio() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "USD"));
        asset_service.add_asset(create_test_asset("AAPL", "USD"));

        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            Arc::new(MockQuoteService),
        );

        let new_activity = NewActivity {
            id: Some("split-1".to_string()),
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                id: Some("AAPL".to_string()),
                ..Default::default()
            }),
            activity_type: "SPLIT".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: None,
            unit_price: None,
            currency: "USD".to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: None,
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let result = activity_service.create_activity(new_activity).await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("positive amount ratio"));
    }

    #[tokio::test]
    async fn test_create_split_accepts_negative_amount_after_normalization() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        let event_sink = Arc::new(MockDomainEventSink::new());

        account_service.add_account(create_test_account("acc-1", "USD"));
        asset_service.add_asset(create_test_asset("AAPL", "USD"));

        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            Arc::new(MockQuoteService),
        )
        .with_event_sink(event_sink.clone());

        let new_activity = NewActivity {
            id: Some("split-1".to_string()),
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                id: Some("AAPL".to_string()),
                ..Default::default()
            }),
            activity_type: "SPLIT".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: None,
            unit_price: None,
            currency: "USD".to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(-2)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let result = activity_service.create_activity(new_activity).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, Some(dec!(2)));
        assert!(event_sink.events().iter().any(|event| matches!(
            event,
            DomainEvent::AssetSplitActivitiesChanged { asset_ids, .. }
                if asset_ids == &["AAPL".to_string()]
        )));
    }

    #[tokio::test]
    async fn test_update_to_split_rejects_missing_amount_ratio() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "USD"));
        asset_service.add_asset(create_test_asset("AAPL", "USD"));
        activity_repository.add_activity(create_stored_activity(
            "activity-1",
            "acc-1",
            Some("AAPL"),
        ));

        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            Arc::new(MockQuoteService),
        );

        let mut update = create_test_activity_update(
            "activity-1",
            "acc-1",
            Some(AssetResolutionInput {
                id: Some("AAPL".to_string()),
                ..Default::default()
            }),
            "USD",
        );
        update.activity_type = "SPLIT".to_string();
        update.quantity = None;
        update.unit_price = None;
        update.amount = None;

        let result = activity_service.update_activity(update).await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("positive amount ratio"));
    }

    #[tokio::test]
    async fn test_update_existing_split_allows_omitted_amount_ratio() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "USD"));
        asset_service.add_asset(create_test_asset("AAPL", "USD"));
        let mut existing = create_stored_activity("activity-1", "acc-1", Some("AAPL"));
        existing.activity_type = "SPLIT".to_string();
        existing.amount = Some(dec!(2));
        existing.quantity = None;
        existing.unit_price = None;
        activity_repository.add_activity(existing);

        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            Arc::new(MockQuoteService),
        );

        let mut update = create_test_activity_update(
            "activity-1",
            "acc-1",
            Some(AssetResolutionInput {
                id: Some("AAPL".to_string()),
                ..Default::default()
            }),
            "USD",
        );
        update.activity_type = "SPLIT".to_string();
        update.quantity = None;
        update.unit_price = None;
        update.amount = None;
        update.notes = Some("Updated note".to_string());

        let result = activity_service.update_activity(update).await;

        assert!(result.is_ok());
        let updated = result.unwrap();
        assert_eq!(updated.amount, Some(dec!(2)));
        assert_eq!(updated.notes.as_deref(), Some("Updated note"));
    }

    #[tokio::test]
    async fn credit_card_accounts_reject_investment_activity_types() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        let mut account = create_test_account("card-1", "USD");
        account.account_type = "CREDIT_CARD".to_string();
        account_service.add_account(account);

        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            Arc::new(MockQuoteService),
        );

        let new_activity = NewActivity {
            id: Some("activity-1".to_string()),
            account_id: "card-1".to_string(),
            asset: None,
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: Some(dec!(1)),
            unit_price: Some(dec!(100)),
            currency: "USD".to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(100)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let err = activity_service
            .create_activity(new_activity)
            .await
            .expect_err("credit cards should reject investment activity types");

        assert!(err
            .to_string()
            .contains("BUY activities are not supported for credit card accounts"));
    }

    /// A credit card can send money out as a transfer: a balance transfer to
    /// another card, a cash advance into a bank account, or funding a wallet
    /// such as PayPal (#1227). The card's cash math already books TRANSFER_OUT
    /// as more owed, and an unlinked one is neither spending nor income.
    #[tokio::test]
    async fn credit_card_accounts_accept_transfer_out() {
        let account_service = Arc::new(MockAccountService::new());
        let mut account = create_test_account("card-1", "USD");
        account.account_type = "CREDIT_CARD".to_string();
        account_service.add_account(account);

        let activity_service = ActivityService::new(
            Arc::new(MockActivityRepository::new()),
            account_service,
            Arc::new(MockAssetService::new()),
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        activity_service
            .create_activity(create_test_cash_create(
                "balance-transfer",
                "card-1",
                "TRANSFER_OUT",
                "USD",
            ))
            .await
            .expect("credit cards should accept TRANSFER_OUT");

        let err = activity_service
            .create_activity(create_test_cash_create("gift", "card-1", "DEPOSIT", "USD"))
            .await
            .expect_err("credit cards should still reject DEPOSIT");
        assert!(err
            .to_string()
            .contains("DEPOSIT activities are not supported for credit card accounts"));
    }

    /// A security transfer on a card account: TRANSFER_IN/OUT with an asset.
    fn create_test_card_security_transfer(id: &str, activity_type: &str) -> NewActivity {
        NewActivity {
            asset: Some(AssetResolutionInput {
                id: Some("AAPL".to_string()),
                ..Default::default()
            }),
            quantity: Some(dec!(10)),
            ..create_test_cash_create(id, "card-1", activity_type, "USD")
        }
    }

    const CARD_SECURITY_TRANSFER_ERROR: &str =
        "Securities transfers are not supported for credit card accounts";

    /// A card holds no positions: transfers on it move cash only, whichever
    /// direction, on every write path.
    #[tokio::test]
    async fn credit_card_accounts_reject_security_transfers() {
        let asset_service = Arc::new(MockAssetService::new());
        asset_service.add_asset(create_test_asset("AAPL", "USD"));
        let activity_repository = Arc::new(MockActivityRepository::new());
        activity_repository.add_activity(create_stored_activity(
            "activity-1",
            "card-1",
            Some("AAPL"),
        ));
        let activity_service = ActivityService::new(
            activity_repository,
            mixed_account_service(),
            asset_service,
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        for activity_type in ["TRANSFER_OUT", "TRANSFER_IN"] {
            let err = activity_service
                .create_activity(create_test_card_security_transfer(
                    "card-security",
                    activity_type,
                ))
                .await
                .expect_err("credit cards should reject security transfers");
            assert!(
                err.to_string().contains(CARD_SECURITY_TRANSFER_ERROR),
                "{activity_type}: {err}"
            );
        }

        let mut update = create_test_activity_update(
            "activity-1",
            "card-1",
            Some(AssetResolutionInput {
                id: Some("AAPL".to_string()),
                ..Default::default()
            }),
            "USD",
        );
        update.activity_type = "TRANSFER_OUT".to_string();
        let err = activity_service
            .update_activity(update)
            .await
            .expect_err("credit cards should reject security transfer updates");
        assert!(err.to_string().contains(CARD_SECURITY_TRANSFER_ERROR));

        let result = activity_service
            .bulk_mutate_activities(ActivityBulkMutationRequest {
                creates: vec![create_test_card_security_transfer(
                    "card-security",
                    "TRANSFER_OUT",
                )],
                updates: vec![],
                delete_ids: vec![],
            })
            .await
            .expect("bulk mutation reports row errors");
        assert!(result.created.is_empty());
        assert!(
            result
                .errors
                .iter()
                .any(|error| error.message.contains(CARD_SECURITY_TRANSFER_ERROR)),
            "expected the security transfer to be refused, got {:?}",
            result.errors
        );
    }

    /// A cash placeholder in the symbol column must not read as a security on a card.
    /// The reserved cash symbol names the account's cash, not a security
    /// (#1335). Sent through the bulk endpoint as `{ symbol: "$CASH-USD" }`, it
    /// used to be parsed as a ticker: every row got a minted "$CASH" asset,
    /// which a quote provider then tried to price, and a TRANSFER leg carrying
    /// it booked no cash and could not be paired.
    #[tokio::test]
    async fn bulk_create_treats_the_cash_symbol_as_cash_for_every_cash_type() {
        let account_service = Arc::new(MockAccountService::new());
        let mut bank = create_test_account("bank-1", "USD");
        bank.account_type = "CASH".to_string();
        account_service.add_account(bank);
        let asset_service = Arc::new(MockAssetService::new());
        let activity_service = ActivityService::new(
            Arc::new(MockActivityRepository::new()),
            account_service,
            asset_service.clone(),
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let types = [
            "DEPOSIT",
            "WITHDRAWAL",
            "TRANSFER_IN",
            "TRANSFER_OUT",
            "CREDIT",
            "FEE",
            "INTEREST",
        ];
        let creates = types
            .iter()
            .map(|t| {
                let mut create = create_test_cash_create(&format!("row-{t}"), "bank-1", t, "USD");
                create.asset = Some(AssetResolutionInput {
                    symbol: Some("$CASH-USD".to_string()),
                    ..Default::default()
                });
                create
            })
            .collect();
        let result = activity_service
            .bulk_mutate_activities(ActivityBulkMutationRequest {
                creates,
                updates: vec![],
                delete_ids: vec![],
            })
            .await
            .expect("bulk create should succeed");

        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors
        );
        assert_eq!(result.created.len(), types.len());
        for activity in &result.created {
            assert_eq!(
                activity.asset_id, None,
                "{} should book cash, not a security",
                activity.activity_type
            );
        }
        assert!(
            asset_service.get_assets().unwrap().is_empty(),
            "no asset should be minted for the cash symbol"
        );
    }

    /// The reported case (#1335): a transfer pair sent with the cash symbol
    /// and a shared group keeps the group, because both legs are cash.
    #[tokio::test]
    async fn bulk_create_keeps_the_group_on_a_cash_symbol_transfer_pair() {
        let account_service = Arc::new(MockAccountService::new());
        for id in ["bank-a", "bank-b"] {
            let mut account = create_test_account(id, "USD");
            account.account_type = "CASH".to_string();
            account_service.add_account(account);
        }
        let activity_service = ActivityService::new(
            Arc::new(MockActivityRepository::new()),
            account_service,
            Arc::new(MockAssetService::new()),
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );
        let leg = |id: &str, account: &str, activity_type: &str| {
            let mut create = create_test_cash_create(id, account, activity_type, "USD");
            create.asset = Some(AssetResolutionInput {
                symbol: Some("$CASH-USD".to_string()),
                ..Default::default()
            });
            create.source_group_id = Some("grp-1".to_string());
            create
        };

        let result = activity_service
            .bulk_mutate_activities(ActivityBulkMutationRequest {
                creates: vec![
                    leg("out", "bank-a", "TRANSFER_OUT"),
                    leg("in", "bank-b", "TRANSFER_IN"),
                ],
                updates: vec![],
                delete_ids: vec![],
            })
            .await
            .expect("bulk create should succeed");

        assert!(
            result.errors.is_empty(),
            "unexpected errors: {:?}",
            result.errors
        );
        for activity in &result.created {
            assert_eq!(activity.asset_id, None);
            assert_eq!(activity.source_group_id.as_deref(), Some("grp-1"));
        }
    }

    /// Cash-only create for the mixed-account bulk tests below. `currency` is
    /// passed through verbatim so a test can leave it empty and observe which
    /// account currency preparation falls back to.
    fn create_test_cash_create(
        id: &str,
        account_id: &str,
        activity_type: &str,
        currency: &str,
    ) -> NewActivity {
        NewActivity {
            id: Some(id.to_string()),
            account_id: account_id.to_string(),
            asset: None,
            activity_type: activity_type.to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: None,
            unit_price: None,
            currency: currency.to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(100)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: Some(id.to_string()),
            source_group_id: None,
            idempotency_key: None,
        }
    }

    fn mixed_account_service() -> Arc<MockAccountService> {
        let account_service = Arc::new(MockAccountService::new());

        let mut card = create_test_account("card-1", "USD");
        card.account_type = "CREDIT_CARD".to_string();
        account_service.add_account(card);

        let mut bank = create_test_account("bank-1", "USD");
        bank.account_type = "CASH".to_string();
        account_service.add_account(bank);

        account_service
    }

    /// A credit-card row must not disqualify a later row that belongs to a
    /// different account. Regression test: preparation used to validate the
    /// whole batch against the first create's account, so this ordering failed
    /// with "DEPOSIT activities are not supported for credit card accounts".
    #[tokio::test]
    async fn bulk_create_validates_each_row_against_its_own_account() {
        let activity_service = ActivityService::new(
            Arc::new(MockActivityRepository::new()),
            mixed_account_service(),
            Arc::new(MockAssetService::new()),
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let result = activity_service
            .bulk_mutate_activities(ActivityBulkMutationRequest {
                creates: vec![
                    create_test_cash_create("card-purchase", "card-1", "WITHDRAWAL", "USD"),
                    create_test_cash_create("bank-deposit", "bank-1", "DEPOSIT", "USD"),
                ],
                updates: vec![],
                delete_ids: vec![],
            })
            .await
            .expect("mixed-account bulk create should succeed");

        assert!(
            result.errors.is_empty(),
            "expected no errors, got {:?}",
            result.errors
        );
        assert_eq!(result.created.len(), 2);
        assert_eq!(
            result
                .created
                .iter()
                .map(|activity| activity.account_id.as_str())
                .collect::<Vec<_>>(),
            vec!["card-1", "bank-1"],
            "created rows should stay in request order"
        );
    }

    /// The same two rows in the opposite order must behave identically.
    #[tokio::test]
    async fn bulk_create_mixed_accounts_is_order_independent() {
        let activity_service = ActivityService::new(
            Arc::new(MockActivityRepository::new()),
            mixed_account_service(),
            Arc::new(MockAssetService::new()),
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let result = activity_service
            .bulk_mutate_activities(ActivityBulkMutationRequest {
                creates: vec![
                    create_test_cash_create("bank-deposit", "bank-1", "DEPOSIT", "USD"),
                    create_test_cash_create("card-purchase", "card-1", "WITHDRAWAL", "USD"),
                ],
                updates: vec![],
                delete_ids: vec![],
            })
            .await
            .expect("mixed-account bulk create should succeed in either order");

        assert!(
            result.errors.is_empty(),
            "expected no errors, got {:?}",
            result.errors
        );
        assert_eq!(result.created.len(), 2);
        assert_eq!(
            result
                .created
                .iter()
                .map(|activity| activity.account_id.as_str())
                .collect::<Vec<_>>(),
            vec!["bank-1", "card-1"],
            "created rows should stay in request order"
        );
    }

    /// Per-account validation must still reject a row that its own account
    /// disallows, and report it against that row rather than the batch.
    #[tokio::test]
    async fn bulk_create_still_rejects_row_invalid_for_its_own_account() {
        let activity_service = ActivityService::new(
            Arc::new(MockActivityRepository::new()),
            mixed_account_service(),
            Arc::new(MockAssetService::new()),
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let result = activity_service
            .bulk_mutate_activities(ActivityBulkMutationRequest {
                creates: vec![
                    create_test_cash_create("bank-deposit", "bank-1", "DEPOSIT", "USD"),
                    create_test_cash_create("card-deposit", "card-1", "DEPOSIT", "USD"),
                ],
                updates: vec![],
                delete_ids: vec![],
            })
            .await
            .expect("bulk mutation should return structured errors");

        assert!(result.created.is_empty());
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].id.as_deref(), Some("card-deposit"));
        assert!(
            result.errors[0]
                .message
                .contains("DEPOSIT activities are not supported for credit card accounts"),
            "unexpected message: {}",
            result.errors[0].message
        );
    }

    /// Preparation also resolves a missing activity currency from the account,
    /// so a mixed-currency batch must fall back per row, not per batch.
    #[tokio::test]
    async fn bulk_create_resolves_missing_currency_from_each_row_account() {
        let account_service = Arc::new(MockAccountService::new());
        account_service.add_account(create_test_account("acc-usd", "USD"));
        account_service.add_account(create_test_account("acc-eur", "EUR"));

        let activity_service = ActivityService::new(
            Arc::new(MockActivityRepository::new()),
            account_service,
            Arc::new(MockAssetService::new()),
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let result = activity_service
            .bulk_mutate_activities(ActivityBulkMutationRequest {
                creates: vec![
                    create_test_cash_create("usd-deposit", "acc-usd", "DEPOSIT", ""),
                    create_test_cash_create("eur-deposit", "acc-eur", "DEPOSIT", ""),
                ],
                updates: vec![],
                delete_ids: vec![],
            })
            .await
            .expect("mixed-currency bulk create should succeed");

        assert!(
            result.errors.is_empty(),
            "expected no errors, got {:?}",
            result.errors
        );
        assert_eq!(
            result
                .created
                .iter()
                .map(|activity| (activity.account_id.as_str(), activity.currency.as_str()))
                .collect::<Vec<_>>(),
            vec![("acc-usd", "USD"), ("acc-eur", "EUR")],
            "each row should inherit its own account currency"
        );
    }

    #[tokio::test]
    async fn credit_card_accounts_reject_investment_activity_updates() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        let mut account = create_test_account("card-1", "USD");
        account.account_type = "CREDIT_CARD".to_string();
        account_service.add_account(account);
        asset_service.add_asset(create_test_asset("AAPL", "USD"));
        activity_repository.add_activity(create_stored_activity(
            "activity-1",
            "card-1",
            Some("AAPL"),
        ));

        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            Arc::new(MockQuoteService),
        );

        let update = create_test_activity_update(
            "activity-1",
            "card-1",
            Some(AssetResolutionInput {
                id: Some("AAPL".to_string()),
                ..Default::default()
            }),
            "USD",
        );

        let err = activity_service
            .update_activity(update)
            .await
            .expect_err("credit cards should reject investment activity updates");

        assert!(err
            .to_string()
            .contains("BUY activities are not supported for credit card accounts"));
    }

    /// Test: When creating an activity where the activity currency matches the account currency,
    /// but the asset has a different currency, we should still register the FX pair for the asset currency.
    ///
    /// Scenario:
    /// - Account currency: USD
    /// - Asset currency: EUR (e.g., European stock)
    /// - Activity currency: USD (frontend sends account currency for new assets not in lookup)
    ///
    /// Expected: FX pair USD/EUR should be registered
    #[tokio::test]
    async fn test_registers_fx_pair_for_asset_currency_different_from_account() {
        // Setup
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        // Create account with USD currency
        let account = create_test_account("acc-1", "USD");
        account_service.add_account(account);

        // Create asset with EUR currency (different from account)
        let asset = create_test_asset("NESN", "EUR");
        asset_service.add_asset(asset);

        // Create the activity service
        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service.clone(),
            quote_service,
        );

        // Create activity with USD currency (same as account) but for EUR asset
        let new_activity = NewActivity {
            id: Some("activity-1".to_string()),
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                id: Some("NESN".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: Some(dec!(10)),
            unit_price: Some(dec!(100)),
            currency: "USD".to_string(), // Same as account currency
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(1000)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        // Execute
        let result = activity_service.create_activity(new_activity).await;

        // Assert
        assert!(result.is_ok());

        // Check that FX pair was registered for asset currency
        let registered_pairs = fx_service.get_registered_pairs();

        // Should have registered EUR/USD (from=EUR asset currency, to=USD account currency)
        // This creates FX:EUR:USD for converting EUR values to account's USD
        assert!(
            registered_pairs.contains(&("EUR".to_string(), "USD".to_string())),
            "Expected FX pair EUR/USD to be registered for asset currency. Registered pairs: {:?}",
            registered_pairs
        );
    }

    /// Test: When activity currency differs from account currency, register that FX pair
    #[tokio::test]
    async fn test_registers_fx_pair_for_activity_currency_different_from_account() {
        // Setup
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        // Create account with USD currency
        let account = create_test_account("acc-1", "USD");
        account_service.add_account(account);

        // Create asset with EUR currency
        let asset = create_test_asset("NESN", "EUR");
        asset_service.add_asset(asset);

        // Create the activity service
        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service.clone(),
            quote_service,
        );

        // Create activity with EUR currency (different from account USD)
        let new_activity = NewActivity {
            id: Some("activity-1".to_string()),
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                id: Some("NESN".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: Some(dec!(10)),
            unit_price: Some(dec!(100)),
            currency: "EUR".to_string(), // Different from account currency
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(1000)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        // Execute
        let result = activity_service.create_activity(new_activity).await;

        // Assert
        assert!(result.is_ok());

        // Check that FX pair was registered
        let registered_pairs = fx_service.get_registered_pairs();

        // Should have registered EUR/USD (from=EUR activity currency, to=USD account currency)
        // This creates FX:EUR:USD for converting EUR values to account's USD
        assert!(
            registered_pairs.contains(&("EUR".to_string(), "USD".to_string())),
            "Expected FX pair EUR/USD to be registered. Registered pairs: {:?}",
            registered_pairs
        );
    }

    #[tokio::test]
    async fn test_duplicate_manual_create_returns_clear_error() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "USD"));
        asset_service.add_asset(create_test_asset("AAPL", "USD"));

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let duplicate_activity = NewActivity {
            id: None,
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                id: Some("AAPL".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2026-02-27T21:32:00Z".to_string(),
            quantity: Some(dec!(25)),
            unit_price: Some(dec!(51.90)),
            currency: "USD".to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: None,
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        activity_service
            .create_activity(duplicate_activity.clone())
            .await
            .expect("first create should succeed");
        let err = activity_service
            .create_activity(duplicate_activity)
            .await
            .expect_err("second identical create should be rejected as duplicate");

        assert!(
            err.to_string().contains("Duplicate activity detected"),
            "error should clearly explain duplicate detection: {}",
            err
        );
        match &err {
            Error::Activity(ActivityError::DuplicateActivity {
                existing_activity_id,
            }) => {
                assert!(
                    existing_activity_id.is_some(),
                    "the pre-check knows the existing id"
                );
                assert_eq!(
                    err.to_string(),
                    format!(
                        "Activity error: Invalid data: Duplicate activity detected. A matching activity already exists (id: {}).",
                        existing_activity_id.as_deref().unwrap()
                    )
                );
            }
            other => panic!("expected a typed duplicate error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_create_allows_same_trade_with_different_tax() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "USD"));
        asset_service.add_asset(create_test_asset("AAPL", "USD"));

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let taxable_activity = NewActivity {
            id: None,
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                id: Some("AAPL".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2026-02-27T21:32:00Z".to_string(),
            quantity: Some(dec!(25)),
            unit_price: Some(dec!(51.90)),
            currency: "USD".to_string(),
            fee: Some(dec!(0)),
            tax: Some(dec!(1)),
            amount: None,
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        activity_service
            .create_activity(taxable_activity.clone())
            .await
            .expect("first create should succeed");

        // `tax` is not part of the idempotency hash, but the final cash amount
        // is, and the writer derives it from quantity, price, fee and tax. Two
        // trades that differ only in tax therefore carry different authoritative
        // totals and are distinct activities.
        let mut different_tax_activity = taxable_activity;
        different_tax_activity.tax = Some(dec!(2));
        let second = activity_service
            .create_activity(different_tax_activity)
            .await
            .expect("a different authoritative total is a distinct activity");

        let stored = activity_repository
            .get_activities()
            .expect("stored activities should be readable");
        assert_eq!(stored.len(), 2);
        let first = stored
            .iter()
            .find(|activity| activity.id != second.id)
            .expect("first activity remains stored");
        assert_eq!(first.amount, Some(dec!(1298.50)));
        assert_eq!(second.amount, Some(dec!(1299.50)));
        assert_ne!(
            first.idempotency_key, second.idempotency_key,
            "the derived final amount must separate the two keys"
        );
    }

    #[tokio::test]
    async fn test_create_rejects_identical_trade_as_duplicate() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "USD"));
        asset_service.add_asset(create_test_asset("AAPL", "USD"));

        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            asset_service,
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let trade = NewActivity {
            id: None,
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                id: Some("AAPL".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2026-02-27T21:32:00Z".to_string(),
            quantity: Some(dec!(25)),
            unit_price: Some(dec!(51.90)),
            currency: "USD".to_string(),
            fee: Some(dec!(0)),
            tax: Some(dec!(1)),
            amount: None,
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        activity_service
            .create_activity(trade.clone())
            .await
            .expect("first create should succeed");

        let err = activity_service
            .create_activity(trade)
            .await
            .expect_err("an identical trade is still a duplicate");

        assert!(
            err.to_string().contains("Duplicate activity detected"),
            "error should clearly explain duplicate detection: {}",
            err
        );
        match &err {
            Error::Activity(ActivityError::DuplicateActivity {
                existing_activity_id,
            }) => {
                assert_eq!(
                    err.to_string(),
                    format!(
                        "Activity error: Invalid data: Duplicate activity detected. A matching activity already exists (id: {}).",
                        existing_activity_id.as_deref().unwrap()
                    )
                );
            }
            other => panic!("expected a typed duplicate error, got {other:?}"),
        }
        assert_eq!(
            activity_repository
                .get_activities()
                .expect("stored activities should be readable")
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn test_source_record_id_changes_idempotency_for_provider_create() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "USD"));
        asset_service.add_asset(create_test_asset("AAPL", "USD"));

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let provider_activity_one = NewActivity {
            id: None,
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                id: Some("AAPL".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2026-02-27T21:32:00Z".to_string(),
            quantity: Some(dec!(25)),
            unit_price: Some(dec!(51.90)),
            currency: "USD".to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: None,
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: Some("SNAPTRADE".to_string()),
            source_record_id: Some("provider-1".to_string()),
            source_group_id: None,
            idempotency_key: None,
        };

        let mut provider_activity_two = provider_activity_one.clone();
        provider_activity_two.source_record_id = Some("provider-2".to_string());

        activity_service
            .create_activity(provider_activity_one)
            .await
            .expect("first provider create should succeed");
        activity_service
            .create_activity(provider_activity_two)
            .await
            .expect("second provider create with different source record id should succeed");
    }

    #[tokio::test]
    async fn test_bulk_create_assigns_idempotency_key() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "USD"));
        asset_service.add_asset(create_test_asset("AAPL", "USD"));

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let request = ActivityBulkMutationRequest {
            creates: vec![NewActivity {
                id: Some("temp-1".to_string()),
                account_id: "acc-1".to_string(),
                asset: Some(AssetResolutionInput {
                    id: Some("AAPL".to_string()),
                    ..Default::default()
                }),
                activity_type: "BUY".to_string(),
                subtype: None,
                activity_date: "2026-02-27T21:32:00Z".to_string(),
                quantity: Some(dec!(25)),
                unit_price: Some(dec!(51.90)),
                currency: "USD".to_string(),
                fee: Some(dec!(0)),
                tax: None,
                amount: None,
                status: None,
                notes: None,
                fx_rate: None,
                metadata: None,
                needs_review: None,
                source_system: None,
                source_record_id: None,
                source_group_id: None,
                idempotency_key: None,
            }],
            updates: vec![],
            delete_ids: vec![],
        };

        let result = activity_service
            .bulk_mutate_activities(request)
            .await
            .expect("bulk create should succeed");

        assert_eq!(result.created.len(), 1);
        let key = result.created[0]
            .idempotency_key
            .as_deref()
            .expect("bulk create should assign idempotency key");
        assert_eq!(key.len(), 64, "key should be a sha256 hex string");
    }

    #[tokio::test]
    async fn test_bulk_update_preserves_existing_asset_and_subtype_when_omitted() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "USD"));
        asset_service.add_asset(create_test_asset_with_instrument(
            "ETH",
            "ETH",
            None,
            Some(InstrumentType::Crypto),
            "USD",
        ));

        let mut existing = create_stored_activity("staking-1", "acc-1", Some("ETH"));
        existing.activity_type = "INTEREST".to_string();
        existing.subtype = Some("STAKING_REWARD".to_string());
        existing.quantity = Some(dec!(1));
        existing.unit_price = Some(dec!(100));
        existing.amount = Some(dec!(100));
        activity_repository.add_activity(existing);

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let mut update = create_test_activity_update("staking-1", "acc-1", None, "USD");
        update.activity_type = "INTEREST".to_string();
        update.subtype = None;
        update.quantity = None;
        update.unit_price = None;
        update.amount = None;

        let result = activity_service
            .bulk_mutate_activities(ActivityBulkMutationRequest {
                creates: vec![],
                updates: vec![update],
                delete_ids: vec![],
            })
            .await
            .expect("bulk update should succeed");

        assert!(result.errors.is_empty());
        assert_eq!(result.updated.len(), 1);
        assert_eq!(result.updated[0].asset_id.as_deref(), Some("ETH"));
        assert_eq!(result.updated[0].subtype.as_deref(), Some("STAKING_REWARD"));
        assert_eq!(result.updated[0].quantity, Some(dec!(1)));
        assert_eq!(result.updated[0].unit_price, Some(dec!(100)));
    }

    #[tokio::test]
    async fn test_update_requires_explicit_clear_and_rejects_clear_for_required_asset() {
        let account_service = Arc::new(MockAccountService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-1", "USD"));

        let mut existing = create_stored_activity("adjustment-1", "acc-1", Some("AAPL"));
        existing.activity_type = "ADJUSTMENT".to_string();
        existing.subtype = Some("CASH_SWEEP".to_string());
        activity_repository.add_activity(existing);

        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            Arc::new(MockAssetService::new()),
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let mut clear = create_test_activity_update(
            "adjustment-1",
            "acc-1",
            Some(AssetResolutionInput::default()),
            "USD",
        );
        clear.activity_type = "ADJUSTMENT".to_string();
        clear.subtype = Some("CASH_SWEEP".to_string());
        clear.quantity = Some(None);
        clear.unit_price = Some(None);
        clear.amount = Some(Some(dec!(25)));

        let cleared = activity_service
            .update_activity(clear)
            .await
            .expect("optional adjustment asset should clear explicitly");
        assert_eq!(cleared.asset_id, None);

        let required_clear = create_test_activity_update(
            "adjustment-1",
            "acc-1",
            Some(AssetResolutionInput::default()),
            "USD",
        );
        let error = activity_service
            .update_activity(required_clear)
            .await
            .expect_err("BUY must reject an explicit asset clear");
        assert!(error.to_string().contains("asset_id or symbol"));
    }

    /// An update that names the cash symbol clears a stored security rather
    /// than keeping it: the repair path for rows already written with a minted
    /// "$CASH" asset (#1335), and the reason an omitted asset is not enough.
    #[tokio::test]
    async fn test_update_with_cash_symbol_clears_a_stored_cash_security() {
        let account_service = Arc::new(MockAccountService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-1", "USD"));

        let mut existing = create_stored_activity("withdrawal-1", "acc-1", Some("minted-cash"));
        existing.activity_type = "WITHDRAWAL".to_string();
        existing.quantity = None;
        existing.unit_price = None;
        existing.amount = Some(dec!(25));
        activity_repository.add_activity(existing);

        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            Arc::new(MockAssetService::new()),
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let mut update = create_test_activity_update(
            "withdrawal-1",
            "acc-1",
            Some(AssetResolutionInput {
                symbol: Some("$CASH-USD".to_string()),
                ..Default::default()
            }),
            "USD",
        );
        update.activity_type = "WITHDRAWAL".to_string();
        update.quantity = Some(None);
        update.unit_price = Some(None);
        update.amount = Some(Some(dec!(25)));

        let updated = activity_service
            .update_activity(update)
            .await
            .expect("a cash-symbol update should succeed");
        assert_eq!(updated.asset_id, None);
    }

    fn cash_symbol_update_service(
        existing: Activity,
    ) -> (
        ActivityService,
        Arc<MockActivityRepository>,
        Arc<MockAssetService>,
    ) {
        let accounts = Arc::new(MockAccountService::new());
        accounts.add_account(create_test_account("acc-1", "USD"));
        let repository = Arc::new(MockActivityRepository::new());
        repository.add_activity(existing);
        let assets = Arc::new(MockAssetService::new());
        let service = ActivityService::new(
            repository.clone(),
            accounts,
            assets.clone(),
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );
        (service, repository, assets)
    }

    async fn save_cash_symbol_update(
        service: &ActivityService,
        update: ActivityUpdate,
        bulk: bool,
    ) -> Activity {
        if bulk {
            let result = service
                .bulk_mutate_activities(ActivityBulkMutationRequest {
                    creates: vec![],
                    updates: vec![update],
                    delete_ids: vec![],
                })
                .await
                .expect("bulk update should succeed");
            assert!(result.errors.is_empty(), "{:?}", result.errors);
            result.updated.into_iter().next().expect("updated row")
        } else {
            service
                .update_activity(update)
                .await
                .expect("single update")
        }
    }

    #[tokio::test]
    async fn cash_symbol_updates_preserve_currency_and_final_amount() {
        for bulk in [false, true] {
            for (symbol, currency, expected_currency) in [
                ("$CASH-EUR", "", "EUR"),
                (" cash:eur ", " ", "EUR"),
                ("$CASH-EUR", "USD", "USD"),
            ] {
                let mut existing =
                    create_stored_activity("transfer-1", "acc-1", Some("minted-cash"));
                existing.activity_type = "TRANSFER_OUT".to_string();
                // The stored amount is a user-entered total, not quantity x price.
                existing.amount = Some(dec!(75));
                let (service, _, assets) = cash_symbol_update_service(existing);
                let events = MockDomainEventSink::new();
                let service = service.with_event_sink(Arc::new(events.clone()));
                let mut update = create_test_activity_update(
                    "transfer-1",
                    "acc-1",
                    Some(AssetResolutionInput {
                        symbol: Some(symbol.to_string()),
                        ..Default::default()
                    }),
                    currency,
                );
                update.activity_type = "TRANSFER_OUT".to_string();
                update.quantity = Some(Some(dec!(2)));
                update.unit_price = None;
                update.amount = None;

                let saved = save_cash_symbol_update(&service, update, bulk).await;
                assert_eq!(saved.asset_id, None);
                assert_eq!(saved.currency, expected_currency);
                assert_eq!(saved.amount, Some(dec!(75)));
                assert!(assets.get_assets().unwrap().is_empty());
                let emitted = events.events();
                assert_eq!(emitted.len(), 1);
                match &emitted[0] {
                    DomainEvent::ActivitiesChanged {
                        account_ids,
                        asset_ids,
                        currencies,
                        ..
                    } => {
                        assert_eq!(account_ids, &["acc-1".to_string()]);
                        assert_eq!(asset_ids, &["minted-cash".to_string()]);
                        assert!(currencies.contains(&"USD".to_string()));
                        assert!(currencies.contains(&expected_currency.to_string()));
                    }
                    event => panic!("unexpected event: {event:?}"),
                }
            }
        }
    }

    #[tokio::test]
    async fn cash_symbol_creates_infer_currency_without_live_asset_resolution() {
        for mode in ["single", "save"] {
            for (symbol, currency, expected_currency) in [
                ("$cash-eur", "", "EUR"),
                (" CASH_EUR ", "", "EUR"),
                ("CASH:EUR", "USD", "USD"),
                // is_cash_symbol uppercases before checking the currency.
                // Such UTF-8 input must not panic during extraction.
                ("CASH-ııı", "USD", "USD"),
            ] {
                let accounts = Arc::new(MockAccountService::new());
                let account = create_test_account("acc-1", "USD");
                accounts.add_account(account.clone());
                let assets = Arc::new(MockAssetService::new());
                let service = ActivityService::new(
                    Arc::new(MockActivityRepository::new()),
                    accounts,
                    assets.clone(),
                    Arc::new(MockFxService::new()),
                    Arc::new(MockQuoteService),
                );
                let activity = NewActivity {
                    asset: Some(AssetResolutionInput {
                        symbol: Some(symbol.to_string()),
                        ..Default::default()
                    }),
                    ..create_test_cash_create("cash-1", "acc-1", "DEPOSIT", currency)
                };
                if mode == "single" {
                    let saved = service.create_activity(activity).await.unwrap();
                    assert_eq!(saved.asset_id, None);
                    assert_eq!(saved.currency, expected_currency);
                } else {
                    let result = service
                        .prepare_activities_for_save(vec![activity], &account)
                        .await
                        .unwrap();
                    assert!(result.errors.is_empty(), "{:?}", result.errors);
                    assert_eq!(result.prepared.len(), 1);
                    let saved = &result.prepared[0].activity;
                    assert_eq!(saved.get_symbol_id(), None);
                    assert_eq!(saved.currency, expected_currency);
                }
                assert!(assets.get_assets().unwrap().is_empty());
            }
        }
    }

    #[tokio::test]
    async fn cash_symbol_updates_respect_preserved_asset_backed_subtypes() {
        for bulk in [false, true] {
            for (activity_type, subtype) in [
                ("INTEREST", "STAKING_REWARD"),
                ("ADJUSTMENT", "OPTION_EXPIRY"),
                ("ADJUSTMENT", "RETURN_OF_CAPITAL"),
                ("ADJUSTMENT", "NOTIONAL_DISTRIBUTION"),
            ] {
                let mut existing = create_stored_activity("income-1", "acc-1", Some("asset-1"));
                existing.activity_type = activity_type.to_string();
                existing.subtype = Some(subtype.to_string());
                let (service, _, _) = cash_symbol_update_service(existing);
                let mut update = create_test_activity_update(
                    "income-1",
                    "acc-1",
                    Some(AssetResolutionInput {
                        symbol: Some("$CASH-USD".to_string()),
                        ..Default::default()
                    }),
                    "USD",
                );
                update.activity_type = activity_type.to_string();
                update.subtype = None;
                update.quantity = None;
                update.unit_price = None;
                update.amount = None;

                let saved = save_cash_symbol_update(&service, update, bulk).await;
                assert_eq!(saved.subtype.as_deref(), Some(subtype));
                assert!(
                    saved.asset_id.is_some(),
                    "{subtype} must remain asset-backed"
                );
            }
        }
    }

    #[tokio::test]
    async fn cash_symbol_updates_allow_explicit_subtype_clear() {
        for bulk in [false, true] {
            let mut existing = create_stored_activity("income-1", "acc-1", Some("asset-1"));
            existing.activity_type = "INTEREST".to_string();
            existing.subtype = Some("STAKING_REWARD".to_string());
            let (service, _, assets) = cash_symbol_update_service(existing);
            let mut update = create_test_activity_update(
                "income-1",
                "acc-1",
                Some(AssetResolutionInput {
                    symbol: Some("$CASH-USD".to_string()),
                    ..Default::default()
                }),
                "USD",
            );
            update.activity_type = "INTEREST".to_string();
            update.subtype = Some(String::new());
            update.quantity = Some(None);
            update.unit_price = Some(None);
            update.amount = None;

            let saved = save_cash_symbol_update(&service, update, bulk).await;
            assert_eq!(saved.subtype, None);
            assert_eq!(saved.asset_id, None);
            assert_eq!(saved.amount, Some(dec!(100)));
            assert!(assets.get_assets().unwrap().is_empty());
        }
    }

    #[tokio::test]
    async fn cash_symbol_updates_reject_missing_final_cash_before_writing() {
        for bulk in [false, true] {
            let mut existing = create_stored_activity("transfer-1", "acc-1", Some("minted-cash"));
            existing.activity_type = "TRANSFER_OUT".to_string();
            existing.quantity = None;
            existing.unit_price = None;
            existing.amount = None;
            let (service, repository, assets) = cash_symbol_update_service(existing);
            let mut update = create_test_activity_update(
                "transfer-1",
                "acc-1",
                Some(AssetResolutionInput {
                    symbol: Some("$CASH-USD".to_string()),
                    ..Default::default()
                }),
                "USD",
            );
            update.activity_type = "TRANSFER_OUT".to_string();
            update.quantity = None;
            update.unit_price = None;
            update.amount = None;

            let error = if bulk {
                let result = service
                    .bulk_mutate_activities(ActivityBulkMutationRequest {
                        creates: vec![],
                        updates: vec![update],
                        delete_ids: vec![],
                    })
                    .await
                    .unwrap();
                assert!(result.updated.is_empty());
                assert_eq!(result.errors.len(), 1);
                result.errors[0].message.clone()
            } else {
                service
                    .update_activity(update)
                    .await
                    .expect_err("missing amount")
                    .to_string()
            };
            assert!(error.contains("require a final cash amount"), "{error}");
            assert_eq!(
                repository
                    .get_activity("transfer-1")
                    .unwrap()
                    .asset_id
                    .as_deref(),
                Some("minted-cash")
            );
            assert!(assets.get_assets().unwrap().is_empty());
        }
    }

    #[test]
    fn cash_symbol_update_preview_still_rejects_symbols_without_writes() {
        let mut existing = create_stored_activity("cash-1", "acc-1", None);
        existing.activity_type = "WITHDRAWAL".to_string();
        let (service, repository, assets) = cash_symbol_update_service(existing);
        let before = stored_snapshot(&repository);
        let mut update = create_test_activity_update(
            "cash-1",
            "acc-1",
            Some(AssetResolutionInput {
                symbol: Some("$CASH-EUR".to_string()),
                ..Default::default()
            }),
            "",
        );
        update.activity_type = "WITHDRAWAL".to_string();
        let error = service
            .preview_activity_update(update)
            .expect_err("symbols are not previewable");
        assert!(error.to_string().contains("not by symbol"));
        assert_eq!(stored_snapshot(&repository), before);
        assert!(assets.get_assets().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_bulk_update_clears_existing_subtype_when_explicitly_cleared() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "USD"));
        asset_service.add_asset(create_test_asset_with_instrument(
            "ETH",
            "ETH",
            None,
            Some(InstrumentType::Crypto),
            "USD",
        ));

        let mut existing = create_stored_activity("staking-1", "acc-1", Some("ETH"));
        existing.activity_type = "INTEREST".to_string();
        existing.subtype = Some("STAKING_REWARD".to_string());
        existing.quantity = Some(dec!(1));
        existing.unit_price = Some(dec!(100));
        existing.amount = Some(dec!(100));
        activity_repository.add_activity(existing);

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let mut update = create_test_activity_update(
            "staking-1",
            "acc-1",
            Some(AssetResolutionInput {
                id: Some("ETH".to_string()),
                ..Default::default()
            }),
            "USD",
        );
        update.activity_type = "DIVIDEND".to_string();
        update.subtype = Some(String::new());
        update.quantity = None;
        update.unit_price = None;
        update.amount = Some(Some(dec!(100)));

        let result = activity_service
            .bulk_mutate_activities(ActivityBulkMutationRequest {
                creates: vec![],
                updates: vec![update],
                delete_ids: vec![],
            })
            .await
            .expect("bulk update should succeed");

        assert!(result.errors.is_empty());
        assert_eq!(result.updated.len(), 1);
        assert_eq!(result.updated[0].activity_type, "DIVIDEND");
        assert_eq!(result.updated[0].subtype, None);
    }

    #[tokio::test]
    async fn test_bulk_update_preserves_provider_subtype_label_when_omitted() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "USD"));
        asset_service.add_asset(create_test_asset_with_instrument(
            "AAPL_OPT",
            "AAPL251219C00200000",
            None,
            Some(InstrumentType::Option),
            "USD",
        ));

        let mut existing = create_stored_activity("option-1", "acc-1", Some("AAPL_OPT"));
        existing.activity_type = "BUY".to_string();
        existing.subtype = Some("BUY_TO_OPEN".to_string());
        activity_repository.add_activity(existing);

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let mut update = create_test_activity_update(
            "option-1",
            "acc-1",
            Some(AssetResolutionInput {
                id: Some("AAPL_OPT".to_string()),
                ..Default::default()
            }),
            "USD",
        );
        update.activity_type = "BUY".to_string();
        update.subtype = None;
        update.quantity = None;
        update.unit_price = None;
        update.amount = None;

        let result = activity_service
            .bulk_mutate_activities(ActivityBulkMutationRequest {
                creates: vec![],
                updates: vec![update],
                delete_ids: vec![],
            })
            .await
            .expect("bulk update should succeed");

        assert!(result.errors.is_empty());
        assert_eq!(result.updated.len(), 1);
        assert_eq!(result.updated[0].subtype.as_deref(), Some("BUY_TO_OPEN"));
    }

    #[tokio::test]
    async fn test_bulk_update_rejects_asset_backed_subtype_without_quantity() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "USD"));
        asset_service.add_asset(create_test_asset_with_instrument(
            "ETH",
            "ETH",
            None,
            Some(InstrumentType::Crypto),
            "USD",
        ));

        let mut existing = create_stored_activity("interest-1", "acc-1", Some("ETH"));
        existing.activity_type = "INTEREST".to_string();
        existing.subtype = None;
        existing.quantity = None;
        existing.unit_price = None;
        existing.amount = Some(dec!(25));
        activity_repository.add_activity(existing);

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let mut update = create_test_activity_update(
            "interest-1",
            "acc-1",
            Some(AssetResolutionInput {
                id: Some("ETH".to_string()),
                ..Default::default()
            }),
            "USD",
        );
        update.activity_type = "INTEREST".to_string();
        update.subtype = Some("STAKING_REWARD".to_string());
        update.quantity = None;
        update.unit_price = Some(Some(dec!(100)));
        update.amount = Some(Some(dec!(100)));

        let result = activity_service
            .bulk_mutate_activities(ActivityBulkMutationRequest {
                creates: vec![],
                updates: vec![update],
                delete_ids: vec![],
            })
            .await
            .expect("bulk mutation should return structured errors");

        assert_eq!(result.updated.len(), 0);
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].action, "update");
        assert!(
            result.errors[0]
                .message
                .contains("Asset-backed income activities require a positive quantity"),
            "unexpected error: {}",
            result.errors[0].message
        );
    }

    /// Test: When activity currency, asset currency, and account currency are all the same,
    /// no FX pair should be registered
    #[tokio::test]
    async fn test_no_fx_pair_registered_when_all_currencies_match() {
        // Setup
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        // Create account with USD currency
        let account = create_test_account("acc-1", "USD");
        account_service.add_account(account);

        // Create asset with USD currency (same as account)
        let asset = create_test_asset("AAPL", "USD");
        asset_service.add_asset(asset);

        // Create the activity service
        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service.clone(),
            quote_service,
        );

        // Create activity with USD currency (same as account and asset)
        let new_activity = NewActivity {
            id: Some("activity-1".to_string()),
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                id: Some("AAPL".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: Some(dec!(10)),
            unit_price: Some(dec!(150)),
            currency: "USD".to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(1500)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        // Execute
        let result = activity_service.create_activity(new_activity).await;

        // Assert
        assert!(result.is_ok());

        // Check that no FX pair was registered
        let registered_pairs = fx_service.get_registered_pairs();

        assert!(
            registered_pairs.is_empty(),
            "Expected no FX pairs to be registered. Registered pairs: {:?}",
            registered_pairs
        );
    }

    // ==========================================================================
    // resolve_asset_id() and infer_asset_kind() Tests (via create_activity)
    // ==========================================================================

    /// Test: When symbol + exchange_mic are provided, finds existing asset by instrument fields
    #[tokio::test]
    async fn test_resolve_asset_id_with_symbol_and_exchange() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        let account = create_test_account("acc-1", "USD");
        account_service.add_account(account);

        let asset = create_test_asset_with_instrument(
            "aapl-uuid",
            "AAPL",
            Some("XNAS"),
            Some(InstrumentType::Equity),
            "USD",
        );
        asset_service.add_asset(asset);

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let new_activity = NewActivity {
            id: Some("activity-1".to_string()),
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                symbol: Some("AAPL".to_string()),
                exchange_mic: Some("XNAS".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: Some(dec!(10)),
            unit_price: Some(dec!(150)),
            currency: "USD".to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(1500)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let result = activity_service.create_activity(new_activity).await;
        assert!(result.is_ok());

        let created = result.unwrap();
        assert_eq!(
            created.asset_id,
            Some("aapl-uuid".to_string()),
            "Should find existing asset by instrument fields"
        );
    }

    /// Test: When symbol is provided without exchange, generates SEC:SYMBOL:UNKNOWN
    #[tokio::test]
    async fn test_resolve_asset_id_symbol_without_exchange() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        let account = create_test_account("acc-1", "USD");
        account_service.add_account(account);

        let asset = create_test_asset_with_instrument(
            "tsla-uuid",
            "TSLA",
            None,
            Some(InstrumentType::Equity),
            "USD",
        );
        asset_service.add_asset(asset);

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let new_activity = NewActivity {
            id: Some("activity-1".to_string()),
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                symbol: Some("TSLA".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: Some(dec!(5)),
            unit_price: Some(dec!(200)),
            currency: "USD".to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(1000)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let result = activity_service.create_activity(new_activity).await;
        assert!(result.is_ok());

        let created = result.unwrap();
        assert_eq!(
            created.asset_id,
            Some("tsla-uuid".to_string()),
            "Should find existing asset by instrument symbol"
        );
    }

    #[tokio::test]
    async fn test_create_rejects_new_equity_without_requested_quote_ccy() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        let account = create_test_account("acc-1", "USD");
        account_service.add_account(account);

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let new_activity = NewActivity {
            id: Some("activity-missing-quote".to_string()),
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                symbol: Some("NFLX".to_string()),
                exchange_mic: Some("XNAS".to_string()),
                instrument_type: Some("EQUITY".to_string()),
                quote_mode: Some("MARKET".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: Some(dec!(1)),
            unit_price: Some(dec!(500)),
            currency: "USD".to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(500)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let result = activity_service.create_activity(new_activity).await;
        assert!(result.is_err());
        let error = result.err().unwrap().to_string();
        assert!(
            error.contains("Quote currency is required"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn test_create_rejects_staking_reward_without_symbol_or_asset_id() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "CAD"));

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let new_activity = NewActivity {
            id: Some("staking-reward-1".to_string()),
            account_id: "acc-1".to_string(),
            asset: None,
            activity_type: "INTEREST".to_string(),
            subtype: Some("STAKING_REWARD".to_string()),
            activity_date: "2024-01-15".to_string(),
            quantity: Some(dec!(0.25)),
            unit_price: Some(dec!(4000)),
            currency: "CAD".to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(1000)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let result = activity_service.create_activity(new_activity).await;
        assert!(result.is_err());
        let error = result.err().unwrap().to_string();
        assert!(
            error.contains("Asset-backed activities need either asset_id or symbol"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn test_create_canonicalizes_case_insensitive_subtype() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "USD"));
        asset_service.add_asset(create_test_asset_with_instrument(
            "ETH",
            "ETH",
            None,
            Some(InstrumentType::Crypto),
            "USD",
        ));

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let activity = activity_service
            .create_activity(NewActivity {
                id: Some("staking-reward-lowercase".to_string()),
                account_id: "acc-1".to_string(),
                asset: Some(AssetResolutionInput {
                    id: Some("ETH".to_string()),
                    ..Default::default()
                }),
                activity_type: "INTEREST".to_string(),
                subtype: Some("staking_reward".to_string()),
                activity_date: "2024-01-15".to_string(),
                quantity: Some(dec!(0.25)),
                unit_price: Some(dec!(4000)),
                currency: "USD".to_string(),
                fee: Some(dec!(0)),
                tax: None,
                amount: Some(dec!(1000)),
                status: None,
                notes: None,
                fx_rate: None,
                metadata: None,
                needs_review: None,
                source_system: None,
                source_record_id: None,
                source_group_id: None,
                idempotency_key: None,
            })
            .await
            .expect("lowercase subtype should save");

        assert_eq!(activity.subtype.as_deref(), Some("STAKING_REWARD"));
    }

    #[tokio::test]
    async fn test_create_asset_backed_income_normalizes_negative_values_before_validation() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "USD"));
        asset_service.add_asset(create_test_asset_with_instrument(
            "ETH",
            "ETH",
            None,
            Some(InstrumentType::Crypto),
            "USD",
        ));

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let activity = activity_service
            .create_activity(NewActivity {
                id: Some("staking-reward-negative".to_string()),
                account_id: "acc-1".to_string(),
                asset: Some(AssetResolutionInput {
                    id: Some("ETH".to_string()),
                    ..Default::default()
                }),
                activity_type: "INTEREST".to_string(),
                subtype: Some("STAKING_REWARD".to_string()),
                activity_date: "2024-01-15".to_string(),
                quantity: Some(dec!(-0.25)),
                unit_price: Some(dec!(-4000)),
                currency: "USD".to_string(),
                fee: Some(dec!(0)),
                tax: None,
                amount: Some(dec!(-1000)),
                status: None,
                notes: None,
                fx_rate: None,
                metadata: None,
                needs_review: None,
                source_system: None,
                source_record_id: None,
                source_group_id: None,
                idempotency_key: None,
            })
            .await
            .expect("negative provider-style signs should normalize before validation");

        assert_eq!(activity.quantity, Some(dec!(0.25)));
        assert_eq!(activity.unit_price, Some(dec!(4000)));
        assert_eq!(activity.amount, Some(dec!(1000)));
    }

    #[tokio::test]
    async fn test_save_prepare_normalizes_minor_currency_tax() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        let account = create_test_account("acc-1", "GBP");
        account_service.add_account(account.clone());
        asset_service.add_asset(create_test_asset("SEC:AZN:XLON", "GBp"));

        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            Arc::new(MockQuoteService),
        );

        let result = activity_service
            .prepare_activities_for_save(
                vec![NewActivity {
                    id: Some("gbp-tax-save".to_string()),
                    account_id: "acc-1".to_string(),
                    asset: Some(AssetResolutionInput {
                        id: Some("SEC:AZN:XLON".to_string()),
                        ..Default::default()
                    }),
                    activity_type: "BUY".to_string(),
                    subtype: None,
                    activity_date: "2024-01-15".to_string(),
                    quantity: Some(dec!(10)),
                    unit_price: Some(dec!(14082)),
                    currency: "GBp".to_string(),
                    fee: Some(dec!(999)),
                    tax: Some(dec!(150)),
                    amount: Some(dec!(140820)),
                    status: None,
                    notes: None,
                    fx_rate: None,
                    metadata: None,
                    needs_review: None,
                    source_system: None,
                    source_record_id: None,
                    source_group_id: None,
                    idempotency_key: None,
                }],
                &account,
            )
            .await
            .expect("save preparation should normalize minor currency values");

        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(result.prepared.len(), 1);

        let prepared = &result.prepared[0].activity;
        assert_eq!(prepared.currency, "GBP");
        assert_eq!(prepared.unit_price, Some(dec!(140.82)));
        // The normalized amount matches the derived gross exactly, so the
        // writer converts it to final: 1408.20 + 9.99 + 1.50.
        assert_eq!(prepared.amount, Some(dec!(1419.69)));
        assert_eq!(prepared.fee, Some(dec!(9.99)));
        assert_eq!(prepared.tax, Some(dec!(1.50)));
    }

    #[tokio::test]
    async fn test_bulk_create_rejects_new_equity_without_requested_quote_ccy() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        let account = create_test_account("acc-1", "USD");
        account_service.add_account(account);

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let request = ActivityBulkMutationRequest {
            creates: vec![NewActivity {
                id: Some("temp-1".to_string()),
                account_id: "acc-1".to_string(),
                asset: Some(AssetResolutionInput {
                    symbol: Some("NVDA".to_string()),
                    exchange_mic: Some("XNAS".to_string()),
                    instrument_type: Some("EQUITY".to_string()),
                    quote_mode: Some("MARKET".to_string()),
                    ..Default::default()
                }),
                activity_type: "BUY".to_string(),
                subtype: None,
                activity_date: "2024-01-15".to_string(),
                quantity: Some(dec!(1)),
                unit_price: Some(dec!(100)),
                currency: "USD".to_string(),
                fee: Some(dec!(0)),
                tax: None,
                amount: Some(dec!(100)),
                status: None,
                notes: None,
                fx_rate: None,
                metadata: None,
                needs_review: None,
                source_system: None,
                source_record_id: None,
                source_group_id: None,
                idempotency_key: None,
            }],
            updates: vec![],
            delete_ids: vec![],
        };

        let result = activity_service
            .bulk_mutate_activities(request)
            .await
            .unwrap();
        assert_eq!(result.errors.len(), 1);
        assert_eq!(result.errors[0].action, "create");
        assert!(
            result.errors[0]
                .message
                .contains("Quote currency is required"),
            "unexpected error: {}",
            result.errors[0].message
        );
    }

    /// Test: For NEW activities, symbol takes priority over asset_id to ensure canonical ID generation
    /// This is intentional - for new activities we always want canonical IDs
    #[tokio::test]
    async fn test_resolve_asset_id_backward_compatibility() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        let account = create_test_account("acc-1", "USD");
        account_service.add_account(account);

        let asset = create_test_asset_with_instrument(
            "aapl-uuid-2",
            "AAPL",
            Some("XNAS"),
            Some(InstrumentType::Equity),
            "USD",
        );
        asset_service.add_asset(asset);

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let new_activity = NewActivity {
            id: Some("activity-1".to_string()),
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                id: Some("IGNORED".to_string()), // Should be ignored when symbol is provided
                symbol: Some("AAPL".to_string()),
                exchange_mic: Some("XNAS".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: Some(dec!(10)),
            unit_price: Some(dec!(150)),
            currency: "USD".to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(1500)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let result = activity_service.create_activity(new_activity).await;
        assert!(result.is_ok());

        let created = result.unwrap();
        assert_eq!(
            created.asset_id,
            Some("aapl-uuid-2".to_string()),
            "Symbol + exchange_mic should find existing asset, ignoring provided asset_id"
        );
    }

    #[tokio::test]
    async fn test_update_stale_asset_id_with_changed_mic_rebinds_to_matching_asset() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "CAD"));
        asset_service.add_asset(create_test_asset_with_instrument(
            "SEC:FBTC:NEOE",
            "FBTC",
            Some("NEOE"),
            Some(InstrumentType::Equity),
            "CAD",
        ));
        asset_service.add_asset(create_test_asset_with_instrument(
            "SEC:FBTC:XTSE",
            "FBTC",
            Some("XTSE"),
            Some(InstrumentType::Equity),
            "CAD",
        ));
        activity_repository.add_activity(create_stored_activity(
            "activity-fbtc",
            "acc-1",
            Some("SEC:FBTC:NEOE"),
        ));

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let update = create_test_activity_update(
            "activity-fbtc",
            "acc-1",
            Some(AssetResolutionInput {
                id: Some("SEC:FBTC:NEOE".to_string()),
                symbol: Some("FBTC".to_string()),
                exchange_mic: Some("XTSE".to_string()),
                quote_ccy: Some("CAD".to_string()),
                instrument_type: Some("EQUITY".to_string()),
                ..Default::default()
            }),
            "CAD",
        );

        let updated = activity_service
            .update_activity(update)
            .await
            .expect("update should rebind to matching MIC asset");

        assert_eq!(updated.asset_id.as_deref(), Some("SEC:FBTC:XTSE"));
    }

    #[tokio::test]
    async fn test_update_stale_crypto_quote_ccy_rebinds_to_matching_asset() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "USD"));
        asset_service.add_asset(create_test_asset_with_instrument(
            "CRYPTO:BTC:USD",
            "BTC",
            None,
            Some(InstrumentType::Crypto),
            "USD",
        ));
        asset_service.add_asset(create_test_asset_with_instrument(
            "CRYPTO:BTC:EUR",
            "BTC",
            None,
            Some(InstrumentType::Crypto),
            "EUR",
        ));
        activity_repository.add_activity(create_stored_activity(
            "activity-btc",
            "acc-1",
            Some("CRYPTO:BTC:USD"),
        ));

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let update = create_test_activity_update(
            "activity-btc",
            "acc-1",
            Some(AssetResolutionInput {
                id: Some("CRYPTO:BTC:USD".to_string()),
                symbol: Some("BTC".to_string()),
                quote_ccy: Some("EUR".to_string()),
                instrument_type: Some("CRYPTO".to_string()),
                ..Default::default()
            }),
            "EUR",
        );

        let updated = activity_service
            .update_activity(update)
            .await
            .expect("update should rebind to matching crypto quote currency asset");

        assert_eq!(updated.asset_id.as_deref(), Some("CRYPTO:BTC:EUR"));
    }

    #[tokio::test]
    async fn test_create_id_only_missing_asset_errors() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "USD"));
        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let new_activity = NewActivity {
            id: Some("activity-missing-asset".to_string()),
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                id: Some("missing-asset".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: Some(dec!(1)),
            unit_price: Some(dec!(100)),
            currency: "USD".to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(100)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let error = activity_service
            .create_activity(new_activity)
            .await
            .expect_err("id-only missing asset should fail")
            .to_string();

        assert!(
            error.contains("Asset not found"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn test_create_matching_id_and_identity_keeps_id_without_duplicate_asset() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "USD"));
        asset_service.add_asset(create_test_asset_with_instrument(
            "SEC:AAPL:XNAS",
            "AAPL",
            Some("XNAS"),
            Some(InstrumentType::Equity),
            "USD",
        ));

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service.clone(),
            fx_service,
            quote_service,
        );

        let new_activity = NewActivity {
            id: Some("activity-aapl".to_string()),
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                id: Some("SEC:AAPL:XNAS".to_string()),
                symbol: Some("AAPL".to_string()),
                exchange_mic: Some("XNAS".to_string()),
                quote_ccy: Some("USD".to_string()),
                instrument_type: Some("EQUITY".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: Some(dec!(1)),
            unit_price: Some(dec!(100)),
            currency: "USD".to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(100)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let created = activity_service
            .create_activity(new_activity)
            .await
            .expect("matching id + identity should create activity");

        assert_eq!(created.asset_id.as_deref(), Some("SEC:AAPL:XNAS"));
        assert_eq!(asset_service.get_assets().unwrap().len(), 1);
    }

    /// The save path is where a synced activity's asset identity is really decided,
    /// so the share-class rule has to hold here and not only in canonicalization.
    /// `ZAAA.F` on Cboe Canada is BMO's currency-hedged unit class and `.F` is
    /// Yahoo's Frankfurt suffix; both candidate rows are seeded, and the unhedged
    /// `ZAAA` is the one this used to bind to.
    #[tokio::test]
    async fn test_create_keeps_a_share_class_the_supplied_venue_contradicts() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "CAD"));
        asset_service.add_asset(create_test_asset_with_instrument(
            "SEC:ZAAA.F:NEOE",
            "ZAAA.F",
            Some("NEOE"),
            Some(InstrumentType::Equity),
            "CAD",
        ));
        asset_service.add_asset(create_test_asset_with_instrument(
            "SEC:ZAAA:NEOE",
            "ZAAA",
            Some("NEOE"),
            Some(InstrumentType::Equity),
            "CAD",
        ));

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let new_activity = NewActivity {
            id: Some("activity-zaaa".to_string()),
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                symbol: Some("ZAAA.F".to_string()),
                exchange_mic: Some("NEOE".to_string()),
                quote_ccy: Some("CAD".to_string()),
                instrument_type: Some("EQUITY".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2026-07-30".to_string(),
            quantity: Some(dec!(10)),
            unit_price: Some(dec!(29.48)),
            currency: "CAD".to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(294.8)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let created = activity_service
            .create_activity(new_activity)
            .await
            .expect("hedged unit class should save");

        assert_eq!(created.asset_id.as_deref(), Some("SEC:ZAAA.F:NEOE"));
    }

    /// The same path, on the venue the suffix agrees with: `SHOP.TO` with XTSE is an
    /// ordinary exchange suffix and must still be stripped off the ticker.
    #[tokio::test]
    async fn test_create_still_strips_a_suffix_the_supplied_venue_agrees_with() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "CAD"));
        asset_service.add_asset(create_test_asset_with_instrument(
            "SEC:SHOP:XTSE",
            "SHOP",
            Some("XTSE"),
            Some(InstrumentType::Equity),
            "CAD",
        ));
        asset_service.add_asset(create_test_asset_with_instrument(
            "SEC:SHOP.TO:XTSE",
            "SHOP.TO",
            Some("XTSE"),
            Some(InstrumentType::Equity),
            "CAD",
        ));

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let new_activity = NewActivity {
            id: Some("activity-shop".to_string()),
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                symbol: Some("SHOP.TO".to_string()),
                exchange_mic: Some("XTSE".to_string()),
                quote_ccy: Some("CAD".to_string()),
                instrument_type: Some("EQUITY".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2026-07-30".to_string(),
            quantity: Some(dec!(1)),
            unit_price: Some(dec!(100)),
            currency: "CAD".to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(100)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let created = activity_service
            .create_activity(new_activity)
            .await
            .expect("suffixed symbol on its own venue should save");

        assert_eq!(created.asset_id.as_deref(), Some("SEC:SHOP:XTSE"));
    }

    #[tokio::test]
    async fn test_create_id_lookup_transient_error_is_not_swallowed() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "USD"));
        asset_service.set_get_asset_by_id_error(
            "Asset not found lookup failed: database temporarily unavailable",
        );

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let new_activity = NewActivity {
            id: Some("activity-transient".to_string()),
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                id: Some("SEC:AAPL:XNAS".to_string()),
                symbol: Some("AAPL".to_string()),
                exchange_mic: Some("XNAS".to_string()),
                quote_ccy: Some("USD".to_string()),
                instrument_type: Some("EQUITY".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: Some(dec!(1)),
            unit_price: Some(dec!(100)),
            currency: "USD".to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(100)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let error = activity_service
            .create_activity(new_activity)
            .await
            .expect_err("transient get_asset_by_id error should fail")
            .to_string();

        assert!(
            error.contains("database temporarily unavailable"),
            "unexpected error: {error}"
        );
    }

    /// Test: Cash activity (DEPOSIT) generates CASH:{currency} asset ID
    #[tokio::test]
    async fn test_resolve_asset_id_cash_deposit_no_asset() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        let account = create_test_account("acc-1", "USD");
        account_service.add_account(account);

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let new_activity = NewActivity {
            id: Some("activity-1".to_string()),
            account_id: "acc-1".to_string(),
            asset: None,
            activity_type: "DEPOSIT".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: None,
            unit_price: None,
            currency: "USD".to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(1000)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let result = activity_service.create_activity(new_activity).await;
        assert!(result.is_ok());

        let created = result.unwrap();
        assert_eq!(
            created.asset_id, None,
            "DEPOSIT should have no asset_id (cash activities have no asset in v2)"
        );
    }

    #[tokio::test]
    async fn test_cash_adjustment_without_asset_can_be_saved() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "USD"));
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            Arc::new(MockQuoteService),
        );

        let created = activity_service
            .create_activity(NewActivity {
                id: Some("cash-adjustment".to_string()),
                account_id: "acc-1".to_string(),
                asset: None,
                activity_type: "ADJUSTMENT".to_string(),
                subtype: Some("CASH_SWEEP".to_string()),
                activity_date: "2024-01-15".to_string(),
                quantity: None,
                unit_price: None,
                currency: "USD".to_string(),
                fee: None,
                tax: None,
                amount: Some(Decimal::ZERO),
                status: None,
                notes: None,
                fx_rate: None,
                metadata: None,
                needs_review: None,
                source_system: None,
                source_record_id: None,
                source_group_id: None,
                idempotency_key: None,
            })
            .await
            .expect("cash-style adjustments should not require an asset");

        assert_eq!(created.asset_id, None);
        assert_eq!(created.subtype.as_deref(), Some("CASH_SWEEP"));
        assert_eq!(created.amount, Some(Decimal::ZERO));
    }

    #[tokio::test]
    async fn test_option_expiry_adjustment_without_asset_is_rejected() {
        let account_service = Arc::new(MockAccountService::new());
        account_service.add_account(create_test_account("acc-1", "USD"));
        let activity_service = ActivityService::new(
            Arc::new(MockActivityRepository::new()),
            account_service,
            Arc::new(MockAssetService::new()),
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let error = activity_service
            .create_activity(NewActivity {
                id: Some("option-expiry".to_string()),
                account_id: "acc-1".to_string(),
                asset: None,
                activity_type: "ADJUSTMENT".to_string(),
                subtype: Some("OPTION_EXPIRY".to_string()),
                activity_date: "2024-01-15".to_string(),
                quantity: Some(Decimal::ONE),
                unit_price: Some(Decimal::ZERO),
                currency: "USD".to_string(),
                fee: None,
                tax: None,
                amount: Some(Decimal::ZERO),
                status: None,
                notes: None,
                fx_rate: None,
                metadata: None,
                needs_review: None,
                source_system: None,
                source_record_id: None,
                source_group_id: None,
                idempotency_key: None,
            })
            .await
            .expect_err("option expiry still requires an asset");

        assert!(error.to_string().contains("asset_id or symbol"));
    }

    #[tokio::test]
    async fn test_cost_basis_adjustment_without_asset_is_rejected() {
        for subtype in ["RETURN_OF_CAPITAL", "NOTIONAL_DISTRIBUTION"] {
            let account_service = Arc::new(MockAccountService::new());
            account_service.add_account(create_test_account("acc-1", "USD"));
            let activity_service = ActivityService::new(
                Arc::new(MockActivityRepository::new()),
                account_service,
                Arc::new(MockAssetService::new()),
                Arc::new(MockFxService::new()),
                Arc::new(MockQuoteService),
            );

            let error = activity_service
                .create_activity(NewActivity {
                    id: Some("cost-adjustment".to_string()),
                    account_id: "acc-1".to_string(),
                    asset: None,
                    activity_type: "ADJUSTMENT".to_string(),
                    subtype: Some(subtype.to_string()),
                    activity_date: "2024-01-15".to_string(),
                    quantity: None,
                    unit_price: None,
                    currency: "USD".to_string(),
                    fee: None,
                    tax: None,
                    amount: Some(dec!(12.5)),
                    status: None,
                    notes: None,
                    fx_rate: None,
                    metadata: None,
                    needs_review: None,
                    source_system: None,
                    source_record_id: None,
                    source_group_id: None,
                    idempotency_key: None,
                })
                .await
                .expect_err("a cost basis adjustment moves an asset's cost");

            assert!(
                error.to_string().contains("asset_id or symbol"),
                "{subtype}: {error}"
            );
        }
    }

    /// Test: Cash activity (WITHDRAWAL) has no asset_id
    #[tokio::test]
    async fn test_resolve_asset_id_cash_withdrawal_no_asset() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        let account = create_test_account("acc-1", "USD");
        account_service.add_account(account);

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let new_activity = NewActivity {
            id: Some("activity-1".to_string()),
            account_id: "acc-1".to_string(),
            asset: None,
            activity_type: "WITHDRAWAL".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: None,
            unit_price: None,
            currency: "USD".to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(500)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let result = activity_service.create_activity(new_activity).await;
        assert!(result.is_ok());

        let created = result.unwrap();
        assert_eq!(
            created.asset_id, None,
            "WITHDRAWAL should have no asset_id (cash activities have no asset in v2)"
        );
    }

    #[tokio::test]
    async fn test_update_cash_adjustment_without_asset() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-1", "USD"));
        let mut existing = create_stored_activity("cash-adjustment", "acc-1", None);
        existing.activity_type = ACTIVITY_TYPE_ADJUSTMENT.to_string();
        existing.quantity = None;
        existing.unit_price = None;
        existing.amount = Some(dec!(25));
        activity_repository.add_activity(existing);

        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            Arc::new(MockQuoteService),
        );
        let mut update = create_test_activity_update("cash-adjustment", "acc-1", None, "USD");
        update.activity_type = ACTIVITY_TYPE_ADJUSTMENT.to_string();
        update.quantity = Some(None);
        update.unit_price = Some(None);
        update.amount = Some(Some(dec!(30)));

        let updated = activity_service
            .update_activity(update)
            .await
            .expect("cash adjustment update should not require an asset");

        assert_eq!(updated.asset_id, None);
        assert_eq!(updated.amount, Some(dec!(30)));
    }

    /// Test: Non-cash activity (BUY) without symbol or asset_id fails
    #[tokio::test]
    async fn test_resolve_asset_id_buy_without_symbol_fails() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        let account = create_test_account("acc-1", "USD");
        account_service.add_account(account);

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let new_activity = NewActivity {
            id: Some("activity-1".to_string()),
            account_id: "acc-1".to_string(),
            asset: None, // No asset info
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: Some(dec!(10)),
            unit_price: Some(dec!(150)),
            currency: "USD".to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(1500)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let result = activity_service.create_activity(new_activity).await;
        assert!(
            result.is_err(),
            "BUY without symbol or asset_id should fail"
        );
    }

    /// Test: Crypto symbol (BTC) without exchange infers CRYPTO kind
    #[tokio::test]
    async fn test_infer_asset_kind_common_crypto_symbol() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        let account = create_test_account("acc-1", "USD");
        account_service.add_account(account);

        // Add crypto asset with instrument fields
        let asset = create_test_asset_with_instrument(
            "btc-uuid",
            "BTC",
            None,
            Some(InstrumentType::Crypto),
            "USD",
        );
        asset_service.add_asset(asset);

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let new_activity = NewActivity {
            id: Some("activity-1".to_string()),
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                symbol: Some("BTC".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: Some(dec!(1)),
            unit_price: Some(dec!(50000)),
            currency: "USD".to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(50000)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let result = activity_service.create_activity(new_activity).await;
        assert!(result.is_ok());

        let created = result.unwrap();
        assert_eq!(
            created.asset_id,
            Some("btc-uuid".to_string()),
            "BTC should match existing crypto asset"
        );
    }

    /// Test: Crypto pattern (BTC-USD) infers CRYPTO kind
    #[tokio::test]
    async fn test_infer_asset_kind_crypto_pattern() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        let account = create_test_account("acc-1", "USD");
        account_service.add_account(account);

        // Add crypto asset with normalized symbol (BTC-USD -> BTC)
        let asset = create_test_asset_with_instrument(
            "btc-uuid-2",
            "BTC",
            None,
            Some(InstrumentType::Crypto),
            "USD",
        );
        asset_service.add_asset(asset);

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let new_activity = NewActivity {
            id: Some("activity-1".to_string()),
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                symbol: Some("BTC-USD".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: Some(dec!(1)),
            unit_price: Some(dec!(50000)),
            currency: "USD".to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(50000)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let result = activity_service.create_activity(new_activity).await;
        assert!(result.is_ok(), "Expected Ok, got {:?}", result);

        let created = result.unwrap();
        // In v2, asset_id is a UUID. The BTC-USD symbol should be normalized to BTC
        // and matched against the existing crypto asset.
        assert!(
            created.asset_id.is_some(),
            "BTC-USD pattern should resolve to an asset"
        );
    }

    /// Test: Explicit kind input overrides inference
    #[tokio::test]
    async fn test_infer_asset_kind_explicit_input() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        let account = create_test_account("acc-1", "USD");
        account_service.add_account(account);

        // BTC would normally be inferred as crypto, but we're forcing security with exchange_mic
        let asset = create_test_asset_with_instrument(
            "btc-equity-uuid",
            "BTC",
            Some("XNAS"),
            Some(InstrumentType::Equity),
            "USD",
        );
        asset_service.add_asset(asset);

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let new_activity = NewActivity {
            id: Some("activity-1".to_string()),
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                symbol: Some("BTC".to_string()),
                exchange_mic: Some("XNAS".to_string()),
                kind: Some("SECURITY".to_string()), // Explicit input
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: Some(dec!(100)),
            unit_price: Some(dec!(50)),
            currency: "USD".to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(5000)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let result = activity_service.create_activity(new_activity).await;
        assert!(result.is_ok());

        let created = result.unwrap();
        assert_eq!(
            created.asset_id,
            Some("btc-equity-uuid".to_string()),
            "Explicit SECURITY input with exchange should find existing equity asset"
        );
    }

    /// Test: Exchange MIC presence forces Security kind
    #[tokio::test]
    async fn test_infer_asset_kind_exchange_mic_forces_security() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        let account = create_test_account("acc-1", "CAD");
        account_service.add_account(account);

        let asset = create_test_asset_with_instrument(
            "eth-equity-uuid",
            "ETH",
            Some("XTSE"),
            Some(InstrumentType::Equity),
            "CAD",
        );
        asset_service.add_asset(asset);

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        // ETH would be inferred as crypto, but exchange_mic forces security
        let new_activity = NewActivity {
            id: Some("activity-1".to_string()),
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                symbol: Some("ETH".to_string()),
                exchange_mic: Some("XTSE".to_string()), // Has exchange = security
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: Some(dec!(100)),
            unit_price: Some(dec!(30)),
            currency: "CAD".to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(3000)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let result = activity_service.create_activity(new_activity).await;
        assert!(result.is_ok());

        let created = result.unwrap();
        assert_eq!(
            created.asset_id,
            Some("eth-equity-uuid".to_string()),
            "Exchange MIC should match existing equity asset"
        );
    }

    /// Test: All cash activity types generate CASH:{currency} asset_id
    #[tokio::test]
    async fn test_all_cash_activity_types_no_asset() {
        let cash_types = [
            "DEPOSIT",
            "WITHDRAWAL",
            "INTEREST",
            "TAX",
            "FEE",
            "TRANSFER_IN",
            "TRANSFER_OUT",
        ];

        for activity_type in cash_types {
            let account_service = Arc::new(MockAccountService::new());
            let asset_service = Arc::new(MockAssetService::new());
            let fx_service = Arc::new(MockFxService::new());
            let activity_repository = Arc::new(MockActivityRepository::new());

            let account = create_test_account("acc-1", "USD");
            account_service.add_account(account);

            let quote_service = Arc::new(MockQuoteService);
            let activity_service = ActivityService::new(
                activity_repository.clone(),
                account_service,
                asset_service,
                fx_service,
                quote_service,
            );

            let new_activity = NewActivity {
                id: Some(format!("activity-{}", activity_type)),
                account_id: "acc-1".to_string(),
                asset: None,
                activity_type: activity_type.to_string(),
                subtype: None,
                activity_date: "2024-01-15".to_string(),
                quantity: None,
                unit_price: None,
                currency: "USD".to_string(),
                fee: Some(dec!(0)),
                tax: None,
                amount: Some(dec!(100)),
                status: None,
                notes: None,
                fx_rate: None,
                metadata: None,
                needs_review: None,
                source_system: None,
                source_record_id: None,
                source_group_id: None,
                idempotency_key: None,
            };

            let result = activity_service.create_activity(new_activity).await;
            assert!(
                result.is_ok(),
                "{} should succeed without asset_id",
                activity_type
            );

            let created = result.unwrap();
            assert_eq!(
                created.asset_id, None,
                "{} should have no asset_id (cash activities have no asset in v2)",
                activity_type
            );
        }
    }

    /// Test: Bulk mutation also registers FX pairs correctly
    #[tokio::test]
    async fn test_bulk_mutate_registers_fx_pair_for_asset_currency() {
        // Setup
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        // Create account with USD currency
        let account = create_test_account("acc-1", "USD");
        account_service.add_account(account);

        // Create asset with CHF currency
        let asset = create_test_asset("NESN", "CHF");
        asset_service.add_asset(asset);

        // Create the activity service
        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service.clone(),
            quote_service,
        );

        // Create bulk mutation request
        let request = ActivityBulkMutationRequest {
            creates: vec![NewActivity {
                id: Some("activity-1".to_string()),
                account_id: "acc-1".to_string(),
                asset: Some(AssetResolutionInput {
                    id: Some("NESN".to_string()),
                    ..Default::default()
                }),
                activity_type: "BUY".to_string(),
                subtype: None,
                activity_date: "2024-01-15".to_string(),
                quantity: Some(dec!(10)),
                unit_price: Some(dec!(100)),
                currency: "USD".to_string(), // Same as account, different from asset
                fee: Some(dec!(0)),
                tax: None,
                amount: Some(dec!(1000)),
                status: None,
                notes: None,
                fx_rate: None,
                metadata: None,
                needs_review: None,
                source_system: None,
                source_record_id: None,
                source_group_id: None,
                idempotency_key: None,
            }],
            updates: vec![],
            delete_ids: vec![],
        };

        // Execute
        let result = activity_service.bulk_mutate_activities(request).await;

        // Assert
        assert!(result.is_ok());

        // Check that FX pair was registered for asset currency
        let registered_pairs = fx_service.get_registered_pairs();

        // Should have registered CHF/USD (from=CHF asset currency, to=USD account currency)
        // This creates FX:CHF:USD for converting CHF values to account's USD
        assert!(
            registered_pairs.contains(&("CHF".to_string(), "USD".to_string())),
            "Expected FX pair CHF/USD to be registered. Registered pairs: {:?}",
            registered_pairs
        );
    }

    // Regression for #1388. When the CSV has no currency column the review step
    // resolves the account currency, but the frontend keeps `currencySource:
    // "default"` and sends an empty currency on confirm. The import must fall back
    // to the account currency instead of persisting "", which later breaks FX
    // conversion ("" -> CAD) during valuation.

    #[test]
    fn find_transfer_match_candidates_returns_cash_matches_only() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        activity_repository.add_activity(create_cash_transfer_activity(
            "source-out",
            "acc-a",
            "TRANSFER_OUT",
            "2024-01-15T00:00:00Z",
            dec!(100),
            "USD",
        ));
        activity_repository.add_activity(create_cash_transfer_activity(
            "cash-match",
            "acc-b",
            "TRANSFER_IN",
            "2024-01-17T00:00:00Z",
            dec!(100),
            "USD",
        ));
        activity_repository.add_activity(create_cash_transfer_activity(
            "wrong-amount",
            "acc-b",
            "TRANSFER_IN",
            "2024-01-17T00:00:00Z",
            dec!(101),
            "USD",
        ));
        activity_repository.add_activity(create_cash_transfer_activity(
            "same-account",
            "acc-a",
            "TRANSFER_IN",
            "2024-01-17T00:00:00Z",
            dec!(100),
            "USD",
        ));
        activity_repository.add_activity(create_cash_transfer_activity(
            "same-account-fx",
            "acc-a",
            "TRANSFER_IN",
            "2024-01-17T00:00:00Z",
            dec!(135),
            "CAD",
        ));
        let mut linked = create_cash_transfer_activity(
            "already-linked",
            "acc-c",
            "TRANSFER_IN",
            "2024-01-17T00:00:00Z",
            dec!(100),
            "USD",
        );
        linked.source_group_id = Some("group-1".to_string());
        activity_repository.add_activity(linked);
        let mut linked_counterpart = create_cash_transfer_activity(
            "already-linked-out",
            "acc-d",
            "TRANSFER_OUT",
            "2024-01-17T00:00:00Z",
            dec!(100),
            "USD",
        );
        linked_counterpart.source_group_id = Some("group-1".to_string());
        activity_repository.add_activity(linked_counterpart);

        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            Arc::new(MockQuoteService),
        );

        let candidates = activity_service
            .find_transfer_match_candidates(TransferMatchCandidateRequest {
                activity_id: "source-out".to_string(),
                window_days: Some(7),
                limit: Some(25),
            })
            .expect("candidate search should succeed");

        assert_eq!(
            candidates
                .iter()
                .map(|candidate| candidate.activity.id.as_str())
                .collect::<Vec<_>>(),
            vec!["cash-match", "same-account-fx"]
        );
        let cash_match = candidates
            .iter()
            .find(|candidate| candidate.activity.id == "cash-match")
            .expect("cross-account cash candidate");
        assert_eq!(cash_match.match_kind, "cash");
        assert!(cash_match
            .warnings
            .iter()
            .any(|warning| warning.contains("Dates differ")));

        let fx_match = candidates
            .iter()
            .find(|candidate| candidate.activity.id == "same-account-fx")
            .expect("same-account FX candidate");
        assert_eq!(fx_match.match_kind, "cash_fx_conversion");
        assert!(fx_match
            .reasons
            .iter()
            .any(|reason| reason == "Cash FX conversion"));
        assert!(!candidates
            .iter()
            .any(|candidate| candidate.activity.id == "same-account"));
    }

    #[test]
    fn find_transfer_match_candidates_allows_orphan_source_group() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        let mut source = create_cash_transfer_activity(
            "source-out",
            "acc-a",
            "TRANSFER_OUT",
            "2024-01-15T00:00:00Z",
            dec!(100),
            "USD",
        );
        source.source_group_id = Some("orphan-group".to_string());
        activity_repository.add_activity(source);
        activity_repository.add_activity(create_cash_transfer_activity(
            "cash-match",
            "acc-b",
            "TRANSFER_IN",
            "2024-01-15T00:00:00Z",
            dec!(100),
            "USD",
        ));

        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            Arc::new(MockQuoteService),
        );

        let candidates = activity_service
            .find_transfer_match_candidates(TransferMatchCandidateRequest {
                activity_id: "source-out".to_string(),
                window_days: Some(7),
                limit: Some(25),
            })
            .expect("candidate search should succeed");

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].activity.id, "cash-match");
    }

    /// Unlinked transfers for the scan, one per link state: two that match
    /// each other and an orphan from a broken pair (all three reported by the
    /// Health Center), plus an old one and one with no match, both marked
    /// external, as imports mark them. Left out: a linked pair without the
    /// internal marker, a pair linked across to an archived account, a draft,
    /// and a transfer in the archived account (`acc-closed` is not among the
    /// open accounts).
    fn unlinked_transfer_service() -> ActivityService {
        let account_service = Arc::new(MockAccountService::new());
        account_service.add_account(create_test_account("acc-a", "USD"));
        account_service.add_account(create_test_account("acc-b", "USD"));
        let activity_repository = Arc::new(MockActivityRepository::new());
        // The fixture marks transfers external; these three are not.
        let internal = |id, account_id, activity_type, date, amount, group: Option<&str>| {
            let mut transfer =
                create_cash_transfer_activity(id, account_id, activity_type, date, amount, "USD");
            transfer.metadata = None;
            transfer.source_group_id = group.map(str::to_string);
            transfer
        };
        for transfer in [
            internal(
                "out-1",
                "acc-a",
                "TRANSFER_OUT",
                "2024-01-15T00:00:00Z",
                dec!(100),
                None,
            ),
            internal(
                "in-1",
                "acc-b",
                "TRANSFER_IN",
                "2024-01-15T00:00:00Z",
                dec!(100),
                None,
            ),
            internal(
                "orphan-out",
                "acc-a",
                "TRANSFER_OUT",
                "2024-02-01T00:00:00Z",
                dec!(70),
                Some("orphan-group"),
            ),
            create_cash_transfer_activity(
                "in-old",
                "acc-b",
                "TRANSFER_IN",
                "2023-06-01T00:00:00Z",
                dec!(50),
                "USD",
            ),
            create_cash_transfer_activity(
                "outside-in",
                "acc-b",
                "TRANSFER_IN",
                "2024-01-12T00:00:00Z",
                dec!(45),
                "USD",
            ),
            create_cash_transfer_activity(
                "closed-out",
                "acc-closed",
                "TRANSFER_OUT",
                "2024-01-10T00:00:00Z",
                dec!(40),
                "USD",
            ),
            internal(
                "paired-out",
                "acc-a",
                "TRANSFER_OUT",
                "2024-01-20T00:00:00Z",
                dec!(30),
                Some("pair-group"),
            ),
            internal(
                "paired-in",
                "acc-b",
                "TRANSFER_IN",
                "2024-01-20T00:00:00Z",
                dec!(30),
                Some("pair-group"),
            ),
        ] {
            activity_repository.add_activity(transfer);
        }
        // Linked across to an account that is now archived.
        activity_repository.add_activity(internal(
            "kept-out",
            "acc-a",
            "TRANSFER_OUT",
            "2024-01-25T00:00:00Z",
            dec!(20),
            Some("cross-group"),
        ));
        activity_repository.add_activity(internal(
            "archived-in",
            "acc-closed",
            "TRANSFER_IN",
            "2024-01-25T00:00:00Z",
            dec!(20),
            Some("cross-group"),
        ));
        let mut draft = internal(
            "draft-in",
            "acc-b",
            "TRANSFER_IN",
            "2024-01-16T00:00:00Z",
            dec!(100),
            None,
        );
        draft.status = ActivityStatus::Draft;
        activity_repository.add_activity(draft);

        ActivityService::new(
            activity_repository,
            account_service,
            Arc::new(MockAssetService::new()),
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        )
    }

    fn unlinked_ids(unlinked: &UnlinkedTransfers) -> Vec<&str> {
        unlinked
            .transfers
            .iter()
            .map(|transfer| transfer.activity.id.as_str())
            .collect()
    }

    #[tokio::test]
    async fn find_unlinked_transfers_lists_unlinked_transfers_with_state_and_candidates() {
        let service = unlinked_transfer_service();
        let unlinked = service
            .find_unlinked_transfers(UnlinkedTransfersRequest::default())
            .await
            .expect("scan");

        assert_eq!(unlinked.total, 5);
        assert_eq!(
            unlinked_ids(&unlinked),
            vec!["orphan-out", "in-1", "out-1", "outside-in", "in-old"]
        );
        assert_eq!(
            unlinked
                .transfers
                .iter()
                .map(|transfer| transfer.link_state)
                .collect::<Vec<_>>(),
            vec![
                TransferLinkState::BrokenLink,
                TransferLinkState::NeedsCounterpart,
                TransferLinkState::NeedsCounterpart,
                TransferLinkState::External,
                TransferLinkState::External,
            ]
        );
        for transfer in &unlinked.transfers {
            // The same candidates the Link Transfer dialog shows.
            let dialog = service
                .find_transfer_match_candidates(TransferMatchCandidateRequest {
                    activity_id: transfer.activity.id.clone(),
                    window_days: Some(7),
                    limit: Some(3),
                })
                .expect("candidates");
            let ids = |candidates: &[TransferMatchCandidate]| {
                candidates
                    .iter()
                    .map(|candidate| candidate.activity.id.clone())
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                ids(&transfer.candidates),
                ids(&dialog),
                "{}",
                transfer.activity.id
            );
        }
        let out = &unlinked.transfers[2];
        assert_eq!(out.candidates.len(), 1);
        assert_eq!(out.candidates[0].activity.id, "in-1");
        assert!(unlinked.transfers[4].candidates.is_empty());
    }

    #[tokio::test]
    async fn find_unlinked_transfers_applies_scope_filters_and_paging() {
        let service = unlinked_transfer_service();
        let scan = |request: UnlinkedTransfersRequest| {
            let service = &service;
            async move {
                service
                    .find_unlinked_transfers(request)
                    .await
                    .expect("scan")
            }
        };

        let by_account = scan(UnlinkedTransfersRequest {
            account_id: Some("acc-a".to_string()),
            ..Default::default()
        })
        .await;
        assert_eq!(unlinked_ids(&by_account), vec!["orphan-out", "out-1"]);

        let by_dates = scan(UnlinkedTransfersRequest {
            start_date: NaiveDate::from_ymd_opt(2024, 1, 1),
            end_date: NaiveDate::from_ymd_opt(2024, 1, 31),
            ..Default::default()
        })
        .await;
        assert_eq!(unlinked_ids(&by_dates), vec!["in-1", "out-1", "outside-in"]);

        let health_reported = scan(UnlinkedTransfersRequest {
            link_states: Some(vec![
                TransferLinkState::NeedsCounterpart,
                TransferLinkState::BrokenLink,
            ]),
            ..Default::default()
        })
        .await;
        assert_eq!(
            unlinked_ids(&health_reported),
            vec!["orphan-out", "in-1", "out-1"]
        );

        let page = scan(UnlinkedTransfersRequest {
            offset: Some(1),
            limit: Some(2),
            ..Default::default()
        })
        .await;
        assert_eq!(unlinked_ids(&page), vec!["in-1", "out-1"]);
        assert_eq!(page.total, 5);

        let one = scan(UnlinkedTransfersRequest {
            activity_id: Some("out-1".to_string()),
            ..Default::default()
        })
        .await;
        assert_eq!(unlinked_ids(&one), vec!["out-1"]);
        assert!(one.linked.is_none());
    }

    /// A lookup of a linked transfer names its other side from the same pairs
    /// the scan resolves: a pair without the internal marker, and a pair whose
    /// other side is in an archived account.
    #[tokio::test]
    async fn find_unlinked_transfers_names_the_other_side_of_a_linked_transfer() {
        let service = unlinked_transfer_service();
        for (id, counterpart) in [("paired-out", "paired-in"), ("kept-out", "archived-in")] {
            let lookup = service
                .find_unlinked_transfers(UnlinkedTransfersRequest {
                    activity_id: Some(id.to_string()),
                    ..Default::default()
                })
                .await
                .expect("lookup");
            assert_eq!(lookup.total, 0, "{id}");
            let linked = lookup.linked.expect("linked");
            assert_eq!(linked.counterpart_id, counterpart);
            assert_eq!(linked.link_state, TransferLinkState::Linked);
        }
    }

    #[test]
    fn find_transfer_match_candidates_matches_security_by_asset_and_quantity() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        activity_repository.add_activity(create_security_transfer_activity(
            "security-out",
            "acc-a",
            "TRANSFER_OUT",
            "2024-01-15T00:00:00Z",
            "SEC:AAPL:XNAS",
            dec!(10),
            dec!(150),
        ));
        activity_repository.add_activity(create_security_transfer_activity(
            "security-match",
            "acc-b",
            "TRANSFER_IN",
            "2024-01-15T00:00:00Z",
            "SEC:AAPL:XNAS",
            dec!(10),
            dec!(153),
        ));
        activity_repository.add_activity(create_security_transfer_activity(
            "same-account-security",
            "acc-a",
            "TRANSFER_IN",
            "2024-01-15T00:00:00Z",
            "SEC:AAPL:XNAS",
            dec!(10),
            dec!(150),
        ));
        activity_repository.add_activity(create_security_transfer_activity(
            "wrong-quantity",
            "acc-b",
            "TRANSFER_IN",
            "2024-01-15T00:00:00Z",
            "SEC:AAPL:XNAS",
            dec!(9),
            dec!(150),
        ));
        activity_repository.add_activity(create_security_transfer_activity(
            "wrong-asset",
            "acc-b",
            "TRANSFER_IN",
            "2024-01-15T00:00:00Z",
            "SEC:MSFT:XNAS",
            dec!(10),
            dec!(150),
        ));

        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            Arc::new(MockQuoteService),
        );

        let candidates = activity_service
            .find_transfer_match_candidates(TransferMatchCandidateRequest {
                activity_id: "security-out".to_string(),
                window_days: Some(7),
                limit: Some(25),
            })
            .expect("candidate search should succeed");

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].activity.id, "security-match");
        assert_eq!(candidates[0].match_kind, "security");
        assert!(candidates[0]
            .warnings
            .iter()
            .any(|warning| warning.contains("Prices differ")));
    }

    #[tokio::test]
    async fn unlink_transfer_activities_emits_activities_changed_event() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        let event_sink = Arc::new(MockDomainEventSink::new());
        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            asset_service,
            fx_service,
            quote_service,
        )
        .with_event_sink(event_sink.clone());

        let earlier = DateTime::parse_from_rfc3339("2024-01-15T00:00:00Z")
            .expect("valid date")
            .with_timezone(&Utc);
        let later = DateTime::parse_from_rfc3339("2024-01-16T00:00:00Z")
            .expect("valid date")
            .with_timezone(&Utc);
        activity_repository.activities.lock().unwrap().extend([
            Activity {
                id: "transfer-in".to_string(),
                account_id: "acc-in".to_string(),
                asset_id: Some("asset-in".to_string()),
                activity_type: "TRANSFER_IN".to_string(),
                activity_type_override: None,
                source_type: None,
                subtype: None,
                status: ActivityStatus::Posted,
                activity_date: later,
                settlement_date: None,
                quantity: None,
                unit_price: None,
                amount: Some(dec!(100)),
                fee: Some(dec!(0)),
                tax: None,
                currency: "CAD".to_string(),
                fx_rate: None,
                notes: None,
                metadata: Some(json!({ "flow": { "is_external": false } })),
                source_system: Some("MANUAL".to_string()),
                source_record_id: None,
                source_group_id: Some("transfer-group".to_string()),
                idempotency_key: None,
                import_run_id: None,
                is_user_modified: false,
                needs_review: false,
                created_at: earlier,
                updated_at: earlier,
            },
            Activity {
                id: "transfer-out".to_string(),
                account_id: "acc-out".to_string(),
                asset_id: Some("asset-out".to_string()),
                activity_type: "TRANSFER_OUT".to_string(),
                activity_type_override: None,
                source_type: None,
                subtype: None,
                status: ActivityStatus::Posted,
                activity_date: earlier,
                settlement_date: None,
                quantity: None,
                unit_price: None,
                amount: Some(dec!(100)),
                fee: Some(dec!(0)),
                tax: None,
                currency: "USD".to_string(),
                fx_rate: None,
                notes: None,
                metadata: Some(json!({ "flow": { "is_external": false } })),
                source_system: Some("MANUAL".to_string()),
                source_record_id: None,
                source_group_id: Some("transfer-group".to_string()),
                idempotency_key: None,
                import_run_id: None,
                is_user_modified: false,
                needs_review: false,
                created_at: earlier,
                updated_at: earlier,
            },
        ]);

        activity_service
            .unlink_transfer_activities("transfer-in".to_string(), "transfer-out".to_string())
            .await
            .expect("unlink should succeed");

        let stored = activity_repository
            .get_activities()
            .expect("stored activities");
        assert!(stored
            .iter()
            .all(|activity| activity.source_group_id.is_none()));
        assert!(stored.iter().all(|activity| {
            activity
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("flow"))
                .and_then(|flow| flow.get("is_external"))
                .and_then(|value| value.as_bool())
                == Some(true)
        }));

        let events = event_sink.events();
        assert_eq!(events.len(), 1);
        match &events[0] {
            DomainEvent::ActivitiesChanged {
                account_ids,
                asset_ids,
                currencies,
                earliest_activity_at_utc,
            } => {
                let mut account_ids = account_ids.clone();
                account_ids.sort();
                assert_eq!(account_ids, vec!["acc-in", "acc-out"]);

                let mut asset_ids = asset_ids.clone();
                asset_ids.sort();
                assert_eq!(asset_ids, vec!["asset-in", "asset-out"]);

                let mut currencies = currencies.clone();
                currencies.sort();
                assert_eq!(currencies, vec!["CAD", "USD"]);
                assert_eq!(*earliest_activity_at_utc, Some(earlier));
            }
            event => panic!("expected ActivitiesChanged event, got {event:?}"),
        }
    }

    #[tokio::test]
    async fn save_internal_transfer_pair_creates_cross_currency_legs() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-twd", "TWD"));
        account_service.add_account(create_test_account("acc-usd", "USD"));

        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            Arc::new(MockQuoteService),
        );

        let result = activity_service
            .save_internal_transfer_pair(InternalTransferPairRequest {
                transfer_out_id: None,
                transfer_in_id: None,
                source_group_id: None,
                from_account_id: "acc-twd".to_string(),
                to_account_id: "acc-usd".to_string(),
                activity_date: "2026-06-03T20:20:00Z".to_string(),
                source_amount: Some(dec!(1000)),
                destination_amount: Some(dec!(31.20)),
                source_currency: "TWD".to_string(),
                destination_currency: "USD".to_string(),
                fx_rate: Some(dec!(0.0312)),
                notes: Some("Move cash".to_string()),
                transfer_mode: Some("cash".to_string()),
            })
            .await
            .expect("pair create should succeed");

        assert_eq!(result.transfer_out.account_id, "acc-twd");
        assert_eq!(result.transfer_out.currency, "TWD");
        assert_eq!(result.transfer_out.amount, Some(dec!(1000)));
        assert_eq!(result.transfer_in.account_id, "acc-usd");
        assert_eq!(result.transfer_in.currency, "USD");
        assert_eq!(result.transfer_in.amount, Some(dec!(31.20)));
        assert_eq!(result.transfer_in.fx_rate, None);
        assert_eq!(
            result.transfer_out.source_group_id,
            result.transfer_in.source_group_id
        );
        assert_eq!(
            result
                .transfer_out
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("flow"))
                .and_then(|flow| flow.get("is_external"))
                .and_then(|value| value.as_bool()),
            Some(false)
        );
    }

    #[tokio::test]
    async fn save_internal_transfer_pair_same_currency_uses_source_amount() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-from", "USD"));
        account_service.add_account(create_test_account("acc-to", "USD"));

        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            asset_service,
            fx_service,
            Arc::new(MockQuoteService),
        );

        let result = activity_service
            .save_internal_transfer_pair(InternalTransferPairRequest {
                transfer_out_id: None,
                transfer_in_id: None,
                source_group_id: None,
                from_account_id: "acc-from".to_string(),
                to_account_id: "acc-to".to_string(),
                activity_date: "2026-06-03T20:20:00Z".to_string(),
                source_amount: Some(dec!(100)),
                destination_amount: Some(dec!(90)),
                source_currency: "HKD".to_string(),
                destination_currency: "HKD".to_string(),
                fx_rate: Some(dec!(0.9)),
                notes: None,
                transfer_mode: Some("cash".to_string()),
            })
            .await
            .expect("pair create should succeed");

        assert_eq!(result.transfer_out.amount, Some(dec!(100)));
        assert_eq!(result.transfer_in.amount, Some(dec!(100)));
        assert_eq!(result.transfer_out.currency, "HKD");
        assert_eq!(result.transfer_in.currency, "HKD");
        assert_eq!(result.transfer_in.fx_rate, None);
    }

    #[tokio::test]
    async fn save_internal_transfer_pair_update_preserves_leg_currencies_and_lifecycle() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-twd", "TWD"));
        account_service.add_account(create_test_account("acc-usd", "USD"));

        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            asset_service,
            fx_service,
            Arc::new(MockQuoteService),
        );

        let created = activity_service
            .save_internal_transfer_pair(InternalTransferPairRequest {
                transfer_out_id: None,
                transfer_in_id: None,
                source_group_id: None,
                from_account_id: "acc-twd".to_string(),
                to_account_id: "acc-usd".to_string(),
                activity_date: "2026-06-03T20:20:00Z".to_string(),
                source_amount: Some(dec!(1000)),
                destination_amount: Some(dec!(31.20)),
                source_currency: "TWD".to_string(),
                destination_currency: "USD".to_string(),
                fx_rate: Some(dec!(0.0312)),
                notes: None,
                transfer_mode: Some("cash".to_string()),
            })
            .await
            .expect("pair create should succeed");

        {
            let mut activities = activity_repository.activities.lock().unwrap();
            for activity in activities.iter_mut() {
                if activity.id == created.transfer_out.id {
                    activity.status = ActivityStatus::Draft;
                    activity.needs_review = true;
                    activity.fee = Some(dec!(3));
                    activity.fx_rate = Some(dec!(2));
                } else if activity.id == created.transfer_in.id {
                    activity.status = ActivityStatus::Draft;
                    activity.needs_review = true;
                    activity.fee = Some(dec!(2));
                }
            }
        }

        let updated = activity_service
            .save_internal_transfer_pair(InternalTransferPairRequest {
                transfer_out_id: Some(created.transfer_out.id.clone()),
                transfer_in_id: Some(created.transfer_in.id.clone()),
                source_group_id: None,
                from_account_id: "acc-twd".to_string(),
                to_account_id: "acc-usd".to_string(),
                activity_date: "2026-06-04T20:20:00Z".to_string(),
                source_amount: Some(dec!(2000)),
                destination_amount: Some(dec!(62.40)),
                source_currency: "TWD".to_string(),
                destination_currency: "USD".to_string(),
                fx_rate: Some(dec!(0.0312)),
                notes: Some("Updated".to_string()),
                transfer_mode: Some("cash".to_string()),
            })
            .await
            .expect("pair update should succeed");

        assert_eq!(updated.transfer_out.currency, "TWD");
        assert_eq!(updated.transfer_out.amount, Some(dec!(2000)));
        assert_eq!(updated.transfer_in.currency, "USD");
        assert_eq!(updated.transfer_in.amount, Some(dec!(62.40)));
        assert_eq!(updated.transfer_out.status, ActivityStatus::Draft);
        assert_eq!(updated.transfer_in.status, ActivityStatus::Draft);
        assert!(updated.transfer_out.needs_review);
        assert!(updated.transfer_in.needs_review);
        assert_eq!(updated.transfer_out.fee, Some(dec!(3)));
        assert_eq!(updated.transfer_in.fee, Some(dec!(2)));
        assert_eq!(updated.transfer_out.fx_rate, Some(dec!(2)));
    }

    fn independent_transfer_fixture() -> (
        ActivityService,
        Arc<MockActivityRepository>,
        InternalTransferPairRequest,
    ) {
        let accounts = Arc::new(MockAccountService::new());
        accounts.add_account(create_test_account("from", "USD"));
        accounts.add_account(create_test_account("to", "USD"));
        accounts.add_account(create_test_account("other", "EUR"));
        let repository = Arc::new(MockActivityRepository::new());
        let service = ActivityService::new(
            repository.clone(),
            accounts,
            Arc::new(MockAssetService::new()),
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );
        let request = InternalTransferPairRequest {
            transfer_out_id: None,
            transfer_in_id: None,
            source_group_id: None,
            from_account_id: "from".into(),
            to_account_id: "to".into(),
            activity_date: "2026-06-03T20:20:00Z".into(),
            source_amount: Some(dec!(780)),
            destination_amount: Some(dec!(100)),
            source_currency: "HKD".into(),
            destination_currency: "USD".into(),
            fx_rate: Some(dec!(99)),
            notes: None,
            transfer_mode: Some("cash".into()),
        };
        (service, repository, request)
    }

    #[tokio::test]
    async fn independent_cash_transfer_normalizes_units_and_uses_amounts_without_balance_checks() {
        for (source, destination, expected_source, expected_amount) in [
            ("HKD", "USD", "HKD", dec!(780)),
            ("USD", "HKD", "USD", dec!(780)),
            ("HKD", "EUR", "HKD", dec!(780)),
            ("GBp", "USD", "GBP", dec!(7.8)),
            ("GBX", "GBP", "GBP", dec!(7.8)),
            ("POINTS", "USD", "POINTS", dec!(780)),
        ] {
            let (service, repository, mut request) = independent_transfer_fixture();
            request.source_currency = source.into();
            request.destination_currency = destination.into();
            let saved = service.save_internal_transfer_pair(request).await.unwrap();
            assert_eq!(saved.transfer_out.currency, expected_source);
            assert_eq!(saved.transfer_out.amount, Some(expected_amount));
            assert_eq!(
                saved.transfer_in.amount,
                Some(if expected_source == destination {
                    expected_amount
                } else {
                    dec!(100)
                })
            );
            assert!(saved.transfer_out.fx_rate.is_none());
            assert!(saved.transfer_in.fx_rate.is_none());
            // There are no deposits, balances or market quotes in this fixture.
            assert_eq!(repository.get_activities().unwrap().len(), 2);
        }
    }

    #[tokio::test]
    async fn independent_cash_transfer_preserves_valuation_overrides_until_their_pair_changes() {
        let (service, repository, mut request) = independent_transfer_fixture();
        let created = service
            .save_internal_transfer_pair(request.clone())
            .await
            .unwrap();
        for row in repository.activities.lock().unwrap().iter_mut() {
            row.fx_rate = Some(dec!(0.125));
        }
        request.transfer_out_id = Some(created.transfer_out.id);
        request.transfer_in_id = Some(created.transfer_in.id);
        request.notes = Some("notes only".into());
        let unchanged = service
            .save_internal_transfer_pair(request.clone())
            .await
            .unwrap();
        assert_eq!(unchanged.transfer_out.fx_rate, Some(dec!(0.125)));
        assert_eq!(unchanged.transfer_in.fx_rate, Some(dec!(0.125)));
        assert_eq!(unchanged.transfer_in.amount, Some(dec!(100)));
        request.source_currency = "GBP".into();
        request.to_account_id = "other".into();
        let changed = service.save_internal_transfer_pair(request).await.unwrap();
        assert!(changed.transfer_out.fx_rate.is_none());
        assert!(changed.transfer_in.fx_rate.is_none());
    }

    #[tokio::test]
    async fn independent_cash_transfer_rejects_single_and_explicit_bulk_currency_changes_atomically(
    ) {
        let (service, repository, request) = independent_transfer_fixture();
        let created = service.save_internal_transfer_pair(request).await.unwrap();
        for account_change in [false, true] {
            let mut outgoing =
                create_test_activity_update(&created.transfer_out.id, "from", None, "HKD");
            outgoing.activity_type = "TRANSFER_OUT".into();
            if account_change {
                outgoing.account_id = "other".into();
            } else {
                outgoing.currency = "EUR".into();
            }
            assert!(service
                .update_activity(outgoing.clone())
                .await
                .unwrap_err()
                .to_string()
                .contains("pair editor"));
            let mut incoming =
                create_test_activity_update(&created.transfer_in.id, "to", None, "USD");
            incoming.activity_type = "TRANSFER_IN".into();
            incoming.amount = Some(Some(dec!(123)));
            let result = service
                .bulk_mutate_activities(ActivityBulkMutationRequest {
                    creates: vec![],
                    updates: vec![outgoing, incoming],
                    delete_ids: vec![],
                })
                .await
                .unwrap();
            assert!(!result.errors.is_empty());
            assert!(result.updated.is_empty());
        }
        assert_eq!(
            repository
                .get_activity(&created.transfer_in.id)
                .unwrap()
                .amount,
            Some(dec!(100))
        );
        assert_eq!(
            repository
                .get_activity(&created.transfer_out.id)
                .unwrap()
                .currency,
            "HKD"
        );
    }

    #[tokio::test]
    async fn pair_editor_converts_a_cash_pair_to_a_securities_transfer_in_one_bulk_update() {
        let accounts = Arc::new(MockAccountService::new());
        accounts.add_account(create_test_account("from", "USD"));
        accounts.add_account(create_test_account("to", "USD"));
        accounts.add_account(create_test_account("other", "EUR"));
        let assets = Arc::new(MockAssetService::new());
        assets.add_asset(create_test_asset("asset-aapl", "USD"));
        let repository = Arc::new(MockActivityRepository::new());
        let service = ActivityService::new(
            repository.clone(),
            accounts,
            assets,
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );
        // A cash pair in a currency neither account uses, which only the new
        // independent currencies allow.
        let (_, _, request) = independent_transfer_fixture();
        let created = service
            .save_internal_transfer_pair(InternalTransferPairRequest {
                destination_currency: "HKD".into(),
                destination_amount: Some(dec!(780)),
                ..request
            })
            .await
            .unwrap();

        // The pair editor's securities save: both legs, in their account
        // currencies, with the incoming leg moved to another account.
        let securities_leg = |id: &str, account: &str, currency: &str, activity_type: &str| {
            let mut update = create_test_activity_update(
                id,
                account,
                Some(AssetResolutionInput {
                    id: Some("asset-aapl".into()),
                    ..Default::default()
                }),
                currency,
            );
            update.activity_type = activity_type.into();
            update.quantity = Some(Some(dec!(10)));
            update
        };
        let result = service
            .bulk_mutate_activities(ActivityBulkMutationRequest {
                creates: vec![],
                updates: vec![
                    securities_leg(&created.transfer_out.id, "from", "USD", "TRANSFER_OUT"),
                    securities_leg(&created.transfer_in.id, "other", "EUR", "TRANSFER_IN"),
                ],
                delete_ids: vec![],
            })
            .await
            .unwrap();
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        let transfer_out = repository.get_activity(&created.transfer_out.id).unwrap();
        let transfer_in = repository.get_activity(&created.transfer_in.id).unwrap();
        assert_eq!(transfer_out.asset_id.as_deref(), Some("asset-aapl"));
        assert_eq!(transfer_in.asset_id.as_deref(), Some("asset-aapl"));
        assert_eq!(transfer_in.account_id, "other");
        assert_eq!(transfer_in.currency, "EUR");

        // A single leg is still not converted on its own.
        let created = service
            .save_internal_transfer_pair(InternalTransferPairRequest {
                activity_date: "2026-06-04T20:20:00Z".into(),
                ..independent_transfer_fixture().2
            })
            .await
            .unwrap();
        let result = service
            .bulk_mutate_activities(ActivityBulkMutationRequest {
                creates: vec![],
                updates: vec![securities_leg(
                    &created.transfer_in.id,
                    "other",
                    "EUR",
                    "TRANSFER_IN",
                )],
                delete_ids: vec![],
            })
            .await
            .unwrap();
        assert!(result.errors[0].message.contains("pair editor"));
    }

    #[tokio::test]
    async fn independent_cash_transfer_amount_edits_ignore_valuation_rates_and_preserve_explicit_legs(
    ) {
        let (service, repository, request) = independent_transfer_fixture();
        let created = service.save_internal_transfer_pair(request).await.unwrap();
        for row in repository.activities.lock().unwrap().iter_mut() {
            row.fx_rate = Some(dec!(7.8));
        }
        let mut outgoing =
            create_test_activity_update(&created.transfer_out.id, "from", None, "HKD");
        outgoing.activity_type = "TRANSFER_OUT".into();
        outgoing.amount = Some(Some(dec!(1560)));
        service.update_activity(outgoing.clone()).await.unwrap();
        assert_eq!(
            repository
                .get_activity(&created.transfer_in.id)
                .unwrap()
                .amount,
            Some(dec!(200))
        );
        let mut incoming = create_test_activity_update(&created.transfer_in.id, "to", None, "USD");
        incoming.activity_type = "TRANSFER_IN".into();
        incoming.amount = Some(Some(dec!(200)));
        incoming.fx_rate = Some(Some(dec!(123)));
        service.update_activity(incoming.clone()).await.unwrap();
        assert_eq!(
            repository
                .get_activity(&created.transfer_out.id)
                .unwrap()
                .amount,
            Some(dec!(1560))
        );
        incoming.amount = Some(Some(dec!(50)));
        service.update_activity(incoming.clone()).await.unwrap();
        assert_eq!(
            repository
                .get_activity(&created.transfer_out.id)
                .unwrap()
                .amount,
            Some(dec!(390))
        );
        outgoing.amount = Some(Some(dec!(2000)));
        incoming.amount = Some(Some(dec!(300)));
        let result = service
            .bulk_mutate_activities(ActivityBulkMutationRequest {
                creates: vec![],
                updates: vec![outgoing, incoming],
                delete_ids: vec![],
            })
            .await
            .unwrap();
        assert!(result.errors.is_empty());
        assert_eq!(
            repository
                .get_activity(&created.transfer_out.id)
                .unwrap()
                .amount,
            Some(dec!(2000))
        );
        assert_eq!(
            repository
                .get_activity(&created.transfer_in.id)
                .unwrap()
                .amount,
            Some(dec!(300))
        );
    }

    #[tokio::test]
    async fn legacy_pair_with_a_stored_execution_rate_keeps_it_through_leg_edits() {
        let (service, repository, request) = independent_transfer_fixture();
        let created = service
            .save_internal_transfer_pair(InternalTransferPairRequest {
                to_account_id: "other".into(),
                source_amount: Some(dec!(1000)),
                destination_amount: Some(dec!(920)),
                source_currency: "USD".into(),
                destination_currency: "EUR".into(),
                ..request
            })
            .await
            .unwrap();
        // Saved before independent currencies: each leg in its account's
        // currency, with the execution rate on the incoming leg.
        for row in repository.activities.lock().unwrap().iter_mut() {
            if row.id == created.transfer_in.id {
                row.fx_rate = Some(dec!(0.92));
            }
        }
        let mut outgoing =
            create_test_activity_update(&created.transfer_out.id, "from", None, "USD");
        outgoing.activity_type = "TRANSFER_OUT".into();
        outgoing.activity_date = "2026-06-03T20:20:00Z".into();
        outgoing.quantity = None;
        outgoing.unit_price = None;
        outgoing.fee = None;
        outgoing.amount = None;
        outgoing.notes = Some("notes only".into());
        service.update_activity(outgoing.clone()).await.unwrap();
        let incoming = repository.get_activity(&created.transfer_in.id).unwrap();
        assert_eq!(incoming.amount, Some(dec!(920)));
        assert_eq!(incoming.fx_rate, Some(dec!(0.92)));
        assert_eq!(
            repository
                .get_activity(&created.transfer_out.id)
                .unwrap()
                .amount,
            Some(dec!(1000))
        );

        outgoing.amount = Some(Some(dec!(2000)));
        service.update_activity(outgoing).await.unwrap();
        let incoming = repository.get_activity(&created.transfer_in.id).unwrap();
        assert_eq!(incoming.amount, Some(dec!(1840)));
        assert_eq!(incoming.fx_rate, Some(dec!(0.92)));
    }

    #[tokio::test]
    async fn independent_cash_transfer_invalid_legacy_amount_does_not_partially_update() {
        let (service, repository, request) = independent_transfer_fixture();
        let created = service.save_internal_transfer_pair(request).await.unwrap();
        repository
            .activities
            .lock()
            .unwrap()
            .iter_mut()
            .find(|row| row.id == created.transfer_out.id)
            .unwrap()
            .amount = Some(Decimal::ZERO);
        let mut update = create_test_activity_update(&created.transfer_out.id, "from", None, "HKD");
        update.activity_type = "TRANSFER_OUT".into();
        update.amount = Some(Some(dec!(1000)));
        let result = service
            .bulk_mutate_activities(ActivityBulkMutationRequest {
                creates: vec![],
                updates: vec![update],
                delete_ids: vec![],
            })
            .await
            .unwrap();
        assert_eq!(result.errors.len(), 1);
        assert!(result.updated.is_empty());
        assert_eq!(
            repository
                .get_activity(&created.transfer_in.id)
                .unwrap()
                .amount,
            Some(dec!(100))
        );
    }

    #[tokio::test]
    async fn delete_internal_transfer_pair_deletes_both_legs_and_emits_both_accounts() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        let event_sink = Arc::new(MockDomainEventSink::new());

        let mut transfer_out = create_stored_activity("transfer-out", "acc-out", None);
        transfer_out.activity_type = "TRANSFER_OUT".to_string();
        transfer_out.source_group_id = Some("transfer-group".to_string());
        transfer_out.metadata = Some(json!({ "flow": { "is_external": false } }));
        transfer_out.currency = "USD".to_string();
        let mut transfer_in = create_stored_activity("transfer-in", "acc-in", None);
        transfer_in.activity_type = "TRANSFER_IN".to_string();
        transfer_in.source_group_id = Some("transfer-group".to_string());
        transfer_in.metadata = Some(json!({ "flow": { "is_external": false } }));
        transfer_in.currency = "CAD".to_string();
        activity_repository.add_activity(transfer_out);
        activity_repository.add_activity(transfer_in);

        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            asset_service,
            fx_service,
            Arc::new(MockQuoteService),
        )
        .with_event_sink(event_sink.clone());

        let deleted = activity_service
            .delete_activity("transfer-out".to_string())
            .await
            .expect("delete should cascade");

        assert_eq!(deleted.id, "transfer-out");
        assert!(activity_repository.get_activities().unwrap().is_empty());
        let events = event_sink.events();
        assert_eq!(events.len(), 1);
        match &events[0] {
            DomainEvent::ActivitiesChanged {
                account_ids,
                currencies,
                ..
            } => {
                let mut account_ids = account_ids.clone();
                account_ids.sort();
                assert_eq!(account_ids, vec!["acc-in", "acc-out"]);
                let mut currencies = currencies.clone();
                currencies.sort();
                assert_eq!(currencies, vec!["CAD", "USD"]);
            }
            event => panic!("expected ActivitiesChanged event, got {event:?}"),
        }
    }

    #[tokio::test]
    async fn delete_group_with_extra_non_transfer_row_does_not_cascade() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        let mut transfer_out = create_stored_activity("transfer-out", "acc-out", None);
        transfer_out.activity_type = "TRANSFER_OUT".to_string();
        transfer_out.source_group_id = Some("provider-group".to_string());
        transfer_out.metadata = Some(json!({ "flow": { "is_external": false } }));
        let mut transfer_in = create_stored_activity("transfer-in", "acc-in", None);
        transfer_in.activity_type = "TRANSFER_IN".to_string();
        transfer_in.source_group_id = Some("provider-group".to_string());
        transfer_in.metadata = Some(json!({ "flow": { "is_external": false } }));
        let mut dividend = create_stored_activity("dividend", "acc-in", None);
        dividend.activity_type = "DIVIDEND".to_string();
        dividend.source_group_id = Some("provider-group".to_string());
        activity_repository.add_activity(transfer_out);
        activity_repository.add_activity(transfer_in);
        activity_repository.add_activity(dividend);

        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            asset_service,
            fx_service,
            Arc::new(MockQuoteService),
        );

        activity_service
            .delete_activity("transfer-out".to_string())
            .await
            .expect("single delete should succeed");

        let remaining: HashSet<String> = activity_repository
            .get_activities()
            .unwrap()
            .into_iter()
            .map(|activity| activity.id)
            .collect();
        assert_eq!(
            remaining,
            HashSet::from(["transfer-in".to_string(), "dividend".to_string()])
        );
    }

    #[tokio::test]
    async fn bulk_delete_expands_valid_transfer_pair() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        let mut transfer_out = create_stored_activity("transfer-out", "acc-out", None);
        transfer_out.activity_type = "TRANSFER_OUT".to_string();
        transfer_out.source_group_id = Some("transfer-group".to_string());
        transfer_out.metadata = Some(json!({ "flow": { "is_external": false } }));
        let mut transfer_in = create_stored_activity("transfer-in", "acc-in", None);
        transfer_in.activity_type = "TRANSFER_IN".to_string();
        transfer_in.source_group_id = Some("transfer-group".to_string());
        transfer_in.metadata = Some(json!({ "flow": { "is_external": false } }));
        activity_repository.add_activity(transfer_out);
        activity_repository.add_activity(transfer_in);

        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            Arc::new(MockQuoteService),
        );

        let result = activity_service
            .bulk_mutate_activities(ActivityBulkMutationRequest {
                creates: vec![],
                updates: vec![],
                delete_ids: vec!["transfer-in".to_string()],
            })
            .await
            .expect("bulk delete should succeed");

        let mut deleted_ids: Vec<String> = result
            .deleted
            .into_iter()
            .map(|activity| activity.id)
            .collect();
        deleted_ids.sort();
        assert_eq!(deleted_ids, vec!["transfer-in", "transfer-out"]);
    }

    #[tokio::test]
    async fn bulk_cross_currency_pair_amount_update_uses_existing_amounts_without_fx() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        account_service.add_account(create_test_account("acc-usd", "USD"));
        account_service.add_account(create_test_account("acc-eur", "EUR"));

        let mut transfer_out = create_stored_activity("transfer-out", "acc-usd", None);
        transfer_out.activity_type = "TRANSFER_OUT".to_string();
        transfer_out.source_group_id = Some("transfer-group".to_string());
        transfer_out.metadata = Some(json!({ "flow": { "is_external": false } }));
        transfer_out.currency = "USD".to_string();
        transfer_out.amount = Some(dec!(100));
        let mut transfer_in = create_stored_activity("transfer-in", "acc-eur", None);
        transfer_in.activity_type = "TRANSFER_IN".to_string();
        transfer_in.source_group_id = Some("transfer-group".to_string());
        transfer_in.metadata = Some(json!({ "flow": { "is_external": false } }));
        transfer_in.currency = "EUR".to_string();
        transfer_in.amount = Some(dec!(98));
        transfer_in.fx_rate = None;
        activity_repository.add_activity(transfer_out);
        activity_repository.add_activity(transfer_in);

        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            Arc::new(MockQuoteService),
        );
        let mut update = create_test_activity_update("transfer-out", "acc-usd", None, "USD");
        update.activity_type = "TRANSFER_OUT".to_string();
        update.amount = Some(Some(dec!(110)));

        let result = activity_service
            .bulk_mutate_activities(ActivityBulkMutationRequest {
                creates: vec![],
                updates: vec![update],
                delete_ids: vec![],
            })
            .await
            .expect("bulk mutation should return structured errors");

        assert!(result.errors.is_empty());
        assert_eq!(result.updated.len(), 2);
        let incoming = result
            .updated
            .iter()
            .find(|row| row.id == "transfer-in")
            .unwrap();
        assert_eq!(incoming.amount, Some(dec!(107.8)));
    }

    #[tokio::test]
    async fn bulk_update_event_uses_old_activity_date_when_moved_later() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        let event_sink = Arc::new(MockDomainEventSink::new());

        account_service.add_account(create_test_account("acc-usd", "USD"));

        let old_date = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let new_date = DateTime::parse_from_rfc3339("2026-02-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut activity = create_stored_activity("cash-activity", "acc-usd", None);
        activity.activity_type = "DEPOSIT".to_string();
        activity.activity_date = old_date;
        activity.quantity = None;
        activity.unit_price = None;
        activity.amount = Some(dec!(100));
        activity_repository.add_activity(activity);

        let activity_service = ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            Arc::new(MockQuoteService),
        )
        .with_event_sink(event_sink.clone());

        activity_service
            .bulk_mutate_activities(ActivityBulkMutationRequest {
                creates: vec![],
                updates: vec![ActivityUpdate {
                    id: "cash-activity".to_string(),
                    account_id: "acc-usd".to_string(),
                    asset: None,
                    activity_type: "DEPOSIT".to_string(),
                    subtype: None,
                    activity_date: new_date.to_rfc3339(),
                    quantity: Some(None),
                    unit_price: Some(None),
                    currency: "USD".to_string(),
                    fee: Some(Some(dec!(0))),
                    tax: None,
                    amount: Some(Some(dec!(125))),
                    status: None,
                    needs_review: None,
                    notes: None,
                    fx_rate: None,
                    metadata: None,
                }],
                delete_ids: vec![],
            })
            .await
            .expect("bulk update should succeed");

        let events = event_sink.events();
        assert_eq!(events.len(), 1);
        match &events[0] {
            DomainEvent::ActivitiesChanged {
                earliest_activity_at_utc,
                ..
            } => assert_eq!(*earliest_activity_at_utc, Some(old_date)),
            event => panic!("expected ActivitiesChanged event, got {event:?}"),
        }
    }

    // ==========================================================================
    // Currency normalization tests (GBp -> GBP, etc.)
    // ==========================================================================

    /// GBp is normalized to GBP with amount conversion. Unit price and amounts
    /// are multiplied by 0.01, and fees follow the same conversion.
    #[tokio::test]
    async fn test_gbp_pence_normalization_on_create() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        // Create GBP account
        let account = create_test_account("acc-1", "GBP");
        account_service.add_account(account);

        // LSE stock with GBp currency
        let asset = create_test_asset("SEC:AZN:XLON", "GBp");
        asset_service.add_asset(asset);

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        // User submits activity in GBp (pence) - 14082 pence per share
        let new_activity = NewActivity {
            id: Some("activity-1".to_string()),
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                id: Some("SEC:AZN:XLON".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: Some(dec!(10)),
            unit_price: Some(dec!(14082)), // 14082 pence
            currency: "GBp".to_string(),   // Pence currency
            fee: Some(dec!(999)),          // 999 pence fee
            tax: None,
            amount: Some(dec!(140820)), // 140820 pence total
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let result = activity_service.create_activity(new_activity).await;
        assert!(result.is_ok(), "Activity creation should succeed");

        let created = result.unwrap();

        // Currency should be normalized to GBP
        assert_eq!(
            created.currency, "GBP",
            "Currency should be normalized from GBp to GBP"
        );

        // Unit price should be converted: 14082 pence * 0.01 = 140.82 GBP
        assert_eq!(
            created.unit_price,
            Some(dec!(140.82)),
            "Unit price should be converted from pence to pounds"
        );

        // Fee should be converted: 999 pence * 0.01 = 9.99 GBP
        assert_eq!(
            created.fee,
            Some(dec!(9.99)),
            "Fee should be converted from pence to pounds"
        );

        // Amount converted (140820 pence -> 1408.20 GBP) matches the derived
        // gross exactly, so the writer converts it to final: 1408.20 + 9.99.
        assert_eq!(
            created.amount,
            Some(dec!(1418.19)),
            "Gross-matching amount should be normalized and converted to final"
        );

        // Quantity should NOT be converted (shares, not currency)
        assert_eq!(
            created.quantity,
            Some(dec!(10)),
            "Quantity should remain unchanged"
        );
    }

    /// Test: Activity with GBX currency (alternative pence code) is also normalized
    #[tokio::test]
    async fn test_gbx_normalization_on_create() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        let account = create_test_account("acc-1", "GBP");
        account_service.add_account(account);

        let asset = create_test_asset("SEC:VOD:XLON", "GBX");
        asset_service.add_asset(asset);

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let new_activity = NewActivity {
            id: Some("activity-1".to_string()),
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                id: Some("SEC:VOD:XLON".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: Some(dec!(100)),
            unit_price: Some(dec!(7500)), // 7500 pence
            currency: "GBX".to_string(),  // Alternative pence code
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(750000)), // 750000 pence
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let result = activity_service.create_activity(new_activity).await;
        assert!(result.is_ok());

        let created = result.unwrap();
        assert_eq!(created.currency, "GBP", "GBX should normalize to GBP");
        assert_eq!(created.unit_price, Some(dec!(75)), "7500 pence = 75 pounds");
        assert_eq!(
            created.amount,
            Some(dec!(7500)),
            "750000 pence = 7500 pounds"
        );
    }

    /// Test: Activity with ZAc (South African cents) is normalized to ZAR
    #[tokio::test]
    async fn test_zac_normalization_on_create() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        let account = create_test_account("acc-1", "ZAR");
        account_service.add_account(account);

        let asset = create_test_asset("SEC:NPN:XJSE", "ZAc");
        asset_service.add_asset(asset);

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let new_activity = NewActivity {
            id: Some("activity-1".to_string()),
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                id: Some("SEC:NPN:XJSE".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: Some(dec!(50)),
            unit_price: Some(dec!(200000)), // 200000 cents = 2000 ZAR
            currency: "ZAc".to_string(),
            fee: Some(dec!(1000)), // 1000 cents = 10 ZAR
            tax: None,
            amount: Some(dec!(10000000)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let result = activity_service.create_activity(new_activity).await;
        assert!(result.is_ok());

        let created = result.unwrap();
        assert_eq!(created.currency, "ZAR", "ZAc should normalize to ZAR");
        assert_eq!(
            created.unit_price,
            Some(dec!(2000)),
            "200000 cents = 2000 ZAR"
        );
        assert_eq!(created.fee, Some(dec!(10)), "1000 cents = 10 ZAR");
    }

    /// Test: Activity with regular GBP currency is NOT modified
    #[tokio::test]
    async fn test_regular_gbp_not_modified() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        let account = create_test_account("acc-1", "GBP");
        account_service.add_account(account);

        let asset = create_test_asset("SEC:LLOY:XLON", "GBP");
        asset_service.add_asset(asset);

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        let new_activity = NewActivity {
            id: Some("activity-1".to_string()),
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                id: Some("SEC:LLOY:XLON".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: Some(dec!(1000)),
            unit_price: Some(dec!(0.45)), // Already in GBP
            currency: "GBP".to_string(),  // Major currency
            fee: Some(dec!(5)),
            tax: None,
            amount: Some(dec!(450)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let result = activity_service.create_activity(new_activity).await;
        assert!(result.is_ok());

        let created = result.unwrap();
        assert_eq!(created.currency, "GBP", "GBP should remain GBP");
        assert_eq!(
            created.unit_price,
            Some(dec!(0.45)),
            "Unit price should not change for GBP"
        );
        // 450 matches the derived gross (1000 x 0.45) exactly, so the writer
        // converts it to final by adding the fee; no pence scaling happens.
        assert_eq!(
            created.amount,
            Some(dec!(455)),
            "Gross-matching amount should convert to final without scaling"
        );
        assert_eq!(created.fee, Some(dec!(5)), "Fee should not change for GBP");
    }

    // --- Bond instrument type tests ---

    // --- Option instrument type tests ---

    /// Test: OCC symbol pattern (e.g. AAPL240119C00150000) infers OPTION kind
    /// and matches against an existing option asset.
    #[tokio::test]
    async fn test_infer_asset_kind_occ_symbol() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());

        let account = create_test_account("acc-1", "USD");
        account_service.add_account(account);

        // Add an option asset matching the OCC symbol
        let asset = create_test_asset_with_instrument(
            "aapl-opt-uuid",
            "AAPL240119C00150000",
            None,
            Some(InstrumentType::Option),
            "USD",
        );
        asset_service.add_asset(asset);

        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            asset_service,
            fx_service,
            quote_service,
        );

        // OCC symbol with no explicit kind input — should be inferred as OPTION
        let new_activity = NewActivity {
            id: Some("activity-occ-1".to_string()),
            account_id: "acc-1".to_string(),
            asset: Some(AssetResolutionInput {
                symbol: Some("AAPL240119C00150000".to_string()),
                ..Default::default()
            }),
            activity_type: "BUY".to_string(),
            subtype: None,
            activity_date: "2024-01-15".to_string(),
            quantity: Some(dec!(2)),
            unit_price: Some(dec!(5)),
            currency: "USD".to_string(),
            fee: Some(dec!(0)),
            tax: None,
            amount: Some(dec!(10)),
            status: None,
            notes: None,
            fx_rate: None,
            metadata: None,
            needs_review: None,
            source_system: None,
            source_record_id: None,
            source_group_id: None,
            idempotency_key: None,
        };

        let result = activity_service.create_activity(new_activity).await;
        assert!(result.is_ok(), "Expected Ok, got {:?}", result);

        let created = result.unwrap();
        assert_eq!(
            created.asset_id,
            Some("aapl-opt-uuid".to_string()),
            "OCC symbol should match existing option asset"
        );
    }

    // ── Transfer pair sync ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_update_activity_propagates_to_transfer_counterpart() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        let event_sink = Arc::new(MockDomainEventSink::new());
        let quote_service = Arc::new(MockQuoteService);
        account_service.add_account(create_test_account("acc-out", "USD"));
        account_service.add_account(create_test_account("acc-in", "USD"));

        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            asset_service,
            fx_service,
            quote_service,
        )
        .with_event_sink(event_sink.clone());

        let date_original = DateTime::parse_from_rfc3339("2024-01-15T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let date_updated = DateTime::parse_from_rfc3339("2024-02-01T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        activity_repository.activities.lock().unwrap().extend([
            Activity {
                id: "transfer-out".to_string(),
                account_id: "acc-out".to_string(),
                asset_id: None,
                activity_type: "TRANSFER_OUT".to_string(),
                activity_type_override: None,
                source_type: None,
                subtype: None,
                status: ActivityStatus::Draft,
                activity_date: date_original,
                settlement_date: None,
                quantity: None,
                unit_price: None,
                amount: Some(dec!(500)),
                fee: Some(dec!(0)),
                tax: None,
                currency: "USD".to_string(),
                fx_rate: None,
                notes: None,
                metadata: Some(json!({ "flow": { "is_external": false } })),
                source_system: None,
                source_record_id: None,
                source_group_id: Some("grp-1".to_string()),
                idempotency_key: None,
                import_run_id: None,
                is_user_modified: false,
                needs_review: true,
                created_at: date_original,
                updated_at: date_original,
            },
            Activity {
                id: "transfer-in".to_string(),
                account_id: "acc-in".to_string(),
                asset_id: None,
                activity_type: "TRANSFER_IN".to_string(),
                activity_type_override: None,
                source_type: None,
                subtype: None,
                status: ActivityStatus::Draft,
                activity_date: date_original,
                settlement_date: None,
                quantity: None,
                unit_price: None,
                amount: Some(dec!(500)),
                fee: Some(dec!(0)),
                tax: None,
                currency: "USD".to_string(),
                fx_rate: None,
                notes: None,
                metadata: Some(json!({ "flow": { "is_external": false } })),
                source_system: None,
                source_record_id: None,
                source_group_id: Some("grp-1".to_string()),
                idempotency_key: None,
                import_run_id: None,
                is_user_modified: false,
                needs_review: true,
                created_at: date_original,
                updated_at: date_original,
            },
        ]);

        let update = crate::activities::ActivityUpdate {
            id: "transfer-out".to_string(),
            account_id: "acc-out".to_string(),
            asset: None,
            activity_type: "TRANSFER_OUT".to_string(),
            subtype: None,
            activity_date: date_updated.to_rfc3339(),
            quantity: None,
            unit_price: None,
            currency: "USD".to_string(),
            fee: None,
            tax: None,
            amount: Some(Some(dec!(750))),
            status: Some(ActivityStatus::Posted),
            needs_review: Some(false),
            notes: Some("moved funds".to_string()),
            fx_rate: None,
            metadata: None,
        };

        activity_service
            .update_activity(update)
            .await
            .expect("update should succeed");

        let stored = activity_repository.activities.lock().unwrap().clone();

        let counterpart = stored
            .iter()
            .find(|a| a.id == "transfer-in")
            .expect("transfer-in should still exist");

        assert_eq!(counterpart.amount, Some(dec!(750)), "amount propagated");
        assert_eq!(counterpart.activity_date, date_updated, "date propagated");
        assert_eq!(
            counterpart.notes,
            Some("moved funds".to_string()),
            "notes propagated"
        );
        assert_eq!(counterpart.account_id, "acc-in", "account_id not changed");
        assert_eq!(
            counterpart.status,
            ActivityStatus::Posted,
            "status propagated"
        );
        assert!(!counterpart.needs_review, "review approval propagated");
        assert_eq!(
            counterpart.activity_type, "TRANSFER_IN",
            "activity_type not changed"
        );
    }

    #[tokio::test]
    async fn update_leaves_other_activities_of_its_source_group_alone() {
        // Broker sync can group rows that are not a linked transfer pair,
        // such as a deposit and its fee: editing one must not copy onto the
        // other.
        let account_service = Arc::new(MockAccountService::new());
        account_service.add_account(create_test_account("acc-1", "USD"));
        let activity_repository = Arc::new(MockActivityRepository::new());
        let mut deposit = create_stored_activity("deposit-1", "acc-1", None);
        deposit.activity_type = "DEPOSIT".to_string();
        deposit.quantity = None;
        deposit.unit_price = None;
        deposit.source_group_id = Some("broker-group".to_string());
        let mut fee = deposit.clone();
        fee.id = "fee-1".to_string();
        fee.activity_type = "FEE".to_string();
        fee.amount = Some(dec!(5));
        fee.notes = Some("broker fee".to_string());
        fee.activity_date = DateTime::parse_from_rfc3339("2024-01-10T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        activity_repository.add_activity(deposit);
        activity_repository.add_activity(fee);
        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            Arc::new(MockAssetService::new()),
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );
        let stored_fee =
            || serde_json::to_value(activity_repository.get_activity("fee-1").unwrap()).unwrap();
        let fee_before = stored_fee();

        let updated = activity_service
            .update_activity(ActivityUpdate {
                id: "deposit-1".to_string(),
                account_id: "acc-1".to_string(),
                asset: None,
                activity_type: "DEPOSIT".to_string(),
                subtype: None,
                activity_date: "2024-02-01T00:00:00Z".to_string(),
                quantity: None,
                unit_price: None,
                currency: "EUR".to_string(),
                fee: None,
                tax: None,
                amount: Some(Some(dec!(150))),
                status: None,
                needs_review: None,
                notes: Some("corrected".to_string()),
                fx_rate: None,
                metadata: None,
            })
            .await
            .expect("update");

        assert_eq!(updated.amount, Some(dec!(150)));
        assert_eq!(stored_fee(), fee_before);
    }

    #[tokio::test]
    async fn ordinary_edit_leaves_draft_counterpart_lifecycle_alone() {
        // The grid echoes the primary's current status on every save. An edit
        // that changes nothing about the primary's lifecycle must neither fail
        // on nor mutate a Draft-in-review counterpart.
        let account_service = Arc::new(MockAccountService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-out", "USD"));
        account_service.add_account(create_test_account("acc-in", "USD"));
        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            Arc::new(MockAssetService::new()),
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let date = DateTime::parse_from_rfc3339("2024-01-15T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut transfer_out = create_stored_activity("transfer-out", "acc-out", None);
        transfer_out.activity_type = "TRANSFER_OUT".to_string();
        transfer_out.quantity = None;
        transfer_out.unit_price = None;
        transfer_out.amount = Some(dec!(500));
        transfer_out.source_group_id = Some("grp-mixed".to_string());
        transfer_out.activity_date = date;
        transfer_out.metadata = Some(json!({ "flow": { "is_external": false } }));
        let mut transfer_in = transfer_out.clone();
        transfer_in.id = "transfer-in".to_string();
        transfer_in.account_id = "acc-in".to_string();
        transfer_in.activity_type = "TRANSFER_IN".to_string();
        transfer_in.status = ActivityStatus::Draft;
        transfer_in.needs_review = true;
        activity_repository
            .activities
            .lock()
            .unwrap()
            .extend([transfer_out, transfer_in]);

        activity_service
            .update_activity(crate::activities::ActivityUpdate {
                id: "transfer-out".to_string(),
                account_id: "acc-out".to_string(),
                asset: None,
                activity_type: "TRANSFER_OUT".to_string(),
                subtype: None,
                activity_date: date.to_rfc3339(),
                quantity: None,
                unit_price: None,
                currency: "USD".to_string(),
                fee: None,
                tax: None,
                amount: Some(Some(dec!(600))),
                status: Some(ActivityStatus::Posted),
                needs_review: None,
                notes: Some("just a note".to_string()),
                fx_rate: None,
                metadata: None,
            })
            .await
            .expect("editing the posted leg must not trip the counterpart's review guard");

        let stored = activity_repository.activities.lock().unwrap().clone();
        let counterpart = stored
            .iter()
            .find(|a| a.id == "transfer-in")
            .expect("counterpart exists");
        assert_eq!(counterpart.status, ActivityStatus::Draft);
        assert!(counterpart.needs_review);
    }

    #[tokio::test]
    async fn approving_posted_leg_posts_and_clears_draft_counterpart() {
        let account_service = Arc::new(MockAccountService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        account_service.add_account(create_test_account("acc-out", "USD"));
        account_service.add_account(create_test_account("acc-in", "USD"));
        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            Arc::new(MockAssetService::new()),
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        );

        let date = DateTime::parse_from_rfc3339("2024-01-15T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let mut transfer_out = create_stored_activity("transfer-out", "acc-out", None);
        transfer_out.activity_type = "TRANSFER_OUT".to_string();
        transfer_out.quantity = None;
        transfer_out.unit_price = None;
        transfer_out.amount = Some(dec!(500));
        transfer_out.needs_review = true;
        transfer_out.source_group_id = Some("grp-approve".to_string());
        transfer_out.activity_date = date;
        transfer_out.metadata = Some(json!({ "flow": { "is_external": false } }));
        let mut transfer_in = transfer_out.clone();
        transfer_in.id = "transfer-in".to_string();
        transfer_in.account_id = "acc-in".to_string();
        transfer_in.activity_type = "TRANSFER_IN".to_string();
        transfer_in.status = ActivityStatus::Draft;
        activity_repository
            .activities
            .lock()
            .unwrap()
            .extend([transfer_out, transfer_in]);

        activity_service
            .update_activity(crate::activities::ActivityUpdate {
                id: "transfer-out".to_string(),
                account_id: "acc-out".to_string(),
                asset: None,
                activity_type: "TRANSFER_OUT".to_string(),
                subtype: None,
                activity_date: date.to_rfc3339(),
                quantity: None,
                unit_price: None,
                currency: "USD".to_string(),
                fee: None,
                tax: None,
                amount: Some(Some(dec!(500))),
                status: Some(ActivityStatus::Posted),
                needs_review: Some(false),
                notes: None,
                fx_rate: None,
                metadata: None,
            })
            .await
            .expect("approving one leg approves the pair");

        let stored = activity_repository.activities.lock().unwrap().clone();
        let counterpart = stored
            .iter()
            .find(|a| a.id == "transfer-in")
            .expect("counterpart exists");
        assert_eq!(counterpart.status, ActivityStatus::Posted);
        assert!(!counterpart.needs_review);
    }

    #[tokio::test]
    async fn test_delete_activity_cascades_transfer_pair() {
        let account_service = Arc::new(MockAccountService::new());
        let asset_service = Arc::new(MockAssetService::new());
        let fx_service = Arc::new(MockFxService::new());
        let activity_repository = Arc::new(MockActivityRepository::new());
        let event_sink = Arc::new(MockDomainEventSink::new());
        let quote_service = Arc::new(MockQuoteService);
        let activity_service = ActivityService::new(
            activity_repository.clone(),
            account_service,
            asset_service,
            fx_service,
            quote_service,
        )
        .with_event_sink(event_sink.clone());

        let date = DateTime::parse_from_rfc3339("2024-01-15T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        activity_repository.activities.lock().unwrap().extend([
            Activity {
                id: "transfer-out".to_string(),
                account_id: "acc-out".to_string(),
                asset_id: None,
                activity_type: "TRANSFER_OUT".to_string(),
                activity_type_override: None,
                source_type: None,
                subtype: None,
                status: ActivityStatus::Posted,
                activity_date: date,
                settlement_date: None,
                quantity: None,
                unit_price: None,
                amount: Some(dec!(100)),
                fee: Some(dec!(0)),
                tax: None,
                currency: "USD".to_string(),
                fx_rate: None,
                notes: None,
                metadata: Some(json!({ "flow": { "is_external": false } })),
                source_system: None,
                source_record_id: None,
                source_group_id: Some("grp-cascade".to_string()),
                idempotency_key: None,
                import_run_id: None,
                is_user_modified: false,
                needs_review: false,
                created_at: date,
                updated_at: date,
            },
            Activity {
                id: "transfer-in".to_string(),
                account_id: "acc-in".to_string(),
                asset_id: None,
                activity_type: "TRANSFER_IN".to_string(),
                activity_type_override: None,
                source_type: None,
                subtype: None,
                status: ActivityStatus::Posted,
                activity_date: date,
                settlement_date: None,
                quantity: None,
                unit_price: None,
                amount: Some(dec!(100)),
                fee: Some(dec!(0)),
                tax: None,
                currency: "USD".to_string(),
                fx_rate: None,
                notes: None,
                metadata: Some(json!({ "flow": { "is_external": false } })),
                source_system: None,
                source_record_id: None,
                source_group_id: Some("grp-cascade".to_string()),
                idempotency_key: None,
                import_run_id: None,
                is_user_modified: false,
                needs_review: false,
                created_at: date,
                updated_at: date,
            },
        ]);

        activity_service
            .delete_activity("transfer-out".to_string())
            .await
            .expect("delete should succeed");

        let stored = activity_repository.activities.lock().unwrap().clone();
        assert!(
            stored
                .iter()
                .all(|a| a.source_group_id.as_deref() != Some("grp-cascade")),
            "both transfer legs should be deleted"
        );
        assert_eq!(stored.len(), 0, "no activities should remain");

        // Both accounts must appear in the emitted event
        let events = event_sink.events();
        assert_eq!(events.len(), 1);
        match &events[0] {
            DomainEvent::ActivitiesChanged { account_ids, .. } => {
                let mut ids = account_ids.clone();
                ids.sort();
                assert_eq!(ids, vec!["acc-in", "acc-out"]);
            }
            event => panic!("expected ActivitiesChanged, got {event:?}"),
        }
    }

    fn seed_internal_cash_transfer_pair(
        activity_repository: &MockActivityRepository,
        group_id: &str,
    ) {
        for (id, account_id, activity_type) in [
            ("pair-out", "acc-a", "TRANSFER_OUT"),
            ("pair-in", "acc-b", "TRANSFER_IN"),
        ] {
            let mut activity = create_cash_transfer_activity(
                id,
                account_id,
                activity_type,
                "2024-01-17T00:00:00Z",
                dec!(100),
                "USD",
            );
            activity.source_group_id = Some(group_id.to_string());
            activity.metadata = Some(json!({ "flow": { "is_external": false } }));
            activity_repository.add_activity(activity);
        }
    }

    fn build_transfer_pair_service(
        activity_repository: Arc<MockActivityRepository>,
    ) -> ActivityService {
        ActivityService::new(
            activity_repository,
            Arc::new(MockAccountService::new()),
            Arc::new(MockAssetService::new()),
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        )
    }

    #[tokio::test]
    async fn test_get_transfer_pair_returns_pair_for_linked_activity() {
        let activity_repository = Arc::new(MockActivityRepository::new());
        seed_internal_cash_transfer_pair(&activity_repository, "group-internal");
        let activity_service = build_transfer_pair_service(activity_repository);

        let pair = activity_service
            .get_transfer_pair_for_activity("pair-in".to_string())
            .expect("lookup should succeed")
            .expect("linked activity should resolve to a pair");

        assert_eq!(pair.transfer_out.id, "pair-out");
        assert_eq!(pair.transfer_in.id, "pair-in");
    }

    #[tokio::test]
    async fn test_get_transfer_pair_returns_none_for_unpaired_activity() {
        let activity_repository = Arc::new(MockActivityRepository::new());
        activity_repository.add_activity(create_cash_transfer_activity(
            "lonely-out",
            "acc-a",
            "TRANSFER_OUT",
            "2024-01-17T00:00:00Z",
            dec!(100),
            "USD",
        ));
        let activity_service = build_transfer_pair_service(activity_repository);

        let pair = activity_service
            .get_transfer_pair_for_activity("lonely-out".to_string())
            .expect("unpaired activity should not be an error");

        assert!(pair.is_none());
    }

    #[tokio::test]
    async fn test_get_transfer_pair_returns_none_for_grouped_external_transfer() {
        let activity_repository = Arc::new(MockActivityRepository::new());
        for (id, account_id, activity_type) in [
            ("external-out", "acc-a", "TRANSFER_OUT"),
            ("external-in", "acc-b", "TRANSFER_IN"),
        ] {
            let mut activity = create_cash_transfer_activity(
                id,
                account_id,
                activity_type,
                "2024-01-17T00:00:00Z",
                dec!(100),
                "USD",
            );
            activity.source_group_id = Some("group-external".to_string());
            activity_repository.add_activity(activity);
        }
        let activity_service = build_transfer_pair_service(activity_repository);

        let pair = activity_service
            .get_transfer_pair_for_activity("external-in".to_string())
            .expect("external pair should not be an error");

        assert!(pair.is_none());
    }

    #[tokio::test]
    async fn test_get_transfer_pair_errors_for_unknown_activity() {
        let activity_repository = Arc::new(MockActivityRepository::new());
        let activity_service = build_transfer_pair_service(activity_repository);

        let result = activity_service.get_transfer_pair_for_activity("missing".to_string());

        assert!(result.is_err());
    }

    // ───────────────────────────────────────────────────────────────────
    // Bare activity dates (#1701). A bare `YYYY-MM-DD` is a calendar day and
    // is stored on that day in the configured timezone: UTC midnight, where it
    // used to land, falls on the previous day west of UTC.
    // ───────────────────────────────────────────────────────────────────

    fn bank_account() -> Account {
        let mut bank = create_test_account("bank-1", "USD");
        bank.account_type = "CASH".to_string();
        bank
    }

    fn dated_service(timezone: &str) -> ActivityService {
        let account_service = Arc::new(MockAccountService::new());
        account_service.add_account(bank_account());
        ActivityService::new(
            Arc::new(MockActivityRepository::new()),
            account_service,
            Arc::new(MockAssetService::new()),
            Arc::new(MockFxService::new()),
            Arc::new(MockQuoteService),
        )
        .with_timezone(Arc::new(std::sync::RwLock::new(timezone.to_string())))
    }

    fn dated_deposit(activity_date: &str) -> NewActivity {
        let mut deposit = create_test_cash_create("deposit", "bank-1", "DEPOSIT", "USD");
        deposit.activity_date = activity_date.to_string();
        deposit
    }

    fn instant(timestamp: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(timestamp)
            .unwrap()
            .with_timezone(&Utc)
    }

    /// The in-app assistant's batch confirm sends each draft's bare date to the
    /// bulk path. With Toronto configured it was stored at UTC midnight, which
    /// shows there as the day before.
    #[tokio::test]
    async fn bulk_create_stores_a_bare_date_on_its_day_in_the_configured_timezone() {
        let result = dated_service("America/Toronto")
            .bulk_mutate_activities(ActivityBulkMutationRequest {
                creates: vec![dated_deposit("2026-04-01")],
                updates: vec![],
                delete_ids: vec![],
            })
            .await
            .expect("bulk create");
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert_eq!(
            result.created[0].activity_date,
            instant("2026-04-01T04:00:00Z")
        );
    }

    #[tokio::test]
    async fn activity_dates_keep_timestamps_and_place_bare_dates_by_timezone() {
        for (timezone, submitted, stored) in [
            // Timestamps keep their instant, including the CSV wizard's
            // browser-local midnight.
            (
                "America/Toronto",
                "2026-04-01T09:30:00-04:00",
                "2026-04-01T13:30:00Z",
            ),
            (
                "America/Toronto",
                "2026-04-01T04:00:00.000Z",
                "2026-04-01T04:00:00Z",
            ),
            (
                "Europe/Paris",
                "2026-03-31T22:00:00.000Z",
                "2026-03-31T22:00:00Z",
            ),
            // West of UTC a bare date is stored at local midnight.
            ("America/Toronto", "2026-04-01", "2026-04-01T04:00:00Z"),
            ("Pacific/Honolulu", "2026-04-01", "2026-04-01T10:00:00Z"),
            // At or east of UTC, or with no timezone, at UTC midnight as before.
            ("Europe/Paris", "2026-04-01", "2026-04-01T00:00:00Z"),
            ("UTC", "2026-04-01", "2026-04-01T00:00:00Z"),
            ("", "2026-04-01", "2026-04-01T00:00:00Z"),
            // Surrounding whitespace is ignored whatever the timezone.
            ("America/Toronto", " 2026-04-01\n", "2026-04-01T04:00:00Z"),
            ("Europe/Paris", " 2026-04-01 ", "2026-04-01T00:00:00Z"),
            ("", "2026-04-01\n", "2026-04-01T00:00:00Z"),
            // Chile skips midnight on 2024-09-08: the day starts at 01:00 -03:00.
            ("America/Santiago", "2024-09-08", "2024-09-08T04:00:00Z"),
        ] {
            let created = dated_service(timezone)
                .create_activity(dated_deposit(submitted))
                .await
                .unwrap_or_else(|error| panic!("{timezone} {submitted:?}: {error}"));
            assert_eq!(
                created.activity_date,
                instant(stored),
                "{timezone} {submitted:?}"
            );
        }
    }

    // ───────────────────────────────────────────────────────────────────
    // Manual activity update validation
    // ───────────────────────────────────────────────────────────────────

    fn stored_snapshot(activity_repository: &MockActivityRepository) -> serde_json::Value {
        serde_json::to_value(&*activity_repository.activities.lock().unwrap()).unwrap()
    }

    fn preview_test_service(
        activity_repository: Arc<MockActivityRepository>,
        fx_service: Arc<MockFxService>,
    ) -> ActivityService {
        let account_service = Arc::new(MockAccountService::new());
        for (id, account_type) in [
            ("acc-1", "SECURITIES"),
            ("acc-a", "SECURITIES"),
            ("acc-b", "SECURITIES"),
            ("acc-card", crate::accounts::account_types::CREDIT_CARD),
        ] {
            let mut account = create_test_account(id, "USD");
            account.account_type = account_type.to_string();
            account_service.add_account(account);
        }
        let asset_service = Arc::new(MockAssetService::new());
        asset_service.add_asset(create_test_asset("NESN", "USD"));
        ActivityService::new(
            activity_repository,
            account_service,
            asset_service,
            fx_service,
            Arc::new(MockQuoteService),
        )
    }

    fn utc(timestamp: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(timestamp)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[tokio::test]
    async fn preview_activity_update_writes_nothing_that_the_update_writes() {
        // An EUR edit on a USD account: the update registers EUR/USD, the
        // preview only reports what would be stored.
        let activity_repository = Arc::new(MockActivityRepository::new());
        activity_repository.add_activity(create_stored_activity("buy-1", "acc-1", Some("NESN")));
        let fx_service = Arc::new(MockFxService::new());
        let activity_service =
            preview_test_service(activity_repository.clone(), fx_service.clone());
        let before = stored_snapshot(&activity_repository);

        let mut update = create_test_activity_update("buy-1", "acc-1", None, "EUR");
        update.fee = Some(Some(dec!(-2.5)));
        let preview = activity_service
            .preview_activity_update(update.clone())
            .expect("preview");

        let previewed = &preview.activity;
        assert_eq!(previewed.existing.id, "buy-1");
        assert_eq!(previewed.update.currency, "EUR");
        assert_eq!(
            previewed.update.fee,
            Some(Some(dec!(2.5))),
            "stored unsigned"
        );
        assert_eq!(
            previewed
                .update
                .asset
                .as_ref()
                .and_then(|asset| asset.id.as_deref()),
            Some("NESN"),
            "an omitted asset keeps the stored one"
        );
        assert_eq!(previewed.activity_date, utc("2024-01-15T00:00:00Z"));
        assert!(preview.linked.is_none());
        assert_eq!(stored_snapshot(&activity_repository), before);
        assert!(fx_service.get_registered_pairs().is_empty());

        activity_service
            .update_activity(update)
            .await
            .expect("update");
        assert!(fx_service
            .get_registered_pairs()
            .contains(&("EUR".to_string(), "USD".to_string())));
        assert_ne!(stored_snapshot(&activity_repository), before);
    }

    #[tokio::test]
    async fn preview_activity_update_places_a_bare_date_on_the_local_day() {
        let activity_repository = Arc::new(MockActivityRepository::new());
        activity_repository.add_activity(create_stored_activity("buy-1", "acc-1", Some("NESN")));
        let activity_service =
            preview_test_service(activity_repository, Arc::new(MockFxService::new()))
                .with_timezone(Arc::new(std::sync::RwLock::new(
                    "America/Toronto".to_string(),
                )));

        let mut update = create_test_activity_update("buy-1", "acc-1", None, "USD");
        update.activity_date = "2024-01-15".to_string();
        let preview = activity_service
            .preview_activity_update(update)
            .expect("preview");

        // Midnight in Toronto (EST) is 05:00 UTC.
        assert_eq!(preview.activity.activity_date, utc("2024-01-15T05:00:00Z"));
    }

    #[tokio::test]
    async fn preview_activity_update_mirrors_onto_the_linked_leg() {
        let activity_repository = Arc::new(MockActivityRepository::new());
        seed_internal_cash_transfer_pair(&activity_repository, "group-internal");
        let activity_service =
            preview_test_service(activity_repository.clone(), Arc::new(MockFxService::new()));
        let before = stored_snapshot(&activity_repository);

        let update = ActivityUpdate {
            id: "pair-out".to_string(),
            account_id: "acc-a".to_string(),
            asset: None,
            activity_type: "TRANSFER_OUT".to_string(),
            subtype: None,
            activity_date: "2024-02-01T00:00:00Z".to_string(),
            quantity: None,
            unit_price: None,
            currency: "USD".to_string(),
            fee: None,
            tax: None,
            amount: Some(Some(dec!(150))),
            status: None,
            needs_review: None,
            notes: Some("rent".to_string()),
            fx_rate: None,
            metadata: None,
        };
        let preview = activity_service
            .preview_activity_update(update)
            .expect("preview");

        let linked = preview.linked.expect("the other leg");
        assert_eq!(linked.existing.id, "pair-in");
        assert_eq!(linked.update.account_id, "acc-b");
        assert_eq!(linked.activity_date, utc("2024-02-01T00:00:00Z"));
        assert_eq!(linked.update.amount, Some(Some(dec!(150))));
        assert_eq!(linked.update.notes.as_deref(), Some("rent"));
        assert_eq!(stored_snapshot(&activity_repository), before);
    }

    #[tokio::test]
    async fn preview_activity_update_rejects_what_the_update_rejects_and_symbols() {
        let activity_repository = Arc::new(MockActivityRepository::new());
        activity_repository.add_activity(create_stored_activity("buy-1", "acc-1", Some("NESN")));
        let activity_service =
            preview_test_service(activity_repository.clone(), Arc::new(MockFxService::new()));
        let before = stored_snapshot(&activity_repository);
        let update = || create_test_activity_update("buy-1", "acc-1", None, "USD");
        let asset = |id: Option<&str>, symbol: Option<&str>| {
            Some(AssetResolutionInput {
                id: id.map(str::to_string),
                symbol: symbol.map(str::to_string),
                ..Default::default()
            })
        };

        let mut missing_activity = update();
        missing_activity.id = "missing".to_string();
        let mut missing_asset = update();
        missing_asset.asset = asset(Some("NOT-STORED"), None);
        let mut invalid_date = update();
        invalid_date.activity_date = "15/01/2024".to_string();
        let mut card_account = update();
        card_account.account_id = "acc-card".to_string();
        for (case, invalid) in [
            ("missing activity", missing_activity),
            ("asset not stored", missing_asset),
            ("invalid date", invalid_date),
            ("type not allowed for the account", card_account),
        ] {
            let preview_error = activity_service
                .preview_activity_update(invalid.clone())
                .expect_err(case)
                .to_string();
            let update_error = activity_service
                .update_activity(invalid)
                .await
                .expect_err(case)
                .to_string();
            assert_eq!(preview_error, update_error, "{case}");
        }

        // Resolving a symbol may create an asset, so a preview refuses it.
        let mut symbol = update();
        symbol.asset = asset(None, Some("AAPL"));
        let error = activity_service
            .preview_activity_update(symbol)
            .expect_err("symbol")
            .to_string();
        assert!(error.contains("not by symbol"), "{error}");
        assert_eq!(stored_snapshot(&activity_repository), before);
    }
}
