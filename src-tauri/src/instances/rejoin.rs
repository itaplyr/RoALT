//! Auto-rejoin and auto-restart: the tracker loop hands over clients Roblox disconnected, that
//! crashed, hung or stayed over the memory limit (`Tracker::take_rejoins`). With
//! `settings.auto_rejoin` (disconnects), `settings.auto_restart_on_crash` (crashes, hangs) or
//! `settings.mem_restart_mb` (memory) on, each still open is closed and the account launched again into the same target after a backoff
//! (`launcher::bulk::jittered(backoff(settings.launch_delay_ms, recent))`), within the crash
//! breaker: at most `settings.breaker_max` relaunches of one account per
//! `settings.breaker_window_min` minutes. A client stays open until the launch looks possible, and
//! what may pass by itself (no network, Roblox down, a batch, a locked vault) is tried again for
//! `RETRY_FOR`: a short outage disconnects every client at once, and must not end them all.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use super::tracker::Rejoin;
use crate::App;
use crate::commands::{self, notice};
use crate::error::AppError;
use crate::launcher::bulk::{jittered, name_of};
use crate::model::{AccountStatus, AccountSummary, CrashKind, Instance, InstanceState, LaunchTarget, Settings};

/// Rejoins take turns: launches go one at a time, each after its own delay.
static TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The crash breaker's memory: when each account was last relaunched (Unix ms), oldest first,
/// persisted in `settings` under `RECENT_KEY` so restarting RoALT does not reset the breaker.
/// Changed only while holding `TURN`.
const RECENT_KEY: &str = "rejoin_recent";
/// Upper bound on how long persisted breaker entries are kept (24 hours).
const RECENT_KEEP_MS: i64 = 24 * 60 * 60 * 1000;

struct RecentStore {
    loaded: bool,
    entries: Vec<(i64, VecDeque<i64>)>,
}

static RECENT: Mutex<RecentStore> = Mutex::new(RecentStore { loaded: false, entries: Vec::new() });

/// How long a relaunch that can't go ahead yet is tried again.
const RETRY_FOR: Duration = Duration::from_secs(60 * 60);
/// Wakes the relaunches waiting to try again (`retry_now`).
static RETRY_NOW: tokio::sync::Notify = tokio::sync::Notify::const_new();
/// One `launcher::launch_check` at a time, outside `TURN`, so a slow network holds up no launches.
static CHECK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
/// When clients waiting for that were last mentioned, and how often: an outage holds up all of them.
static WAITING_SAID: Mutex<Option<Instant>> = Mutex::new(None);
const WAITING_SAY_EVERY: Duration = Duration::from_secs(5 * 60);
/// Once per run, since closing Roblox's window by hand looks like a crash too.
static CRASH_HINT_SAID: AtomicBool = AtomicBool::new(false);

/// Called by the tracker loop after each poll with what `take_rejoins` returned. Drops those
/// whose setting is off (`enabled`) or whose account allows disconnects, silently. Otherwise,
/// per rejoin, on the async runtime: wait the backoff, then go through `attempt`, again after
/// `retry_wait` for as long as it says the problem may pass (at most `RETRY_FOR`, then note "Gave
/// up after ..." and an error notice). A crash with only auto-rejoin on is said once per run.
pub fn handle(app: &AppHandle, rejoins: Vec<Rejoin>) {
    if rejoins.is_empty() {
        return;
    }
    let s = app.state::<App>();
    let v = match s.db.settings() {
        Ok(v) => v,
        Err(e) => {
            log::error!("auto-rejoin/restart: could not read settings, {} pending dropped: {e}", rejoins.len());
            return;
        }
    };
    let (on, off): (Vec<Rejoin>, Vec<Rejoin>) = rejoins.into_iter().partition(|r| wanted(&s, &v, r));
    if v.auto_rejoin
        && off.iter().any(|r| r.cause == CrashKind::Crash && s.db.effective(r.account_id).is_ok_and(|a| !a.allow_disconnect))
        && !CRASH_HINT_SAID.swap(true, Ordering::Relaxed)
    {
        notice(app, true, tr!("A client closed unexpectedly and wasn't relaunched: {rejoin} only covers disconnects. To relaunch crashed clients too, turn on {restart} in Settings.", rejoin = tr!("Auto-rejoin"), restart = tr!("Auto-restart after a crash")));
    }
    for r in on {
        // Who the account is now, so an id removed and reused during the wait is told apart.
        let account = s
            .db
            .effective(r.account_id)
            .inspect_err(|e| {
                if !matches!(e, crate::error::AppError::NotFound) {
                    log::warn!("auto-rejoin: could not read account {}: {e}", r.account_id);
                }
            })
            .ok();
        if account.as_ref().is_some_and(|a| a.allow_disconnect) {
            continue;
        }
        // One is at work on it already (the client recovered for a moment and got into trouble again).
        if !s.tracker.begin_relaunch(&r.from, commands::now_ms()) {
            continue;
        }
        let user_id = account.map(|a| a.user_id);
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let from = r.from.clone();
            rejoin(&app, r, user_id).await;
            app.state::<App>().tracker.end_relaunch(&from);
            commands::instances_changed(&app);
        });
    }
}

