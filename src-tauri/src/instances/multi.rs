//! Multi-instance: hold a mutex named exactly `ROBLOX_singletonEvent` (no prefix, not owned)
//! before any client starts, so a new client can't create its singleton event and doesn't
//! close the running one. Optionally, while RoALT's clients run, hold Roblox's shared login files
//! (`%LOCALAPPDATA%\Roblox\LocalStorage\RobloxCookies.dat`, `appStorage.json`) open with share mode
//! 0 so clients can't overwrite each other's session.
//!
//! When a client already owns the event, `take_over` (only on the user's confirmation) closes
//! that event's handle inside each client with `DuplicateHandle(DUPLICATE_CLOSE_SOURCE)` and then
//! takes the name. Nothing is written to the client's memory.

use std::collections::{BTreeSet, HashMap};
use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::os::windows::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    CompareObjectHandles, DUPLICATE_CLOSE_SOURCE, DUPLICATE_HANDLE_OPTIONS, DuplicateHandle, ERROR_ACCESS_DENIED, ERROR_INVALID_HANDLE,
    GENERIC_READ, HANDLE, STATUS_INFO_LENGTH_MISMATCH,
};
use windows::Win32::System::Threading::{
    CreateMutexW, GetCurrentProcess, OpenEventW, OpenMutexW, OpenProcess, PROCESS_DUP_HANDLE, SYNCHRONIZATION_SYNCHRONIZE,
};
use windows::core::{HSTRING, Owned};

use super::process;
use crate::error::{AppError, Result};
use crate::model::MultiInstanceStatus;

pub const SINGLETON_NAME: &str = "ROBLOX_singletonEvent";

fn running() -> String {
    tr!("Roblox is already running. Close it (including its tray icon) to run several clients.")
}

fn elevated() -> String {
    tr!("Roblox's lock belongs to a program running as administrator (often Roblox started by a bootstrapper set to run as administrator). Close it, or turn that off.")
}

/// How long `holders` keeps its answer: the status is asked every 5 s while the lock is wanted.
const HOLDERS_FOR: Duration = Duration::from_secs(30);

/// Processes holding Roblox's event (pid, exe name), or why they can't be found.
type Holders = std::result::Result<Vec<(u32, String)>, String>;

pub struct Multi {
    inner: Mutex<Option<Held>>,
    /// Launches between `launching` and the client being tracked; outside `Held` so turning
    /// multi-instance off and on during a launch doesn't forget it.
    launches: AtomicUsize,
    /// Why the lock couldn't be taken the last time; None once it was.
    why: Mutex<Option<String>>,
    /// `holders`' last answer and when.
    holders: Mutex<Option<(Instant, Holders)>>,
}

/// The mutex handle and the locked files; dropping it releases everything.
struct Held {
    _mutex: Singleton,
    /// Empty unless RoALT's clients run with protection on.
    files: Vec<File>,
    /// Clients (pid, start time) already running when the lock was taken: started under an
    /// earlier lock (a normal Roblox would have made `take` fail), e.g. before RoALT restarted.
    survivors: Vec<(u32, u64)>,
}

/// A launch in flight (see `Multi::launching`); ends when dropped.
pub struct Launching<'a>(&'a Multi);

