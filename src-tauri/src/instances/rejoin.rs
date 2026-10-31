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
use crate::model::{AccountStatus, AccountSummary, CrashKind, Instance, InstanceState, Settings};

/// Rejoins take turns: launches go one at a time (DESIGN §6), each after its own delay.
static TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The crash breaker's memory: when each account was last relaunched, oldest first. Changed
/// only while holding `TURN`.
static RECENT: Mutex<Vec<(i64, VecDeque<Instant>)>> = Mutex::new(Vec::new());

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
    let (on, off): (Vec<Rejoin>, Vec<Rejoin>) = rejoins.into_iter().partition(|r| enabled(&v, r.cause));
    if v.auto_rejoin
        && off.iter().any(|r| r.cause == CrashKind::Crash && s.db.account(r.account_id).is_ok_and(|a| !a.allow_disconnect))
        && !CRASH_HINT_SAID.swap(true, Ordering::Relaxed)
    {
        notice(app, true, tr!("A client closed unexpectedly and wasn't relaunched: {rejoin} only covers disconnects. To relaunch crashed clients too, turn on {restart} in Settings.", rejoin = tr!("Auto-rejoin"), restart = tr!("Auto-restart after a crash")));
    }
    for r in on {
        // Who the account is now, so an id removed and reused during the wait is told apart.
        let account = s
            .db
            .account(r.account_id)
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

/// Disconnects follow auto-rejoin; crashes and hangs auto-restart; memory the memory limit.
fn enabled(v: &Settings, cause: CrashKind) -> bool {
    match cause {
        CrashKind::Disconnect => v.auto_rejoin,
        CrashKind::Crash | CrashKind::Hang => v.auto_restart_on_crash,
        CrashKind::Mem => v.mem_restart_mb > 0,
    }
}

async fn rejoin(app: &AppHandle, r: Rejoin, user_id: Option<i64>) {
    let s = app.state::<App>();
    let v = s.db.settings().unwrap_or_else(|e| {
        log::warn!("auto-rejoin of instance {}: could not read settings, using defaults: {e}", r.from);
        Settings::default()
    });
    let window = Duration::from_secs(u64::from(v.breaker_window_min) * 60);
    let n = with_recent(r.account_id, |t| recent(t, Instant::now(), window));
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
            let name = s.db.account(r.account_id).map_or_else(|_| tr!("An account"), |a| name_of(&a));
            let text = if r.cause == CrashKind::Disconnect {
                tr!("{name} wasn't rejoined after {minutes} minutes of trying: {why}", name = name, minutes = minutes, why = why)
            } else {
                tr!("{name} wasn't restarted after {minutes} minutes of trying: {why}", name = name, minutes = minutes, why = why)
            };
            notice(app, true, text);
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
/// marked as closed by RoALT); `commands::launch_tracked(account, target, no favourite, the old
/// client's FPS cap)` and `tracker.set_rejoins(new id, r.rejoins)`. Only a launch that happened
/// counts for the breaker. `closed`: the old client was closed by an earlier go too.
async fn attempt(app: &AppHandle, s: &App, r: &Rejoin, user_id: Option<i64>, kills: u64, closed: &mut bool) -> Option<String> {
    let v = s.db.settings().unwrap_or_else(|e| {
        log::warn!("auto-rejoin of instance {}: could not read settings, using defaults: {e}", r.from);
        Settings::default()
    });
    let window = Duration::from_secs(u64::from(v.breaker_window_min) * 60);
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
        .account(r.account_id)
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
        let check = crate::launcher::launch_check(s, r.account_id, &r.target).await;
        if over(*closed) {
            return None;
        }
        if let Err(e) = check {
            if let Err(e) = commands::record_cookie_error(app, s, r.account_id, &e) {
                log::warn!("auto-{again}: could not record account {}'s cookie problem: {e}", r.account_id);
            }
            return failed(app, &name, rejoin, &e);
        }
    }
    let _turn = TURN.lock().await;
    // Until the launch is done, batches and single launches wait (DESIGN §6: one at a time).
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
    if let Some((why, passes)) = skip_reason(s, r, account.as_ref(), busy.is_none()) {
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
        return None;
    }
    let n = with_recent(r.account_id, |t| recent(t, Instant::now(), window));
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
    match commands::launch_tracked(app, s, r.account_id, r.target.clone(), None, None, fps_cap, !*closed, None).await {
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
            with_recent(r.account_id, |t| t.push_back(Instant::now()));
            s.tracker.set_rejoins(&inst.id, r.rejoins);
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
            failed(app, &name, rejoin, &e)
        }
    }
}

