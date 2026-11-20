//! What a mod may hold, and its zip. A mod is content files only: images, sounds, fonts, meshes
//! and font-family json, under `content`, `PlatformContent` or `ExtraContent`. Nothing that runs
//! (scripts, models, places), no config that points the client at other files, nothing outside
//! those folders (`ssl`, the exe). Every file is checked by name and by its first bytes, at
//! import and again at every build: the library folder can be edited by hand.

use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};

/// The folders a mod may change, as Roblox spells them.
const TOPS: [&str; 3] = ["content", "PlatformContent", "ExtraContent"];
pub const MAX_MOD_BYTES: u64 = 1 << 30;
pub const MAX_FILES: usize = 20_000;
pub const MAX_FILE_BYTES: u64 = 256 << 20;
/// Roblox's own longest is 121; a deep data folder plus the copy's path must stay under 260.
const MAX_PATH: usize = 140;
const MAX_JSON_BYTES: usize = 64 << 10;
/// RoALT's own file in each mod's folder, and the optional one in a pack.
pub const META_FILE: &str = "mod.json";

/// A pack's `mod.json`: all a mod says about itself (clipped, control characters dropped).
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(default)]
pub struct Meta {
    pub name: String,
    pub author: String,
    pub version: String,
    pub description: String,
}

impl Meta {
    pub fn clean(self) -> Self {
        Meta { name: clip(&self.name, 60), author: clip(&self.author, 40), version: clip(&self.version, 20), description: clip(&self.description, 300) }
    }
}

fn clip(s: &str, max: usize) -> String {
    s.chars().filter(|c| !c.is_control()).take(max).collect::<String>().trim().to_owned()
}

/// What `unzip` kept.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Unzipped {
    pub files: u32,
    pub bytes: u64,
    pub skipped: u32,
    pub meta: Meta,
}

/// Why a file isn't taken.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// Not under content, PlatformContent or ExtraContent (or not a file there).
    Outside,
    /// A name Windows could read as another file, or too long.
    Name,
    /// A file type mods can't change.
    Type,
}

/// `raw` (either slash) as the path a mod file has in the version folder, top folder in Roblox's
/// spelling; why not, otherwise (pure).
pub fn checked_path(raw: &str) -> std::result::Result<String, Refused> {
    let parts: Vec<&str> = raw.split(['/', '\\']).collect();
    if parts.len() < 2 {
        return Err(Refused::Outside);
    }
    let top = TOPS.iter().find(|t| t.eq_ignore_ascii_case(parts[0])).ok_or(Refused::Outside)?;
    if !parts.iter().all(|p| good_part(p)) {
        return Err(Refused::Name);
    }
    let path = std::iter::once(*top).chain(parts[1..].iter().copied()).collect::<Vec<_>>().join("/");
    if path.len() > MAX_PATH {
        return Err(Refused::Name);
    }
    let ext = parts[parts.len() - 1].rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).ok_or(Refused::Type)?;
    let kind = kind_of(&ext).ok_or(Refused::Type)?;
    if kind == Kind::FontFamily && !path.to_ascii_lowercase().starts_with("content/fonts/families/") {
        return Err(Refused::Type);
    }
    Ok(path)
}

/// One path part Windows reads only one way: plain ASCII, no 8.3 short name (`UNIVER~1`), no
/// device name (`nul.png`), no trailing dot or space, not `.` or `..` (pure).
fn good_part(p: &str) -> bool {
    let plain = !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b" _.()-".contains(&b));
    let short = p.as_bytes().windows(2).any(|w| w[0] == b'~' && w[1].is_ascii_digit());
    // Windows drops spaces before the dot too: `nul .png` is the NUL device.
    let stem = p.split('.').next().unwrap_or("").trim_end_matches(' ').to_ascii_uppercase();
    let device = ["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&stem.as_str())
        || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit());
    plain && !short && !device && !p.ends_with(['.', ' ']) && p != "." && p != ".."
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Image,
    Mesh,
    Sound,
    Font,
    FontFamily,
}