/// Relaunches waiting to try again do so now, say once the network is back.
pub fn retry_now() {
    RETRY_NOW.notify_waiters();
}

/// `enabled` for `r`, under its account's own performance profile (it can set the memory limit).
fn wanted(s: &App, v: &Settings, r: &Rejoin) -> bool {
    let own = s.db.account(r.account_id).ok().and_then(|a| a.profile);
    enabled(&crate::performance::overlay(v, own.as_ref()), r.cause)
}

/// Disconnects follow auto-rejoin; crashes and hangs auto-restart; memory the memory limit.
fn enabled(v: &Settings, cause: CrashKind) -> bool {
    match cause {
        CrashKind::Disconnect => v.auto_rejoin,
        CrashKind::Crash | CrashKind::Hang => v.auto_restart_on_crash,
        CrashKind::Mem => v.mem_restart_mb > 0,
        CrashKind::Timed => v.restart_every_h > 0,
    }
}

async fn rejoin(app: &AppHandle, r: Rejoin, user_id: Option<i64>) {
    let s = app.state::<App>();
    let v = s.db.settings().unwrap_or_else(|e| {
        log::warn!("auto-rejoin of instance {}: could not read settings, using defaults: {e}", r.from);
        Settings::default()
    });
    let window_ms = i64::from(v.breaker_window_min) * 60_000;
    let n = with_recent(&s.db, r.account_id, |t| recent_ms(t, commands::now_ms(), window_ms));
    let (kills, since) = (s.bulk.kills(), Instant::now());
    let (mut closed, mut tries) = (false, 0);
    // The waits are outside `attempt`'s turn, so one account's don't hold up the rest.
    let mut wait = jittered(backoff(v.launch_delay_ms, n));
    let mut press = RETRY_NOW.notified();
    loop {
        // Ok: `retry_now`; Err: the wait is over.
        let _ = tokio::time::timeout(wait, press).await;
        // From here: a press during the attempt counts for the wait after it.
        press = RETRY_NOW.notified();
        let Some(why) = attempt(app, &s, &r, user_id, kills, &mut closed).await else { return };
        s.tracker.set_relaunch_problem(&r.from, Some(why.clone()));
        commands::instances_changed(app);
        let again = if r.cause == CrashKind::Disconnect { "rejoin" } else { "restart" };
        log::info!("auto-{again} of instance {} waits (try {}): {why}", r.from, tries + 1);
        if since.elapsed() >= RETRY_FOR {
            let minutes = RETRY_FOR.as_secs() / 60;
            give_up(&s, &r, closed, tr!("Gave up after {minutes} minutes of trying", minutes = minutes));
            commands::instances_changed(app);
            let acct = s.db.effective(r.account_id).ok();
            let name = acct.as_ref().map_or_else(|| tr!("An account"), name_of);
            let text = if r.cause == CrashKind::Disconnect {
                tr!("{name} wasn't rejoined after {minutes} minutes of trying: {why}", name = name, minutes = minutes, why = why)
            } else {
                tr!("{name} wasn't restarted after {minutes} minutes of trying: {why}", name = name, minutes = minutes, why = why)
            };
            notice(app, true, text);
            discord_gave_up(app, acct.as_ref(), r.account_id, tr!("gave up after {minutes} minutes: {why}", minutes = minutes, why = why));
            return;
        }
        if say_waiting(Instant::now()) {
            notice(app, false, tr!("Waiting to relaunch clients. RoALT keeps trying for up to {minutes} minutes. Last problem: {why}", minutes = RETRY_FOR.as_secs() / 60, why = why));
        }
        wait = retry_wait(tries);
        tries += 1;
    }
}

