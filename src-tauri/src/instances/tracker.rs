//! Following the clients RoALT starts: one entry per launch, polled once a second.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use super::logwatch::{self, LogFacts};
use super::metrics::{MEM_STRIKES, mem_strikes, mem_throttled};
use super::process::{self, ProcInfo};
use crate::model::{CrashKind, Instance, InstanceState, LaunchInfo, LaunchTarget, Limits};

/// A client that vanishes this soon after launch without reaching a game may come back under
/// a new PID (Roblox's updater relaunches it, research M8/M9), or, behind a bootstrapper, may
/// not have started yet.
const REBIND_MS: i64 = 120_000;
const MAX_ENDED: usize = 30;
/// How long a teleport may take to reach its next game; then it failed (`LogFacts::teleporting`).
const TELEPORT_MS: i64 = 60_000;
/// How far a client's start may be from its log file's creation, in seconds.
const LOG_SKEW_SECS: u64 = 3;
// Notes are made in the current language and compared against these (a note made before a
// language switch stops matching until the next poll remakes it).
/// The note of a client whose log signed in as a different user than its account.
pub fn wrong_account() -> String {
    tr!("Signed in as another account")
}
pub fn challenged() -> String {
    tr!("Roblox wants a human check")
}
/// The note of a client that ended without RoALT closing it and with nothing else to blame.
pub fn unexpected() -> String {
    tr!("Closed unexpectedly")
}
/// The note of a running client whose window stopped responding.
pub fn hung() -> String {
    tr!("Not responding")
}
/// The note of a client the user (or RoALT) closed.
pub fn closed() -> String {
    tr!("Closed")
}
/// Polls (about seconds) a client's window must be not responding in a row to count as hung.
const HANG_POLLS: u32 = 30;
/// Polls a client's window must be not responding in a row for its limits to come off.
const HANG_FREE_POLLS: u32 = 5;

pub struct Tracker {
    inner: Mutex<Vec<Entry>>,
    /// Accounts being launched (`claim`); locked before `inner`.
    claimed: Mutex<Vec<i64>>,
    /// Clients that ended badly since the last `take_crashes`; locked after `inner`.
    crashes: Mutex<Vec<Ended>>,
    /// What `persist` last wrote to the database; locked after `inner`.
    persisted: Mutex<Vec<Saved>>,
}

/// What a restarted RoALT needs to follow a running client again (`readopt`), kept in the
/// database while the client runs (`persist`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Saved {
    inst: Instance,
    btid: i64,
    /// Start time of `inst.pid`, the client itself.
    start_time: u64,
    log: Option<PathBuf>,
    offset: u64,
    facts: LogFacts,
    in_game_at: Option<i64>,
}

/// A client that ended badly (`crash_kind`), for the crash history.
#[derive(Clone, Debug, PartialEq)]
pub struct Ended {
    pub instance_id: String,
    pub account_id: i64,
    pub kind: CrashKind,
    /// Unix ms.
    pub at: i64,
    pub uptime_ms: i64,
    pub reason: Option<String>,
    pub place_id: Option<i64>,
    pub log: Option<PathBuf>,
}

/// An account being launched; released on drop.
pub struct Claim<'a> {
    tracker: &'a Tracker,
    account_id: i64,
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        self.tracker.claimed.lock().unwrap_or_else(|e| e.into_inner()).retain(|&a| a != self.account_id);
    }
}

/// One launch: what the UI sees plus what following it needs.
#[derive(Clone)]
struct Entry {
    inst: Instance,
    btid: i64,
    /// Start time of `inst.pid` when first seen (0: already gone), so a reused PID is not it.
    start_time: Option<u64>,
    /// `inst.pid` is the client, not a bootstrapper standing in for it.
    adopted: bool,
    alive: bool,
    log: Option<PathBuf>,
    offset: u64,
    facts: LogFacts,
    /// The account's Roblox user id, looked up on the first poll.
    expected_user: Option<i64>,
    /// The `roalt_<user id>` profile seen on the client's command line ("cordial-run …",
    /// or "cage -- cordial-run …" for a headless client).
    profile_user: Option<i64>,
    user_checked: bool,
    closing: bool,
    /// Unix ms the client first reached a server.
    in_game_at: Option<i64>,
    /// Polls in a row its window was not responding (`process::hung`).
    hung_polls: u32,
    /// RoALT gave up restarting it (the crash breaker): its note from then on.
    gave_up: Option<String>,
    /// Unix ms `rejoin` started working on relaunching it, which can take an hour: the one task for
    /// it, it isn't pruned, and Close all ends it (`cancel_relaunches`).
    relaunching: Option<i64>,
    /// Unix ms the bootstrapper of a not yet adopted entry was first seen gone.
    gone_at: Option<i64>,
    /// Handed out by `take_rejoins` (or ended): not again until it is back in a game.
    rejoin_seen: bool,
    /// Unix ms a teleport was first seen under way.
    teleport_at: Option<i64>,
    /// Unix ms it last entered Starting or Joining; None once it is in a game.
    loading_since: Option<i64>,
    /// The server last handed out by `take_servers`.
    server_seen: Option<String>,
    /// Memory samples in a row over `Settings.mem_restart_mb` (`check_memory`).
    mem_strikes: u32,
    /// Its private memory at the last sample, MB.
    mem_mb: u64,
    /// RoALT is closing it for its memory (`mark_restart`): its exit is recorded as `Mem`.
    mem_restart: bool,
    /// Its main window has been seen (`first_windows`), so "Minimize on launch" is done with it.
    window_seen: bool,
    /// A failure to read its log was already logged at warn (later ones go to debug).
    log_warned: bool,
}

impl Entry {
    fn new(inst: Instance, btid: i64, start_time: Option<u64>, adopted: bool) -> Entry {
        Entry {
            inst,
            btid,
            start_time,
            adopted,
            alive: true,
            log: None,
            offset: 0,
            facts: LogFacts::default(),
            expected_user: None,
            profile_user: None,
            user_checked: false,
            closing: false,
            in_game_at: None,
            hung_polls: 0,
            gave_up: None,
            relaunching: None,
            gone_at: None,
            rejoin_seen: false,
            teleport_at: None,
            loading_since: None,
            server_seen: None,
            mem_strikes: 0,
            mem_mb: 0,
            mem_restart: false,
            window_seen: false,
            log_warned: false,
        }
    }

    fn hung(&self) -> bool {
        self.hung_polls >= HANG_POLLS
    }

    /// Loading (for as long as a first load or a teleport may take: one stuck there is limited
    /// again) or not responding: limits would only slow that down (`limits::plan`).
    fn runs_free(&self, now: i64) -> bool {
        let may_take = if self.in_game_at.is_none() { REBIND_MS } else { TELEPORT_MS };
        self.hung_polls >= HANG_FREE_POLLS || self.loading_since.is_some_and(|t| now - t < may_take)
    }

    /// Over the memory limit long enough to be restarted.
    fn mem_high(&self) -> bool {
        self.mem_strikes >= MEM_STRIKES
    }

    /// Why it is worth launching again (`wants_rejoin`), or its memory (`Mem`) while it still
    /// runs with no problem note.
    fn stop_relaunch(&mut self) {
        self.relaunching = None;
        self.inst.relaunching = false;
        self.inst.relaunch_problem = None;
    }

    fn cause(&self) -> Option<CrashKind> {
        wants_rejoin(&self.inst, &self.facts, self.hung()).or_else(|| {
            (self.mem_high() && self.inst.state != InstanceState::Exited && self.inst.note.is_none()).then_some(CrashKind::Mem)
        })
    }
}

/// Live Cordial/cage account profiles, when there is at least one entry that can be checked by
/// profile. `None` means "not checked", not "nothing is running".
fn live_profiles(entries: &[Entry]) -> Option<std::collections::HashSet<i64>> {
    entries.iter().any(|e| e.profile_user.is_some()).then(process::profiles_by_user).flatten()
}

/// Whether this entry's profile still has a live Cordial/cage process. Entries without a
/// recognised profile (Sober, Wine, a hand-started client) keep the process check alone.
fn profile_is_live(e: &Entry, live: &Option<std::collections::HashSet<i64>>) -> bool {
    match (e.profile_user, live) {
        (Some(user), Some(profiles)) => profiles.contains(&user),
        _ => true,
    }
}

impl Tracker {
    pub fn new() -> Tracker {
        Tracker {
            inner: Mutex::new(Vec::new()),
            claimed: Mutex::new(Vec::new()),
            crashes: Mutex::new(Vec::new()),
            persisted: Mutex::new(Vec::new()),
        }
    }

    fn entries(&self) -> MutexGuard<'_, Vec<Entry>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Starts following a launch (state Starting).
    pub fn register(&self, info: &LaunchInfo, target: LaunchTarget) -> Instance {
        let inst = Instance {
            id: uuid::Uuid::new_v4().to_string(),
            account_id: info.account_id,
            pid: info.pid,
            state: InstanceState::Starting,
            target,
            place_id: info.place_id,
            job_id: None,
            started_at: info.started_at,
            ended_at: None,
            via_bootstrapper: info.via_bootstrapper,
            note: None,
            rejoins: 0,
            limits: Default::default(),
            anti_afk: true,
            afk_at: None,
            throttled: false,
            micro: false,
            fps_cap: info.fps_cap,
            relaunching: false,
            relaunch_problem: None,
            sign_in_problem: false,
        };
        let mut entries = self.entries();
        // The account has a new client: nothing is waiting to bring back an older one any more.
        for e in entries.iter_mut().filter(|e| e.inst.account_id == info.account_id) {
            e.inst.relaunch_problem = None;
        }
        entries.push(Entry::new(inst.clone(), info.browser_tracker_id, info.pid_start, !info.via_bootstrapper));
        inst
    }

    /// The running clients a restarted RoALT can follow again: those known to be the client
    /// itself (not a bootstrapper standing in for it).
    pub fn saved(&self) -> Vec<Saved> {
        self.entries()
            .iter()
            .filter(|e| e.inst.state != InstanceState::Exited)
            .filter_map(|e| {
                let (_, start_time) = client_of(e)?;
                let (log, facts) = (e.log.clone(), e.facts.clone());
                Some(Saved { inst: e.inst.clone(), btid: e.btid, start_time, log, offset: e.offset, facts, in_game_at: e.in_game_at })
            })
            .collect()
    }

    /// Writes `saved` to the database when a client started, changed or ended; when only how
    /// far their logs were read changed, only if `due` (re-reading a few lines after a restart
    /// gives the same facts).
    pub fn persist(&self, db: &crate::storage::Db, due: bool) {
        let now = self.saved();
        let mut last = self.persisted.lock().unwrap_or_else(|e| e.into_inner());
        let moved = now.len() != last.len() || now.iter().zip(last.iter()).any(|(a, b)| a.inst != b.inst);
        if *last == now || !(moved || due) {
            return;
        }
        let rows: Vec<(String, String)> =
            now.iter()
                .filter_map(|c| match serde_json::to_string(c) {
                    Ok(j) => Some((c.inst.id.clone(), j)),
                    Err(e) => {
                        log::warn!("tracked instance {} not saved: {:?} error", c.inst.id, e.classify());
                        None
                    }
                })
                .collect();
        match db.save_tracked(&rows) {
            Ok(()) => *last = now,
            Err(e) => log::warn!("could not save the tracked clients: {e}"),
        }
    }

    /// Follows clients from before RoALT restarted again (before the first poll): each one
    /// `readoptable` among the running Roblox clients `roblox`, its log read on from where it
    /// was. Returns the (pid, start time) of each one followed again.
    pub fn readopt(&self, saved: Vec<Saved>, roblox: &[ProcInfo]) -> Vec<(u32, u64)> {
        let mut entries = self.entries();
        let mut out = Vec::new();
        for s in saved {
            if !readoptable(&s, roblox, &entries) {
                continue;
            }
            out.push((s.inst.pid, s.start_time));
            let mut e = Entry::new(s.inst, s.btid, Some(s.start_time), true);
            (e.log, e.offset, e.facts, e.in_game_at) = (s.log, s.offset, s.facts, s.in_game_at);
            // "Minimize on launch" was done with it before.
            e.window_seen = true;
            entries.push(e);
        }
        out
    }

    /// Running ones first, then ended ones (newest first, at most 30 kept).
    pub fn list(&self) -> Vec<Instance> {
        let mut v: Vec<Instance> = self.entries().iter().map(|e| e.inst.clone()).collect();
        // Running ones in launch order.
        v.sort_by_key(|i| match i.state {
            InstanceState::Exited => (1, -i.ended_at.unwrap_or(0)),
            _ => (0, i.started_at),
        });
        v
    }

    pub fn get(&self, id: &str) -> Option<Instance> {
        self.entries().iter().find(|e| e.inst.id == id).map(|e| e.inst.clone())
    }

