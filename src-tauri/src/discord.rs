//! Discord alerts (opt-in): what happens to the clients, posted to a webhook the user pastes.
//! The only host besides Roblox RoALT talks to, and only once a webhook is set.
//!
//! The webhook URL holds its token, so it is a secret: kept in the vault (AES-GCM, `DISCORD_AAD`)
//! as hex in the settings table, never logged, never sent to the UI (only its id's last digits).
//! Messages go out only while the vault is unlocked. Events (`Settings.discord` picks them) are
//! coalesced: at most one message every 2 s, carrying everything queued since; a 429 is waited
//! out as Discord says (`retry_after`). Messages name accounts by display name and username,
//! and never carry a cookie, ticket, launch link or log line.

use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use reqwest::StatusCode;
use reqwest::header::RETRY_AFTER;
use serde_json::{Value, json};
use tauri::{AppHandle, Manager};

use crate::App;
use crate::error::{AppError, Result};
use crate::instances::{Transition, hung, unexpected};
use crate::model::{AccountSummary, BulkProgress, BulkStage, CrashKind, DiscordEvents, DiscordStatus, InstanceState};
use crate::vault::Secret;

const DISCORD_AAD: &[u8] = b"roalt.discord.webhook";
/// The settings-table row holding the encrypted webhook (hex).
const KEY: &str = "discord_webhook";
/// At most one message this often.
const GAP: Duration = Duration::from_secs(2);
/// Events waiting beyond this many drop the oldest.
const MAX_QUEUED: usize = 50;
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

/// What the UI shows of a webhook id: its last 4 digits.
fn masked(id: &str) -> String {
    format!("…{}", &id[id.len().saturating_sub(4)..])
}

/// Something worth telling the user about.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    Crash { name: String, place_id: Option<i64> },
    Hang { name: String },
    Disconnect { name: String },
    /// A client first got into a game.
    Launch { name: String, place_id: Option<i64> },
    /// An auto-rejoin (`Disconnect`) or auto-restart went ahead.
    Restart { name: String, cause: CrashKind },
    /// The crash breaker gave up on the account.
    Breaker { name: String, count: u32, window_min: u32 },
    BatchDone { total: usize, in_game: usize, done: usize, failed: usize, stopped: Option<String> },
    Status { clients: usize, ram_mb: f64, cpu_pct: Option<f64> },
}

impl Event {
    fn wanted(&self, v: &DiscordEvents) -> bool {
        match self {
            Self::Crash { .. } => v.crash,
            Self::Hang { .. } => v.hang,
            Self::Disconnect { .. } => v.disconnect,
            Self::Launch { .. } => v.launch,
            Self::Restart { cause: CrashKind::Mem, .. } => v.mem_restart,
            Self::Restart { .. } => v.restart,
            Self::Breaker { .. } => v.breaker,
            Self::BatchDone { .. } => v.batch_done,
            Self::Status { .. } => v.status_every_min > 0,
        }
    }

    /// 2: bad, 1: worth a look, 0: news.
    fn severity(&self) -> u8 {
        match self {
            Self::Crash { .. } | Self::Breaker { .. } => 2,
            Self::Hang { .. } | Self::Disconnect { .. } => 1,
            Self::BatchDone { failed, .. } if *failed > 0 => 1,
            _ => 0,
        }
    }

