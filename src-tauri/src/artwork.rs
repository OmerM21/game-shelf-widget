//! Downloads game artwork from SteamGridDB and caches it on disk.
//!
//! Each game gets a folder `artwork/<game id>/` holding the images and a
//! `meta.json` that records which ones exist. Once `meta.json` is written the
//! game is never looked up again, so missing art isn't re-queried every launch.
//! Changing a game's art clears its folder, so the next lookup starts fresh.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::games::{ArtLookup, Game};

const API_BASE: &str = "https://www.steamgriddb.com/api/v2";
const META_FILE: &str = "meta.json";
/// Bumped whenever `shrink` changes, so cached images are processed again.
const SIZE_VERSION: u32 = 3;

/// A game's cached images: file names, and the URLs they came from.
/// `None` means SteamGridDB has none.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Meta {
    cover: Option<String>,
    hero: Option<String>,
    logo: Option<String>,
    // Older caches don't have these.
    #[serde(default)]
    cover_url: Option<String>,
    #[serde(default)]
    hero_url: Option<String>,
    #[serde(default)]
    logo_url: Option<String>,
    /// Which version of `shrink` processed the images (0 for older caches).
    #[serde(default)]
    size_version: u32,
}

/// A game's cached images, sent to the frontend.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Artwork {
    pub cover: Option<PathBuf>,
    pub hero: Option<PathBuf>,
    pub logo: Option<PathBuf>,
    /// Where each image came from, so the picker can mark the current one.
    pub cover_url: Option<String>,
    pub hero_url: Option<String>,
    pub logo_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Vertical box art, shown when a game is pulled out.
    Cover,
    /// Wide banner, cropped into the spine background.
    Hero,
    /// Transparent title logo, drawn on the spine.
    Logo,
}

/// How SteamGridDB identifies a game in its URLs.
#[derive(Clone, Copy)]
enum GameRef {
    Steam(u32),
    /// SteamGridDB's own game id.
    Grid(u64),
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Cover => "cover",
            Kind::Hero => "hero",
            Kind::Logo => "logo",
        }
    }

    /// The largest size the shelf ever shows this kind of image at. Bigger
    /// downloads are shrunk to this (banners are also cropped, see `shrink`):
    /// the page keeps every image decoded in memory, and a full-size
    /// 3840x1240 banner alone takes about 19 MB.
    fn max_size(self) -> (u32, u32) {
        match self {
            Kind::Cover => (480, 720),
            // Before it's turned to run along the spine; see `shrink`.
            Kind::Hero => (900, 300),
            Kind::Logo => (640, 320),
        }
    }

    fn endpoint(self, game: GameRef) -> String {
        let (platform, id) = match game {
            GameRef::Steam(id) => ("steam", id as u64),
            GameRef::Grid(id) => ("game", id),
        };
        let filters = "types=static&nsfw=false&humor=false";
        match self {
            // The portrait sizes SteamGridDB offers, all close to 2:3.
            Kind::Cover => format!(
                "{API_BASE}/grids/{platform}/{id}?dimensions=600x900,342x482,660x930&{filters}"
            ),
            Kind::Hero => format!("{API_BASE}/heroes/{platform}/{id}?{filters}"),
            Kind::Logo => format!("{API_BASE}/logos/{platform}/{id}?{filters}"),
        }
    }
}

#[derive(Deserialize)]
struct ApiResponse<T> {
    #[serde(default = "Vec::new")]
    data: Vec<T>,
}

/// One image the user can pick in the artwork picker.
#[derive(Debug, Serialize, Deserialize)]
pub struct ArtOption {
    pub url: String,
    /// A smaller preview.
    pub thumb: String,
}

#[derive(Deserialize)]
struct ApiGame {
    id: u64,
    name: String,
    /// Unix seconds.
    release_date: Option<i64>,
}

/// A SteamGridDB search result.
#[derive(Debug, Serialize)]
pub struct Match {
    pub id: u64,
    pub name: String,
    pub year: Option<i64>,
}

/// Explains a failed request. reqwest's own message ("error sending request
/// for url ...") hides the reason; the innermost error in the chain has it.
fn describe(error: &reqwest::Error) -> String {
    let mut reason = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(inner) = source {
        reason = inner.to_string();
        source = inner.source();
    }
    format!("Couldn't reach SteamGridDB ({reason}). Check your connection and try again.")
}

