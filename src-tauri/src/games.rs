//! Combines games from every source into one list.

use std::path::Path;

use serde::Serialize;

use crate::{config, epic, steam, xbox};

/// How a game is started.
#[derive(Debug, Clone)]
pub enum Target {
    /// A URL handled by a store launcher (steam://, com.epicgames.launcher://,
    /// or shell:AppsFolder for Xbox app games).
    Url(String),
    /// A file on disk: an .exe, shortcut (.lnk/.url) or script.
    File { path: String, args: String },
}

/// What to search SteamGridDB for.
#[derive(Debug, Clone, PartialEq)]
pub enum ArtLookup {
    SteamAppId(u32),
    Name(String),
    /// A SteamGridDB game id the user picked.
    SteamGridDb(u64),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    /// Stable and filesystem-safe; used for the artwork cache folder.
    pub id: String,
    /// The name shown on the shelf (the user's, if they renamed it).
    pub name: String,
    /// The name as detected, before any rename.
    pub original_name: String,
    /// "steam", "epic", "xbox" or "custom".
    pub source: &'static str,
    /// Unix seconds; 0 if never played.
    pub last_played: u64,
    /// What the user changed with "Configure".
    #[serde(rename = "override")]
    pub overrides: config::GameOverride,
    #[serde(skip)]
    pub target: Target,
    #[serde(skip)]
    pub art: ArtLookup,
}

impl Game {
    fn new(id: String, name: String, source: &'static str, target: Target, art: ArtLookup) -> Game {
        Game {
            id,
            original_name: name.clone(),
            name,
            source,
            last_played: 0,
            overrides: config::GameOverride::default(),
            target,
            art,
        }
    }

    /// Applies the user's rename and SteamGridDB match. A rename also changes
    /// what's searched for, since it's usually done because art wasn't found.
    fn apply_override(&mut self, o: &config::GameOverride) {
        if let Some(name) = &o.name {
            self.name = name.clone();
            self.art = ArtLookup::Name(name.clone());
        }
        if let Some(id) = o.sgdb_id {
            self.art = ArtLookup::SteamGridDb(id);
        }
        self.overrides = o.clone();
    }
}

/// Keeps ids safe to use as folder names.
fn safe_id(prefix: &str, raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{prefix}-{cleaned}")
}

impl From<steam::SteamGame> for Game {
    fn from(g: steam::SteamGame) -> Self {
        let mut game = Game::new(
            format!("steam-{}", g.app_id),
            g.name,
            "steam",
            Target::Url(format!("steam://rungameid/{}", g.app_id)),
            ArtLookup::SteamAppId(g.app_id),
        );
        // Steam records play time even when games start from Steam itself.
        game.last_played = g.last_played;
        game
    }
}

impl From<epic::EpicGame> for Game {
    fn from(g: epic::EpicGame) -> Self {
        let art = ArtLookup::Name(g.name.clone());
        Game::new(
            safe_id("epic", &g.app_name),
            g.name,
            "epic",
            Target::Url(g.launch_url),
            art,
        )
    }
}

impl From<xbox::XboxGame> for Game {
    fn from(g: xbox::XboxGame) -> Self {
        let art = ArtLookup::Name(g.name.clone());
        Game::new(
            safe_id("xbox", &g.package_name),
            g.name,
            "xbox",
            Target::Url(g.launch_uri),
            art,
        )
    }
}

impl From<config::CustomGame> for Game {
    fn from(g: config::CustomGame) -> Self {
        let art = ArtLookup::Name(g.name.clone());
        let target = Target::File {
            path: g.path,
            args: g.args,
        };
        Game::new(safe_id("custom", &g.id), g.name, "custom", target, art)
    }
}

#[derive(Debug, Serialize)]
pub struct Listing {
    /// Games shown on the shelf, in the user's chosen order.
    pub games: Vec<Game>,
    /// Games the user hid, listed in settings so they can be unhidden.
    pub hidden: Vec<Game>,
    pub preferences: config::Preferences,
    /// Set when games.json couldn't be read; the other games still load.
    pub warning: Option<String>,
}

