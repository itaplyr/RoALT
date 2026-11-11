//! Anti-AFK. Roblox disconnects a player after 20 minutes without input (reason 278). Every
//! `Settings.anti_afk.interval_min` minutes, each client in a game that hasn't opted out
//! (`Instance.anti_afk`) is brought to the front for a moment, gets a real F13 press through
//! `SendInput` (by scan code, as a keyboard sends it; no game binds F13, so the character
//! doesn't move), and the window that was in front gets the focus back. One client at a time,
//! about 100 ms each. Only while nobody has touched the keyboard or mouse for 5 s
//! (`GetLastInputInfo`); otherwise it tries again 30 s later. A minimized client is restored for
//! its press (`window::unhide`) and minimized again.
//!
//! Measured 2026-11-02 on minimized clients over 21 minutes: keys posted to the window
//! (WM_KEYDOWN, with or without WM_ACTIVATE) and focus without a key don't count as activity
//! and the client is kicked; a real Space or F13 does. Games with their own idle handling
//! teleport such clients back to spawn before the kick, which stops egg hatching.
//!
//! Nothing is injected into or read from the client; auto-rejoin on the 278 kick stays the
//! fallback.

use std::thread::sleep;
use std::time::Duration;

use tauri::{AppHandle, Manager};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetLastInputInfo, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
    KEYEVENTF_SCANCODE, LASTINPUTINFO, MOUSEEVENTF_MOVE, MOUSEINPUT, SendInput, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow};

use super::{process, window};
use crate::App;
use crate::commands::{instances_changed, now_ms};

/// No keyboard or mouse input for this long counts as the user being away.
const IDLE_MS: u32 = 5_000;
const TICK: Duration = Duration::from_secs(10);
/// After the user was active, or a client couldn't be brought to the front.
const RETRY: Duration = Duration::from_secs(30);
const BETWEEN: Duration = Duration::from_millis(100);
/// F13's set-1 scan code.
const F13: u16 = 0x64;
/// A restored client's time to draw again before it takes the focus.
const RESTORED: Duration = Duration::from_millis(300);

/// A client anti-AFK may press keys in (`Tracker::afk_clients`).
#[derive(Clone, Debug, PartialEq)]
pub struct AfkClient {
    pub id: String,
    pub pid: u32,
    /// Unix ms of its last press, else of reaching the game.
    pub since: i64,
}

/// What one tick does.
#[derive(Debug, PartialEq)]
pub enum Plan {
    /// Nobody is due.
    Wait,
    /// Someone is due but the user is at the PC: again in 30 s.
    Retry,
    /// Press a key in these clients (ids).
    Press(Vec<String>),
}

/// Which clients are due (`interval_min` since their `since`), gated on the user having been
/// idle for 5 s (pure).
pub fn plan(clients: &[AfkClient], now: i64, interval_min: u32, idle_ms: u32) -> Plan {
    let every = i64::from(interval_min) * 60_000;
    let due: Vec<String> = clients.iter().filter(|c| now - c.since >= every).map(|c| c.id.clone()).collect();
    if due.is_empty() {
        Plan::Wait
    } else if idle_ms < IDLE_MS {
        Plan::Retry
    } else {
        Plan::Press(due)
    }
}

/// Runs for the app's lifetime on its own thread (focus changes and `SendInput` block briefly).
pub fn spawn(app: AppHandle) {
    let started = std::thread::Builder::new().name("anti-afk".into()).spawn(move || {
        loop {
            sleep(tick(&app));
        }
    });
    if let Err(e) = started {
        log::warn!("anti-AFK not running: {e}");
    }
}

/// One round; how long to wait before the next.
fn tick(app: &AppHandle) -> Duration {
    let s = app.state::<App>();
    let v = match s.db.settings() {
        Ok(v) => v,
        Err(e) => {
            log::warn!("anti-AFK: could not read settings: {e}");
            return TICK;
        }
    };
    if !v.anti_afk.enabled {
        return TICK;
    }
    // A hung client would block this one thread, and the others would get kicked for idling.
    let clients: Vec<_> = s.tracker.afk_clients(&s.macros.busy()).into_iter().filter(|c| !process::hung(c.pid)).collect();
    let ids = match plan(&clients, now_ms(), v.anti_afk.interval_min, idle_ms()) {
        Plan::Wait => return TICK,
        Plan::Retry => return RETRY,
        Plan::Press(ids) => ids,
    };
    let mut failed = false;
    // Our own SendInput moves GetLastInputInfo too, so "the user came back" is any input newer
    // than our last injection.
    let mut ours = last_input();
    for c in clients.iter().filter(|c| ids.contains(&c.id)) {
        if last_input() != ours {
            failed = true;
            break;
        }
        let Some((hwnd, minimized)) = process::main_window(c.pid) else {
            log::debug!("anti-AFK: instance {} has no window", c.id);
            continue;
        };
        if minimized {
            window::unhide(c.pid, hwnd);
            sleep(RESTORED);
        }
        let r = press(hwnd);
        if minimized {
            window::hide(hwnd);
        }
        ours = last_input();
        match r {
            Pressed::Yes => {
                log::info!("anti-AFK: pressed F13 in instance {} (minimized: {minimized})", c.id);
                s.tracker.mark_afk(&c.id, now_ms());
            }
            Pressed::No => {
                failed = true;
                log::info!("anti-AFK: instance {} couldn't be brought to the front", c.id);
            }
            Pressed::Interrupted => {
                failed = true;
                break;
            }
        }
        sleep(BETWEEN);
    }
    instances_changed(app);
    if failed { RETRY } else { TICK }
}