/// Sends a request, retrying once after a short pause if it never got a
/// response (a dropped connection or a brief network hiccup).
async fn send(request: reqwest::RequestBuilder) -> Result<reqwest::Response, String> {
    let retry = request.try_clone();
    match (request.send().await, retry) {
        (Err(e), Some(retry)) if e.is_connect() || e.is_timeout() || e.is_request() => {
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
            retry.send().await.map_err(|e| describe(&e))
        }
        (result, _) => result.map_err(|e| describe(&e)),
    }
}

/// Calls the API and returns all results; a 404 means none.
async fn fetch_list<T: DeserializeOwned>(
    client: &reqwest::Client,
    key: &str,
    url: reqwest::Url,
) -> Result<Vec<T>, String> {
    let response = send(client.get(url).bearer_auth(key)).await?;

    match response.status() {
        reqwest::StatusCode::NOT_FOUND => return Ok(Vec::new()),
        reqwest::StatusCode::UNAUTHORIZED => {
            return Err("SteamGridDB rejected your API key. Check it in settings.".into())
        }
        status if !status.is_success() => return Err(format!("SteamGridDB returned {status}")),
        _ => {}
    }

    let body: ApiResponse<T> = response
        .json()
        .await
        .map_err(|e| format!("Unexpected SteamGridDB response: {e}"))?;
    Ok(body.data)
}

/// The calendar year of a Unix timestamp.
fn year_of(secs: i64) -> i64 {
    // Howard Hinnant's days-to-civil algorithm, reduced to the year.
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let month = (5 * doy + 2) / 153;
    yoe + era * 400 + i64::from(month >= 10)
}

/// Searches SteamGridDB for games by name.
pub async fn search(
    client: &reqwest::Client,
    key: &str,
    query: &str,
) -> Result<Vec<Match>, String> {
    let mut url = reqwest::Url::parse(&format!("{API_BASE}/search/autocomplete")).unwrap();
    url.path_segments_mut().unwrap().push(query.trim());
    let games: Vec<ApiGame> = fetch_list(client, key, url).await?;
    Ok(games
        .into_iter()
        .map(|g| Match {
            id: g.id,
            name: g.name,
            year: g.release_date.map(year_of),
        })
        .collect())
}

/// Works out which SteamGridDB game to use, or `None` if there's no match.
async fn resolve(
    client: &reqwest::Client,
    key: &str,
    game: &Game,
) -> Result<Option<GameRef>, String> {
    Ok(match &game.art {
        ArtLookup::SteamAppId(id) => Some(GameRef::Steam(*id)),
        ArtLookup::SteamGridDb(id) => Some(GameRef::Grid(*id)),
        ArtLookup::Name(name) => search(client, key, name)
            .await?
            .first()
            .map(|m| GameRef::Grid(m.id)),
    })
}

/// Lists every image of one kind that SteamGridDB has for a game.
pub async fn options(
    client: &reqwest::Client,
    key: &str,
    game: &Game,
    kind: Kind,
) -> Result<Vec<ArtOption>, String> {
    let Some(game_ref) = resolve(client, key, game).await? else {
        return Ok(Vec::new());
    };
    let url = reqwest::Url::parse(&kind.endpoint(game_ref)).unwrap();
    fetch_list(client, key, url).await
}

/// Shrinks an image to fit `kind`'s display size. Logos stay PNG to keep
/// their transparency; covers and banners become JPEG. Returns the new bytes
/// and file extension, or `None` if the image is already small enough or
/// can't be decoded (it's then kept as it is).
fn shrink(bytes: &[u8], kind: Kind) -> Option<(Vec<u8>, &'static str)> {
    let image = image::load_from_memory(bytes).ok()?;
    let original = (image.width(), image.height());
    let (max_w, max_h) = kind.max_size();
    let mut resized = if image.width() > max_w || image.height() > max_h {
        image.resize(max_w, max_h, image::imageops::FilterType::Lanczos3)
    } else {
        image
    };
    if kind == Kind::Hero {
        // The banner runs along the spine: turned a quarter counter-clockwise,
        // so it reads bottom to top like the logo. A spine is far narrower than
        // the turned banner, so only a band from its middle ever shows; keep
        // a generous one (a quarter of its length) and drop the rest.
        resized = resized.rotate270();
        let band = (resized.height() / 4).max(1);
        if resized.width() > band {
            resized = resized.crop_imm((resized.width() - band) / 2, 0, band, resized.height());
        }
    }
    if (resized.width(), resized.height()) == original {
        return None;
    }

    let mut out = std::io::Cursor::new(Vec::new());
    if kind == Kind::Logo {
        resized.write_to(&mut out, image::ImageFormat::Png).ok()?;
        Some((out.into_inner(), "png"))
    } else {
        let rgb = image::DynamicImage::ImageRgb8(resized.to_rgb8());
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90)
            .encode_image(&rgb)
            .ok()?;
        Some((out.into_inner(), "jpg"))
    }
}

