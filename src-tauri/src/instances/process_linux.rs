//! Roblox-family processes on Linux: Cordial, Sober and a Wine Roblox client.
//!
//! Linux has no Windows job objects or window APIs here. A process is identified by its pid and
//! kernel start time (`/proc/<pid>/stat` field 22), so a reused pid is never mistaken for a
//! client RoALT started. Closing sends SIGTERM, waits, then SIGKILL.

use std::collections::{HashMap, HashSet};
use std::thread::sleep;
use std::time::{Duration, Instant};

use crate::error::{AppError, Result};

/// Cordial's launcher/runtime, the nested compositor it can exec into, Sober, and a Roblox
/// client under Wine (Grapejuice).
const CLIENT_NAMES: [&str; 5] = ["cordial", "cordial-run", "cage", "sober", "RobloxPlayerBeta.exe"];
/// The profiles RoALT gives its Cordial clients (`launcher::client_linux::profile_name`).
const PROFILE_PREFIX: &str = "roalt_";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: u32,
    /// Process start, seconds since the Unix epoch.
    pub start_time: u64,
}

pub fn roblox_processes() -> Vec<ProcInfo> {
    let Ok(entries) = std::fs::read_dir("/proc") else { return Vec::new() };
    entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| is_client(*pid))
        .filter_map(|pid| start_time(pid).map(|start_time| ProcInfo { pid, start_time }))
        .collect()
}

pub fn installer_running() -> bool {
    false
}

fn is_client(pid: u32) -> bool {
    let Ok(exe) = std::fs::read_link(format!("/proc/{pid}/exe")) else { return false };
    exe.file_name().and_then(|n| n.to_str()).is_some_and(|n| CLIENT_NAMES.iter().any(|c| n.eq_ignore_ascii_case(c)))
}

fn is_cordial_or_cage(pid: u32) -> bool {
    let Ok(exe) = std::fs::read_link(format!("/proc/{pid}/exe")) else { return false };
    exe.file_name().and_then(|n| n.to_str()).is_some_and(|n| {
        ["cordial", "cordial-run", "cage"].iter().any(|c| n.eq_ignore_ascii_case(c))
    })
}

/// The RoALT account user id in a Cordial command line, if it names one.
///
/// Cordial is started with `--profile roalt_<user id>`, including when `--headless` turns the
/// process into `cage -- cordial-run …`; the option survives that rewrite.
pub fn profile_from_cmdline(cmdline: &[u8]) -> Option<i64> {
    let args: Vec<&[u8]> = cmdline.split(|b| *b == 0).filter(|a| !a.is_empty()).collect();
    let user = |value: &[u8]| {
        std::str::from_utf8(value).ok()?.strip_prefix(PROFILE_PREFIX)?.parse().ok()
    };
    for (i, arg) in args.iter().enumerate() {
        if *arg == b"--profile" {
            if let Some(user) = args.get(i + 1).and_then(|v| user(v)) {
                return Some(user);
            }
        } else if let Some(value) = arg.strip_prefix(b"--profile=")
            && let Some(user) = user(value)
        {
            return Some(user);
        }
    }
    None
}

/// The RoALT account profile a running process was started with.
pub fn profile_of(pid: u32) -> Option<i64> {
    profile_from_cmdline(&std::fs::read(format!("/proc/{pid}/cmdline")).ok()?)
}

/// Accounts with a live Cordial or cage process, from every process on the machine.
///
/// `None` when `/proc` cannot be read, so callers do not mistake a failed scan for "nothing runs".
pub fn profiles_by_user() -> Option<HashSet<i64>> {
    let entries = std::fs::read_dir("/proc").ok()?;
    let mut out = HashSet::new();
    for entry in entries.filter_map(|e| e.ok()) {
        let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        if is_cordial_or_cage(pid)
            && let Some(user) = profile_of(pid)
        {
            out.insert(user);
        }
    }
    Some(out)
}

/// The process and all of its descendants (children, grandchildren, …).
pub fn descendants(pid: u32) -> Vec<u32> {
    let Ok(entries) = std::fs::read_dir("/proc") else { return Vec::new() };
    let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
    for entry in entries.filter_map(|e| e.ok()) {
        let Some(child) = entry.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{child}/stat")) else { continue };
        let Some(after) = stat.rsplit_once(')').map(|(_, a)| a) else { continue };
        let Some(parent) = after.split_whitespace().nth(1).and_then(|s| s.parse::<u32>().ok()) else { continue };
        children.entry(parent).or_default().push(child);
    }
    let mut out = Vec::new();
    let mut stack = vec![pid];
    while let Some(parent) = stack.pop() {
        for &child in children.get(&parent).into_iter().flatten() {
            out.push(child);
            stack.push(child);
        }
    }
    out
}