/// Tick count of the last keyboard or mouse input anywhere in the session (ours included);
/// None if it can't be read.
fn last_input() -> Option<u32> {
    let mut li = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    // SAFETY: `li` is a local with cbSize set, as GetLastInputInfo requires.
    unsafe { GetLastInputInfo(&mut li) }.as_bool().then_some(li.dwTime)
}

/// How long since the last keyboard or mouse input anywhere in the session.
fn idle_ms() -> u32 {
    let Some(t) = last_input() else {
        log::warn!("anti-AFK: GetLastInputInfo failed: {}", std::io::Error::last_os_error());
        return 0;
    };
    // SAFETY: no arguments; the tick count wraps the same way dwTime does.
    unsafe { GetTickCount() }.wrapping_sub(t)
}

enum Pressed {
    Yes,
    /// Couldn't get or keep the focus: no key was sent.
    No,
    /// The user (or the restore) needs the round to stop.
    Interrupted,
}

/// F13 in `hwnd`, then the focus back where it was.
fn press(hwnd: HWND) -> Pressed {
    // SAFETY: reads which window is in front.
    let prev = unsafe { GetForegroundWindow() };
    if !to_front(hwnd) {
        return Pressed::No;
    }
    // Never into another window: only while the client is still the one in front.
    // SAFETY: reads which window is in front.
    let sent = unsafe { GetForegroundWindow() } == hwnd;
    // Input other than ours between F13 down and up: the user is back.
    let mut user = false;
    if sent {
        key(VIRTUAL_KEY(0), F13, KEYEVENTF_SCANCODE);
        let after_down = last_input();
        sleep(Duration::from_millis(40));
        user = last_input() != after_down;
        key(VIRTUAL_KEY(0), F13, KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP);
        sleep(Duration::from_millis(40));
    }
    if !prev.is_invalid() && prev != hwnd && !to_front(prev) {
        log::warn!("anti-AFK: couldn't give the focus back to the previous window; stopping this round");
        return Pressed::Interrupted;
    }
    match (sent, user) {
        (_, true) => Pressed::Interrupted,
        (true, false) => Pressed::Yes,
        (false, false) => Pressed::No,
    }
}

/// Brings `hwnd` to the front: plainly; else attached to the input of the window in front
/// (whose thread may hand the focus over); else after a zero-movement mouse input, which makes
/// this process the last input source and so lifts Windows' foreground lock without any window
/// reacting (an ALT tap would open menus).
pub(super) fn to_front(hwnd: HWND) -> bool {
    // SAFETY: reads which window is in front.
    let front = || unsafe { GetForegroundWindow() } == hwnd;
    // SAFETY: a window-manager call on a top-level window handle; one that is gone just fails.
    if unsafe { SetForegroundWindow(hwnd) }.as_bool() && front() {
        return true;
    }
    // SAFETY: attaches this thread's input to the foreground window's thread only for these
    // calls and detaches it again; both thread ids are live (ours, the window's owner).
    unsafe {
        let fg_thread = GetWindowThreadProcessId(GetForegroundWindow(), None);
        let me = GetCurrentThreadId();
        if fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, true).as_bool() {
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
            let _ = AttachThreadInput(me, fg_thread, false);
        }
    }
    if front() {
        return true;
    }
    let nudge = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi: MOUSEINPUT { dx: 0, dy: 0, dwFlags: MOUSEEVENTF_MOVE, ..Default::default() } },
    };
    // SAFETY: one fully initialised mouse INPUT and its size, as SendInput requires.
    if unsafe { SendInput(&[nudge], size_of::<INPUT>() as i32) } == 0 {
        log::debug!("anti-AFK: mouse nudge not sent: {}", std::io::Error::last_os_error());
    }
    // SAFETY: as the first SetForegroundWindow.
    let _ = unsafe { SetForegroundWindow(hwnd) };
    front()
}

/// One key event through `SendInput`.
fn key(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) {
    let input = INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    };
    // SAFETY: one fully initialised keyboard INPUT and its size, as SendInput requires.
    if unsafe { SendInput(&[input], size_of::<INPUT>() as i32) } == 0 {
        log::warn!("anti-AFK: SendInput sent no key: {}", std::io::Error::last_os_error());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(id: &str, since: i64) -> AfkClient {
        AfkClient { id: id.into(), pid: 1, since }
    }

    #[test]
    fn presses_only_due_clients_while_the_user_is_away() {
        let min = 60_000;
        let clients = [client("a", 0), client("b", 10 * min), client("c", 15 * min + 1)];
        let ids = |v: &[&str]| Plan::Press(v.iter().map(|s| s.to_string()).collect());
        // Nobody due yet.
        assert_eq!(plan(&clients, 15 * min - 1, 15, 60_000), Plan::Wait);
        assert_eq!(plan(&[], 99 * min, 15, 60_000), Plan::Wait);
        // Due exactly at the interval.
        assert_eq!(plan(&clients, 15 * min, 15, 60_000), ids(&["a"]));
        assert_eq!(plan(&clients, 25 * min, 15, 5_000), ids(&["a", "b"]));
        // The user touched the keyboard or mouse within 5 s: later.
        assert_eq!(plan(&clients, 25 * min, 15, 4_999), Plan::Retry);
        assert_eq!(plan(&clients, 25 * min, 15, 0), Plan::Retry);
        // Nobody due wins over a busy user: nothing to retry.
        assert_eq!(plan(&clients, 5 * min, 15, 0), Plan::Wait);
        assert_eq!(plan(&clients, 31 * min, 1, 10_000), ids(&["a", "b", "c"]));
    }
}
