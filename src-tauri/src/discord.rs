//! Discord alerts (opt-in): what happens to the clients, posted to a webhook the user pastes.
//! Contacted only once a webhook is set.
//!
//! The webhook URL holds its token, so it is a secret: kept in the vault (AES-GCM, `DISCORD_AAD`)
//! as hex in the settings table, never logged, never sent to the UI (only its id's last digits).
//! Messages go out only while the vault is unlocked. Events (`Settings.discord` picks them) are
//! coalesced: at most one message every 2 s, carrying everything queued since; a 429 is waited
//! out as Discord says (`retry_after`). Messages name accounts by display name and username,
//! and never carry a cookie, ticket, launch link or log line.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use reqwest::StatusCode;
use reqwest::header::RETRY_AFTER;
use serde_json::{Value, json};
use tauri::{AppHandle, Manager};

use crate::App;
use crate::error::{AppError, Result};
use crate::instances::{Transition, hung, unexpected};
use crate::model::{AccountSummary, BulkProgress, BulkStage, CrashKind, DiscordEvents, DiscordStatus, InstanceState, PingLevel};
use crate::vault::Secret;

const DISCORD_AAD: &[u8] = b"roalt.discord.webhook";
/// The settings-table row holding the encrypted webhook (hex).
const KEY: &str = "discord_webhook";
/// At most one message this often.
const GAP: Duration = Duration::from_secs(2);
/// Events waiting beyond this many drop the oldest.
const MAX_QUEUED: usize = 50;
/// Consecutive network failures before a failing batch is dropped instead of retrying forever.
const MAX_RETRIES: u32 = 6;
/// Lines in one message; the rest are counted.
const MAX_LINES: usize = 15;
const HOSTS: [&str; 6] =
    ["discord.com", "ptb.discord.com", "canary.discord.com", "discordapp.com", "ptb.discordapp.com", "canary.discordapp.com"];

/// A checked webhook: `https://<host>/api/webhooks/<id>/<token>`.
#[derive(Debug)]
pub struct Webhook {
    pub url: Secret,
    pub id: String,
}

/// Accepts only Discord's own webhook URLs (discord.com or discordapp.com, also their ptb. and
/// canary. hosts), https on the default port, with nothing but `/api/webhooks/<id>/<token>`.
pub fn parse_webhook(raw: &str) -> Result<Webhook> {
    let bad = || AppError::Invalid(tr!("Paste a Discord webhook URL: https://discord.com/api/webhooks/<id>/<token>."));
    let u = url::Url::parse(raw.trim()).map_err(|_| bad())?;
    let host = u.host_str().filter(|h| HOSTS.contains(h)).ok_or_else(bad)?;
    if u.scheme() != "https" || !u.username().is_empty() || u.password().is_some() || u.port().is_some() {
        return Err(bad());
    }
    if u.query().is_some() || u.fragment().is_some() {
        return Err(bad());
    }
    let parts: Vec<&str> = u.path_segments().ok_or_else(bad)?.collect();
    let [api, webhooks, id, token] = parts[..] else { return Err(bad()) };
    let id_ok = (1..=25).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit());
    let token_ok = (1..=128).contains(&token.len()) && token.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if api != "api" || webhooks != "webhooks" || !id_ok || !token_ok {
        return Err(bad());
    }
    Ok(Webhook { url: Secret::new(format!("https://{host}/api/webhooks/{id}/{token}")), id: id.to_owned() })
}

/// Who alerts ping, as typed: a user id (`123`, `<@123>`, `<@!123>`) or a role (`<@&123>`).
/// Stored as `<@id>` or `<@&id>`; "" for no one.
pub fn parse_ping(raw: &str) -> Result<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(String::new());
    }
    let (role, id) = match raw.strip_prefix("<@").and_then(|r| r.strip_suffix('>')) {
        Some(inner) => match inner.strip_prefix('&') {
            Some(id) => (true, id),
            None => (false, inner.strip_prefix('!').unwrap_or(inner)),
        },
        None => (false, raw),
    };
    if !(1..=25).contains(&id.len()) || !id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(AppError::Invalid(tr!("Ping takes a Discord user ID (only digits), or a role as <@&id>.")));
    }
    Ok(if role { format!("<@&{id}>") } else { format!("<@{id}>") })
}

/// Puts a stored ping (`parse_ping`) in a webhook body: as its text, and as the one mention
/// Discord may fire. Nothing for "".
fn add_ping(body: &mut Value, ping: &str) {
    let (key, rest) = match ping.strip_prefix("<@&") {
        Some(r) => ("roles", r),
        None => ("users", ping.strip_prefix("<@").unwrap_or_default()),
    };
    let Some(id) = rest.strip_suffix('>') else { return };
    body["content"] = json!(ping);
    body["allowed_mentions"][key] = json!([id]);
}

/// What the UI shows of a webhook id: its last 4 digits.
fn masked(id: &str) -> String {
    format!("…{}", &id[id.len().saturating_sub(4)..])
}

/// Something worth telling the user about.
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)]
pub enum Event {
    /// `game`: the game's name when RoALT has it (escaped); else the place is named by its id.
    Crash { name: String, place_id: Option<i64>, game: Option<String> },
    Hang { name: String },
    /// `reason`: why, in words (`reason_words`).
    Disconnect { name: String, reason: Option<String> },
    /// A client first got into a game.
    Launch { name: String, place_id: Option<i64>, game: Option<String> },
    /// The account's sign-in is in trouble; `note` says how (a client's note, or the saved sign-in died).
    SignIn { name: String, note: String },
    /// A client is held to the memory throttle's CPU cap.
    Throttled { name: String },
    /// A newer RoALT is out.
    Update { version: String },
    /// The PC's memory stayed nearly full with `clients` running.
    PcMemory { pct: u32, clients: usize },
    /// The clients' GPU use stayed nearly at its most.
    PcGpu { pct: u32, clients: usize },
    /// An auto-rejoin (`Disconnect`) or auto-restart went ahead.
    Restart { name: String, cause: CrashKind },
    /// The crash breaker gave up on the account.
    Breaker { name: String, count: u32, window_min: u32 },
    /// A relaunch stopped for good (60-min timeout, invalid cookie, client wouldn't close, permanent error).
    GaveUp { name: String, reason: String },
    BatchDone { total: usize, in_game: usize, done: usize, failed: usize },
    Status { in_game: usize, disconnected: usize, given_up: usize, ram_mb: f64, cpu_pct: Option<f64> },
    /// Windows was locked while anti-AFK looks after clients: it can't press keys until unlocked.
    AfkBlocked,
}