/// A launch (or its check) failed: `Some(why)` when it may work later; else the user is told. Roblox
/// missing counts as passing: its updater swaps the install, and that closes clients too.
fn failed(app: &AppHandle, name: &str, rejoin: bool, e: &AppError) -> Option<String> {
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

/// Runs `f` on the account's relaunch times.
fn with_recent<T>(account_id: i64, f: impl FnOnce(&mut VecDeque<Instant>) -> T) -> T {
    let mut all = RECENT.lock().unwrap_or_else(|e| e.into_inner());
    let i = match all.iter().position(|(a, _)| *a == account_id) {
        Some(i) => i,
        None => {
            all.push((account_id, VecDeque::new()));
            all.len() - 1
        }
    };
    f(&mut all[i].1)
}

/// Forgets relaunches older than `window`; how many are left (pure).
fn recent(times: &mut VecDeque<Instant>, now: Instant, window: Duration) -> u32 {
    while times.front().is_some_and(|&t| now.saturating_duration_since(t) >= window) {
        times.pop_front();
    }
    times.len() as u32
}

/// The wait before a relaunch: the launch delay doubled for each recent relaunch of the
/// account (`jittered` then caps it at 60 s).
fn backoff(delay_ms: u32, recent: u32) -> u32 {
    delay_ms.saturating_mul(1 << recent.min(16))
}

/// Why the rejoin can't go ahead now, and whether that may pass by itself (None: launch it).
/// `account` is None when it was removed or its id now belongs to another user; `batch`: a batch is running.
fn skip_reason(s: &App, r: &Rejoin, account: Option<&AccountSummary>, batch: bool) -> Option<(String, bool)> {
    let busy = |i: &Instance| {
        i.id != r.from
            && i.account_id == r.account_id
            && matches!(i.state, InstanceState::Starting | InstanceState::Joining | InstanceState::InGame)
    };
    let others = |i: &Instance| i.id != r.from && i.state != InstanceState::Exited;
    if !s.db.settings().inspect_err(|e| log::warn!("auto-rejoin of instance {}: could not read settings: {e}", r.from)).is_ok_and(|v| enabled(&v, r.cause)) {
        Some((
            match r.cause {
                CrashKind::Disconnect => tr!("auto-rejoin was turned off"),
                CrashKind::Mem => tr!("the memory restart was turned off"),
                _ => tr!("auto-restart was turned off"),
            },
            false,
        ))
    } else if s.vault.is_locked() {
        Some((tr!("the vault is locked"), true))
    } else if batch {
        Some((tr!("a batch is running"), true))
    } else if account.is_none() {
        Some((tr!("it was removed"), false))
    } else if account.is_some_and(|a| a.allow_disconnect) {
        Some((tr!("it is set to allow disconnects"), false))
    } else if account.is_some_and(|a| a.status != AccountStatus::Valid) {
        Some((tr!("its cookie isn't valid. Sign in to it again"), false))
    } else if s.tracker.list().iter().any(busy) {
        Some((tr!("it already has a client running"), false))
    } else if !s.multi.is_active() && s.tracker.list().iter().any(others) {
        // Without the multi-instance lock the new client would replace the running one.
        Some((tr!("another client is running and multi-instance is off"), false))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_breaker_counts_relaunches_within_the_window() {
        let t0 = Instant::now();
        let min = Duration::from_secs(60);
        let window = 10 * min;
        let mut times = VecDeque::new();
        assert_eq!(recent(&mut times, t0, window), 0);
        for m in [0, 4, 8] {
            times.push_back(t0 + m * min);
        }
        assert_eq!(recent(&mut times, t0 + 9 * min, window), 3, "3 in 10 minutes: a breaker of 3 trips");
        // The first one leaves the window at 10 minutes, the next at 14.
        assert_eq!(recent(&mut times, t0 + 10 * min, window), 2);
        assert_eq!(recent(&mut times, t0 + 14 * min, window), 1);
        assert_eq!(times.front(), Some(&(t0 + 8 * min)));
        assert_eq!(recent(&mut times, t0 + 60 * min, window), 0);
        assert!(times.is_empty());
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
        let passing = [AppError::Network("x".into()), AppError::RateLimited(5), AppError::Roblox("HTTP 503".into()), AppError::Roblox("no ticket".into()), AppError::Locked, AppError::Wait("x".into())];
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
