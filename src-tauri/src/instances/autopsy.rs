//! What a client that ended badly leaves behind, written into RoALT's log (so into the diagnostics
//! report too): how long it really lived, the PC's state, the end of its logs cleaned line by line
//! (`diagnostics::redact`), a crash dump if Roblox's crash handler wrote one, Windows' own record of
//! the fault, and what all that points to. The process's exit code isn't here: reading it would mean
//! holding a handle on the client, which Roblox's anti-cheat has flagged.

use std::os::windows::process::CommandExt;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::tracker::Ended;
use super::{logwatch, metrics, process};
use crate::diagnostics::clean_tail;
use crate::model::CrashKind;

/// Lines of the client's log and of its crash handler's.
const CLIENT_LINES: usize = 30;
const HANDLER_LINES: usize = 5;
/// Roblox's lines can be very long.
const MAX_LINE: usize = 300;
/// The crash handler names the client's log in its first lines.
const HANDLER_HEAD: u64 = 64 * 1024;
const CLIENT_EXE: &str = "RobloxPlayerBeta.exe";

/// Logs the whole autopsy. Blocking (it reads files and asks Windows): on a thread of its own.
pub fn report(c: &Ended) {
    let id = &c.instance_id;
    let launched = UNIX_EPOCH + Duration::from_millis(c.started_at.max(0) as u64);
    let logs = logwatch::logs_dir();
    let dumps = logs.as_deref().map_or_else(Vec::new, |d| dumps_since(d, launched));
    let faults = windows_faults(c);
    log::warn!("autopsy {id}: {}", verdict(c, c.log.is_some(), !dumps.is_empty(), !faults.is_empty()));
    let lived = c.died_after_ms.map_or_else(|| "unknown".to_owned(), |ms| format!("{} s", ms / 1000));
    log::info!("autopsy {id}: {:?} of account {}, lived {lived}, seen as ended after {} s; {}", c.kind, c.account_id, c.uptime_ms / 1000, c.clues);
    let others = process::roblox_processes().len();
    let memory = metrics::memory().map_or_else(|| "unknown".to_owned(), |(used, all)| format!("{} of {} MB", used >> 20, all >> 20));
    log::info!("autopsy {id}: now {others} Roblox client(s) running, Roblox's installer running: {}, memory in use {memory}", process::installer_running());
    for dump in &dumps {
        log::info!("autopsy {id}: crash dump {dump}");
    }
    for fault in &faults {
        log::info!("autopsy {id}: Windows recorded {fault}");
    }
    match &c.log {
        Some(path) => log_tail(id, "client log", path, CLIENT_LINES),
        None => log::info!("autopsy {id}: no client log names this launch"),
    }
    if let (Some(dir), Some(name)) = (&logs, c.log.as_deref().and_then(Path::file_name).and_then(|n| n.to_str())) {
        match handler_log(dir, name, launched) {
            Some(path) => log_tail(id, "crash handler log", &path, HANDLER_LINES),
            None => log::info!("autopsy {id}: no crash handler log names this client's"),
        }
    }
}

/// What the evidence points to, first of all (pure).
fn verdict(c: &Ended, has_log: bool, dump: bool, fault: bool) -> String {
    let early = c.died_after_ms.is_some_and(|ms| ms < 30_000);
    match () {
        _ if matches!(c.kind, CrashKind::Hang) => "the client stopped responding and was given up on: look at the end of its log for what it was doing".to_owned(),
        _ if matches!(c.kind, CrashKind::Disconnect) => format!("Roblox disconnected it, reason {:?}: not a crash", c.reason),
        _ if dump || fault => "the client itself crashed: see the crash dump and Windows' record below".to_owned(),
        _ if !has_log => "it ended before writing a log, with no crash recorded: it didn't get to start properly (look at antivirus, a damaged Roblox install or the client's version)".to_owned(),
        _ if early => "it ended early by itself with no crash recorded: something closed it or it closed itself (look at the end of its log, an update or the anti-cheat)".to_owned(),
        _ => "it ended with no crash recorded: closed by hand, by another program or by itself (Roblox can't tell RoALT which)".to_owned(),
    }
}

fn log_tail(id: &str, what: &str, path: &Path, lines: usize) {
    let meta = std::fs::metadata(path).ok();
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    log::info!("autopsy {id}: {what} {name}, {} KB", meta.map_or(0, |m| m.len() >> 10));
    for line in clean_tail(path, lines, &[]) {
        log::info!("autopsy {id}:   {}", shortened(&line));
    }
}

fn shortened(line: &str) -> &str {
    &line[..line.char_indices().nth(MAX_LINE).map_or(line.len(), |(i, _)| i)]
}

/// The crash dumps Roblox's handler wrote since `since`: name and size.
fn dumps_since(logs: &Path, since: SystemTime) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(logs.join("crashes").join("reports")) else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            let new = m.modified().ok()? >= since;
            let dump = e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("dmp"));
            (new && dump).then(|| format!("{}, {} KB", e.file_name().to_string_lossy(), m.len() >> 10))
        })
        .collect()
}

