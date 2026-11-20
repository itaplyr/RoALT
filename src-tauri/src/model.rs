//! Types shared with the UI. `npm run bindings` exports each one to `src/bindings/<Type>.ts`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum VaultMode {
    /// Data key wrapped with Windows DPAPI for the current user; opens without a prompt.
    Dpapi,
    /// Data key wrapped with a key derived from a master password (and DPAPI on top).
    Master,
}

#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VaultStatus {
    pub mode: VaultMode,
    pub locked: bool,
    pub accounts: u32,
    pub auto_lock_minutes: u32,
    /// A recovery code exists for the master password.
    pub has_recovery: bool,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum AccountStatus {
    Unknown,
    Valid,
    Invalid,
    /// Roblox asked for a human check (captcha / 2SV) on the last validation.
    Challenged,
}

impl AccountStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Valid => "valid",
            Self::Invalid => "invalid",
            Self::Challenged => "challenged",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "valid" => Self::Valid,
            "invalid" => Self::Invalid,
            "challenged" => Self::Challenged,
            "unknown" => Self::Unknown,
            other => {
                log::debug!("unrecognised account status {other:?}");
                Self::Unknown
            }
        }
    }
}

/// One stored account as the UI sees it. Never carries the cookie.
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AccountSummary {
    pub id: i64,
    pub user_id: i64,
    pub username: String,
    pub display_name: String,
    pub alias: String,
    pub notes: String,
    pub color: Option<String>,
    pub favourite: bool,
    pub sort_order: i64,
    pub group_ids: Vec<i64>,
    pub status: AccountStatus,
    /// Unix ms.
    pub status_checked_at: Option<i64>,
    /// Unix ms.
    pub created_at: i64,
    /// Its clients may stay disconnected: never rejoined or restarted, and a batch counts a
    /// disconnect as done.
    pub allow_disconnect: bool,
    /// Extra client arguments, used instead of `Settings.launch_args`; None: the default.
    pub launch_args: Option<String>,
    /// No Discord alerts about this account's clients (batch summaries and status still go).
    pub discord_muted: bool,
    /// Target FPS for its launches, used instead of `Settings.fps_cap`; None: the default.
    pub fps_cap: Option<u32>,
    /// Its clients keep the user's own graphics: no lowest graphics or lighter rendering.
    pub full_quality: bool,
    /// The user's own account (at most one): listed first, left out of select-all while
    /// `Settings.protect_main`, and confirmed before removal or showing its cookie.
    pub main: bool,
    /// Its own FFlags (`launcher::flags`), used while `Settings.custom_flags_scope` is Account.
    pub custom_flags: BTreeMap<String, String>,
    /// What was last set on one of its clients; its next client starts with it. None: nothing.
    pub client_limits: Option<ClientPrefs>,
    /// Its own performance profile: its clients get these settings instead of the ones in
    /// Settings (`performance::overlay`). None: the ones in Settings.
    pub profile: Option<crate::performance::Profile>,
}

/// What a running client was given by hand and its account keeps for the next one: its limits and
/// whether anti-AFK presses keys in it.
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct ClientPrefs {
    pub limits: Limits,
    pub anti_afk: bool,
}

impl Default for ClientPrefs {
    fn default() -> Self {
        Self { limits: Limits::default(), anti_afk: true }
    }
}

/// Partial update from the UI; `None` leaves a field as it is.
/// `color: Some("")` clears the colour.
#[derive(Deserialize, TS, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AccountPatch {
    #[ts(optional)]
    pub alias: Option<String>,
    #[ts(optional)]
    pub notes: Option<String>,
    #[ts(optional)]
    pub color: Option<String>,
    #[ts(optional)]
    pub favourite: Option<bool>,
    #[ts(optional)]
    pub group_ids: Option<Vec<i64>>,
    #[ts(optional)]
    pub allow_disconnect: Option<bool>,
    /// `Some("")` goes back to the default from Settings.
    #[ts(optional)]
    pub launch_args: Option<String>,
    #[ts(optional)]
    pub discord_muted: Option<bool>,
    /// `Some(0)` goes back to the default from Settings; else 5..=360.
    #[ts(optional)]
    pub fps_cap: Option<u32>,
    #[ts(optional)]
    pub full_quality: Option<bool>,
    /// `Some(true)` also unmarks the previous MAIN.
    #[ts(optional)]
    pub main: Option<bool>,
    /// Replaces the account's own FFlags (`launcher::flags::check` rules); empty: none.
    #[ts(optional)]
    pub custom_flags: Option<BTreeMap<String, String>>,
    /// `Some(true)` forgets `client_limits`: its next client starts with the defaults.
    #[ts(optional)]
    pub clear_client_limits: Option<bool>,
    /// Replaces the account's own performance profile (`performance::per_client` of it is kept);
    /// an empty one removes it.
    #[ts(optional)]
    pub profile: Option<crate::performance::Profile>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Group {
    pub id: i64,
    pub name: String,
    pub color: Option<String>,
    pub sort_order: i64,
    #[serde(default)]
    pub defaults: GroupDefaults,
}

/// What a group gives its accounts where they set nothing themselves: the fields of an account's
/// Overrides tab, and the limits and anti-AFK of a new client. None: nothing from this group. An
/// account in several groups takes each from the first group, in the groups' order, that sets it
/// (`storage::with_group_defaults`).
#[derive(Serialize, Deserialize, TS, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct GroupDefaults {
    /// 5..=360.
    pub fps_cap: Option<u32>,
    pub full_quality: Option<bool>,
    pub launch_args: Option<String>,
    pub discord_muted: Option<bool>,
    pub allow_disconnect: Option<bool>,
    /// For an account that keeps none of its own (`AccountSummary::client_limits`).
    pub client: Option<ClientPrefs>,
}

/// Create (`id: None`) or rename / recolour a group.
#[derive(Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GroupInput {
    #[ts(optional)]
    pub id: Option<i64>,
    pub name: String,
    #[ts(optional)]
    pub color: Option<String>,
    /// None leaves the group's defaults as they are.
    #[ts(optional)]
    pub defaults: Option<GroupDefaults>,
}

/// Where launched clients' windows go (`instances::window::slot_rect`) once they reach a game.
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum WindowLayout {
    /// Where Roblox puts them (and "Tiny on launch", if on).
    #[default]
    Off,
    /// A grid as big as all of them fit, like the Dashboard's Tile.
    Fit,
    /// Tiny-sized cells, as many as fit, left to right.
    Tiny,
}

/// Where the accounts go in the window.
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum TabLayout {
    /// Accounts as tabs on Home, sections on the left.
    #[default]
    Top,
    /// Accounts listed on the left, sections as tabs along the top.
    Side,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Theme {
    /// `neon` and `bricks` were dark palettes in 0.5.0; their colours are now an accent.
    #[default]
    #[serde(alias = "neon", alias = "bricks")]
    Dark,
    #[serde(alias = "sakura")]
    Light,
    System,
    /// The user's own colours (`Settings::look`), on its base.
    Custom,
}

/// The theme a custom look is built on.
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum LookBase {
    #[default]
    Dark,
    Light,
}

/// The user's own colours: part or token id (the UI's list) → `#rrggbb`. An id left out keeps the
/// base's colour; the UI works out the shades in between and keeps text readable.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct Look {
    pub base: LookBase,
    pub colors: BTreeMap<String, String>,
}

/// When an animated background moves.
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum BackgroundMotion {
    /// While RoALT's window has the focus; a still frame otherwise.
    #[default]
    Focused,
    Always,
    Never,
}


#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Density {
    #[default]
    Detailed,
    Compact,
}