/// `start_time` said None, but this client hasn't exited: the process is there and unreadable.
pub fn unreadable_client(pid: u32) -> bool {
    start_time(pid).is_none() && std::fs::metadata(format!("/proc/{pid}")).is_ok()
}

/// Start time of a running process, `None` if it is gone.
pub fn start_time(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after = stat.rsplit_once(')')?.1;
    let start_ticks: u64 = after.split_whitespace().nth(19)?.parse().ok()?;
    // SAFETY: sysconf has no preconditions; a failed call is treated as the usual 100 Hz.
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    let hz = if hz > 0 { hz as u64 } else { 100 };
    Some(boot_time()? + start_ticks / hz)
}

fn boot_time() -> Option<u64> {
    std::fs::read_to_string("/proc/stat")
        .ok()?
        .lines()
        .find_map(|l| l.strip_prefix("btime "))
        .and_then(|s| s.trim().parse().ok())
}

/// Asks the process to close (SIGTERM), then SIGKILL after `grace`. `force`: SIGKILL now.
/// `expected` is the start time the process was seen with; a reused pid is left alone.
pub fn close(pid: u32, expected: Option<u64>, force: bool, grace: Duration) -> Result<()> {
    let Some(t0) = start_time(pid).filter(|t| expected.is_none_or(|e| e == *t)) else { return Ok(()) };
    let alive = || start_time(pid) == Some(t0);
    if !force {
        signal(pid, libc::SIGTERM);
        let deadline = Instant::now() + grace;
        while alive() && Instant::now() < deadline {
            sleep(Duration::from_millis(100));
        }
        if !alive() {
            return Ok(());
        }
    }
    signal(pid, libc::SIGKILL);
    let deadline = Instant::now() + Duration::from_secs(2);
    while alive() && Instant::now() < deadline {
        sleep(Duration::from_millis(50));
    }
    if alive() {
        return Err(AppError::Internal(tr!("Could not close Roblox (process {pid}).", pid = pid)));
    }
    Ok(())
}

fn signal(pid: u32, sig: i32) {
    // SAFETY: kill on another process; ESRCH simply means it is already gone.
    let _ = unsafe { libc::kill(pid as libc::pid_t, sig) };
}

/// Linux doesn't expose the client's window here yet.
pub fn hung(_pid: u32) -> bool {
    false
}

/// Linux window tracking is not wired up (Wayland has no portable global window list).
pub fn main_window(_pid: u32) -> Option<(u32, bool)> {
    None
}

pub fn foreground_pid() -> Option<u32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_this_process_and_its_start_time() {
        let pid = std::process::id();
        assert!(start_time(pid).is_some());
        assert!(!is_client(pid));
    }

    #[test]
    fn closing_a_dead_process_is_ok() {
        assert!(close(u32::MAX - 1, None, true, Duration::ZERO).is_ok());
    }

    #[test]
    fn a_client_name_is_matched_case_insensitively() {
        assert!(CLIENT_NAMES.iter().any(|n| n.eq_ignore_ascii_case("robloxplayerbeta.exe")));
        assert!(CLIENT_NAMES.iter().any(|n| n.eq_ignore_ascii_case("cage")));
        assert_eq!(std::path::Path::new("/app/bin/cordial").file_name().unwrap(), "cordial");
    }

    #[test]
    fn reads_the_roalt_profile_from_a_cordial_or_cage_command_line() {
        let direct = b"/usr/bin/cordial-run\0--profile\0roalt_1234\0--join-url\0roblox://x\0";
        assert_eq!(profile_from_cmdline(direct), Some(1234));
        let nested = b"cage\0--\0/usr/bin/cordial-run\0--profile\0roalt_987\0";
        assert_eq!(profile_from_cmdline(nested), Some(987));
        let equals = b"/usr/bin/cordial-run\0--profile=roalt_55\0";
        assert_eq!(profile_from_cmdline(equals), Some(55));
        assert_eq!(profile_from_cmdline(b"/usr/bin/cordial\0--profile\0roalt_1\0"), Some(1));
        assert_eq!(profile_from_cmdline(b"/usr/bin/cordial-run\0--profile\0other\0"), None);
        assert_eq!(profile_from_cmdline(b""), None);
    }

    #[test]
    fn finds_a_child_process_when_looking_for_the_window_owner() {
        let mut child = std::process::Command::new("sleep").arg("5").spawn().unwrap();
        let kids = descendants(std::process::id());
        assert!(kids.contains(&child.id()), "{kids:?} does not contain {}", child.id());
        let _ = child.kill();
        let _ = child.wait();
    }
}
