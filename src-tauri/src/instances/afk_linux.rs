//! Anti-AFK on Linux. Injected global input isn't wired up, so the feature stays off. The pure
//! scheduling plan is kept so the UI and future backends share the same rules.
#![allow(dead_code)]


use tauri::AppHandle;

use crate::model::AfkMode;

const IDLE_MS: u32 = 5_000;

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

pub fn plan(clients: &[AfkClient], now: i64, interval_min: u32, idle_ms: u32, mode: AfkMode) -> Plan {
    let every = i64::from(interval_min) * 60_000;
    let due: Vec<String> = clients.iter().filter(|c| now - c.since >= every).map(|c| c.id.clone()).collect();
    if due.is_empty() {
        Plan::Wait
    } else if mode == AfkMode::Focus && idle_ms < IDLE_MS {
        Plan::Retry
    } else {
        Plan::Press(due)
    }
}

pub fn spawn(_app: AppHandle) {
    log::info!("anti-AFK is not available on Linux");
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
        assert_eq!(plan(&clients, 11 * 60_000, 10, 10_000, AfkMode::Background), Plan::Press(vec!["a".into()]));
        assert_eq!(plan(&clients, 11 * 60_000, 10, 0, AfkMode::Focus), Plan::Retry);
        assert_eq!(plan(&clients, 0, 10, 0, AfkMode::Focus), Plan::Wait);
    }
}