impl Drop for Launching<'_> {
    fn drop(&mut self) {
        self.0.launches.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Our handle to the named mutex; closed on drop.
struct Singleton {
    _handle: Owned<HANDLE>,
}

// SAFETY: a kernel handle is valid on any thread of the process; it is never used after
// creation except by CloseHandle on drop, which may run on any thread.
unsafe impl Send for Singleton {}
// SAFETY: as above; `&Singleton` gives no access to the handle.
unsafe impl Sync for Singleton {}

impl Multi {
    pub fn new() -> Multi {
        Multi { inner: Mutex::new(None), launches: AtomicUsize::new(0), why: Mutex::new(None), holders: Mutex::new(None) }
    }

    /// Takes the lock (no-op when held). The name decides: if it already belongs to an event
    /// (`ERROR_INVALID_HANDLE`), Roblox is running unprotected and nothing is held; the error
    /// says Roblox must be closed first. Clients RoALT started under an earlier lock never
    /// created that event, so the lock can be taken again while they run (e.g. after a
    /// restart). An existing *mutex* of that name (another tool) is fine: it blocks clients the
    /// same way.
    pub fn enable(&self) -> Result<()> {
        self.take(SINGLETON_NAME)
    }

    /// Takes the lock while Roblox runs (the user confirmed it): closes the singleton event in
    /// every running client, then takes the name as `enable` does. No-op when held.
    pub fn take_over(&self) -> Result<()> {
        let mut pids: Vec<u32> = process::roblox_processes().iter().map(|p| p.pid).collect();
        if pids.is_empty() {
            pids = self.holders(SINGLETON_NAME).unwrap_or_default().into_iter().map(|(pid, _)| pid).collect();
        }
        self.take_over_at(SINGLETON_NAME, &pids)
    }

    /// Releases the mutex and the files.
    pub fn disable(&self) {
        *self.held() = None;
    }

    pub fn is_active(&self) -> bool {
        self.held().is_some()
    }

    /// Locks both shared login files while the lock is held, creating missing ones (a fresh
    /// install has none yet). No-op when both are already locked; a busy file is logged and
    /// tried again next time.
    pub fn lock_shared(&self) {
        self.lock_shared_at(&shared_files());
    }

    /// Releases the shared login files, keeping the lock.
    pub fn release_shared(&self) {
        if let Some(h) = self.held().as_mut() {
            h.files.clear();
        }
    }

    /// A launch is about to start a client: until the guard drops (after the client is
    /// tracked), `release_if` keeps the files; with `protect` they are locked now.
    pub fn launching(&self, protect: bool) -> Launching<'_> {
        self.launching_at(protect, &shared_files())
    }

    /// Releases the shared login files when no launch is in flight, no survivor runs and
    /// `idle()` (no tracked client runs) holds; checked under this lock, so a launch can't slip
    /// in between the check and the release.
    pub fn release_if(&self, idle: impl FnOnce() -> bool) {
        if let Some(h) = self.held().as_mut()
            && !h.files.is_empty()
            && !self.in_flight()
            && !survivors_alive(h)
            && idle()
        {
            h.files.clear();
        }
    }

    /// A launch hasn't got its client tracked yet.
    pub fn in_flight(&self) -> bool {
        self.launches.load(Ordering::SeqCst) > 0
    }

    /// Clients RoALT follows again after a restart (`Tracker::readopt`): its own, not survivors.
    pub fn adopt(&self, clients: &[(u32, u64)]) {
        if let Some(h) = self.held().as_mut() {
            h.survivors.retain(|s| !clients.contains(s));
        }
    }

    /// Clients from before the lock still run.
    pub fn has_survivors(&self) -> bool {
        self.held().as_mut().is_some_and(survivors_alive)
    }

    fn launching_at(&self, protect: bool, paths: &[PathBuf]) -> Launching<'_> {
        // Counted before locking: a `release_if` racing with this either sees the count or
        // runs before the relock below.
        self.launches.fetch_add(1, Ordering::SeqCst);
        if protect && let Some(h) = self.held().as_mut() {
            relock(h, paths);
        }
        Launching(self)
    }

    fn lock_shared_at(&self, paths: &[PathBuf]) {
        if let Some(h) = self.held().as_mut() {
            relock(h, paths);
        }
    }

    /// For the UI; `blocked_by` lists running RobloxPlayerBeta.exe PIDs when not active, else
    /// whatever else holds Roblox's lock.
    pub fn status(&self, enabled: bool, protect_shared_login: bool) -> MultiInstanceStatus {
        self.status_at(SINGLETON_NAME, enabled, protect_shared_login)
    }

    fn status_at(&self, name: &str, enabled: bool, protect_shared_login: bool) -> MultiInstanceStatus {
        let active = self.is_active();
        let mut blocked_by: Vec<u32> =
            if active { Vec::new() } else { process::roblox_processes().iter().map(|p| p.pid).collect() };
        let message = if !enabled || active {
            None
        } else if !blocked_by.is_empty() {
            Some(running())
        } else {
            // No client to blame: who holds Roblox's lock, else why it couldn't be taken.
            match self.holders(name) {
                Ok(h) if !h.is_empty() => {
                    blocked_by = h.iter().map(|(pid, _)| *pid).collect();
                    let names: Vec<String> = h.into_iter().map(|(pid, exe)| format!("{exe} ({pid})")).collect();
                    Some(tr!("Roblox's lock is held by {names}. Close it, or take it over.", names = names.join(", ")))
                }
                Err(why) => Some(why),
                Ok(_) => Some(self.why().clone().unwrap_or_else(|| tr!("Multi-instance isn't active yet."))),
            }
        };
        MultiInstanceStatus { enabled, active, protect_shared_login, blocked_by, message }
    }

    /// Creates (or opens) the mutex `name`; no-op when already held. A failure is kept for
    /// `status`.
    fn take(&self, name: &str) -> Result<()> {
        let mut held = self.held();
        if held.is_none() {
            let survivors = process::roblox_processes().iter().map(|p| (p.pid, p.start_time)).collect();
            let mutex = create_mutex(name);
            *self.why() = mutex.as_ref().err().map(AppError::message);
            *held = Some(Held { _mutex: mutex?, files: Vec::new(), survivors });
        }
        Ok(())
    }

    /// Who holds Roblox's event `name` (`find_holders`), kept for `HOLDERS_FOR`.
    fn holders(&self, name: &str) -> Holders {
        let mut cache = self.holders.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, h)) = cache.as_ref()
            && at.elapsed() < HOLDERS_FOR
        {
            return h.clone();
        }
        let h = find_holders(name);
        match &h {
            Ok(list) if !list.is_empty() => log::info!("multi-instance: Roblox's lock is held by {list:?}"),
            Err(e) => log::info!("multi-instance: who holds Roblox's lock can't be found: {e}"),
            Ok(_) => {}
        }
        *cache = Some((Instant::now(), h.clone()));
        h
    }

    fn why(&self) -> MutexGuard<'_, Option<String>> {
        self.why.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `take_over` for the event `name` in `pids`. A client that can't be opened is reported
    /// only if the name is still taken afterwards.
    fn take_over_at(&self, name: &str, pids: &[u32]) -> Result<()> {
        if self.is_active() {
            return Ok(());
        }
        let suffix = format!("\\{name}");
        let table: Vec<Handle> = handle_table()?.into_iter().filter(|h| pids.contains(&h.pid)).collect();
        let mut failed = None;
        for &pid in pids {
            match close_in(pid, &table, &suffix) {
                Ok(n) => log::info!("multi-instance take-over: closed {n} singleton handle(s) in process {pid}"),
                Err(e) => {
                    log::warn!("multi-instance take-over: process {pid}: {e}");
                    failed.get_or_insert(e);
                }
            }
        }
        self.take(name).map_err(|e| match (e, failed) {
            (e, Some(f)) => {
                log::debug!("multi-instance take-over: {name} still taken: {e}");
                f
            }
            (AppError::Invalid(_), None) => AppError::Invalid(still_running()),
            (e, None) => e,
        })
    }

    fn held(&self) -> MutexGuard<'_, Option<Held>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Default for Multi {
    fn default() -> Self {
        Self::new()
    }
}