/// App settings, stored as one JSON value. New fields need `#[serde(default)]` semantics,
/// which the struct-level default gives.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct Settings {
    pub theme: Theme,
    pub tab_layout: TabLayout,
    /// `#rrggbb`, or "" for the theme's own (monochrome).
    pub accent: String,
    /// What `Theme::Custom` shows.
    pub look: Look,
    /// Corner roundness, % of the design's radii: one of `CORNERS`.
    pub corners: u8,
    /// How much of the theme's background covers a custom background image, 0-95 %.
    pub background_dim: u8,
    pub background_motion: BackgroundMotion,
    pub density: Density,
    /// Master-password mode only.
    pub auto_lock_minutes: u32,
    /// Master-password mode only: lock when Windows locks or goes to sleep (`session`), unless
    /// running clients hold the vault open (`vault_held_open`).
    pub lock_on_session_lock: bool,
    pub clipboard_clear_seconds: u32,
    /// How long account info is reused before Roblox is asked again.
    pub cache_minutes: u32,
    /// Hold Roblox's single-instance lock so several clients can run.
    pub multi_instance: bool,
    /// While multi-instance is on, lock Roblox's shared login files so clients can't
    /// overwrite each other's session (teleport error 773, wrong account shown).
    pub protect_shared_login: bool,
    /// Bulk launch: wait between two launches (±1 s jitter is added), 2 000..=60 000.
    pub launch_delay_ms: u32,
    /// Relaunch a client Roblox disconnects (kicked, server shut down, connection lost; not the
    /// player's own Leave or a sign-in elsewhere) into the same target, within the breaker.
    pub auto_rejoin: bool,
    /// Relaunch a client that ended without RoALT closing it or stopped responding (a window
    /// closed by hand looks the same, so it is opt-in), within the breaker.
    pub auto_restart_on_crash: bool,
    /// Crash breaker: at most this many rejoins and restarts of one account (1..=20) ...
    pub breaker_max: u32,
    /// ... within this many minutes (1..=240); then RoALT gives up on it.
    pub breaker_window_min: u32,
    /// How long a closed client gets to exit before it is terminated, 1..=60 s.
    pub close_grace_s: u32,
    /// Extra client arguments after the launch URI (`launcher::launch_args` rules); "" for none.
    pub launch_args: String,
    /// Closing the window hides it; the tray icon brings it back or quits.
    pub close_to_tray: bool,
    /// Open at full size, without the start-up animation (`splash`); from the next start.
    pub skip_splash: bool,
    /// Select-all (accounts list, launch picker) leaves the MAIN account out.
    pub protect_main: bool,
    /// Each client's own Windows audio session is muted once it reaches a game (`instances::audio`).
    pub mute_on_launch: bool,
    /// Launched clients go into this layout on RoALT's monitor; not Off: it replaces `tiny_on_launch`.
    pub window_layout: WindowLayout,
    /// Restart a client whose private memory stays over this many MB (3 samples, 10 s apart),
    /// within the breaker; 0: off, else 512..=65 536.
    pub mem_restart_mb: u32,
    /// Hold a client to a CPU cap (lowering its frame rate) while its private memory is over a limit.
    pub mem_throttle: MemThrottle,
    /// Trim each client's working set this often (minutes, 0: off) unless its own limits say.
    pub trim_every_min: u32,
    /// Give each client its own share of the CPU cores (round-robin) unless its limits say.
    pub auto_spread: bool,
    /// Divide the CPU among the clients: each gets a cap that shrinks as more run
    /// (`limits::fair_cap`), unless it has its own; separate from `adaptive`'s, which it tightens.
    pub fair_cap: bool,
    pub adaptive: Adaptive,
    /// Outbound limit for all Roblox clients together (a Windows QoS policy, set with admin
    /// rights by `bandwidth_apply`, which alone changes this); 0: none.
    pub bandwidth_kbps: u32,
    pub anti_afk: AntiAfk,
    /// Which events go to the Discord webhook (the webhook itself is in the vault).
    pub discord: DiscordEvents,
    /// Install a new Roblox version on its own once no client runs and no launch or batch does
    /// (never over a bootstrapper; the installer is signature-checked as always).
    pub auto_update: bool,
    /// While RoALT opens Roblox's links: the website's Play button fills in Play, where the
    /// account is picked, instead of starting the client as the browser's account.
    pub ask_browser_links: bool,
    /// Ask GitHub for a newer RoALT at start and every 6 hours (`self_update`); it is offered,
    /// never installed without a click.
    pub self_update: bool,
    /// Discord rich presence through the Discord app on this PC (`rpc`): "Playing RoALT" with
    /// the game and how many alts run.
    pub discord_presence: bool,
    /// What Discord rich presence shows (`rpc`): game name, alt count, elapsed timer, buttons,
    /// and optional custom lines.
    pub discord_rpc: DiscordRpc,
    /// Ping RoALT's online counter every few minutes (it says how often) with a random id made once per launch, and
    /// show how many people use RoALT beside the Discord button (`online`). Off: nothing is sent.
    pub online_count: bool,
    /// Restart each client in a game once it has run this many hours (0: off, else 1..=168): long
    /// runs leak memory. Never the clients of Full quality accounts or the one in front.
    pub restart_every_h: u32,
    /// Close each client once its account's run has lasted this many hours (0: off, else 1..=168);
    /// a rejoin or restart goes on with the same run. Same exceptions.
    pub close_after_h: u32,
    /// Home's News tab: RoALT's releases on GitHub, the Roblox DevForum and Roblox's newsroom, every
    /// 30 minutes while shown (`news`). Off: none of them is asked.
    pub news: bool,
    /// Show the sponsor's card in Home's News (`extras`, read from roalt.dev). Off: roalt.dev isn't
    /// asked for it (Settings › About still reads the supporters' names).
    pub show_sponsor: bool,
    /// Check every account's sign-in with Roblox by itself, 90 s after start and every 6 hours
    /// (`watch`), so a dead one is known before a launch finds it. Off: only when asked.
    pub sign_in_watch: bool,
    /// While counting an account's limiteds, download Rolimons' public item list (`rolimons`;
    /// api.rolimons.com, at most every 30 minutes) and add its value, demand and trend. No user id,
    /// name or cookie is sent. Off by default: Rolimons isn't Roblox.
    pub rolimons_values: bool,
    /// Hide each client (without taking the focus) once, when it first reaches a game, unless its
    /// account is full quality. A hidden client (`instances::window`) stays live while Home shows
    /// it and sleeps otherwise, and comes back from the taskbar or Alt-Tab.
    pub minimize_on_launch: bool,
    /// System-wide key combination for Kill all, like "Ctrl+Shift+F12"; "" for none.
    pub panic_hotkey: String,
    /// System-wide keys (`hotkeys`), "" for none: show RoALT or hide it to the tray; tile the
    /// clients' windows; minimize them all.
    pub toggle_hotkey: String,
    pub tile_hotkey: String,
    pub minimize_hotkey: String,
    /// Delete old or huge Roblox client logs at start and every hour (`logwatch::clean`).
    pub clean_logs: bool,
    /// FPS cap given to each client a launch starts (Roblox's own Maximum Frame Rate,
    /// `launcher::client_settings`), unless the account has its own; 0: leave Roblox alone, else 5..=360.
    pub fps_cap: u32,
    /// Start clients at Roblox's lowest graphics quality (`launcher::client_settings`).
    pub low_graphics: bool,
    /// Start clients with Roblox's allowed lightweight rendering flags (`launcher::flags`): low
    /// geometry detail and what `render_flags` picks.
    pub lighter_rendering: bool,
    pub render_flags: RenderFlags,
    /// The user's own FFlags (`launcher::flags`), written over `lighter_rendering`'s at each
    /// launch while `custom_flags_scope` is Global; Roblox ignores those off its allowlist.
    pub custom_flags: BTreeMap<String, String>,
    /// Whose FFlags a launch writes: `custom_flags`, or the account's own.
    pub custom_flags_scope: FlagScope,
    /// Browse refuses anything that spends Robux (free items still work) and the Robux, Premium and
    /// billing pages (`browse`). On by default.
    pub lock_purchases: bool,
    /// Width and height "Make it tiny" and "Tile tiny windows" use.
    pub micro_width: u32,
    pub micro_height: u32,
    /// Linux/Cordial: render resolution for headless clients. Cordial reads it at launch.
    pub headless_width: u32,
    pub headless_height: u32,
    /// Each new client goes tiny as soon as it has a window, unless its account is Full quality.
    pub tiny_on_launch: bool,
    /// Linux/Cordial: start the client under `cage` with no game window (`cordial-run --headless`).
    /// It keeps running in the background; close it from RoALT. Needs `cage` installed.
    pub headless: bool,
    /// Linux/Cordial: closing the game window leaves the client running in the background
    /// (`CORDIAL_NO_CLOSE_EXIT=1`). Close it from RoALT when done.
    pub keep_running_on_close: bool,
    /// Launch on a Public network too (`network`); off: refused. On by default.
    pub allow_public_networks: bool,
    /// UI and message language (`i18n::LANGS`), or "" to follow Windows' display language.
    pub language: String,
    /// Launches start clients with the mods that are on (`mods`). On by default; nothing is
    /// applied until a mod is imported.
    pub mods_on: bool,
    /// The user has read what importing someone else's mod means (asked before the first import).
    pub mods_ack: bool,
    /// The first-run setup was finished or skipped (the UI shows it while this is off and there
    /// are no accounts).
    pub onboarded: bool,
}