    /// The user asked to close it: its exit is not unexpected (no auto-rejoin, note "Closed").
    /// Returns the process to close, under the same lock: its pid and the start time it was
    /// first seen with (None before that). None once it ended or its process is gone, so a
    /// reused pid is never closed; the next poll ends the entry.
    pub fn mark_closing(&self, id: &str) -> Option<(u32, Option<u64>)> {
        self.mark_restart(id, None)
    }

    /// `mark_closing` for an auto-rejoin or restart of `cause`; a `Mem` one ends as `Mem`.
    pub fn mark_restart(&self, id: &str, cause: Option<CrashKind>) -> Option<(u32, Option<u64>)> {
        let mut entries = self.entries();
        let e = entries.iter_mut().find(|e| e.inst.id == id && e.inst.state != InstanceState::Exited)?;
        e.closing = true;
        e.mem_restart = cause == Some(CrashKind::Mem);
        e.alive.then_some((e.inst.pid, e.start_time))
    }

    /// Launches still in a bootstrapper's hands, whose client hasn't appeared: for the panic button.
    pub fn bootstrappers(&self) -> Vec<(u32, Option<u64>)> {
        self.entries().iter().filter(|e| !e.adopted && e.alive && e.inst.state != InstanceState::Exited).map(|e| (e.inst.pid, e.start_time)).collect()
    }

    /// Panic button: every entry, ended ones too, counts as closed by the user, so no rejoin or
    /// restart waiting for its turn goes ahead (`still_wants_rejoin`).
    pub fn mark_all_closing(&self) {
        self.entries().iter_mut().for_each(|e| e.closing = true);
    }

    /// The crash breaker tripped for this client: `note` ("Gave up after ...") stays its note,
    /// unless the user closes it.
    pub fn give_up(&self, id: &str, note: String) {
        if let Some(e) = self.entries().iter_mut().find(|e| e.inst.id == id) {
            if !e.closing {
                e.inst.note = Some(note.clone());
            }
            e.gave_up = Some(note);
        }
    }

    /// Clients that ended badly since the last call, oldest first.
    /// The servers clients reached since the last call: (account id, what to record). A teleport
    /// counts as a new one; a client followed again after a restart is sorted out by `add_server`.
    pub fn take_servers(&self) -> Vec<(i64, crate::model::ServerVisit)> {
        let mut out = Vec::new();
        for e in self.entries().iter_mut().filter(|e| e.facts.in_game && !e.facts.teleporting) {
            if let (Some(job), Some(place)) = (&e.facts.job_id, e.facts.place_id)
                && e.server_seen.as_ref() != Some(job)
            {
                // Where a teleport leads may need a code too, and its log doesn't say.
                let private = e.server_seen.is_some() || matches!(e.inst.target, LaunchTarget::PrivateLink { .. } | LaunchTarget::PrivateAccess { .. });
                out.push((e.inst.account_id, crate::model::ServerVisit { place_id: place, job_id: job.clone(), private, joined_at: crate::commands::now_ms() }));
                e.server_seen = Some(job.clone());
            }
        }
        out
    }

    pub fn take_crashes(&self) -> Vec<Ended> {
        std::mem::take(&mut *self.crashes.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// A close that didn't land: its exit counts as unexpected again.
    pub fn clear_closing(&self, id: &str) {
        if let Some(e) = self.entries().iter_mut().find(|e| e.inst.id == id) {
            e.closing = false;
            e.mem_restart = false;
        }
    }

    /// The account has a client starting, joining or in a game (a disconnected one doesn't
    /// count: it is waiting to be closed or rejoined). A Cordial/cage client whose profile is
    /// no longer running does not count, even if the last poll has not ended its entry yet.
    pub fn has_live_client(&self, account_id: i64) -> bool {
        self.has_live_client_for(account_id, None)
    }

    /// `has_live_client`, also asking whether a Cordial/cage process with this account's
    /// profile exists even when RoALT lost its tracker entry.
    pub fn has_live_client_for(&self, account_id: i64, user_id: Option<i64>) -> bool {
        let entries = self.entries();
        let live = live_profiles(&entries);
        if entries.iter().any(|e| {
            e.inst.account_id == account_id
                && matches!(e.inst.state, InstanceState::Starting | InstanceState::Joining | InstanceState::InGame)
                && profile_is_live(e, &live)
        }) {
            return true;
        }
        user_id.is_some_and(|user| process::profiles_by_user().is_some_and(|p| p.contains(&user)))
    }

    /// Reserves the account for one launch: None while it has a live client or another launch
    /// holds it. Hold the claim until the new client is registered.
    pub fn claim(&self, account_id: i64) -> Option<Claim<'_>> {
        self.claim_for(account_id, None)
    }

    /// `claim`, also refusing when a Cordial/cage process with this account's profile runs.
    pub fn claim_for(&self, account_id: i64, user_id: Option<i64>) -> Option<Claim<'_>> {
        let mut claimed = self.claimed.lock().unwrap_or_else(|e| e.into_inner());
        if claimed.contains(&account_id) || self.has_live_client_for(account_id, user_id) {
            return None;
        }
        claimed.push(account_id);
        Some(Claim { tracker: self, account_id })
    }

    /// The logs of clients that haven't ended (`logwatch::clean` leaves them).
    pub fn live_logs(&self) -> Vec<PathBuf> {
        self.entries().iter().filter(|e| e.inst.state != InstanceState::Exited).filter_map(|e| e.log.clone()).collect()
    }

    /// True while a client RoALT started hasn't ended.
    pub fn any_running(&self) -> bool {
        let entries = self.entries();
        let live = live_profiles(&entries);
        entries.iter().any(|e| e.inst.state != InstanceState::Exited && profile_is_live(e, &live))
    }

    /// How many auto-rejoins in a row led to the client `id` (shown in the UI).
    pub fn set_rejoins(&self, id: &str, rejoins: u32) {
        if let Some(e) = self.entries().iter_mut().find(|e| e.inst.id == id) {
            e.inst.rejoins = rejoins;
        }
    }

    /// Sets the client's limits; returns the previous ones and its process (pid, start time)
    /// once it is known to be the client itself (None while only a bootstrapper is seen).
    /// None when there is no such running client.
    pub fn set_limits(&self, id: &str, limits: Limits) -> Option<(Limits, Option<(u32, u64)>)> {
        let mut entries = self.entries();
        let e = entries.iter_mut().find(|e| e.inst.id == id && e.inst.state != InstanceState::Exited)?;
        let prev = std::mem::replace(&mut e.inst.limits, limits);
        Some((prev, client_of(e)))
    }

    /// Opts a running client in or out of anti-AFK; false when there is no such client.
    pub fn set_anti_afk(&self, id: &str, on: bool) -> bool {
        let mut entries = self.entries();
        let Some(e) = entries.iter_mut().find(|e| e.inst.id == id && e.inst.state != InstanceState::Exited) else { return false };
        e.inst.anti_afk = on;
        true
    }

    /// Clients anti-AFK may press keys in: in a game, opted in, not being closed, the client
    /// itself known. `since`: its last anti-AFK press, else when it reached the game.
    pub fn afk_clients(&self) -> Vec<super::afk::AfkClient> {
        self.entries()
            .iter()
            .filter(|e| e.inst.state == InstanceState::InGame && e.inst.anti_afk && !e.closing)
            .filter_map(|e| {
                let (pid, _) = client_of(e)?;
                Some(super::afk::AfkClient { id: e.inst.id.clone(), pid, since: e.inst.afk_at.or(e.in_game_at)? })
            })
            .collect()
    }

    /// Marks a running client's window as made small (or back to normal); false when there is
    /// no such client.
    pub fn set_micro(&self, id: &str, on: bool) -> bool {
        let mut entries = self.entries();
        let Some(e) = entries.iter_mut().find(|e| e.inst.id == id && e.inst.state != InstanceState::Exited) else { return false };
        e.inst.micro = on;
        true
    }

    /// Pids of running clients whose main window `has_window` sees for the first time (each
    /// returned once per instance, also while "Minimize on launch" is off, so turning it on
    /// later leaves clients already up alone). Windows are looked for outside the lock.
    pub fn first_windows(&self, has_window: &dyn Fn(u32) -> bool) -> Vec<u32> {
        let waiting: Vec<(String, u32)> = self
            .entries()
            .iter()
            .filter(|e| !e.window_seen && e.inst.state != InstanceState::Exited)
            .filter_map(|e| client_of(e).map(|(pid, _)| (e.inst.id.clone(), pid)))
            .collect();
        let seen: Vec<(String, u32)> = waiting.into_iter().filter(|&(_, pid)| has_window(pid)).collect();
        let mut entries = self.entries();
        seen.into_iter()
            .filter(|(id, pid)| {
                let Some(e) = entries.iter_mut().find(|e| e.inst.id == *id && e.inst.pid == *pid && !e.window_seen) else { return false };
                e.window_seen = true;
                true
            })
            .map(|(_, pid)| pid)
            .collect()
    }

    /// Anti-AFK pressed a key in the client at `at` (unix ms).
    pub fn mark_afk(&self, id: &str, at: i64) {
        if let Some(e) = self.entries().iter_mut().find(|e| e.inst.id == id) {
            e.inst.afk_at = Some(at);
        }
    }

    /// Running clients by (id, pid, start time, limits), for `Limiter::sync`.
    pub fn clients(&self) -> Vec<(String, u32, u64, Limits)> {
        self.entries()
            .iter()
            .filter(|e| e.inst.state != InstanceState::Exited)
            .filter_map(|e| client_of(e).map(|(pid, t0)| (e.inst.id.clone(), pid, t0, e.inst.limits)))
            .collect()
    }

    /// The running clients `limits::plan` lifts, besides the one in front: `runs_free` ones and
    /// those of the `full_quality` accounts.
    pub fn free(&self, full_quality: &[i64], now: i64) -> Vec<String> {
        self.entries()
            .iter()
            .filter(|e| e.inst.state != InstanceState::Exited && (e.runs_free(now) || full_quality.contains(&e.inst.account_id)))
            .map(|e| e.inst.id.clone())
            .collect()
    }

    /// Still a rejoin case (`wants_rejoin`), checked again after the launch delay: false once
    /// the user closed it, it reconnected, responds again or was cleared.
    pub fn still_wants_rejoin(&self, id: &str) -> bool {
        self.entries().iter().any(|e| e.inst.id == id && !e.closing && e.cause().is_some())
    }

    /// Still to be relaunched, after a check that took a while: `still_wants_rejoin` (`closed`: RoALT
    /// closed it for this, so only that it isn't cleared), unless Close all stopped the relaunch or
    /// the account was launched since it began.
    pub fn relaunch_wanted(&self, id: &str, closed: bool) -> bool {
        let entries = self.entries();
        let Some(me) = entries.iter().find(|e| e.inst.id == id) else { return false };
        let Some(began) = me.relaunching else { return false };
        (closed || (!me.closing && me.cause().is_some()))
            && !entries.iter().any(|e| e.inst.id != id && e.inst.account_id == me.inst.account_id && e.inst.started_at > began)
    }

    /// A relaunch of this client starts (`now`: Unix ms); false if one is at work already.
    pub fn begin_relaunch(&self, id: &str, now: i64) -> bool {
        self.entries().iter_mut().find(|e| e.inst.id == id).is_some_and(|e| {
            e.inst.relaunching = true;
            e.relaunching.replace(e.relaunching.unwrap_or(now)).is_none()
        })
    }

    /// Why this client's relaunch has to wait (None: it no longer does); the UI shows it.
    pub fn set_relaunch_problem(&self, id: &str, why: Option<String>) {
        if let Some(e) = self.entries().iter_mut().find(|e| e.inst.id == id && e.relaunching.is_some()) {
            e.inst.relaunch_problem = why;
        }
    }

    /// Not stopped by Close all or a clear (it is over when its task ends).
    pub fn is_relaunching(&self, id: &str) -> bool {
        self.entries().iter().any(|e| e.inst.id == id && e.relaunching.is_some())
    }

    pub fn end_relaunch(&self, id: &str) {
        if let Some(e) = self.entries().iter_mut().find(|e| e.inst.id == id) {
            e.stop_relaunch();
        }
    }

    /// What is wrong with the client now, if it is still a rejoin case.
    pub fn cause_of(&self, id: &str) -> Option<CrashKind> {
        self.entries().iter().find(|e| e.inst.id == id).and_then(Entry::cause)
    }

    /// Close all: no relaunch still waiting goes ahead, and an ended client handed out but not yet
    /// picked up counts as closed by the user.
    pub fn cancel_relaunches(&self) {
        for e in self.entries().iter_mut() {
            e.stop_relaunch();
            e.closing |= e.inst.state == InstanceState::Exited;
        }
    }

