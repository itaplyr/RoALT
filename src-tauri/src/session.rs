//! Windows locking and going to sleep (`WM_WTSSESSION_CHANGE`, `WM_POWERBROADCAST`), followed on
//! a thread with a hidden window of its own (a message-only window gets no power broadcasts).
//! Then a master-password vault locks (`settings.lock_on_session_lock`) unless running clients
//! hold it open (`vault_held_open`). While Windows is locked anti-AFK can't press keys (`locked`):
//! it says so once on Discord.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Manager};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTS_CURRENT_SESSION, WTS_SESSIONSTATE_LOCK, WTSFreeMemory, WTSINFOEXW, WTSQuerySessionInformationW,
    WTSRegisterSessionNotification, WTSSessionInfoEx,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, PBT_APMSUSPEND, RegisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_POWERBROADCAST, WM_WTSSESSION_CHANGE, WNDCLASSW, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
};
use windows::core::{PWSTR, w};

use crate::App;
use crate::model::VaultMode;

static LOCKED: AtomicBool = AtomicBool::new(false);
static APP: OnceLock<AppHandle> = OnceLock::new();

/// Windows is locked (the lock screen is up), as last told.
pub fn locked() -> bool {
    LOCKED.load(Ordering::Relaxed)
}

/// Runs for the app's lifetime.
pub fn spawn(app: AppHandle) {
    let _ = APP.set(app);
    let started = std::thread::Builder::new().name("session".into()).spawn(|| {
        if let Err(e) = run() {
            log::warn!("Windows locking and sleep not followed: {e}");
        }
    });
    if let Err(e) = started {
        log::warn!("Windows locking and sleep not followed: {e}");
    }
}

fn run() -> windows::core::Result<()> {
    // SAFETY: a hidden window of this thread, kept for the app's lifetime; the loop is its pump.
    unsafe {
        let class = WNDCLASSW { lpfnWndProc: Some(on_message), lpszClassName: w!("roalt-session"), ..Default::default() };
        RegisterClassW(&class);
        let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), w!("roalt-session"), w!(""), WINDOW_STYLE(0), 0, 0, 0, 0, None, None, None, None)?;
        WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION)?;
        // RoALT can start on a locked PC (the watchdog restarts it in the tray).
        LOCKED.store(locked_now(), Ordering::Relaxed);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

/// Whether the session is locked now (`WTSSessionInfoEx`); false when it can't be read.
fn locked_now() -> bool {
    let mut buf = PWSTR::null();
    let mut len = 0;
    // SAFETY: Windows allocates `buf` (a WTSINFOEXW) for this call; it's read, then freed.
    unsafe {
        if WTSQuerySessionInformationW(None, WTS_CURRENT_SESSION, WTSSessionInfoEx, &mut buf, &mut len).is_err() || buf.is_null() {
            return false;
        }
        let info = &*(buf.0 as *const WTSINFOEXW);
        let locked = info.Level == 1 && info.Data.WTSInfoExLevel1.SessionFlags == WTS_SESSIONSTATE_LOCK as i32;
        WTSFreeMemory(buf.0 as _);
        locked
    }
}

unsafe extern "system" fn on_message(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let what = w.0 as u32;
    match msg {
        WM_WTSSESSION_CHANGE if what == WTS_SESSION_LOCK => {
            LOCKED.store(true, Ordering::Relaxed);
            went_away(false);
        }
        WM_WTSSESSION_CHANGE if what == WTS_SESSION_UNLOCK => LOCKED.store(false, Ordering::Relaxed),
        WM_POWERBROADCAST if what == PBT_APMSUSPEND => went_away(true),
        _ => {}
    }
    // SAFETY: the default for every message (TRUE for a power broadcast).
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

/// Whether Windows locking or sleeping locks the vault (pure): a master password, the setting on,
/// not locked already, and nothing running that needs it open.
fn should_lock(mode: VaultMode, setting: bool, already: bool, held_open: bool) -> bool {
    mode == VaultMode::Master && setting && !already && !held_open
}

/// Windows was locked, or is going to sleep (`asleep`). Blocks: before sleep the vault must be
/// locked by the time this returns.
fn went_away(asleep: bool) {
    let Some(app) = APP.get() else { return };
    let s = app.state::<App>();
    // Unreadable settings lock all the same (the default): a security check fails closed.
    let v = s.db.settings().unwrap_or_else(|e| {
        log::warn!("Windows {}: settings unreadable, using the defaults: {e}", if asleep { "sleeps" } else { "locked" });
        crate::model::Settings::default()
    });
    if should_lock(s.vault.mode(), v.lock_on_session_lock, s.vault.is_locked(), crate::vault_held_open(app)) {
        log::info!("vault locked: Windows {}", if asleep { "goes to sleep" } else { "was locked" });
        crate::commands::lock_vault_now(app);
    }
    if !asleep && v.anti_afk.enabled && !s.tracker.afk_clients().is_empty() {
        crate::discord::afk_blocked(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_unneeded_master_vault_locks() {
        assert!(should_lock(VaultMode::Master, true, false, false));
        assert!(!should_lock(VaultMode::Dpapi, true, false, false), "DPAPI has nothing to lock");
        assert!(!should_lock(VaultMode::Master, false, false, false), "turned off");
        assert!(!should_lock(VaultMode::Master, true, true, false), "already locked");
        assert!(!should_lock(VaultMode::Master, true, false, true), "running clients need it");
    }
}