/// Saves image bytes into `dir`, shrunk if needed, and returns the file name.
/// Each file gets a new name so the webview never shows a stale cached copy.
async fn save_image(
    bytes: Vec<u8>,
    original_ext: String,
    dir: &Path,
    kind: Kind,
) -> Result<String, String> {
    // Decoding and resizing takes a moment; keep it off the async workers.
    let (bytes, ext) = tauri::async_runtime::spawn_blocking(move || match shrink(&bytes, kind) {
        Some((small, ext)) => (small, ext.to_string()),
        None => (bytes, original_ext),
    })
    .await
    .map_err(|e| e.to_string())?;

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let file_name = format!("{}-{stamp}.{ext}", kind.name());
    std::fs::write(dir.join(&file_name), &bytes)
        .map_err(|e| format!("Couldn't save {}: {e}", kind.name()))?;
    Ok(file_name)
}

fn extension_of(name: &str) -> String {
    Path::new(name.split('?').next().unwrap_or(name))
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .filter(|e| ["png", "jpg", "jpeg", "webp"].contains(&e.as_str()))
        .unwrap_or_else(|| "png".into())
}

/// Downloads `url` into `dir` and returns the file name.
async fn download(
    client: &reqwest::Client,
    url: &str,
    dir: &Path,
    kind: Kind,
) -> Result<String, String> {
    let bytes = send(client.get(url))
        .await?
        .error_for_status()
        .map_err(|e| format!("Couldn't download the {}: {e}", kind.name()))?
        .bytes()
        .await
        .map_err(|e| describe(&e))?;
    save_image(bytes.to_vec(), extension_of(url), dir, kind).await
}

/// Re-processes images cached by an older version of `shrink`, without
/// downloading them again.
async fn upgrade_cache(
    client: &reqwest::Client,
    key: Option<&str>,
    game: &Game,
    dir: &Path,
    meta: &mut Meta,
) {
    // Covers and logos only need resizing, which works on the cached files.
    for (kind, file) in [(Kind::Cover, &mut meta.cover), (Kind::Logo, &mut meta.logo)] {
        let Some(name) = file.clone() else { continue };
        // Already small enough: leave the file alone (only its header is read).
        let (max_w, max_h) = kind.max_size();
        if let Ok((w, h)) = image::image_dimensions(dir.join(&name)) {
            if w <= max_w && h <= max_h {
                continue;
            }
        }
        let Ok(bytes) = std::fs::read(dir.join(&name)) else {
            continue;
        };
        let Ok(new_name) = save_image(bytes, extension_of(&name), dir, kind).await else {
            continue;
        };
        let _ = std::fs::remove_file(dir.join(&name));
        *file = Some(new_name);
    }

    // Banners used to be stored as a narrow upright strip, so the full image
    // has to be downloaded again. If that fails (offline, no key), the old
    // strip stays and the upgrade is tried again next time.
    if let Some(old) = meta.hero.clone() {
        let Some(key) = key else { return };
        let url = match meta
            .hero_url
            .clone()
            .or_else(|| game.overrides.hero.clone())
        {
            Some(url) => Some(url),
            None => match fetch_kind_url(client, key, Kind::Hero, game).await {
                Ok(url) => url,
                Err(_) => return,
            },
        };
        if let Some(url) = url {
            let Ok(name) = download(client, &url, dir, Kind::Hero).await else {
                return;
            };
            let _ = std::fs::remove_file(dir.join(&old));
            meta.hero = Some(name);
            meta.hero_url = Some(url);
        }
    }
    meta.size_version = SIZE_VERSION;
}

/// The top-rated image URL of one kind for a game, if there is one.
async fn fetch_kind_url(
    client: &reqwest::Client,
    key: &str,
    kind: Kind,
    game: &Game,
) -> Result<Option<String>, String> {
    let Some(game_ref) = resolve(client, key, game).await? else {
        return Ok(None);
    };
    let endpoint = reqwest::Url::parse(&kind.endpoint(game_ref)).unwrap();
    Ok(fetch_list::<ArtOption>(client, key, endpoint)
        .await?
        .into_iter()
        .next()
        .map(|o| o.url))
}