impl Event {
    fn wanted(&self, v: &DiscordEvents) -> bool {
        match self {
            Self::Crash { .. } => v.crash,
            Self::Hang { .. } => v.hang,
            // Told with the disconnects it leads to.
            Self::Disconnect { .. } | Self::AfkBlocked => v.disconnect,
            Self::Launch { .. } => v.launch,
            Self::SignIn { .. } => v.sign_in,
            Self::Throttled { .. } => v.throttle,
            Self::Update { .. } => v.update,
            Self::PcMemory { .. } | Self::PcGpu { .. } => v.pc,
            Self::Restart { cause: CrashKind::Mem, .. } => v.mem_restart,
            Self::Restart { .. } => v.restart,
            Self::Breaker { .. } | Self::GaveUp { .. } => v.breaker,
            Self::BatchDone { .. } => v.batch_done,
            Self::Status { .. } => v.status_every_min > 0,
        }
    }

    /// 2: bad, 1: worth a look, 0: news.
    fn severity(&self) -> u8 {
        match self {
            Self::Crash { .. } | Self::Breaker { .. } | Self::GaveUp { .. } | Self::AfkBlocked => 2,
            Self::Hang { .. } | Self::Disconnect { .. } | Self::SignIn { .. } | Self::PcMemory { .. } | Self::PcGpu { .. } => 1,
            Self::BatchDone { failed, .. } if *failed > 0 => 1,
            _ => 0,
        }
    }

    fn line(&self) -> String {
        match self {
            Self::Crash { name, game: Some(g), .. } => format!("💥 {}", tr!("**{name}**: the client crashed in {game}.", name = name, game = g)),
            Self::Crash { name, place_id: Some(p), .. } => {
                format!("💥 {}", tr!("**{name}**: the client crashed (place {place}).", name = name, place = p))
            }
            Self::Crash { name, .. } => format!("💥 {}", tr!("**{name}**: the client crashed.", name = name)),
            Self::Hang { name } => format!("🧊 {}", tr!("**{name}**: the client stopped responding.", name = name)),
            Self::Disconnect { name, reason: Some(r) } => {
                format!("🔌 {}", tr!("**{name}**: disconnected by Roblox ({reason}).", name = name, reason = r))
            }
            Self::Disconnect { name, reason: None } => format!("🔌 {}", tr!("**{name}**: disconnected by Roblox.", name = name)),
            Self::Launch { name, game: Some(g), .. } => format!("🎮 {}", tr!("**{name}**: in {game}.", name = name, game = g)),
            Self::Launch { name, place_id: Some(p), .. } => {
                format!("🎮 {}", tr!("**{name}**: in game (place {place}).", name = name, place = p))
            }
            Self::Launch { name, .. } => format!("🎮 {}", tr!("**{name}**: in game.", name = name)),
            Self::SignIn { name, note } => format!("🔐 **{name}**: {note}."),
            Self::Throttled { name } => format!("🐢 {}", tr!("**{name}**: slowed down, it uses too much memory.", name = name)),
            Self::Update { version } => format!("⬆️ {}", tr!("RoALT {version} is out.", version = version)),
            Self::PcMemory { pct, clients } => format!("🧠 {}", tr!("This PC's memory is {pct}% full (clients running: {n}).", pct = pct, n = clients)),
            Self::PcGpu { pct, clients } => format!("🖥️ {}", tr!("The clients keep the GPU at {pct}% (clients running: {n}).", pct = pct, n = clients)),
            Self::Restart { name, cause: CrashKind::Disconnect } => format!("🔁 {}", tr!("**{name}**: rejoined.", name = name)),
            Self::Restart { name, cause: CrashKind::Mem } => {
                format!("🧹 {}", tr!("**{name}**: restarted, it used too much memory.", name = name))
            }
            Self::Restart { name, cause: CrashKind::Timed } => format!("⏱️ {}", tr!("**{name}**: restarted on its timer.", name = name)),
            Self::Restart { name, cause: CrashKind::Hang } => {
                format!("🔁 {}", tr!("**{name}**: restarted after it stopped responding.", name = name))
            }
            Self::Restart { name, cause: CrashKind::Crash } => {
                format!("🔁 {}", tr!("**{name}**: restarted after a crash.", name = name))
            }
            Self::Breaker { name, count, window_min } => format!(
                "⛔ {}",
                trn!(
                    *count,
                    "**{name}**: RoALT gave up after {n} relaunch in {minutes} min.",
                    "**{name}**: RoALT gave up after {n} relaunches in {minutes} min.",
                    name = name,
                    minutes = window_min
                )
            ),
            Self::GaveUp { name, reason } => {
                let why = reason.trim().trim_end_matches('.');
                format!("⛔ **{name}**: {why}.")
            }
            // Why a batch stopped early is left out: that text names accounts by alias, also muted ones.
            Self::AfkBlocked => format!(
                "🔒 {}",
                tr!("Windows is locked, so anti-AFK can't press keys: clients in a game will be disconnected for idling after 20 minutes. Unlock the PC to keep them in.")
            ),
            Self::BatchDone { total, in_game, done, failed } => {
                let mut s = format!("🚀 {}", tr!("Batch finished: {in_game} of {total} in game", in_game = in_game, total = total));
                if *done > 0 {
                    s += &tr!(", {done} done", done = done);
                }
                if *failed > 0 {
                    s += &tr!(", {failed} failed", failed = failed);
                }
                s + "."
            }
            Self::Status { in_game, disconnected, given_up, ram_mb, cpu_pct } => {
                if *in_game + *disconnected + *given_up == 0 {
                    return format!("📊 {}", tr!("0 clients running."));
                }
                let cpu = cpu_pct.map(|c| format!(" · {c:.0}% CPU")).unwrap_or_default();
                let ram = format!("{ram_mb:.0}");
                if *disconnected == 0 && *given_up == 0 {
                    format!(
                        "📊 {}",
                        trn!(
                            *in_game,
                            "{n} client running · {ram} MB RAM{cpu}.",
                            "{n} clients running · {ram} MB RAM{cpu}.",
                            ram = ram,
                            cpu = cpu
                        )
                    )
                } else {
                    let mut parts = vec![tr!("{n} in game", n = in_game)];
                    if *disconnected > 0 {
                        parts.push(tr!("{n} disconnected", n = disconnected));
                    }
                    if *given_up > 0 {
                        parts.push(tr!("{n} gave up", n = given_up));
                    }
                    format!("📊 {} · {ram} MB RAM{cpu}.", parts.join(", "))
                }
            }
        }
    }
}

/// An account as messages name it: display name and username (never a cookie or alias notes).
pub fn name_of(a: Option<&AccountSummary>, account_id: i64) -> String {
    match a {
        Some(a) => escape(&format!("{} (@{})", a.display_name, a.username)),
        None => tr!("Account {id}", id = account_id),
    }
}

/// Keeps Discord's markdown out of names.
fn escape(s: &str) -> String {
    s.chars().fold(String::with_capacity(s.len()), |mut out, c| {
        if matches!(c, '\\' | '*' | '_' | '~' | '`' | '|' | '>' | '[' | ']' | '@') {
            out.push('\\');
        }
        out.push(c);
        out
    })
}

