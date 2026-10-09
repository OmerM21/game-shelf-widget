//! The notification-area icon: the widget has no window frame or taskbar
//! button, so this is how it's shown, hidden and quit.

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;

/// The "Start with Windows" item, kept so settings can update its check mark.
pub struct AutostartItem(pub CheckMenuItem<Wry>);

pub fn show_shelf(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn toggle_shelf(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            show_shelf(app);
        }
    }
}

pub fn set_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let launcher = app.autolaunch();
    let result = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    result.map_err(|e| format!("Couldn't change Start with Windows: {e}"))?;
    if let Some(item) = app.try_state::<AutostartItem>() {
        let _ = item.0.set_checked(enabled);
    }
    Ok(())
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);

    let toggle = MenuItem::with_id(app, "toggle", "Show or hide shelf", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, "refresh", "Refresh games", true, None::<&str>)?;
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        "Start with Windows",
        true,
        autostart_on,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &toggle,
            &settings,
            &refresh,
            &PredefinedMenuItem::separator(app)?,
            &autostart,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    app.manage(AutostartItem(autostart.clone()));

    TrayIconBuilder::with_id("main")
        .icon(
            app.default_window_icon()
                .cloned()
                .expect("app icon is missing"),
        )
        .tooltip("Game Shelf")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "toggle" => toggle_shelf(app),
            "settings" => {
                let _ = crate::windows::open_settings(app);
            }
            "refresh" => {
                show_shelf(app);
                let _ = app.emit("reload-shelf", ());
            }
            "autostart" => {
                // The menu flips the check mark itself; apply what it now shows.
                let wanted = autostart.is_checked().unwrap_or(false);
                if set_autostart(app, wanted).is_err() {
                    let _ = autostart.set_checked(!wanted);
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                toggle_shelf(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}
