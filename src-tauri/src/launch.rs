//! Starts games.

use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;

use tauri_plugin_opener::OpenerExt;

use crate::games::{Game, Target};

pub fn launch(app: &tauri::AppHandle, game: &Game) -> Result<(), String> {
    match &game.target {
        // The store launcher handles updates, cloud saves and DRM.
        Target::Url(url) => app
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|e| format!("Couldn't start {}: {e}", game.name)),
        Target::File { path, args } => launch_file(app, Path::new(path), args)
            .map_err(|e| format!("Couldn't start {}: {e}", game.name)),
    }
}

fn launch_file(app: &tauri::AppHandle, path: &Path, args: &str) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("{} doesn't exist", path.display()));
    }

    let is_exe = path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"));
    if !is_exe {
        // Shortcuts and other files open the way Explorer would open them.
        return app
            .opener()
            .open_path(path.to_string_lossy(), None::<&str>)
            .map_err(|e| e.to_string());
    }

    // Many games expect to start in their own folder.
    let mut command = Command::new(path);
    if let Some(dir) = path.parent() {
        command.current_dir(dir);
    }
    if !args.trim().is_empty() {
        // Passed through untouched so quoting works as it would in a shortcut.
        command.raw_arg(args.trim());
    }
    command.spawn().map(|_| ()).map_err(|e| e.to_string())
}