/// The network RoALT runs on (`network`). Never its name: streamers show RoALT on screen.
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NetworkStatus {
    /// A connected Public network with internet access.
    pub public: bool,
}

/// The memory throttle: a client whose private memory is over `mb` at a 10 s sample gets at
/// most `cpu_cap` percent of the CPU (its frame rate drops with it) until it is back under 90 %
/// of `mb`. Tightens a client's own cap, never loosens it; the window in front too.
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct MemThrottle {
    /// 0: off, else 512..=65 536.
    pub mb: u32,
    /// 1..=100.
    pub cpu_cap: u8,
}

impl Default for MemThrottle {
    fn default() -> Self {
        Self { mb: 0, cpu_cap: 10 }
    }
}

/// Anti-aliasing samples; `Default` leaves it to Roblox.
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum Msaa {
    Default,
    #[default]
    X1,
    X2,
    X4,
}

/// Roblox's texture detail tiers; `Default` leaves it to Roblox.
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum TextureQuality {
    Default,
    #[default]
    Lowest,
    Low,
    Medium,
    High,
}

/// The graphics backend to ask for. Roblox can ignore it: client 0.745 stayed on Direct3D 11 when
/// asked for Vulkan or OpenGL.
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RenderMode {
    #[default]
    Default,
    D3d11,
    Vulkan,
    Opengl,
}

/// What `lighter_rendering` writes (`launcher::flags`). The default is what it has always done.
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct RenderFlags {
    pub msaa: Msaa,
    pub texture: TextureQuality,
    pub mode: RenderMode,
    pub gray_sky: bool,
    pub no_grass: bool,
    pub lowest_frm: bool,
    pub pause_voxelizer: bool,
}

impl Default for RenderFlags {
    fn default() -> Self {
        Self { msaa: Msaa::X1, texture: TextureQuality::Lowest, mode: RenderMode::Default, gray_sky: false, no_grass: true, lowest_frm: true, pause_voxelizer: false }
    }
}

/// Where custom FFlags come from (`Settings.custom_flags_scope`).
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FlagScope {
    /// `Settings.custom_flags`, for every account.
    #[default]
    Global,
    /// Each account's own `custom_flags`.
    Account,
}

/// Anti-AFK: every `interval_min` (1..=19) minutes each client in a game that hasn't opted out
/// gets a key press (`instances::afk`).
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct AntiAfk {
    pub enabled: bool,
    pub interval_min: u32,
}

impl Default for AntiAfk {
    fn default() -> Self {
        Self { enabled: false, interval_min: 15 }
    }
}


/// What Discord rich presence shows on the user's profile (`rpc`).
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct DiscordRpc {
    /// Show the current game's title (off: generic "Playing Roblox").
    pub show_game: bool,
    /// Show how many alts are running.
    pub show_count: bool,
    /// Show the elapsed timer.
    pub show_elapsed: bool,
    /// Show the "Get RoALT" and "Join Discord" buttons.
    pub show_buttons: bool,
    /// Optional custom top line (`details`, up to 128 chars; supports `{game}` and `{count}`).
    pub custom_details: String,
    /// Optional custom bottom line (`state`, up to 128 chars; supports `{game}` and `{count}`).
    pub custom_state: String,
}

impl Default for DiscordRpc {
    fn default() -> Self {
        Self {
            show_game: true,
            show_count: true,
            show_elapsed: true,
            show_buttons: true,
            custom_details: String::new(),
            custom_state: String::new(),
        }
    }
}

/// Which Discord alerts mention `DiscordEvents.ping`.
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PingLevel {
    /// Only urgent problems (crashes, gave up, breaker, anti-AFK blocked).
    #[default]
    Urgent,
    /// Also warnings (disconnects, hangs, sign-in trouble, PC alerts, batch failures).
    Warnings,
}

/// Events sent to the Discord webhook (`discord`), each on or off.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct DiscordEvents {
    pub crash: bool,
    pub hang: bool,
    pub mem_restart: bool,
    pub disconnect: bool,
    /// Auto-rejoins and auto-restarts that went ahead.
    pub restart: bool,
    /// The crash breaker gave up on an account.
    pub breaker: bool,
    pub batch_done: bool,
    /// A client first got into a game (not after a rejoin, restart or reconnect).
    pub launch: bool,
    /// An account's saved sign-in stopped working, or Roblox refused a client's sign-in, wants a
    /// human check, or the client is signed in as another account.
    pub sign_in: bool,
    /// A client is held to the memory throttle's CPU cap.
    pub throttle: bool,
    /// A newer RoALT is out.
    pub update: bool,
    /// With clients running: the PC's memory stayed at 90 % or more for a minute, or the clients'
    /// GPU use at 95 % or more for five (`discord::Sustained`).
    pub pc: bool,
    /// A summary of the running clients every this many minutes (0: off, else 5..=1440).
    pub status_every_min: u32,
    /// Who bad alerts mention, as `<@id>` (a user) or `<@&id>` (a role); "" for no one.
    pub ping: String,
    /// Which severities fire `ping`: only urgent ones (default), or warnings too.
    pub ping_level: PingLevel,
}

impl Default for DiscordEvents {
    fn default() -> Self {
        Self {
            crash: true,
            hang: true,
            mem_restart: true,
            disconnect: true,
            restart: true,
            breaker: true,
            batch_done: true,
            launch: false,
            sign_in: true,
            throttle: false,
            update: true,
            pc: false,
            status_every_min: 0,
            ping: String::new(),
            ping_level: PingLevel::default(),
        }
    }
}

/// The Discord webhook as the UI may see it: never the URL or its token.
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DiscordStatus {
    pub configured: bool,
    /// The webhook id's last 4 digits, e.g. "…4821".
    pub webhook: Option<String>,
}

/// The adaptive resource manager: clients whose window isn't in front get `background`;
/// a client with its own limits keeps them.
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct Adaptive {
    pub enabled: bool,
    pub background: Limits,
}