/// Locks `paths` unless all are already held. Our own handles would make the new opens fail:
/// they are dropped first.
fn relock(h: &mut Held, paths: &[PathBuf]) {
    if h.files.len() < paths.len() {
        h.files.clear();
        h.files = lock_files(paths);
    }
}

/// Drops survivors that have exited; true while any runs.
fn survivors_alive(h: &mut Held) -> bool {
    h.survivors.retain(|&(pid, t)| process::start_time(pid) == Some(t));
    !h.survivors.is_empty()
}

/// Success, with or without ERROR_ALREADY_EXISTS (another tool's mutex blocks clients the same
/// way), means held. ERROR_INVALID_HANDLE means the name belongs to an event, i.e. Roblox's.
/// ERROR_ACCESS_DENIED means its creator has more rights (it runs as administrator): a mutex of
/// another tool still blocks clients, so opening it is enough; anything else is that program's.
fn create_mutex(name: &str) -> Result<Singleton> {
    let name = HSTRING::from(name);
    // SAFETY: CreateMutexW and OpenMutexW only read the name; on success the handle is ours to close.
    let h = match unsafe { CreateMutexW(None, false, &name) } {
        Ok(h) => h,
        Err(e) if e.code() == ERROR_INVALID_HANDLE.to_hresult() => return Err(AppError::Invalid(running())),
        // SAFETY: as above.
        Err(e) if e.code() == ERROR_ACCESS_DENIED.to_hresult() => unsafe { OpenMutexW(SYNCHRONIZATION_SYNCHRONIZE, false, &name) }
            .inspect(|_| log::info!("multi-instance: another program's mutex holds the name; it blocks clients the same way"))
            .map_err(|_| AppError::Invalid(elevated()))?,
        Err(e) => return Err(AppError::Internal(tr!("Could not take the multi-instance lock: {why}", why = e.message()))),
    };
    // SAFETY: as above.
    Ok(Singleton { _handle: unsafe { Owned::new(h) } })
}

