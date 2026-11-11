//! Performance profiles: the performance settings as a small JSON file (or the share code the UI
//! makes of it), to pass a working setup to another PC. Only performance settings travel, never
//! launch arguments, hotkeys, the webhook, the vault or anything per account. Bandwidth isn't one
//! either: applying it asks for admin rights. An import is checked with the same rules as Settings
//! (`commands::apply_performance`) and saved through `settings_set` once the user has seen what
//! changes. `suggest` makes such profiles for this PC from a benchmark of its running clients.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::error::{AppError, Result};
use crate::model::{Adaptive, Limits, MemThrottle, Msaa, RenderFlags, Settings, SettingsPatch, TextureQuality, WindowLayout};

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
    pub mute_on_launch: Option<bool>,
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
        mute_on_launch: Some(v.mute_on_launch),
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
        mute_on_launch: p.mute_on_launch,
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
    check(&p, current)?;
    p.preview_fps = p.preview_fps.filter(|f| PREVIEW_FPS.contains(f));
    Ok(p)
}

/// `current` with `p` applied, checked as Settings checks it (pure); mute too, which `settings_set` sets.
fn applied(p: &Profile, current: &Settings) -> Result<Settings> {
    let mut v = current.clone();
    crate::commands::apply_performance(&mut v, &patch_of(p))?;
    if let Some(m) = p.mute_on_launch {
        v.mute_on_launch = m;
    }
    Ok(v)
}

/// `p` passes the rules Settings applies to `current` (pure).
pub fn check(p: &Profile, current: &Settings) -> Result<()> {
    applied(p, current).map(drop)
}

// ---------- "For this PC": what the clients in a game use, and what to turn on ----------

// shortcut: every threshold below is a guess, unmeasured (like `limits::BUDGET`); tune from real benchmarks.
/// Projected share of RAM or CPU under which the PC has plenty of room, then under which it is
/// tight (Home's verdict cut-offs); from there on it is over the limit.
const LIGHT_BELOW: f64 = 0.60;
const TIGHT_BELOW: f64 = 0.85;
/// The FPS caps, lightest first: plenty of room (2 clients at most, then more), tight, over.
const FPS_STEPS: [u32; 4] = [60, 30, 20, 10];
/// The clients' summed GPU use (percent) from which the FPS cap goes a step lower, and from which
/// lowest graphics is worth it even with room.
const GPU_BUSY: f64 = 80.0;
const GPU_LOW_GRAPHICS: f64 = 50.0;
/// When tight, minimize clients on launch from this many, mute them from this many.
const MINIMIZE_FROM: u32 = 4;
const MUTE_FROM: u32 = 3;
/// The tiny size when over the limit.
const TINY: (u32, u32) = (240, 180);
/// The adaptive manager's background CPU cap (percent), tight and over.
const BG_CAP: (u8, u8) = (25, 10);
const THROTTLE_CAP: u8 = 10;
/// The memory throttle's and restart's limits: peak private memory times these, the restart at
/// least `RESTART_MIN_MB`.
const THROTTLE_X: f64 = 1.5;
const RESTART_X: f64 = 2.0;
const RESTART_MIN_MB: f64 = 2048.0;
/// With a restart limit on, the throttle at most this share of it.
const THROTTLE_UNDER_RESTART: f64 = 0.75;
/// A crash or hang after this long counts as a long run; this many in a week suggest a memory restart.
pub const LONG_RUN_MS: i64 = 2 * 3_600_000;
const LONG_CRASHES: u32 = 2;

/// How the projected load fits the PC.
#[derive(Serialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Tier {
    Light,
    Tight,
    Over,
}

/// Whether Windows holds clients to a CPU cap here (`Limiter::uncapped`).
#[derive(Serialize, TS, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Caps {
    Works,
    Refused,
    /// No cap was tried yet; taken as working.
    #[default]
    Unknown,
}

