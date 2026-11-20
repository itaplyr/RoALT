//! The main thread runs the window, the tray icon and its menu: stuck there, RoALT can't even be
//! quit from the tray. Every 5 s it is asked to answer; after 30 s without an answer what it is
//! stuck in is logged (its call stack, as module+offset: `roalt.exe` ones are named with that
//! build's PDB), and after 60 s RoALT restarts in the tray. Its clients run on and the new RoALT
//! follows them again (`Tracker::readopt`). An open menu or a window being dragged is the user, not
//! a hang.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager};
use windows::Win32::Foundation::{CloseHandle, HMODULE};
use windows::Win32::System::LibraryLoader::{GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, GetModuleFileNameW, GetModuleHandleExW};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThreadId, OpenProcess, OpenThread, PROCESS_SYNCHRONIZE, ResumeThread, SuspendThread, THREAD_GET_CONTEXT, THREAD_SUSPEND_RESUME, TerminateProcess,
    WaitForSingleObject,
};
use windows::Win32::UI::WindowsAndMessaging::{GUI_INMENUMODE, GUI_INMOVESIZE, GUI_POPUPMENUMODE, GUI_SYSTEMMENUMODE, GUITHREADINFO, GetGUIThreadInfo};
use windows::core::PCWSTR;

/// The restarted RoALT's arguments: wait for this pid to be gone, then start in the tray.
const AFTER_ARG: &str = "--after";
pub const TRAY_ARG: &str = "--tray";
/// File in the data folder holding recent watchdog restart timestamps (Unix seconds).
const RESTART_LOG: &str = "watchdog.json";
/// Cap watchdog restarts so a persistent hang doesn't loop forever every 65 s.
const MAX_RESTARTS: usize = 3;
const RESTART_WINDOW_SECS: u64 = 10 * 60;

const TICK: Duration = Duration::from_secs(5);
/// Unanswered ticks before the stack is logged (30 s), and before RoALT restarts (60 s).
const LOG_AFTER: u32 = 6;
const RESTART_AFTER: u32 = 12;

static MAIN_THREAD: AtomicU32 = AtomicU32::new(0);
/// Quitting: the main thread is busy closing, and nothing is to restart.
static EXITING: AtomicBool = AtomicBool::new(false);

pub fn exiting() {
    EXITING.store(true, Ordering::Relaxed);
}

/// Watches the main thread; called on it, once the app is set up.
pub fn spawn(app: AppHandle) {
    // SAFETY: no arguments.
    MAIN_THREAD.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
    let started = std::thread::Builder::new().name("watchdog".into()).spawn(move || {
        let answered = Arc::new(AtomicU64::new(0));
        let (mut asked, mut stuck) = (0u64, 0u32);
        loop {
            std::thread::sleep(TICK);
            if EXITING.load(Ordering::Relaxed) {
                return;
            }
            if answered.load(Ordering::Acquire) >= asked {
                if stuck >= LOG_AFTER {
                    log::info!("watchdog: the main thread answers again");
                }
                stuck = 0;
                asked += 1;
                let (answered, n) = (answered.clone(), asked);
                if app.run_on_main_thread(move || answered.store(n, Ordering::Release)).is_err() {
                    return;
                }
                continue;
            }
            if user_holds_it() {
                continue;
            }
            stuck += 1;
            if stuck == LOG_AFTER {
                log::warn!("watchdog: the main thread hasn't answered for {} s; RoALT {} is stuck in: {}", stuck * 5, env!("CARGO_PKG_VERSION"), main_stack());
            } else if stuck == RESTART_AFTER {
                log::error!("watchdog: the main thread hasn't answered for {} s; restarting in the tray (still in: {})", stuck * 5, main_stack());
                if !restart(&app) {
                    return;
                }
            }
        }
    });
    if let Err(e) = started {
        log::warn!("watchdog not running: {e}");
    }
}

/// A menu is open or a window is being moved or sized on the main thread: it pumps that loop only.
fn user_holds_it() -> bool {
    let mut gui = GUITHREADINFO { cbSize: size_of::<GUITHREADINFO>() as u32, ..Default::default() };
    // SAFETY: fills a local with cbSize set.
    unsafe { GetGUIThreadInfo(MAIN_THREAD.load(Ordering::Relaxed), &mut gui) }.is_ok()
        && gui.flags.0 & (GUI_INMENUMODE.0 | GUI_POPUPMENUMODE.0 | GUI_SYSTEMMENUMODE.0 | GUI_INMOVESIZE.0) != 0
}