fn still_running() -> String {
    tr!("Roblox's lock is still there after the take-over (a client may have just started). Close Roblox instead.")
}

/// SystemExtendedHandleInformation, ObjectNameInformation, ObjectTypeInformation.
const SYSTEM_EXTENDED_HANDLE_INFORMATION: u32 = 64;
const OBJECT_NAME_INFORMATION: u32 = 1;
const OBJECT_TYPE_INFORMATION: u32 = 2;

#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQuerySystemInformation(class: u32, info: *mut c_void, len: u32, ret_len: *mut u32) -> i32;
    fn NtQueryObject(handle: HANDLE, class: u32, info: *mut c_void, len: u32, ret_len: *mut u32) -> i32;
}

/// SYSTEM_HANDLE_TABLE_ENTRY_INFO_EX; only `pid` and `handle` are read.
#[repr(C)]
#[allow(dead_code)]
struct HandleEntry {
    object: *mut c_void,
    pid: usize,
    handle: usize,
    access: u32,
    backtrace: u16,
    type_index: u16,
    attributes: u32,
    reserved: u32,
}

/// UNICODE_STRING (heads OBJECT_NAME_INFORMATION and OBJECT_TYPE_INFORMATION).
#[repr(C)]
#[allow(dead_code)]
struct UnicodeString {
    length: u16,
    max: u16,
    buffer: *const u16,
}

/// One handle of the system's handle table. `object` reads 0 where Windows hides kernel
/// addresses from callers without admin rights (Windows 11 24H2).
#[derive(Clone, Copy, Debug)]
struct Handle {
    pid: u32,
    value: usize,
    object: usize,
    type_index: u16,
}

/// Every handle open on the system, from its handle table.
fn handle_table() -> Result<Vec<Handle>> {
    let mut size = 1usize << 20;
    loop {
        // u64s: SYSTEM_HANDLE_INFORMATION_EX needs pointer alignment.
        let mut buf = vec![0u64; size / 8];
        let mut need = 0u32;
        // SAFETY: `buf` is writable for the length given; `need` is a local.
        let st = unsafe {
            NtQuerySystemInformation(SYSTEM_EXTENDED_HANDLE_INFORMATION, buf.as_mut_ptr().cast(), size as u32, &mut need)
        };
        if st == STATUS_INFO_LENGTH_MISMATCH.0 {
            // The table grows between calls: some room on top.
            size = (need as usize + (1 << 16)).max(size * 2);
            if size > 1 << 30 {
                return Err(AppError::Internal(tr!("The system's handle table is too large to read.")));
            }
            continue;
        }
        if st < 0 {
            return Err(AppError::Internal(tr!("Could not read the system's handle table (NTSTATUS {status}).", status = format!("{st:#x}"))));
        }
        // NumberOfHandles, Reserved, then the entries.
        let count = buf[0] as usize;
        if count.saturating_mul(size_of::<HandleEntry>()).saturating_add(16) > size {
            return Err(AppError::Internal(tr!("The system's handle table came back truncated.")));
        }
        // SAFETY: the kernel wrote `count` entries right after the two-word header, inside `buf`
        // (checked above); HandleEntry is plain data with the kernel's layout.
        let entries = unsafe { std::slice::from_raw_parts(buf.as_ptr().add(2).cast::<HandleEntry>(), count) };
        return Ok(entries
            .iter()
            .filter_map(|e| {
                let pid = u32::try_from(e.pid).ok()?;
                Some(Handle { pid, value: e.handle, object: e.object as usize, type_index: e.type_index })
            })
            .collect());
    }
}