fn kind_of(ext: &str) -> Option<Kind> {
    Some(match ext {
        "png" | "jpg" | "jpeg" | "dds" | "ktx" | "tex" => Kind::Image,
        "mesh" => Kind::Mesh,
        "ogg" | "mp3" | "wav" => Kind::Sound,
        "ttf" | "otf" | "ttc" => Kind::Font,
        "json" => Kind::FontFamily,
        _ => return None,
    })
}

/// Bytes a file of this kind starts with (pure). `.tex` is DDS, as Roblox ships it.
fn magic_ok(kind: Kind, head: &[u8]) -> bool {
    let starts = |m: &[u8]| head.starts_with(m);
    match kind {
        Kind::Image => starts(b"\x89PNG\r\n\x1a\n") || starts(b"\xff\xd8\xff") || starts(b"DDS ") || starts(b"\xabKTX"),
        Kind::Mesh => starts(b"version "),
        Kind::Sound => {
            starts(b"OggS") || starts(b"ID3") || (head.len() > 1 && head[0] == 0xff && head[1] & 0xe0 == 0xe0) || (starts(b"RIFF") && head.get(8..12) == Some(b"WAVE"))
        }
        Kind::Font => starts(b"\0\x01\0\0") || starts(b"OTTO") || starts(b"ttcf") || starts(b"true"),
        Kind::FontFamily => starts(b"{") || starts(b"\xef\xbb\xbf{"),
    }
}

/// A font family names only fonts: every `assetId` is an `rbxasset://fonts/` file (pure).
fn family_ok(bytes: &[u8]) -> bool {
    fn ids_ok(v: &serde_json::Value) -> bool {
        match v {
            serde_json::Value::Object(o) => o.iter().all(|(k, v)| match (k.as_str(), v) {
                ("assetId", serde_json::Value::String(s)) => s.starts_with("rbxasset://fonts/") && !s.contains(".."),
                ("assetId", _) => false,
                _ => ids_ok(v),
            }),
            serde_json::Value::Array(a) => a.iter().all(ids_ok),
            _ => true,
        }
    }
    let text = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    serde_json::from_slice::<serde_json::Value>(text).is_ok_and(|v| ids_ok(&v))
}

/// Writes one checked file from `src` to `dest` (a new file: never over an existing one, so never
/// through a hard link). Ok(false): its bytes don't match its type, nothing written. Err past the
/// size limits (`budget` is what the mod may still take).
fn take(src: &mut impl Read, rel: &str, dest: &Path, budget: &mut u64) -> Result<bool> {
    let kind = rel
        .rsplit_once('.')
        .and_then(|(_, e)| kind_of(&e.to_ascii_lowercase()))
        .ok_or_else(|| AppError::Internal(format!("{rel} wasn't checked")))?;
    let mut head = Vec::with_capacity(16);
    src.take(16).read_to_end(&mut head)?;
    if kind == Kind::FontFamily {
        let mut all = head;
        src.take(MAX_JSON_BYTES as u64).read_to_end(&mut all)?;
        if all.len() > MAX_JSON_BYTES || !family_ok(&all) {
            return Ok(false);
        }
        head = all;
    } else if !magic_ok(kind, &head) {
        return Ok(false);
    }
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut out = File::create_new(dest)?;
    out.write_all(&head)?;
    let limit = MAX_FILE_BYTES.min(*budget);
    let rest = io::copy(&mut src.take(limit.saturating_sub(head.len() as u64) + 1), &mut out)?;
    // Head and rest together: the head alone can be past what the mod may still take.
    let size = head.len() as u64 + rest;
    if size > limit {
        return Err(too_big());
    }
    *budget -= size;
    Ok(true)
}

fn too_big() -> AppError {
    AppError::Invalid(tr!("That modpack is too big: at most 1 GB, 20,000 files and 256 MB per file."))
}

/// The one folder everything in a pack sits in, when there is one and it isn't a mod folder.
fn wrapper(names: &[String]) -> Option<String> {
    let first = names.iter().find(|n| n.contains(['/', '\\']))?.split(['/', '\\']).next()?.to_owned();
    let all = names.iter().all(|n| n.split(['/', '\\']).next() == Some(&first) && n.len() > first.len());
    (all && !TOPS.iter().any(|t| t.eq_ignore_ascii_case(&first))).then_some(first)
}