/// The main thread's call stack, innermost first, as `module+0xoffset`. Read on a thread of its
/// own, given 2 s: the unwinder takes a lock the stuck thread could hold. If the reader times out
/// while the main thread is suspended, we resume it here so it is never left suspended.
fn main_stack() -> String {
    let tid = MAIN_THREAD.load(Ordering::Relaxed);
    let suspended = Arc::new(AtomicBool::new(false));
    let flag = suspended.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(read_stack(tid, &flag));
    });
    match rx.recv_timeout(Duration::from_secs(2)) {
        Ok(frames) if !frames.is_empty() => frames.into_iter().map(module_offset).collect::<Vec<_>>().join(" < "),
        Ok(_) => "(stack unreadable)".into(),
        Err(_) => {
            if suspended.swap(false, Ordering::AcqRel) {
                // SAFETY: `read_stack` suspended `tid` and has not resumed it; resume it once.
                unsafe {
                    if let Ok(h) = OpenThread(THREAD_SUSPEND_RESUME, false, tid) {
                        ResumeThread(h);
                        let _ = CloseHandle(h);
                    }
                }
            }
            "(stack reader stuck)".into()
        }
    }
}

/// True when `addr` is 8-byte aligned and points to 8 bytes of committed, readable memory.
#[cfg(target_arch = "x86_64")]
fn is_readable_stack_ptr(addr: u64) -> bool {
    use windows::Win32::System::Memory::{
        MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READ, PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOACCESS, PAGE_READONLY, PAGE_READWRITE,
        PAGE_WRITECOPY, VirtualQuery,
    };
    if addr == 0 || !addr.is_multiple_of(8) {
        return false;
    }
    let mut mbi = MEMORY_BASIC_INFORMATION::default();
    // SAFETY: queries virtual memory metadata for `addr` into a local struct.
    let n = unsafe { VirtualQuery(Some(addr as *const _), &mut mbi, size_of::<MEMORY_BASIC_INFORMATION>()) };
    if n == 0 || mbi.State != MEM_COMMIT {
        return false;
    }
    let prot = mbi.Protect.0;
    if prot & (PAGE_GUARD.0 | PAGE_NOACCESS.0) != 0 {
        return false;
    }
    const READABLE: u32 = PAGE_READONLY.0 | PAGE_READWRITE.0 | PAGE_WRITECOPY.0 | PAGE_EXECUTE_READ.0 | PAGE_EXECUTE_READWRITE.0 | PAGE_EXECUTE_WRITECOPY.0;
    if prot & READABLE == 0 {
        return false;
    }
    let region_end = (mbi.BaseAddress as u64).saturating_add(mbi.RegionSize as u64);
    addr.saturating_add(8) <= region_end
}

/// Return addresses of thread `tid`, suspended meanwhile. Nothing is allocated while it is: it may
/// hold the heap's lock.
#[cfg(target_arch = "x86_64")]
fn read_stack(tid: u32, suspended: &AtomicBool) -> Vec<u64> {
    use windows::Win32::System::Diagnostics::Debug::{CONTEXT, CONTEXT_FULL_AMD64, GetThreadContext, RtlLookupFunctionEntry, RtlVirtualUnwind, UNW_FLAG_NHANDLER};
    // GetThreadContext wants it 16-byte aligned.
    #[repr(C, align(16))]
    struct Aligned(CONTEXT);
    let mut frames = [0u64; 48];
    let mut n = 0;
    // SAFETY: a thread of this process, resumed before its handle closes; the unwind reads only
    // its stack (after verifying readability via `VirtualQuery`) and the loaded modules' unwind
    // tables, into locals.
    unsafe {
        let Ok(h) = OpenThread(THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT, false, tid) else { return Vec::new() };
        if SuspendThread(h) != u32::MAX {
            suspended.store(true, Ordering::Release);
            let mut ctx: Aligned = std::mem::zeroed();
            ctx.0.ContextFlags = CONTEXT_FULL_AMD64;
            if GetThreadContext(h, &mut ctx.0).is_ok() {
                let c = &mut ctx.0;
                while n < frames.len() && c.Rip != 0 && c.Rsp != 0 {
                    frames[n] = c.Rip;
                    n += 1;
                    let mut base = 0u64;
                    let f = RtlLookupFunctionEntry(c.Rip, &mut base, None);
                    if f.is_null() {
                        // A leaf function: its return address is on top of the stack.
                        if !is_readable_stack_ptr(c.Rsp) {
                            break;
                        }
                        c.Rip = *(c.Rsp as *const u64);
                        c.Rsp = c.Rsp.saturating_add(8);
                    } else {
                        let (mut data, mut frame) = (std::ptr::null_mut(), 0u64);
                        RtlVirtualUnwind(UNW_FLAG_NHANDLER, base, c.Rip, f, c, &mut data, &mut frame, None);
                    }
                }
            }
            if suspended.swap(false, Ordering::AcqRel) {
                ResumeThread(h);
            }
        }
        let _ = CloseHandle(h);
    }
    frames[..n].to_vec()
}