/// `Tracker::give_up`, with the note shown also when RoALT itself closed the client (`closed`).
fn give_up(s: &App, r: &Rejoin, closed: bool, note: String) {
    if closed {
        s.tracker.clear_closing(&r.from);
    }
    s.tracker.give_up(&r.from, note);
}

/// One go at the relaunch; `Some(why)` when it may work later, None when it is over one way or
/// another. It is dropped silently when Kill all was pressed or it is no longer wanted
/// (`Tracker::relaunch_wanted`). A public network waits. A client still open stays so until
/// `launcher::update::update_hold` and `launcher::launch_check` pass; both go before the turn,
/// so waiting on the network holds up no launches. Then, in its turn and in flight
/// (`Bulk::begin_rejoin`), whether it is still wanted is asked again; skip it (`skip_reason`;
/// logged, plus an `app://notice`, unless it may pass); give up (note "Gave up after N ...", error
/// notice) when the breaker trips; close the client (graceful for `close_grace_s`, then forced;
/// marked as closed by RoALT); `commands::launch_tracked(account, target, the old
/// client's FPS cap)` and `tracker.set_rejoins(new id, r.rejoins)`. Only a launch that happened
/// counts for the breaker. `closed`: the old client was closed by an earlier go too.
async fn attempt(app: &AppHandle, s: &App, r: &Rejoin, user_id: Option<i64>, kills: u64, closed: &mut bool) -> Option<String> {
    let v = s.db.settings().unwrap_or_else(|e| {
        log::warn!("auto-rejoin of instance {}: could not read settings, using defaults: {e}", r.from);
        Settings::default()
    });
    let window_ms = i64::from(v.breaker_window_min) * 60_000;
    // Kill all, Close all, cleared, or back in a game (the player pressed Reconnect), responding
    // again or closed by the user: nothing to do.
    let over = |closed: bool| s.bulk.kills() != kills || !s.tracker.relaunch_wanted(&r.from, closed);
    if over(*closed) {
        return None;
    }
    // The client may have recovered and got into other trouble during the wait: its setting counts.
    let r = &Rejoin { cause: if *closed { r.cause } else { s.tracker.cause_of(&r.from).unwrap_or(r.cause) }, ..r.clone() };
    let account = s
        .db
        .effective(r.account_id)
        .inspect_err(|e| {
            if !matches!(e, crate::error::AppError::NotFound) {
                log::warn!("auto-rejoin: could not read account {}: {e}", r.account_id);
            }
        })
        .ok()
        .filter(|a| Some(a.user_id) == user_id);
    let name = account.as_ref().map_or_else(|| tr!("An account"), name_of);
    let rejoin = r.cause == CrashKind::Disconnect;
    let again = if rejoin { "rejoins" } else { "restarts" };
    if let Some(why) = crate::network::blocked(s) {
        return Some(why);
    }
    // Target for this relaunch: after a 288 shutdown, failed join, or repeat rejoin, a fixed
    // Server job id falls back to its Place so RoALT doesn't loop on a dead server.
    let target = effective_target(&r.target, r.fallback_place);
    // Before anything is closed: the relaunch would be refused, and the old client gone for nothing.
    let open = |s: &App| s.tracker.get(&r.from).is_some_and(|i| i.state != InstanceState::Exited);
    if open(s) && account.is_some() {
        let _one = CHECK.lock().await;
        if over(*closed) {
            return None;
        }
        // All those waiting go anyway, so they don't wait for each other.
        let hold = crate::launcher::update::update_hold(s, s.tracker.relaunches_open()).await;
        if over(*closed) {
            return None;
        }
        if hold.is_some() {
            return hold;
        }
        crate::launcher::update::publish(app);
        let check = crate::launcher::launch_check(s, r.account_id, &target).await;
        if over(*closed) {
            return None;
        }
        if let Err(e) = check {
            if let Err(e) = commands::record_cookie_error(app, s, r.account_id, &e) {
                log::warn!("auto-{again}: could not record account {}'s cookie problem: {e}", r.account_id);
            }
            return failed(app, account.as_ref(), r.account_id, &name, rejoin, &e);
        }
    }
    let _turn = TURN.lock().await;
    // Until the launch is done, batches and single launches wait (one at a time).
    // A Roblox install waits out first: after `busy` is set, one starting now sees this rejoin.
    let wait_until = Instant::now() + crate::launcher::update::INSTALL_TIMEOUT;
    let busy = loop {
        let busy = s.bulk.begin_rejoin();
        if busy.is_none() || !s.update.installing() || s.bulk.kills() != kills || Instant::now() > wait_until {
            break busy;
        }
        drop(busy);
        tokio::time::sleep(Duration::from_secs(1)).await;
    };
    // Things may have moved during those waits; from here to the close nothing waits.
    if over(*closed) {
        return None;
    }
    if s.update.installing() {
        return Some(commands::installing());
    }
    if let Some((why, passes, notify_discord)) = skip_reason(s, r, account.as_ref(), busy.is_none()) {
        if passes {
            return Some(format!("{why}."));
        }
        log::info!("auto-{again} of instance {} skipped: {why}", r.from);
        let text = if rejoin {
            tr!("{name} wasn't rejoined: {why}.", name = name, why = why)
        } else {
            tr!("{name} wasn't restarted: {why}.", name = name, why = why)
        };
        notice(app, false, text);
        if notify_discord {
            discord_gave_up(app, account.as_ref(), r.account_id, why);
        }
        return None;
    }
    let n = with_recent(&s.db, r.account_id, |t| recent_ms(t, commands::now_ms(), window_ms));
    if n >= v.breaker_max {
        log::info!("instance {}: crash breaker tripped ({n} relaunches in {} min)", r.from, v.breaker_window_min);
        let (note, text) = if rejoin {
            (
                trn!(n, "Gave up after {n} rejoin", "Gave up after {n} rejoins"),
                trn!(
                    n,
                    "{name} wasn't rejoined: it needed {n} rejoin in {minutes} minutes, so RoALT gave up.",
                    "{name} wasn't rejoined: it needed {n} rejoins in {minutes} minutes, so RoALT gave up.",
                    name = name,
                    minutes = v.breaker_window_min
                ),
            )
        } else {
            (
                trn!(n, "Gave up after {n} restart", "Gave up after {n} restarts"),
                trn!(
                    n,
                    "{name} wasn't restarted: it needed {n} restart in {minutes} minutes, so RoALT gave up.",
                    "{name} wasn't restarted: it needed {n} restarts in {minutes} minutes, so RoALT gave up.",
                    name = name,
                    minutes = v.breaker_window_min
                ),
            )
        };
        give_up(s, r, *closed, note);
        commands::instances_changed(app);
        notice(app, true, text);
        if !account.as_ref().is_some_and(|a| a.discord_muted) {
            let name = crate::discord::name_of(account.as_ref(), r.account_id);
            crate::discord::notify(app, crate::discord::Event::Breaker { name, count: n, window_min: v.breaker_window_min });
        }
        return None;
    }
    *closed |= open(s);
    // The client stuck on its disconnect dialog or hung; its exit is ours, not unexpected.
    if let Some((pid, started)) = s.tracker.mark_restart(&r.from, Some(r.cause)) {
        let grace = Duration::from_secs(u64::from(v.close_grace_s));
        let ended =
            tauri::async_runtime::spawn_blocking(move || crate::instances::process::close(pid, started, false, grace)).await;
        if let Ok(Err(e)) | Err(e) = ended.map_err(|e| crate::error::AppError::Internal(e.to_string())) {
            log::warn!("auto-{again}: closing instance {} failed: {e}", r.from);
            s.tracker.clear_closing(&r.from);
            *closed = false;
            let text = if rejoin {
                tr!("{name} couldn't be rejoined: its old client didn't close.", name = name)
            } else {
                tr!("{name} couldn't be restarted: its old client didn't close.", name = name)
            };
            notice(app, true, text);
            discord_gave_up(app, account.as_ref(), r.account_id, tr!("its old client didn't close"));
            return None;
        }
        // A hung client is still live in the tracker until the next poll ends it; launching
        // before that would be refused as "already running".
        let deadline = Instant::now() + Duration::from_secs(5);
        while s.tracker.get(&r.from).is_some_and(|i| i.state != InstanceState::Exited) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        if s.tracker.get(&r.from).is_some_and(|i| i.state != InstanceState::Exited) {
            log::debug!("auto-{again}: instance {} still not exited after 5 s; launching anyway", r.from);
        }
    }
    // Kill all or Close all pressed while the old client was closing.
    if over(*closed) {
        log::debug!("auto-{again} of instance {} dropped: Kill all or Close all was pressed", r.from);
        return None;
    }
    // The launch's own FPS cap carries over.
    let fps_cap = s.tracker.get(&r.from).and_then(|i| i.fps_cap);
    match commands::launch_tracked(app, s, r.account_id, target, None, fps_cap, !*closed, None).await {
        // Close all pressed during the launch: this client goes too.
        Ok((_, inst)) if !s.tracker.is_relaunching(&r.from) => {
            log::debug!("auto-{again} of instance {} dropped during the launch: closing {}", r.from, inst.id);
            if let Some((pid, started)) = s.tracker.mark_closing(&inst.id) {
                match tauri::async_runtime::spawn_blocking(move || crate::instances::process::close(pid, started, true, Duration::ZERO)).await {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => log::warn!("could not close client {pid} started during close all: {e}"),
                    Err(e) => log::warn!("close of client {pid} started during close all failed: {e}"),
                }
            }
            None
        }
        Ok((_, inst)) => {
            record_relaunch(&s.db, r.account_id, commands::now_ms(), window_ms);
            s.tracker.set_rejoins(&inst.id, r.rejoins);
            // "Close after" counts the whole run, relaunches included.
            s.tracker.carry_session(&inst.id, &r.from);
            commands::instances_changed(app);
            log::info!("auto-{again} ({}/{} in the window): instance {} relaunched as {}", n + 1, v.breaker_max, r.from, inst.id);
            if !account.as_ref().is_some_and(|a| a.discord_muted) {
                let name = crate::discord::name_of(account.as_ref(), r.account_id);
                crate::discord::notify(app, crate::discord::Event::Restart { name, cause: r.cause });
            }
            None
        }
        // Kill all during the launch: dropped, as above.
        Err(_) if s.bulk.kills() != kills => None,
        Err(e) => {
            log::warn!("auto-{again} of instance {} failed: {e}", r.from);
            failed(app, account.as_ref(), r.account_id, &name, rejoin, &e)
        }
    }
}

