//! Performance profiles: the performance settings as a small JSON file (or the share code the UI
//! makes of it), to pass a working setup to another PC. Only performance settings travel, never
//! launch arguments, hotkeys, the webhook, the vault or anything per account. Bandwidth isn't one
//! either: applying it asks for admin rights. An import is checked with the same rules as Settings
//! (`commands::apply_performance`) and saved through `settings_set` once the user has seen what
//! changes.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{AppError, Result};
use crate::model::{Adaptive, MemThrottle, RenderFlags, Settings, SettingsPatch};

const FORMAT: &str = "roalt-performance";
const VERSION: u32 = 1;
/// A profile is a few hundred bytes; anything bigger isn't one.
pub const MAX_BYTES: usize = 64 * 1024;
/// The Dashboard's preview frame rates (`src/features/dashboard/previewFps.ts`).
const PREVIEW_FPS: [u32; 4] = [5, 15, 30, 60];

/// The settings a profile carries; None: not in it (left as it is).
#[derive(Serialize, Deserialize, TS, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct Profile {
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps_cap: Option<u32>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub low_graphics: Option<bool>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lighter_rendering: Option<bool>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub render_flags: Option<RenderFlags>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimize_on_launch: Option<bool>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub micro_width: Option<u32>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub micro_height: Option<u32>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tiny_on_launch: Option<bool>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem_restart_mb: Option<u32>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem_throttle: Option<MemThrottle>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trim_every_min: Option<u32>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_spread: Option<bool>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fair_cap: Option<bool>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adaptive: Option<Adaptive>,
    /// The Dashboard's preview frame rate (this PC's UI keeps it, not the settings).
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_fps: Option<u32>,
}

/// The file.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct File {
    format: String,
    version: u32,
    #[serde(default)]
    exported_at: i64,
    #[serde(default)]
    settings: Profile,
}

fn not_a_profile() -> AppError {
    AppError::Invalid(tr!("This isn't a RoALT performance profile."))
}

/// The profile of `v` (pure): every performance setting, plus the preview rate.
pub fn of(v: &Settings, preview_fps: Option<u32>, now_ms: i64) -> String {
    let settings = Profile {
        fps_cap: Some(v.fps_cap),
        low_graphics: Some(v.low_graphics),
        lighter_rendering: Some(v.lighter_rendering),
        render_flags: Some(v.render_flags),
        minimize_on_launch: Some(v.minimize_on_launch),
        micro_width: Some(v.micro_width),
        micro_height: Some(v.micro_height),
        tiny_on_launch: Some(v.tiny_on_launch),
        mem_restart_mb: Some(v.mem_restart_mb),
        mem_throttle: Some(v.mem_throttle),
        trim_every_min: Some(v.trim_every_min),
        auto_spread: Some(v.auto_spread),
        fair_cap: Some(v.fair_cap),
        adaptive: Some(without_cores(v.adaptive)),
        preview_fps: preview_fps.filter(|f| PREVIEW_FPS.contains(f)),
    };
    let file = File { format: FORMAT.into(), version: VERSION, exported_at: now_ms, settings };
    serde_json::to_string_pretty(&file).expect("plain data serializes")
}

/// Which cores are this PC's business: a mask from another one could pin clients to a single core.
fn without_cores(mut a: Adaptive) -> Adaptive {
    a.background.affinity = None;
    a
}

fn patch_of(p: &Profile) -> SettingsPatch {
    SettingsPatch {
        fps_cap: p.fps_cap,
        low_graphics: p.low_graphics,
        lighter_rendering: p.lighter_rendering,
        render_flags: p.render_flags,
        minimize_on_launch: p.minimize_on_launch,
        micro_width: p.micro_width,
        micro_height: p.micro_height,
        tiny_on_launch: p.tiny_on_launch,
        mem_restart_mb: p.mem_restart_mb,
        mem_throttle: p.mem_throttle,
        trim_every_min: p.trim_every_min,
        auto_spread: p.auto_spread,
        fair_cap: p.fair_cap,
        adaptive: p.adaptive,
        ..Default::default()
    }
}