fn strip<'a>(name: &'a str, wrap: Option<&str>) -> &'a str {
    match wrap {
        Some(w) => name.get(w.len() + 1..).unwrap_or(""),
        None => name,
    }
}

/// Unpacks the checked files of the zip at `zip` into the empty folder `dest`, with its
/// `mod.json` (read only for its four texts). Errors leave `dest` for the caller to delete.
pub fn unzip(zip: &Path, dest: &Path) -> Result<Unzipped> {
    let file = File::open(zip)?;
    if file.metadata()?.len() > MAX_MOD_BYTES {
        return Err(too_big());
    }
    // shortcut: the zip crate reads the whole central directory here, before the entry count can be
    // checked: a crafted 1 GB zip of millions of empty entries takes GBs of memory. Read the end
    // record's entry count first if such packs ever turn up.
    let mut archive = zip::ZipArchive::new(file).map_err(|_| AppError::Invalid(tr!("That isn't a zip file RoALT can read.")))?;
    if archive.len() > MAX_FILES {
        return Err(too_big());
    }
    let names: Vec<String> = (0..archive.len()).filter_map(|i| archive.name_for_index(i)?.ok().map(|n| n.into_owned())).collect();
    let wrap = wrapper(&names);
    let mut out = Unzipped::default();
    let mut seen = HashSet::new();
    let mut budget = MAX_MOD_BYTES;
    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            // Encrypted entries: skipped, with no password nothing of them is read.
            Err(zip::result::ZipError::UnsupportedArchive(zip::result::ZipError::PASSWORD_REQUIRED)) => {
                out.skipped += 1;
                continue;
            }
            Err(zip::result::ZipError::UnsupportedArchive(_)) => {
                return Err(AppError::Invalid(tr!("That zip is packed in a way RoALT can't read. Re-zip it as a normal zip.")));
            }
            Err(e) => return Err(AppError::Invalid(tr!("That zip is damaged: {why}", why = e))),
        };
        if entry.is_dir() {
            continue;
        }
        let full = entry.name().map_err(|e| AppError::Invalid(tr!("That zip is damaged: {why}", why = e)))?.into_owned();
        let name = strip(&full, wrap.as_deref()).to_owned();
        if name.eq_ignore_ascii_case(META_FILE) {
            let mut text = Vec::new();
            entry.by_ref().take(16 << 10).read_to_end(&mut text)?;
            out.meta = serde_json::from_slice::<Meta>(&text).unwrap_or_default().clean();
            continue;
        }
        let Ok(rel) = checked_path(&name).map_err(|why| log::debug!("mod import: {name:?} skipped: {why:?}")) else {
            out.skipped += 1;
            continue;
        };
        // A symlink entry needs no check of its own: `take` writes every entry as a plain new file.
        if !seen.insert(rel.to_ascii_lowercase()) {
            out.skipped += 1;
            continue;
        }
        if take(&mut entry, &rel, &dest.join(&rel), &mut budget)? {
            out.files += 1;
        } else {
            log::debug!("mod import: {rel:?} skipped: its bytes don't match its type");
            out.skipped += 1;
        }
    }
    out.bytes = MAX_MOD_BYTES - budget;
    Ok(out)
}