impl Default for Adaptive {
    fn default() -> Self {
        Self { enabled: false, background: Limits { cpu_cap: Some(25), eco: true, ..Limits::default() } }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::Dark,
            tab_layout: TabLayout::Top,
            accent: String::new(),
            look: Look::default(),
            corners: 100,
            background_dim: 60,
            background_motion: BackgroundMotion::Focused,
            density: Density::Detailed,
            auto_lock_minutes: 15,
            lock_on_session_lock: true,
            clipboard_clear_seconds: 30,
            cache_minutes: 10,
            multi_instance: true,
            protect_shared_login: true,
            launch_delay_ms: 6_000,
            auto_rejoin: false,
            auto_restart_on_crash: false,
            breaker_max: 3,
            breaker_window_min: 10,
            close_grace_s: 5,
            launch_args: String::new(),
            close_to_tray: false,
            skip_splash: false,
            protect_main: true,
            mute_on_launch: false,
            window_layout: WindowLayout::Off,
            mem_restart_mb: 0,
            mem_throttle: MemThrottle::default(),
            trim_every_min: 0,
            auto_spread: false,
            fair_cap: false,
            adaptive: Adaptive::default(),
            bandwidth_kbps: 0,
            anti_afk: AntiAfk::default(),
            discord: DiscordEvents::default(),
            auto_update: false,
            ask_browser_links: false,
            self_update: true,
            discord_presence: true,
            discord_rpc: DiscordRpc::default(),
            online_count: true,
            restart_every_h: 0,
            close_after_h: 0,
            news: true,
            show_sponsor: true,
            sign_in_watch: true,
            rolimons_values: false,
            minimize_on_launch: false,
panic_hotkey: String::new(),
toggle_hotkey: String::new(),
tile_hotkey: String::new(),
minimize_hotkey: String::new(),
            clean_logs: false,
            fps_cap: 0,
            low_graphics: true,
            lighter_rendering: true,
            render_flags: RenderFlags::default(),
            custom_flags: BTreeMap::new(),
            custom_flags_scope: FlagScope::Global,
            lock_purchases: true,
            micro_width: 320,
            micro_height: 240,
            headless_width: 640,
            headless_height: 360,
            tiny_on_launch: false,
            headless: false,
            keep_running_on_close: false,
            allow_public_networks: true,
            language: String::new(),
            mods_on: true,
            mods_ack: false,
            onboarded: false,
        }
    }
}

#[derive(Deserialize, TS, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SettingsPatch {
    #[ts(optional)]
    pub theme: Option<Theme>,
    #[ts(optional)]
    pub tab_layout: Option<TabLayout>,
    #[ts(optional)]
    pub accent: Option<String>,
    #[ts(optional)]
    pub look: Option<Look>,
    #[ts(optional)]
    pub corners: Option<u8>,
    #[ts(optional)]
    pub background_dim: Option<u8>,
    #[ts(optional)]
    pub background_motion: Option<BackgroundMotion>,
    #[ts(optional)]
    pub density: Option<Density>,
    #[ts(optional)]
    pub auto_lock_minutes: Option<u32>,
    #[ts(optional)]
    pub lock_on_session_lock: Option<bool>,
    #[ts(optional)]
    pub clipboard_clear_seconds: Option<u32>,
    #[ts(optional)]
    pub cache_minutes: Option<u32>,
    #[ts(optional)]
    pub protect_shared_login: Option<bool>,
    #[ts(optional)]
    pub launch_delay_ms: Option<u32>,
    #[ts(optional)]
    pub auto_rejoin: Option<bool>,
    #[ts(optional)]
    pub auto_restart_on_crash: Option<bool>,
    #[ts(optional)]
    pub breaker_max: Option<u32>,
    #[ts(optional)]
    pub breaker_window_min: Option<u32>,
    #[ts(optional)]
    pub close_grace_s: Option<u32>,
    #[ts(optional)]
    pub launch_args: Option<String>,
    #[ts(optional)]
    pub close_to_tray: Option<bool>,
    #[ts(optional)]
    pub skip_splash: Option<bool>,
    #[ts(optional)]
    pub protect_main: Option<bool>,
    #[ts(optional)]
    pub mute_on_launch: Option<bool>,
    #[ts(optional)]
    pub window_layout: Option<WindowLayout>,
    #[ts(optional)]
    pub mem_restart_mb: Option<u32>,
    #[ts(optional)]
    pub mem_throttle: Option<MemThrottle>,
    #[ts(optional)]
    pub trim_every_min: Option<u32>,
    #[ts(optional)]
    pub auto_spread: Option<bool>,
    #[ts(optional)]
    pub fair_cap: Option<bool>,
    #[ts(optional)]
    pub adaptive: Option<Adaptive>,
    #[ts(optional)]
    pub anti_afk: Option<AntiAfk>,
    #[ts(optional)]
    pub discord: Option<DiscordEvents>,
    #[ts(optional)]
    pub auto_update: Option<bool>,
    #[ts(optional)]
    pub ask_browser_links: Option<bool>,
    #[ts(optional)]
    pub self_update: Option<bool>,
    #[ts(optional)]
    pub discord_presence: Option<bool>,
    #[ts(optional)]
    pub discord_rpc: Option<DiscordRpc>,
    #[ts(optional)]
    pub online_count: Option<bool>,
    #[ts(optional)]
    pub restart_every_h: Option<u32>,
    #[ts(optional)]
    pub close_after_h: Option<u32>,
    #[ts(optional)]
    pub news: Option<bool>,
    #[ts(optional)]
    pub show_sponsor: Option<bool>,
    #[ts(optional)]
    pub sign_in_watch: Option<bool>,
    #[ts(optional)]
    pub rolimons_values: Option<bool>,
    #[ts(optional)]
    pub minimize_on_launch: Option<bool>,
    #[ts(optional)]
    pub panic_hotkey: Option<String>,
    #[ts(optional)]
    pub toggle_hotkey: Option<String>,
    #[ts(optional)]
    pub tile_hotkey: Option<String>,
    #[ts(optional)]
    pub minimize_hotkey: Option<String>,
    #[ts(optional)]
    pub clean_logs: Option<bool>,
    #[ts(optional)]
    pub fps_cap: Option<u32>,
    #[ts(optional)]
    pub low_graphics: Option<bool>,
    #[ts(optional)]
    pub lighter_rendering: Option<bool>,
    #[ts(optional)]
    pub render_flags: Option<RenderFlags>,
    #[ts(optional)]
    pub custom_flags: Option<BTreeMap<String, String>>,
    #[ts(optional)]
    pub custom_flags_scope: Option<FlagScope>,
    #[ts(optional)]
    pub lock_purchases: Option<bool>,
    #[ts(optional)]
    pub micro_width: Option<u32>,
    #[ts(optional)]
    pub micro_height: Option<u32>,
    #[ts(optional)]
    pub headless_width: Option<u32>,
    #[ts(optional)]
    pub headless_height: Option<u32>,
    #[ts(optional)]
    pub tiny_on_launch: Option<bool>,
    #[ts(optional)]
    pub headless: Option<bool>,
    #[ts(optional)]
    pub keep_running_on_close: Option<bool>,
    #[ts(optional)]
    pub allow_public_networks: Option<bool>,
    #[ts(optional)]
    pub language: Option<String>,
    #[ts(optional)]
    pub mods_on: Option<bool>,
    #[ts(optional)]
    pub mods_ack: Option<bool>,
    #[ts(optional)]
    pub onboarded: Option<bool>,
}