/// Closes, inside process `pid`, every event or mutant whose name ends with `suffix`; how many.
/// Each handle is copied into this process with no access rights for the queries; its type is
/// checked first and only events and mutants are asked their name (asking a pipe or file can
/// hang).
fn close_in(pid: u32, table: &[Handle], suffix: &str) -> Result<usize> {
    // SAFETY: opens the process with the one right DuplicateHandle needs; closed on drop.
    let proc = unsafe { OpenProcess(PROCESS_DUP_HANDLE, false, pid) }
        // SAFETY: as above.
        .map(|h| unsafe { Owned::new(h) })
        .map_err(|e| {
            if e.code() == ERROR_ACCESS_DENIED.to_hresult() {
                AppError::Invalid(elevated())
            } else {
                AppError::Invalid(tr!("Windows wouldn't let RoALT into Roblox (process {pid}): {why}", pid = pid, why = e.message()))
            }
        })?;
    let mut closed = 0;
    for h in table.iter().filter(|h| h.pid == pid) {
        let source = HANDLE(h.value as *mut c_void);
        let Some(copy) = dup(*proc, h.value) else { continue };
        if !matches!(object_string(*copy, OBJECT_TYPE_INFORMATION).as_deref(), Some("Event" | "Mutant")) {
            continue;
        }
        if !object_string(*copy, OBJECT_NAME_INFORMATION).is_some_and(|n| n.ends_with(suffix)) {
            continue;
        }
        // The client may have closed `source` and reused its value since: close it only while it
        // still names the object just checked (a fresh copy, compared right before the close).
        let Some(again) = dup(*proc, h.value) else { continue };
        // SAFETY: both are our own handles.
        if !unsafe { CompareObjectHandles(*copy, *again) }.as_bool() {
            continue;
        }
        // SAFETY: closes `source` inside `proc` (DUPLICATE_CLOSE_SOURCE, no copy is made); no
        // memory of the process is touched.
        let r = unsafe { DuplicateHandle(*proc, source, HANDLE::default(), std::ptr::null_mut(), 0, false, DUPLICATE_CLOSE_SOURCE) };
        match r {
            Ok(()) => closed += 1,
            Err(e) => log::warn!("multi-instance take-over: could not close a handle in process {pid}: {e}"),
        }
    }
    Ok(closed)
}

/// A copy of handle `value` of process `proc` (opened with PROCESS_DUP_HANDLE) in this process,
/// with no access rights; None when that handle is gone.
fn dup(proc: HANDLE, value: usize) -> Option<Owned<HANDLE>> {
    let mut copy = HANDLE::default();
    // SAFETY: `proc` has PROCESS_DUP_HANDLE; `value` is a handle value in that process (one
    // closed since just fails); the copy is ours, with no access, closed on drop.
    unsafe { DuplicateHandle(proc, HANDLE(value as *mut c_void), GetCurrentProcess(), &mut copy, 0, false, DUPLICATE_HANDLE_OPTIONS(0)) }.ok()?;
    // SAFETY: as above.
    Some(unsafe { Owned::new(copy) })
}