/// What a client's change is worth telling (pure): it got disconnected, stopped responding,
/// or ended by itself (not after a disconnect, which was told already).
pub fn change_of(t: &Transition) -> Option<CrashKind> {
    let (b, a) = (&t.before, &t.after);
    if a.state == InstanceState::Exited {
        let by_itself = b.state != InstanceState::Exited && a.note.as_deref() == Some(&*unexpected());
        return (by_itself && b.state != InstanceState::Disconnected).then_some(CrashKind::Crash);
    }
    if a.state == InstanceState::Disconnected && b.state != InstanceState::Disconnected {
        return Some(CrashKind::Disconnect);
    }
    let hung = hung();
    (a.note.as_deref() == Some(&*hung) && b.note.as_deref() != Some(&*hung)).then_some(CrashKind::Hang)
}

/// The client got into a game for the first time (pure): not a rejoin, restart or reconnect.
pub fn launched(t: &Transition) -> bool {
    let (b, a) = (&t.before, &t.after);
    t.first_game
        && a.state == InstanceState::InGame
        && a.rejoins == 0
        && !matches!(b.state, InstanceState::InGame | InstanceState::Disconnected)
}

/// The webhook body for some events (pure): one embed, one line each. `ping` is mentioned
/// according to `ping_level` (`Urgent`: severity 2; `Warnings`: severity 1 or 2), never for news.
pub fn message(events: &[Event], ping: &str, ping_level: PingLevel) -> Value {
    let mut lines: Vec<String> = events.iter().take(MAX_LINES).map(Event::line).collect();
    if events.len() > MAX_LINES {
        lines.push(trn!(events.len() - MAX_LINES, "…and {n} more.", "…and {n} more."));
    }
    let severity = events.iter().map(Event::severity).max();
    let color = match severity {
        Some(2) => 0xE5_48_4D,
        Some(1) => 0xFF_A0_5C,
        _ => 0xB4_F0_00,
    };
    let mut body = json!({
        "username": "RoALT",
        "allowed_mentions": { "parse": [] },
        "embeds": [{ "title": "RoALT", "description": lines.join("\n"), "color": color }],
    });
    let min_sev = match ping_level {
        PingLevel::Urgent => 2,
        PingLevel::Warnings => 1,
    };
    if severity.is_some_and(|s| s >= min_sev) {
        add_ping(&mut body, ping);
    }
    body
}

/// How long Discord asks to wait after a 429: its JSON `retry_after` (seconds), else the
/// Retry-After header, else 5 s; at least `GAP`, at most 10 minutes.
fn retry_after(body: &str, header: Option<&str>) -> Duration {
    let secs = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("retry_after")?.as_f64())
        .or_else(|| header?.trim().parse::<f64>().ok())
        .filter(|s| s.is_finite() && *s >= 0.0)
        .unwrap_or(5.0);
    Duration::from_secs_f64(secs.min(600.0)).max(GAP)
}

/// The events waiting to go out and when the next message may (pure apart from the clock
/// passed in).
#[derive(Default)]
pub struct Outbox {
    queue: VecDeque<Event>,
    not_before: Option<Instant>,
    /// Consecutive network failures for the current batch (reset on success or drop).
    retries: u32,
}

impl Outbox {
    fn push(&mut self, e: Event) {
        if self.queue.len() == MAX_QUEUED {
            log::warn!("discord: outbox full ({MAX_QUEUED}), oldest event dropped");
            self.queue.pop_front();
        }
        self.queue.push_back(e);
    }

    /// Everything queued, once the last message is far enough back.
    fn take(&mut self, now: Instant) -> Vec<Event> {
        if self.not_before.is_some_and(|t| now < t) {
            return Vec::new();
        }
        self.queue.drain(..).collect()
    }

    /// A message went out (or failed for good) at `now`.
    /// Never shortens a longer wait Discord asked for.
    fn sent(&mut self, now: Instant) {
        self.retries = 0;
        self.not_before = Some(self.not_before.map_or(now + GAP, |t| t.max(now + GAP)));
    }

    /// Discord said to wait `after`: the events go back in front.
    fn retry(&mut self, events: Vec<Event>, now: Instant, after: Duration) {
        for e in events.into_iter().rev() {
            self.queue.push_front(e);
        }
        while self.queue.len() > MAX_QUEUED {
            self.queue.pop_front();
        }
        self.not_before = Some(now + after.max(GAP));
    }

    /// Network failure backoff (30 s, 60 s, 120 s, 240 s, 300 s, capped at `MAX_RETRIES`).
    /// Returns `Some(wait)` when requeued, or `None` when the retry cap was reached and the batch dropped.
    fn network_retry(&mut self, events: Vec<Event>, now: Instant) -> Option<Duration> {
        self.retries += 1;
        if self.retries > MAX_RETRIES {
            self.sent(now);
            return None;
        }
        let secs = 30u64.saturating_mul(1u64 << (self.retries - 1).min(4)).min(300);
        let wait = Duration::from_secs(secs);
        self.retry(events, now, wait);
        Some(wait)
    }
}

pub struct Discord {
    outbox: Mutex<Outbox>,
    http: std::result::Result<reqwest::Client, String>,
    /// When the last status summary was queued.
    last_status: Mutex<Option<Instant>>,
    /// Cached `configured` flag so background loops don't hit SQLite when no webhook is set.
    configured_cache: Mutex<Option<bool>>,
    /// True once at least one client was seen by the status check, so dropping to 0 reports once.
    had_clients: AtomicBool,
    /// Rate-limits the warning when alerts are dropped because the vault is locked.
    locked_warned: AtomicBool,
}

impl Discord {
    pub fn new() -> Discord {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| e.without_url().to_string())
            .inspect_err(|e| log::error!("discord: http client unavailable: {e}"));
        Discord {
            outbox: Mutex::default(),
            http,
            last_status: Mutex::new(None),
            configured_cache: Mutex::new(None),
            had_clients: AtomicBool::new(false),
            locked_warned: AtomicBool::new(false),
        }
    }

    fn outbox(&self) -> MutexGuard<'_, Outbox> {
        self.outbox.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Default for Discord {
    fn default() -> Self {
        Self::new()
    }
}

/// Lowercase hex, how vault blobs sit in the settings table.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

/// The stored webhook; None when none is set. `Locked` while the vault is.
fn webhook(s: &App) -> Result<Option<Webhook>> {
    let Some(stored) = s.db.setting_value(KEY)? else { return Ok(None) };
    let blob = unhex(&stored).ok_or(AppError::Crypto)?;
    parse_webhook(s.vault.decrypt_secret(DISCORD_AAD, &blob)?.expose()).map(Some)
}

/// The stored webhook's URL, for a backup; None when none is set or it can't be read now.
pub fn stored_url(s: &App) -> Option<zeroize::Zeroizing<String>> {
    webhook(s).ok().flatten().map(|h| h.url.expose().to_owned().into())
}

/// Re-encrypts the stored Discord webhook under `new_key`, returning the new hex string (or `None`
/// if no webhook is stored).
pub fn reencrypt_webhook(s: &App, new_key: &[u8; 32]) -> Result<Option<String>> {
    let Some(stored) = s.db.setting_value(KEY)? else { return Ok(None) };
    let blob = unhex(&stored).ok_or(AppError::Crypto)?;
    let rotated = s.vault.reencrypt_with_key(new_key, DISCORD_AAD, &blob)?;
    Ok(Some(hex(&rotated)))
}