/// Fetches one kind of image: the one the user picked, or else the top-rated
/// one. Returns the cached file name and its source URL.
async fn fetch_kind(
    client: &reqwest::Client,
    key: &str,
    kind: Kind,
    game_ref: Option<GameRef>,
    picked: Option<&str>,
    dir: &Path,
) -> Result<(Option<String>, Option<String>), String> {
    let url = match (picked, game_ref) {
        (Some(url), _) => Some(url.to_string()),
        (None, Some(game_ref)) => {
            let endpoint = reqwest::Url::parse(&kind.endpoint(game_ref)).unwrap();
            fetch_list::<ArtOption>(client, key, endpoint)
                .await?
                .into_iter()
                .next()
                .map(|o| o.url)
        }
        (None, None) => None,
    };
    match url {
        Some(url) => Ok((Some(download(client, &url, dir, kind).await?), Some(url))),
        None => Ok((None, None)),
    }
}

fn read_meta(dir: &Path) -> Option<Meta> {
    let text = std::fs::read_to_string(dir.join(META_FILE)).ok()?;
    serde_json::from_str(&text).ok()
}

fn to_artwork(dir: &Path, meta: Meta) -> Artwork {
    let resolve = |name: Option<String>| name.map(|n| dir.join(n));
    Artwork {
        cover: resolve(meta.cover),
        hero: resolve(meta.hero),
        logo: resolve(meta.logo),
        cover_url: meta.cover_url,
        hero_url: meta.hero_url,
        logo_url: meta.logo_url,
    }
}

/// Returns a game's artwork, downloading it on first use.
pub async fn get(
    client: &reqwest::Client,
    api_key: Option<&str>,
    art_root: &Path,
    game: &Game,
) -> Result<Artwork, String> {
    // One task at a time per game: two at once could each replace the files
    // and leave meta.json pointing at one the other just deleted.
    let lock = game_lock(&game.id);
    let _guard = lock.lock().await;

    let dir = art_root.join(&game.id);
    if let Some(mut meta) = read_meta(&dir) {
        if meta.size_version < SIZE_VERSION {
            upgrade_cache(client, api_key, game, &dir, &mut meta).await;
            write_meta(&dir, &meta)?;
        }
        if files_exist(&dir, &meta) {
            return Ok(to_artwork(&dir, meta));
        }
        // A file went missing: start over rather than show a broken image.
        clear_dir(&dir);
    }

    let key = api_key.ok_or("Add your SteamGridDB API key in settings to get artwork")?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Couldn't create the artwork folder: {e}"))?;

    let game_ref = resolve(client, key, game).await?;
    let picked = &game.overrides;
    let (cover, hero, logo) = tokio::join!(
        fetch_kind(
            client,
            key,
            Kind::Cover,
            game_ref,
            picked.cover.as_deref(),
            &dir
        ),
        fetch_kind(
            client,
            key,
            Kind::Hero,
            game_ref,
            picked.hero.as_deref(),
            &dir
        ),
        fetch_kind(
            client,
            key,
            Kind::Logo,
            game_ref,
            picked.logo.as_deref(),
            &dir
        ),
    );
    // Any error leaves meta.json unwritten, so the next launch retries.
    let ((cover, cover_url), (hero, hero_url), (logo, logo_url)) = (cover?, hero?, logo?);
    let meta = Meta {
        cover,
        hero,
        logo,
        cover_url,
        hero_url,
        logo_url,
        size_version: SIZE_VERSION,
    };
    write_meta(&dir, &meta)?;
    Ok(to_artwork(&dir, meta))
}

fn write_meta(dir: &Path, meta: &Meta) -> Result<(), String> {
    let json = serde_json::to_string_pretty(meta).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(META_FILE), json)
        .map_err(|e| format!("Couldn't save the artwork cache: {e}"))
}

/// The lock that keeps two tasks from processing one game's art at once.
fn game_lock(game_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();
    let mut locks = LOCKS.get_or_init(Default::default).lock().unwrap();
    locks.entry(game_id.to_string()).or_default().clone()
}

/// Whether every image `meta` refers to is actually on disk.
fn files_exist(dir: &Path, meta: &Meta) -> bool {
    [&meta.cover, &meta.hero, &meta.logo]
        .into_iter()
        .flatten()
        .all(|name| dir.join(name).is_file())
}