    /// Clients still open that are only waiting to be closed and relaunched.
    pub fn relaunches_open(&self) -> usize {
        self.entries().iter().filter(|e| e.relaunching.is_some() && !e.closing && e.inst.state != InstanceState::Exited && e.cause().is_some()).count()
    }

    /// Clients that got disconnected, crashed or hung since the last call and may be relaunched
    /// (`wants_rejoin`), each returned once per trouble (again after it got back into a game or
    /// responds again). Drained whether or not auto-rejoin or auto-restart is on, so turning
    /// one on later doesn't relaunch old troubles.
    pub fn take_rejoins(&self) -> Vec<Rejoin> {
        let mut out = Vec::new();
        for e in self.entries().iter_mut() {
            let trouble = e.hung() || e.mem_high() || matches!(e.inst.state, InstanceState::Disconnected | InstanceState::Exited);
            if !trouble {
                // Reconnected, teleported or responding again: later trouble counts again, and
                // an earlier "Gave up" no longer says anything about it.
                e.rejoin_seen = false;
                e.gave_up = None;
                e.inst.relaunch_problem = None;
            } else if !e.rejoin_seen {
                e.rejoin_seen = true;
                if let Some(cause) = e.cause() {
                    let i = &e.inst;
                    out.push(Rejoin {
                        from: i.id.clone(),
                        account_id: i.account_id,
                        target: i.target.clone(),
                        rejoins: i.rejoins + 1,
                        cause,
                    });
                }
            }
        }
        out
    }

    /// One memory sample of every running client (`private_mb` reads it, MB), counted against
    /// `limit_mb` (`metrics::mem_strikes`; 0: off). A client over it for `MEM_STRIKES` samples
    /// in a row is handed out by `take_rejoins` as `Mem`.
    pub fn check_memory(&self, limit_mb: u32, private_mb: &dyn Fn(u32, u64) -> Option<u64>) {
        let read: Vec<(String, u32, u64)> = self
            .clients()
            .into_iter()
            .filter_map(|(id, pid, t0, _)| Some((id, pid, private_mb(pid, t0)?)))
            .collect();
        // One being closed keeps its count (and the memory it is recorded with).
        for e in self.entries().iter_mut().filter(|e| !e.closing) {
            match read.iter().find(|(id, pid, _)| *id == e.inst.id && *pid == e.inst.pid) {
                Some(&(_, _, mb)) => {
                    e.mem_strikes = mem_strikes(e.mem_strikes, mb, limit_mb);
                    e.mem_mb = mb;
                }
                None if e.inst.state != InstanceState::Exited => e.mem_strikes = 0,
                None => {}
            }
        }
    }

    /// Brings each running client's `throttled` to its last memory sample against `limit_mb`
    /// (`metrics::mem_throttled`; 0: off). The ids held, and whether any changed.
    pub fn throttle(&self, limit_mb: u32) -> (Vec<String>, bool) {
        let (mut held, mut changed) = (Vec::new(), false);
        for e in self.entries().iter_mut().filter(|e| e.inst.state != InstanceState::Exited) {
            let t = mem_throttled(e.inst.throttled, e.mem_mb, limit_mb);
            changed |= t != e.inst.throttled;
            e.inst.throttled = t;
            if t {
                held.push(e.inst.id.clone());
            }
        }
        (held, changed)
    }

    /// Forgets ended entries.
    pub fn clear_ended(&self) {
        self.entries().retain(|e| e.inst.state != InstanceState::Exited);
    }

    /// Runs for the app's lifetime (started in setup). Once a second: process liveness by
    /// (pid, start time); binds each entry to its log (`logwatch::find_log` by browser tracker
    /// id) and reads new lines; maps facts to states (Joining / InGame / Disconnected; note
    /// "Signed in as another account" when the log's userid differs from the account's,
    /// "Roblox wants a human check", "Replaced by another Roblox client"); when a process ends
    /// before reaching the game within 120 s, looks for a new client whose log names the same
    /// browser tracker id (Roblox's updater relaunches with a new PID) and rebinds to it;
    /// otherwise marks it Exited (note "Closed" if mark_closing was called, else "Closed
    /// unexpectedly"). A client whose window doesn't respond for 30 polls is hung (note "Not
    /// responding"). Clients that ended badly go to the crash history. Emits
    /// `instances://changed` whenever an entry changes. The bootstrapper case
    /// (`via_bootstrapper`): the started PID is the bootstrapper; bind to the client by log.
    /// Each poll also brings the clients to their limits (`limits::plan`: the adaptive manager
    /// by the window in front, fair cap, default trim, auto-spread), and every 10 s samples their memory
    /// for the memory restart (not while a launch or batch runs). With `clean_logs`, Roblox's old
    /// logs go hourly (`logwatch::clean`).
    pub fn spawn(app: AppHandle) {
        restore(&app);
        super::resize::start();
        let followed: Vec<u32> = app.state::<crate::App>().tracker.clients().into_iter().map(|c| c.1).collect();
        // Not on this thread: changing a window waits for its client.
        std::thread::spawn(move || super::window::recover(&followed));
        tauri::async_runtime::spawn(async move {
            let mut tick: u64 = 0;
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                tick += 1;
                let handle = app.clone();
                let (moved, rejoins, throttle_moved) = tauri::async_runtime::spawn_blocking(move || {
                    let s = handle.state::<crate::App>();
                    // Polled every second: a settings error is said once, until it reads again.
                    static SETTINGS_BAD: AtomicBool = AtomicBool::new(false);
                    let v = match s.db.settings() {
                        Ok(v) => {
                            SETTINGS_BAD.store(false, Ordering::Relaxed);
                            v
                        }
                        Err(e) => {
                            if !SETTINGS_BAD.swap(true, Ordering::Relaxed) {
                                log::warn!("tracker: could not read settings, using defaults: {e}");
                            }
                            crate::model::Settings::default()
                        }
                    };
                    let user_of = |account_id| {
                        s.db.account(account_id)
                            .inspect_err(|e| {
                                if !matches!(e, crate::error::AppError::NotFound) {
                                    // Polled every second: debug, not warn.
                                    log::debug!("tracker: could not read account {account_id}, wrong-account check off: {e}");
                                }
                            })
                            .ok()
                            .map(|a| a.user_id)
                    };
                    let dir = logwatch::logs_dir();
                    if dir.is_none() {
                        static ONCE: std::sync::Once = std::sync::Once::new();
                        ONCE.call_once(|| log::warn!("LOCALAPPDATA not set: Roblox client logs can't be followed"));
                    }
                    let moved = s.tracker.poll(dir.as_deref(), &Live, &user_of, crate::commands::now_ms());
                    // The shared login files are held only while RoALT's clients run (or launch).
                    s.multi.release_if(|| !s.tracker.any_running());
                    // Also gives a client re-adopted under a new pid its limits again.
                    let clients = s.tracker.clients();
                    super::resize::set_clients(clients.iter().map(|c| c.1).collect());
                    let (held, throttle_moved) = s.tracker.throttle(v.mem_throttle.mb);
                    if !clients.is_empty() {
                        s.limiter.sync(&super::limits::plan_now(&s, &v, &clients, &held));
                    }
                    // Once per client, without taking the focus.
                    for pid in s.tracker.first_windows(&|pid| process::main_window(pid).is_some()) {
                        let id = clients.iter().find(|c| c.1 == pid).map(|c| c.0.clone());
                        let full_quality = id.as_deref().and_then(|id| s.tracker.get(id)).and_then(|i| s.db.account(i.account_id).ok()).is_some_and(|a| a.full_quality);
                        let tiny = id.filter(|id| v.tiny_on_launch && !full_quality && s.tracker.set_micro(id, true));
                        let (size, minimize) = ((v.micro_width, v.micro_height), v.minimize_on_launch && !full_quality);
                        // Not on this thread: a busy client's ShowWindow would stop every poll.
                        tauri::async_runtime::spawn_blocking(move || {
                            if let Some(id) = tiny
                                && let Err(e) = super::window::micro(&id, pid, true, size)
                            {
                                log::debug!("instance {id}: not made tiny at launch: {e}");
                            }
                            if minimize {
                                super::window::hide_on_launch(pid);
                            }
                        });
                    }
                    // Hidden clients nobody watches go to sleep. One round at a time, off this thread.
                    static SWEEPING: AtomicBool = AtomicBool::new(false);
                    if !clients.is_empty() && !SWEEPING.swap(true, Ordering::Relaxed) {
                        let size = (v.micro_width, v.micro_height);
                        let list: Vec<_> = clients.iter().map(|c| (c.1, s.tracker.get(&c.0).is_some_and(|i| i.micro).then_some(size))).collect();
                        tauri::async_runtime::spawn_blocking(move || {
                            super::window::sweep(&list);
                            SWEEPING.store(false, Ordering::Relaxed);
                        });
                    }
                    // Skipped (not reset) while a launch, batch or rejoin runs.
                    if tick.is_multiple_of(10) && !clients.is_empty() && !s.bulk.any_busy() {
                        let mb = |pid, t0| super::metrics::read(pid, t0).map(|r| r.private >> 20);
                        s.tracker.check_memory(v.mem_restart_mb, &mb);
                    }
                    // Roblox's own logs: 30 s after start, then hourly.
                    if v.clean_logs
                        && tick % 3600 == 30
                        && let Some(dir) = logwatch::logs_dir()
                    {
                        let done = logwatch::clean(&dir, &s.tracker.live_logs());
                        log::info!("cleaned {} Roblox logs ({} MB)", done.files, done.bytes >> 20);
                    }
                    for (account, visit) in s.tracker.take_servers() {
                        if let Err(e) = s.db.add_server(account, &visit) {
                            log::warn!("could not record a server of account {account}: {e}");
                        }
                    }
                    for c in s.tracker.take_crashes() {
                        log::warn!(
                            "instance {} of account {} ended: {:?} after {} s, place {:?}, reason {:?}",
                            c.instance_id,
                            c.account_id,
                            c.kind,
                            c.uptime_ms / 1000,
                            c.place_id,
                            c.reason
                        );
                        let row = crate::storage::NewCrash {
                            instance_id: &c.instance_id,
                            account_id: c.account_id,
                            kind: c.kind,
                            at: c.at,
                            uptime_ms: c.uptime_ms,
                            reason: c.reason.as_deref(),
                            place_id: c.place_id,
                            log_path: c.log.as_ref().and_then(|p| p.to_str()),
                        };
                        if let Err(e) = s.db.add_crash(&row) {
                            log::warn!("could not record the end of instance {}: {e}", c.instance_id);
                        }
                    }
                    s.tracker.persist(&s.db, tick.is_multiple_of(10));
                    (moved, s.tracker.take_rejoins(), throttle_moved)
                })
                .await
                .unwrap_or_else(|e| {
                    log::error!("tracker poll failed: {e}");
                    Default::default()
                });
                for t in &moved {
                    let (b, a) = (&t.before, &t.after);
                    if b.state != a.state || b.note != a.note {
                        // Ordinary progress (and the user's own close) at info; problems at warn.
                        let problem = a.state == InstanceState::Disconnected || a.note.as_ref().is_some_and(|n| *n != closed());
                        let level = if problem { log::Level::Warn } else { log::Level::Info };
                        let note = a.note.as_deref().unwrap_or("");
                        log::log!(level, "instance {} of account {}: {:?} -> {:?} {note}", a.id, a.account_id, b.state, a.state);
                    }
                }
                if !moved.is_empty() || throttle_moved {
                    crate::commands::instances_changed(&app);
                    crate::discord::transitions(&app, &moved);
                }
                super::rejoin::handle(&app, rejoins);
            }
        });
    }

    /// One round over the running entries; what the UI sees of each one that changed (none:
    /// nothing changed). The files and processes are looked at on copies, outside the lock.
    fn poll(&self, dir: Option<&Path>, procs: &dyn Procs, user_of: &dyn Fn(i64) -> Option<i64>, now: i64) -> Vec<Transition> {
        let mut work: Vec<Entry> = self.entries().iter().filter(|e| e.inst.state != InstanceState::Exited).cloned().collect();
        if work.is_empty() {
            return Vec::new();
        }
        let mut taken: Vec<u32> = work.iter().filter(|e| e.adopted).map(|e| e.inst.pid).collect();
        let mut roblox = None;
        let profiles = procs.profiles();
        for e in &mut work {
            if !e.user_checked {
                e.expected_user = user_of(e.inst.account_id);
                e.user_checked = true;
            }
            observe(e, dir, procs, &mut roblox, &mut taken, now);
            // A Cordial/cage process can outlive the PID RoALT follows (or a zombie can keep
            // that PID around). The account profile is the source of truth: when it is gone,
            // the instance is gone even if the old process table entry still exists.
            if e.alive
                && let (Some(user), Some(live)) = (e.profile_user, profiles.as_ref())
                && !live.contains(&user)
            {
                log::debug!("instance {}: profile {user} no longer has a Cordial/cage process", e.inst.id);
                e.alive = false;
                if !e.adopted {
                    e.gone_at.get_or_insert(now);
                }
            }
        }
        let mut entries = self.entries();
        let mut moved = Vec::new();
        for w in work {
            // Only this loop ends entries, so a running one is still there.
            let Some(e) = entries.iter_mut().find(|e| e.inst.id == w.inst.id) else { continue };
            // A close that reached only the bootstrapper doesn't cover the client it started.
            let closing = e.closing && (e.adopted || !w.adopted);
            let old = std::mem::replace(e, w);
            e.closing = closing;
            // set_rejoins / set_limits / give_up / set_anti_afk / mark_afk may have run since the
            // copy was taken.
            e.inst.rejoins = old.inst.rejoins;
            e.inst.limits = old.inst.limits;
            (e.inst.anti_afk, e.inst.afk_at) = (old.inst.anti_afk, old.inst.afk_at);
            (e.inst.throttled, e.inst.micro, e.window_seen) = (old.inst.throttled, old.inst.micro, old.window_seen);
            (e.gave_up, e.relaunching) = (old.gave_up, old.relaunching);
            e.profile_user = old.profile_user.or(e.profile_user);
            (e.inst.relaunching, e.inst.relaunch_problem) = (old.inst.relaunching, old.inst.relaunch_problem.clone());
            let had_game = old.in_game_at.is_some();
            (e.mem_strikes, e.mem_mb, e.mem_restart) = (old.mem_strikes, old.mem_mb, old.mem_restart);
            if let Some(kind) = settle(e, now) {
                self.crashes.lock().unwrap_or_else(|e| e.into_inner()).push(ended(e, kind, now));
            }
            if e.inst != old.inst {
                moved.push(Transition { before: old.inst, after: e.inst.clone(), first_game: !had_game && e.in_game_at.is_some() });
            }
        }
        prune(&mut entries);
        moved
    }
}

