use async_trait::async_trait;
use diesel::prelude::*;
use std::sync::Arc;

use super::model::AppSettingDB;
use crate::db::{get_connection, DbPool, WriteHandle};
use crate::errors::StorageError;
use crate::schema::app_settings::dsl::*;
use crate::schema::{accounts, assets};
use wealthfolio_core::assets::AssetKind;
use wealthfolio_core::errors::Result;
use wealthfolio_core::settings::{
    effective_base_currency, Settings, SettingsRepositoryTrait, SettingsUpdate,
    INSIGHTS_OVERVIEW_LAYOUT_KEY,
};

pub struct SettingsRepository {
    pool: Arc<DbPool>,
    writer: WriteHandle,
}

impl SettingsRepository {
    pub fn new(pool: Arc<DbPool>, writer: WriteHandle) -> Self {
        SettingsRepository { pool, writer }
    }
}

// Implement the trait for SettingsRepository
#[async_trait]
impl SettingsRepositoryTrait for SettingsRepository {
    fn get_settings(&self) -> Result<Settings> {
        let mut conn = get_connection(&self.pool)?;
        let all_settings: Vec<(String, String)> = app_settings
            .select((setting_key, setting_value))
            .load::<(String, String)>(&mut conn)
            .map_err(StorageError::from)?;

        let mut settings = Settings::default(); // Use default implementation

        for (key, value) in all_settings {
            match key.as_str() {
                "theme" => settings.theme = value,
                "font" => settings.font = value,
                "language" => settings.language = value,
                "formatting_region" => settings.formatting_region = value,
                "base_currency" => {
                    settings.base_currency = effective_base_currency(&value).to_string();
                }
                "timezone" => settings.timezone = value,
                "onboarding_completed" => {
                    settings.onboarding_completed = value.parse().unwrap_or(false);
                }
                "auto_update_check_enabled" => {
                    settings.auto_update_check_enabled = value.parse().unwrap_or(true);
                }
                "menu_bar_visible" => {
                    settings.menu_bar_visible = value.parse().unwrap_or(true);
                }
                "sync_enabled" => {
                    settings.sync_enabled = value.parse().unwrap_or(true);
                }
                "restore_reconnect_required" => {
                    settings.restore_reconnect_required = value == "true";
                }
                "default_return_metric" => settings.default_return_metric = value,
                INSIGHTS_OVERVIEW_LAYOUT_KEY => {
                    settings.insights_overview_layout = serde_json::from_str(&value).ok();
                }
                _ => {} // Ignore unknown settings
            }
        }

        Ok(settings)
    }

