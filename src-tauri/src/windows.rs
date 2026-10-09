//! The settings and configure windows. They're ordinary framed windows so
//! they aren't squeezed inside the small shelf widget.

use tauri::{AppHandle, Manager, Theme, WebviewUrl, WebviewWindowBuilder};

/// Brings an already-open window to the front.
fn focus_existing(app: &AppHandle, label: &str) -> bool {
    let Some(window) = app.get_webview_window(label) else {
        return false;
    };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
    true
}

pub fn open_settings(app: &AppHandle) -> tauri::Result<()> {
    if focus_existing(app, "settings") {
        return Ok(());
    }
    WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings.html".into()))
        .title("Game Shelf settings")
        .theme(Some(Theme::Dark))
        .inner_size(480.0, 760.0)
        .min_inner_size(380.0, 400.0)
        .center()
        .build()?;
    Ok(())
}

/// Opens the configure window for one game; each game gets its own window.
pub fn open_configure(app: &AppHandle, game_id: &str) -> tauri::Result<()> {
    // Game ids only contain letters, digits, '-' and '_', which labels allow.
    let label = format!("configure-{game_id}");
    if focus_existing(app, &label) {
        return Ok(());
    }
    let id_json = serde_json::to_string(game_id).expect("a string always serializes");
    WebviewWindowBuilder::new(app, &label, WebviewUrl::App("configure.html".into()))
        .title("Configure game")
        .theme(Some(Theme::Dark))
        .initialization_script(format!("window.__GAME_ID__ = {id_json};"))
        .inner_size(820.0, 780.0)
        .min_inner_size(480.0, 400.0)
        .center()
        .build()?;
    Ok(())
}
