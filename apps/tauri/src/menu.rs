use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{AppHandle, Emitter, Manager, Runtime};

pub fn create_menu<R: Runtime>(app: &AppHandle<R>) -> Result<Menu<R>, tauri::Error> {
    let app_menu = SubmenuBuilder::new(app, "Wealthlog")
        .item(&MenuItemBuilder::with_id("show_main_window", "显示主窗口").build(app)?)
        .item(&MenuItemBuilder::with_id("open_settings", "设置…").build(app)?)
        .separator()
        .item(&PredefinedMenuItem::hide(app, Some("隐藏"))?)
        .item(&PredefinedMenuItem::hide_others(app, Some("隐藏其他"))?)
        .item(&PredefinedMenuItem::show_all(app, Some("全部显示"))?)
        .separator()
        .item(&PredefinedMenuItem::quit(app, Some("退出 Wealthlog"))?)
        .build()?;
    let edit_menu = SubmenuBuilder::new(app, "编辑")
        .item(&PredefinedMenuItem::undo(app, Some("撤销"))?)
        .item(&PredefinedMenuItem::redo(app, Some("重做"))?)
        .separator()
        .item(&PredefinedMenuItem::cut(app, Some("剪切"))?)
        .item(&PredefinedMenuItem::copy(app, Some("拷贝"))?)
        .item(&PredefinedMenuItem::paste(app, Some("粘贴"))?)
        .item(&PredefinedMenuItem::select_all(app, Some("全选"))?)
        .build()?;
    let view_menu = SubmenuBuilder::new(app, "显示")
        .item(
            &MenuItemBuilder::with_id("toggle_fullscreen", "切换全屏")
                .accelerator("F11")
                .build(app)?,
        )
        .build()?;
    MenuBuilder::new(app)
        .item(&app_menu)
        .item(&edit_menu)
        .item(&view_menu)
        .build()
}

pub fn handle_menu_event(app: &AppHandle, event_id: &str) {
    if event_id == "show_main_window" {
        crate::show_main_window(app);
        return;
    }
    if let Some(window) = app.get_webview_window("main") {
        match event_id {
            "open_settings" => {
                let _ = window.emit(
                    "navigate-to-route",
                    serde_json::json!({ "route": "/settings/general" }),
                );
            }
            "toggle_fullscreen" => {
                if let Ok(is_fullscreen) = window.is_fullscreen() {
                    let _ = window.set_fullscreen(!is_fullscreen);
                }
            }
            _ => {}
        }
    }
}
