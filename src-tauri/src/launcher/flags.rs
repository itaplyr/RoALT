//! Lighter rendering for the clients RoALT starts (`Settings.lighter_rendering`), through the only
//! local flags Roblox still honours: its allowlist (DevForum "Allowlist for local client
//! configuration via Fast Flags"; any other is logged as "Denied local configuration for: ...")
//! in `<version folder>\ClientSettings\ClientAppSettings.json`. Lowest geometry detail always, and
//! what `Settings.render_flags` picks: less GPU memory and time per client.
//!
//! Every client shares the file and reads it once, as it starts; Roblox never writes it. A launch
//! merges these flags into the user's own flags and puts the file back exactly as it was once its
//! client has read its settings (inside `launch`'s `client_settings::LOCK` window, so another
//! launch never sees them by mistake). The original waits in a file beside it (`SAVED`), so a quit
//! or crash meanwhile loses nothing: `restore` at start and exit, or the next `apply`, puts it back.
//! Not under a bootstrapper: Bloxstrap and the like copy their own file over this one at every
//! launch.
#![cfg_attr(not(windows), allow(dead_code))]


use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::client_settings::{retry, set_readonly};
use crate::model::{Msaa, RenderFlags, RenderMode, TextureQuality};

/// Only one of these can be on.
const MODES: [&str; 3] = ["FFlagDebugGraphicsPreferD3D11", "FFlagDebugGraphicsPreferVulkan", "FFlagDebugGraphicsPreferOpenGL"];

/// The allowlisted flags for `r`, as strings like Bloxstrap writes them (pure). "Roblox default" is
/// no flag at all, so a user's own value for it stays.
fn flags_of(r: &RenderFlags) -> Vec<(&'static str, &'static str)> {
    let mut f = vec![
        ("DFIntCSGLevelOfDetailSwitchingDistance", "0"),
        ("DFIntCSGLevelOfDetailSwitchingDistanceL12", "0"),
        ("DFIntCSGLevelOfDetailSwitchingDistanceL23", "0"),
        ("DFIntCSGLevelOfDetailSwitchingDistanceL34", "0"),
    ];
    match r.msaa {
        Msaa::Default => {}
        Msaa::X1 => f.push(("FIntDebugForceMSAASamples", "1")),
        Msaa::X2 => f.push(("FIntDebugForceMSAASamples", "2")),
        Msaa::X4 => f.push(("FIntDebugForceMSAASamples", "4")),
    }
    let level = match r.texture {
        TextureQuality::Default => None,
        TextureQuality::Lowest => Some("0"),
        TextureQuality::Low => Some("1"),
        TextureQuality::Medium => Some("2"),
        TextureQuality::High => Some("3"),
    };
    if let Some(l) = level {
        f.extend([("DFFlagTextureQualityOverrideEnabled", "True"), ("DFIntTextureQualityOverride", l)]);
    }
    match r.mode {
        RenderMode::Default => {}
        RenderMode::D3d11 => f.push((MODES[0], "True")),
        RenderMode::Vulkan => f.push((MODES[1], "True")),
        RenderMode::Opengl => f.push((MODES[2], "True")),
    }
    if r.gray_sky {
        f.push(("FFlagDebugSkyGray", "True"));
    }
    if r.no_grass {
        f.extend([("FIntFRMMinGrassDistance", "0"), ("FIntFRMMaxGrassDistance", "0")]);
    }
    if r.lowest_frm {
        f.push(("DFIntDebugFRMQualityLevelOverride", "1"));
    }
    if r.pause_voxelizer {
        f.push(("DFFlagDebugPauseVoxelizer", "True"));
    }
    f
}

/// The file's text with `set` set and the user's other flags kept (pure); None when it isn't a JSON
/// object (it is left alone then). A chosen rendering mode replaces one of the user's own.
fn merge(text: Option<&str>, set: &[(&str, &str)]) -> Option<String> {
    let mut flags: Map<String, Value> = match text.map(str::trim) {
        None | Some("") => Map::new(),
        Some(t) => serde_json::from_str(t).ok()?,
    };
    if set.iter().any(|(k, _)| MODES.contains(k)) {
        flags.retain(|k, _| !MODES.contains(&k.as_str()));
    }
    for (k, v) in set {
        flags.insert((*k).into(), (*v).into());
    }
    serde_json::to_string_pretty(&flags).ok()
}

/// Replaces the file in one step; a read-only one stays read-only.
fn write(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = file.with_extension("json.roalt");
    let readonly = std::fs::metadata(file).is_ok_and(|m| m.permissions().readonly());
    let done = retry(|| std::fs::write(&tmp, bytes)).and_then(|()| {
        if readonly {
            set_readonly(file, false)?;
        }
        retry(|| std::fs::rename(&tmp, file))
    });
    if readonly && let Err(e) = set_readonly(file, true) {
        log::warn!("lighter rendering: couldn't make {} read-only again: {e}", file.display());
    }
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    done
}