    async fn update_settings(&self, new_settings: &SettingsUpdate) -> Result<()> {
        let settings = new_settings.clone();
        self.writer
            .exec_tx(move |tx| {
                if let Some(ref theme) = settings.theme {
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: "theme".to_string(),
                            setting_value: theme.clone(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                if let Some(ref font) = settings.font {
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: "font".to_string(),
                            setting_value: font.clone(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                if let Some(ref language) = settings.language {
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: "language".to_string(),
                            setting_value: language.clone(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                if let Some(ref formatting_region) = settings.formatting_region {
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: "formatting_region".to_string(),
                            setting_value: formatting_region.clone(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                if let Some(ref base_currency) = settings.base_currency {
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: "base_currency".to_string(),
                            setting_value: base_currency.clone(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                if let Some(ref timezone) = settings.timezone {
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: "timezone".to_string(),
                            setting_value: timezone.clone(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                if let Some(onboarding_completed) = settings.onboarding_completed {
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: "onboarding_completed".to_string(),
                            setting_value: onboarding_completed.to_string(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                if let Some(auto_update_check_enabled) = settings.auto_update_check_enabled {
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: "auto_update_check_enabled".to_string(),
                            setting_value: auto_update_check_enabled.to_string(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                if let Some(menu_bar_visible) = settings.menu_bar_visible {
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: "menu_bar_visible".to_string(),
                            setting_value: menu_bar_visible.to_string(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                if let Some(ref default_return_metric) = settings.default_return_metric {
                    diesel::replace_into(app_settings)
                        .values(&AppSettingDB {
                            setting_key: "default_return_metric".to_string(),
                            setting_value: default_return_metric.clone(),
                        })
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                if let Some(ref layout) = settings.insights_overview_layout {
                    let row = AppSettingDB {
                        setting_key: INSIGHTS_OVERVIEW_LAYOUT_KEY.to_string(),
                        setting_value: serde_json::Value::Object(layout.clone()).to_string(),
                    };
                    diesel::replace_into(app_settings)
                        .values(&row)
                        .execute(tx.conn())
                        .map_err(StorageError::from)?;
                }

                Ok(())
            })
            .await
    }

    fn get_setting(&self, setting_key_param: &str) -> Result<String> {
        let mut conn = get_connection(&self.pool)?;
        let result = app_settings
            .filter(setting_key.eq(setting_key_param))
            .select(setting_value)
            .first(&mut conn);

        match result {
            Ok(value) => Ok(value),
            Err(diesel::result::Error::NotFound) => {
                // Return default values for known settings
                let default_value = match setting_key_param {
                    "theme" => "light",
                    "font" => "font-mono",
                    "language" => "zh",
                    "timezone" => "",
                    "onboarding_completed" => "false",
                    "auto_update_check_enabled" => "true",
                    "menu_bar_visible" => "true",
                    "sync_enabled" => "true",
                    "default_return_metric" => "twr",
                    _ => return Err(StorageError::from(diesel::result::Error::NotFound).into()),
                };
                Ok(default_value.to_string())
            }
            Err(e) => Err(StorageError::from(e).into()),
        }
    }

    async fn update_setting(
        &self,
        setting_key_param: &str,
        setting_value_param: &str,
    ) -> Result<()> {
        let key = setting_key_param.to_string();
        let value = setting_value_param.to_string();

        self.writer
            .exec_tx(move |tx| {
                let row = AppSettingDB {
                    setting_key: key,
                    setting_value: value,
                };
                diesel::replace_into(app_settings)
                    .values(&row)
                    .execute(tx.conn())
                    .map_err(StorageError::from)?;

                Ok(())
            })
            .await
    }

    fn get_distinct_currencies_excluding_base(&self, base_currency: &str) -> Result<Vec<String>> {
        let mut conn = get_connection(&self.pool)?;

        let currency_assets: Vec<String> = assets::table
            .filter(assets::kind.eq(AssetKind::Fx.as_db_str()))
            .filter(assets::quote_ccy.ne(base_currency))
            .select(assets::quote_ccy)
            .distinct()
            .load::<String>(&mut conn)
            .map_err(StorageError::from)?;

        let account_currencies: Vec<String> = accounts::table
            .filter(accounts::currency.ne(base_currency))
            .select(accounts::currency)
            .distinct()
            .load::<String>(&mut conn)
            .map_err(StorageError::from)?;

        let mut all_currencies: Vec<String> = Vec::new();
        all_currencies.extend(currency_assets);
        all_currencies.extend(account_currencies);
        all_currencies.sort();
        all_currencies.dedup();

        Ok(all_currencies)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{create_pool, run_migrations, write_actor::spawn_writer};
    use serde_json::json;

    async fn setup() -> (SettingsRepository, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("settings.db").to_string_lossy().to_string();
        run_migrations(&db_path).unwrap();
        let pool = create_pool(&db_path).unwrap();
        let writer = spawn_writer((*pool).clone()).unwrap();
        (SettingsRepository::new(pool, writer), dir)
    }

    #[tokio::test]
    async fn base_currency_defaults_to_cny_without_rewriting_stored_values() {
        let (repo, _dir) = setup().await;
        assert_eq!(Settings::default().base_currency, "CNY");
        // The original migrations can leave the key absent or blank.
        {
            let mut conn = get_connection(&repo.pool).unwrap();
            diesel::delete(app_settings.filter(setting_key.eq("base_currency")))
                .execute(&mut conn)
                .unwrap();
        }
        assert_eq!(repo.get_settings().unwrap().base_currency, "CNY");
        assert!(repo.get_setting("base_currency").is_err());

        for stored in ["", " \t", "USD", "CAD", "CNY"] {
            repo.update_setting("base_currency", stored).await.unwrap();
            let expected = if stored.trim().is_empty() {
                "CNY"
            } else {
                stored
            };
            assert_eq!(repo.get_settings().unwrap().base_currency, expected);
            assert_eq!(repo.get_setting("base_currency").unwrap(), stored);
        }
    }

    #[tokio::test]
    async fn insights_layout_round_trips_and_survives_unrelated_updates() {
        let (repo, _dir) = setup().await;
        assert!(repo
            .get_settings()
            .unwrap()
            .insights_overview_layout
            .is_none());
        let layout = json!({
            "version": 1,
            "hiddenWidgets": ["regions"],
            "layouts": {"desktop": [{"i": "composition", "x": 0, "y": 0, "w": 9, "h": 6}]}
        });
        let update: SettingsUpdate = serde_json::from_value(json!({
            "insightsOverviewLayout": layout
        }))
        .unwrap();
        repo.update_settings(&update).await.unwrap();
        let theme_update = serde_json::from_value(json!({"theme": "dark"})).unwrap();
        repo.update_settings(&theme_update).await.unwrap();
        let loaded = repo.get_settings().unwrap();
        assert_eq!(loaded.theme, "dark");
        assert_eq!(loaded.insights_overview_layout.as_ref(), layout.as_object());
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                &repo.get_setting(INSIGHTS_OVERVIEW_LAYOUT_KEY).unwrap()
            )
            .unwrap(),
            layout
        );
        let reset = serde_json::from_value(json!({
            "insightsOverviewLayout": {"version": 1, "hiddenWidgets": [], "layouts": {}}
        }))
        .unwrap();
        repo.update_settings(&reset).await.unwrap();
        assert_eq!(
            repo.get_settings()
                .unwrap()
                .insights_overview_layout
                .unwrap()["hiddenWidgets"],
            json!([])
        );
    }

    #[tokio::test]
    async fn malformed_stored_insights_layout_uses_default() {
        let (repo, _dir) = setup().await;
        for invalid in ["not json", "null", "[]", "42"] {
            repo.update_setting(INSIGHTS_OVERVIEW_LAYOUT_KEY, invalid)
                .await
                .unwrap();
            assert!(repo
                .get_settings()
                .unwrap()
                .insights_overview_layout
                .is_none());
        }
    }
}
