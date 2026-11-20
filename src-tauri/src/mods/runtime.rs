//! RoALT's own copy of the Roblox version folder with the mods laid over, which a launch starts
//! the client from: `<mods>\runtime\<uuid>\Versions\version-…\` (the `Versions\version-…` ending
//! is what `update::installed_version` reads). Roblox's folder is never written:
//! - the files no mod changes are hard links (a plain copy where linking fails), and every file
//!   a mod changes is written as a new file, never over a link;
//! - `ClientSettings` is never linked: each launch gets the user's own flag file copied in fresh,
//!   and `flags::apply` works on that copy.
//!
//! A copy is reused while its marker (source folder, version, the mods on, in order) matches.
//! Roblox reads content files during play, so a copy a running client started from is never
//! deleted; the others are, after each launch's prepare. Prepare and the client's start both run
//! under the launch lock, so the copy picked can't go before its client starts.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use ts_rs::TS;

use super::pack::{self, META_FILE};
use crate::App;
use crate::error::{AppError, Result};
use crate::launcher::client::ClientExe;

const MARKER: &str = "roalt-mods.json";

#[cfg(windows)]
fn client_exe(path: PathBuf) -> ClientExe {
    ClientExe { path, bootstrapper: false }
}

#[cfg(not(windows))]
fn client_exe(path: PathBuf) -> ClientExe {
    ClientExe {
        path,
        bootstrapper: false,
        kind: crate::launcher::client::LauncherKind::Handler,
        profiles: false,
        direct: None,
    }
}
const PLAYER: &str = "RobloxPlayerBeta.exe";
/// More than Roblox will ever ship in one folder, fewer than a runaway walk.
const MAX_SOURCE_FILES: usize = 100_000;

/// The last launch's mods, for the Mods view: applied (`note` is the version) or not (why).
#[derive(Serialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ModsLast {
    pub applied: bool,
    pub note: String,
}

static LAST: Mutex<Option<ModsLast>> = Mutex::new(None);