/// The original beside the flag file: `1` then its bytes, or `0` when there was no file.
const SAVED: &str = "ClientAppSettings.json.roalt-before";

/// Puts back the original kept beside `file`, if one is; it stays there until that worked. False
/// when it is still there.
fn put_back_at(file: &Path) -> bool {
    let saved = file.with_file_name(SAVED);
    let Ok(kept) = std::fs::read(&saved) else { return true };
    let done = match kept.split_first() {
        Some((b'1', bytes)) => write(file, bytes),
        _ => retry(|| std::fs::remove_file(file)).or_else(|e| if e.kind() == ErrorKind::NotFound { Ok(()) } else { Err(e) }),
    };
    match done.and_then(|()| retry(|| std::fs::remove_file(&saved))) {
        Ok(()) => true,
        Err(e) => {
            log::warn!("lighter rendering: couldn't put {} back: {e}", file.display());
            false
        }
    }
}

/// A launch's flags are in the file until this runs.
pub struct Undo {
    file: PathBuf,
}

impl Undo {
    /// Puts the file back exactly as it was (removed if there was none); failures are logged.
    pub fn put_back(self) {
        let _ = put_back_at(&self.file);
    }
}

/// Puts back what a launch cut short by a quit or crash left behind (at start, and at exit while a
/// launch may still be waiting for its client), in every build: a launch can use another than
/// `client::find`'s.
pub fn restore() {
    #[cfg(not(windows))]
    return;
    #[cfg(windows)]
    {
        for dir in super::client::versions_dirs() {
            for build in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let _ = put_back_at(&build.path().join("ClientSettings").join("ClientAppSettings.json"));
            }
        }
        if let Ok(exe) = super::client::find()
            && let Some(dir) = exe.path.parent()
        {
            let _ = put_back_at(&dir.join("ClientSettings").join("ClientAppSettings.json"));
        }
    }
}

/// Writes `render`'s flags next to the client at `exe` for the launch about to start; what to put
/// back once it has started, None when nothing was written. Failures are logged and never stop the
/// launch.
pub fn apply(exe: &Path, render: &RenderFlags) -> Option<Undo> {
    #[cfg(not(windows))]
    {
        let _ = (exe, render);
        None
    }
    #[cfg(windows)]
    {
        apply_windows(exe, render)
    }
}

