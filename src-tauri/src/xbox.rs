//! Finds games installed with the Xbox app / Microsoft Store.
//!
//! The Xbox app installs PC games into a folder on each drive (normally
//! `XboxGames`) that it records in a `.GamingRoot` file at the drive's root.
//! Each game has a folder there with `Content\appxmanifest.xml` (how Windows
//! launches it) and `Content\MicrosoftGame.config` (its name). DLC gets its
//! own folder too, but has no application to launch, so it's skipped.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct XboxGame {
    /// The package name, e.g. `Microsoft.624F8B84B80` for Forza Horizon 5.
    pub package_name: String,
    pub name: String,
    /// `shell:AppsFolder\<package family name>!<app id>`, which Windows launches.
    pub launch_uri: String,
}

/// Reads a `.GamingRoot` file: "RGBX", a 4-byte version, then one or more
/// UTF-16 folder paths relative to the drive, each ending in a null.
fn parse_gaming_root(bytes: &[u8], drive: &Path) -> Vec<PathBuf> {
    if bytes.len() < 8 || &bytes[..4] != b"RGBX" {
        return Vec::new();
    }
    let units: Vec<u16> = bytes[8..]
        .as_chunks::<2>().0.iter()
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
        .split('\0')
        .map(|p| p.trim().trim_start_matches(['\\', '/']))
        .filter(|p| !p.is_empty())
        .map(|p| drive.join(p))
        .collect()
}

/// Drives that exist and are local disks. Network and optical drives are
/// skipped: reading a file on an offline network drive can block for
/// seconds, and this runs on every game lookup.
fn local_drives() -> Vec<PathBuf> {
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;

    let present = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| present & (1 << i) != 0)
        .map(|i| format!("{}:\\", (b'A' + i) as char))
        .filter(|root| {
            let wide: Vec<u16> = root.encode_utf16().chain([0]).collect();
            matches!(
                unsafe { GetDriveTypeW(wide.as_ptr()) },
                DRIVE_FIXED | DRIVE_REMOVABLE
            )
        })
        .map(PathBuf::from)
        .collect()
}

/// Every folder the Xbox app installs games into, across all drives.
fn gaming_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = local_drives()
        .into_iter()
        .filter_map(|drive| {
            let bytes = std::fs::read(drive.join(".GamingRoot")).ok()?;
            Some(parse_gaming_root(&bytes, &drive))
        })
        .flatten()
        .collect();
    if roots.is_empty() {
        roots.push(PathBuf::from(r"C:\XboxGames"));
    }
    roots
}

/// The 13-character publisher id at the end of a package family name
/// (`8wekyb3d8bbwe` for Microsoft): the first 8 bytes of the SHA-256 of the
/// UTF-16 publisher string, in Crockford-style base32.
fn publisher_id(publisher: &str) -> String {
    const ALPHABET: &[u8; 32] = b"0123456789abcdefghjkmnpqrstvwxyz";
    let utf16: Vec<u8> = publisher
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    let hash = Sha256::digest(&utf16);
    // 64 bits plus one zero bit of padding makes 13 groups of 5 bits.
    let value = u128::from(u64::from_be_bytes(hash[..8].try_into().unwrap())) << 1;
    (0..13)
        .map(|i| ALPHABET[((value >> (60 - 5 * i)) & 0x1f) as usize] as char)
        .collect()
}

fn read_xml(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(text.trim_start_matches('\u{feff}').to_string())
}

fn find<'a>(doc: &'a roxmltree::Document, tag: &str) -> Option<roxmltree::Node<'a, 'a>> {
    doc.descendants().find(|n| n.tag_name().name() == tag)
}