/// Follows the clients saved before RoALT closed again (`readopt`), as its own (not multi-
/// instance survivors); the saved ones no longer running go from the database.
fn restore(app: &AppHandle) {
    let s = app.state::<crate::App>();
    let saved: Vec<Saved> = match s.db.tracked() {
        Ok(rows) => rows
            .iter()
            .filter_map(|j| {
                serde_json::from_str(j)
                    .inspect_err(|e| {
                        log::warn!("a saved tracked client was dropped: {:?} error at {}:{}", e.classify(), e.line(), e.column())
                    })
                    .ok()
            })
            .collect(),
        Err(e) => return log::warn!("could not read the tracked clients: {e}"),
    };
    if saved.is_empty() {
        return;
    }
    // What the database holds, so `persist` drops the ones not followed again.
    *s.tracker.persisted.lock().unwrap_or_else(|e| e.into_inner()) = saved.clone();
    let total = saved.len();
    let found = s.tracker.readopt(saved, &process::roblox_processes());
    for &(pid, t0) in &found {
        super::limits::reset_followed(pid, t0);
    }
    s.multi.adopt(&found);
    log::info!("following {} client(s) from before RoALT restarted", found.len());
    if total > found.len() {
        log::debug!("{} saved client(s) no longer running, dropped", total - found.len());
    }
    s.tracker.persist(&s.db, true);
}

/// A saved client to follow again (pure): its pid still runs as a Roblox client started at the
/// same time, and no entry follows that instance or that process already.
fn readoptable(s: &Saved, roblox: &[ProcInfo], entries: &[Entry]) -> bool {
    roblox.iter().any(|p| p.pid == s.inst.pid && p.start_time == s.start_time)
        && !entries.iter().any(|e| e.inst.id == s.inst.id || (e.inst.pid == s.inst.pid && e.inst.state != InstanceState::Exited))
}

/// One client's change in a poll: what the UI saw before and sees now.
#[derive(Clone, Debug, PartialEq)]
pub struct Transition {
    pub before: Instance,
    pub after: Instance,
    /// It reached a game for the first time in this poll.
    pub first_game: bool,
}

fn client_of(e: &Entry) -> Option<(u32, u64)> {
    (e.adopted && e.alive).then_some((e.inst.pid, e.start_time?)).filter(|&(_, t0)| t0 != 0)
}

/// A client to launch again: same account, same target.
#[derive(Clone, Debug, PartialEq)]
pub struct Rejoin {
    /// The instance that ended (or is to be closed).
    pub from: String,
    pub account_id: i64,
    pub target: LaunchTarget,
    /// For the new client: the ended one's `rejoins` + 1.
    pub rejoins: u32,
    /// Disconnect: an auto-rejoin; crash or hang: an auto-restart.
    pub cause: CrashKind,
}

/// Why a client is worth launching again, if it is (pure; the settings and the crash breaker
/// decide later whether it happens):
/// - `Disconnect`: Roblox disconnected it (kicked, server shut down, connection lost) and it
///   still shows the disconnect dialog (no problem note) or then closed by itself;
/// - `Crash`: it ended by itself ("Closed unexpectedly") with no disconnect in its log. Closing
///   Roblox's window by hand leaves no trace in its log, so that looks the same;
/// - `Hang`: its window has not responded for `HANG_POLLS` polls (no other problem note).
///
/// Never when the player left (reason 285 / 273) or another client replaced it.
pub fn wants_rejoin(inst: &Instance, facts: &LogFacts, hung: bool) -> Option<CrashKind> {
    if facts.left || facts.replaced {
        return None;
    }
    match inst.state {
        InstanceState::Exited if inst.note.as_deref() == Some(&*unexpected()) => {
            Some(if facts.disconnected { CrashKind::Disconnect } else { CrashKind::Crash })
        }
        InstanceState::Exited => None,
        _ if hung => inst.note.as_deref().is_none_or(|n| n == self::hung()).then_some(CrashKind::Hang),
        InstanceState::Disconnected if inst.note.is_none() => Some(CrashKind::Disconnect),
        _ => None,
    }
}

/// How a client that just ended ended, when badly: hung, disconnected by Roblox, or gone by
/// itself (`note` as `state_of` gave it). None for a close by the user, a replacement, a wrong
/// account or a human check (pure).
fn crash_kind(facts: &LogFacts, hung: bool, note: Option<&str>) -> Option<CrashKind> {
    if hung {
        Some(CrashKind::Hang)
    } else if facts.disconnected && !facts.left {
        Some(CrashKind::Disconnect)
    } else if note == Some(&*unexpected()) && !facts.left {
        Some(CrashKind::Crash)
    } else {
        None
    }
}

fn ended(e: &Entry, kind: CrashKind, now: i64) -> Ended {
    Ended {
        instance_id: e.inst.id.clone(),
        account_id: e.inst.account_id,
        kind,
        at: now,
        uptime_ms: (now - e.inst.started_at).max(0),
        reason: match kind {
            CrashKind::Hang => Some(hung()),
            CrashKind::Mem => Some(tr!("Commit size reached {mb} MB", mb = e.mem_mb)),
            _ => e.facts.reason.clone(),
        },
        place_id: e.inst.place_id,
        log: e.log.clone(),
    }
}

impl Default for Tracker {
    fn default() -> Self {
        Self::new()
    }
}

/// The process queries a poll makes (a fixed list in tests).
trait Procs {
    fn start_time(&self, pid: u32) -> Option<u64>;
    fn roblox(&self) -> Vec<ProcInfo>;
    fn hung(&self, pid: u32) -> bool;
    fn profile(&self, _pid: u32) -> Option<i64> {
        None
    }
    fn profiles(&self) -> Option<std::collections::HashSet<i64>> {
        None
    }
}

struct Live;

impl Procs for Live {
    fn start_time(&self, pid: u32) -> Option<u64> {
        process::start_time(pid)
    }

    fn roblox(&self) -> Vec<ProcInfo> {
        process::roblox_processes()
    }

    fn hung(&self, pid: u32) -> bool {
        process::hung(pid)
    }

    fn profile(&self, pid: u32) -> Option<i64> {
        process::profile_of(pid)
    }

    fn profiles(&self) -> Option<std::collections::HashSet<i64>> {
        process::profiles_by_user()
    }
}

/// The blocking part of a poll for one entry: liveness, rebinding, log binding and reading.
/// `roblox` lists the Roblox processes at most once per poll; `taken` holds the PIDs entries follow.
fn observe(e: &mut Entry, dir: Option<&Path>, procs: &dyn Procs, roblox: &mut Option<Vec<ProcInfo>>, taken: &mut Vec<u32>, now: i64) {
    let seen = procs.start_time(e.inst.pid);
    let first = *e.start_time.get_or_insert(seen.unwrap_or(0));
    e.alive = seen == Some(first);
    if !e.adopted && !e.alive {
        e.gone_at.get_or_insert(now);
    }
    // ponytail: one window enumeration per client per poll; batch them if clients number hundreds.
    // Kept once it is gone, so a hung client RoALT then closed still ends as hung.
    if e.alive && e.adopted {
        e.hung_polls = if procs.hung(e.inst.pid) { e.hung_polls.saturating_add(1) } else { 0 };
    }
    if e.alive && e.profile_user.is_none() {
        e.profile_user = procs.profile(e.inst.pid);
    }
    let Some(dir) = dir else { return };
    // Read back from the database: a value too large must not panic.
    let launched = UNIX_EPOCH.checked_add(Duration::from_millis(e.inst.started_at.max(0) as u64)).unwrap_or(UNIX_EPOCH);
    // A client starts after the launch (whole seconds: one of slack) and after the process it
    // replaces or that started it.
    let after = first.max(u64::try_from(e.inst.started_at / 1000 - 1).unwrap_or(0));

    // Roblox's updater relaunched it: a log created after the dead client's own (which is
    // created within seconds of its start) names the same tracker id. Judged by creation time,
    // not by the bound path: the dead client may have died before its log was bound, or the
    // relaunch's log may have been bound to the dead PID.
    if !e.alive
        && e.adopted
        && may_appear(e, now)
        && let Some(path) = logwatch::find_log(dir, e.btid, launched)
        && let Some(created) = created_secs(&path).filter(|&c| c > after + LOG_SKEW_SECS)
        && let Some(p) = pick_client(roblox.get_or_insert_with(|| procs.roblox()), Some(created), after, taken)
    {
        log::info!("instance {}: rebound from process {} to {} (updater relaunch)", e.inst.id, e.inst.pid, p.pid);
        adopt(e, p, taken);
        if e.log.as_ref() != Some(&path) {
            e.log = Some(path);
            e.offset = 0;
            e.facts = LogFacts::default();
        }
    }
    if e.log.is_none() {
        e.log = logwatch::find_log(dir, e.btid, launched);
    }
    // Behind a bootstrapper: the client is the one that wrote the log.
    if !e.adopted
        && let Some(path) = &e.log
        && let Some(p) = pick_client(roblox.get_or_insert_with(|| procs.roblox()), created_secs(path), after, taken)
    {
        log::debug!("instance {}: adopted client process {} behind bootstrapper {}", e.inst.id, p.pid, e.inst.pid);
        adopt(e, p, taken);
    }
    if let Some(path) = &e.log {
        let (account, id) = (e.inst.account_id, &e.inst.id);
        let on_problem = |p: &str, n: usize| {
            let times = if n > 1 { format!(" (x{n})") } else { String::new() };
            // Read every second and the client repeats its problems: info, not warn.
            log::info!(target: "roblox", "account {account} instance {id}: {p}{times}");
        };
        match logwatch::read_new(path, e.offset, &mut e.facts, on_problem) {
            Ok(offset) => e.offset = offset,
            Err(err) if e.log_warned => log::debug!("instance {}: client log not readable: {err}", e.inst.id),
            Err(err) => {
                e.log_warned = true;
                log::warn!("instance {}: client log not readable: {err}", e.inst.id);
            }
        }
    }
}

fn adopt(e: &mut Entry, p: ProcInfo, taken: &mut Vec<u32>) {
    e.inst.pid = p.pid;
    e.start_time = Some(p.start_time);
    e.adopted = true;
    e.alive = true;
    // Its load starts now, not with the bootstrapper or the client it replaces.
    e.loading_since = None;
    taken.push(p.pid);
}

/// The client that wrote a log: started within a few seconds of the log's creation, else the
/// newest one started since `after`. PIDs already followed are skipped.
fn pick_client(procs: &[ProcInfo], log_created: Option<u64>, after: u64, taken: &[u32]) -> Option<ProcInfo> {
    let fresh = || procs.iter().filter(|p| p.start_time >= after && !taken.contains(&p.pid));
    log_created
        .and_then(|c| fresh().filter(|p| p.start_time.abs_diff(c) <= LOG_SKEW_SECS).min_by_key(|p| p.start_time.abs_diff(c)))
        .or_else(|| fresh().max_by_key(|p| p.start_time))
        .cloned()
}