/// Maps a fixed `Server` target to its `Place` when `fallback_place` is set (after a 288 shutdown,
/// failed join, or repeat rejoin), so RoALT doesn't loop on a dead job id.
fn effective_target(target: &LaunchTarget, fallback_place: bool) -> LaunchTarget {
    if fallback_place && let LaunchTarget::Server { place_id, .. } = target {
        return LaunchTarget::Place { place_id: *place_id, launch_data: None };
    }
    target.clone()
}

/// Tells Discord when a relaunch stops for good (respecting `discord_muted`).
fn discord_gave_up(app: &AppHandle, account: Option<&AccountSummary>, account_id: i64, reason: String) {
    if !account.is_some_and(|a| a.discord_muted) {
        let name = crate::discord::name_of(account, account_id);
        crate::discord::notify(app, crate::discord::Event::GaveUp { name, reason });
    }
}

/// A launch (or its check) failed: `Some(why)` when it may work later; else the user is told. Roblox
/// missing counts as passing: its updater swaps the install, and that closes clients too.
fn failed(app: &AppHandle, account: Option<&AccountSummary>, account_id: i64, name: &str, rejoin: bool, e: &AppError) -> Option<String> {
    let why = e.message();
    if e.temporary() || crate::launcher::client::find().is_err() {
        return Some(why);
    }
    let text = if rejoin {
        tr!("{name} couldn't be rejoined: {why}", name = name, why = why)
    } else {
        tr!("{name} couldn't be restarted: {why}", name = name, why = why)
    };
    notice(app, true, text);
    discord_gave_up(app, account, account_id, why);
    None
}