/// The stored ping and ping level; defaults when settings are unreadable, so the alert still goes.
fn ping_settings(s: &App) -> (String, PingLevel) {
    s.db.settings().map(|v| (v.discord.ping, v.discord.ping_level)).unwrap_or_default()
}

/// While the vault is locked a stored webhook still shows as configured (its id unknown).
pub fn status(s: &App) -> Result<DiscordStatus> {
    match webhook(s) {
        Ok(hook) => Ok(DiscordStatus { configured: hook.is_some(), webhook: hook.map(|h| masked(&h.id)) }),
        Err(AppError::Locked) => Ok(DiscordStatus { configured: s.db.setting_value(KEY)?.is_some(), webhook: None }),
        Err(e) => Err(e),
    }
}

/// Stores the webhook (checked by `parse_webhook`), or forgets it on None or "".
pub fn set(s: &App, url: Option<&str>) -> Result<DiscordStatus> {
    match url.map(str::trim).filter(|u| !u.is_empty()) {
        Some(u) => {
            let hook = parse_webhook(u)?;
            let blob = s.vault.encrypt(DISCORD_AAD, hook.url.expose().as_bytes())?;
            s.db.set_setting_value(KEY, Some(&hex(&blob)))?;
            *s.discord.configured_cache.lock().unwrap_or_else(|e| e.into_inner()) = Some(true);
            log::info!("discord webhook set (id ending {})", masked(&hook.id));
        }
        None => {
            s.db.set_setting_value(KEY, None)?;
            *s.discord.configured_cache.lock().unwrap_or_else(|e| e.into_inner()) = Some(false);
            s.discord.outbox().queue.clear();
            log::info!("discord webhook removed");
        }
    }
    status(s)
}

/// Sends a test message at once (it still counts for the 2 s gap).
pub async fn test(s: &App) -> Result<()> {
    let hook = webhook(s)?.ok_or_else(|| AppError::Invalid(tr!("Set a webhook first.")))?;
    let mut body = json!({
        "username": "RoALT",
        "allowed_mentions": { "parse": [] },
        "embeds": [{ "title": "RoALT", "description": format!("✅ {}", tr!("Test message: RoALT's alerts reach this channel.")), "color": 0xB4_F0_00 }],
    });
    // The test pings too, so it shows the mention works.
    add_ping(&mut body, &ping_settings(s).0);
    let now = Instant::now();
    let r = post(s, &hook, &body).await;
    match r {
        Ok(Some(wait)) => s.discord.outbox().retry(Vec::new(), now, wait),
        _ => s.discord.outbox().sent(now),
    }
    match r? {
        None => Ok(()),
        Some(wait) => Err(AppError::Invalid(tr!("Discord is limiting messages. Try again in {seconds} s.", seconds = wait.as_secs().max(1)))),
    }
}

/// One POST; Some(wait) when Discord rate-limited it. Errors never carry the URL.
async fn post(s: &App, hook: &Webhook, body: &Value) -> Result<Option<Duration>> {
    let http = s.discord.http.as_ref().map_err(|e| AppError::Internal(e.clone()))?;
    let r = http.post(hook.url.expose()).json(body).send().await.map_err(|e| {
        let post_connect_timeout = e.is_timeout() && !e.is_connect();
        let msg = e.without_url().to_string();
        // A timeout after connecting may mean Discord already received the message: don't retry
        // and duplicate it. Connect/DNS errors haven't sent anything and can safely retry.
        if post_connect_timeout {
            AppError::Internal(format!("Discord timed out: {msg}"))
        } else {
            AppError::Network(msg)
        }
    })?;
    match r.status() {
        st if st.is_success() => Ok(None),
        StatusCode::TOO_MANY_REQUESTS => {
            let header = r.headers().get(RETRY_AFTER).and_then(|v| v.to_str().ok()).map(str::to_owned);
            Ok(Some(retry_after(&r.text().await.unwrap_or_default(), header.as_deref())))
        }
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND => {
            Err(AppError::Invalid(tr!("Discord refused the webhook; it may have been deleted. Paste a new one.")))
        }
        // Any other 4xx (e.g. 400 Bad Request, 413 Payload Too Large) will never succeed on retry.
        st if st.is_client_error() => Err(AppError::Invalid(tr!("Discord rejected the alert ({status}).", status = st.as_u16()))),
        st => Err(AppError::Network(tr!("Discord answered {status}.", status = st.as_u16()))),
    }
}

/// Queues an event when its toggle is on, a webhook is set and the vault is unlocked; whether it was.
pub fn notify(app: &AppHandle, event: Event) -> bool {
    let s = app.state::<App>();
    if !configured(&s) {
        return false;
    }
    let wanted = s
        .db
        .settings()
        .inspect_err(|e| log::warn!("discord: event dropped, settings unreadable: {e}"))
        .is_ok_and(|v| event.wanted(&v.discord));
    if !wanted {
        return false;
    }
    if s.vault.is_locked() {
        if !s.discord.locked_warned.swap(true, Ordering::Relaxed) {
            log::warn!("discord: alerts paused while the vault is locked");
        }
        return false;
    }
    s.discord.locked_warned.store(false, Ordering::Relaxed);
    s.discord.outbox().push(event);
    true
}

/// A disconnect's reason in words (pure): Roblox's error code when the logged line ends in a known
/// one, another code as it is, else the line itself, cut short. Never empty.
// shortcut: the four codes are the common ones, from Roblox's error dialogs; add more as users meet them.
pub fn reason_words(reason: &str) -> String {
    match reason.trim().rsplit(' ').next().and_then(|c| c.parse::<u32>().ok()) {
        Some(267) => tr!("kicked by the game"),
        Some(277) => tr!("connection lost"),
        Some(278) => tr!("idle for 20 minutes"),
        Some(288) => tr!("the server shut down"),
        Some(code) => tr!("error {code}", code = code),
        None => escape(&reason.trim().chars().take(80).collect::<String>()),
    }
}

/// How messages name the account, or None when its alerts are muted (`discord_muted`).
fn named(app: &AppHandle, account_id: i64) -> Option<String> {
    let a = app.state::<App>().db.effective(account_id).inspect_err(|e| log::debug!("discord: account {account_id} unreadable: {e}")).ok();
    (!a.as_ref().is_some_and(|a| a.discord_muted)).then(|| name_of(a.as_ref(), account_id))
}