fn clear_dir(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

/// Deletes a game's cached artwork so it's downloaded again next time.
pub fn clear(art_root: &Path, game_id: &str) {
    clear_dir(&art_root.join(game_id));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_meta_resolves_to_absolute_paths() {
        let dir = Path::new(r"C:\shelf\artwork\steam-367520");
        let meta = Meta {
            cover: Some("cover.png".into()),
            logo: Some("logo.png".into()),
            cover_url: Some("https://cdn/cover.png".into()),
            ..Default::default()
        };
        let art = to_artwork(dir, meta);
        assert_eq!(art.cover, Some(dir.join("cover.png")));
        assert_eq!(art.hero, None);
        assert_eq!(art.logo, Some(dir.join("logo.png")));
        assert_eq!(art.cover_url.as_deref(), Some("https://cdn/cover.png"));
    }

    #[test]
    fn reads_caches_written_before_urls_were_stored() {
        let meta: Meta =
            serde_json::from_str(r#"{ "cover": "cover.png", "hero": null, "logo": null }"#)
                .unwrap();
        assert_eq!(meta.cover.as_deref(), Some("cover.png"));
        assert_eq!(meta.cover_url, None);
    }

    #[test]
    fn notices_missing_cached_files() {
        let dir = std::env::temp_dir().join(format!("game-shelf-art-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hero-1.jpg"), b"x").unwrap();
        let meta = |cover: &str| Meta {
            cover: Some(cover.into()),
            hero: Some("hero-1.jpg".into()),
            ..Default::default()
        };

        std::fs::write(dir.join("cover-1.jpg"), b"x").unwrap();
        assert!(files_exist(&dir, &meta("cover-1.jpg")));
        assert!(!files_exist(&dir, &meta("cover.png")));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn endpoints_use_steam_or_steamgriddb_ids() {
        let steam = Kind::Cover.endpoint(GameRef::Steam(367520));
        assert!(steam.starts_with("https://www.steamgriddb.com/api/v2/grids/steam/367520?"));
        assert!(steam.contains("dimensions=600x900") && steam.contains("types=static"));

        let grid = Kind::Logo.endpoint(GameRef::Grid(5258));
        assert!(grid.starts_with("https://www.steamgriddb.com/api/v2/logos/game/5258?"));
    }

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(width, height)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn shrinks_large_images_to_display_size() {
        let (bytes, ext) = shrink(&png(1200, 1800), Kind::Cover).unwrap();
        let cover = image::load_from_memory(&bytes).unwrap();
        assert_eq!(ext, "jpg");
        assert_eq!((cover.width(), cover.height()), (480, 720));
    }

    #[test]
    fn banners_are_turned_to_run_along_the_spine() {
        // A wide banner whose left half is red and right half blue.
        let mut banner = image::RgbImage::new(3840, 1240);
        for (x, _, pixel) in banner.enumerate_pixels_mut() {
            *pixel = if x < 1920 {
                image::Rgb([255, 0, 0])
            } else {
                image::Rgb([0, 0, 255])
            };
        }
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(banner)
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();

        // 3840x1240 fits 900x300 as 900x291, turns into 291x900, and the
        // middle quarter of its length (225 px) is kept.
        let (bytes, _) = shrink(bytes.get_ref(), Kind::Hero).unwrap();
        let spine = image::load_from_memory(&bytes).unwrap().to_rgb8();
        assert_eq!(spine.dimensions(), (225, 900));
        // Turned counter-clockwise: the banner's left end is now at the bottom.
        assert!(spine.get_pixel(112, 890)[0] > 200, "bottom should be red");
        assert!(spine.get_pixel(112, 10)[2] > 200, "top should be blue");
    }

    #[test]
    fn logos_stay_png_and_small_images_are_left_alone() {
        let (_, ext) = shrink(&png(1600, 600), Kind::Logo).unwrap();
        assert_eq!(ext, "png");
        assert!(shrink(&png(400, 600), Kind::Cover).is_none());
        assert!(shrink(b"not an image", Kind::Cover).is_none());
    }

    #[test]
    fn converts_release_dates_to_years() {
        assert_eq!(year_of(0), 1970);
        assert_eq!(year_of(1_588_291_200), 2020); // 2020-05-01
        assert_eq!(year_of(1_609_459_199), 2020); // 2020-12-31 23:59:59
        assert_eq!(year_of(1_609_459_200), 2021); // 2021-01-01
    }
}