/// Reads one game folder; `None` for DLC, or anything that isn't a game.
fn parse_game(dir: &Path) -> Option<XboxGame> {
    let content = dir.join("Content");
    let manifest_text = read_xml(&content.join("appxmanifest.xml"))?;
    let manifest = roxmltree::Document::parse(&manifest_text).ok()?;

    let identity = find(&manifest, "Identity")?;
    let package_name = identity.attribute("Name")?.to_string();
    let publisher = identity.attribute("Publisher")?;
    // DLC has no application of its own.
    let app_id = find(&manifest, "Application")?.attribute("Id")?;

    // The friendly name is in MicrosoftGame.config; the manifest's is often a
    // resource reference like "ms-resource:...".
    let config_text = read_xml(&content.join("MicrosoftGame.config"));
    let config = config_text
        .as_deref()
        .and_then(|t| roxmltree::Document::parse(t).ok());
    let from_config = config.as_ref().and_then(|c| {
        let executable = find(c, "Executable").and_then(|n| n.attribute("OverrideDisplayName"));
        let visuals = find(c, "ShellVisuals").and_then(|n| n.attribute("DefaultDisplayName"));
        executable.or(visuals).map(str::to_string)
    });
    let from_manifest = find(&manifest, "DisplayName")
        .and_then(|n| n.text())
        .filter(|t| !t.starts_with("ms-resource:"))
        .map(str::to_string);
    let name = from_config
        .or(from_manifest)
        .or_else(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()))?;

    let family = format!("{package_name}_{}", publisher_id(publisher));
    Some(XboxGame {
        launch_uri: format!(r"shell:AppsFolder\{family}!{app_id}"),
        package_name,
        name,
    })
}

/// Lists installed Xbox app games. Returns nothing if there are none.
pub fn installed_games() -> Vec<XboxGame> {
    gaming_roots()
        .iter()
        .filter_map(|root| std::fs::read_dir(root).ok())
        .flatten()
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| parse_game(&e.path()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MICROSOFT: &str =
        "CN=Microsoft Corporation, O=Microsoft Corporation, L=Redmond, S=Washington, C=US";

    /// Prints the Xbox games on this machine. Run with `cargo test -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn lists_local_xbox_games() {
        for game in installed_games() {
            println!("{}  ->  {}", game.name, game.launch_uri);
        }
    }

    #[test]
    fn computes_microsofts_publisher_id() {
        assert_eq!(publisher_id(MICROSOFT), "8wekyb3d8bbwe");
    }

    #[test]
    fn reads_gaming_root_file() {
        let mut bytes = b"RGBX\x01\x00\x00\x00".to_vec();
        bytes.extend("XboxGames\0".encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(
            parse_gaming_root(&bytes, Path::new(r"D:\")),
            [PathBuf::from(r"D:\XboxGames")]
        );
        assert!(parse_gaming_root(b"nope", Path::new(r"D:\")).is_empty());
    }

    fn temp_game(name: &str, manifest: &str, config: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("game-shelf-xbox-{name}-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("Content")).unwrap();
        std::fs::write(dir.join("Content/appxmanifest.xml"), manifest).unwrap();
        std::fs::write(dir.join("Content/MicrosoftGame.config"), config).unwrap();
        dir
    }

    #[test]
    fn reads_game_and_builds_launch_uri() {
        let dir = temp_game(
            "game",
            &format!(
                r#"<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10">
                     <Identity Name="Microsoft.624F8B84B80" Publisher="{MICROSOFT}" />
                     <Properties><DisplayName>ms-resource:Title</DisplayName></Properties>
                     <Applications><Application Id="Forzahorizon5" Executable="GameLaunchHelper.exe" /></Applications>
                   </Package>"#
            ),
            // With a byte-order mark, like the real files.
            "\u{feff}<Game><Executable Name=\"ForzaHorizon5.exe\" Id=\"Forzahorizon5\" OverrideDisplayName=\"Forza Horizon 5\" /></Game>",
        );
        let game = parse_game(&dir).unwrap();
        assert_eq!(game.name, "Forza Horizon 5");
        assert_eq!(
            game.launch_uri,
            r"shell:AppsFolder\Microsoft.624F8B84B80_8wekyb3d8bbwe!Forzahorizon5"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn skips_dlc_without_an_application() {
        let dir = temp_game(
            "dlc",
            &format!(
                r#"<Package><Identity Name="Microsoft.Expansion1FH5" Publisher="{MICROSOFT}" /></Package>"#
            ),
            r#"<Game><ShellVisuals DefaultDisplayName="Forza Horizon 5: Hot Wheels" /></Game>"#,
        );
        assert!(parse_game(&dir).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
