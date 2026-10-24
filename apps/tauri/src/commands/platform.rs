use serde::Serialize;

#[derive(Serialize)]
pub struct PlatformInfo {
    pub os: &'static str,
    pub arch: &'static str,
    pub is_mobile: bool,
    pub is_desktop: bool,
    pub is_tauri: bool,
}

#[tauri::command]
pub fn get_platform() -> PlatformInfo {
    PlatformInfo {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        is_mobile: false,
        is_desktop: true,
        is_tauri: true,
    }
}

// Alternative: Use compile-time constants for even better performance
#[tauri::command]
pub fn is_mobile() -> bool {
    false
}

#[tauri::command]
pub fn is_desktop() -> bool {
    true
}
