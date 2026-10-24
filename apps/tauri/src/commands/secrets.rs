use crate::profiles::ProfileAccess;
use wealthfolio_core::secrets::validate_unscoped_secret_service_id;

#[tauri::command]
pub async fn set_secret(
    secret_key: String,
    secret: String,
    profile: ProfileAccess,
) -> Result<(), String> {
    validate_unscoped_secret_service_id(&secret_key)?;
    profile
        .secret_store
        .set_secret(&secret_key, &secret)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_secret(
    secret_key: String,
    profile: ProfileAccess,
) -> Result<Option<String>, String> {
    validate_unscoped_secret_service_id(&secret_key)?;
    profile
        .secret_store
        .get_secret(&secret_key)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_secret(secret_key: String, profile: ProfileAccess) -> Result<(), String> {
    validate_unscoped_secret_service_id(&secret_key)?;
    profile
        .secret_store
        .delete_secret(&secret_key)
        .map_err(|e| e.to_string())
}
