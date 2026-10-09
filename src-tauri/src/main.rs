// Hide the console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod artwork;
mod config;
mod desktop;
mod epic;
mod games;
mod launch;
mod steam;
mod tray;
mod vdf;
mod windows;
mod xbox;

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{Manager, State, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_window_state::StateFlags;

struct AppState {
    data_dir: PathBuf,
    client: reqwest::Client,
    api_key: Mutex<Option<String>>,
    /// Serializes changes to games.json so concurrent edits aren't lost.
    config_lock: Mutex<()>,
}

impl AppState {
    fn art_root(&self) -> PathBuf {
        self.data_dir.join("artwork")
    }

    /// Loads games.json, applies `change`, and saves it, all under one lock.
    fn update_config<T>(
        &self,
        change: impl FnOnce(&mut config::Config) -> Result<T, String>,
    ) -> Result<T, String> {
        let _guard = self.config_lock.lock().unwrap();
        let mut config = config::Config::load(&self.data_dir)?;
        let result = change(&mut config)?;
        config.save(&self.data_dir)?;
        Ok(result)
    }
}

/// Asks WebView2 to keep its memory use low. The widget sits idle on the
/// desktop nearly all the time, which is what this setting is meant for.
fn prefer_low_memory(window: &tauri::WebviewWindow) {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
    };
    use windows_core::Interface;

    let _ = window.with_webview(|webview| unsafe {
        let Ok(core) = webview.controller().CoreWebView2() else {
            return;
        };
        // Older WebView2 runtimes don't have this setting; just skip it there.
        if let Ok(core) = core.cast::<ICoreWebView2_19>() {
            let _ = core.SetMemoryUsageTargetLevel(COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW);
        }
    });
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Settings {
    api_key: String,
    custom_games: Vec<config::CustomGame>,
    hidden_games: Vec<games::Game>,
    preferences: config::Preferences,
    autostart: bool,
    /// Set when games.json couldn't be read.
    config_error: Option<String>,
    data_dir: PathBuf,
}

#[tauri::command]
fn list_games(state: State<'_, AppState>) -> games::Listing {
    games::all_games(&state.data_dir)
}

#[tauri::command]
fn launch_game(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let game = games::find(&state.data_dir, &id)?;
    launch::launch(&app, &game)?;
    // For "recently played" sorting. A broken games.json mustn't block playing.
    let _ = state.update_config(|config| {
        config.played.insert(game.id.clone(), now_secs());
        Ok(())
    });
    Ok(())
}

fn require_api_key(state: &AppState) -> Result<String, String> {
    state
        .api_key
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "Add your SteamGridDB API key in settings first".to_string())
}

/// Renames a game; an empty name or the original one removes the rename.
/// The artwork is looked up again under the new name.
#[tauri::command]
fn rename_game(state: State<'_, AppState>, id: String, name: String) -> Result<(), String> {
    let game = games::find(&state.data_dir, &id)?;
    let name = name.trim();
    let new_name = (!name.is_empty() && name != game.original_name).then(|| name.to_string());
    state.update_config(|config| {
        config.edit_override(&id, |o| {
            if o.name != new_name && o.sgdb_id.is_none() {
                // The search changes, so images picked for the old match no longer apply.
                o.clear_images();
            }
            o.name = new_name;
        });
        Ok(())
    })?;
    if game.overrides.sgdb_id.is_none() {
        artwork::clear(&state.art_root(), &id);
    }
    Ok(())
}

#[tauri::command]
async fn search_steamgriddb(
    state: State<'_, AppState>,
    query: String,
) -> Result<Vec<artwork::Match>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let key = require_api_key(&state)?;
    artwork::search(&state.client, &key, &query).await
}

/// Sets which SteamGridDB game supplies the artwork; `None` goes back to automatic.
#[tauri::command]
fn set_game_match(
    state: State<'_, AppState>,
    id: String,
    sgdb_id: Option<u64>,
    sgdb_name: Option<String>,
) -> Result<(), String> {
    state.update_config(|config| {
        config.edit_override(&id, |o| {
            o.sgdb_id = sgdb_id;
            o.sgdb_name = sgdb_id.and(sgdb_name);
            o.clear_images();
        });
        Ok(())
    })?;
    artwork::clear(&state.art_root(), &id);
    Ok(())
}

#[tauri::command]
async fn artwork_options(
    state: State<'_, AppState>,
    id: String,
    kind: artwork::Kind,
) -> Result<Vec<artwork::ArtOption>, String> {
    let game = games::find(&state.data_dir, &id)?;
    let key = require_api_key(&state)?;
    artwork::options(&state.client, &key, &game, kind).await
}

/// Picks one image for a game; `None` goes back to the top-rated one.
#[tauri::command]
fn choose_artwork(
    state: State<'_, AppState>,
    id: String,
    kind: artwork::Kind,
    url: Option<String>,
) -> Result<(), String> {
    state.update_config(|config| {
        config.edit_override(&id, |o| match kind {
            artwork::Kind::Cover => o.cover = url,
            artwork::Kind::Hero => o.hero = url,
            artwork::Kind::Logo => o.logo = url,
        });
        Ok(())
    })?;
    artwork::clear(&state.art_root(), &id);
    Ok(())
}

/// Throws away a game's cached artwork so it's downloaded again.
#[tauri::command]
fn rescan_artwork(state: State<'_, AppState>, id: String) {
    artwork::clear(&state.art_root(), &id);
}