/// The processes with a handle to the event `name` (pid, exe name), found through a handle of
/// our own to it. Err when Windows won't give us one (its creator runs as administrator); none
/// when there is no such event (or the name is a mutex: another tool's, which blocks clients).
fn find_holders(name: &str) -> Holders {
    // SAFETY: OpenEventW only reads the name; the handle is ours, closed on drop.
    let ours = match unsafe { OpenEventW(SYNCHRONIZATION_SYNCHRONIZE, false, &HSTRING::from(name)) } {
        // SAFETY: as above.
        Ok(h) => unsafe { Owned::new(h) },
        Err(e) if e.code() == ERROR_ACCESS_DENIED.to_hresult() => return Err(elevated()),
        Err(_) => return Ok(Vec::new()),
    };
    let table = handle_table().map_err(|e| e.message())?;
    let Some(mine) = table.iter().find(|h| h.pid == std::process::id() && h.value == ours.0 as usize).copied() else {
        return Ok(Vec::new());
    };
    let pids = holding(&table, mine, *ours, mine.object != 0);
    Ok(pids.into_iter().map(|pid| (pid, process::exe_name(pid).unwrap_or_else(|| tr!("process {pid}", pid = pid)))).collect())
}

/// The pids with a handle to the object behind `mine` (our handle `ours`), `mine` left out: by
/// kernel address, else (address hidden) by comparing each handle of its type with ours.
fn holding(table: &[Handle], mine: Handle, ours: HANDLE, by_address: bool) -> BTreeSet<u32> {
    let others = table.iter().filter(|h| !(h.pid == mine.pid && h.value == mine.value));
    if by_address {
        return others.filter(|h| h.object == mine.object).map(|h| h.pid).collect();
    }
    let mut procs: HashMap<u32, Option<Owned<HANDLE>>> = HashMap::new();
    let mut pids = BTreeSet::new();
    for h in others.filter(|h| h.type_index == mine.type_index) {
        if pids.contains(&h.pid) {
            continue;
        }
        // SAFETY: opens the process with the one right DuplicateHandle needs; closed on drop.
        let proc = procs.entry(h.pid).or_insert_with(|| unsafe { OpenProcess(PROCESS_DUP_HANDLE, false, h.pid) }.ok().map(|p| unsafe { Owned::new(p) }));
        // SAFETY: both are our own handles.
        if let Some(proc) = proc
            && dup(**proc, h.value).is_some_and(|copy| unsafe { CompareObjectHandles(ours, *copy) }.as_bool())
        {
            pids.insert(h.pid);
        }
    }
    pids
}

/// The type or name (`class`) of the object behind `h`; None if it can't be read or is longer
/// than 4 KiB.
fn object_string(h: HANDLE, class: u32) -> Option<String> {
    let mut buf = vec![0u64; 512];
    let mut len = 0u32;
    // SAFETY: `h` is our own handle; `buf` is writable and pointer-aligned for the length given.
    let st = unsafe { NtQueryObject(h, class, buf.as_mut_ptr().cast(), (buf.len() * 8) as u32, &mut len) };
    if st < 0 {
        return None;
    }
    // SAFETY: on success both classes start with a UNICODE_STRING whose buffer (if any) points
    // into `buf`, which is alive and unchanged here.
    unsafe {
        let us = &*buf.as_ptr().cast::<UnicodeString>();
        if us.buffer.is_null() {
            return Some(String::new());
        }
        Some(String::from_utf16_lossy(std::slice::from_raw_parts(us.buffer, usize::from(us.length) / 2)))
    }
}

/// Opens each file read-only with share mode 0 so nothing else can open it until it is
/// dropped. A missing file (and its folder) is created empty, as evanovar does (OPEN_ALWAYS:
/// `write` only lets std create it, `access_mode` keeps the access read-only, nothing is
/// truncated). A busy one is logged and skipped (the mutex alone still works).
fn lock_files(paths: &[PathBuf]) -> Vec<File> {
    paths
        .iter()
        .filter_map(|p| {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            match OpenOptions::new().write(true).create(true).access_mode(GENERIC_READ.0).share_mode(0).open(p) {
                Ok(f) => Some(f),
                Err(e) => {
                    log::warn!("could not lock {}: {e}", p.display());
                    None
                }
            }
        })
        .collect()
}