/// The tracker's poll saw these changes. Muted accounts (`discord_muted`) are left out.
pub fn transitions(app: &AppHandle, moved: &[Transition]) {
    for t in moved {
        let kind = change_of(t);
        // Told when it starts, not for as long as it lasts.
        let sign_in = t.after.sign_in_problem && !t.before.sign_in_problem;
        if kind.is_none() && !launched(t) && !sign_in {
            continue;
        }
        let Some(name) = named(app, t.after.account_id) else { continue };
        let place_id = t.after.place_id;
        let game = || place_id.and_then(|p| crate::commands::cached_game_name(&app.state::<App>(), p)).map(|g| escape(&g));
        if sign_in && let Some(note) = &t.after.note {
            notify(app, Event::SignIn { name: name.clone(), note: note.clone() });
        }
        notify(
            app,
            match kind {
                None if launched(t) => Event::Launch { name, place_id, game: game() },
                None => continue,
                Some(CrashKind::Disconnect) => Event::Disconnect { name, reason: t.reason.as_deref().map(reason_words) },
                Some(CrashKind::Hang) => Event::Hang { name },
                Some(_) => Event::Crash { name, place_id, game: game() },
            },
        );
    }
}

/// A client is now held to the memory throttle's CPU cap.
pub fn throttled(app: &AppHandle, account_id: i64) {
    if let Some(name) = named(app, account_id) {
        notify(app, Event::Throttled { name });
    }
}

/// A webhook is set (whether or not the vault is unlocked to read it). Cached in memory so
/// background loops do zero SQLite reads when no webhook is configured.
pub fn configured(s: &App) -> bool {
    let mut cache = s.discord.configured_cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(v) = *cache {
        return v;
    }
    let v = s.db.setting_value(KEY).is_ok_and(|v| v.is_some());
    *cache = Some(v);
    v
}

/// Windows was locked while anti-AFK looks after clients (`session`).
#[allow(dead_code)]
pub fn afk_blocked(app: &AppHandle) {
    notify(app, Event::AfkBlocked);
}

/// The account's saved sign-in stopped working, or Roblox wants a human check for it.
pub fn sign_in(app: &AppHandle, account_id: i64, challenged: bool) {
    if let Some(name) = named(app, account_id) {
        let note = if challenged { crate::instances::challenged() } else { tr!("The saved sign-in no longer works. Sign in again") };
        notify(app, Event::SignIn { name, note });
    }
}

/// A batch finished (`launcher::bulk`).
pub fn batch_done(app: &AppHandle, p: &BulkProgress) {
    let count = |f: fn(&BulkStage) -> bool| p.items.iter().filter(|i| f(&i.stage)).count();
    notify(
        app,
        Event::BatchDone {
            total: p.items.len(),
            in_game: count(|s| *s == BulkStage::InGame),
            done: count(|s| *s == BulkStage::Done),
            failed: count(|s| matches!(s, BulkStage::Failed { .. })),
        },
    );
}

/// The PC's memory use (percent) from which it counts as nearly full, and for how long it must stay so.
const PC_MEM_PCT: f64 = 90.0;
const PC_MEM_FOR: Duration = Duration::from_secs(60);
/// The same for the clients' summed GPU use.
const PC_GPU_PCT: f64 = 95.0;
const PC_GPU_FOR: Duration = Duration::from_secs(5 * 60);
/// How often the PC is looked at for those.
const PC_EVERY: Duration = Duration::from_secs(10);

/// Something that is told once it has lasted, and again only after it stopped in between (pure
/// apart from the clock passed in).
#[derive(Default)]
pub struct Sustained {
    since: Option<Instant>,
    told: bool,
}

impl Sustained {
    /// `over`: it is so now. True the one time it has been so for `hold` without a break.
    pub fn step(&mut self, over: bool, now: Instant, hold: Duration) -> bool {
        if !over {
            *self = Self::default();
            return false;
        }
        let since = *self.since.get_or_insert(now);
        let tell = !self.told && now.duration_since(since) >= hold;
        self.told |= tell;
        tell
    }
}

/// Runs for the app's lifetime: queues the periodic status and the PC alerts, sends what is queued.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let (mut memory, mut gpu, mut looked) = (Sustained::default(), Sustained::default(), Instant::now());
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let s = app.state::<App>();
            if looked.elapsed() >= PC_EVERY {
                looked = Instant::now();
                if configured(&s) {
                    queue_status(&app).await;
                    queue_pc(&app, &mut memory, &mut gpu).await;
                } else {
                    (memory, gpu) = Default::default();
                }
            }
            let events = s.discord.outbox().take(Instant::now());
            if !events.is_empty() {
                deliver(&s, events).await;
            }
        }
    });
}

async fn deliver(s: &App, events: Vec<Event>) {
    let hook = match webhook(s) {
        Ok(Some(h)) => h,
        Ok(None) => return,
        Err(e) => {
            log::info!("discord: {} event(s) dropped: {e}", events.len());
            return;
        }
    };
    let (ping_to, ping_level) = ping_settings(s);
    let now = Instant::now();
    match post(s, &hook, &message(&events, &ping_to, ping_level)).await {
        Ok(None) => s.discord.outbox().sent(now),
        Ok(Some(wait)) => {
            log::info!("discord: rate limited, waiting {} s", wait.as_secs());
            s.discord.outbox().retry(events, now, wait);
        }
        // An outage is when the alerts matter most: back off up to MAX_RETRIES before dropping.
        Err(AppError::Network(e)) => {
            let n = events.len();
            match s.discord.outbox().network_retry(events, now) {
                Some(wait) => log::warn!("discord: message not sent, trying again in {} s: {e}", wait.as_secs()),
                None => log::warn!("discord: {n} event(s) dropped after {MAX_RETRIES} network failures: {e}"),
            }
        }
        Err(e) => {
            log::warn!("discord: message not sent: {e}");
            s.discord.outbox().sent(now);
        }
    }
}

/// While its switch is on and clients run: tells once when the PC's memory, or the clients' GPU
/// use, has stayed nearly at its most (`Sustained`). Nothing is measured while it is off.
async fn queue_pc(app: &AppHandle, memory: &mut Sustained, gpu: &mut Sustained) {
    let s = app.state::<App>();
    let clients = s.tracker.clients();
    if clients.is_empty() || !s.db.settings().is_ok_and(|v| v.discord.pc) || !configured(&s) {
        (*memory, *gpu) = Default::default();
        return;
    }
    let (n, handle) = (clients.len(), app.clone());
    let read = tauri::async_runtime::spawn_blocking(move || {
        let s = handle.state::<App>();
        (s.metrics.system(), s.metrics.sample(&clients))
    })
    .await;
    let Ok((system, sampled)) = read.inspect_err(|e| log::warn!("discord: PC not measured: {e}")) else { return };
    let now = Instant::now();
    if let Some(u) = system.filter(|u| u.mem_total_mb > 0.0) {
        let pct = u.mem_used_mb / u.mem_total_mb * 100.0;
        if memory.step(pct >= PC_MEM_PCT, now, PC_MEM_FOR) {
            notify(app, Event::PcMemory { pct: pct.round() as u32, clients: n });
        }
    }
    // The clients' busiest engines, summed: what they ask of the GPU together (None: Windows didn't say).
    if let Some(pct) = sampled.iter().filter_map(|c| c.gpu_pct).reduce(|a, b| a + b).map(|p| p.min(100.0))
        && gpu.step(pct >= PC_GPU_PCT, now, PC_GPU_FOR)
    {
        notify(app, Event::PcGpu { pct: pct.round() as u32, clients: n });
    }
}