/// What the benchmark measured (`commands::performance_benchmark`).
#[derive(Serialize, TS, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Bench {
    pub cores: u32,
    pub ram_mb: f64,
    /// Clients in a game, and how many of them run in the background (not played, not in front).
    pub clients: u32,
    pub background: u32,
    /// A client in a game: mean private working set and the highest private bytes, MB.
    pub per_mb: f64,
    pub peak_private_mb: f64,
    /// A background client's mean CPU, in cores (all in-game clients' when none is in the background).
    pub per_cores: f64,
    /// The free clients' (played, in front) summed CPU, in cores; 0 when none is in the background.
    pub free_cores: f64,
    /// A background client had a CPU cap while measured, so `per_cores` reads low.
    pub under_limits: bool,
    /// What everything but the clients uses: MB, cores.
    pub other_mb: f64,
    pub other_cores: f64,
    /// The clients' summed busiest-engine GPU use at its highest, percent (rough); None: Windows didn't say.
    pub gpu_peak: Option<f64>,
    pub caps: Caps,
    /// In the last 7 days: RoALT's memory restarts, crashes and hangs after a long run (`LONG_RUN_MS`).
    pub mem_restarts: u32,
    pub long_crashes: u32,
}

/// What a suggestion turns on; the UI words it, from `params`.
#[derive(Serialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SuggestionId {
    FpsCap,
    LowGraphics,
    LighterRendering,
    MinimizeOnLaunch,
    TinyOnLaunch,
    MuteOnLaunch,
    AutoSpread,
    FairCap,
    Adaptive,
    TrimEveryMin,
    MemThrottle,
    MemRestart,
    PreviewFps,
}

/// One thing to turn on, as the profile fragment that does it.
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Suggestion {
    pub id: SuggestionId,
    /// The numbers its reason needs (see `suggest`).
    pub params: std::collections::BTreeMap<String, f64>,
    pub profile: Profile,
}

/// The projection for `n` clients like the measured ones, and what to turn on for it.
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Advice {
    pub n: u32,
    pub tier: Tier,
    /// Projected share of the RAM and of the CPU (1: all of it).
    pub ram_ratio: f64,
    pub cpu_ratio: f64,
    /// How many such clients stay under 85 % (`TIGHT_BELOW`) of both; None: nothing to go by.
    pub room: Option<u32>,
    pub suggestions: Vec<Suggestion>,
}

/// What `performance_benchmark` answers.
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Benchmark {
    pub bench: Bench,
    pub advice: Advice,
}

/// One round of the benchmark.
pub struct Round {
    /// The clients in a game; true: in the background.
    pub games: Vec<(crate::model::ClientMetrics, bool)>,
    /// Private working set (MB) and CPU (percent of the PC) of every client, in a game or not.
    pub clients_mb: f64,
    pub clients_cpu: f64,
    pub system: crate::model::SystemUsage,
}

/// What `rounds` measured on a PC with `cores` (pure); the rest of `Bench` is the caller's.
pub fn measured(rounds: &[Round], cores: u32) -> Bench {
    let cores = cores.max(1);
    let mean = |v: Vec<f64>| if v.is_empty() { 0.0 } else { v.iter().sum::<f64>() / v.len() as f64 };
    let games = || rounds.iter().flat_map(|r| &r.games);
    let background: Vec<f64> = games().filter(|g| g.1).filter_map(|g| g.0.cpu_pct).collect();
    // With no background client, per_cores already averages the free ones.
    let free = if background.is_empty() { 0.0 } else { mean(rounds.iter().map(|r| r.games.iter().filter(|g| !g.1).filter_map(|g| g.0.cpu_pct).sum()).collect()) };
    let cpu = if background.is_empty() { games().filter_map(|g| g.0.cpu_pct).collect() } else { background };
    let last = rounds.last();
    Bench {
        cores,
        ram_mb: last.map_or(0.0, |r| r.system.mem_total_mb),
        clients: last.map_or(0, |r| r.games.len() as u32),
        background: last.map_or(0, |r| r.games.iter().filter(|g| g.1).count() as u32),
        per_mb: mean(games().map(|g| g.0.memory_mb).collect()),
        peak_private_mb: games().map(|g| g.0.private_mb).fold(0.0, f64::max),
        per_cores: mean(cpu) * f64::from(cores) / 100.0,
        free_cores: free * f64::from(cores) / 100.0,
        other_mb: mean(rounds.iter().map(|r| (r.system.mem_used_mb - r.clients_mb).max(0.0)).collect()),
        other_cores: mean(rounds.iter().filter_map(|r| r.system.cpu_pct.map(|c| (c - r.clients_cpu).max(0.0))).collect()) * f64::from(cores) / 100.0,
        gpu_peak: rounds
            .iter()
            .filter_map(|r| r.games.iter().filter_map(|g| g.0.gpu_pct).reduce(|a, b| a + b))
            .reduce(f64::max)
            .map(|g| g.min(100.0)),
        ..Bench::default()
    }
}

