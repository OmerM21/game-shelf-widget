//! User data kept next to the app: `games.json`, `.env` and the `artwork/` cache.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const CONFIG_FILE: &str = "games.json";
const ENV_FILE: &str = ".env";
const API_KEY_VAR: &str = "STEAMGRIDDB_API_KEY";

/// The folder holding user data. During development that's the project root;
/// in a release build it's the folder containing the .exe.
pub fn data_dir() -> PathBuf {
    if cfg!(debug_assertions) {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf()
    } else {
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

/// A game the user added by hand: an .exe, shortcut, or any other launchable file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomGame {
    pub id: String,
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub args: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub games: Vec<CustomGame>,
    /// Ids of games hidden from the shelf.
    #[serde(default)]
    pub hidden: Vec<String>,
    /// When each game was last launched from the shelf (Unix seconds).
    #[serde(default)]
    pub played: BTreeMap<String, u64>,
    /// Per-game changes made with "Configure", keyed by game id.
    #[serde(default)]
    pub overrides: BTreeMap<String, GameOverride>,
    #[serde(default)]
    pub preferences: Preferences,
}

/// What the user changed about one game. Every field is optional; `None`
/// means "use what was detected automatically".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The SteamGridDB game the user matched this game to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sgdb_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sgdb_name: Option<String>,
    /// Image URLs the user picked instead of the top-rated ones.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hero: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
}

impl GameOverride {
    /// Forgets picked images, e.g. when they belonged to a different match.
    pub fn clear_images(&mut self) {
        self.cover = None;
        self.hero = None;
        self.logo = None;
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortOrder {
    #[default]
    Name,
    /// Most recently played first.
    Recent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    #[serde(default)]
    pub sort: SortOrder,
    #[serde(default)]
    pub hover_sound: bool,
    /// How much shorter a spine's far edge is, in percent of its height
    /// (0 = flat, 12 = strongly angled).
    #[serde(default = "default_slant")]
    pub slant: f32,
    /// Hover sound loudness, 0-100.
    #[serde(default = "default_volume")]
    pub hover_volume: u8,
}

fn default_slant() -> f32 {
    3.0
}

fn default_volume() -> u8 {
    30
}

impl Default for Preferences {
    fn default() -> Self {
        Preferences {
            sort: SortOrder::default(),
            hover_sound: false,
            slant: default_slant(),
            hover_volume: default_volume(),
        }
    }
}

impl Config {
    pub fn set_hidden(&mut self, game_id: &str, hidden: bool) {
        self.hidden.retain(|id| id != game_id);
        if hidden {
            self.hidden.push(game_id.to_string());
        }
    }

    /// Changes a game's override, dropping it entirely once it's back to defaults.
    pub fn edit_override(&mut self, game_id: &str, change: impl FnOnce(&mut GameOverride)) {
        let entry = self.overrides.entry(game_id.to_string()).or_default();
        change(entry);
        if *entry == GameOverride::default() {
            self.overrides.remove(game_id);
        }
    }
}

impl Config {
    pub fn load(dir: &Path) -> Result<Config, String> {
        match std::fs::read_to_string(dir.join(CONFIG_FILE)) {
            // Notepad and PowerShell may save with a byte-order mark.
            Ok(text) => serde_json::from_str(text.trim_start_matches('\u{feff}'))
                .map_err(|e| format!("{CONFIG_FILE} isn't valid: {e}")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(format!("Couldn't read {CONFIG_FILE}: {e}")),
        }
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(CONFIG_FILE), json)
            .map_err(|e| format!("Couldn't save {CONFIG_FILE}: {e}"))
    }
}

/// Reads the SteamGridDB key from `.env`, if one is set.
pub fn load_api_key(dir: &Path) -> Option<String> {
    dotenvy::from_path_iter(dir.join(ENV_FILE))
        .ok()?
        .filter_map(Result::ok)
        .find(|(key, _)| key == API_KEY_VAR)
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Writes the key into `.env`, keeping any other lines already there.
pub fn save_api_key(dir: &Path, key: &str) -> Result<(), String> {
    let path = dir.join(ENV_FILE);
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let new_line = format!("{API_KEY_VAR}={}", key.trim());

    let mut replaced = false;
    let mut lines: Vec<String> = existing
        .lines()
        .map(|line| {
            if line.trim_start().starts_with(&format!("{API_KEY_VAR}=")) {
                replaced = true;
                new_line.clone()
            } else {
                line.to_string()
            }
        })
        .collect();
    if !replaced {
        lines.push(new_line);
    }

    std::fs::write(&path, lines.join("\n") + "\n").map_err(|e| format!("Couldn't save .env: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("game-shelf-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn api_key_round_trips_and_keeps_other_lines() {
        let dir = temp_dir("env");
        std::fs::write(
            dir.join(ENV_FILE),
            "# comment\nSTEAMGRIDDB_API_KEY=old\nOTHER=1\n",
        )
        .unwrap();

        save_api_key(&dir, " new-key ").unwrap();

        assert_eq!(load_api_key(&dir).as_deref(), Some("new-key"));
        let text = std::fs::read_to_string(dir.join(ENV_FILE)).unwrap();
        assert!(text.contains("# comment") && text.contains("OTHER=1") && !text.contains("old"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn empty_api_key_counts_as_missing() {
        let dir = temp_dir("empty");
        std::fs::write(dir.join(ENV_FILE), "STEAMGRIDDB_API_KEY=\n").unwrap();
        assert_eq!(load_api_key(&dir), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn config_with_byte_order_mark_loads() {
        let dir = temp_dir("bom");
        let json = "\u{feff}{ \"games\": [{ \"id\": \"1\", \"name\": \"Notepad\", \"path\": \"n.exe\" }] }";
        std::fs::write(dir.join(CONFIG_FILE), json).unwrap();
        assert_eq!(Config::load(&dir).unwrap().games[0].name, "Notepad");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn hiding_is_idempotent_and_reversible() {
        let mut config = Config::default();
        config.set_hidden("steam-1495710", true);
        config.set_hidden("steam-1495710", true);
        assert_eq!(config.hidden, ["steam-1495710"]);
        config.set_hidden("steam-1495710", false);
        assert!(config.hidden.is_empty());
    }

    #[test]
    fn overrides_back_at_defaults_are_removed() {
        let mut config = Config::default();
        config.edit_override("steam-1", |o| o.name = Some("Renamed".into()));
        assert_eq!(config.overrides["steam-1"].name.as_deref(), Some("Renamed"));
        config.edit_override("steam-1", |o| o.name = None);
        assert!(config.overrides.is_empty());
    }

    #[test]
    fn missing_config_is_empty() {
        let dir = temp_dir("config");
        assert!(Config::load(&dir).unwrap().games.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
