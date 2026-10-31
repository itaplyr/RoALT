//! Roblox client processes: listing, liveness, closing. Handles are short-lived and
//! limited-rights (Roblox's anti-cheat has flagged long-held full handles).
//!
//! Listing uses a Toolhelp snapshot (no process handles at all); sysinfo would open every
//! process with PROCESS_VM_READ and keep those handles, so it is not used here.

use std::thread::sleep;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{E_ACCESSDENIED, FILETIME, HANDLE, HWND, LPARAM, WAIT_TIMEOUT, WPARAM};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, PROCESS_ACCESS_RIGHTS, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GetForegroundWindow, GetWindow, GetWindowThreadProcessId, IsHungAppWindow, IsIconic, IsWindowVisible,
    PostMessageW, WM_CLOSE,
};
use windows::core::{BOOL, Owned};

use crate::error::{AppError, Result};

const CLIENT_EXE: &str = "RobloxPlayerBeta.exe";
const INSTALLER_EXE: &str = "RobloxPlayerInstaller.exe";
/// 1601-01-01 (FILETIME's epoch) to 1970-01-01, in 100 ns ticks.
const UNIX_EPOCH_TICKS: u64 = 116_444_736_000_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: u32,
    /// Process start, seconds since the Unix epoch (pid + start time identify a process).
    pub start_time: u64,
}

/// Running `RobloxPlayerBeta.exe` processes.
pub fn roblox_processes() -> Vec<ProcInfo> {
    named(CLIENT_EXE)
}

/// Windows has no Cordial/cage profiles to scan.
pub fn profile_of(_pid: u32) -> Option<i64> {
    None
}

/// Windows has no Cordial/cage profiles to scan.
pub fn profiles_by_user() -> Option<std::collections::HashSet<i64>> {
    None
}

/// Whether Roblox's own installer runs: it updates the client, then starts it again.
pub fn installer_running() -> bool {
    !named(INSTALLER_EXE).is_empty()
}

/// Running processes whose executable is `exe`.
fn named(exe: &str) -> Vec<ProcInfo> {
    let mut out = Vec::new();
    // SAFETY: the snapshot handle is owned and closed on drop; `e` is a zeroed PROCESSENTRY32W
    // with dwSize set, as Process32FirstW/NextW require.
    unsafe {
        let snap = match CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            Ok(snap) => snap,
            Err(e) => {
                // Polled every second: said once.
                static ONCE: std::sync::Once = std::sync::Once::new();
                ONCE.call_once(|| log::warn!("could not list processes: {e}"));
                return out;
            }
        };
        let snap = Owned::new(snap);
        let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut more = Process32FirstW(*snap, &mut e).is_ok();
        while more {
            if is_named(&e.szExeFile, exe) {
                match start_time(e.th32ProcessID) {
                    Some(start_time) => out.push(ProcInfo { pid: e.th32ProcessID, start_time }),
                    None => log::debug!("{exe} process {} can't be opened; left out", e.th32ProcessID),
                }
            }
            more = Process32NextW(*snap, &mut e).is_ok();
        }
    }
    out
}

/// Start time of a running process, `None` if it is gone.
pub fn start_time(pid: u32) -> Option<u64> {
    started(*open(pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE)?)
}

/// Asks the client to close: WM_CLOSE to its top-level windows, again after ~400 ms (Roblox may
/// show a leave prompt), then TerminateProcess if it is still alive after `grace`. `force`
/// terminates at once. `expected` is the start time the process was seen with; another
/// process now holding `pid` is left alone. Already gone → Ok.
pub fn close(pid: u32, expected: Option<u64>, force: bool, grace: Duration) -> Result<()> {
    // Pid + start time, so a pid already reused, or reused while we wait, is never touched.
    let Some(t0) = start_time(pid).filter(|t| expected.is_none_or(|e| e == *t)) else { return Ok(()) };
    let alive = || start_time(pid) == Some(t0);
    if !force {
        let deadline = Instant::now() + grace;
        post_close(pid);
        sleep(Duration::from_millis(400));
        if alive() {
            post_close(pid);
        }
        while alive() && Instant::now() < deadline {
            sleep(Duration::from_millis(100));
        }
        if !alive() {
            return Ok(());
        }
    }
    terminate(pid, t0)
}