fn shared_files() -> Vec<PathBuf> {
    let Some(base) = std::env::var_os("LOCALAPPDATA") else {
        log::warn!("LOCALAPPDATA not set: shared Roblox login files not locked");
        return Vec::new();
    };
    let dir = PathBuf::from(base).join("Roblox").join("LocalStorage");
    vec![dir.join("RobloxCookies.dat"), dir.join("appStorage.json")]
}

#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::ERROR_SHARING_VIOLATION;
    use windows::Win32::System::Threading::CreateEventW;

    use super::*;

    /// Never the real name: tests must not touch Roblox's singleton.
    fn test_name() -> String {
        format!("RoALT_test_singleton_{}", uuid::Uuid::new_v4())
    }

    /// What a starting client does: create an event of that name (closed again right away).
    fn event(name: &str) -> windows::core::Result<Owned<HANDLE>> {
        // SAFETY: CreateEventW only reads the name; the handle is closed on drop.
        unsafe { CreateEventW(None, false, false, &HSTRING::from(name)).map(|h| Owned::new(h)) }
    }

    #[test]
    fn holds_and_releases_the_name() {
        let name = test_name();
        let (a, b) = (Multi::new(), Multi::new());
        assert!(!a.status(false, false).active && a.status(false, false).message.is_none());
        a.take(&name).unwrap();
        assert!(a.is_active());
        let s = a.status(true, true);
        assert!(s.active && s.blocked_by.is_empty() && s.message.is_none() && s.protect_shared_login);
        // A client can't create its event while the mutex holds the name.
        assert_eq!(event(&name).unwrap_err().code(), ERROR_INVALID_HANDLE.to_hresult());
        // Creating the same mutex again (ERROR_ALREADY_EXISTS) still counts as held.
        b.take(&name).unwrap();
        assert!(b.is_active());
        a.disable();
        b.disable();
        assert!(!a.is_active() && !b.is_active());
        event(&name).unwrap();
    }

    #[test]
    fn an_existing_event_means_roblox_runs() {
        let name = test_name();
        let _roblox = event(&name).unwrap();
        let m = Multi::new();
        let err = m.take(&name).unwrap_err();
        assert!(matches!(err, AppError::Invalid(ref s) if *s == running()), "{err}");
        assert!(!m.is_active());
    }

    #[test]
    fn says_why_the_lock_was_not_taken() {
        let name = test_name();
        let roblox = event(&name).unwrap();
        let m = Multi::new();
        assert!(m.take(&name).is_err());
        assert_eq!(m.why().as_deref(), Some(&*running()));
        drop(roblox);
        m.take(&name).unwrap();
        assert!(m.why().is_none(), "held: no reason left");
        m.disable();
    }

    #[test]
    fn finds_who_holds_the_event() {
        let name = test_name();
        let _roblox = event(&name).unwrap();
        let me = std::process::id();
        let found = find_holders(&name).unwrap();
        assert_eq!(found.iter().map(|h| h.0).collect::<Vec<_>>(), [me]);
        assert!(found[0].1.to_ascii_lowercase().ends_with(".exe"), "{found:?}");

        // Where Windows hides kernel addresses: compared handle by handle, the same answer.
        // SAFETY: as in `find_holders`.
        let ours = unsafe { Owned::new(OpenEventW(SYNCHRONIZATION_SYNCHRONIZE, false, &HSTRING::from(&name)).unwrap()) };
        let table = handle_table().unwrap();
        let mine = *table.iter().find(|h| h.pid == me && h.value == ours.0 as usize).unwrap();
        assert_eq!(holding(&table, mine, *ours, false), BTreeSet::from([me]));
        if mine.object != 0 {
            assert_eq!(holding(&table, mine, *ours, true), BTreeSet::from([me]));
        }

        // No such event, or a mutex of that name (another tool's): nobody to name.
        assert!(find_holders(&test_name()).unwrap().is_empty());
        let other = test_name();
        let tool = Multi::new();
        tool.take(&other).unwrap();
        assert!(find_holders(&other).unwrap().is_empty());
        tool.disable();

        // The status names the holder when no Roblox client is running to blame.
        let m = Multi::new();
        assert!(m.take(&name).is_err());
        let s = m.status_at(&name, true, false);
        if process::roblox_processes().is_empty() {
            assert_eq!(s.blocked_by, [me]);
            assert!(s.message.as_deref().is_some_and(|t| t.contains(&found[0].1)), "{s:?}");
        }
    }

    #[test]
    fn take_over_closes_the_event_in_its_owner_only() {
        let (name, other) = (test_name(), test_name());
        // Our own process plays Roblox: its event handle gets closed by the take-over, so it
        // must not be closed again on drop.
        std::mem::forget(event(&name).unwrap());
        let _kept = event(&other).unwrap();
        let m = Multi::new();
        assert!(m.take(&name).is_err());
        m.take_over_at(&name, &[std::process::id()]).unwrap();
        assert!(m.is_active());
        // An event of another name is left alone.
        assert!(matches!(create_mutex(&other), Err(AppError::Invalid(ref s)) if *s == running()));
        m.disable();
        // Nobody to take over from: the plain error.
        let _roblox = event(&name).unwrap();
        let err = Multi::new().take_over_at(&name, &[]).unwrap_err();
        assert!(matches!(err, AppError::Invalid(ref s) if *s == still_running()), "{err}");
    }

    #[test]
    fn survivors_count_only_while_they_run() {
        let m = Multi::new();
        m.take(&test_name()).unwrap();
        let mut child = std::process::Command::new("PING.EXE")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let t = process::start_time(child.id()).unwrap();
        m.held().as_mut().unwrap().survivors = vec![(child.id(), t), (child.id(), t - 5)];
        assert!(m.has_survivors());
        assert_eq!(m.held().as_ref().unwrap().survivors.len(), 1);
        m.adopt(&[(child.id(), t)]);
        assert!(!m.has_survivors(), "followed again: RoALT's own");
        m.held().as_mut().unwrap().survivors = vec![(child.id(), t)];
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(!m.has_survivors());
        m.disable();
    }

    #[test]
    fn locks_shared_files_while_held_and_creates_missing_ones() {
        let dir = std::env::temp_dir().join(format!("roalt-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("RobloxCookies.dat");
        std::fs::write(&file, b"{}").unwrap();
        let missing = dir.join("LocalStorage").join("appStorage.json");
        let files = [file.clone(), missing.clone()];
        let locked = |p: &PathBuf| File::open(p).unwrap_err().raw_os_error() == Some(ERROR_SHARING_VIOLATION.0 as i32);

        let m = Multi::new();
        m.lock_shared_at(&files); // not held: nothing
        File::open(&file).unwrap();
        let name = test_name();
        m.take(&name).unwrap();
        m.held().as_mut().unwrap().survivors.clear(); // a real Roblox on this machine must not count
        m.lock_shared_at(&files);
        assert!(locked(&file) && locked(&missing));
        m.lock_shared_at(&files); // both held: kept, not reopened
        assert!(locked(&file) && locked(&missing));
        // A busy file is skipped, not an error.
        let other = Multi::new();
        other.take(&test_name()).unwrap();
        other.lock_shared_at(&files);
        assert!(other.is_active());

        // A launch in flight keeps them; so does a tracked client (idle false).
        let launch = m.launching_at(true, &files);
        m.release_if(|| true);
        assert!(locked(&file));
        drop(launch);
        m.release_if(|| false);
        assert!(locked(&file));
        m.release_if(|| true);
        File::open(&file).unwrap();
        // Turning multi-instance off and on during a launch keeps it counted.
        let launch = m.launching_at(false, &files);
        m.disable();
        m.take(&name).unwrap();
        m.held().as_mut().unwrap().survivors.clear();
        assert!(m.in_flight());
        drop(launch);
        assert!(!m.in_flight());
        m.lock_shared_at(&files);

        // Releasing frees the files but keeps the lock; contents are untouched.
        m.release_shared();
        assert_eq!(std::fs::read(&file).unwrap(), b"{}");
        assert!(m.is_active());
        m.lock_shared_at(&files);
        assert!(locked(&file));
        m.disable();
        File::open(&file).unwrap();
        other.disable();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