/// The profile in `text`, checked against the rules Settings applies to `current` (pure; nothing is
/// saved). Unknown keys are ignored; a preview rate the Dashboard doesn't offer is dropped.
pub fn parse(text: &str, current: &Settings) -> Result<Profile> {
    if text.len() > MAX_BYTES {
        return Err(not_a_profile());
    }
    let file: File = serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|_| not_a_profile())?;
    if file.format != FORMAT {
        return Err(not_a_profile());
    }
    if file.version > VERSION {
        return Err(AppError::Invalid(tr!("This profile was made by a newer RoALT; update RoALT first.")));
    }
    let mut p = file.settings;
    p.adaptive = p.adaptive.map(without_cores);
    crate::commands::apply_performance(&mut current.clone(), &patch_of(&p))?;
    p.preview_fps = p.preview_fps.filter(|f| PREVIEW_FPS.contains(f));
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_profile_round_trips() {
        let v = Settings { fps_cap: 30, mem_restart_mb: 2048, fair_cap: true, ..Settings::default() };
        let p = parse(&of(&v, Some(5), 1), &Settings::default()).unwrap();
        assert_eq!(p.fps_cap, Some(30));
        assert_eq!(p.mem_restart_mb, Some(2048));
        assert_eq!(p.lighter_rendering, Some(true));
        assert_eq!((p.fair_cap, p.preview_fps), (Some(true), Some(5)));
        let mut applied = Settings::default();
        crate::commands::apply_performance(&mut applied, &patch_of(&p)).unwrap();
        assert_eq!(applied, v, "applying it gives the same settings");
    }

    #[test]
    fn a_profile_from_before_park_minimized_went_still_loads() {
        let v = Settings::default();
        let old = of(&v, None, 1).replacen("\"fpsCap\":", "\"parkMinimized\":true,\"fpsCap\":", 1);
        assert!(old.contains("parkMinimized"));
        assert_eq!(parse(&old, &v).unwrap().fps_cap, Some(v.fps_cap));
    }

    #[test]
    fn refuses_what_isnt_a_valid_profile() {
        let s = Settings::default();
        for bad in ["", "not json", "{}", r#"{"format":"roalt-backup","version":1}"#, "[1]"] {
            assert!(parse(bad, &s).is_err(), "{bad}");
        }
        assert!(parse(r#"{"format":"roalt-performance","version":2}"#, &s).is_err(), "newer");
        assert!(parse(r#"{"format":"roalt-performance","version":1,"settings":{"fpsCap":2}}"#, &s).is_err(), "out of range");
        assert!(parse(r#"{"format":"roalt-performance","version":1,"settings":{"memRestartMb":100}}"#, &s).is_err());
        assert!(parse(&"x".repeat(MAX_BYTES + 1), &s).is_err(), "too big");
    }

    #[test]
    fn what_isnt_in_it_stays_out_and_cores_dont_travel() {
        let text = r#"{"format":"roalt-performance","version":1,"settings":{"fpsCap":15,"adaptive":{"enabled":true,"background":{"cpuCap":25,"eco":true,"affinity":1}}}}"#;
        let p = parse(text, &Settings::default()).unwrap();
        assert_eq!(p.adaptive.unwrap().background.affinity, None);
        let json = serde_json::to_value(parse(r#"{"format":"roalt-performance","version":1,"settings":{"fpsCap":15}}"#, &Settings::default()).unwrap()).unwrap();
        assert_eq!(json, serde_json::json!({ "fpsCap": 15 }), "no nulls for the UI to save");
    }

    #[test]
    fn carries_only_performance_settings() {
        let text = r#"{"format":"roalt-performance","version":1,"settings":{"fpsCap":15,"launchArgs":"--evil","panicHotkey":"Ctrl+A","previewFps":45},"extra":1}"#;
        let p = parse(text, &Settings::default()).unwrap();
        assert_eq!(p, Profile { fps_cap: Some(15), ..Default::default() }, "unknown keys and a bad preview rate are dropped");
        let patch = patch_of(&p);
        assert!(patch.launch_args.is_none() && patch.panic_hotkey.is_none());
    }
}