fn terminate(pid: u32, t0: u64) -> Result<()> {
    let failed = |e: String| AppError::Internal(tr!("Could not close Roblox (process {pid}): {why}", pid = pid, why = e));
    let Some(h) = open(pid, PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE) else {
        return if start_time(pid) == Some(t0) { Err(failed(tr!("access denied"))) } else { Ok(()) };
    };
    if started(*h) != Some(t0) {
        return Ok(());
    }
    // SAFETY: `h` is a live handle with PROCESS_TERMINATE | SYNCHRONIZE, closed on drop.
    unsafe {
        if let Err(e) = TerminateProcess(*h, 1)
            && started(*h).is_some()
            // Access is denied also for a process already on its way out.
            && WaitForSingleObject(*h, 3_000) == WAIT_TIMEOUT
        {
            return Err(failed(e.message()));
        }
        // Termination is asynchronous: wait (≤ 3 s) until its objects are gone, so taking the
        // multi-instance lock right after doesn't find Roblox's event still there.
        if WaitForSingleObject(*h, 3_000) == WAIT_TIMEOUT {
            log::debug!("process {pid} still not gone 3 s after TerminateProcess");
        }
    }
    Ok(())
}

/// Posts WM_CLOSE to every top-level window (visible or not) of `pid`.
fn post_close(pid: u32) {
    unsafe extern "system" fn each(hwnd: HWND, pid: LPARAM) -> BOOL {
        let mut owner = 0u32;
        // SAFETY: `hwnd` comes from EnumWindows; PostMessageW only queues a message.
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&raw mut owner));
            if owner as isize == pid.0 {
                let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        true.into()
    }
    // SAFETY: `each` has the WNDENUMPROC signature and only uses its arguments.
    let _ = unsafe { EnumWindows(Some(each), LPARAM(pid as isize)) };
}

/// A visible top-level window of `pid` is not responding (`IsHungAppWindow`: it handled no
/// messages for 5 s). Window queries only: no process handle is opened.
pub fn hung(pid: u32) -> bool {
    struct Find {
        pid: u32,
        hung: bool,
    }
    unsafe extern "system" fn each(hwnd: HWND, data: LPARAM) -> BOOL {
        // SAFETY: `data` is the `&mut Find` passed to EnumWindows below, alive for the whole call.
        let f = unsafe { &mut *(data.0 as *mut Find) };
        let mut owner = 0u32;
        // SAFETY: `hwnd` comes from EnumWindows; these calls only read the window's state.
        let hung = unsafe {
            GetWindowThreadProcessId(hwnd, Some(&raw mut owner));
            owner == f.pid && IsWindowVisible(hwnd).as_bool() && IsHungAppWindow(hwnd).as_bool()
        };
        f.hung |= hung;
        (!hung).into()
    }
    let mut f = Find { pid, hung: false };
    // SAFETY: `each` has the WNDENUMPROC signature; `f` outlives the call. Stopping early makes
    // EnumWindows report an error, which says nothing here.
    let _ = unsafe { EnumWindows(Some(each), LPARAM(&raw mut f as isize)) };
    f.hung
}

/// The client's main window: its first visible top-level window without an owner (window
/// queries only), and whether it is minimized.
pub fn main_window(pid: u32) -> Option<(HWND, bool)> {
    struct Find {
        pid: u32,
        found: Option<HWND>,
    }
    unsafe extern "system" fn each(hwnd: HWND, data: LPARAM) -> BOOL {
        // SAFETY: `data` is the `&mut Find` passed to EnumWindows below, alive for the whole call.
        let f = unsafe { &mut *(data.0 as *mut Find) };
        let mut owner = 0u32;
        // SAFETY: `hwnd` comes from EnumWindows; these calls only read the window's state.
        let main = unsafe {
            GetWindowThreadProcessId(hwnd, Some(&raw mut owner));
            owner == f.pid && IsWindowVisible(hwnd).as_bool() && GetWindow(hwnd, GW_OWNER).is_err()
        };
        if main {
            f.found = Some(hwnd);
        }
        (!main).into()
    }
    let mut f = Find { pid, found: None };
    // SAFETY: `each` has the WNDENUMPROC signature; `f` outlives the call. Stopping early makes
    // EnumWindows report an error, which says nothing here.
    let _ = unsafe { EnumWindows(Some(each), LPARAM(&raw mut f as isize)) };
    // SAFETY: reads one window's state.
    f.found.map(|h| (h, unsafe { IsIconic(h) }.as_bool()))
}

/// The process owning the window in front (window queries only), for the adaptive manager.
pub fn foreground_pid() -> Option<u32> {
    let mut pid = 0u32;
    // SAFETY: reads which window is in front and its owner into a local; no handle is opened.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return None;
        }
        GetWindowThreadProcessId(hwnd, Some(&raw mut pid));
    }
    (pid != 0).then_some(pid)
}