/// What a Roblox log clean-up deleted (`logs_clean`).
#[derive(Serialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LogsCleaned {
    pub files: u32,
    #[ts(type = "number")]
    pub bytes: u64,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Subscription {
    None,
    /// Legacy Premium (no longer sold since 2026-04-30).
    Premium,
    /// Roblox Plus.
    Plus,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum PresenceKind {
    Offline,
    Online,
    InGame,
    Studio,
    Unknown,
}

impl PresenceKind {
    /// presence.roblox.com `userPresenceType`.
    pub fn from_code(code: i64) -> Self {
        match code {
            0 => Self::Offline,
            1 => Self::Online,
            2 => Self::InGame,
            3 => Self::Studio,
            other => {
                log::debug!("unrecognised presence code {other}");
                Self::Unknown
            }
        }
    }
}

/// What Roblox shows about where a user is. Game fields are `None` when the viewer
/// is not allowed to see them.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Presence {
    pub kind: PresenceKind,
    /// Game name when in game, else e.g. "Website".
    pub last_location: Option<String>,
    pub place_id: Option<i64>,
    pub root_place_id: Option<i64>,
    pub universe_id: Option<i64>,
    /// The server (job) id.
    pub game_id: Option<String>,
}

#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AccountPresence {
    pub account_id: i64,
    pub presence: Presence,
}

#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Headshot {
    pub account_id: i64,
    pub url: String,
}

/// The account info panel. Each part is `None` when Roblox did not answer for it;
/// `unavailable` names those parts for the UI.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AccountInfo {
    pub account_id: i64,
    pub user_id: i64,
    pub username: String,
    pub display_name: String,
    pub description: String,
    /// ISO 8601 account creation time.
    pub created: Option<String>,
    pub is_banned: bool,
    pub verified_badge: bool,
    pub headshot_url: Option<String>,
    pub robux: Option<i64>,
    pub subscription: Option<Subscription>,
    pub friends: Option<i64>,
    pub followers: Option<i64>,
    pub following: Option<i64>,
    pub email_verified: Option<bool>,
    /// Enabled 2-step methods ("Email", "Authenticator", ...); empty = 2SV off.
    pub two_step_methods: Option<Vec<String>>,
    /// The account seen with its own cookie.
    pub presence: Option<Presence>,
    /// Unix ms.
    pub fetched_at: i64,
    pub unavailable: Vec<String>,
}

#[derive(Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum ConflictPolicy {
    /// Keep the account already stored.
    Skip,
    /// Overwrite cookie and details with the backup's.
    Replace,
}

#[derive(Serialize, TS, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ImportReport {
    pub added: u32,
    pub updated: u32,
    pub skipped: u32,
    /// Cookies Roblox refused, or entries without one.
    pub failed: u32,
    /// A backup's settings were asked for and it had some: whether they were applied.
    pub settings: Option<bool>,
    /// Secret-free description of each failed entry (e.g. `"Cookie #3: ..."` or `"user: ..."`).
    #[serde(default)]
    pub failures: Vec<String>,
}

/// What "Export cookies" wrote (`cookie_export`).
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CookieExport {
    /// The file, selected in Explorer.
    pub path: String,
    pub count: u32,
    /// Accounts whose cookie couldn't be read.
    pub skipped: u32,
    /// Unix ms when RoALT deletes it.
    pub deletes_at: i64,
}

/// What "make friends" did, counted in pairs of accounts.
#[derive(Serialize, TS, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FriendReport {
    pub added: u32,
    pub already: u32,
    pub failed: u32,
    /// Accounts Roblox asked to solve a captcha (on roblox.com) before sending requests.
    pub challenged: Vec<i64>,
}

/// Where to launch. Ids are numbers; codes stay strings (link codes can be 25-32 digits).
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum LaunchTarget {
    /// A game (its start place); `launch_data` ≤ 200 bytes goes to the game.
    Place { place_id: i64, launch_data: Option<String> },
    /// One server (job) of a place.
    Server { place_id: i64, job_id: String },
    /// Private server via its link code (classic `?privateServerLinkCode=` links).
    PrivateLink { place_id: i64, link_code: String },
    /// Private server via its access code (a GUID).
    PrivateAccess { place_id: i64, access_code: String },
    /// `roblox.com/share?code=..&type=..`: resolved with the launching account's cookie.
    ShareLink { code: String, link_type: String },
    /// `roblox.com/share?code=..&v=v2`.
    ShareLinkV2 { code: String },
    /// A game named by universe; launched at its root place.
    Universe { universe_id: i64 },
    /// Join whatever server this user is in.
    FollowUser { user_id: i64 },
}

/// What `target_parse` made of pasted text.
#[derive(Serialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum ParsedInput {
    Target { target: LaunchTarget },
    /// A bare job id: the UI must ask for the place.
    JobIdOnly { job_id: String },
    /// `@name` or a bare username: look it up, then follow.
    Username { name: String },
    Invalid { reason: String },
    Unsupported { reason: String },
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Favourite {
    pub id: i64,
    pub name: String,
    pub target: LaunchTarget,
    /// Accounts a one-click (or tray) launch uses.
    pub account_ids: Vec<i64>,
    pub in_tray: bool,
    pub sort_order: i64,
    pub options: FavouriteOptions,
}

/// How a favourite launches besides where: what a one-click, tray or hotkey launch uses, and what
/// picking it in Play sets.
#[derive(Serialize, Deserialize, TS, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct FavouriteOptions {
    /// How several accounts share or spread over servers; None: as a favourite always did (the
    /// first account picks the server from the tray, each on its own from Home).
    pub same_server: Option<SameServer>,
    /// The FPS cap of its launches (5..=360); None: each account's own, else the default.
    pub fps_cap: Option<u32>,
    /// Groups whose accounts launch with it too, besides `account_ids`: whoever is in them then.
    pub group_ids: Vec<i64>,
    /// A system-wide key combination that launches it, like "Ctrl+Alt+1"; "" for none.
    pub hotkey: String,
}

#[derive(Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FavouriteInput {
    #[ts(optional)]
    pub id: Option<i64>,
    pub name: String,
    pub target: LaunchTarget,
    pub account_ids: Vec<i64>,
    pub in_tray: bool,
    /// Left out: an edit keeps what the favourite has, a new one gets the defaults.
    #[ts(optional)]
    pub options: Option<FavouriteOptions>,
}

/// Someone an account can join: a friend online on Roblox, or another stored account.
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct JoinCandidate {
    pub user_id: i64,
    pub username: String,
    pub display_name: String,
    /// Set when this is one of the stored accounts.
    pub account_id: Option<i64>,
    pub presence: Presence,
}

#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UserRef {
    pub user_id: i64,
    pub username: String,
    pub display_name: String,
}

/// A client RoALT started.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LaunchInfo {
    pub account_id: i64,
    /// Process id of what was started (the Roblox client, or a bootstrapper such as Bloxstrap).
    pub pid: u32,
    /// The `browsertrackerid` this launch used; the client's log names it.
    pub browser_tracker_id: i64,
    /// The place it was sent to, when known before the client starts.
    pub place_id: Option<i64>,
    /// Unix ms.
    pub started_at: i64,
    /// True when a bootstrapper (not RobloxPlayerBeta.exe) handled the launch.
    pub via_bootstrapper: bool,
    /// Something the user should know (expired share link, launch data left out).
    pub warning: Option<String>,
    /// This launch's own FPS cap, used instead of the account's and the default; None: those.
    pub fps_cap: Option<u32>,
    /// Start time of `pid` (Unix s, 0: already gone), so a reused PID is never taken for it.
    #[serde(skip)]
    #[ts(skip)]
    pub pid_start: Option<u64>,
}

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum InstanceState {
    /// Process started, its log not found yet.
    Starting,
    /// Log found; joining a server.
    Joining,
    InGame,
    /// Lost the server (kick, shutdown, network); the client may still be open.
    Disconnected,
    Exited,
}