#[cfg(not(target_arch = "x86_64"))]
fn read_stack(_: u32, _: &AtomicBool) -> Vec<u64> {
    Vec::new()
}

/// `user32.dll+0x1a2b`; a bare address outside any module.
fn module_offset(addr: u64) -> String {
    let mut m = HMODULE::default();
    // SAFETY: looks up the module holding an address without keeping it loaded; the name goes into
    // a local buffer.
    unsafe {
        if GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, PCWSTR(addr as *const u16), &mut m).is_err() {
            return format!("0x{addr:x}");
        }
        let mut buf = [0u16; 260];
        let len = GetModuleFileNameW(Some(m), &mut buf) as usize;
        let path = String::from_utf16_lossy(&buf[..len]);
        let name = path.rsplit('\\').next().unwrap_or("?");
        format!("{name}+0x{:x}", addr - m.0 as u64)
    }
}

/// Records `now_secs` if fewer than `max_restarts` occurred within `window_secs`; `None` when
/// the restart cap is reached (pure).
fn record_restart_in_window(times: &[u64], now_secs: u64, window_secs: u64, max_restarts: usize) -> Option<Vec<u64>> {
    let mut recent: Vec<u64> = times.iter().copied().filter(|&t| now_secs.saturating_sub(t) < window_secs).collect();
    if recent.len() >= max_restarts {
        return None;
    }
    recent.push(now_secs);
    Some(recent)
}

/// Checks and updates `<data_dir>\watchdog.json`; returns `false` if the restart cap is reached.
fn allow_restart(data_dir: &std::path::Path) -> bool {
    let path = data_dir.join(RESTART_LOG);
    let prev: Vec<u64> = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let Some(updated) = record_restart_in_window(&prev, now, RESTART_WINDOW_SECS, MAX_RESTARTS) else {
        return false;
    };
    if let Ok(json) = serde_json::to_string(&updated) {
        let _ = std::fs::write(&path, json);
    }
    true
}

/// Does what quitting does off the main thread (at most 5 s: it may hold what that needs), starts
/// RoALT again in the tray once this one is gone, and ends this one. Returns `false` without
/// restarting when the restart cap has been reached.
fn restart(app: &AppHandle) -> bool {
    if let Some(state) = app.try_state::<crate::App>()
        && !allow_restart(&state.data_dir)
    {
        log::error!("watchdog: already restarted {MAX_RESTARTS} times in {} minutes; not restarting again", RESTART_WINDOW_SECS / 60);
        log::logger().flush();
        return false;
    }
    EXITING.store(true, Ordering::Relaxed);
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = app.clone();
    std::thread::spawn(move || {
        crate::commands::clear_copied_cookie(&handle);
        crate::exit_cleanup(&handle);
        let _ = tx.send(());
    });
    if rx.recv_timeout(Duration::from_secs(5)).is_err() {
        log::warn!("watchdog: quitting's clean-up didn't finish in 5 s; restarting anyway");
    }
    match std::env::current_exe() {
        Ok(exe) => {
            if let Err(e) = std::process::Command::new(exe).args([AFTER_ARG, &std::process::id().to_string(), TRAY_ARG]).spawn() {
                log::error!("watchdog: could not start RoALT again: {e}");
            }
        }
        Err(e) => log::error!("watchdog: could not start RoALT again: {e}"),
    }
    remove_tray_icon();
    log::logger().flush();
    // SAFETY: ends this process. Not `exit`: its DLL clean-up could wait on the stuck thread.
    let _ = unsafe { TerminateProcess(GetCurrentProcess(), 1) };
    std::process::abort()
}

