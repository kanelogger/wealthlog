use serde_json::{json, Value};
use tauri::Manager;

#[cfg(target_os = "macos")]
use std::sync::{Arc, Mutex};
#[cfg(target_os = "macos")]
use wealthlog_agent_install::{Action, Installer, Tool};

pub struct AgentBundleState {
    #[cfg(target_os = "macos")]
    installer: Arc<Mutex<Installer>>,
    initial: Value,
}

pub fn setup(app: &tauri::AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let home = app.path().home_dir().unwrap_or_default();
        let contents = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent()?.parent().map(std::path::Path::to_path_buf))
            .unwrap_or_default();
        let installer = Installer::new(
            home.clone(),
            contents,
            vec!["/Applications".into(), home.join("Applications")],
            app.package_info().version.to_string(),
            !home.as_os_str().is_empty() && app.config().identifier == "local.kanelogger.wealthlog",
        );
        let initial = serde_json::to_value(installer.status()).unwrap_or_else(|_| unsupported());
        let installer = Arc::new(Mutex::new(installer));
        app.manage(AgentBundleState {
            installer: installer.clone(),
            initial,
        });
        tauri::async_runtime::spawn_blocking(move || {
            if let Ok(mut installer) = installer.lock() {
                installer.startup();
            } else {
                log::error!("Agent bundle installer lock is poisoned");
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    app.manage(AgentBundleState {
        initial: unsupported(),
    });
}

fn unsupported() -> Value {
    json!({
        "supported": false,
        "locationSupported": false,
        "cli": { "state": "unsupported", "path": "", "desired": false, "error": null },
        "skill": { "state": "unsupported", "path": "", "desired": false, "error": null },
        "providerApp": null,
        "currentApp": null,
        "repairApp": null,
        "version": null,
        "config": null,
        "error": null,
        "canRepair": false,
        "canUseCopy": false
    })
}

#[tauri::command]
pub fn agent_bundle_status(state: tauri::State<'_, AgentBundleState>) -> Result<Value, String> {
    #[cfg(target_os = "macos")]
    {
        match state.installer.try_lock() {
            Ok(installer) => serde_json::to_value(installer.status()).map_err(|e| e.to_string()),
            Err(std::sync::TryLockError::WouldBlock) => {
                let mut initial = state.initial.clone();
                if initial["supported"] == true && initial["locationSupported"] == true {
                    initial["cli"]["state"] = json!("installing");
                    initial["skill"]["state"] = json!("installing");
                }
                Ok(initial)
            }
            Err(_) => Err("Agent bundle installer lock is poisoned".into()),
        }
    }
    #[cfg(not(target_os = "macos"))]
    Ok(state.initial.clone())
}

#[tauri::command]
pub async fn agent_bundle_action(
    state: tauri::State<'_, AgentBundleState>,
    action: String,
    tool: Option<String>,
) -> Result<Value, String> {
    #[cfg(target_os = "macos")]
    {
        let action: Action = serde_json::from_value(json!(action)).map_err(|e| e.to_string())?;
        let tool: Option<Tool> = tool
            .map(|t| serde_json::from_value(json!(t)))
            .transpose()
            .map_err(|e| e.to_string())?;
        let installer = state.installer.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let mut installer = installer.lock().map_err(|_| "Installer lock is poisoned")?;
            serde_json::to_value(installer.action(action, tool)?).map_err(|e| e.to_string())
        })
        .await
        .map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (state, action, tool);
        Err("Agent tool installation is unsupported in this environment".into())
    }
}