    fn line(&self) -> String {
        match self {
            Self::Crash { name, place_id: Some(p) } => {
                format!("💥 {}", tr!("**{name}**: the client crashed (place {place}).", name = name, place = p))
            }
            Self::Crash { name, place_id: None } => format!("💥 {}", tr!("**{name}**: the client crashed.", name = name)),
            Self::Hang { name } => format!("🧊 {}", tr!("**{name}**: the client stopped responding.", name = name)),
            Self::Disconnect { name } => format!("🔌 {}", tr!("**{name}**: disconnected by Roblox.", name = name)),
            Self::Launch { name, place_id: Some(p) } => {
                format!("🎮 {}", tr!("**{name}**: in game (place {place}).", name = name, place = p))
            }
            Self::Launch { name, place_id: None } => format!("🎮 {}", tr!("**{name}**: in game.", name = name)),
            Self::Restart { name, cause: CrashKind::Disconnect } => format!("🔁 {}", tr!("**{name}**: rejoined.", name = name)),
            Self::Restart { name, cause: CrashKind::Mem } => {
                format!("🧹 {}", tr!("**{name}**: restarted, it used too much memory.", name = name))
            }
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
            Self::BatchDone { total, in_game, done, failed, stopped } => {
                let mut s = format!("🚀 {}", tr!("Batch finished: {in_game} of {total} in game", in_game = in_game, total = total));
                if *done > 0 {
                    s += &tr!(", {done} done", done = done);
                }
                if *failed > 0 {
                    s += &tr!(", {failed} failed", failed = failed);
                }
                if let Some(why) = stopped {
                    s += &tr!(" (stopped: {why})", why = escape(why));
                }
                s + "."
            }
            Self::Status { clients, ram_mb, cpu_pct } => {
                let cpu = cpu_pct.map(|c| format!(" · {c:.0}% CPU")).unwrap_or_default();
                format!(
                    "📊 {}",
                    trn!(
                        *clients,
                        "{n} client running · {ram} MB RAM{cpu}.",
                        "{n} clients running · {ram} MB RAM{cpu}.",
                        ram = format!("{ram_mb:.0}"),
                        cpu = cpu
                    )
                )
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

/// The webhook body for some events (pure): one embed, one line each.
pub fn message(events: &[Event]) -> Value {
    let mut lines: Vec<String> = events.iter().take(MAX_LINES).map(Event::line).collect();
    if events.len() > MAX_LINES {
        lines.push(trn!(events.len() - MAX_LINES, "…and {n} more.", "…and {n} more."));
    }
    let color = match events.iter().map(Event::severity).max() {
        Some(2) => 0xE5_48_4D,
        Some(1) => 0xFF_A0_5C,
        _ => 0xB4_F0_00,
    };
    json!({
        "username": "RoALT",
        "allowed_mentions": { "parse": [] },
        "embeds": [{ "title": "RoALT", "description": lines.join("\n"), "color": color }],
    })
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
}

pub struct Discord {
    outbox: Mutex<Outbox>,
    http: std::result::Result<reqwest::Client, String>,
    /// When the last status summary was queued.
    last_status: Mutex<Option<Instant>>,
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
        Discord { outbox: Mutex::default(), http, last_status: Mutex::new(None) }
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
            log::info!("discord webhook set (id ending {})", masked(&hook.id));
        }
        None => {
            s.db.set_setting_value(KEY, None)?;
            s.discord.outbox().queue.clear();
            log::info!("discord webhook removed");
        }
    }
    status(s)
}

/// Sends a test message at once (it still counts for the 2 s gap).
pub async fn test(s: &App) -> Result<()> {
    let hook = webhook(s)?.ok_or_else(|| AppError::Invalid(tr!("Set a webhook first.")))?;
    let body = json!({
        "username": "RoALT",
        "allowed_mentions": { "parse": [] },
        "embeds": [{ "title": "RoALT", "description": format!("✅ {}", tr!("Test message: RoALT's alerts reach this channel.")), "color": 0xB4_F0_00 }],
    });
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
    let r = http
        .post(hook.url.expose())
        .json(body)
        .send()
        .await
        .map_err(|e| AppError::Network(e.without_url().to_string()))?;
    match r.status() {
        st if st.is_success() => Ok(None),
        StatusCode::TOO_MANY_REQUESTS => {
            let header = r.headers().get(RETRY_AFTER).and_then(|v| v.to_str().ok()).map(str::to_owned);
            Ok(Some(retry_after(&r.text().await.unwrap_or_default(), header.as_deref())))
        }
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND => {
            Err(AppError::Invalid(tr!("Discord refused the webhook; it may have been deleted. Paste a new one.")))
        }
        st => Err(AppError::Network(tr!("Discord answered {status}.", status = st.as_u16()))),
    }
}

/// Queues an event when its toggle is on, a webhook is set and the vault is unlocked.
pub fn notify(app: &AppHandle, event: Event) {
    let s = app.state::<App>();
    let wanted = s
        .db
        .settings()
        .inspect_err(|e| log::warn!("discord: event dropped, settings unreadable: {e}"))
        .is_ok_and(|v| event.wanted(&v.discord));
    if wanted
        && !s.vault.is_locked()
        && s.db
            .setting_value(KEY)
            .inspect_err(|e| log::warn!("discord: event dropped, webhook setting unreadable: {e}"))
            .is_ok_and(|v| v.is_some())
    {
        s.discord.outbox().push(event);
    }
}

/// The tracker's poll saw these changes. Muted accounts (`discord_muted`) are left out.
pub fn transitions(app: &AppHandle, moved: &[Transition]) {
    for t in moved {
        let kind = change_of(t);
        if kind.is_none() && !launched(t) {
            continue;
        }
        let a = app
            .state::<App>()
            .db
            .account(t.after.account_id)
            .inspect_err(|e| log::debug!("discord: account {} unreadable: {e}", t.after.account_id))
            .ok();
        if a.as_ref().is_some_and(|a| a.discord_muted) {
            continue;
        }
        let name = name_of(a.as_ref(), t.after.account_id);
        let place_id = t.after.place_id;
        notify(
            app,
            match kind {
                None => Event::Launch { name, place_id },
                Some(CrashKind::Disconnect) => Event::Disconnect { name },
                Some(CrashKind::Hang) => Event::Hang { name },
                Some(_) => Event::Crash { name, place_id },
            },
        );
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
            // Its text names accounts by alias, and also muted ones.
            stopped: None,
        },
    );
}

/// Runs for the app's lifetime: queues the periodic status, sends what is queued.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            queue_status(&app).await;
            let s = app.state::<App>();
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
    let now = Instant::now();
    match post(s, &hook, &message(&events)).await {
        Ok(None) => s.discord.outbox().sent(now),
        Ok(Some(wait)) => {
            log::info!("discord: rate limited, waiting {} s", wait.as_secs());
            s.discord.outbox().retry(events, now, wait);
        }
        // An outage is when the alerts matter most: they go when it's over.
        Err(AppError::Network(e)) => {
            log::warn!("discord: message not sent, trying again: {e}");
            s.discord.outbox().retry(events, now, Duration::from_secs(30));
        }
        Err(e) => {
            log::warn!("discord: message not sent: {e}");
            s.discord.outbox().sent(now);
        }
    }
}