/// The crash handler log naming the client log `player_log`, written since `since`.
fn handler_log(logs: &Path, player_log: &str, since: SystemTime) -> Option<std::path::PathBuf> {
    use std::io::Read;
    std::fs::read_dir(logs).ok()?.flatten().map(|e| e.path()).find(|p| {
        let handler = p.file_name().is_some_and(|n| n.to_string_lossy().contains("_CrashHandler_"));
        let new = std::fs::metadata(p).and_then(|m| m.modified()).is_ok_and(|t| t >= since);
        let mut head = Vec::new();
        handler && new && std::fs::File::open(p).and_then(|f| f.take(HANDLER_HEAD).read_to_end(&mut head)).is_ok() && String::from_utf8_lossy(&head).contains(player_log)
    })
}

/// Windows' records of this client faulting, from the Application event log (no rights needed).
fn windows_faults(c: &Ended) -> Vec<String> {
    faults_of(&fault_events((c.uptime_ms + 60_000).max(60_000)), c.pid)
}

/// The Application log's fault events of the last `ms` milliseconds, as XML; empty on failure.
fn fault_events(ms: i64) -> String {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let out = std::process::Command::new(Path::new(&root).join(r"System32\wevtutil.exe"))
        .args(["qe", "Application", "/f:xml", "/rd:true", "/c:50"])
        .arg(format!("/q:*[System[Provider[@Name='Application Error'] and TimeCreated[timediff(@SystemTime) <= {ms}]]]"))
        .creation_flags(0x0800_0000)
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        Ok(o) => {
            log::debug!("autopsy: wevtutil failed: {}", String::from_utf8_lossy(&o.stderr).trim());
            String::new()
        }
        Err(e) => {
            log::debug!("autopsy: wevtutil didn't run: {e}");
            String::new()
        }
    }
}

/// The events of `xml` about Roblox's client with this pid, as a line each (pure).
fn faults_of(xml: &str, pid: u32) -> Vec<String> {
    let field = |event: &str, name: &str| {
        let open = format!("<Data Name='{name}'>");
        let start = event.find(&open)? + open.len();
        Some(event[start..].split("</Data>").next()?.to_owned())
    };
    xml.split("<Event ")
        .filter(|ev| field(ev, "AppName").is_some_and(|n| n.eq_ignore_ascii_case(CLIENT_EXE)))
        .filter(|ev| field(ev, "ProcessId").and_then(|p| u32::from_str_radix(p.trim_start_matches("0x"), 16).ok()) == Some(pid))
        .map(|ev| {
            let get = |name| field(ev, name).unwrap_or_else(|| "?".to_owned());
            format!(
                "a fault: exception {} in {} {} at offset {}, client version {}",
                get("ExceptionCode"),
                get("ModuleName"),
                get("ModuleVersion"),
                get("FaultingOffset"),
                get("AppVersion"),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ended(kind: CrashKind, died_after_ms: Option<i64>) -> Ended {
        Ended {
            instance_id: "i".into(),
            account_id: 1,
            kind,
            at: 0,
            uptime_ms: 120_000,
            reason: None,
            place_id: None,
            log: None,
            pid: 1,
            started_at: 0,
            died_after_ms,
            clues: String::new(),
        }
    }

    #[test]
    fn the_verdict_goes_by_the_strongest_evidence() {
        let early = ended(CrashKind::Crash, Some(8_000));
        assert!(verdict(&early, true, true, false).contains("itself crashed"));
        assert!(verdict(&early, false, false, true).contains("itself crashed"));
        assert!(verdict(&early, false, false, false).contains("before writing a log"));
        assert!(verdict(&early, true, false, false).contains("ended early"));
        assert!(verdict(&ended(CrashKind::Crash, Some(900_000)), true, false, false).contains("no crash recorded"));
        assert!(verdict(&ended(CrashKind::Disconnect, None), true, true, true).contains("not a crash"));
        assert!(verdict(&ended(CrashKind::Hang, None), true, true, true).contains("stopped responding"));
    }

    #[test]
    fn only_this_clients_faults_count() {
        let event = |app: &str, pid: &str| {
            format!(
                "<Event xmlns='x'><System><Provider Name='Application Error'/></System><EventData><Data Name='AppName'>{app}</Data><Data Name='AppVersion'>0.745</Data><Data Name='ModuleName'>d3d11.dll</Data><Data Name='ModuleVersion'>10.0</Data><Data Name='ExceptionCode'>c0000005</Data><Data Name='FaultingOffset'>00ff</Data><Data Name='ProcessId'>{pid}</Data></EventData></Event>"
            )
        };
        let xml = [event("RobloxPlayerBeta.exe", "0x988"), event("RobloxPlayerBeta.exe", "0x10"), event("other.exe", "0x988")].concat();
        assert_eq!(faults_of(&xml, 0x988), ["a fault: exception c0000005 in d3d11.dll 10.0 at offset 00ff, client version 0.745"]);
        assert!(faults_of("", 1).is_empty());
    }

    /// Against this PC's event log, which needs a recent fault of any program:
    /// `cargo test -- --ignored asks_windows --nocapture`.
    #[test]
    #[ignore]
    fn asks_windows() {
        let xml = fault_events(90 * 24 * 60 * 60 * 1000);
        println!("{} bytes", xml.len());
        assert!(xml.contains("<Data Name='AppName'>") && xml.contains("<Data Name='ProcessId'>"));
    }

    #[test]
    fn long_lines_are_cut_on_a_character() {
        assert_eq!(shortened("short"), "short");
        assert_eq!(shortened(&"é".repeat(MAX_LINE + 5)).chars().count(), MAX_LINE);
    }
}