/// Every file under `dir` (relative, `/`-separated), refusing links and other reparse points
/// so nothing outside it is ever read.
pub fn files_in(dir: &Path, max: usize) -> Result<Vec<String>> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>, max: usize) -> Result<()> {
        for e in std::fs::read_dir(dir)? {
            let e = e?;
            let meta = std::fs::symlink_metadata(e.path())?;
            if is_reparse(&meta) {
                return Err(AppError::Invalid(tr!("A modpack folder holds a link, which modpacks can't.")));
            }
            if meta.is_dir() {
                walk(base, &e.path(), out, max)?;
            } else if meta.is_file() {
                let rel = e.path().strip_prefix(base).map_err(|e| AppError::Internal(e.to_string()))?.to_string_lossy().replace('\\', "/");
                out.push(rel);
            }
            if out.len() > max {
                return Err(too_big());
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out, max)?;
    Ok(out)
}

#[cfg(windows)]
fn is_reparse(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    meta.file_type().is_symlink() || meta.file_attributes() & 0x400 != 0
}

/// Unix has no reparse points; a symlink is the one indirection a mod must not carry.
#[cfg(not(windows))]
fn is_reparse(meta: &std::fs::Metadata) -> bool {
    meta.file_type().is_symlink()
}

/// A file of an imported mod is still one a mod may hold, by name and by its bytes.
pub fn still_ok(dir: &Path, rel: &str) -> bool {
    if checked_path(rel).as_deref() != Ok(rel) {
        return false;
    }
    let Some(kind) = rel.rsplit_once('.').and_then(|(_, e)| kind_of(&e.to_ascii_lowercase())) else { return false };
    let Ok(mut f) = File::open(dir.join(rel)) else { return false };
    let mut head = Vec::with_capacity(MAX_JSON_BYTES + 1);
    let cap = if kind == Kind::FontFamily { MAX_JSON_BYTES as u64 + 1 } else { 16 };
    if Read::by_ref(&mut f).take(cap).read_to_end(&mut head).is_err() {
        return false;
    }
    if kind == Kind::FontFamily { head.len() <= MAX_JSON_BYTES && family_ok(&head) } else { magic_ok(kind, &head) }
}

/// Writes the mod in `dir` as a zip at `dest`: its files and a `mod.json` with only `meta`.
/// Through a temporary file, so a failure never leaves half a zip at `dest`.
pub fn zip_to(dir: &Path, meta: &Meta, dest: &Path) -> Result<()> {
    let tmp = dest.with_extension("zip.roalt");
    let done = (|| -> Result<()> {
        let mut w = zip::ZipWriter::new(File::create(&tmp)?);
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let bad = |e: zip::result::ZipError| AppError::Internal(e.to_string());
        w.start_file(META_FILE, opts).map_err(bad)?;
        w.write_all(&serde_json::to_vec_pretty(meta).map_err(|e| AppError::Internal(e.to_string()))?)?;
        for rel in files_in(dir, MAX_FILES + 1)?.into_iter().filter(|r| !r.eq_ignore_ascii_case(META_FILE)) {
            w.start_file(rel.as_str(), opts).map_err(bad)?;
            io::copy(&mut File::open(dir.join(&rel))?, &mut w)?;
        }
        w.finish().map_err(bad)?;
        crate::launcher::client_settings::retry(|| std::fs::rename(&tmp, dest))?;
        Ok(())
    })();
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    done
}

/// A file name for an export: the mod's name with anything Windows refuses taken out.
pub fn file_name(name: &str) -> String {
    let clean: String = name.chars().map(|c| if c.is_alphanumeric() || " _-().".contains(c) { c } else { '_' }).collect();
    let clean = clean.trim().trim_end_matches('.').to_owned();
    if clean.is_empty() { "mod.zip".into() } else { format!("{clean}.zip") }
}

/// Where a zip's `rel` file goes under `dir` (for tests and the build).
pub fn under(dir: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(dir.to_path_buf(), |p, part| p.join(part))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n0000";
    const OGG: &[u8] = b"OggS\0\x02000000";

    #[test]
    fn paths() {
        assert_eq!(checked_path("content/sky/sky512_bk.tex"), Ok("content/sky/sky512_bk.tex".into()));
        assert_eq!(checked_path("Content\\textures\\Cursors\\KeyboardMouse\\ArrowCursor.png"), Ok("content/textures/Cursors/KeyboardMouse/ArrowCursor.png".into()));
        assert_eq!(checked_path("platformcontent/pc/textures/x.dds"), Ok("PlatformContent/pc/textures/x.dds".into()));
        assert_eq!(checked_path("content/fonts/families/Arial.json"), Ok("content/fonts/families/Arial.json".into()));
        for (bad, why) in [
            ("RobloxPlayerBeta.exe", Refused::Outside),
            ("ssl/cacert.pem", Refused::Outside),
            ("shaders/x.pack", Refused::Outside),
            ("ClientSettings/ClientAppSettings.json", Refused::Outside),
            ("content", Refused::Outside),
            ("content/../ssl/x.png", Refused::Name),
            ("content\\..\\..\\x.png", Refused::Name),
            ("content/./x.png", Refused::Name),
            ("content//x.png", Refused::Name),
            ("content/configs/UNIVER~1/x.png", Refused::Name),
            ("content/sky/nul.png", Refused::Name),
            ("content/sky/COM1.ogg", Refused::Name),
            ("content/sky/x.png.", Refused::Name),
            ("content/sky/x.png ", Refused::Name),
            ("content/sky/nul .png", Refused::Name),
            ("content/sky/LPT1  .ogg", Refused::Name),
            ("content/sky/x.png:evil", Refused::Name),
            ("content/sky/ä.png", Refused::Name),
            ("C:/content/x.png", Refused::Outside),
            ("/content/x.png", Refused::Outside),
            ("content/x.lua", Refused::Type),
            ("content/x.js", Refused::Type),
            ("content/x.rbxm", Refused::Type),
            ("ExtraContent/x.rbxmx", Refused::Type),
            ("content/x.rbxl", Refused::Type),
            ("ExtraContent/translations/CoreScriptLocalization.csv", Refused::Type),
            ("content/configs/UniversalAppPatchConfig/UniversalAppPatchConfig.json", Refused::Type),
            ("PlatformContent/pc/terrain/materials.json", Refused::Type),
            ("content/noext", Refused::Type),
        ] {
            assert_eq!(checked_path(bad), Err(why), "{bad}");
        }
        assert_eq!(checked_path(&format!("content/{}.png", "a".repeat(140))), Err(Refused::Name));
    }

    #[test]
    fn magic() {
        assert!(magic_ok(Kind::Image, PNG));
        assert!(magic_ok(Kind::Image, b"DDS |\0\0\0"));
        assert!(!magic_ok(Kind::Image, b"<roblox!\x89\xff\x0d\x0a"));
        assert!(magic_ok(Kind::Sound, OGG) && magic_ok(Kind::Sound, b"ID3\x03") && magic_ok(Kind::Sound, b"RIFF\0\0\0\0WAVEfmt "));
        assert!(!magic_ok(Kind::Sound, b"RIFF\0\0\0\0AVI "));
        assert!(magic_ok(Kind::Mesh, b"version 2.00\n"));
        assert!(magic_ok(Kind::Font, b"OTTO\0\x10"));
        assert!(family_ok(br#"{"name":"A","faces":[{"name":"Regular","assetId":"rbxasset://fonts/A.ttf"}]}"#));
        assert!(!family_ok(br#"{"faces":[{"assetId":"rbxasset://models/UniversalApp/UniversalApp.rbxm"}]}"#));
        assert!(!family_ok(br#"{"faces":[{"assetId":"rbxasset://fonts/../models/x.rbxm"}]}"#));
        assert!(!family_ok(br#"{"faces":[{"assetId":5}]}"#));
    }

    #[test]
    fn wrapper_folder() {
        let n = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(wrapper(&n(&["Night Sky/", "Night Sky/content/sky/a.tex", "Night Sky/mod.json"])), Some("Night Sky".into()));
        assert_eq!(wrapper(&n(&["content/sky/a.tex", "content/sky/b.tex"])), None);
        assert_eq!(wrapper(&n(&["a/content/x.png", "b/content/y.png"])), None);
        assert_eq!(wrapper(&n(&["mod.json", "content/x.png"])), None);
    }

    fn zip_with(path: &Path, entries: &[(&str, &[u8])]) {
        let mut w = zip::ZipWriter::new(File::create(path).unwrap());
        let opts = zip::write::SimpleFileOptions::default();
        for (name, bytes) in entries {
            w.start_file(*name, opts).unwrap();
            w.write_all(bytes).unwrap();
        }
        w.finish().unwrap();
    }

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("roalt-pack-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn unzip_keeps_only_content() {
        let d = temp("unzip");
        let z = d.join("pack.zip");
        zip_with(
            &z,
            &[
                ("Pack/mod.json", br#"{"name":"Night\u0007 Sky","author":"me","extra":1}"#),
                ("Pack/content/sky/a.png", PNG),
                ("Pack/Content/Sky/A.PNG", PNG),
                ("Pack/content/sounds/oof.ogg", OGG),
                ("Pack/content/sky/fake.png", b"<roblox!\x89\xff\x0d\x0a\x1a\x0a\0\0"),
                ("Pack/RobloxPlayerBeta.exe", b"MZ"),
                ("Pack/ssl/cacert.pem", b"-----"),
                ("Pack/content/x.lua", b"print(1)"),
                ("Pack/content/configs/x.json", b"{}"),
            ],
        );
        let out = d.join("out");
        std::fs::create_dir(&out).unwrap();
        let got = unzip(&z, &out).unwrap();
        assert_eq!((got.files, got.skipped), (2, 6));
        assert_eq!(got.meta, Meta { name: "Night Sky".into(), author: "me".into(), ..Meta::default() });
        let mut files = files_in(&out, MAX_FILES).unwrap();
        files.sort();
        assert_eq!(files, ["content/sky/a.png", "content/sounds/oof.ogg"]);
        assert!(files.iter().all(|f| still_ok(&out, f)));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn zip_round_trip() {
        let d = temp("round");
        let src = d.join("src");
        for (rel, bytes) in [("content/sky/a.png", PNG), ("content/sounds/oof.ogg", OGG)] {
            let p = under(&src, rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, bytes).unwrap();
        }
        let meta = Meta { name: "Sky".into(), ..Meta::default() };
        let z = d.join("Sky.zip");
        zip_to(&src, &meta, &z).unwrap();
        let back = d.join("back");
        std::fs::create_dir(&back).unwrap();
        let got = unzip(&z, &back).unwrap();
        assert_eq!((got.files, got.skipped, got.meta.name.as_str()), (2, 0, "Sky"));
        assert_eq!(std::fs::read(under(&back, "content/sky/a.png")).unwrap(), PNG);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn unzip_refuses_bombs_and_links() {
        let d = temp("bomb");
        // The bytes actually written count, whatever a zip claims: past what the mod may still take, refused.
        let mut budget = 100;
        let big = [PNG, &[0u8; 200][..]].concat();
        assert!(matches!(take(&mut &big[..], "content/sky/big.png", &d.join("big.png"), &mut budget), Err(AppError::Invalid(_))));
        let mut budget = 100;
        assert!(take(&mut &PNG[..], "content/sky/a.png", &d.join("a.png"), &mut budget).unwrap());
        assert_eq!(budget, 100 - PNG.len() as u64);
        // Never over an existing file (it could be a hard link to Roblox's).
        assert!(take(&mut &PNG[..], "content/sky/a.png", &d.join("a.png"), &mut budget).is_err());
        // Less left than one file's head: refused, and the budget never wraps below zero.
        let mut budget = 8;
        assert!(matches!(take(&mut &PNG[..], "content/sky/b.png", &d.join("b.png"), &mut budget), Err(AppError::Invalid(_))));
        assert_eq!(budget, 8);
        // A symlink entry becomes no link: its target text isn't a picture, so it is left out.
        let z = d.join("link.zip");
        {
            let mut w = zip::ZipWriter::new(File::create(&z).unwrap());
            w.add_symlink("content/sky/a.png", "../../../../Windows/win.ini", zip::write::SimpleFileOptions::default()).unwrap();
            w.finish().unwrap();
        }
        let out = d.join("out2");
        std::fs::create_dir(&out).unwrap();
        assert_eq!(unzip(&z, &out).unwrap().skipped, 1);
        assert!(files_in(&out, MAX_FILES).unwrap().is_empty());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn export_names() {
        assert_eq!(file_name("Night Sky v2"), "Night Sky v2.zip");
        assert_eq!(file_name("a/b:c*"), "a_b_c_.zip");
        assert_eq!(file_name("..."), "mod.zip");
    }
}
