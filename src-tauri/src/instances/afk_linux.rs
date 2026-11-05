//! Anti-AFK on Linux. `window::press_f13` picks the compositor's own input path: Hyprland's
//! targeted shortcut dispatch, Sway's `wtype`/`ydotool` after focusing the window, or X11's
//! `xdotool key`. On a desktop with none of those it returns false and the feature stays off.
#![allow(dead_code)]

use std::thread::sleep;
use std::time::Duration;

use tauri::{AppHandle, Manager};

use super::{process, window};
use crate::App;

const IDLE_MS: u32 = 5_000;
const TICK: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, PartialEq)]
pub struct AfkClient {
    pub id: String,
    pub pid: u32,
    pub since: i64,
}

#[derive(Debug, PartialEq)]
pub enum Plan {
    Wait,
    Retry,
    Press(Vec<String>),
}

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

pub fn spawn(app: AppHandle) {
    let started = std::thread::Builder::new().name("anti-afk".into()).spawn(move || loop {
        sleep(tick(&app));
    });
    if let Err(e) = started {
        log::warn!("anti-AFK not running: {e}");
    }
}

/// One round. Wayland has no portable last-input timestamp here, so presses are gated only by
/// the configured interval; F13 is unbound in games and does not move the character.
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
    let clients: Vec<_> = s.tracker.afk_clients().into_iter().filter(|c| !process::hung(c.pid)).collect();
    let ids = match plan(&clients, crate::commands::now_ms(), v.anti_afk.interval_min, u32::MAX) {
        Plan::Wait | Plan::Retry => return TICK,
        Plan::Press(ids) => ids,
    };
    for c in clients.iter().filter(|c| ids.contains(&c.id)) {
        if window::press_f13(c.pid) {
            log::info!("anti-AFK: pressed F13 in instance {}", c.id);
            s.tracker.mark_afk(&c.id, crate::commands::now_ms());
        } else {
            log::debug!("anti-AFK: instance {} has no controllable window", c.id);
        }
    }
    TICK
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(id: &str, since: i64) -> AfkClient {
        AfkClient { id: id.into(), pid: 1, since }
    }

    #[test]
    fn only_due_clients_are_pressed() {
        let clients = [c("a", 0), c("b", 10 * 60_000)];
        assert_eq!(plan(&clients, 11 * 60_000, 10, 10_000), Plan::Press(vec!["a".into()]));
        assert_eq!(plan(&clients, 11 * 60_000, 10, 0), Plan::Retry);
        assert_eq!(plan(&clients, 0, 10, 0), Plan::Wait);
    }
}