fn created_secs(path: &Path) -> Option<u64> {
    let created = std::fs::metadata(path)
        .and_then(|m| m.created())
        .inspect_err(|e| log::debug!("log creation time unreadable: {e}"))
        .ok()?;
    Some(created.duration_since(UNIX_EPOCH).ok()?.as_secs())
}

/// Too early to call a vanished client gone: it never reached a game, was not replaced, was
/// launched under 120 s ago, and wasn't closed (a close that reached only a bootstrapper gets
/// 5 s for the client it may already have started to show up).
fn may_appear(e: &Entry, now: i64) -> bool {
    #[cfg(not(windows))]
    {
        // Linux launches `cordial-run` directly: it does not hand over to an updater process,
        // so a vanished pid is the end, not a relaunch to wait 120 s for.
        let _ = (e, now);
        return false;
    }
    #[cfg(windows)]
    {
        let closed = e.closing && (e.adopted || e.gone_at.is_some_and(|g| now - g >= BOOTSTRAPPER_GRACE_MS));
        e.in_game_at.is_none() && !closed && !e.facts.replaced && now - e.inst.started_at < REBIND_MS
    }
}

const BOOTSTRAPPER_GRACE_MS: i64 = 5_000;

/// Recomputes what the UI sees (under the lock, with the current closing flag). Returns how
/// the client ended when this ended it badly (`crash_kind`).
fn settle(e: &mut Entry, now: i64) -> Option<CrashKind> {
    if e.facts.in_game && e.in_game_at.is_none() {
        e.in_game_at = Some(now);
    }
    if !e.facts.teleporting {
        e.teleport_at = None;
    } else if now - *e.teleport_at.get_or_insert(now) >= TELEPORT_MS {
        log::info!("instance {}: its teleport didn't reach a game in {} s", e.inst.id, TELEPORT_MS / 1000);
        e.facts.teleporting = false;
        e.teleport_at = None;
    }
    let ended = !e.alive && !may_appear(e, now);
    let (state, mut note) = state_of(&e.facts, e.log.is_some(), ended, e.closing, e.expected_user);
    // A client RoALT closed for its memory ends as `Mem` (its note: "Closed").
    let crash = match ended {
        true if e.closing && e.mem_restart => Some(CrashKind::Mem),
        true => crash_kind(&e.facts, e.hung(), note.as_deref()),
        false => None,
    };
    e.inst.sign_in_problem = note.as_ref().is_some_and(|n| *n == wrong_account() || *n == challenged());
    if !ended && e.hung() && note.is_none() {
        note = Some(hung());
    }
    if let Some(g) = e.gave_up.as_ref().filter(|_| !e.closing) {
        note = Some(g.clone());
    }
    e.inst.state = state;
    e.inst.note = note;
    if matches!(state, InstanceState::Starting | InstanceState::Joining) {
        e.loading_since.get_or_insert(now);
    } else {
        e.loading_since = None;
    }
    if e.facts.place_id.is_some() {
        e.inst.place_id = e.facts.place_id;
    }
    if e.facts.job_id.is_some() {
        e.inst.job_id.clone_from(&e.facts.job_id);
    }
    if ended {
        e.inst.ended_at = Some(now);
    }
    crash
}

/// State and note from what the log said, whether the log is bound, whether the client is
/// gone for good, whether the user closed it, and the account's user id (pure).
fn state_of(facts: &LogFacts, bound: bool, ended: bool, closing: bool, expected_user: Option<i64>) -> (InstanceState, Option<String>) {
    let problem = if facts.replaced {
        Some(tr!("Replaced by another Roblox client"))
    } else if matches!((facts.user_id, expected_user), (Some(got), Some(want)) if got != want) {
        Some(wrong_account())
    } else if facts.challenged {
        Some(challenged())
    } else {
        None
    };
    if ended {
        let note = if closing { closed() } else { problem.unwrap_or_else(unexpected) };
        return (InstanceState::Exited, Some(note));
    }
    let state = if facts.teleporting {
        InstanceState::Joining
    } else if facts.disconnected {
        InstanceState::Disconnected
    } else if facts.in_game {
        InstanceState::InGame
    } else if bound {
        InstanceState::Joining
    } else {
        InstanceState::Starting
    };
    (state, problem)
}