/// A Roblox client RoALT started and is following.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Instance {
    pub id: String,
    pub account_id: i64,
    pub pid: u32,
    pub state: InstanceState,
    pub target: LaunchTarget,
    pub place_id: Option<i64>,
    pub job_id: Option<String>,
    /// Unix ms.
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub via_bootstrapper: bool,
    /// Why it ended or what went wrong: "Closed", "Replaced by another Roblox client",
    /// "Signed in as another account", "Roblox wants a human check", ...
    pub note: Option<String>,
    /// How many auto-rejoins led to this client (0: launched by the user).
    pub rejoins: u32,
    pub limits: Limits,
    /// Anti-AFK presses keys in it (when on in Settings); false: opted out.
    pub anti_afk: bool,
    /// Unix ms of its last anti-AFK key press.
    pub afk_at: Option<i64>,
    /// Held to `Settings.mem_throttle`'s CPU cap: its memory is over that limit.
    pub throttled: bool,
    /// Its window was made small by RoALT (`instance_micro`, `instances_tile`); "Normal size"
    /// puts it back.
    pub micro: bool,
    /// The client size it was made small at, when not the Settings tiny size (a window-layout cell):
    /// the tiny sweep keeps it at that, also after RoALT restarts.
    #[serde(default)]
    #[ts(skip)]
    pub micro_size: Option<(u32, u32)>,
    /// The FPS cap its launch asked for (`LaunchInfo::fps_cap`); a rejoin uses it again.
    pub fps_cap: Option<u32>,
    /// RoALT is working on relaunching it (`instances::rejoin`). Not kept over a restart of RoALT.
    #[serde(default, skip_deserializing)]
    pub relaunching: bool,
    /// Why that relaunch has to wait, while it does.
    #[serde(default, skip_deserializing)]
    pub relaunch_problem: Option<String>,
    /// Its note is "Signed in as another account", "Roblox wants a human check" or "Roblox refused
    /// the sign-in (error 403)" (the note itself is translated, so the UI can't match its text).
    #[serde(default)]
    pub sign_in_problem: bool,
    /// What it really has now, settings' limits included (`limits` is only what was set on it).
    #[serde(default, skip_deserializing)]
    pub applied_limits: Limits,
    /// The user took control of it ("Take control"): until it closes or is given back, RoALT doesn't
    /// rejoin, restart, close, press anti-AFK keys in, limit or lay it out.
    #[serde(default)]
    pub held: bool,
}

/// What Windows holds one client to (instances::limits). Lasts as long as the client.
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct Limits {
    /// Hard cap on its CPU time, in percent of the whole machine (1-100); None: no cap.
    pub cpu_cap: Option<u8>,
    /// Efficiency mode: EcoQoS and below-normal priority, as Task Manager's.
    pub eco: bool,
    /// Trim its working set every this many minutes (0: never); None: `Settings.trim_every_min`.
    pub trim_every_min: Option<u32>,
    /// The CPU cores it may run on, one bit each (the first 53 cores: a JS number); None: all
    /// (or its share with `Settings.auto_spread`).
    #[ts(type = "number | null")]
    pub affinity: Option<u64>,
}

/// How busy the whole PC is (`system_usage`), sampled at most every 2 s.
#[derive(Serialize, TS, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SystemUsage {
    /// Of all cores; None on the first sample, which has nothing to compare with.
    pub cpu_pct: Option<f64>,
    pub mem_used_mb: f64,
    pub mem_total_mb: f64,
}

/// What one running client uses (`instances_metrics`), sampled at most every 2 s.
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ClientMetrics {
    pub instance_id: String,
    pub pid: u32,
    /// Percent of the whole machine since the previous sample (None on the first one).
    pub cpu_pct: Option<f64>,
    /// Private working set (Task Manager's Memory column, what a trim lowers), MB.
    pub memory_mb: f64,
    /// Private bytes (Task Manager's Commit size, what a leak grows and the memory limits watch), MB.
    pub private_mb: f64,
    /// Disk and network reads and writes, bytes per second since the previous sample.
    pub read_bps: Option<f64>,
    pub write_bps: Option<f64>,
    /// Use of its busiest GPU engine (Task Manager's GPU column), percent (None when Windows can't say).
    pub gpu_pct: Option<f64>,
    /// Dedicated GPU memory, MB (None when Windows can't say).
    pub gpu_mem_mb: Option<f64>,
}

/// The Roblox bandwidth limit (`bandwidth_status`).
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BandwidthStatus {
    /// The setting: what was last applied (0: none).
    pub kbps: u32,
    /// The policy RoALT set is in force: applied since Windows last started.
    pub active: bool,
}

#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MultiInstanceStatus {
    /// The setting.
    pub enabled: bool,
    /// The lock is actually held.
    pub active: bool,
    pub protect_shared_login: bool,
    /// PIDs of Roblox processes that stop the lock from being taken.
    pub blocked_by: Vec<u32>,
    pub message: Option<String>,
}

// ---------- M5: bulk launch ----------

/// How a bulk launch places its accounts: all in one server, or each in a different one
/// (public-game targets only; targets that already name one server ignore it).
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SameServer {
    /// Each account is matched by Roblox on its own.
    #[default]
    Off,
    /// Choose one server with room for everyone from the server list, then send each account
    /// to it (`RequestGameJob`).
    PickFirst,
    /// Launch the first account normally, read its server from its client log (presence as a
    /// fallback), send the rest there.
    FollowFirst,
    /// Each account to a private server it owns of the game; one without goes `RandomPublic`.
    OwnPrivate,
    /// Each account to a different public server with room, picked at random from the list.
    RandomPublic,
    /// Each account to a different public server with room, the closest first (`BestLatency`).
    Closest,
    /// Each account to a different public server with room, the most players first.
    MostFull,
    /// Each account to a different public server with room, the fewest players first.
    MostEmpty,
}

impl SameServer {
    /// Each account in a different server.
    pub fn spreads(self) -> bool {
        matches!(self, Self::OwnPrivate | Self::RandomPublic | Self::Closest | Self::MostFull | Self::MostEmpty)
    }
}

#[derive(Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BulkRequest {
    /// Launch order.
    pub account_ids: Vec<i64>,
    pub target: LaunchTarget,
    /// Wait between launches; clamped to 2 000..=60 000, ±1 s jitter is added.
    pub delay_ms: u32,
    pub same_server: SameServer,
    /// FPS cap for each of these launches, instead of each account's and the default; None:
    /// those.
    #[serde(default)]
    pub fps_cap: Option<u32>,
}

/// One account's progress. After `Starting` it mirrors its client (`Instance`).
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub enum BulkStage {
    Queued,
    /// Resolving the target / getting the ticket.
    Ticket,
    /// The client was started; its log isn't bound yet.
    Starting,
    Joining,
    InGame,
    /// Disconnected, and its account allows that (`allow_disconnect`).
    Done,
    Failed { message: String },
    Cancelled,
}

#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BulkItem {
    pub account_id: i64,
    /// The Roblox user the account was when the batch started (account ids are reused).
    pub user_id: i64,
    pub stage: BulkStage,
    /// The tracked client, once started.
    pub instance_id: Option<String>,
}

/// The whole batch; `bulk://progress` carries it on every change.
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BulkProgress {
    pub batch_id: String,
    pub target: LaunchTarget,
    pub same_server: SameServer,
    pub delay_ms: u32,
    /// The server everyone is sent to, once known.
    pub job_id: Option<String>,
    pub items: Vec<BulkItem>,
    /// Why the batch stopped early ("Cancelled", a challenge, a wrong account); None while it
    /// runs and after a normal finish.
    pub stopped: Option<String>,
    /// No more launches or stage changes will come.
    pub done: bool,
    /// Something the user should know (no server had room, the first account's server
    /// couldn't be read, ...).
    pub note: Option<String>,
}

// ---------- M5/M6: servers ----------

#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ServerOrder {
    OccupancyDesc,
    OccupancyAsc,
    /// Needs a cookie.
    Recommended,
    /// Needs a cookie.
    BestLatency,
}