/// The wait before trying again after `tries` retries: 15 s, doubling up to 2 minutes (pure).
fn retry_wait(tries: u32) -> Duration {
    Duration::from_secs(15 << tries.min(3))
}

/// Whether waiting clients should be mentioned now; at most every `WAITING_SAY_EVERY`.
fn say_waiting(now: Instant) -> bool {
    let mut said = WAITING_SAID.lock().unwrap_or_else(|e| e.into_inner());
    if said.is_some_and(|t| now.saturating_duration_since(t) < WAITING_SAY_EVERY) {
        return false;
    }
    *said = Some(now);
    true
}

/// Runs `f` on the account's relaunch times (Unix ms), loading persisted history once.
fn with_recent<T>(db: &crate::storage::Db, account_id: i64, f: impl FnOnce(&mut VecDeque<i64>) -> T) -> T {
    let mut store = RECENT.lock().unwrap_or_else(|e| e.into_inner());
    if !store.loaded {
        store.loaded = true;
        if let Ok(Some(raw)) = db.setting_value(RECENT_KEY) {
            store.entries = decode_recent(&raw);
        }
    }
    let i = match store.entries.iter().position(|(a, _)| *a == account_id) {
        Some(i) => i,
        None => {
            store.entries.push((account_id, VecDeque::new()));
            store.entries.len() - 1
        }
    };
    f(&mut store.entries[i].1)
}