pub(super) fn open(pid: u32, rights: PROCESS_ACCESS_RIGHTS) -> Option<Owned<HANDLE>> {
    // SAFETY: on success the handle is ours; Owned closes it on drop.
    match unsafe { OpenProcess(rights, false, pid) } {
        // SAFETY: as above.
        Ok(h) => Some(unsafe { Owned::new(h) }),
        Err(e) => {
            // Polled every second; a process that is gone is normal, access denied (an elevated
            // client) is said once.
            if e.code() == E_ACCESSDENIED {
                static ONCE: std::sync::Once = std::sync::Once::new();
                ONCE.call_once(|| log::warn!("process {pid}: access denied (is Roblox running as administrator?)"));
            }
            log::debug!("could not open process {pid}: {e}");
            None
        }
    }
}

/// Start of the process behind `h` in Unix seconds, `None` once it has exited (signalled).
pub(super) fn started(h: HANDLE) -> Option<u64> {
    let (mut created, mut exited, mut kernel, mut user) =
        (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: `h` is a live handle with PROCESS_QUERY_LIMITED_INFORMATION | SYNCHRONIZE; the out
    // pointers are locals.
    unsafe {
        if WaitForSingleObject(h, 0) != WAIT_TIMEOUT {
            return None;
        }
        GetProcessTimes(h, &mut created, &mut exited, &mut kernel, &mut user)
            .inspect_err(|e| log::debug!("process times unreadable: {e}"))
            .ok()?;
    }
    let ticks = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
    Some(ticks.saturating_sub(UNIX_EPOCH_TICKS) / 10_000_000)
}

/// Whether a snapshot's NUL-terminated executable name is `name` (pure).
fn is_named(exe: &[u16], name: &str) -> bool {
    let len = exe.iter().position(|&c| c == 0).unwrap_or(exe.len());
    String::from_utf16_lossy(&exe[..len]).eq_ignore_ascii_case(name)
}

#[cfg(test)]
mod tests {
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    use std::process::{Child, Command, Stdio};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// A windowless child from System32 (never Roblox).
    fn spawn(exe: &str, args: &[&str]) -> Child {
        let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| r"C:\Windows".into());
        Command::new(root.join("System32").join(exe))
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .unwrap()
    }

    fn sleeper() -> Child {
        spawn("PING.EXE", &["-n", "30", "127.0.0.1"])
    }

    #[test]
    fn start_time_of_live_and_dead_processes() {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let own = start_time(std::process::id()).unwrap();
        assert!(own <= now && own > now - 3600, "{own} vs {now}");
        assert_eq!(start_time(u32::MAX), None);
        // Exited, but our Child still holds its handle so the process object lingers: still gone.
        let mut done = spawn("cmd.exe", &["/c", "exit", "0"]);
        done.wait().unwrap();
        assert_eq!(start_time(done.id()), None);
    }

    #[test]
    fn force_close_terminates() {
        let mut child = sleeper();
        let pid = child.id();
        let t0 = start_time(pid);
        assert!(t0.is_some());
        close(pid, t0, true, Duration::from_secs(5)).unwrap();
        assert_eq!(child.try_wait().unwrap().and_then(|s| s.code()), Some(1));
        close(pid, t0, true, Duration::from_secs(5)).unwrap(); // already gone
    }

    #[test]
    fn a_process_started_at_another_time_is_left_alone() {
        let mut child = sleeper();
        let pid = child.id();
        let other = start_time(pid).map(|t| t - 10);
        close(pid, other, true, Duration::from_secs(5)).unwrap();
        assert!(child.try_wait().unwrap().is_none(), "a reused pid must not be closed");
        close(pid, None, true, Duration::from_secs(5)).unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn graceful_close_without_windows_terminates_after_grace() {
        let mut child = sleeper();
        let begun = Instant::now();
        close(child.id(), None, false, Duration::from_millis(200)).unwrap();
        assert!(begun.elapsed() >= Duration::from_millis(400));
        assert_eq!(child.try_wait().unwrap().and_then(|s| s.code()), Some(1));
    }

    #[test]
    fn windowless_and_missing_processes_are_not_hung() {
        let mut child = sleeper();
        assert!(!hung(child.id()));
        assert!(!hung(std::process::id()));
        assert!(!hung(u32::MAX));
        assert_eq!(main_window(child.id()), None);
        assert_eq!(main_window(u32::MAX), None);
        close(child.id(), None, true, Duration::from_secs(5)).unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn roblox_processes_does_not_panic() {
        let _ = roblox_processes();
        let utf16 = |s: &str| s.encode_utf16().collect::<Vec<_>>();
        assert!(is_named(&utf16("robloxplayerbeta.EXE\0junk"), CLIENT_EXE));
        assert!(!is_named(&utf16("RobloxPlayerBeta.exe.bak"), CLIENT_EXE));
        assert!(is_named(&utf16("RobloxPlayerInstaller.exe\0"), INSTALLER_EXE));
        let _ = installer_running();
    }
}