pub fn last() -> Option<ModsLast> {
    LAST.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn set_last(l: ModsLast) {
    *LAST.lock().unwrap_or_else(|e| e.into_inner()) = Some(l);
}

/// The client a launch starts: inside a copy with the mods on, else `exe` as is. The second
/// value says why mods that are on weren't applied. `self_updates`: the client asks for a build
/// that isn't installed and will update itself into Roblox's folder, unmodded.
pub fn prepare(s: &App, exe: ClientExe, self_updates: bool) -> (ClientExe, Option<String>) {
    let on = s.db.settings().map(|v| v.mods_on).unwrap_or(false);
    let mods = if on { super::library::enabled(&s.db).unwrap_or_default() } else { Vec::new() };
    let runtime = super::root().join("runtime");
    if mods.is_empty() {
        // Modpacks off: the last copy (a whole Roblox build, once Roblox has moved on) goes too.
        if runtime.is_dir() {
            prune(&runtime, Path::new(""), &running());
        }
        return (exe, None);
    }
    let skip = |why: String| {
        log::info!("mods not applied: {why}");
        set_last(ModsLast { applied: false, note: why.clone() });
        Some(why)
    };
    if exe.bootstrapper {
        return (exe, skip(tr!("Roblox runs through a bootstrapper, which has its own mods.")));
    }
    if self_updates {
        return (exe, skip(tr!("Roblox is updating itself this launch. Modpacks apply from the next one.")));
    }
    let Some(version) = crate::launcher::update::installed_version(&exe.path) else {
        return (exe, skip(tr!("RoALT can't tell which Roblox version this is.")));
    };
    match copy_for(&runtime, &exe.path, &version, &mods, &running()) {
        Ok((path, stats)) => {
            if stats.matched == 0 && stats.unmatched > 0 {
                let why = tr!("{version} — no mod files matched this Roblox version", version = version);
                set_last(ModsLast { applied: false, note: why });
            } else if stats.unmatched > 0 {
                let note = trn!(
                    stats.unmatched,
                    "{version} ({n} file didn't match this Roblox version)",
                    "{version} ({n} files didn't match this Roblox version)",
                    version = version
                );
                set_last(ModsLast { applied: true, note });
            } else {
                set_last(ModsLast { applied: true, note: version });
            }
            (client_exe(path), None)
        }
        Err(e) => {
            log::warn!("mods: couldn't prepare the copy: {e}");
            (exe, skip(tr!("RoALT couldn't prepare them ({why}).", why = e)))
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct MatchStats {
    matched: usize,
    unmatched: usize,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Marker {
    source: String,
    version: String,
    mods: Vec<String>,
    #[serde(default)]
    stats: MatchStats,
}

/// The exe in a copy under `runtime` of `source_exe`'s folder with `mods` (top first) laid over:
/// one that matches, else a new one. Then the user's own flag file goes in, and every other copy
/// no `running` client started from is deleted.
fn copy_for(runtime: &Path, source_exe: &Path, version: &str, mods: &[(String, PathBuf)], running: &[PathBuf]) -> Result<(PathBuf, MatchStats)> {
    let source = source_exe.parent().ok_or_else(|| AppError::Internal("no folder".into()))?;
    let source_str = source.display().to_string();
    let mod_ids: Vec<String> = mods.iter().map(|(id, _)| id.clone()).collect();
    std::fs::create_dir_all(runtime)?;
    let reuse = std::fs::read_dir(runtime)?.flatten().map(|e| e.path()).find_map(|d| {
        if d.to_string_lossy().ends_with(".partial") || !d.join("Versions").join(version).join(PLAYER).is_file() {
            return None;
        }
        let m: Marker = serde_json::from_str(&std::fs::read_to_string(d.join(MARKER)).ok()?).ok()?;
        (m.source == source_str && m.version == version && m.mods == mod_ids).then_some((d, m.stats))
    });
    let (copy, stats) = match reuse {
        Some((d, stats)) => {
            log::info!("mods: reusing the copy {}", d.display());
            (d, stats)
        }
        None => build(runtime, source, version, mods, &source_str, &mod_ids)?,
    };
    let vdir = copy.join("Versions").join(version);
    fresh_client_settings(source, &vdir)?;
    prune(runtime, &copy, running);
    Ok((vdir.join(PLAYER), stats))
}

fn build(runtime: &Path, source: &Path, version: &str, mods: &[(String, PathBuf)], source_str: &str, mod_ids: &[String]) -> Result<(PathBuf, MatchStats)> {
    let id = uuid::Uuid::new_v4().to_string();
    let partial = runtime.join(format!("{id}.partial"));
    let done = (|| {
        let (linked, copied, modded, stats) = lay(source, mods, &partial.join("Versions").join(version))?;
        let marker = serde_json::to_string(&Marker { source: source_str.to_owned(), version: version.to_owned(), mods: mod_ids.to_vec(), stats })
            .map_err(|e| AppError::Internal(e.to_string()))?;
        std::fs::write(partial.join(MARKER), marker)?;
        let copy = runtime.join(&id);
        crate::launcher::client_settings::retry(|| std::fs::rename(&partial, &copy))?;
        log::info!(
            "mods: built {} ({linked} linked, {copied} copied, {modded} from mods: {} matched, {} unmatched)",
            copy.display(),
            stats.matched,
            stats.unmatched
        );
        Ok((copy, stats))
    })();
    if done.is_err() {
        let _ = std::fs::remove_dir_all(&partial);
    }
    done
}

#[derive(Clone, Copy)]
enum From {
    Roblox,
    Mod(usize),
}

/// Fills the empty `dest` with `source`'s files and the mods' (bottom mod first, so the top one
/// wins). Every path is decided before anything is written, and each file is written once to a
/// new path: a mod's file is never written over a link to Roblox's. (linked, copied, modded, stats).
fn lay(source: &Path, mods: &[(String, PathBuf)], dest: &Path) -> Result<(usize, usize, usize, MatchStats)> {
    let mut map: std::collections::BTreeMap<String, (String, From)> = std::collections::BTreeMap::new();
    let mut roblox_keys = std::collections::HashSet::new();
    for rel in pack::files_in(source, MAX_SOURCE_FILES)? {
        if !rel.to_ascii_lowercase().starts_with("clientsettings/") {
            let key = rel.to_ascii_lowercase();
            roblox_keys.insert(key.clone());
            map.insert(key, (rel, From::Roblox));
        }
    }
    let mut matched_keys = std::collections::HashSet::new();
    let mut unmatched_keys = std::collections::HashSet::new();
    for (i, (id, dir)) in mods.iter().enumerate().rev() {
        for rel in pack::files_in(dir, pack::MAX_FILES + 1)? {
            if rel.eq_ignore_ascii_case(META_FILE) {
                continue;
            }
            if !pack::still_ok(dir, &rel) {
                log::warn!("mods: {id}'s {rel:?} isn't a file mods may change; left out");
                continue;
            }
            let key = rel.to_ascii_lowercase();
            if roblox_keys.contains(&key) {
                matched_keys.insert(key.clone());
            } else {
                unmatched_keys.insert(key.clone());
            }
            let path = map.get(&key).map_or(rel, |(roblox, _)| roblox.clone());
            map.insert(key, (path, From::Mod(i)));
        }
    }
    let stats = MatchStats { matched: matched_keys.len(), unmatched: unmatched_keys.len() };
    let (mut linked, mut copied, mut modded) = (0, 0, 0);
    let mut why_copied = None;
    for (rel, from) in map.values() {
        let to = pack::under(dest, rel);
        if let Some(dir) = to.parent() {
            std::fs::create_dir_all(dir)?;
        }
        match from {
            From::Roblox => {
                let src = pack::under(source, rel);
                match std::fs::hard_link(&src, &to) {
                    Ok(()) => linked += 1,
                    Err(e) => {
                        why_copied.get_or_insert(e);
                        copy_new(&src, &to)?;
                        copied += 1;
                    }
                }
            }
            From::Mod(i) => {
                copy_new(&pack::under(&mods[*i].1, rel), &to)?;
                modded += 1;
            }
        }
    }
    if let Some(e) = why_copied {
        log::info!("mods: {copied} of Roblox's files copied instead of linked ({e})");
    }
    std::fs::create_dir_all(dest.join("ClientSettings"))?;
    Ok((linked, copied, modded, stats))
}

/// Copies to a path nothing is at (never through a link).
fn copy_new(from: &Path, to: &Path) -> Result<()> {
    let mut out = std::fs::File::create_new(to)?;
    std::io::copy(&mut std::fs::File::open(from)?, &mut out)?;
    Ok(())
}

/// The copy's `ClientSettings` as the user's own: emptied, then their flag file copied in (a
/// copy, never a link: `flags` writes and toggles read-only there).
fn fresh_client_settings(source: &Path, vdir: &Path) -> Result<()> {
    let dir = vdir.join("ClientSettings");
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    std::fs::create_dir_all(&dir)?;
    let own = source.join("ClientSettings").join("ClientAppSettings.json");
    if own.is_file() {
        copy_new(&own, &dir.join("ClientAppSettings.json"))?;
    }
    Ok(())
}

/// The exes running Roblox clients started from.
fn running() -> Vec<PathBuf> {
    crate::instances::process::roblox_processes().iter().filter_map(|p| crate::instances::process::exe_path(p.pid)).collect()
}

/// A path with links and junctions resolved, so two spellings of one folder compare equal.
fn real(p: PathBuf) -> PathBuf {
    std::fs::canonicalize(&p).unwrap_or(p)
}

/// Deletes every copy but `keep` that no `running` client started from (its marker first, so a
/// half-deleted copy is never reused). Failures are left for the next time. Both sides are
/// resolved: Windows reports a client's real path, the copies' come from `%LOCALAPPDATA%`'s text.
fn prune(runtime: &Path, keep: &Path, running: &[PathBuf]) {
    let running: Vec<PathBuf> = running.iter().cloned().map(real).collect();
    for d in std::fs::read_dir(runtime).into_iter().flatten().flatten().map(|e| e.path()) {
        let resolved = real(d.clone());
        if d == keep || running.iter().any(|exe| super::inside(exe, &resolved)) {
            continue;
        }
        let _ = std::fs::remove_file(d.join(MARKER));
        if let Err(e) = std::fs::remove_dir_all(&d) {
            log::debug!("mods: couldn't delete the old copy {} yet: {e}", d.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nmod!";
    const V: &str = "version-0123456789abcdef";

    fn write(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    struct Fixture {
        base: PathBuf,
        exe: PathBuf,
        runtime: PathBuf,
        mods: Vec<(String, PathBuf)>,
    }

    fn fixture() -> Fixture {
        let base = std::env::temp_dir().join(format!("roalt-runtime-{}", uuid::Uuid::new_v4()));
        let src = base.join("Roblox").join("Versions").join(V);
        write(&src.join(PLAYER), b"MZ roblox");
        write(&src.join("content/sky/sky512_bk.tex"), b"DDS roblox sky");
        write(&src.join("content/sounds/oof.ogg"), b"OggS roblox oof");
        write(&src.join("ClientSettings/ClientAppSettings.json"), br#"{"DFIntUser":"1"}"#);
        let top = base.join("lib").join("top");
        write(&top.join("content/sky/sky512_bk.tex"), b"DDS top sky");
        write(&top.join(META_FILE), b"{}");
        let low = base.join("lib").join("low");
        write(&low.join("content/sky/sky512_bk.tex"), b"DDS low sky");
        write(&low.join("content/textures/new.png"), PNG);
        write(&low.join("content/x.lua"), b"print('no')");
        // Edited by hand after import: a model named as a picture.
        write(&low.join("content/sky/fake.png"), b"<roblox!\x89\xff\r\n\x1a\n");
        write(&low.join(META_FILE), b"{}");
        Fixture { exe: src.join(PLAYER), runtime: base.join("runtime"), mods: vec![("top".into(), top), ("low".into(), low)], base }
    }

    #[test]
    fn builds_reuses_and_prunes_without_touching_roblox() {
        let f = fixture();
        let sky = f.exe.with_file_name("content").join("sky").join("sky512_bk.tex");
        let mut ro = std::fs::metadata(&sky).unwrap().permissions();
        ro.set_readonly(true);
        std::fs::set_permissions(&sky, ro).unwrap();

        let (exe, stats) = copy_for(&f.runtime, &f.exe, V, &f.mods, &[]).unwrap();
        assert_eq!(stats, MatchStats { matched: 1, unmatched: 1 }, "sky matched Roblox, textures/new.png did not");
        let vdir = exe.parent().unwrap();
        #[cfg(windows)]
        assert_eq!(crate::launcher::update::installed_version(&exe).as_deref(), Some(V));
        assert_eq!(std::fs::read(vdir.join("content/sky/sky512_bk.tex")).unwrap(), b"DDS top sky", "top mod wins");
        assert_eq!(std::fs::read(vdir.join("content/textures/new.png")).unwrap(), PNG);
        assert_eq!(std::fs::read(vdir.join("content/sounds/oof.ogg")).unwrap(), b"OggS roblox oof");
        assert!(!vdir.join("content/x.lua").exists(), "a script is never laid over");
        assert!(!vdir.join("content/sky/fake.png").exists(), "nor a file whose bytes aren't its type");
        // Roblox's own files: same bytes, same read-only flag.
        assert_eq!(std::fs::read(&sky).unwrap(), b"DDS roblox sky");
        assert!(std::fs::metadata(&sky).unwrap().permissions().readonly());
        // ClientSettings is the user's file, copied: writing it never reaches Roblox's.
        std::fs::write(vdir.join("ClientSettings/ClientAppSettings.json"), b"{}").unwrap();
        assert_eq!(std::fs::read(f.exe.with_file_name("ClientSettings").join("ClientAppSettings.json")).unwrap(), br#"{"DFIntUser":"1"}"#);

        // Same mods: the same copy, its flag file fresh again, and match stats preserved.
        assert_eq!(copy_for(&f.runtime, &f.exe, V, &f.mods, &[]).unwrap(), (exe.clone(), stats));
        assert_eq!(std::fs::read(vdir.join("ClientSettings/ClientAppSettings.json")).unwrap(), br#"{"DFIntUser":"1"}"#);

        // Other mods while a client runs from the first copy: a new copy, the first one kept.
        let one = f.mods[1..].to_vec();
        let (second, _) = copy_for(&f.runtime, &f.exe, V, &one, std::slice::from_ref(&exe)).unwrap();
        assert_ne!(second, exe);
        assert!(exe.is_file(), "a copy a running client started from is kept");
        assert_eq!(std::fs::read(second.with_file_name("content").join("sky").join("sky512_bk.tex")).unwrap(), b"DDS low sky");
        // That client gone: the next prepare deletes the first copy.
        copy_for(&f.runtime, &f.exe, V, &one, &[]).unwrap();
        assert!(!exe.exists());

        let mut ro = std::fs::metadata(&sky).unwrap().permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        ro.set_readonly(false);
        std::fs::set_permissions(&sky, ro).unwrap();
        std::fs::remove_dir_all(&f.base).unwrap();
    }

    #[test]
    fn half_made_copies_are_never_used() {
        let f = fixture();
        // A crash mid-build left a .partial with the right marker; a prune cut short left a copy without one.
        let partial = f.runtime.join("dead.partial");
        let marker = serde_json::to_string(&Marker {
            source: f.exe.parent().unwrap().display().to_string(),
            version: V.into(),
            mods: f.mods.iter().map(|(id, _)| id.clone()).collect(),
            stats: MatchStats::default(),
        })
        .unwrap();
        write(&partial.join(MARKER), marker.as_bytes());
        write(&partial.join("Versions").join(V).join(PLAYER), b"MZ half");
        write(&f.runtime.join("cut").join("Versions").join(V).join(PLAYER), b"MZ cut");
        let (exe, _) = copy_for(&f.runtime, &f.exe, V, &f.mods, &[]).unwrap();
        assert!(!super::super::inside(&exe, &partial) && !super::super::inside(&exe, &f.runtime.join("cut")));
        assert!(!partial.exists() && !f.runtime.join("cut").exists(), "left-overs are swept");
        std::fs::remove_dir_all(&f.base).unwrap();
    }
}