/// Records a relaunch timestamp for `account_id` and persists the pruned breaker history to SQLite.
fn record_relaunch(db: &crate::storage::Db, account_id: i64, now_ms: i64, window_ms: i64) {
    let encoded = {
        let mut store = RECENT.lock().unwrap_or_else(|e| e.into_inner());
        if !store.loaded {
            store.loaded = true;
            if let Ok(Some(raw)) = db.setting_value(RECENT_KEY) {
                store.entries = decode_recent(&raw);
            }
        }
        match store.entries.iter().position(|(a, _)| *a == account_id) {
            Some(i) => store.entries[i].1.push_back(now_ms),
            None => store.entries.push((account_id, VecDeque::from([now_ms]))),
        }
        encode_recent(&store.entries, now_ms, window_ms.max(RECENT_KEEP_MS))
    };
    if let Err(e) = db.set_setting_value(RECENT_KEY, Some(&encoded)) {
        log::debug!("auto-rejoin: could not persist breaker history: {e}");
    }
}

/// Forgets relaunches older than `window_ms`; how many are left (pure).
fn recent_ms(times: &mut VecDeque<i64>, now_ms: i64, window_ms: i64) -> u32 {
    while times.front().is_some_and(|&t| now_ms.saturating_sub(t) >= window_ms) {
        times.pop_front();
    }
    times.len() as u32
}