/// Takes this process's tray icon away, which a killed process leaves behind until the pointer
/// passes over it. The shell knows it by tray-icon's window and its own id, counted from 1 per
/// process: the first few go, those not in use just fail.
fn remove_tray_icon() {
    use windows::Win32::UI::Shell::{NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW};
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, GetWindowThreadProcessId};
    let mut after = None;
    // SAFETY: finds windows by class and asks the shell to drop icons of the ones in this process.
    unsafe {
        while let Ok(hwnd) = FindWindowExW(None, after, windows::core::w!("tray_icon_app"), None) {
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == std::process::id() {
                for id in 1..=8 {
                    let nid = NOTIFYICONDATAW { cbSize: size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: id, ..Default::default() };
                    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
                }
            }
            after = Some(hwnd);
        }
    }
}

/// Started by `restart`: waits (at most 10 s) until the RoALT it replaces is gone, so that one isn't
/// the instance this start hands over to.
pub fn wait_for_previous() {
    let mut args = std::env::args().skip_while(|a| a != AFTER_ARG).skip(1);
    let Some(pid) = args.next().and_then(|p| p.parse::<u32>().ok()) else { return };
    // SAFETY: waits on a process handle, closed after.
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            WaitForSingleObject(h, 10_000);
            let _ = CloseHandle(h);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_blocked_threads_stack() {
        let (tid_tx, tid_rx) = std::sync::mpsc::channel();
        let (go_tx, go_rx) = std::sync::mpsc::channel::<()>();
        let t = std::thread::spawn(move || {
            // SAFETY: no arguments.
            tid_tx.send(unsafe { GetCurrentThreadId() }).unwrap();
            let _ = go_rx.recv();
        });
        let tid = tid_rx.recv().unwrap();
        std::thread::sleep(Duration::from_millis(200));
        let frames: Vec<String> = read_stack(tid, &AtomicBool::new(false)).into_iter().map(module_offset).collect();
        go_tx.send(()).unwrap();
        t.join().unwrap();
        println!("{}", frames.join(" < "));
        // Waiting in the kernel, called from this test's own code.
        assert!(frames.len() > 3, "{frames:?}");
        assert!(frames[0].starts_with("ntdll.dll+"), "{frames:?}");
        let me = std::env::current_exe().unwrap();
        let me = me.file_name().unwrap().to_string_lossy();
        assert!(frames.iter().any(|f| f.starts_with(&*me)), "{frames:?}");
    }

    #[test]
    fn restart_cap_blocks_fourth_restart_in_window_and_allows_after_expiry() {
        let t0 = 1_700_000_000u64;
        let r1 = record_restart_in_window(&[], t0, 600, 3).unwrap();
        let r2 = record_restart_in_window(&r1, t0 + 65, 600, 3).unwrap();
        let r3 = record_restart_in_window(&r2, t0 + 130, 600, 3).unwrap();
        assert_eq!(r3.len(), 3);
        assert!(record_restart_in_window(&r3, t0 + 200, 600, 3).is_none());
        // After the oldest entry ages out of the 10-minute window, restarting is allowed again.
        let r4 = record_restart_in_window(&r3, t0 + 601, 600, 3).unwrap();
        assert_eq!(r4.len(), 3);
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn stack_ptr_check_rejects_null_and_unaligned_and_accepts_stack_local() {
        assert!(!is_readable_stack_ptr(0));
        assert!(!is_readable_stack_ptr(3));
        let local: u64 = 0x1234_5678;
        let ptr = (&local as *const u64) as u64;
        assert!(is_readable_stack_ptr(ptr));
        assert!(!is_readable_stack_ptr(ptr + 1));
    }
}