/// `mb` rounded up to 256 MB, within the memory limits' 512..=65 536.
fn mem_limit(mb: f64) -> u32 {
    ((mb / 256.0).ceil() * 256.0).clamp(512.0, 65_536.0) as u32
}

/// The projection of `target` clients (default: as many as `b` measured) and what `v` could turn on
/// for it (pure). Only adds: a suggestion is there only when it makes `v` lighter, never looser.
/// Params: fpsCap fps (gpu: when the GPU lowered it); lowGraphics gpu (when the GPU is why);
/// minimize/mute n; tiny width, height; autoSpread n, cores; fairCap cap (percent each), n;
/// adaptive cap (none: efficiency mode only); trimEveryMin min; memThrottle mb, peak; memRestart mb
/// (crashes: when long runs crashed); previewFps fps (the UI drops it when its own rate is lower).
pub fn suggest(b: &Bench, v: &Settings, target: Option<u32>) -> Advice {
    use crate::instances::limits::{fair_cap, spread};
    let n = target.unwrap_or(b.clients).max(1);
    let (nf, cores) = (f64::from(n), f64::from(b.cores.max(1)));
    // The free clients stay as they are (played, full quality); the rest are like the background ones.
    let free = if b.free_cores > 0.0 { b.clients.saturating_sub(b.background) } else { 0 };
    let ram_ratio = (nf * b.per_mb + b.other_mb) / b.ram_mb.max(1.0);
    // Fewer clients than the free ones measured: only the kept free clients' share counts.
    let kept = free.min(n);
    let free_share = if free > 0 { b.free_cores * f64::from(kept) / f64::from(free) } else { 0.0 };
    let cpu_ratio = (free_share + f64::from(n - kept) * b.per_cores + b.other_cores) / cores;
    let worst = ram_ratio.max(cpu_ratio);
    let tier = if worst < LIGHT_BELOW { Tier::Light } else if worst < TIGHT_BELOW { Tier::Tight } else { Tier::Over };
    let room = [(b.ram_mb, b.other_mb, b.per_mb, 0), (cores, b.other_cores + b.free_cores, b.per_cores, free)]
        .into_iter()
        .filter(|&(_, _, per, _)| per > 0.0)
        .map(|(all, other, per, fixed)| (((TIGHT_BELOW * all - other) / per).floor() + f64::from(fixed)).max(0.0) as u32)
        .min();
    let busy = tier != Tier::Light;
    let over = tier == Tier::Over;
    let gpu = b.gpu_peak.unwrap_or(0.0);

    let mut out = Vec::new();
    let mut add = |id, params: &[(&str, f64)], profile: Profile| match applied(&profile, v) {
        Ok(after) if after != *v || profile.preview_fps.is_some() => {
            out.push(Suggestion { id, params: params.iter().map(|&(k, x)| (k.to_owned(), x)).collect(), profile })
        }
        Ok(_) => {}
        Err(e) => log::warn!("benchmark: dropped suggestion {id:?}: {e}"),
    };

    let step = match tier {
        Tier::Light if n <= 2 => 0,
        Tier::Light => 1,
        Tier::Tight => 2,
        Tier::Over => 3,
    };
    let gpu_busy = gpu >= GPU_BUSY;
    let fps = FPS_STEPS[(step + usize::from(gpu_busy)).min(FPS_STEPS.len() - 1)];
    let fps = if v.fps_cap == 0 { fps } else { v.fps_cap.min(fps) };
    let gpu_param: &[(&str, f64)] = if gpu_busy { &[("gpu", gpu)] } else { &[] };
    add(SuggestionId::FpsCap, &[&[("fps", f64::from(fps))], gpu_param].concat(), Profile { fps_cap: Some(fps), ..Profile::default() });
    if busy || gpu >= GPU_LOW_GRAPHICS {
        let why: &[(&str, f64)] = if busy { &[] } else { &[("gpu", gpu)] };
        add(SuggestionId::LowGraphics, why, Profile { low_graphics: Some(true), ..Profile::default() });
    }
    if busy {
        let mut f = RenderFlags { msaa: Msaa::X1, texture: TextureQuality::Lowest, no_grass: true, lowest_frm: true, ..v.render_flags };
        if over {
            (f.gray_sky, f.pause_voxelizer) = (true, true);
        }
        add(SuggestionId::LighterRendering, &[], Profile { lighter_rendering: Some(true), render_flags: Some(f), ..Profile::default() });
    }
    if over || (busy && n >= MINIMIZE_FROM) {
        add(SuggestionId::MinimizeOnLaunch, &[("n", nf)], Profile { minimize_on_launch: Some(true), ..Profile::default() });
    }
    // A window layout replaces tiny on launch.
    if over && v.window_layout == WindowLayout::Off {
        let (w, h) = (v.micro_width.min(TINY.0), v.micro_height.min(TINY.1));
        let p = Profile { tiny_on_launch: Some(true), micro_width: Some(w), micro_height: Some(h), ..Profile::default() };
        add(SuggestionId::TinyOnLaunch, &[("width", f64::from(w)), ("height", f64::from(h))], p);
    }
    if busy && n >= MUTE_FROM {
        add(SuggestionId::MuteOnLaunch, &[("n", nf)], Profile { mute_on_launch: Some(true), ..Profile::default() });
    }
    // Pinning is what a client whose cap fails has left (`limits::plan`).
    if n >= 2 && (spread(0, n, b.cores).is_some() || b.caps == Caps::Refused) {
        add(SuggestionId::AutoSpread, &[("n", nf), ("cores", cores)], Profile { auto_spread: Some(true), ..Profile::default() });
    }
    if n >= 2 && b.caps != Caps::Refused && cpu_ratio >= TIGHT_BELOW {
        add(SuggestionId::FairCap, &[("cap", f64::from(fair_cap(n, b.cores))), ("n", nf)], Profile { fair_cap: Some(true), ..Profile::default() });
    }
    if n >= 2 && busy {
        let bg = v.adaptive.background;
        let cap = if over { BG_CAP.1 } else { BG_CAP.0 };
        let cpu_cap = match b.caps {
            // A cap Windows refuses does nothing: efficiency mode only.
            Caps::Refused => bg.cpu_cap.filter(|_| v.adaptive.enabled),
            _ => Some(bg.cpu_cap.map_or(cap, |c| c.min(cap))),
        };
        let a = Adaptive { enabled: true, background: Limits { cpu_cap, eco: true, ..bg } };
        let params: &[(&str, f64)] = if b.caps == Caps::Refused { &[] } else { &[("cap", f64::from(cpu_cap.unwrap_or(cap)))] };
        add(SuggestionId::Adaptive, params, Profile { adaptive: Some(a), ..Profile::default() });
    }
    // Memory restarts already firing say memory runs out: trim and throttle sooner.
    let ram_tight = ram_ratio >= TIGHT_BELOW || b.mem_restarts > 0;
    let trim = if ram_tight { Some(15) } else if ram_ratio >= LIGHT_BELOW { Some(30) } else { None };
    if let Some(t) = trim {
        let t = if v.trim_every_min == 0 { t } else { v.trim_every_min.min(t) };
        add(SuggestionId::TrimEveryMin, &[("min", f64::from(t))], Profile { trim_every_min: Some(t), ..Profile::default() });
    }
    let mut throttle = v.mem_throttle.mb;
    // The throttle holds by a CPU cap, so it does nothing where caps are refused.
    if ram_tight && v.mem_throttle.mb == 0 && b.caps != Caps::Refused {
        let mut mb = mem_limit(b.peak_private_mb * THROTTLE_X);
        // Under a restart limit that's on, or the restart fires first and the throttle never holds.
        if v.mem_restart_mb > 0 {
            mb = mb.min(((f64::from(v.mem_restart_mb) * THROTTLE_UNDER_RESTART / 256.0).floor() * 256.0) as u32);
        }
        if mb >= 512 {
            throttle = mb;
            let t = MemThrottle { mb, cpu_cap: v.mem_throttle.cpu_cap.min(THROTTLE_CAP) };
            add(SuggestionId::MemThrottle, &[("mb", f64::from(mb)), ("peak", b.peak_private_mb)], Profile { mem_throttle: Some(t), ..Profile::default() });
        }
    }
    // A restart disconnects the client: only past all the RAM, or after long runs crashed.
    let crashed = b.long_crashes >= LONG_CRASHES;
    if (ram_ratio >= 1.0 || crashed) && v.mem_restart_mb == 0 {
        let mb = mem_limit((b.peak_private_mb * RESTART_X).max(RESTART_MIN_MB).max(f64::from(throttle) + 512.0));
        let why: &[(&str, f64)] = if crashed { &[("crashes", f64::from(b.long_crashes))] } else { &[] };
        add(SuggestionId::MemRestart, &[&[("mb", f64::from(mb))], why].concat(), Profile { mem_restart_mb: Some(mb), ..Profile::default() });
    }
    if busy {
        let fps = if over { 5 } else { 15 };
        add(SuggestionId::PreviewFps, &[("fps", f64::from(fps))], Profile { preview_fps: Some(fps), ..Profile::default() });
    }
    Advice { n, tier, ram_ratio, cpu_ratio, room, suggestions: out }
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

    fn pc(cores: u32, ram_gb: f64, clients: u32, per_mb: f64, per_cores: f64) -> Bench {
        Bench { cores, ram_mb: ram_gb * 1024.0, clients, background: clients, per_mb, peak_private_mb: per_mb * 1.3, per_cores, other_mb: 1500.0, other_cores: 0.5, ..Bench::default() }
    }

    fn find(a: &Advice, id: SuggestionId) -> Option<&Profile> {
        a.suggestions.iter().find(|s| s.id == id).map(|s| &s.profile)
    }

    #[test]
    fn plenty_of_room_caps_fps_and_spreads() {
        let a = suggest(&pc(16, 32.0, 2, 1000.0, 0.5), &Settings::default(), None);
        assert_eq!((a.n, a.tier), (2, Tier::Light));
        assert_eq!(find(&a, SuggestionId::FpsCap).unwrap().fps_cap, Some(60));
        assert!(find(&a, SuggestionId::AutoSpread).is_some());
        for id in [SuggestionId::FairCap, SuggestionId::MemThrottle, SuggestionId::MemRestart, SuggestionId::Adaptive, SuggestionId::PreviewFps] {
            assert!(find(&a, id).is_none(), "{id:?}");
        }
    }

    #[test]
    fn over_the_limit_turns_on_the_heavy_ones() {
        let a = suggest(&pc(4, 8.0, 6, 1000.0, 0.5), &Settings::default(), None);
        assert_eq!(a.tier, Tier::Over);
        assert_eq!(find(&a, SuggestionId::FpsCap).unwrap().fps_cap, Some(10));
        let tiny = find(&a, SuggestionId::TinyOnLaunch).unwrap();
        assert_eq!((tiny.micro_width, tiny.micro_height), (Some(240), Some(180)));
        assert_eq!(find(&a, SuggestionId::MinimizeOnLaunch).unwrap().minimize_on_launch, Some(true));
        assert_eq!(find(&a, SuggestionId::MuteOnLaunch).unwrap().mute_on_launch, Some(true));
        assert_eq!(find(&a, SuggestionId::Adaptive).unwrap().adaptive.unwrap().background.cpu_cap, Some(10));
        assert!(find(&a, SuggestionId::FairCap).is_some());
        assert_eq!(find(&a, SuggestionId::TrimEveryMin).unwrap().trim_every_min, Some(15));
        assert!(find(&a, SuggestionId::MemThrottle).unwrap().mem_throttle.unwrap().mb >= 512);
        assert!(find(&a, SuggestionId::MemRestart).is_none(), "under all the RAM, no long-run crashes");
        assert_eq!(find(&a, SuggestionId::PreviewFps).unwrap().preview_fps, Some(5));
        assert!(find(&a, SuggestionId::AutoSpread).is_none(), "4 cores can't give 6 clients two each");
    }

    #[test]
    fn a_busy_gpu_lowers_the_fps_cap_a_step() {
        let b = Bench { gpu_peak: Some(85.0), ..pc(16, 32.0, 2, 1000.0, 0.5) };
        let a = suggest(&b, &Settings::default(), None);
        let s = a.suggestions.iter().find(|s| s.id == SuggestionId::FpsCap).unwrap();
        assert_eq!((s.profile.fps_cap, s.params.get("gpu")), (Some(30), Some(&85.0)));
    }

    #[test]
    fn refused_caps_get_no_fair_cap_but_pinning() {
        let b = Bench { caps: Caps::Refused, ..pc(4, 8.0, 6, 1000.0, 0.5) };
        let a = suggest(&b, &Settings::default(), None);
        assert!(find(&a, SuggestionId::FairCap).is_none());
        assert_eq!(find(&a, SuggestionId::Adaptive).unwrap().adaptive.unwrap().background.cpu_cap, None);
        assert!(find(&a, SuggestionId::AutoSpread).is_some());
        assert!(find(&a, SuggestionId::MemThrottle).is_none(), "it holds by a CPU cap");
    }

    #[test]
    fn the_throttle_stays_under_a_restart_limit_thats_on() {
        let v = Settings { mem_restart_mb: 2048, ..Settings::default() };
        let b = Bench { mem_restarts: 1, peak_private_mb: 2000.0, ..pc(16, 32.0, 2, 1000.0, 0.5) };
        let mb = find(&suggest(&b, &v, None), SuggestionId::MemThrottle).unwrap().mem_throttle.unwrap().mb;
        assert!((512..2048).contains(&mb), "{mb}");
        let v = Settings { mem_restart_mb: 600, ..Settings::default() };
        assert!(find(&suggest(&b, &v, None), SuggestionId::MemThrottle).is_none(), "no room under it");
    }

    #[test]
    fn memory_restarts_seen_throttle_and_trim_but_long_crashes_restart() {
        let b = Bench { mem_restarts: 2, ..pc(16, 32.0, 2, 1000.0, 0.5) };
        let a = suggest(&b, &Settings::default(), None);
        assert_eq!(find(&a, SuggestionId::TrimEveryMin).unwrap().trim_every_min, Some(15));
        assert!(find(&a, SuggestionId::MemThrottle).is_some());
        assert!(find(&a, SuggestionId::MemRestart).is_none(), "its own restarts don't justify restarts");

        let b = Bench { long_crashes: 2, ..pc(4, 8.0, 6, 3000.0, 0.5) };
        let a = suggest(&b, &Settings::default(), None);
        let throttle = find(&a, SuggestionId::MemThrottle).unwrap().mem_throttle.unwrap().mb;
        let restart = find(&a, SuggestionId::MemRestart).unwrap().mem_restart_mb.unwrap();
        assert!(restart >= throttle + 512 && (512..=65_536).contains(&throttle) && (512..=65_536).contains(&restart));
    }

    #[test]
    fn a_window_layout_gets_no_tiny() {
        let v = Settings { window_layout: WindowLayout::Fit, ..Settings::default() };
        assert!(find(&suggest(&pc(4, 8.0, 6, 1000.0, 0.5), &v, None), SuggestionId::TinyOnLaunch).is_none());
    }

    #[test]
    fn stronger_settings_stay() {
        let v = Settings { fps_cap: 5, trim_every_min: 5, ..Settings::default() };
        let a = suggest(&pc(4, 8.0, 6, 1000.0, 0.5), &v, None);
        assert!(find(&a, SuggestionId::FpsCap).is_none() && find(&a, SuggestionId::TrimEveryMin).is_none());
    }

    #[test]
    fn room_never_goes_negative_nor_divides_by_zero() {
        let b = Bench { other_mb: 30_000.0, ..pc(8, 16.0, 2, 1000.0, 0.0) };
        let a = suggest(&b, &Settings::default(), Some(3));
        assert_eq!((a.n, a.room), (3, Some(0)));
        assert!(a.ram_ratio.is_finite() && a.cpu_ratio.is_finite());
        let a = suggest(&Bench::default(), &Settings::default(), Some(0));
        assert_eq!((a.n, a.room), (1, None));
        assert!(a.ram_ratio.is_finite() && a.cpu_ratio.is_finite());
    }

    #[test]
    fn every_suggestion_on_any_pc_is_valid() {
        let v = Settings::default();
        for cores in [2, 4, 8, 16, 32] {
            for ram_gb in [4.0, 8.0, 16.0, 64.0] {
                for per_mb in [300.0, 1000.0, 2000.0, 4000.0] {
                    for caps in [Caps::Works, Caps::Refused, Caps::Unknown] {
                        for n in 1..=40 {
                            let b = Bench { caps, gpu_peak: Some(f64::from(n) * 3.0), mem_restarts: n % 2, long_crashes: n % 3, ..pc(cores, ram_gb, n, per_mb, 0.4) };
                            let a = suggest(&b, &v, None);
                            let mut all = v.clone();
                            for s in &a.suggestions {
                                check(&s.profile, &v).unwrap();
                                all = applied(&s.profile, &all).unwrap();
                                assert!(s.profile.adaptive.is_none_or(|a| a.background.affinity.is_none()));
                                assert!(s.profile.fps_cap.is_none_or(|f| f % 5 == 0));
                                assert!(s.profile.preview_fps.is_none_or(|f| PREVIEW_FPS.contains(&f)));
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_benchmark_splits_background_from_free() {
        use crate::model::{ClientMetrics, SystemUsage};
        let m = |cpu, mb, gpu| ClientMetrics {
            instance_id: String::new(), pid: 1, cpu_pct: Some(cpu), memory_mb: mb, private_mb: mb * 2.0,
            read_bps: None, write_bps: None, gpu_pct: gpu, gpu_mem_mb: None,
        };
        let round = |gpu| Round {
            games: vec![(m(5.0, 1000.0, gpu), true), (m(20.0, 2000.0, Some(30.0)), false)],
            clients_mb: 3000.0,
            clients_cpu: 25.0,
            system: SystemUsage { cpu_pct: Some(40.0), mem_used_mb: 7000.0, mem_total_mb: 16_384.0 },
        };
        let b = measured(&[round(Some(10.0)), round(None)], 8);
        assert_eq!((b.clients, b.background, b.ram_mb), (2, 1, 16_384.0));
        assert_eq!((b.per_mb, b.peak_private_mb), (1500.0, 4000.0));
        assert!((b.per_cores - 0.4).abs() < 1e-9, "the background client's 5 % of 8 cores");
        assert!((b.free_cores - 1.6).abs() < 1e-9, "the played client's 20 % of 8 cores");
        assert!((b.other_cores - 1.2).abs() < 1e-9 && b.other_mb == 4000.0);
        assert_eq!(b.gpu_peak, Some(40.0));
        assert_eq!(measured(&[], 8).per_mb, 0.0);
    }

    #[test]
    fn the_played_clients_cpu_counts() {
        use crate::model::{ClientMetrics, SystemUsage};
        let m = |cpu| ClientMetrics {
            instance_id: String::new(), pid: 1, cpu_pct: Some(cpu), memory_mb: 500.0, private_mb: 600.0,
            read_bps: None, write_bps: None, gpu_pct: None, gpu_mem_mb: None,
        };
        // 4 cores: the played client at 50 %, 5 background at 5 %, other apps at 10 %.
        let mut games = vec![(m(50.0), false)];
        games.extend((0..5).map(|_| (m(5.0), true)));
        let round = Round { games, clients_mb: 3000.0, clients_cpu: 75.0, system: SystemUsage { cpu_pct: Some(85.0), mem_used_mb: 8000.0, mem_total_mb: 65_536.0 } };
        let b = measured(&[round], 4);
        let a = suggest(&b, &Settings::default(), None);
        assert_eq!(a.tier, Tier::Over, "{}", a.cpu_ratio);
        assert_eq!(a.room, Some(6), "1 played + (3.4 - 0.4 - 2) / 0.2 background");
    }

    #[test]
    fn fewer_clients_than_played_count_only_the_kept_ones() {
        // 2 played at 1 core each, 2 background at 0.5; projecting 1 keeps one played client.
        let b = Bench { free_cores: 2.0, background: 2, ..pc(4, 64.0, 4, 500.0, 0.5) };
        let a = suggest(&b, &Settings::default(), Some(1));
        assert!((a.cpu_ratio - (1.0 + 0.5) / 4.0).abs() < 1e-9, "{}", a.cpu_ratio);
        let a = suggest(&b, &Settings::default(), Some(3));
        assert!((a.cpu_ratio - (2.0 + 0.5 + 0.5) / 4.0).abs() < 1e-9, "{}", a.cpu_ratio);
    }
}