/// One public server. Roblox may leave anything but id / maxPlayers / playing out.
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GameServer {
    /// The job id (a GUID).
    pub id: String,
    pub playing: i32,
    pub max_players: i32,
    pub fps: Option<f64>,
    pub ping: Option<i64>,
    pub friend_count: Option<i32>,
}

#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ServerPage {
    pub servers: Vec<GameServer>,
    /// Opaque; None on the last page.
    pub next_cursor: Option<String>,
}

// ---------- M6: server browser ----------

/// What the Servers view shows about a game.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GameInfo {
    /// The place that was asked about (may differ from the root place).
    pub place_id: i64,
    pub universe_id: i64,
    pub root_place_id: i64,
    pub name: String,
    pub creator: String,
    pub playing: i64,
    pub visits: i64,
    pub max_players: i32,
    /// 150×150 icon on *.rbxcdn.com, when Roblox has one.
    pub icon_url: Option<String>,
    /// The game sells private servers.
    pub private_servers: bool,
}

/// One private server of a place, as one account sees it.
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PrivateServer {
    pub vip_server_id: i64,
    pub name: String,
    pub owner_id: Option<i64>,
    pub owner_name: Option<String>,
    pub playing: i32,
    pub max_players: i32,
    /// The running server's job id, when it runs.
    pub job_id: Option<String>,
    /// Only when this account can join it (launch with `LaunchTarget::PrivateAccess`).
    pub access_code: Option<String>,
}

#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PrivateServerPage {
    pub servers: Vec<PrivateServer>,
    /// Opaque; None on the last page.
    pub next_cursor: Option<String>,
    /// Roblox says this account may not join servers of this game (`gameJoinRestricted`).
    pub join_restricted: bool,
}

// ---------- M7 ----------

/// The Roblox client RoALT would start.
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RobloxClient {
    /// None when Roblox isn't installed.
    pub path: Option<String>,
    /// A bootstrapper (e.g. Bloxstrap) handles launches; it keeps Roblox up to date itself, so
    /// RoALT never updates or installs over it.
    pub bootstrapper: bool,
    /// The installed version: the `Versions\version-…` folder the client is in. None when not
    /// installed, for a bootstrapper, or a client outside such a folder.
    pub version: Option<String>,
    /// Roblox's live version (`clientVersionUpload`), once checked (at start, every 30 min).
    pub latest: Option<String>,
    /// Both versions are known and differ (`launcher::update::outdated`).
    pub outdated: bool,
}

/// What a default Roblox launcher is (`launcher::links`).
#[derive(Serialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
#[allow(dead_code)]
pub enum LauncherKind {
    Roblox,
    /// Bloxstrap, Fishstrap and the like: RoALT launches through it.
    Bootstrapper,
    /// RoALT opens Roblox's links itself.
    Roalt,
}

/// A program that can open Roblox's links (`roblox-player:` and `roblox:`).
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RobloxLauncher {
    pub kind: LauncherKind,
    /// "Roblox", "RoALT", or the bootstrapper's file name ("Bloxstrap").
    pub name: String,
    pub path: String,
    /// It opens the links now.
    pub current: bool,
}

/// RoALT's own version and a newer published release, once found (`self_update`).
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AppUpdate {
    pub current: String,
    /// The newer version (`x.y.z`); None when up to date or not checked yet.
    pub available: Option<String>,
}

/// Where a News item comes from (`news`). Roblox client updates are the UI's own, from `roblox_client`.
#[derive(Serialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum NewsSource {
    Roalt,
    Devforum,
    Newsroom,
}

/// One item of Home's News tab.
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NewsItem {
    /// The same on every fetch: what "read" is kept by.
    pub id: String,
    pub source: NewsSource,
    pub title: String,
    /// Plain text; a RoALT release's notes as Markdown.
    pub summary: String,
    /// Rebuilt from ids and slugs on the source's own host, never taken as given.
    pub url: String,
    /// RFC 3339, or `YYYY-MM` where the source names only the month (the newsroom).
    pub published: String,
    /// A RoALT release's version (`x.y.z`).
    pub version: Option<String>,
}

/// Newest first; a source that couldn't be read is left out and named in `failed`.
#[derive(Serialize, TS, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct News {
    pub items: Vec<NewsItem>,
    pub failed: Vec<NewsSource>,
}

/// Who sponsors RoALT now (`extras`): plain text. Its link is RoALT's own, never taken from here.
#[derive(Serialize, TS, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Sponsor {
    pub name: String,
    pub text: String,
}

/// What roalt.dev says beyond the app itself (`extras`); empty when it couldn't be read.
#[derive(Serialize, TS, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Extras {
    pub sponsor: Option<Sponsor>,
    /// The names of those who support RoALT, as listed.
    pub supporters: Vec<String>,
}

/// Where `roblox_install` is (`roblox://install`).
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(tag = "stage", rename_all = "camelCase")]
#[ts(export)]
#[allow(dead_code)]
pub enum InstallProgress {
    Downloading { percent: u8 },
    /// Checking the installer's Authenticode signature.
    Verifying,
    /// Roblox's installer runs.
    Installing,
    Done,
    Error { message: String },
}

/// Where RoALT keeps its data.
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DataDir {
    pub path: String,
    /// Set for test runs (`ROALT_DATA_DIR`): moving is not offered.
    pub overridden: bool,
}

/// Why a client ended badly (`crashes_list`).
#[derive(Serialize, Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum CrashKind {
    /// Ended without RoALT closing it and without a disconnect in its log.
    Crash,
    /// Roblox disconnected it (kick, shutdown, connection lost).
    Disconnect,
    /// Its window stopped responding.
    Hang,
    /// RoALT restarted it: its memory stayed over `Settings.mem_restart_mb`.
    Mem,
    /// RoALT restarted it: it had run for `Settings.restart_every_h`.
    Timed,
}

impl CrashKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Crash => "crash",
            Self::Disconnect => "disconnect",
            Self::Hang => "hang",
            Self::Mem => "mem",
            Self::Timed => "timed",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "disconnect" => Self::Disconnect,
            "timed" => Self::Timed,
            "hang" => Self::Hang,
            "mem" => Self::Mem,
            "crash" => Self::Crash,
            other => {
                log::debug!("unrecognised crash kind {other:?}");
                Self::Crash
            }
        }
    }
}

/// One of Roblox's Charts lists (Top Trending, Up-and-Coming, ...), for Browse's Popular.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GameSort {
    pub id: String,
    /// As Roblox names it (in English).
    pub name: String,
    pub games: Vec<PopularGame>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PopularGame {
    pub universe_id: i64,
    pub root_place_id: i64,
    pub name: String,
    pub playing: i64,
    pub up_votes: i64,
    pub down_votes: i64,
    /// On Roblox's CDN; None when it has none yet.
    pub icon_url: Option<String>,
}

/// A game's numbers for its details panel (`game_stats`). A part that is None didn't answer.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GameStats {
    pub universe_id: i64,
    pub playing: i64,
    pub visits: i64,
    pub favourites: i64,
    pub max_players: i32,
    /// ISO 8601, as Roblox sends them.
    pub created: Option<String>,
    pub updated: Option<String>,
    pub genre: Option<String>,
    pub subgenre: Option<String>,
    pub up_votes: Option<i64>,
    pub down_votes: Option<i64>,
    pub passes: Option<Vec<GamePass>>,
    pub badges: Option<Vec<GameBadge>>,
    /// The list goes on past what was read (more than 500, or Roblox stopped answering).
    pub more_passes: bool,
    pub more_badges: bool,
    pub age_rating: Option<AgeRating>,
    /// "MorphToR6", "MorphToR15" or "PlayerChoice", as Roblox says it.
    pub avatar_type: Option<String>,
    /// Anyone may copy it in Studio.
    pub copying: Option<bool>,
    /// Places in the game (its start place and sub-places).
    pub places: Option<i64>,
    /// About how many servers run now: players over server size, rounded up.
    pub servers: Option<i64>,
    /// Members of the group that made it.
    pub group_members: Option<i64>,
    pub archived: Option<bool>,
}