#[cfg(windows)]
fn apply_windows(exe: &Path, render: &RenderFlags) -> Option<Undo> {
    let file = exe.parent()?.join("ClientSettings").join("ClientAppSettings.json");
    // A launch cut short: the user's own file first, or these flags would pass for it and the only
    // copy of it would be lost.
    if !put_back_at(&file) {
        log::warn!("lighter rendering skipped: the user's own {} couldn't be put back", file.display());
        return None;
    }
    let before = match retry(|| std::fs::read(&file)) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == ErrorKind::NotFound => None,
        Err(e) => {
            log::warn!("lighter rendering skipped: can't read {}: {e}", file.display());
            return None;
        }
    };
    let Some(text) = merge(before.as_deref().map(String::from_utf8_lossy).as_deref(), &flags_of(render)) else {
        log::warn!("lighter rendering skipped: {} isn't a JSON object", file.display());
        return None;
    };
    let saved = file.with_file_name(SAVED);
    let kept = match &before {
        Some(bytes) => [b"1", bytes.as_slice()].concat(),
        None => b"0".to_vec(),
    };
    let written = file
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&saved, kept))
        .and_then(|()| write(&file, text.as_bytes()));
    match written {
        Ok(()) => Some(Undo { file }),
        Err(e) => {
            log::warn!("lighter rendering not written to {}: {e}", file.display());
            let _ = std::fs::remove_file(&saved);
            None
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn merges_into_the_users_flags() {
        let set = flags_of(&RenderFlags::default());
        let v: Value = serde_json::from_str(&merge(None, &set).unwrap()).unwrap();
        assert_eq!(v["DFIntTextureQualityOverride"], "0");
        assert_eq!(v.as_object().unwrap().len(), set.len());
        let own = r#"{"FFlagDebugGraphicsPreferD3D11": "True", "DFIntTextureQualityOverride": "3"}"#;
        let v: Value = serde_json::from_str(&merge(Some(own), &set).unwrap()).unwrap();
        assert_eq!(v["FFlagDebugGraphicsPreferD3D11"], "True", "the user's own flag stays");
        assert_eq!(v["DFIntTextureQualityOverride"], "0");
        assert!(merge(Some("  "), &set).is_some(), "blank counts as empty");
        for bad in ["not json", "[1, 2]", "{\"a\":"] {
            assert_eq!(merge(Some(bad), &set), None, "{bad}");
        }
    }

    #[test]
    fn each_choice_gives_its_flags() {
        let has = |r: RenderFlags, k: &str| flags_of(&r).iter().find(|(f, _)| *f == k).map(|(_, v)| *v);
        let d = RenderFlags::default();
        assert_eq!(has(d, "FIntDebugForceMSAASamples"), Some("1"));
        assert_eq!(has(d, "FFlagDebugSkyGray"), None);
        assert_eq!(has(d, "FIntFRMMaxGrassDistance"), Some("0"));
        assert_eq!(has(d, "DFIntDebugFRMQualityLevelOverride"), Some("1"));
        assert_eq!(has(RenderFlags { msaa: Msaa::X4, ..d }, "FIntDebugForceMSAASamples"), Some("4"));
        assert_eq!(has(RenderFlags { msaa: Msaa::Default, ..d }, "FIntDebugForceMSAASamples"), None);
        assert_eq!(has(RenderFlags { texture: TextureQuality::High, ..d }, "DFIntTextureQualityOverride"), Some("3"));
        let off = RenderFlags { texture: TextureQuality::Default, ..d };
        assert_eq!((has(off, "DFIntTextureQualityOverride"), has(off, "DFFlagTextureQualityOverrideEnabled")), (None, None));
        assert_eq!(has(RenderFlags { gray_sky: true, ..d }, "FFlagDebugSkyGray"), Some("True"));
        assert_eq!(has(RenderFlags { no_grass: false, ..d }, "FIntFRMMinGrassDistance"), None);
        assert_eq!(has(RenderFlags { lowest_frm: false, ..d }, "DFIntDebugFRMQualityLevelOverride"), None);
        assert_eq!(has(d, "DFFlagDebugPauseVoxelizer"), None);
        assert_eq!(has(RenderFlags { pause_voxelizer: true, ..d }, "DFFlagDebugPauseVoxelizer"), Some("True"));
        assert_eq!(has(RenderFlags { mode: RenderMode::Vulkan, ..d }, MODES[1]), Some("True"));

        // Only one mode: a chosen one replaces the user's own, and none leaves it.
        let own = r#"{"FFlagDebugGraphicsPreferD3D11": "True"}"#;
        let set = flags_of(&RenderFlags { mode: RenderMode::Opengl, ..d });
        let v: Value = serde_json::from_str(&merge(Some(own), &set).unwrap()).unwrap();
        assert_eq!((v.get(MODES[0]), v[MODES[2]].as_str()), (None, Some("True")));
    }

    #[test]
    fn puts_the_file_back_exactly() {
        let dir = std::env::temp_dir().join(format!("roalt-test-flags-{}", std::process::id()));
        let exe = dir.join("RobloxPlayerBeta.exe");
        let file = dir.join("ClientSettings").join("ClientAppSettings.json");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // No file before: written, then removed again.
        let r = RenderFlags::default();
        let undo = apply(&exe, &r).expect("written");
        assert!(std::fs::read_to_string(&file).unwrap().contains("DFIntDebugFRMQualityLevelOverride"));
        undo.put_back();
        assert!(!file.exists());

        // The user's own file: the very same bytes come back.
        let own = b"{\r\n  \"FFlagDebugSkyGray\": \"True\"\r\n}";
        std::fs::write(&file, own).unwrap();
        apply(&exe, &r).expect("written").put_back();
        assert_eq!(std::fs::read(&file).unwrap(), own);

        // Cut short by a quit: the next launch doesn't mistake the flags for the user's own.
        let _left_behind = apply(&exe, &r).expect("written");
        apply(&exe, &r).expect("written").put_back();
        assert_eq!(std::fs::read(&file).unwrap(), own);
        assert!(!file.with_file_name(SAVED).exists());
        let _left_behind = apply(&exe, &r).expect("written");
        put_back_at(&file);
        assert_eq!(std::fs::read(&file).unwrap(), own, "as `restore` does");

        // Can't be put back (held open): the original isn't lost, and nothing is written over it.
        let _left_behind = apply(&exe, &r).expect("written");
        let flagged = std::fs::read(&file).unwrap();
        let kept = std::fs::read(file.with_file_name(SAVED)).unwrap();
        {
            use std::os::windows::fs::OpenOptionsExt;
            let _held = std::fs::OpenOptions::new().read(true).share_mode(1).open(&file).unwrap();
            assert!(apply(&exe, &r).is_none());
            assert_eq!(std::fs::read(&file).unwrap(), flagged);
            assert_eq!(std::fs::read(file.with_file_name(SAVED)).unwrap(), kept);
        }
        assert!(put_back_at(&file));
        assert_eq!(std::fs::read(&file).unwrap(), own);

        // Not a JSON object: left alone.
        std::fs::write(&file, b"oops").unwrap();
        assert!(apply(&exe, &r).is_none());
        assert_eq!(std::fs::read(&file).unwrap(), b"oops");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