fn sort(games: &mut [Game], order: config::SortOrder) {
    games.sort_by_key(|g| g.name.to_lowercase());
    if order == config::SortOrder::Recent {
        // Stable sort, so never-played games stay alphabetical at the end.
        games.sort_by_key(|g| std::cmp::Reverse(g.last_played));
    }
}

/// Lists every game. A missing Steam or Epic install just contributes no games.
pub fn all_games(data_dir: &Path) -> Listing {
    let (config, warning) = match config::Config::load(data_dir) {
        Ok(config) => (config, None),
        Err(e) => (config::Config::default(), Some(e)),
    };

    let mut games: Vec<Game> = steam::installed_games()
        .unwrap_or_default()
        .into_iter()
        .map(Game::from)
        .chain(epic::installed_games().into_iter().map(Game::from))
        .chain(xbox::installed_games().into_iter().map(Game::from))
        .chain(config.games.into_iter().map(Game::from))
        .collect();

    for game in &mut games {
        if let Some(&launched) = config.played.get(&game.id) {
            game.last_played = game.last_played.max(launched);
        }
        if let Some(o) = config.overrides.get(&game.id) {
            game.apply_override(o);
        }
    }
    sort(&mut games, config.preferences.sort);

    let (hidden, games) = games
        .into_iter()
        .partition(|g| config.hidden.contains(&g.id));
    Listing {
        games,
        hidden,
        preferences: config.preferences,
        warning,
    }
}

pub fn find(data_dir: &Path, id: &str) -> Result<Game, String> {
    let listing = all_games(data_dir);
    listing
        .games
        .into_iter()
        .chain(listing.hidden)
        .find(|g| g.id == id)
        .ok_or_else(|| "That game isn't installed anymore".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(name: &str, last_played: u64) -> Game {
        let mut game = Game::new(
            name.into(),
            name.into(),
            "custom",
            Target::Url(String::new()),
            ArtLookup::Name(name.into()),
        );
        game.last_played = last_played;
        game
    }

    #[test]
    fn rename_searches_by_new_name_unless_matched() {
        let mut g = Game::from(steam::SteamGame {
            app_id: 1495710,
            name: "Bonus".into(),
            last_played: 0,
        });
        g.apply_override(&config::GameOverride {
            name: Some("Cyberpunk 2077".into()),
            ..Default::default()
        });
        assert_eq!(g.name, "Cyberpunk 2077");
        assert_eq!(g.original_name, "Bonus");
        assert_eq!(g.art, ArtLookup::Name("Cyberpunk 2077".into()));

        g.apply_override(&config::GameOverride {
            sgdb_id: Some(5258),
            ..Default::default()
        });
        assert_eq!(g.art, ArtLookup::SteamGridDb(5258));
    }

    fn names(games: &[Game]) -> Vec<&str> {
        games.iter().map(|g| g.name.as_str()).collect()
    }

    #[test]
    fn sorts_by_name_ignoring_case() {
        let mut games = vec![game("steep", 5), game("Hades", 0), game("Celeste", 9)];
        sort(&mut games, config::SortOrder::Name);
        assert_eq!(names(&games), ["Celeste", "Hades", "steep"]);
    }

    #[test]
    fn sorts_recent_first_then_unplayed_by_name() {
        let mut games = vec![
            game("Zelda", 0),
            game("Hades", 10),
            game("Abzu", 0),
            game("Celeste", 20),
        ];
        sort(&mut games, config::SortOrder::Recent);
        assert_eq!(names(&games), ["Celeste", "Hades", "Abzu", "Zelda"]);
    }

    #[test]
    fn ids_are_folder_safe() {
        assert_eq!(safe_id("epic", "8769e240:x/y"), "epic-8769e240_x_y");
    }
}
