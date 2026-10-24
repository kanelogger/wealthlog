use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use wealthfolio_core::portfolios::AccountScope;

#[derive(Deserialize)]
pub struct FilterBody {
    pub filter: AccountScope,
    #[serde(rename = "includeClosed", default)]
    pub include_closed: bool,
}

#[derive(Deserialize)]
pub struct AllocationFilterBody {
    pub filter: AccountScope,
    #[serde(rename = "taxonomyId")]
    pub taxonomy_id: String,
    #[serde(rename = "categoryId")]
    pub category_id: String,
}

#[derive(Deserialize)]
pub struct AccountIdQuery {
    #[serde(rename = "accountId")]
    pub account_id: String,
    #[serde(rename = "includeClosed", default)]
    pub include_closed: bool,
}

#[derive(Deserialize)]
pub struct AllocationHoldingsQuery {
    #[serde(rename = "accountId")]
    pub account_id: String,
    #[serde(rename = "taxonomyId")]
    pub taxonomy_id: String,
    #[serde(rename = "categoryId")]
    pub category_id: String,
}

#[derive(Deserialize)]
pub struct HoldingItemQuery {
    #[serde(rename = "accountId")]
    pub account_id: String,
    #[serde(rename = "assetId")]
    pub asset_id: String,
}

#[derive(Deserialize)]
pub struct AssetHoldingsQuery {
    #[serde(rename = "assetId")]
    pub asset_id: String,
}

#[derive(Deserialize)]
pub struct AssetLotsQuery {
    #[serde(rename = "assetId")]
    pub asset_id: String,
    #[serde(rename = "includeSnapshotPositions", default)]
    pub include_snapshot_positions: bool,
}

#[derive(Deserialize)]
pub struct HistoryQuery {
    #[serde(rename = "accountId")]
    pub account_id: String,
    #[serde(rename = "startDate")]
    pub start_date: Option<String>,
    #[serde(rename = "endDate")]
    pub end_date: Option<String>,
}

#[derive(Deserialize)]
pub struct HistoryFilterBody {
    pub filter: AccountScope,
    #[serde(rename = "startDate")]
    pub start_date: Option<String>,
    #[serde(rename = "endDate")]
    pub end_date: Option<String>,
}

#[derive(Deserialize)]
pub struct CurrentValuationBody {
    pub filter: AccountScope,
    #[serde(rename = "includeAccounts", default)]
    pub include_accounts: bool,
}

#[derive(Deserialize)]
pub struct SnapshotsQuery {
    #[serde(rename = "accountId")]
    pub account_id: String,
    #[serde(rename = "dateFrom")]
    pub date_from: Option<String>,
    #[serde(rename = "dateTo")]
    pub date_to: Option<String>,
}

#[derive(Deserialize)]
pub struct SnapshotDateQuery {
    #[serde(rename = "accountId")]
    pub account_id: String,
    pub date: String,
}

#[derive(Deserialize)]
pub struct DeleteSnapshotQuery {
    #[serde(rename = "accountId")]
    pub account_id: String,
    pub date: String,
    #[serde(rename = "snapshotId")]
    pub snapshot_id: Option<String>,
}

/// Information about a snapshot for UI display
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotInfo {
    pub id: String,
    /// Stored date, returned raw so malformed rows can be remediated.
    pub snapshot_date: String,
    pub is_date_valid: bool,
    pub source: String,
    pub position_count: usize,
    pub cash_currency_count: usize,
    pub cash_total_account_currency: String,
}

/// Input for a single holding when saving manual holdings
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldingInput {
    /// For existing holdings, pass the known asset ID directly (preferred)
    pub asset_id: Option<String>,
    /// Symbol (e.g., "AAPL", "META.TO") - used when asset_id is not provided
    pub symbol: String,
    pub quantity: String,
    pub currency: String,
    pub average_cost: Option<String>,
    /// Exchange MIC code for new holdings (e.g., "XNAS", "XTSE"). Used when asset_id is not provided.
    pub exchange_mic: Option<String>,
    /// Quote currency resolved during search/review (e.g., GBp)
    pub quote_ccy: Option<String>,
    /// Instrument type resolved during search/review (e.g., EQUITY, CRYPTO)
    pub instrument_type: Option<String>,
    /// Market data provider that resolved this holding, if selected.
    pub provider_id: Option<String>,
    /// Provider-native symbol/code selected by search/import.
    pub provider_symbol: Option<String>,
    /// Asset name for new custom assets
    pub name: Option<String>,
    /// Data source (e.g., "MANUAL" for custom assets) — sets quote mode to manual
    pub data_source: Option<String>,
    /// Asset kind (e.g., "INVESTMENT", "OTHER")
    pub asset_kind: Option<String>,
}

/// Request body for saving manual holdings
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveManualHoldingsRequest {
    pub account_id: String,
    pub holdings: Vec<HoldingInput>,
    pub cash_balances: HashMap<String, String>,
    pub snapshot_date: Option<String>,
}
