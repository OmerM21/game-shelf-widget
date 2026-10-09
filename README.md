# Game Shelf Widget

[![Latest release](https://img.shields.io/github/v/release/OmerM21/game-shelf-widget)](https://github.com/OmerM21/game-shelf-widget/releases/latest)
[![CI](https://github.com/OmerM21/game-shelf-widget/actions/workflows/ci.yml/badge.svg)](https://github.com/OmerM21/game-shelf-widget/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A small game launcher for Windows that sits on your desktop like a widget.
Your games stand on a shelf like book spines. Hover over one to pull it out and
see its cover, then click to play.

- **Finds your games:** Steam, Epic Games Store, and Xbox app / Microsoft Store
  games are detected automatically. You can add anything else (an .exe, a
  shortcut, another launcher) yourself.
- **Artwork from [SteamGridDB](https://www.steamgriddb.com/):** covers, spine
  banners and logos are downloaded for every game, and you can pick different ones.
- **Lives on the desktop:** no window frame, transparent, behind your other
  windows, and still visible on "Show desktop" (Win+D), like desktop icons.
- **Light:** about 180 MB of memory, most of it the WebView2 engine that comes
  with Windows.

## Install

1. Download the latest **`game-shelf-widget_x.y.z_x64-setup.exe`** from
   [Releases](https://github.com/OmerM21/game-shelf-widget/releases/latest)
   and run it. No admin rights are needed; it installs to
   `%LOCALAPPDATA%\Game Shelf`.

   Prefer not to install? Download **`game-shelf-widget_x.y.z_portable.exe`**
   instead, put it in a folder of its own, and run it from there.
2. The app isn't code-signed, so Windows SmartScreen may warn about it. Click
   **More info → Run anyway**.
3. Get a free SteamGridDB API key at
   [steamgriddb.com/profile/preferences/api](https://www.steamgriddb.com/profile/preferences/api)
   (you need a SteamGridDB account) and paste it into the app's settings
   (hover the shelf's top-right corner, or right-click the tray icon).
4. In settings, turn on **Start with Windows** if you want the shelf there
   every time you log in.

Requires Windows 10 or 11 with the Microsoft Edge WebView2 runtime. It's built
into Windows 11 and up-to-date Windows 10, and the installer downloads it if
it's missing.

## Using it

- **Hover** over a game to pull it out, **click** to play.
- **Right-click** a game to play it, hide it, or **Configure** it:
  - rename it (its artwork is searched again under the new name)
  - pick the right game on SteamGridDB if the automatic match is wrong
  - choose the cover, spine banner and logo from every image available
  - download its artwork again
- **Hover the top-right corner** for settings, the move handle and the resize
  handle.
- **Settings:** start with Windows, sort by name or recently played, hover
  sound and its volume, how slanted the spines are, your SteamGridDB API key,
  games you added yourself, hidden games, and rescanning for newly installed
  games.
- **Tray icon:** left-click shows or hides the shelf; right-click for
  settings, refresh, start with Windows and quit. Closing the shelf (Alt+F4)
  only hides it.

## Your data

The app keeps everything in the folder its .exe is in
(`%LOCALAPPDATA%\Game Shelf` when installed):

| File | What it holds |
|---|---|
| `.env` | Your SteamGridDB API key (see [`.env.example`](.env.example)) |
| `games.json` | Games you added, hidden games, renames and artwork picks, when you last played each game, and your preferences (see [`games.example.json`](games.example.json)) |
| `artwork/` | Downloaded covers, banners and logos, one folder per game |

The only network traffic is to SteamGridDB, to search for your games' artwork
and download it. There's no telemetry.

To update, install the new version over the old one; your data stays. To
remove the app, uninstall it from Windows Settings → Apps, then delete
`%LOCALAPPDATA%\Game Shelf` if anything is left there.

## Building from source

You need [Node.js](https://nodejs.org/) 20+, [Rust](https://rustup.rs/), and the
[Visual Studio C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
("Desktop development with C++").

```
npm install
npm run dev      # run with live reload; data is kept in the project folder
npm run build    # make the installer in src-tauri/target/release/bundle/nsis/
```

Run the tests with `cargo test` in `src-tauri`.

Built with [Tauri v2](https://tauri.app/): a Rust backend (`src-tauri/src`) and
a plain HTML/CSS/JavaScript frontend (`src`), with no framework or bundler.

## Contributing

Bug reports, ideas and pull requests are welcome. See
[CONTRIBUTING.md](CONTRIBUTING.md) for how changes get checked and released.

## License

[MIT](LICENSE).

Game Shelf Widget is not affiliated with or endorsed by Valve, Epic Games,
Microsoft, or SteamGridDB. Steam, Epic Games Store and Xbox are trademarks of
their owners. Game artwork comes from SteamGridDB and belongs to its creators.
