# Contributing

Thanks for helping out! Bug reports, ideas and pull requests are all welcome.

## Reporting a bug or asking for a feature

Open an [issue](https://github.com/OmerM21/game-shelf-widget/issues/new/choose).
For bugs, say which version you're using (Windows Settings → Apps shows it
for the installed app), your Windows version, and which store the game comes
from (Steam, Epic, Xbox, or one you added).

Please don't paste your SteamGridDB API key or your `.env` file into an issue.

## Making a change

1. Fork the repo and create a branch from `main`.
2. Set up the tools listed under
   [Building from source](README.md#building-from-source), then
   `npm install` and `npm run dev`.
3. Make your change. Keep the style of the code around it: Rust in
   `src-tauri/src`, plain HTML/CSS/JS (ES modules, no framework) in `src`.
4. Before opening the pull request, run in `src-tauri`:

   ```
   cargo fmt
   cargo clippy --all-targets -- -D warnings
   cargo test
   ```

   CI runs the same checks on every pull request.
5. Open a pull request against `main` and describe what changed and how you
   tested it.

## How releases work

Every pull request merged into `main` publishes a new release automatically:
the [Release workflow](.github/workflows/release.yml) builds the installer and
the portable .exe, tags the commit, and creates a GitHub release with notes
generated from the merged pull requests.

The version goes up by one patch number (0.1.0 → 0.1.1) unless the pull
request has one of these labels:

| Label | Effect |
|---|---|
| `release:minor` | New features: 0.1.3 → 0.2.0 |
| `release:major` | Big or breaking changes: 0.4.2 → 1.0.0 |
| `release:skip` | No release (for example, CI or repo housekeeping) |

Changes that only touch documentation (`*.md`, issue templates) never make a
release.

The version in `tauri.conf.json`, `Cargo.toml` and `package.json` is only a
starting point. Releases take their version from the latest `v*` tag and
stamp it into the build. To jump to a specific version, set it in
`src-tauri/tauri.conf.json` in your pull request; if it's newer than the
latest release, it's used as is.