/// Every `status_every_min` minutes while clients run (and once when they all stop):
/// how many are in game / disconnected / gave up, plus their RAM and CPU.
async fn queue_status(app: &AppHandle) {
    let s = app.state::<App>();
    let every = s.db.settings().map_or(0, |v| v.discord.status_every_min);
    let due = {
        let mut last = s.discord.last_status.lock().unwrap_or_else(|e| e.into_inner());
        let start = *last.get_or_insert_with(Instant::now);
        let due = every > 0 && start.elapsed() >= Duration::from_secs(u64::from(every) * 60);
        if due || every == 0 {
            *last = Some(Instant::now());
        }
        due
    };
    if !due {
        return;
    }
    let (in_game, disconnected, given_up) = s.tracker.status_counts();
    let total = in_game + disconnected + given_up;
    if total == 0 {
        if s.discord.had_clients.swap(false, Ordering::Relaxed) {
            notify(app, Event::Status { in_game: 0, disconnected: 0, given_up: 0, ram_mb: 0.0, cpu_pct: None });
        }
        return;
    }
    s.discord.had_clients.store(true, Ordering::Relaxed);
    let handle = app.clone();
    let m = tauri::async_runtime::spawn_blocking(move || {
        let s = handle.state::<App>();
        s.metrics.sample(&s.tracker.clients())
    })
    .await
    .unwrap_or_else(|e| {
        log::warn!("discord: status metrics failed: {e}");
        Vec::new()
    });
    let ram_mb = m.iter().map(|c| c.memory_mb).sum();
    // CPU needs an earlier sample of the same client; without one it's left out, not 0%.
    let cpu: Vec<f64> = m.iter().filter_map(|c| c.cpu_pct).collect();
    let cpu_pct = (!cpu.is_empty()).then(|| cpu.iter().sum());
    notify(app, Event::Status { in_game, disconnected, given_up, ram_mb, cpu_pct });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Instance, LaunchTarget};

    // The notes in English (tests run in English).
    const HUNG: &str = "Not responding";
    const UNEXPECTED: &str = "Closed unexpectedly";

    const TOKEN: &str = "aB3_x-Yz09aB3_x-Yz09aB3_x-Yz09aB3_x-Yz09aB3_x-Yz09aB3_x-Yz09abcd";

    #[test]
    fn accepts_only_discord_webhooks() {
        for host in HOSTS {
            let raw = format!("  https://{host}/api/webhooks/123456789012345678/{TOKEN}  ");
            let h = parse_webhook(&raw).unwrap();
            assert_eq!(h.url.expose(), raw.trim(), "{host}");
            assert_eq!((h.id.as_str(), masked(&h.id).as_str()), ("123456789012345678", "…5678"));
        }
        // Hosts are compared after the parser lowercases them.
        assert!(parse_webhook(&format!("https://Discord.com/api/webhooks/1/{TOKEN}")).is_ok());
        for bad in [
            "",
            "discord.com/api/webhooks/1/abc",
            "http://discord.com/api/webhooks/1/abc",
            "https://discord.com.evil.example/api/webhooks/1/abc",
            "https://evil.example/discord.com/api/webhooks/1/abc",
            "https://media.discord.com/api/webhooks/1/abc",
            "https://discord.gg/api/webhooks/1/abc",
            "https://user@discord.com/api/webhooks/1/abc",
            "https://discord.com:8443/api/webhooks/1/abc",
            "https://discord.com/api/webhooks/1",
            "https://discord.com/api/webhooks/1/abc/extra",
            "https://discord.com/api/v10/webhooks/1/abc",
            "https://discord.com/api/webhooks/1/abc?wait=true",
            "https://discord.com/api/webhooks/1/abc#x",
            "https://discord.com/api/webhooks/12a/abc",
            "https://discord.com/api/webhooks/1/ab%20c",
            "https://discord.com/api/webhooks/1/ab.c",
            "https://discord.com/API/webhooks/1/abc",
        ] {
            assert!(parse_webhook(bad).is_err(), "{bad}");
        }
        assert_eq!(masked("12"), "…12");
    }

    #[test]
    fn hex_round_trip() {
        let blob = [0u8, 1, 0xab, 0xff];
        assert_eq!(hex(&blob), "0001abff");
        assert_eq!(unhex("0001abff").unwrap(), blob);
        assert_eq!(unhex("0001ab"), Some(vec![0, 1, 0xab]));
        assert!(unhex("abc").is_none() && unhex("zz").is_none());
    }

    fn inst(state: InstanceState, note: Option<&str>) -> Instance {
        Instance {
            id: "i".into(),
            account_id: 1,
            pid: 1,
            state,
            target: LaunchTarget::Place { place_id: 1818, launch_data: None },
            place_id: Some(1818),
            job_id: None,
            started_at: 0,
            ended_at: None,
            via_bootstrapper: false,
            note: note.map(Into::into),
            rejoins: 0,
            limits: Default::default(),
            anti_afk: true,
            afk_at: None,
            throttled: false,
            micro: false,
            fps_cap: None,
            relaunching: false,
            relaunch_problem: None,
            sign_in_problem: false,
            micro_size: None,
            applied_limits: crate::model::Limits::default(),
            held: false,
        }
    }

    #[test]
    fn tells_disconnects_hangs_and_crashes_once() {
        use InstanceState::*;
        let t = |b: Instance, a: Instance| change_of(&Transition { before: b, after: a, first_game: false, reason: None });
        assert_eq!(t(inst(InGame, None), inst(Disconnected, None)), Some(CrashKind::Disconnect));
        assert_eq!(t(inst(Disconnected, None), inst(Disconnected, Some(HUNG))), Some(CrashKind::Hang));
        assert_eq!(t(inst(InGame, None), inst(InGame, Some(HUNG))), Some(CrashKind::Hang));
        assert_eq!(t(inst(InGame, Some(HUNG)), inst(InGame, Some(HUNG))), None, "still hung");
        assert_eq!(t(inst(InGame, None), inst(Exited, Some(UNEXPECTED))), Some(CrashKind::Crash));
        assert_eq!(t(inst(Starting, None), inst(Exited, Some(UNEXPECTED))), Some(CrashKind::Crash));
        // Ended after a disconnect (already told), closed by the user or RoALT, replaced.
        assert_eq!(t(inst(Disconnected, None), inst(Exited, Some(UNEXPECTED))), None);
        assert_eq!(t(inst(InGame, None), inst(Exited, Some("Closed"))), None);
        assert_eq!(t(inst(InGame, Some(HUNG)), inst(Exited, Some("Closed"))), None);
        assert_eq!(t(inst(Joining, None), inst(Exited, Some("Replaced by another Roblox client"))), None);
        // Ordinary progress.
        assert_eq!(t(inst(Starting, None), inst(Joining, None)), None);
        assert_eq!(t(inst(Joining, None), inst(InGame, None)), None);
        assert_eq!(t(inst(Disconnected, None), inst(InGame, None)), None, "reconnected");

        // Getting into a game is a launch alert, once, and only for the first client.
        let l = |b: Instance, a: Instance| launched(&Transition { before: b, after: a, first_game: true, reason: None });
        assert!(l(inst(Joining, None), inst(InGame, None)));
        assert!(l(inst(Starting, None), inst(InGame, None)));
        assert!(!l(inst(InGame, None), inst(InGame, Some(HUNG))), "already in game");
        assert!(!l(inst(Disconnected, None), inst(InGame, None)), "reconnected");
        let mut rejoined = inst(InGame, None);
        rejoined.rejoins = 1;
        assert!(!l(inst(Joining, None), rejoined), "a rejoin or restart");
        assert!(!l(inst(Joining, None), inst(Exited, Some(UNEXPECTED))));
    }

    #[test]
    fn builds_one_embed_per_message() {
        let crash = Event::Crash { name: "Nova (@nova\\_builds)".into(), place_id: Some(1818), game: None };
        let one = message(std::slice::from_ref(&crash), "", PingLevel::Urgent);
        assert_eq!(one["username"], "RoALT");
        assert_eq!(one["allowed_mentions"]["parse"], json!([]));
        let embeds = one["embeds"].as_array().unwrap();
        assert_eq!(embeds.len(), 1);
        assert_eq!(embeds[0]["description"], "💥 **Nova (@nova\\_builds)**: the client crashed (place 1818).");
        assert_eq!(embeds[0]["color"], 0xE5484D);

        let rejoin = Event::Restart { name: "A".into(), cause: CrashKind::Disconnect };
        assert_eq!(message(std::slice::from_ref(&rejoin), "", PingLevel::Urgent)["embeds"][0]["color"], 0xB4F000);
        let batch = Event::BatchDone { total: 3, in_game: 1, done: 1, failed: 1 };
        assert_eq!(batch.line(), "🚀 Batch finished: 1 of 3 in game, 1 done, 1 failed.");
        let status = Event::Status { in_game: 2, disconnected: 0, given_up: 0, ram_mb: 1536.4, cpu_pct: Some(12.6) };
        assert_eq!(status.line(), "📊 2 clients running · 1536 MB RAM · 13% CPU.");
        let split = Event::Status { in_game: 8, disconnected: 2, given_up: 1, ram_mb: 4000.0, cpu_pct: None };
        assert_eq!(split.line(), "📊 8 in game, 2 disconnected, 1 gave up · 4000 MB RAM.");
        let zero = Event::Status { in_game: 0, disconnected: 0, given_up: 0, ram_mb: 0.0, cpu_pct: None };
        assert_eq!(zero.line(), "📊 0 clients running.");
        let launch = Event::Launch { name: "A".into(), place_id: Some(1818), game: None };
        assert_eq!(launch.line(), "🎮 **A**: in game (place 1818).");
        let no_cpu = Event::Status { in_game: 1, disconnected: 0, given_up: 0, ram_mb: 512.0, cpu_pct: None };
        assert_eq!(no_cpu.line(), "📊 1 client running · 512 MB RAM.");
        let gave_up = Event::GaveUp { name: "A".into(), reason: "its old client didn't close.".into() };
        assert_eq!(gave_up.line(), "⛔ **A**: its old client didn't close.");
        assert_eq!(gave_up.severity(), 2);

        // Many at once: the first 15 lines, then a count.
        let many: Vec<Event> = (0..20).map(|n| Event::Disconnect { name: format!("A{n}"), reason: None }).collect();
        let text = message(&many, "", PingLevel::Urgent)["embeds"][0]["description"].as_str().unwrap().to_owned();
        assert_eq!(text.lines().count(), 16);
        assert!(text.ends_with("…and 5 more.") && text.starts_with("🔌 **A0**"));
        assert_eq!(message(&many, "", PingLevel::Urgent)["embeds"][0]["color"], 0xFFA05C);
    }

    #[test]
    fn alerts_name_the_game_and_the_reason() {
        let crash = Event::Crash { name: "A".into(), place_id: Some(1818), game: Some("Sky Islands".into()) };
        assert_eq!(crash.line(), "💥 **A**: the client crashed in Sky Islands.");
        let launch = Event::Launch { name: "A".into(), place_id: Some(1818), game: Some("Sky Islands".into()) };
        assert_eq!(launch.line(), "🎮 **A**: in Sky Islands.");
        let idle = Event::Disconnect { name: "A".into(), reason: Some(reason_words("Sending disconnect with reason: 278")) };
        assert_eq!(idle.line(), "🔌 **A**: disconnected by Roblox (idle for 20 minutes).");
        assert_eq!(reason_words("Disconnection Notification. Reason: 267"), "kicked by the game");
        assert_eq!(reason_words("Lost connection with reason : 999"), "error 999");
        assert_eq!(reason_words("Connection closed by *server*"), "Connection closed by \\*server\\*", "markdown kept out");
        assert_eq!(reason_words(&"x".repeat(200)).len(), 80);

        let sign_in = Event::SignIn { name: "A".into(), note: "Roblox wants a human check".into() };
        assert_eq!(sign_in.line(), "🔐 **A**: Roblox wants a human check.");
        assert_eq!(sign_in.severity(), 1, "a sign-in problem pings");
        assert_eq!(Event::Throttled { name: "A".into() }.line(), "🐢 **A**: slowed down, it uses too much memory.");
        assert_eq!(Event::Update { version: "1.2.0".into() }.line(), "⬆️ RoALT 1.2.0 is out.");

        // On by default: sign-in trouble and updates. The throttle is news most don't want.
        let v = DiscordEvents::default();
        assert!(sign_in.wanted(&v) && Event::Update { version: "1.2.0".into() }.wanted(&v));
        assert!(!Event::Throttled { name: "A".into() }.wanted(&v));
    }

    #[test]
    fn something_lasting_is_told_once_until_it_stops() {
        let (mut s, t0, hold) = (Sustained::default(), Instant::now(), Duration::from_secs(60));
        let at = |secs| t0 + Duration::from_secs(secs);
        assert!(!s.step(true, at(0), hold) && !s.step(true, at(59), hold), "not for long enough yet");
        assert!(s.step(true, at(60), hold), "lasted");
        assert!(!s.step(true, at(70), hold) && !s.step(true, at(600), hold), "told once");
        assert!(!s.step(false, at(610), hold));
        assert!(!s.step(true, at(620), hold) && !s.step(true, at(679), hold), "counts from when it began again");
        assert!(s.step(true, at(680), hold), "and is told again");
        // A dip in between starts the count over.
        let mut s = Sustained::default();
        assert!(!s.step(true, at(0), hold) && !s.step(false, at(30), hold) && !s.step(true, at(61), hold));
        assert_eq!(Event::PcMemory { pct: 93, clients: 12 }.line(), "🧠 This PC's memory is 93% full (clients running: 12).");
        assert_eq!(Event::Restart { name: "A".into(), cause: CrashKind::Timed }.line(), "⏱️ **A**: restarted on its timer.");
        let v = DiscordEvents::default();
        assert!(!Event::PcGpu { pct: 99, clients: 1 }.wanted(&v), "opt-in");
    }

    #[test]
    fn parses_pings() {
        for (raw, want) in [("", ""), ("  ", ""), (" 123 ", "<@123>"), ("<@123>", "<@123>"), ("<@!123>", "<@123>"), ("<@&9>", "<@&9>")] {
            assert_eq!(parse_ping(raw).unwrap(), want, "{raw}");
        }
        for bad in ["abc", "<@&>", "<@>", "@everyone", "<@12a>", "<#123>", "<@123", "12 3", &"1".repeat(26)] {
            assert!(parse_ping(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn pings_only_when_something_went_wrong() {
        let crash = [Event::Crash { name: "A".into(), place_id: None, game: None }];
        let m = message(&crash, "<@123>", PingLevel::Urgent);
        assert_eq!(m["content"], "<@123>");
        assert_eq!(m["allowed_mentions"], json!({ "parse": [], "users": ["123"] }));
        let m = message(&crash, "<@&9>", PingLevel::Urgent);
        assert_eq!(m["allowed_mentions"], json!({ "parse": [], "roles": ["9"] }));
        // News and no ping: no mention at all.
        let news = [Event::Launch { name: "A".into(), place_id: None, game: None }, Event::Restart { name: "A".into(), cause: CrashKind::Crash }];
        assert_eq!(message(&news, "<@123>", PingLevel::Warnings).get("content"), None);
        assert_eq!(
            message(&crash, "", PingLevel::Urgent),
            json!({ "username": "RoALT", "allowed_mentions": { "parse": [] }, "embeds": message(&crash, "<@1>", PingLevel::Urgent)["embeds"] })
        );
        // A disconnect (severity 1) doesn't ping on Urgent (default), but does on Warnings.
        let mixed = [news[0].clone(), Event::Disconnect { name: "B".into(), reason: None }];
        assert_eq!(message(&mixed, "<@123>", PingLevel::Urgent).get("content"), None);
        assert_eq!(message(&mixed, "<@123>", PingLevel::Warnings)["content"], "<@123>");
    }

    #[test]
    fn names_escape_markdown_and_mentions() {
        let a = AccountSummary {
            id: 1,
            user_id: 2,
            username: "nova_builds".into(),
            display_name: "*Nova* @everyone".into(),
            alias: "secret alias".into(),
            notes: String::new(),
            color: None,
            favourite: false,
            sort_order: 0,
            group_ids: vec![],
            status: crate::model::AccountStatus::Valid,
            status_checked_at: None,
            created_at: 0,
            allow_disconnect: false,
            launch_args: None,
            discord_muted: false,
            full_quality: false,
            main: false,
            custom_flags: Default::default(),
            client_limits: None,
            profile: None,
            fps_cap: None,

        };
        assert_eq!(name_of(Some(&a), 1), "\\*Nova\\* \\@everyone (\\@nova\\_builds)");
        assert_eq!(name_of(None, 7), "Account 7");
    }

    #[test]
    fn toggles_pick_events() {
        let mut v = DiscordEvents::default();
        let mem = Event::Restart { name: "A".into(), cause: CrashKind::Mem };
        let crash_restart = Event::Restart { name: "A".into(), cause: CrashKind::Crash };
        let status = Event::Status { in_game: 1, disconnected: 0, given_up: 0, ram_mb: 1.0, cpu_pct: Some(1.0) };
        let launch = Event::Launch { name: "A".into(), place_id: None, game: None };
        assert!(mem.wanted(&v) && crash_restart.wanted(&v) && !status.wanted(&v) && !launch.wanted(&v));
        v.launch = true;
        assert!(launch.wanted(&v));
        v.mem_restart = false;
        assert!(!mem.wanted(&v) && crash_restart.wanted(&v));
        v.status_every_min = 30;
        assert!(status.wanted(&v));
    }

    #[test]
    fn coalesces_and_waits_between_messages() {
        let t0 = Instant::now();
        let ev = |n: usize| Event::Disconnect { name: format!("A{n}"), reason: None };
        let mut o = Outbox::default();
        assert!(o.take(t0).is_empty());
        o.push(ev(0));
        o.push(ev(1));
        assert_eq!(o.take(t0), [ev(0), ev(1)], "everything queued goes in one message");
        o.sent(t0);
        o.push(ev(2));
        assert!(o.take(t0 + Duration::from_millis(1_999)).is_empty(), "at most one message per 2 s");
        o.push(ev(3));
        assert_eq!(o.take(t0 + GAP), [ev(2), ev(3)]);

        // A 429: the same events go again, first, after Discord's wait (never under 2 s).
        o.push(ev(4));
        o.retry(vec![ev(2), ev(3)], t0, Duration::from_secs(7));
        assert!(o.take(t0 + Duration::from_secs(6)).is_empty());
        o.sent(t0 + Duration::from_secs(1));
        assert!(o.take(t0 + Duration::from_secs(6)).is_empty(), "a test message doesn't cut Discord's wait short");
        assert_eq!(o.take(t0 + Duration::from_secs(7)), [ev(2), ev(3), ev(4)]);
        o.retry(vec![ev(5)], t0, Duration::ZERO);
        assert!(o.take(t0 + Duration::from_millis(1_999)).is_empty());

        // A flood keeps the newest 50.
        let mut o = Outbox::default();
        (0..60).for_each(|n| o.push(ev(n)));
        let out = o.take(t0);
        assert_eq!((out.len(), out.first(), out.last()), (MAX_QUEUED, Some(&ev(10)), Some(&ev(59))));
        // Network retries back off exponentially and drop after MAX_RETRIES.
        let mut o = Outbox::default();
        let waits: Vec<Option<u64>> = (0..=MAX_RETRIES).map(|_| o.network_retry(vec![ev(1)], t0).map(|d| d.as_secs())).collect();
        assert_eq!(waits, [Some(30), Some(60), Some(120), Some(240), Some(300), Some(300), None]);
    }

    #[test]
    fn honours_retry_after() {
        assert_eq!(retry_after(r#"{"message":"You are being rate limited.","retry_after":3.5,"global":false}"#, None), Duration::from_millis(3_500));
        assert_eq!(retry_after("", Some("9")), Duration::from_secs(9));
        assert_eq!(retry_after(r#"{"retry_after":0.2}"#, Some("9")), GAP, "never under 2 s");
        assert_eq!(retry_after("not json", None), Duration::from_secs(5));
        assert_eq!(retry_after(r#"{"retry_after":99999}"#, None), Duration::from_secs(600));
        assert_eq!(retry_after(r#"{"retry_after":-1}"#, Some("x")), Duration::from_secs(5));
    }
}