/// Every `status_every_min` minutes while clients run: how many, their RAM and CPU.
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
    if !m.is_empty() {
        let ram_mb = m.iter().map(|c| c.memory_mb).sum();
        // CPU needs an earlier sample of the same client; without one it's left out, not 0%.
        let cpu: Vec<f64> = m.iter().filter_map(|c| c.cpu_pct).collect();
        let cpu_pct = (!cpu.is_empty()).then(|| cpu.iter().sum());
        notify(app, Event::Status { clients: m.len(), ram_mb, cpu_pct });
    }
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
        }
    }

    #[test]
    fn tells_disconnects_hangs_and_crashes_once() {
        use InstanceState::*;
        let t = |b: Instance, a: Instance| change_of(&Transition { before: b, after: a, first_game: false });
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
        let l = |b: Instance, a: Instance| launched(&Transition { before: b, after: a, first_game: true });
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
        let crash = Event::Crash { name: "Nova (@nova\\_builds)".into(), place_id: Some(1818) };
        let one = message(std::slice::from_ref(&crash));
        assert_eq!(one["username"], "RoALT");
        assert_eq!(one["allowed_mentions"]["parse"], json!([]));
        let embeds = one["embeds"].as_array().unwrap();
        assert_eq!(embeds.len(), 1);
        assert_eq!(embeds[0]["description"], "💥 **Nova (@nova\\_builds)**: the client crashed (place 1818).");
        assert_eq!(embeds[0]["color"], 0xE5484D);

        let rejoin = Event::Restart { name: "A".into(), cause: CrashKind::Disconnect };
        assert_eq!(message(std::slice::from_ref(&rejoin))["embeds"][0]["color"], 0xB4F000);
        let batch = Event::BatchDone { total: 3, in_game: 1, done: 1, failed: 1, stopped: Some("Cancelled".into()) };
        assert_eq!(batch.line(), "🚀 Batch finished: 1 of 3 in game, 1 done, 1 failed (stopped: Cancelled).");
        let status = Event::Status { clients: 2, ram_mb: 1536.4, cpu_pct: Some(12.6) };
        assert_eq!(status.line(), "📊 2 clients running · 1536 MB RAM · 13% CPU.");
        let launch = Event::Launch { name: "A".into(), place_id: Some(1818) };
        assert_eq!(launch.line(), "🎮 **A**: in game (place 1818).");
        let no_cpu = Event::Status { clients: 1, ram_mb: 512.0, cpu_pct: None };
        assert_eq!(no_cpu.line(), "📊 1 client running · 512 MB RAM.");

        // Many at once: the first 15 lines, then a count.
        let many: Vec<Event> = (0..20).map(|n| Event::Disconnect { name: format!("A{n}") }).collect();
        let text = message(&many)["embeds"][0]["description"].as_str().unwrap().to_owned();
        assert_eq!(text.lines().count(), 16);
        assert!(text.ends_with("…and 5 more.") && text.starts_with("🔌 **A0**"));
        assert_eq!(message(&many)["embeds"][0]["color"], 0xFFA05C);
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
        let status = Event::Status { clients: 1, ram_mb: 1.0, cpu_pct: Some(1.0) };
        let launch = Event::Launch { name: "A".into(), place_id: None };
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
        let ev = |n: usize| Event::Disconnect { name: format!("A{n}") };
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
