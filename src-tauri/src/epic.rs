//! Finds installed Epic Games Store games from the launcher's manifest files.

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone)]
pub struct EpicGame {
    pub app_name: String,
    pub name: String,
    pub launch_url: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Manifest {
    display_name: String,
    app_name: String,
    catalog_namespace: String,
    catalog_item_id: String,
    #[serde(default)]
    app_categories: Vec<String>,
    #[serde(default, rename = "bIsIncompleteInstall")]
    is_incomplete_install: bool,
}

fn manifests_dir() -> PathBuf {
    let program_data = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into());
    PathBuf::from(program_data).join(r"Epic\EpicGamesLauncher\Data\Manifests")
}

fn parse_manifest(text: &str) -> Option<EpicGame> {
    let m: Manifest = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;

    // Add-ons (like "Fortnite Battle Royale") and engine tools aren't tagged "games".
    if m.is_incomplete_install || !m.app_categories.iter().any(|c| c == "games") {
        return None;
    }
    let launch_url = format!(
        "com.epicgames.launcher://apps/{}%3A{}%3A{}?action=launch&silent=true",
        m.catalog_namespace, m.catalog_item_id, m.app_name
    );
    Some(EpicGame {
        app_name: m.app_name,
        name: m.display_name,
        launch_url,
    })
}

/// Lists installed Epic games. Returns nothing if Epic isn't installed.
pub fn installed_games() -> Vec<EpicGame> {
    let Ok(entries) = std::fs::read_dir(manifests_dir()) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "item"))
        // Unreadable or corrupt manifests are skipped.
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|text| parse_manifest(&text))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(categories: &str, incomplete: bool) -> String {
        format!(
            r#"{{ "DisplayName": "Fortnite", "AppName": "Fortnite", "CatalogNamespace": "fn",
                 "CatalogItemId": "4fe75bbc", "AppCategories": [{categories}],
                 "bIsIncompleteInstall": {incomplete} }}"#
        )
    }

    #[test]
    fn reads_game_and_builds_launch_url() {
        let game = parse_manifest(&manifest(r#""games","applications""#, false)).unwrap();
        assert_eq!(game.name, "Fortnite");
        assert_eq!(
            game.launch_url,
            "com.epicgames.launcher://apps/fn%3A4fe75bbc%3AFortnite?action=launch&silent=true"
        );
    }

    #[test]
    fn handles_byte_order_mark() {
        let text = format!("\u{feff}{}", manifest(r#""games""#, false));
        assert!(parse_manifest(&text).is_some());
    }

    #[test]
    fn skips_addons_incomplete_and_corrupt_manifests() {
        assert!(parse_manifest(&manifest(r#""addons/launchable","addons""#, false)).is_none());
        assert!(parse_manifest(&manifest(r#""games""#, true)).is_none());
        assert!(parse_manifest("\0\0\0\0").is_none());
    }
}