#[tauri::command]
fn set_preferences(
    state: State<'_, AppState>,
    mut preferences: config::Preferences,
) -> Result<(), String> {
    preferences.slant = preferences.slant.clamp(0.0, 12.0);
    preferences.hover_volume = preferences.hover_volume.min(100);
    state.update_config(|config| {
        config.preferences = preferences;
        Ok(())
    })
}

#[tauri::command]
async fn get_artwork(state: State<'_, AppState>, id: String) -> Result<artwork::Artwork, String> {
    let game = games::find(&state.data_dir, &id)?;
    let key = state.api_key.lock().unwrap().clone();
    artwork::get(&state.client, key.as_deref(), &state.art_root(), &game).await
}

#[tauri::command]
fn get_settings(app: tauri::AppHandle, state: State<'_, AppState>) -> Settings {
    let listing = games::all_games(&state.data_dir);
    let (custom_games, config_error) = match config::Config::load(&state.data_dir) {
        Ok(config) => {
            // Show the names as they appear on the shelf, including renames.
            let games = config
                .games
                .into_iter()
                .map(|mut g| {
                    let game_id = games::Game::from(g.clone()).id;
                    if let Some(name) = config.overrides.get(&game_id).and_then(|o| o.name.clone())
                    {
                        g.name = name;
                    }
                    g
                })
                .collect();
            (games, None)
        }
        Err(e) => (Vec::new(), Some(e)),
    };
    Settings {
        api_key: state.api_key.lock().unwrap().clone().unwrap_or_default(),
        custom_games,
        hidden_games: listing.hidden,
        preferences: listing.preferences,
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        config_error,
        data_dir: state.data_dir.clone(),
    }
}

// Creating windows from a synchronous command can deadlock on Windows,
// so these two are async.
#[tauri::command]
async fn open_settings(app: tauri::AppHandle) -> Result<(), String> {
    windows::open_settings(&app).map_err(|e| e.to_string())
}

#[tauri::command]
async fn open_configure(app: tauri::AppHandle, id: String) -> Result<(), String> {
    windows::open_configure(&app, &id).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    tray::set_autostart(&app, enabled)
}

#[tauri::command]
fn set_game_hidden(state: State<'_, AppState>, id: String, hidden: bool) -> Result<(), String> {
    state.update_config(|config| {
        config.set_hidden(&id, hidden);
        Ok(())
    })
}

#[tauri::command]
fn save_api_key(state: State<'_, AppState>, key: String) -> Result<(), String> {
    config::save_api_key(&state.data_dir, &key)?;
    let key = key.trim().to_string();
    *state.api_key.lock().unwrap() = (!key.is_empty()).then_some(key);
    Ok(())
}

#[tauri::command]
fn add_custom_game(
    state: State<'_, AppState>,
    name: String,
    path: String,
    args: String,
) -> Result<config::CustomGame, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Enter a name for the game".into());
    }
    if !std::path::Path::new(&path).is_file() {
        return Err("Choose a file that exists".into());
    }

    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let game = config::CustomGame {
        id: millis.to_string(),
        name,
        path,
        args: args.trim().to_string(),
    };
    state.update_config(|config| {
        config.games.push(game.clone());
        Ok(())
    })?;
    Ok(game)
}

#[tauri::command]
fn remove_custom_game(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let removed = state.update_config(|config| {
        let Some(index) = config.games.iter().position(|g| g.id == id) else {
            return Ok(None);
        };
        let game_id = games::Game::from(config.games.remove(index)).id;
        config.set_hidden(&game_id, false);
        config.played.remove(&game_id);
        config.overrides.remove(&game_id);
        Ok(Some(game_id))
    })?;
    if let Some(game_id) = removed {
        artwork::clear(&state.art_root(), &game_id);
    }
    Ok(())
}

fn main() {
    let data_dir = config::data_dir();
    let state = AppState {
        api_key: Mutex::new(config::load_api_key(&data_dir)),
        config_lock: Mutex::new(()),
        data_dir,
        // Without a timeout a stalled request would block artwork loading forever.
        client: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("couldn't create the HTTP client"),
    };

    tauri::Builder::default()
        // Starting the app again just shows the running shelf.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_shelf(app)
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        // Remember where the widget was placed and how big it was.
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(StateFlags::POSITION | StateFlags::SIZE)
                .build(),
        )
        .setup(|app| {
            // Let the page load images from the artwork folder.
            let art_root = app.state::<AppState>().art_root();
            std::fs::create_dir_all(&art_root)?;
            app.asset_protocol_scope()
                .allow_directory(&art_root, true)?;
            tray::create(app.handle())?;
            if let Some(shelf) = app.get_webview_window("main") {
                prefer_low_memory(&shelf);
                if let Ok(hwnd) = shelf.hwnd() {
                    desktop::attach(hwnd.0 as _);
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the widget (e.g. Alt+F4) hides it; quit from the tray icon.
            // Settings and configure windows close normally.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            list_games,
            launch_game,
            get_artwork,
            get_settings,
            set_game_hidden,
            save_api_key,
            add_custom_game,
            remove_custom_game,
            set_autostart,
            set_preferences,
            rename_game,
            search_steamgriddb,
            set_game_match,
            artwork_options,
            choose_artwork,
            rescan_artwork,
            open_settings,
            open_configure,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Game Shelf");
}