/// Serializes breaker entries, pruning timestamps older than `max_age_ms` (pure).
fn encode_recent(entries: &[(i64, VecDeque<i64>)], now_ms: i64, max_age_ms: i64) -> String {
    let pruned: Vec<(i64, Vec<i64>)> = entries
        .iter()
        .filter_map(|(id, q)| {
            let kept: Vec<i64> = q.iter().copied().filter(|&t| now_ms.saturating_sub(t) < max_age_ms).collect();
            (!kept.is_empty()).then_some((*id, kept))
        })
        .collect();
    serde_json::to_string(&pruned).unwrap_or_else(|_| "[]".into())
}

/// Parses persisted breaker entries; returns empty on corrupt or unknown data (pure).
fn decode_recent(raw: &str) -> Vec<(i64, VecDeque<i64>)> {
    serde_json::from_str::<Vec<(i64, Vec<i64>)>>(raw)
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(id, v)| (id, VecDeque::from(v)))
        .collect()
}

/// The wait before a relaunch: the launch delay doubled for each recent relaunch of the
/// account (`jittered` then caps it at 60 s).
pub(crate) fn backoff(delay_ms: u32, recent: u32) -> u32 {
    delay_ms.saturating_mul(1 << recent.min(16))
}

/// Why the rejoin can't go ahead now, whether that may pass by itself, and whether Discord should
/// be told when it is permanent (None: launch it).
/// `account` is None when it was removed or its id now belongs to another user; `batch`: a batch is running.
fn skip_reason(s: &App, r: &Rejoin, account: Option<&AccountSummary>, batch: bool) -> Option<(String, bool, bool)> {
    let busy = |i: &Instance| {
        i.id != r.from
            && i.account_id == r.account_id
            && matches!(i.state, InstanceState::Starting | InstanceState::Joining | InstanceState::InGame)
    };
    let others = |i: &Instance| i.id != r.from && i.state != InstanceState::Exited;
    if !s.db.settings().inspect_err(|e| log::warn!("auto-rejoin of instance {}: could not read settings: {e}", r.from)).is_ok_and(|v| wanted(s, &v, r)) {
        Some((
            match r.cause {
                CrashKind::Disconnect => tr!("auto-rejoin was turned off"),
                CrashKind::Mem => tr!("the memory restart was turned off"),
                _ => tr!("auto-restart was turned off"),
            },
            false,
            false,
        ))
    } else if s.vault.is_locked() {
        Some((tr!("the vault is locked"), true, false))
    } else if batch {
        Some((tr!("a batch is running"), true, false))
    } else if account.is_none() {
        Some((tr!("it was removed"), false, false))
    } else if account.is_some_and(|a| a.allow_disconnect) {
        Some((tr!("it is set to allow disconnects"), false, false))
    } else if account.is_some_and(|a| a.status != AccountStatus::Valid) {
        Some((tr!("its cookie isn't valid. Sign in to it again"), false, true))
    } else if s.tracker.list().iter().any(busy) {
        Some((tr!("it already has a client running"), false, false))
    } else if !s.multi.is_active() && s.tracker.list().iter().any(others) {
        // Without the multi-instance lock the new client would replace the running one.
        Some((tr!("another client is running and multi-instance is off"), false, true))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_breaker_counts_relaunches_within_the_window() {
        let t0: i64 = 1_700_000_000_000;
        let min: i64 = 60_000;
        let window = 10 * min;
        let mut times = VecDeque::new();
        assert_eq!(recent_ms(&mut times, t0, window), 0);
        for m in [0, 4, 8] {
            times.push_back(t0 + m * min);
        }
        assert_eq!(recent_ms(&mut times, t0 + 9 * min, window), 3, "3 in 10 minutes: a breaker of 3 trips");
        // The first one leaves the window at 10 minutes, the next at 14.
        assert_eq!(recent_ms(&mut times, t0 + 10 * min, window), 2);
        assert_eq!(recent_ms(&mut times, t0 + 14 * min, window), 1);
        assert_eq!(times.front(), Some(&(t0 + 8 * min)));
        assert_eq!(recent_ms(&mut times, t0 + 60 * min, window), 0);
        assert!(times.is_empty());
    }

    #[test]
    fn persisted_breaker_round_trips_and_prunes_old_entries() {
        let now: i64 = 1_700_000_000_000;
        let entries = vec![
            (1, VecDeque::from([now - 20_000, now - 5_000])),
            (2, VecDeque::from([now - 100_000])),
        ];
        let json = encode_recent(&entries, now, 30_000);
        let decoded = decode_recent(&json);
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].0, 1);
        assert_eq!(decoded[0].1, VecDeque::from([now - 20_000, now - 5_000]));
        assert!(decode_recent("not valid json").is_empty());
    }

    #[test]
    fn retries_slow_down_to_two_minutes() {
        let secs: Vec<u64> = [0, 1, 2, 3, 4, 40].map(|t| retry_wait(t).as_secs()).to_vec();
        assert_eq!(secs, [15, 30, 60, 120, 120, 120]);
        // The whole hour fits many tries, so a short outage is outlasted.
        assert!(RETRY_FOR > 10 * retry_wait(3));
    }

    #[test]
    fn says_it_is_waiting_once_per_outage_not_once_per_client() {
        let t0 = Instant::now();
        assert!(say_waiting(t0));
        assert!(!say_waiting(t0 + Duration::from_secs(1)));
        assert!(!say_waiting(t0 + WAITING_SAY_EVERY - Duration::from_secs(1)));
        assert!(say_waiting(t0 + WAITING_SAY_EVERY));
    }

    #[test]
    fn only_what_can_pass_by_itself_is_tried_again() {
        let passing = [AppError::Network("x".into()), AppError::RateLimited(5), AppError::Roblox("HTTP 503".into()), AppError::Roblox("no ticket".into()), AppError::Locked, AppError::Wait("x".into()), AppError::Busy("x".into())];
        for e in passing {
            assert!(e.temporary(), "{e}");
        }
        for e in [AppError::CookieInvalid, AppError::Challenge, AppError::BoundSessionRequired, AppError::NotFound, AppError::Invalid("x".into()), AppError::Roblox("HTTP 403".into())] {
            assert!(!e.temporary(), "{e}");
        }
    }

    #[test]
    fn the_backoff_doubles_and_is_capped() {
        assert_eq!(backoff(6_000, 0), 6_000);
        assert_eq!(backoff(6_000, 1), 12_000);
        assert_eq!(backoff(6_000, 3), 48_000);
        assert_eq!(backoff(6_000, 40), backoff(6_000, 16));
        assert_eq!(backoff(u32::MAX, 1), u32::MAX, "saturates");
        // jittered caps it at a minute, ± 1 s.
        assert!((59_000..=61_000).contains(&jittered(backoff(6_000, 5)).as_millis()));
    }

    #[test]
    fn server_rejoin_falls_back_to_place_when_flagged() {
        let srv = LaunchTarget::Server { place_id: 1818, job_id: "job-1".into() };
        assert_eq!(effective_target(&srv, false), srv);
        assert_eq!(effective_target(&srv, true), LaunchTarget::Place { place_id: 1818, launch_data: None });
        let place = LaunchTarget::Place { place_id: 1818, launch_data: Some("hi".into()) };
        assert_eq!(effective_target(&place, true), place, "non-Server targets stay untouched");
    }

    #[test]
    fn disconnects_follow_auto_rejoin_and_the_rest_auto_restart() {
        let v = |auto_rejoin, auto_restart_on_crash| Settings { auto_rejoin, auto_restart_on_crash, ..Settings::default() };
        assert!(enabled(&v(true, false), CrashKind::Disconnect));
        assert!(!enabled(&v(true, false), CrashKind::Crash) && !enabled(&v(true, false), CrashKind::Hang));
        assert!(enabled(&v(false, true), CrashKind::Crash) && enabled(&v(false, true), CrashKind::Hang));
        assert!(!enabled(&v(false, true), CrashKind::Disconnect));
        assert!(!enabled(&v(true, true), CrashKind::Mem));
        assert!(enabled(&Settings { mem_restart_mb: 4_096, ..Settings::default() }, CrashKind::Mem));
    }
}
