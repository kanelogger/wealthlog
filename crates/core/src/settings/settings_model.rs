//! Settings domain models.

use serde::{Deserialize, Serialize};

pub const DEFAULT_BASE_CURRENCY: &str = "CNY";

/// Resolve missing or blank legacy settings without rewriting the ledger.
pub fn effective_base_currency(value: &str) -> &str {
    if value.trim().is_empty() {
        DEFAULT_BASE_CURRENCY
    } else {
        value
    }
}

pub const INSIGHTS_OVERVIEW_LAYOUT_KEY: &str = "insights_overview_layout";

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub theme: String,
    pub font: String,
    pub language: String,
    pub formatting_region: String,
    pub base_currency: String,
    pub timezone: String,
    pub onboarding_completed: bool,
    pub auto_update_check_enabled: bool,
    pub menu_bar_visible: bool,
    pub sync_enabled: bool,
    /// Read-only restore state; clearing UI feedback must not authorize sync.
    #[serde(default)]
    pub restore_reconnect_required: bool,
    pub default_return_metric: String,
    /// Versioned dashboard preferences; the frontend validates the layout schema.
    #[serde(default)]
    pub insights_overview_layout: Option<serde_json::Map<String, serde_json::Value>>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "light".to_string(),
            font: "font-mono".to_string(),
            language: "zh".to_string(),
            formatting_region: "CN".to_string(),
            base_currency: DEFAULT_BASE_CURRENCY.to_string(),
            timezone: "".to_string(),
            onboarding_completed: false,
            auto_update_check_enabled: true,
            menu_bar_visible: true,
            sync_enabled: true,
            restore_reconnect_required: false,
            default_return_metric: "twr".to_string(),
            insights_overview_layout: None,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SettingsUpdate {
    pub theme: Option<String>,
    pub font: Option<String>,
    pub language: Option<String>,
    pub formatting_region: Option<String>,
    pub base_currency: Option<String>,
    pub timezone: Option<String>,
    pub onboarding_completed: Option<bool>,
    pub auto_update_check_enabled: Option<bool>,
    pub menu_bar_visible: Option<bool>,
    pub default_return_metric: Option<String>,
    pub insights_overview_layout: Option<serde_json::Map<String, serde_json::Value>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Sort {
    pub id: String,
    pub desc: bool,
}

/// Domain model for app setting key-value pair
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AppSetting {
    pub setting_key: String,
    pub setting_value: String,
}
