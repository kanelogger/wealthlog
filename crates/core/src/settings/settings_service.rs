use super::SettingsRepositoryTrait;
use crate::errors::{DatabaseError, Error, Result};
use crate::fx::FxServiceTrait;
use crate::settings::{effective_base_currency, Settings, SettingsUpdate, DEFAULT_BASE_CURRENCY};
use crate::utils::time_utils::canonicalize_timezone;
use async_trait::async_trait;
use log::{debug, error};
use std::sync::Arc;

pub const FIXED_UI_LANGUAGE: &str = "zh";
pub const FIXED_FORMATTING_REGION: &str = "CN";

/// Any language string carried by an update is stored as the only supported UI language.
fn canonical_ui_language(_language: &str) -> String {
    FIXED_UI_LANGUAGE.to_string()
}

/// Any region string carried by an update is stored as the only supported formatting region.
fn canonical_formatting_region(_region: &str) -> String {
    FIXED_FORMATTING_REGION.to_string()
}

// Define the trait for SettingsService
#[async_trait]
pub trait SettingsServiceTrait: Send + Sync {
    fn get_settings(&self) -> Result<Settings>;

    async fn update_settings(&self, new_settings: &SettingsUpdate) -> Result<()>;

    fn get_base_currency(&self) -> Result<Option<String>>;

    async fn update_base_currency(&self, new_base_currency: &str) -> Result<()>;

    fn is_auto_update_check_enabled(&self) -> Result<bool>;

    /// Get a single setting value by key. Returns None if not found.
    fn get_setting_value(&self, key: &str) -> Result<Option<String>>;

    /// Set a single setting value by key.
    async fn set_setting_value(&self, key: &str, value: &str) -> Result<()>;
}

pub struct SettingsService {
    settings_repository: Arc<dyn SettingsRepositoryTrait>,
    fx_service: Arc<dyn FxServiceTrait>,
}

// Implement the trait for SettingsService
#[async_trait]
impl SettingsServiceTrait for SettingsService {
    fn get_settings(&self) -> Result<Settings> {
        let mut settings = self.settings_repository.get_settings()?;
        // Stored language and region are historical. Readers always see the fixed
        // values, and this path does not write them back.
        settings.base_currency = effective_base_currency(&settings.base_currency).to_string();
        settings.language = FIXED_UI_LANGUAGE.to_string();
        settings.formatting_region = FIXED_FORMATTING_REGION.to_string();
        Ok(settings)
    }

    async fn update_settings(&self, new_settings: &SettingsUpdate) -> Result<()> {
        // Compare the stored value so explicitly saving the default still registers
        // FX pairs for legacy ledgers whose currency was missing or blank.
        let current_base_currency = self.get_setting_value("base_currency")?;
        let mut normalized_settings = new_settings.clone();

        if let Some(ref new_base_currency_val) = normalized_settings.base_currency {
            if current_base_currency.as_deref() != Some(new_base_currency_val.as_str()) {
                self.update_base_currency(new_base_currency_val.as_str())
                    .await?;
            }
        }

        if let Some(ref timezone_raw) = normalized_settings.timezone {
            normalized_settings.timezone = Some(canonicalize_timezone(timezone_raw)?);
        }

        if let Some(ref region) = normalized_settings.formatting_region {
            normalized_settings.formatting_region = Some(canonical_formatting_region(region));
        }
        if let Some(ref language) = normalized_settings.language {
            normalized_settings.language = Some(canonical_ui_language(language));
        }

        self.settings_repository
            .update_settings(&normalized_settings)
            .await?;
        Ok(())
    }

    fn get_base_currency(&self) -> Result<Option<String>> {
        match self.settings_repository.get_setting("base_currency") {
            Ok(value) => Ok(Some(effective_base_currency(&value).to_string())),
            Err(Error::Database(DatabaseError::NotFound(_))) => {
                Ok(Some(DEFAULT_BASE_CURRENCY.to_string()))
            }
            Err(e) => Err(e),
        }
    }

    async fn update_base_currency(&self, new_base_currency: &str) -> Result<()> {
        let all_currencies = self
            .settings_repository
            .get_distinct_currencies_excluding_base(new_base_currency)?;

        debug!(
            "Registering currency pairs for currencies: {:?}",
            all_currencies
        );

        for currency_code in all_currencies {
            let registration_result = self
                .fx_service
                .register_currency_pair(currency_code.as_str(), new_base_currency)
                .await;

            if let Err(e) = registration_result {
                error!(
                    "Failed to register currency pair {}{}: {}. Skipping.",
                    new_base_currency, currency_code, e
                );
            }
        }

        self.settings_repository
            .update_setting("base_currency", new_base_currency)
            .await?;
        Ok(())
    }

    fn is_auto_update_check_enabled(&self) -> Result<bool> {
        match self
            .settings_repository
            .get_setting("auto_update_check_enabled")
        {
            Ok(value) => Ok(value.parse().unwrap_or(true)),
            Err(Error::Database(DatabaseError::NotFound(_))) => Ok(true),
            Err(e) => Err(e),
        }
    }

    fn get_setting_value(&self, key: &str) -> Result<Option<String>> {
        match self.settings_repository.get_setting(key) {
            Ok(value) => Ok(Some(value)),
            Err(Error::Database(DatabaseError::NotFound(_))) => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn set_setting_value(&self, key: &str, value: &str) -> Result<()> {
        self.settings_repository.update_setting(key, value).await
    }
}

impl SettingsService {
    pub fn new(
        settings_repository: Arc<dyn SettingsRepositoryTrait>,
        fx_service: Arc<dyn FxServiceTrait>,
    ) -> Self {
        SettingsService {
            settings_repository,
            fx_service,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{canonical_formatting_region, canonical_ui_language};

    #[test]
    fn stores_every_language_string_as_simplified_chinese() {
        for language in [
            "zh", "zh-CN", "zh-Hant", "zh-HK", "en", "en-US", "foo_bar", "",
        ] {
            assert_eq!(canonical_ui_language(language), "zh", "{language}");
        }
    }

    #[test]
    fn stores_every_region_string_as_china() {
        for region in ["CN", "system", "US", "de-DE", "zh-TW", "unknown", ""] {
            assert_eq!(canonical_formatting_region(region), "CN", "{region}");
        }
    }
}
