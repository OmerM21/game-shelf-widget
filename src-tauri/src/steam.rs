//! Finds installed Steam games by reading the local Steam library files.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::vdf;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamGame {
    pub app_id: u32,
    pub name: String,
    pub last_played: u64,
}

/// App IDs that show up as installed but aren't games.
const NON_GAME_APP_IDS: &[u32] = &[
    228980,  // Steamworks Common Redistributables
    1070560, // Steam Linux Runtime
    1391110, // Steam Linux Runtime - Soldier
    1628350, // Steam Linux Runtime - Sniper
];

const NON_GAME_NAME_PREFIXES: &[&str] = &["Proton ", "Steam Linux Runtime", "Steamworks"];

/// The manifest's StateFlags bit that means "fully installed".
const STATE_FULLY_INSTALLED: u32 = 4;

/// Locates the Steam install folder, preferring the registry entry.
pub fn steam_root() -> Option<PathBuf> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let from_registry = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Valve\Steam")
        .and_then(|key| key.get_value::<String, _>("SteamPath"))
        .ok()
        .map(PathBuf::from);

    from_registry
        .into_iter()
        .chain([PathBuf::from(r"C:\Program Files (x86)\Steam")])
        .find(|p| p.join("steamapps").is_dir())
}

/// Returns every Steam library folder (the ones containing `steamapps`).
fn library_folders(steam_root: &Path) -> Vec<PathBuf> {
    let mut folders = vec![steam_root.to_path_buf()];

    let vdf_path = steam_root.join("steamapps").join("libraryfolders.vdf");
    if let Some(root) = std::fs::read_to_string(vdf_path)
        .ok()
        .and_then(|text| vdf::parse(&text).ok())
    {
        if let Some(libs) = root.get_obj("libraryfolders") {
            for (_, lib) in libs.entries() {
                if let vdf::Value::Obj(lib) = lib {
                    if let Some(path) = lib.get_str("path") {
                        folders.push(PathBuf::from(path));
                    }
                }
            }
        }
    }

    // The main Steam folder is usually listed again in libraryfolders.vdf.
    let mut seen = std::collections::HashSet::new();
    folders.retain(|p| seen.insert(p.to_string_lossy().to_lowercase().replace('/', "\\")));
    folders
}

fn parse_manifest(text: &str) -> Option<SteamGame> {
    let root = vdf::parse(text).ok()?;
    let state = root.get_obj("AppState")?;

    let app_id: u32 = state.get_str("appid")?.parse().ok()?;
    let name = state.get_str("name")?.to_string();
    let flags: u32 = state
        .get_str("StateFlags")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let last_played = state
        .get_str("LastPlayed")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    if flags & STATE_FULLY_INSTALLED == 0 || !is_game(app_id, &name) {
        return None;
    }
    Some(SteamGame {
        app_id,
        name,
        last_played,
    })
}

fn is_game(app_id: u32, name: &str) -> bool {
    !NON_GAME_APP_IDS.contains(&app_id)
        && !NON_GAME_NAME_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
}

/// Lists all installed Steam games across every library, sorted by name.
pub fn installed_games() -> Result<Vec<SteamGame>, String> {
    let root = steam_root().ok_or("Couldn't find a Steam installation")?;

    let mut games: Vec<SteamGame> = library_folders(&root)
        .iter()
        .filter_map(|lib| std::fs::read_dir(lib.join("steamapps")).ok())
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with("appmanifest_") && name.ends_with(".acf")
        })
        .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
        .filter_map(|text| parse_manifest(&text))
        .collect();

    games.sort_by_key(|g| g.name.to_lowercase());
    games.dedup_by_key(|g| g.app_id);
    Ok(games)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(app_id: u32, name: &str, flags: u32) -> String {
        format!(
            r#""AppState" {{ "appid" "{app_id}" "name" "{name}" "StateFlags" "{flags}" "LastPlayed" "1700000000" }}"#
        )
    }

    #[test]
    fn reads_installed_game() {
        let game = parse_manifest(&manifest(367520, "Hollow Knight", 4)).unwrap();
        assert_eq!(game.app_id, 367520);
        assert_eq!(game.name, "Hollow Knight");
        assert_eq!(game.last_played, 1700000000);
    }

    #[test]
    fn skips_partially_installed_games() {
        assert!(parse_manifest(&manifest(367520, "Hollow Knight", 1026)).is_none());
    }

    /// Prints the games on this machine. Run with `cargo test -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn lists_local_games() {
        for game in installed_games().unwrap() {
            println!("{:>8}  {}", game.app_id, game.name);
        }
    }

    #[test]
    fn skips_tools_and_redistributables() {
        assert!(
            parse_manifest(&manifest(228980, "Steamworks Common Redistributables", 4)).is_none()
        );
        assert!(parse_manifest(&manifest(2805730, "Proton 9.0", 4)).is_none());
    }
}