/// Keeps the newest `MAX_ENDED` ended entries, and those still being relaunched.
fn prune(entries: &mut Vec<Entry>) {
    loop {
        let ended = entries.iter().enumerate().filter(|(_, e)| e.inst.state == InstanceState::Exited && e.relaunching.is_none());
        if ended.clone().count() <= MAX_ENDED {
            return;
        }
        let Some((i, _)) = ended.min_by_key(|(_, e)| e.inst.ended_at) else { return };
        entries.remove(i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::InstanceState::*;

    // The notes in English (tests run in English).
    const WRONG_ACCOUNT: &str = "Signed in as another account";
    const CHALLENGED: &str = "Roblox wants a human check";
    const UNEXPECTED: &str = "Closed unexpectedly";
    const HUNG: &str = "Not responding";
    const BTID: i64 = 5_550_001;
    const USER: i64 = 261;

    /// Fake processes: what `roblox()` lists and `start_time` finds (bootstrappers included),
    /// which of them don't respond, and the Cordial/cage profiles seen on their command lines.
    struct Fake(
        Mutex<Vec<ProcInfo>>,
        Mutex<Vec<u32>>,
        Mutex<std::collections::HashMap<u32, i64>>,
        Mutex<Option<std::collections::HashSet<i64>>>,
    );

    impl Fake {
        fn new(procs: &[(u32, u64)]) -> Fake {
            let f = Fake(
                Mutex::new(Vec::new()),
                Mutex::new(Vec::new()),
                Mutex::new(std::collections::HashMap::new()),
                Mutex::new(Some(std::collections::HashSet::new())),
            );
            f.set(procs);
            f
        }

        fn set(&self, procs: &[(u32, u64)]) {
            *self.0.lock().unwrap() = procs.iter().map(|&(pid, start_time)| ProcInfo { pid, start_time }).collect();
        }

        fn set_hung(&self, pids: &[u32]) {
            *self.1.lock().unwrap() = pids.to_vec();
        }

        fn set_profile(&self, pid: u32, user: i64) {
            self.2.lock().unwrap().insert(pid, user);
            self.3.lock().unwrap().get_or_insert_with(Default::default).insert(user);
        }

        fn clear_live_profiles(&self) {
            self.3.lock().unwrap().get_or_insert_with(Default::default).clear();
        }
    }

    impl Procs for Fake {
        fn start_time(&self, pid: u32) -> Option<u64> {
            self.0.lock().unwrap().iter().find(|p| p.pid == pid).map(|p| p.start_time)
        }

        fn roblox(&self) -> Vec<ProcInfo> {
            self.0.lock().unwrap().clone()
        }

        fn hung(&self, pid: u32) -> bool {
            self.1.lock().unwrap().contains(&pid)
        }

        fn profile(&self, pid: u32) -> Option<i64> {
            self.2.lock().unwrap().get(&pid).copied()
        }

        fn profiles(&self) -> Option<std::collections::HashSet<i64>> {
            self.3.lock().unwrap().clone()
        }
    }

    fn now_ms() -> i64 {
        crate::commands::now_ms()
    }

    fn info(pid: u32, started_at: i64, via_bootstrapper: bool) -> LaunchInfo {
        LaunchInfo { account_id: 1, pid, browser_tracker_id: BTID, place_id: Some(1818), started_at, via_bootstrapper, warning: None, fps_cap: None, pid_start: None }
    }

    fn target() -> LaunchTarget {
        LaunchTarget::Place { place_id: 1818, launch_data: None }
    }

    #[test]
    fn live_client_per_account() {
        let t = Tracker::new();
        let id = t.register(&info(900, now_ms(), false), target()).id;
        assert!(t.has_live_client(1) && !t.has_live_client(2));
        assert!(t.claim(1).is_none(), "a running account can't be claimed");
        let claim = t.claim(2).unwrap();
        assert!(t.claim(2).is_none(), "nor one being launched");
        drop(claim);
        assert!(t.claim(2).is_some());
        t.entries().iter_mut().find(|e| e.inst.id == id).unwrap().inst.state = Disconnected;
        assert!(!t.has_live_client(1) && t.claim(1).is_some());
    }

    #[test]
    fn a_client_taken_over_is_timed_from_then() {
        let t = Tracker::new();
        let start = now_ms();
        t.register(&LaunchInfo { pid_start: Some(50), ..info(900, start, false) }, target());
        let mut entries = t.entries();
        let e = &mut entries[0];
        settle(e, start);
        let later = start + 100_000;
        adopt(e, ProcInfo { pid: 901, start_time: 60 }, &mut Vec::new());
        settle(e, later);
        assert!(e.runs_free(later + REBIND_MS - 1), "its whole load is free");
        assert!(!e.runs_free(later + REBIND_MS));
    }

    #[test]
    fn loading_hung_and_full_quality_clients_run_free() {
        let t = Tracker::new();
        let start = now_ms();
        let a = t.register(&LaunchInfo { pid_start: Some(50), ..info(900, start, false) }, target()).id;
        let free = |now| t.free(&[], now);
        let set = |f: &dyn Fn(&mut Entry)| f(t.entries().iter_mut().find(|e| e.inst.id == a).unwrap());
        let settled = |now| set(&|e| _ = settle(e, now));
        assert!(free(start).is_empty(), "not seen by a poll yet");

        // A first load: free from the poll that sees it, for as long as a load may take.
        settled(start);
        assert_eq!(free(start + REBIND_MS - 1), std::slice::from_ref(&a));
        assert!(free(start + REBIND_MS).is_empty(), "stuck loading: limited again");

        // In a game: limited.
        set(&|e| e.facts.in_game = true);
        settled(start + 200_000);
        assert_eq!(t.get(&a).unwrap().state, InGame);
        assert!(free(start + 200_000).is_empty());

        // A teleport is timed from itself, not from the launch.
        let tp = start + 3_600_000;
        set(&|e| e.facts.teleporting = true);
        settled(tp);
        assert_eq!(t.get(&a).unwrap().state, Joining);
        assert_eq!(free(tp + TELEPORT_MS - 1), std::slice::from_ref(&a));
        assert!(free(tp + TELEPORT_MS).is_empty());
        settled(tp + TELEPORT_MS);
        assert_eq!(t.get(&a).unwrap().state, InGame, "the teleport gave up");
        assert!(free(tp + TELEPORT_MS).is_empty());

        // Not responding: free well before it counts as hung.
        let later = tp + 100_000;
        set(&|e| e.hung_polls = HANG_FREE_POLLS - 1);
        assert!(free(later).is_empty());
        set(&|e| e.hung_polls = HANG_FREE_POLLS);
        assert_eq!(free(later), std::slice::from_ref(&a));
        set(&|e| e.hung_polls = 0);

        assert_eq!(t.free(&[1], later), std::slice::from_ref(&a), "Full quality");
        assert!(t.free(&[2], later).is_empty(), "another account");
    }

    #[test]
    fn each_clients_first_window_is_handed_out_once() {
        let t = Tracker::new();
        let a = t.register(&LaunchInfo { pid_start: Some(50), ..info(900, now_ms(), false) }, target()).id;
        let b = t.register(&LaunchInfo { pid_start: Some(60), ..info(901, now_ms(), true) }, target()).id;
        assert_eq!(t.first_windows(&|_| false), Vec::<u32>::new(), "no window yet");
        assert_eq!(t.first_windows(&|_| true), [900], "a bootstrapper isn't the client");
        assert_eq!(t.first_windows(&|_| true), Vec::<u32>::new(), "once");
        t.entries().iter_mut().find(|e| e.inst.id == b).unwrap().adopted = true;
        assert_eq!(t.first_windows(&|_| true), [901]);
        assert!(t.set_micro(&a, true) && t.get(&a).unwrap().micro);
        assert!(!t.set_micro("nope", true));
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("roalt-tracker-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn append(path: &Path, msgs: &[String]) {
        use std::io::Write;
        let mut f = std::fs::File::options().create(true).append(true).open(path).unwrap();
        for m in msgs {
            writeln!(f, "2026-10-19T09:08:03.437Z,0.437181,0a50,6,Info {m}").unwrap();
        }
    }

    fn btid_line(btid: i64) -> String {
        format!("[FLog::LogWin32BTId] LogWin32BTId, App, web launch, websiteBTId is {btid}")
    }

    fn join_lines(user: i64) -> Vec<String> {
        vec![
            "[FLog::Output] ! Joining game '0d6c3f4e-5b2a-4c1d-9e8f-7a6b5c4d3e2f' place 1818 at 10.9.2.90".into(),
            format!("[FLog::GameJoinLoadTime] Report game_join_loadtime: placeid:1818, join_time:0.68, userid:{user},"),
        ]
    }

    fn replicator() -> String {
        "[FLog::Network] Replicator created: 000000007A26CF00".into()
    }

    fn user_of(_: i64) -> Option<i64> {
        Some(USER)
    }

    fn state(t: &Tracker, id: &str) -> (InstanceState, Option<String>, u32) {
        let i = t.get(id).unwrap();
        (i.state, i.note, i.pid)
    }

    fn note(s: &str) -> Option<String> {
        Some(s.into())
    }

    #[test]
    fn states_from_facts() {
        let f = |set: fn(&mut LogFacts)| {
            let mut f = LogFacts::default();
            set(&mut f);
            f
        };
        let none = LogFacts::default();
        let in_game = f(|f| f.in_game = true);
        assert_eq!(state_of(&none, false, false, false, Some(USER)), (Starting, None));
        assert_eq!(state_of(&none, true, false, false, Some(USER)), (Joining, None));
        assert_eq!(state_of(&in_game, true, false, false, Some(USER)), (InGame, None));
        let dropped = f(|f| {
            f.in_game = true;
            f.disconnected = true;
        });
        assert_eq!(state_of(&dropped, true, false, false, Some(USER)), (Disconnected, None));

        let other = f(|f| f.user_id = Some(USER + 1));
        assert_eq!(state_of(&other, true, false, false, Some(USER)), (Joining, note("Signed in as another account")));
        assert_eq!(state_of(&other, true, false, false, None), (Joining, None));
        assert_eq!(state_of(&f(|f| f.user_id = Some(USER)), true, false, false, Some(USER)), (Joining, None));
        let challenged = f(|f| f.challenged = true);
        assert_eq!(state_of(&challenged, true, false, false, Some(USER)), (Joining, note("Roblox wants a human check")));

        let teleport = f(|f| (f.in_game, f.teleporting, f.disconnected) = (true, true, true));
        assert_eq!(state_of(&teleport, true, false, false, Some(USER)), (Joining, None), "not a disconnect");
        let failed = f(|f| (f.in_game, f.disconnected) = (true, true));
        assert_eq!(state_of(&failed, true, false, false, Some(USER)), (Disconnected, None));

        let replaced = f(|f| f.replaced = true);
        assert_eq!(state_of(&replaced, true, true, false, Some(USER)), (Exited, note("Replaced by another Roblox client")));
        assert_eq!(state_of(&replaced, true, true, true, Some(USER)), (Exited, note("Closed")));
        assert_eq!(state_of(&in_game, true, true, false, Some(USER)), (Exited, note("Closed unexpectedly")));
        assert_eq!(state_of(&in_game, true, true, true, Some(USER)), (Exited, note("Closed")));
        assert_eq!(state_of(&other, true, true, false, Some(USER)), (Exited, note("Signed in as another account")));
        assert_eq!(state_of(&challenged, false, true, false, Some(USER)), (Exited, note("Roblox wants a human check")));
    }

    #[test]
    fn lists_running_first_and_keeps_30_ended() {
        let t = Tracker::new();
        let ids: Vec<String> = (0..40).map(|n| t.register(&info(100 + n, 1_000 + n as i64, false), target()).id).collect();
        {
            let mut v = t.entries();
            // 35 end (the first one last), 5 keep running.
            for (n, e) in v.iter_mut().enumerate().take(35) {
                e.inst.state = Exited;
                e.inst.ended_at = Some(if n == 0 { 99_999 } else { 5_000 + n as i64 });
            }
            prune(&mut v);
        }
        let list = t.list();
        assert_eq!(list.len(), 35);
        let running: Vec<&str> = list[..5].iter().map(|i| i.id.as_str()).collect();
        assert_eq!(running, ids[35..].iter().map(String::as_str).collect::<Vec<_>>());
        assert_eq!(list[5].id, ids[0]);
        assert_eq!(list[6].id, ids[34]);
        assert_eq!(list.last().unwrap().id, ids[6]);

        t.mark_closing(&ids[0]);
        assert!(!t.entries().iter().any(|e| e.closing));
        t.clear_ended();
        assert_eq!(t.list().len(), 5);
    }

    #[test]
    fn follows_a_client_from_start_to_exit() {
        let dir = temp_dir();
        let t = Tracker::new();
        let start = now_ms();
        let secs = (start / 1000) as u64;
        let id = t.register(&info(100, start, false), target()).id;
        let procs = Fake::new(&[(100, secs)]);
        let poll = |now: i64| t.poll(Some(&dir), &procs, &user_of, now);

        assert!(poll(start + 1_000).is_empty());
        assert_eq!(state(&t, &id), (Starting, None, 100));

        let log = dir.join("0.744.0.7441080_20261024T120000Z_Player_ABCDE_last.log");
        append(&dir.join("0.744.0.7441080_20261024T120000Z_Player_ABCDE_CrashHandler_last.log"), &[btid_line(BTID)]);
        append(&log, &[btid_line(BTID)]);
        assert!(!poll(start + 2_000).is_empty());
        assert_eq!(state(&t, &id), (Joining, None, 100));
        assert!(poll(start + 3_000).is_empty());

        append(&log, &join_lines(USER));
        append(&log, &[replicator()]);
        assert!(!poll(start + 4_000).is_empty());
        let i = t.get(&id).unwrap();
        assert_eq!((i.state, i.job_id.as_deref(), i.place_id), (InGame, Some("0d6c3f4e-5b2a-4c1d-9e8f-7a6b5c4d3e2f"), Some(1818)));
        // In a game: anti-AFK may press keys in it, counting from when it got there.
        assert_eq!(t.afk_clients().iter().map(|c| (c.pid, c.since)).collect::<Vec<_>>(), [(100, start + 4_000)]);
        t.mark_afk(&id, start + 4_500);
        assert_eq!(t.afk_clients()[0].since, start + 4_500);
        assert!(t.set_anti_afk(&id, false) && t.afk_clients().is_empty());

        append(&log, &["[FLog::Network] Disconnection Notification. Reason: 267".into()]);
        let moved = poll(start + 5_000);
        assert_eq!(moved.iter().map(|m| (m.before.state, m.after.state)).collect::<Vec<_>>(), [(InGame, Disconnected)]);
        assert_eq!(state(&t, &id).0, Disconnected);
        // The opt-out and the last press outlive the poll.
        assert_eq!(t.get(&id).map(|i| (i.anti_afk, i.afk_at)), Some((false, Some(start + 4_500))));

        // Reached the game, so a vanished process is gone at once.
        procs.set(&[]);
        assert!(!poll(start + 6_000).is_empty());
        assert_eq!(state(&t, &id), (Exited, note("Closed unexpectedly"), 100));
        assert_eq!(t.get(&id).unwrap().ended_at, Some(start + 6_000));
        assert!(poll(start + 7_000).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn notes_the_wrong_account_and_a_requested_close() {
        let dir = temp_dir();
        let t = Tracker::new();
        let start = now_ms();
        let id = t.register(&info(100, start, false), target()).id;
        let procs = Fake::new(&[(100, (start / 1000) as u64)]);
        let log = dir.join("0.744.0.7441080_20261024T120000Z_Player_ABCDE_last.log");
        append(&log, &[btid_line(BTID)]);
        append(&log, &join_lines(USER + 1));
        t.poll(Some(&dir), &procs, &user_of, start + 1_000);
        assert_eq!(state(&t, &id), (Joining, note("Signed in as another account"), 100));

        t.mark_closing(&id);
        procs.set(&[]);
        t.poll(Some(&dir), &procs, &user_of, start + 2_000);
        assert_eq!(state(&t, &id), (Exited, note("Closed"), 100));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn readopts_only_the_same_running_client() {
        let old = Tracker::new();
        let start = now_ms();
        let a = old.register(&LaunchInfo { pid_start: Some(50), ..info(900, start, false) }, target()).id;
        old.register(&LaunchInfo { pid_start: Some(60), ..info(901, start, true) }, target());
        old.entries()[0].offset = 4_096;
        old.entries()[0].facts.in_game = true;
        old.entries()[0].in_game_at = Some(start + 9);
        // Per-client options travel with it.
        assert!(old.set_limits(&a, Limits { cpu_cap: Some(20), ..Limits::default() }).is_some() && old.set_anti_afk(&a, false));
        // The bootstrapper one isn't the client yet: not saved.
        let saved = old.saved();
        assert_eq!(saved.len(), 1);
        let json = serde_json::to_string(&saved).unwrap();
        let saved: Vec<Saved> = serde_json::from_str(&json).unwrap();
        assert_eq!(saved, old.saved(), "JSON round trip");

        let p = |pid, start_time| ProcInfo { pid, start_time };
        let fresh = Tracker::new();
        assert!(readoptable(&saved[0], &[p(900, 50)], &fresh.entries()));
        assert!(!readoptable(&saved[0], &[], &fresh.entries()), "gone");
        assert!(!readoptable(&saved[0], &[p(900, 51)], &fresh.entries()), "pid reused");
        assert!(!readoptable(&saved[0], &[p(900, 50)], &old.entries()), "already followed");

        assert_eq!(fresh.readopt(saved.clone(), &[p(900, 50)]), [(900, 50)]);
        assert_eq!(fresh.readopt(saved, &[p(900, 50)]), [], "not twice");
        let e = fresh.entries()[0].clone();
        assert_eq!((e.offset, e.facts.in_game, e.in_game_at, e.window_seen), (4_096, true, Some(start + 9), true));
        assert_eq!(fresh.get(&a), old.get(&a), "the UI sees it as before");
        assert_eq!(fresh.clients().len(), 1, "limits apply to it again");
    }

    #[test]
    fn a_dead_cordial_profile_ends_a_client_even_if_its_pid_is_still_listed() {
        let t = Tracker::new();
        let start = now_ms();
        let id = t.register(&info(100, start, false), target()).id;
        let procs = Fake::new(&[(100, (start / 1000) as u64)]);
        procs.set_profile(100, USER);
        {
            let mut entries = t.entries();
            let e = entries.iter_mut().find(|e| e.inst.id == id).unwrap();
            e.adopted = true;
            e.inst.state = InGame;
            e.facts.in_game = true;
            e.in_game_at = Some(start);
        }
        t.poll(None, &procs, &user_of, start + 1_000);
        assert_eq!(state(&t, &id).0, InGame, "the profile is running");
        procs.clear_live_profiles();
        t.poll(None, &procs, &user_of, start + 2_000);
        assert_eq!(state(&t, &id).0, Exited, "the process entry may linger, the profile cannot");
    }

    #[test]
    fn a_reused_pid_is_not_the_client() {
        let t = Tracker::new();
        let start = now_ms();
        let id = t.register(&info(100, start, false), target()).id;
        let procs = Fake::new(&[(100, 10)]);
        t.poll(None, &procs, &user_of, start + 1_000);
        procs.set(&[(100, 20)]);
        t.poll(None, &procs, &user_of, start + 121_000);
        assert_eq!(state(&t, &id).0, Exited);
    }

    /// A log written by a client that started at `secs`: created then, modified a bit ago.
    fn old_log(path: &Path, secs: u64) {
        #[cfg(windows)]
        let times = {
            use std::os::windows::fs::FileTimesExt;
            std::fs::FileTimes::new()
                .set_created(UNIX_EPOCH + Duration::from_secs(secs))
                .set_modified(std::time::SystemTime::now() - Duration::from_secs(2))
        };
        #[cfg(not(windows))]
        let times = std::fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(secs + 8));
        std::fs::File::options().write(true).open(path).unwrap().set_times(times).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn rebinds_after_an_updater_relaunch() {
        let dir = temp_dir();
        let t = Tracker::new();
        // Launched 10 s ago; the updater ran in between.
        let start = now_ms() - 10_000;
        let secs = (start / 1000) as u64;
        let id = t.register(&info(100, start, false), target()).id;
        let other = t.register(&LaunchInfo { browser_tracker_id: BTID + 1, ..info(300, start, false) }, target()).id;
        let procs = Fake::new(&[(100, secs), (300, secs)]);
        let poll = |now: i64| t.poll(Some(&dir), &procs, &user_of, now);

        let old = dir.join("0.743.0.7431134_20261024T120000Z_Player_AAAAA_last.log");
        append(&old, &[btid_line(BTID), "[FLog::UpdateController] Update check thread: updateRequired TRUE".into()]);
        old_log(&old, secs);
        poll(start + 1_000);
        assert_eq!(state(&t, &id), (Joining, None, 100));

        // The client quits for the update: not gone yet, nothing new to bind.
        procs.set(&[(300, secs)]);
        poll(start + 2_000);
        assert_eq!(state(&t, &id), (Joining, None, 100));

        // The relaunched client: new PID, new log naming the same tracker id. The other
        // entry's client is never picked.
        let new = dir.join("0.744.0.7441080_20261024T120010Z_Player_BBBBB_last.log");
        append(&new, &[btid_line(BTID)]);
        append(&new, &join_lines(USER));
        procs.set(&[(300, secs), (200, secs + 10)]);
        poll(start + 12_000);
        assert_eq!(state(&t, &id), (Joining, None, 200));
        append(&new, &[replicator()]);
        poll(start + 13_000);
        assert_eq!(state(&t, &id), (InGame, None, 200));
        assert_eq!(state(&t, &other), (Starting, None, 300));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn the_dead_clients_own_log_is_not_a_relaunch() {
        let dir = temp_dir();
        let t = Tracker::new();
        let start = now_ms() - 10_000;
        let secs = (start / 1000) as u64;
        let id = t.register(&info(100, start, false), target()).id;
        let procs = Fake::new(&[(100, secs)]);
        t.poll(Some(&dir), &procs, &user_of, start + 1_000);

        // It quits before its log is bound; a client the user started at about the same time
        // keeps running. Still waiting for a relaunch, not adopting that one.
        let own = dir.join("0.743.0.7431134_20261024T120000Z_Player_AAAAA_last.log");
        append(&own, &[btid_line(BTID)]);
        old_log(&own, secs);
        procs.set(&[(400, secs + 1)]);
        t.poll(Some(&dir), &procs, &user_of, start + 2_000);
        assert_eq!(state(&t, &id), (Joining, None, 100));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn a_client_gone_before_the_game_ends_after_120_s() {
        let t = Tracker::new();
        let start = now_ms();
        let id = t.register(&info(100, start, false), target()).id;
        let procs = Fake::new(&[]);
        t.poll(None, &procs, &user_of, start + 1_000);
        assert_eq!(state(&t, &id).0, Starting);
        t.poll(None, &procs, &user_of, start + 119_000);
        assert_eq!(state(&t, &id).0, Starting);
        t.poll(None, &procs, &user_of, start + 120_000);
        assert_eq!(state(&t, &id), (Exited, note("Closed unexpectedly"), 100));
    }

    #[cfg(windows)]
    #[test]
    fn adopts_the_client_behind_a_bootstrapper() {
        let dir = temp_dir();
        let t = Tracker::new();
        let start = now_ms();
        let secs = (start / 1000) as u64;
        let id = t.register(&info(900, start, true), target()).id;
        // An older Roblox (the user's own) is never the one.
        let procs = Fake::new(&[(900, secs), (50, secs - 600)]);
        let poll = |now: i64| t.poll(Some(&dir), &procs, &user_of, now);
        poll(start + 1_000);
        assert_eq!(state(&t, &id), (Starting, None, 900));

        // The bootstrapper exits after starting the client; its log appears.
        procs.set(&[(50, secs - 600), (901, secs + 1)]);
        poll(start + 3_000);
        assert_eq!(state(&t, &id), (Starting, None, 900));
        append(&dir.join("0.744.0.7441080_20261024T120002Z_Player_CCCCC_last.log"), &[btid_line(BTID)]);
        poll(start + 4_000);
        assert_eq!(state(&t, &id), (Joining, None, 901));
        procs.set(&[(50, secs - 600)]);
        poll(start + 5_000);
        assert_eq!(state(&t, &id).0, Joining);
        assert!(t.get(&id).unwrap().via_bootstrapper);
        poll(start + 121_000);
        assert_eq!(state(&t, &id), (Exited, note("Closed unexpectedly"), 901));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn closing_a_bootstrapper_leaves_its_client_running() {
        let dir = temp_dir();
        let t = Tracker::new();
        let start = now_ms();
        let secs = (start / 1000) as u64;
        let id = t.register(&info(900, start, true), target()).id;
        let procs = Fake::new(&[(900, secs)]);
        let poll = |now: i64| t.poll(Some(&dir), &procs, &user_of, now);
        poll(start + 1_000);
        assert_eq!(t.mark_closing(&id), Some((900, Some(secs))));

        // The close reached only the bootstrapper; its client shows up within the grace.
        procs.set(&[(901, secs + 1)]);
        poll(start + 3_000);
        assert_eq!(state(&t, &id), (Starting, None, 900));
        append(&dir.join("0.744.0.7441080_20261024T120002Z_Player_DDDDD_last.log"), &[btid_line(BTID)]);
        poll(start + 4_000);
        assert_eq!(state(&t, &id), (Joining, None, 901));
        // Closing applies to the client again, which then ends as closed.
        assert_eq!(t.mark_closing(&id), Some((901, Some(secs + 1))));
        procs.set(&[]);
        poll(start + 5_000);
        assert_eq!(state(&t, &id), (Exited, note("Closed"), 901));

        // No client within the grace (and no log naming it): closed.
        let empty = temp_dir();
        let poll = |now: i64| t.poll(Some(&empty), &procs, &user_of, now);
        let id = t.register(&info(910, start, true), target()).id;
        procs.set(&[(910, secs)]);
        poll(start + 1_000);
        t.mark_closing(&id);
        procs.set(&[]);
        poll(start + 2_000);
        assert_eq!(state(&t, &id).0, Starting);
        poll(start + 8_000);
        assert_eq!(state(&t, &id), (Exited, note("Closed"), 910));
        assert_eq!(t.mark_closing(&id), None);
        std::fs::remove_dir_all(&dir).unwrap();
        std::fs::remove_dir_all(&empty).unwrap();
    }

    #[test]
    fn rejoins_disconnects_and_restarts_crashes_and_hangs() {
        use CrashKind::*;
        let base = Tracker::new().register(&info(100, 0, false), target());
        let inst = |state, note: Option<&str>| Instance { state, note: note.map(Into::into), ..base.clone() };
        let facts = |disconnected, left| {
            let mut f = LogFacts::default();
            (f.disconnected, f.left) = (disconnected, left);
            f
        };
        let (kicked, none) = (facts(true, false), facts(false, false));
        assert_eq!(wants_rejoin(&inst(Disconnected, None), &kicked, false), Some(Disconnect));
        assert_eq!(wants_rejoin(&inst(Exited, Some(UNEXPECTED)), &kicked, false), Some(Disconnect), "closed by itself after the kick");
        assert_eq!(wants_rejoin(&inst(Disconnected, None), &facts(true, true), false), None, "the player left");
        assert_eq!(wants_rejoin(&inst(Exited, Some(UNEXPECTED)), &facts(true, true), false), None, "left, then closed");
        // Ended by itself with nothing in its log: a crash (or the window closed by hand).
        assert_eq!(wants_rejoin(&inst(Exited, Some(UNEXPECTED)), &none, false), Some(Crash));
        let mut replaced = none.clone();
        replaced.replaced = true;
        assert_eq!(wants_rejoin(&inst(Exited, Some(UNEXPECTED)), &replaced, false), None);
        for n in ["Closed", WRONG_ACCOUNT, CHALLENGED, "Gave up after 3 restarts"] {
            assert_eq!(wants_rejoin(&inst(Exited, Some(n)), &kicked, false), None, "{n}");
            assert_eq!(wants_rejoin(&inst(InGame, Some(n)), &none, true), None, "hung, {n}");
        }
        assert_eq!(wants_rejoin(&inst(Disconnected, Some(CHALLENGED)), &kicked, false), None);
        for state in [Starting, Joining, InGame] {
            assert_eq!(wants_rejoin(&inst(state, None), &none, false), None, "{state:?}");
            assert_eq!(wants_rejoin(&inst(state, Some(HUNG)), &none, true), Some(Hang), "{state:?}");
        }
        assert_eq!(wants_rejoin(&inst(Disconnected, Some(HUNG)), &kicked, true), Some(Hang));
        assert_eq!(wants_rejoin(&inst(Exited, Some("Closed")), &none, true), None, "a hung client RoALT closed");
    }

    #[test]
    fn what_ended_badly() {
        let facts = |disconnected, left| {
            let mut f = LogFacts::default();
            (f.disconnected, f.left) = (disconnected, left);
            f
        };
        assert_eq!(crash_kind(&facts(false, false), false, Some(UNEXPECTED)), Some(CrashKind::Crash));
        assert_eq!(crash_kind(&facts(true, false), false, Some("Closed")), Some(CrashKind::Disconnect));
        assert_eq!(crash_kind(&facts(false, false), true, Some("Closed")), Some(CrashKind::Hang));
        for note in ["Closed", "Replaced by another Roblox client", WRONG_ACCOUNT, CHALLENGED] {
            assert_eq!(crash_kind(&facts(false, false), false, Some(note)), None, "{note}");
        }
        assert_eq!(crash_kind(&facts(true, true), false, Some(UNEXPECTED)), None, "the player left");
    }

    #[test]
    fn hands_out_each_disconnect_once() {
        let dir = temp_dir();
        let t = Tracker::new();
        let start = now_ms();
        let secs = (start / 1000) as u64;
        let launch = |pid: u32, account_id: i64| LaunchInfo { account_id, browser_tracker_id: BTID + i64::from(pid), ..info(pid, start, false) };
        let follow = LaunchTarget::FollowUser { user_id: 42 };
        let kicked = t.register(&launch(100, 7), follow.clone()).id;
        let left = t.register(&launch(200, 1), target()).id;
        let closed = t.register(&launch(300, 1), target()).id;
        t.set_rejoins(&kicked, 1);
        let log = |pid: u32| dir.join(format!("0.744.0.7441080_20261024T1200{pid}Z_Player_R{pid}_last.log"));
        for pid in [100, 200, 300] {
            append(&log(pid), &[btid_line(BTID + i64::from(pid))]);
            append(&log(pid), &join_lines(USER));
            append(&log(pid), &[replicator()]);
        }
        let procs = Fake::new(&[(100, secs), (200, secs), (300, secs)]);
        let poll = |now: i64| t.poll(Some(&dir), &procs, &user_of, now);
        poll(start + 1_000);
        assert_eq!(state(&t, &kicked).0, InGame);
        assert!(t.take_rejoins().is_empty());

        // Kicked (idle, 267) and left by the player (285): only the kick is handed out, once.
        append(&log(100), &["[FLog::Network] Sending disconnect with reason: 267".into()]);
        append(&log(200), &["[FLog::Network] Disconnection Notification. Reason: 285".into()]);
        // A plain exit (the window closed, or a crash) is no disconnect: a restart.
        procs.set(&[(100, secs), (200, secs)]);
        poll(start + 2_000);
        assert_eq!(state(&t, &kicked).0, Disconnected);
        assert_eq!(state(&t, &closed), (Exited, note("Closed unexpectedly"), 300));
        let crash = Rejoin { from: closed.clone(), account_id: 1, target: target(), rejoins: 1, cause: CrashKind::Crash };
        let kick = Rejoin { from: kicked.clone(), account_id: 7, target: follow.clone(), rejoins: 2, cause: CrashKind::Disconnect };
        assert_eq!(t.take_rejoins(), vec![kick, crash]);
        assert!(t.take_rejoins().is_empty());
        // The crash went to the history; the disconnect hasn't ended yet.
        let ended = t.take_crashes();
        assert_eq!(ended.iter().map(|c| (c.instance_id.as_str(), c.kind)).collect::<Vec<_>>(), [(closed.as_str(), CrashKind::Crash)]);
        assert_eq!((ended[0].reason.as_ref(), ended[0].uptime_ms, ended[0].log.as_ref()), (None, 2_000, Some(&log(300))));
        poll(start + 3_000);
        assert!(t.take_rejoins().is_empty());

        // Reconnected, then kicked again: handed out again.
        append(&log(100), &join_lines(USER));
        append(&log(100), &[replicator()]);
        poll(start + 4_000);
        assert!(t.take_rejoins().is_empty());
        append(&log(100), &["[FLog::Network] Lost connection with reason : You have been kicked from the game".into()]);
        poll(start + 5_000);
        assert_eq!(t.take_rejoins().len(), 1);
        assert_eq!(state(&t, &left).0, Disconnected);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_waiting_relaunch_ends_with_close_all_a_clear_or_a_newer_launch() {
        let dir = temp_dir();
        let t = Tracker::new();
        let start = now_ms();
        let secs = (start / 1000) as u64;
        let id = t.register(&info(100, start, false), target()).id;
        let procs = Fake::new(&[(100, secs)]);
        let poll = |now: i64| t.poll(Some(&dir), &procs, &user_of, now);
        poll(start + 1_000);
        procs.set(&[]);
        poll(start + 200_000);
        assert_eq!(t.take_rejoins().len(), 1);
        assert!(!t.relaunch_wanted(&id, false), "no task at work");

        // One task per client, and it survives the poll.
        assert!(t.begin_relaunch(&id, start));
        assert!(!t.begin_relaunch(&id, start) && t.is_relaunching(&id));
        poll(start + 201_000);
        assert!(t.relaunch_wanted(&id, false));
        // RoALT's own close: `closing` no longer ends it, once it says so.
        t.entries()[0].closing = true;
        assert!(!t.relaunch_wanted(&id, false));
        assert!(t.relaunch_wanted(&id, true));
        t.entries()[0].closing = false;

        // Over the ended limit it stays; finished, it goes.
        for pid in 0..(MAX_ENDED as u32 + 2) {
            let other = t.register(&LaunchInfo { account_id: 50 + i64::from(pid), browser_tracker_id: BTID + 10 + i64::from(pid), ..info(1_000 + pid, start, false) }, target()).id;
            let mut entries = t.entries();
            let e = entries.iter_mut().find(|e| e.inst.id == other).unwrap();
            (e.inst.state, e.inst.ended_at) = (Exited, Some(start + 300_000 + i64::from(pid)));
        }
        prune(&mut t.entries());
        assert_eq!(t.entries().iter().filter(|e| e.inst.state == Exited).count(), MAX_ENDED + 1);
        assert!(t.get(&id).is_some());
        t.end_relaunch(&id);
        prune(&mut t.entries());
        assert!(t.get(&id).is_none());

        // Close all stops the next one, ended or not.
        let second = t.register(&LaunchInfo { account_id: 2, browser_tracker_id: BTID + 1, ..info(200, start + 1, false) }, target()).id;
        procs.set(&[(200, secs)]);
        poll(start + 400_000);
        procs.set(&[]);
        poll(start + 600_000);
        assert!(t.begin_relaunch(&second, start) && t.relaunch_wanted(&second, false));
        assert_eq!(t.relaunches_open(), 0, "ended: nothing to close");
        t.cancel_relaunches();
        assert!(!t.relaunch_wanted(&second, false) && !t.relaunch_wanted(&second, true) && !t.is_relaunching(&second));
        assert!(t.begin_relaunch(&second, start + 1));
        assert!(!t.relaunch_wanted(&second, false), "handed out after Close all: counts as closed by the user");
        t.entries().iter_mut().find(|e| e.inst.id == second).unwrap().closing = false;

        // An older ended client of the account says nothing about this relaunch.
        let older = t.register(&LaunchInfo { account_id: 2, browser_tracker_id: BTID + 3, ..info(202, start, false) }, target()).id;
        t.entries().iter_mut().find(|e| e.inst.id == older).unwrap().inst.state = Exited;
        assert!(t.relaunch_wanted(&second, false));
        t.set_relaunch_problem(&second, Some("no network".into()));
        // The account launched since it began: nothing left to do, whatever became of that client.
        let newer = t.register(&LaunchInfo { account_id: 2, browser_tracker_id: BTID + 2, ..info(201, start + 2, false) }, target()).id;
        assert!(!t.relaunch_wanted(&second, true));
        assert_eq!(t.get(&second).unwrap().relaunch_problem, None, "the new client ended the wait");
        t.entries().iter_mut().find(|e| e.inst.id == newer).unwrap().inst.state = Exited;
        assert!(!t.relaunch_wanted(&second, true));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_recovered_client_loses_its_gave_up_note() {
        let dir = temp_dir();
        let t = Tracker::new();
        let start = now_ms();
        let secs = (start / 1000) as u64;
        let id = t.register(&info(100, start, false), target()).id;
        let log = dir.join("0.744.0.7441080_20261024T120100Z_Player_R100_last.log");
        append(&log, &[btid_line(BTID)]);
        append(&log, &join_lines(USER));
        append(&log, &[replicator()]);
        let procs = Fake::new(&[(100, secs)]);
        let poll = |now: i64| t.poll(Some(&dir), &procs, &user_of, now);
        poll(start + 1_000);
        append(&log, &["[FLog::Network] Sending disconnect with reason: 267".into()]);
        poll(start + 2_000);
        assert_eq!(t.take_rejoins().len(), 1);
        t.begin_relaunch(&id, start);
        t.set_relaunch_problem(&id, Some("no network".into()));
        t.give_up(&id, "Gave up".into());
        poll(start + 3_000);
        assert_eq!(state(&t, &id).1, note("Gave up"));
        assert!(t.take_rejoins().is_empty(), "still the same trouble");

        // The player pressed Reconnect: a normal client again, and its next disconnect counts.
        append(&log, &join_lines(USER));
        append(&log, &[replicator()]);
        poll(start + 4_000);
        assert!(t.take_rejoins().is_empty());
        assert_eq!(t.get(&id).unwrap().relaunch_problem, None, "back in a game: nothing to wait for");
        poll(start + 5_000);
        assert_eq!(state(&t, &id), (InGame, None, 100));
        append(&log, &["[FLog::Network] Sending disconnect with reason: 267".into()]);
        poll(start + 6_000);
        assert_eq!(t.take_rejoins().len(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_closed_client_is_not_rejoined() {
        let dir = temp_dir();
        let t = Tracker::new();
        let start = now_ms();
        let secs = (start / 1000) as u64;
        let id = t.register(&info(100, start, false), target()).id;
        let log = dir.join("0.744.0.7441080_20261024T120100Z_Player_R100_last.log");
        append(&log, &[btid_line(BTID)]);
        append(&log, &join_lines(USER));
        append(&log, &[replicator()]);
        let procs = Fake::new(&[(100, secs)]);
        let poll = |now: i64| t.poll(Some(&dir), &procs, &user_of, now);
        poll(start + 1_000);
        t.set_rejoins(&id, 5);
        append(&log, &["[FLog::Network] Sending disconnect with reason: 267".into()]);
        poll(start + 2_000);
        assert_eq!(t.take_rejoins(), vec![Rejoin { from: id.clone(), account_id: 1, target: target(), rejoins: 6, cause: CrashKind::Disconnect }]);
        assert!(t.still_wants_rejoin(&id));

        // Closed by the user during the launch delay: no longer wanted at once, then ended as
        // closed, with the disconnect in the history; cleared: still not.
        t.mark_closing(&id);
        assert!(!t.still_wants_rejoin(&id));
        procs.set(&[]);
        poll(start + 3_000);
        assert_eq!(state(&t, &id).1, note("Closed"));
        assert!(!t.still_wants_rejoin(&id));
        let ended = t.take_crashes();
        assert_eq!((ended.len(), ended[0].kind), (1, CrashKind::Disconnect));
        assert_eq!(ended[0].reason.as_deref(), Some("Sending disconnect with reason: 267"));
        t.clear_ended();
        assert!(!t.still_wants_rejoin(&id));

        // The panic button covers ended ones waiting for a restart too.
        let crashed = t.register(&LaunchInfo { browser_tracker_id: BTID + 1, ..info(200, start, false) }, target()).id;
        procs.set(&[(200, secs)]);
        poll(start + 4_000);
        procs.set(&[]);
        poll(start + 200_000);
        assert!(t.still_wants_rejoin(&crashed));
        t.mark_all_closing();
        assert!(!t.still_wants_rejoin(&crashed));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_hung_client_is_restarted_once_and_given_up_on() {
        let t = Tracker::new();
        let start = now_ms();
        let secs = (start / 1000) as u64;
        let id = t.register(&info(100, start, false), target()).id;
        let procs = Fake::new(&[(100, secs)]);
        let poll = |n: i64| t.poll(None, &procs, &user_of, start + n * 1_000);
        poll(1);
        procs.set_hung(&[100]);
        for n in 2..=i64::from(HANG_POLLS) {
            poll(n);
        }
        assert_eq!(state(&t, &id).1, None, "not yet");
        assert!(t.take_rejoins().is_empty());
        poll(i64::from(HANG_POLLS) + 1);
        assert_eq!(state(&t, &id), (Starting, note(HUNG), 100));
        assert_eq!(t.take_rejoins().iter().map(|r| r.cause).collect::<Vec<_>>(), [CrashKind::Hang]);
        assert!(t.take_rejoins().is_empty() && t.still_wants_rejoin(&id));

        // Responding again: no longer wanted, and a later hang counts again.
        procs.set_hung(&[]);
        poll(40);
        assert_eq!(state(&t, &id).1, None);
        assert!(!t.still_wants_rejoin(&id) && t.take_rejoins().is_empty());

        // The breaker gave up: its note stays, and it is not handed out again.
        procs.set_hung(&[100]);
        for n in 41..=41 + i64::from(HANG_POLLS) {
            poll(n);
        }
        assert_eq!(t.take_rejoins().len(), 1);
        t.give_up(&id, "Gave up after 3 restarts".into());
        poll(80);
        assert_eq!(state(&t, &id).1, note("Gave up after 3 restarts"));
        assert!(!t.still_wants_rejoin(&id));

        // It then ends (never reached a game: after 120 s) as hung.
        procs.set(&[]);
        poll(200);
        assert_eq!(state(&t, &id), (Exited, note("Gave up after 3 restarts"), 100));
        let ended = t.take_crashes();
        assert_eq!((ended.len(), ended[0].kind, ended[0].reason.as_deref()), (1, CrashKind::Hang, Some(HUNG)));
    }

    #[test]
    fn a_client_over_its_memory_limit_is_restarted_once() {
        let t = Tracker::new();
        let start = now_ms();
        let secs = (start / 1000) as u64;
        let id = t.register(&info(100, start, false), target()).id;
        let procs = Fake::new(&[(100, secs)]);
        t.poll(None, &procs, &user_of, start + 1_000);
        let mb = Mutex::new(5_000u64);
        let read = |_: u32, _: u64| Some(*mb.lock().unwrap());
        t.check_memory(4_096, &read);
        t.check_memory(4_096, &read);
        assert!(t.take_rejoins().is_empty(), "two samples aren't enough");
        // Back under it: counting starts over.
        *mb.lock().unwrap() = 1_000;
        t.check_memory(4_096, &read);
        *mb.lock().unwrap() = 5_000;
        for _ in 0..MEM_STRIKES {
            t.check_memory(4_096, &read);
        }
        t.poll(None, &procs, &user_of, start + 2_000);
        assert_eq!(t.take_rejoins().iter().map(|r| r.cause).collect::<Vec<_>>(), [CrashKind::Mem]);
        assert!(t.take_rejoins().is_empty() && t.still_wants_rejoin(&id));
        t.check_memory(0, &read);
        assert!(!t.still_wants_rejoin(&id), "turned off");
        for _ in 0..MEM_STRIKES {
            t.check_memory(4_096, &read);
        }

        // RoALT closes it: it ends as a memory restart, with what it used.
        t.mark_restart(&id, Some(CrashKind::Mem));
        procs.set(&[]);
        t.poll(None, &procs, &user_of, start + 3_000);
        assert_eq!(state(&t, &id).1, note("Closed"));
        let ended = t.take_crashes();
        assert_eq!((ended.len(), ended[0].kind, ended[0].reason.as_deref()), (1, CrashKind::Mem, Some("Commit size reached 5000 MB")));
    }

    #[test]
    fn a_user_close_over_the_memory_limit_is_not_a_memory_restart() {
        let t = Tracker::new();
        let start = now_ms();
        let id = t.register(&info(100, start, false), target()).id;
        let procs = Fake::new(&[(100, (start / 1000) as u64)]);
        t.poll(None, &procs, &user_of, start + 1_000);
        for _ in 0..MEM_STRIKES {
            t.check_memory(4_096, &|_, _| Some(5_000));
        }
        t.mark_closing(&id);
        procs.set(&[]);
        t.poll(None, &procs, &user_of, start + 2_000);
        assert!(t.take_crashes().is_empty());
    }

    #[test]
    fn a_replaced_client_ends_at_once() {
        let dir = temp_dir();
        let t = Tracker::new();
        let start = now_ms();
        let id = t.register(&info(100, start, false), target()).id;
        let procs = Fake::new(&[(100, (start / 1000) as u64)]);
        let log = dir.join("0.744.0.7441080_20261024T120000Z_Player_ABCDE_last.log");
        append(&log, &[btid_line(BTID)]);
        t.poll(Some(&dir), &procs, &user_of, start + 1_000);
        append(
            &log,
            &[
                "[FLog::WndProcessCheck] WindowsAppReloadTelemetry EventWaitResultUpdate".into(),
                "[FLog::Systray] Main thread: Received close main window request".into(),
            ],
        );
        procs.set(&[]);
        t.poll(Some(&dir), &procs, &user_of, start + 2_000);
        assert_eq!(state(&t, &id), (Exited, note("Replaced by another Roblox client"), 100));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