/// A game's age rating ("Mild", from 9 up) and why.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgeRating {
    /// Roblox's own words, in English.
    pub label: String,
    pub minimum_age: Option<i64>,
    /// "Violence (Repeated/Mild)", …
    pub descriptors: Vec<String>,
}

/// A Roblox player's profile numbers for Browse's player details (`player_stats`). A part that
/// is None didn't answer.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlayerStats {
    pub user_id: i64,
    pub name: String,
    pub display_name: String,
    pub verified: bool,
    pub banned: bool,
    /// ISO 8601.
    pub created: Option<String>,
    pub headshot_url: Option<String>,
    pub friends: Option<i64>,
    pub followers: Option<i64>,
    pub following: Option<i64>,
    /// As the Browse account sees it: where they play only when Roblox tells that account.
    pub presence: Option<PlayerPresence>,
    pub groups: Option<Vec<PlayerGroup>>,
    pub badges: Option<Vec<RobloxBadge>>,
    pub games: Option<Vec<PlayerGame>>,
    /// They made more games than were read.
    pub more_games: bool,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlayerPresence {
    /// 0 offline, 1 website, 2 in a game, 3 in Studio.
    pub kind: i64,
    /// The game's name, when Roblox tells.
    pub place: Option<String>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlayerGroup {
    pub id: i64,
    pub name: String,
    pub role: String,
    pub members: i64,
}

/// A Roblox badge (Veteran, Friendship, …) a player holds.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RobloxBadge {
    pub id: i64,
    pub name: String,
    pub image_url: Option<String>,
}

/// A public game a player made.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlayerGame {
    pub universe_id: i64,
    pub place_id: i64,
    pub name: String,
    pub visits: i64,
}

/// A player's limiteds (`player_value`): `private` when their inventory is hidden.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlayerValue {
    pub private: bool,
    /// Another count was running and none was kept: nothing to show yet.
    pub busy: bool,
    pub value: Option<AccountValue>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GamePass {
    pub id: i64,
    pub name: String,
    /// Robux; None: off sale.
    pub price: Option<i64>,
    pub icon_url: Option<String>,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GameBadge {
    pub id: i64,
    pub name: String,
    pub awarded: i64,
    pub awarded_yesterday: i64,
    /// The share of players who won it, 0..1 (Roblox's "win rate"; 0 when it has none).
    pub win_rate: f64,
    pub icon_url: Option<String>,
}

/// An account's limiteds, as last counted (`account_value`).
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AccountValue {
    /// Copies held.
    pub count: u32,
    /// Robux: RAP of every copy.
    pub rap: i64,
    /// Robux: each copy at its Rolimons value, else its RAP. None without Rolimons values.
    pub value: Option<i64>,
    /// The most valuable, highest first (at most 10).
    pub top: Vec<LimitedItem>,
    /// Not every copy was counted (the inventory is bigger than RoALT reads, or Roblox stopped answering).
    pub partial: bool,
    /// Unix ms.
    pub fetched_at: i64,
}

#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LimitedItem {
    pub asset_id: i64,
    pub name: String,
    pub copies: u32,
    /// Each.
    pub rap: i64,
    pub rolimons: Option<RolimonsItem>,
}

/// What Rolimons says about a limited (`rolimons::parse`).
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RolimonsItem {
    pub name: String,
    /// Robux, Rolimons' copy of Roblox's RAP; None: none.
    pub rap: Option<i64>,
    /// Robux; None: not valued.
    pub value: Option<i64>,
    /// 0 Terrible .. 4 Amazing.
    pub demand: Option<u8>,
    /// 0 Lowering, 1 Unstable, 2 Stable, 3 Raising, 4 Fluctuating.
    pub trend: Option<u8>,
    /// Its RAP is pushed above what it's worth.
    pub projected: bool,
    pub hyped: bool,
    pub rare: bool,
    pub acronym: Option<String>,
}

/// Browse › Limiteds (`market_items`): every limited Rolimons lists.
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Market {
    /// Unix ms the list was downloaded; None: there is none (Rolimons couldn't be reached).
    pub fetched_at: Option<i64>,
    /// Asset id and what Rolimons says about it.
    pub items: Vec<(i64, RolimonsItem)>,
}

/// One account's time in one place (`Db::playtime`), from the clients RoALT followed.
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlaytimeRow {
    pub account_id: i64,
    pub place_id: i64,
    /// In a game, not teleporting.
    pub ms: i64,
    /// Unix ms.
    pub last_played: i64,
}

/// A server an account's client reached (the newest 50 per account are kept).
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ServerVisit {
    pub place_id: i64,
    pub job_id: String,
    /// Can't be joined by its id alone (a private server, whose code isn't kept, or wherever a
    /// teleport led): listed only.
    pub private: bool,
    /// Unix ms.
    #[ts(type = "number")]
    pub joined_at: i64,
}

/// A server any account's client reached, once however many accounts were in it (`Db::recent_servers`).
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlayedServer {
    pub place_id: i64,
    pub job_id: String,
    /// As `ServerVisit::private`: listed only.
    pub private: bool,
    /// The newest visit, Unix ms.
    pub joined_at: i64,
    /// The accounts that were in it.
    pub account_ids: Vec<i64>,
}

/// An account's time in games on one local day (`Db::playtime_days`; the last 90 days are kept).
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PlaytimeDay {
    pub account_id: i64,
    /// The local date as YYYYMMDD.
    pub day: i32,
    pub ms: i64,
}

/// Which of an account's Robux transactions `robux_transactions` reads.
#[derive(Deserialize, TS, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum RobuxKind {
    Purchase,
    Sale,
    GroupPayout,
    PremiumStipend,
}

impl RobuxKind {
    /// Its name in Roblox's URL.
    pub fn path(self) -> &'static str {
        match self {
            Self::Purchase => "Purchase",
            Self::Sale => "Sale",
            Self::GroupPayout => "GroupPayout",
            Self::PremiumStipend => "PremiumStipend",
        }
    }
}

/// One line of an account's Robux history (`robux_transactions`).
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RobuxTransaction {
    /// ISO 8601, as Roblox gave it.
    pub created: Option<String>,
    /// Robux: negative when spent.
    pub amount: i64,
    /// What it was for (an item, a pass, a game); "" when Roblox named nothing.
    pub what: String,
    /// The other side (a player, a group, Roblox); "" when none.
    pub who: String,
    pub pending: bool,
}

/// An account's Robux over its last month (`robux_totals`); a number Roblox left out is none.
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RobuxTotals {
    pub incoming: Option<i64>,
    /// Negative, as Roblox sends it.
    pub outgoing: Option<i64>,
    pub pending: Option<i64>,
}

/// One client that crashed, was disconnected or hung (the newest 200 are kept).
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Crash {
    pub id: i64,
    /// The `Instance` it was.
    pub instance_id: String,
    pub account_id: i64,
    pub kind: CrashKind,
    /// Unix ms it ended.
    pub at: i64,
    /// From launch to end.
    pub uptime_ms: i64,
    /// What its log said (disconnect reason, redacted to plain text), if anything.
    pub reason: Option<String>,
    pub place_id: Option<i64>,
    /// Its client log was found ("Open log"); the path itself stays in the backend.
    pub has_log: bool,
    /// What the autopsy made of it (`autopsy::Verdict::code`); none before it is done, and for an
    /// end RoALT made itself.
    pub verdict: Option<String>,
}

/// A message for the user from the backend (`app://notice`), shown as a toast.
#[derive(Serialize, TS, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Notice {
    pub error: bool,
    pub message: String,
}
