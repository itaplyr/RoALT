//! Reading Roblox client logs (`%LOCALAPPDATA%\Roblox\logs\*_Player_*_last.log`) to tie a
//! client to its launch and follow it. Line formats: research multi-instance M10/M11 and
//! fixtures/multiinstance-log-line-samples.json. Logs can contain the launch ticket
//! (`gameinfo:`); lines are parsed and dropped, never stored or logged.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// What a client's log has said so far (saved with a tracked client, `tracker::Saved`).
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct LogFacts {
    /// From `web launch, websiteBTId is <id>` or `BTID is overriden to <id>`.
    pub browser_tracker_id: Option<i64>,
    /// From `! Joining game '<job>' place <place> at <ip>` (latest).
    pub job_id: Option<String>,
    pub place_id: Option<i64>,
    /// From `[FLog::GameJoinLoadTime] ... userid:<id>`.
    pub user_id: Option<i64>,
    /// `Replicator created` after the latest join.
    pub in_game: bool,
    /// In a game, Roblox started a teleport (`doTeleport`), until the next join. Its disconnect
    /// line isn't trouble meanwhile (`tracker::state_of`; a teleport that fails times out).
    pub teleporting: bool,
    /// Disconnect / lost connection / kick after the latest join.
    pub disconnected: bool,
    /// That disconnect was the player's own (`client_initiated`: Leave, or signed in elsewhere).
    pub left: bool,
    /// What the disconnect line said (`reason_of`: plain text only, no prefix).
    pub reason: Option<String>,
    /// `challengedByGcs`.
    pub challenged: bool,
    /// Closed because another client started (EventWaitResultUpdate then a close request).
    pub replaced: bool,
    /// `EventWaitResultUpdate` seen: another client set the singleton event. A close request
    /// after it is a replacement; before it, the user's own tray Exit (verify M11).
    signalled: bool,
}

/// Mtime slack for `find_log` (clocks and file-time granularity).
const SINCE_SLACK: Duration = Duration::from_secs(5);
/// The tracker id is logged within the first second; this is far more than that.
const HEAD_BYTES: u64 = 256 * 1024;

const JOINING: &str = "! Joining game '";
/// A log line holding one of these (lowercase) is never reported or shared.
pub const SENSITIVE: [&str; 7] = ["gameinfo", "ticket", "placelauncher", "roblosecurity", "cookie", "accesscode", "linkcode"];
/// As Bloxstrap reads it (`ActivityWatcher.cs`); not captured from a log here. If Roblox stops
/// logging it, a teleport looks like a disconnect again, nothing worse.
const TELEPORT: &str = "UgcExperienceController: doTeleport";
const DISCONNECTS: [&str; 6] = [
    "Disconnection Notification",
    "Lost connection with reason",
    "Time to disconnect replication data",
    "Sending disconnect with reason",
    "Disconnected from server for reason",
    "Connection closed by server",
];

/// The player's own disconnect, not to be rejoined: reason 285 (`DisconnectClientInitiated`:
/// Leave, research M-logs) or 273 (the same account launched elsewhere: rejoining would kick
/// that session). Only the message counts, not the timestamp/thread prefix before " [".
fn client_initiated(line: &str) -> bool {
    let msg = line.find(" [").map_or(line, |i| &line[i..]);
    ["DisconnectClientInitiated", "Same account launched", "DuplicateLogin"].iter().any(|m| msg.contains(m))
        || msg.split(|c: char| !c.is_ascii_digit()).any(|n| n == "285" || n == "273")
}

/// `%LOCALAPPDATA%\Roblox\logs` on Windows; RoALT's own client-log folder on Linux.
#[cfg(windows)]
pub fn logs_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Roblox").join("logs"))
}

/// Linux: the launcher writes each Cordial client's output here (`<btid>_Player_last.log`).
#[cfg(not(windows))]
pub fn logs_dir() -> Option<PathBuf> {
    let data = std::env::var_os("ROALT_DATA_DIR").map(PathBuf::from).unwrap_or_else(crate::default_dir);
    Some(data.join("logs").join("clients"))
}

/// The `_Player_` log (not `_CrashHandler_`) modified since `since` whose first lines name
/// this browser tracker id. Reads at most the first ~256 KB of each candidate.
pub fn find_log(dir: &Path, browser_tracker_id: i64, since: SystemTime) -> Option<PathBuf> {
    let since = since.checked_sub(SINCE_SLACK).unwrap_or(UNIX_EPOCH);
    let mut candidates: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .inspect_err(|e| {
            // Polled every second: "no logs yet" is normal, anything else is said once.
            static ONCE: std::sync::Once = std::sync::Once::new();
            if e.kind() != std::io::ErrorKind::NotFound {
                ONCE.call_once(|| log::warn!("Roblox logs folder unreadable: {e}"));
            }
        })
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            name.contains("_Player_") && !name.contains("_CrashHandler_")
        })
        .filter_map(|e| {
            let modified = e.metadata().ok()?.modified().ok()?;
            (modified >= since).then(|| (modified, e.path()))
        })
        .collect();
    candidates.sort_by_key(|c| std::cmp::Reverse(c.0));
    candidates.into_iter().map(|(_, p)| p).find(|p| names_tracker(p, browser_tracker_id))
}

/// `clean` deletes logs last written over a day ago ...
const CLEAN_AGE: Duration = Duration::from_secs(24 * 60 * 60);
/// ... or bigger than this.
const CLEAN_BYTES: u64 = 50 << 20;

/// Which of `files` (path, size, last written) `clean` deletes (pure): `.log` files written over
/// 24 h before `now` or over 50 MB, never one in `keep` (a live client's).
pub fn stale(files: Vec<(PathBuf, u64, SystemTime)>, now: SystemTime, keep: &[PathBuf]) -> Vec<(PathBuf, u64)> {
    files
        .into_iter()
        .filter(|(p, _, _)| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("log")) && !keep.contains(p))
        .filter(|&(_, len, modified)| len > CLEAN_BYTES || now.duration_since(modified).is_ok_and(|age| age > CLEAN_AGE))
        .map(|(p, len, _)| (p, len))
        .collect()
}

/// Deletes the logs `stale` picks in `dir` (not its subfolders). A file Windows won't let go
/// of (a client still writing it) is skipped. What it deleted.
pub fn clean(dir: &Path, keep: &[PathBuf]) -> crate::model::LogsCleaned {
    let files = std::fs::read_dir(dir)
        .inspect_err(|e| log::warn!("could not read the Roblox logs folder to clean it: {e}"))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let m = e.metadata().ok().filter(|m| m.is_file())?;
            Some((e.path(), m.len(), m.modified().ok()?))
        })
        .collect();
    let mut done = crate::model::LogsCleaned::default();
    let mut failed = 0;
    for (path, len) in stale(files, SystemTime::now(), keep) {
        if std::fs::remove_file(&path).is_ok() {
            done.files += 1;
            done.bytes += len;
        } else {
            failed += 1;
        }
    }
    if failed > 0 {
        log::debug!("log clean: {failed} files could not be deleted (in use or no permission)");
    }
    done
}

fn names_tracker(path: &Path, browser_tracker_id: i64) -> bool {
    let mut head = Vec::new();
    if open_shared(path).and_then(|f| f.take(HEAD_BYTES).read_to_end(&mut head)).is_err() {
        return false;
    }
    String::from_utf8_lossy(&head).lines().any(|l| tracker_id(l) == Some(browser_tracker_id))
}

/// Reads lines appended since `offset` (opened shared, so Roblox keeps writing), updates
/// `facts`, returns the new offset. A partial last line is left for the next read. The client's
/// own errors and warnings (`problem_of`) go to `on_problem`, repeats within one read counted.
pub fn read_new(path: &Path, offset: u64, facts: &mut LogFacts, mut on_problem: impl FnMut(&str, usize)) -> std::io::Result<u64> {
    let mut f = open_shared(path)?;
    // Shorter than what was read: a new file under the same name, start over.
    let offset = if f.metadata()?.len() < offset { 0 } else { offset };
    f.seek(SeekFrom::Start(offset))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    let Some(end) = buf.iter().rposition(|&b| b == b'\n') else { return Ok(offset) };
    let mut problems: Vec<(String, usize)> = Vec::new();
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for line in buf[..end].split(|&b| b == b'\n') {
        let line = String::from_utf8_lossy(line);
        if let Some(p) = problem_of(&line).or_else(|| update_line(&line)) {
            match seen.get(p) {
                Some(&i) => problems[i].1 += 1,
                // A game can print any number of different warnings.
                None if problems.len() < MAX_PROBLEMS => {
                    seen.insert(p.to_owned(), problems.len());
                    problems.push((p.to_owned(), 1));
                }
                None => {}
            }
        }
        parse_line(&line, facts);
    }
    problems.iter().for_each(|(p, n)| on_problem(p, *n));
    Ok(offset + end as u64 + 1)
}

/// Different problem lines reported per read.
const MAX_PROBLEMS: usize = 100;

/// A client error or warning line from its `[..Error]` / `[..Warning]` tag on, at most 300
/// characters; None for anything else and for any line that could carry a ticket, cookie or
/// link (pure).
pub fn problem_of(line: &str) -> Option<&str> {
    let start = line.find('[')?;
    let tag = &line[start + 1..start + line[start..].find(']')?];
    if !(tag.ends_with("Error") || tag.ends_with("Warning")) {
        return None;
    }
    let lower = line.to_ascii_lowercase();
    if SENSITIVE.iter().any(|w| lower.contains(w)) || lower.contains("http") {
        return None;
    }
    let rest = line[start..].trim_end();
    Some(&rest[..rest.char_indices().nth(300).map_or(rest.len(), |(i, _)| i)])
}

/// The client's channel and update decision from its tag on, worth a line in RoALT's log: a
/// foreground update closes every client (research multi-instance M9). Never a URL (pure).
fn update_line(line: &str) -> Option<&str> {
    const MARKS: [&str; 3] = ["RobloxChannel has been set to ", "Update check thread: updateRequired ", "Update mode is chosen as "];
    if !MARKS.iter().any(|m| line.contains(m)) {
        return None;
    }
    Some(line[line.find('[')?..].trim_end())
}

/// One log line into `facts` (pure).
pub fn parse_line(line: &str, facts: &mut LogFacts) {
    // A game's scripts write print, warn and error lines: nothing in them is an event. The real
    // join line is an Output line too, so it only counts while no game is running.
    let tag = first_tag(line);
    let joining = line.find(JOINING).filter(|_| matches!(tag, None | Some("FLog::Output")));
    if matches!(tag, Some("FLog::Output" | "FLog::Warning" | "FLog::Error")) && joining.is_none() {
        return;
    }
    if joining.is_some() && facts.in_game && !facts.teleporting && !facts.disconnected {
        return;
    }
    if let Some(id) = tracker_id(line) {
        facts.browser_tracker_id = Some(id);
    } else if let Some(rest) = line.split_once("[cordial] game: joining server ").map(|(_, r)| r) {
        if let Some((job, after)) = rest.split_once(" of place ")
            && is_guid(job)
        {
            facts.job_id = Some(job.to_owned());
            facts.place_id = number_after(after, "");
            facts.in_game = false;
            facts.teleporting = false;
            facts.disconnected = false;
            facts.left = false;
            facts.reason = None;
        }
    } else if let Some(rest) = line.split_once("[cordial] game: joined place ").map(|(_, r)| r) {
        if let Some(place) = number_after(rest, "") {
            facts.place_id = Some(place);
            facts.in_game = true;
            facts.teleporting = false;
            facts.disconnected = false;
            facts.left = false;
            facts.reason = None;
        }
        if let Some(user) = line.split_once(" as ").and_then(|(_, tail)| number_after(tail, "")) {
            facts.user_id = Some(user);
        }
    } else if line.contains("[cordial] game: left") {
        facts.disconnected = true;
        facts.left = true;
        facts.in_game = false;
        facts.reason = Some("left".into());

    } else if let Some(i) = joining {
        if let Some((job, rest)) = line[i + JOINING.len()..].split_once('\'')
            && is_guid(job)
        {
            facts.job_id = Some(job.to_owned());
            facts.place_id = number_after(rest, " place ");
            facts.in_game = false;
            facts.teleporting = false;
            facts.disconnected = false;
            facts.left = false;
            facts.reason = None;
        }
    } else if line.contains("[FLog::Network] Replicator created") || line.contains("[FLog::Network] serverId:") {
        facts.in_game = true;
    } else if facts.in_game && line.contains(TELEPORT) {
        facts.teleporting = true;
    } else if let Some(m) = DISCONNECTS.iter().find(|m| line.contains(*m)) {
        facts.disconnected = true;
        facts.left |= client_initiated(line);
        // The first line that names a reason; "Time to disconnect ..." names none.
        if !facts.reason.as_deref().is_some_and(names_reason) {
            facts.reason = reason_of(line, m).or(facts.reason.take());
        }
    } else if line.contains("challengedByGcs") {
        facts.challenged = true;
    } else if line.contains("[FLog::GameJoinLoadTime] Report game_join_loadtime:") {
        if let Some(id) = number_after(line, "userid:") {
            facts.user_id = Some(id);
        }
    } else if line.contains("EventWaitResultUpdate") {
        facts.signalled = true;
    } else if line.contains("Received close main window request") && facts.signalled {
        facts.replaced = true;
    }
}

/// The tag of a log line, the first `[..]`: the client's own, before any text a script wrote.
fn first_tag(line: &str) -> Option<&str> {
    let start = line.find('[')?;
    Some(&line[start + 1..start + line[start..].find(']')?])
}

/// A disconnect line from its marker on, for crash diagnostics: letters, digits, spaces and
/// `.,:;()_-` only (anything else ends it), at most 160 characters. Never a ticket: those are
/// on `gameinfo:` lines, which are no disconnect lines.
fn reason_of(line: &str, marker: &str) -> Option<String> {
    let rest = &line[line.find(marker)?..];
    let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || " .,:;()_-".contains(c))).unwrap_or(rest.len());
    let text: String = rest[..end].trim_end().chars().take(160).collect();
    (!text.is_empty()).then_some(text)
}

fn names_reason(text: &str) -> bool {
    text.to_ascii_lowercase().contains("reason")
}

fn tracker_id(line: &str) -> Option<i64> {
    number_after(line, "websiteBTId is ").or_else(|| number_after(line, "BTID is overriden to "))
}

/// The digits right after `marker`.
fn number_after(line: &str, marker: &str) -> Option<i64> {
    let rest = &line[line.find(marker)? + marker.len()..];
    let end = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn is_guid(s: &str) -> bool {
    s.len() == 36 && s.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

fn open_shared(path: &Path) -> std::io::Result<File> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE: Roblox keeps writing (and may
        // delete) the file while it is open here.
        return std::fs::OpenOptions::new().read(true).share_mode(0x1 | 0x2 | 0x4).open(path);
    }
    #[cfg(not(windows))]
    std::fs::OpenOptions::new().read(true).open(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::Write;

    const BTID: i64 = 7_351_204_118;
    const JOB: &str = "0d6c3f4e-5b2a-4c1d-9e8f-7a6b5c4d3e2f";
    const PLACE: i64 = 920_587_237;
    const USER: i64 = 261;

    /// The fixture's marker lines with their placeholders filled in.
    fn samples() -> HashMap<String, String> {
        let path = format!("{}/../docs/research/fixtures/multiinstance-log-line-samples.json", env!("CARGO_MANIFEST_DIR"));
        let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        json["markers"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| {
                let line = v["line"]
                    .as_str()
                    .unwrap()
                    .replace("<BTID>", &BTID.to_string())
                    .replace("<JOBID>", JOB)
                    .replace("<PLACEID>", &PLACE.to_string())
                    .replace("<USERID>", &USER.to_string())
                    .replace("<UNIVERSEID>", "1069")
                    .replace("<SID>", "a1b2c3d4-0000-4000-8000-000000000000")
                    .replace("<T>", "1760864884.514");
                (k.clone(), line)
            })
            .collect()
    }

    fn parsed(lines: &[&str]) -> LogFacts {
        let mut f = LogFacts::default();
        for l in lines {
            parse_line(l, &mut f);
        }
        f
    }

    fn line(msg: &str) -> String {
        format!("2026-10-19T09:08:03.437Z,0.437181,0a50,6,Info {msg}\n")
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("roalt-logwatch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn cordial_game_lines_drive_the_same_facts() {
        let mut f = LogFacts::default();
        parse_line(&format!("[cordial] game: joining server {JOB} of place {PLACE}"), &mut f);
        assert_eq!((f.job_id.as_deref(), f.place_id, f.in_game), (Some(JOB), Some(PLACE), false));
        parse_line(&format!("[cordial] game: joined place {PLACE} (universe 1069) as {USER}"), &mut f);
        assert_eq!((f.in_game, f.place_id, f.user_id), (true, Some(PLACE), Some(USER)));
        parse_line("[cordial] game: left", &mut f);
        assert!(f.disconnected && f.left && !f.in_game);
    }

    #[test]
    fn a_games_own_lines_are_no_events() {
        let guid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
        let mut f = parsed(&[&line(&format!("[FLog::Output] ! Joining game '{guid}' place 1 at 1.1.1.1")), &line("[FLog::Network] Replicator created: 1")]);
        assert!(f.in_game);
        for tag in ["FLog::Output", "FLog::Warning", "FLog::Error"] {
            for text in [
                "[FLog::Network] Lost connection with reason : x".to_owned(),
                "Sending disconnect with reason: 285".to_owned(),
                "reason: challengedByGcs".to_owned(),
                "UgcExperienceController: doTeleport".to_owned(),
                "[FLog::GameJoinLoadTime] Report game_join_loadtime: userid:5".to_owned(),
                "! Joining game 'ffffffff-bbbb-cccc-dddd-eeeeeeeeeeee' place 666 at 1.1.1.1".to_owned(),
            ] {
                parse_line(&line(&format!("[{tag}] {text}")), &mut f);
            }
        }
        assert!(f.in_game && !f.disconnected && !f.left && !f.challenged && !f.teleporting);
        assert_eq!((f.job_id.as_deref(), f.place_id, f.user_id), (Some(guid), Some(1), None));
    }

    #[test]
    fn client_problems_are_kept_without_secrets() {
        let l = "2024-05-01T12:00:00.123Z,1.2,1a2c,6 [FLog::Error] Texture failed to load";
        assert_eq!(problem_of(l), Some("[FLog::Error] Texture failed to load"));
        assert_eq!(problem_of("x,6 [DFLog::RenderError] boom"), Some("[DFLog::RenderError] boom"));
        assert_eq!(problem_of("x,6 [FLog::Warning] gameinfo:abc"), None);
        assert_eq!(problem_of("x,6 [FLog::Error] bad https://roblox.com/x?ticket=1"), None);
        assert_eq!(problem_of("x,6 [FLog::Output] fine"), None);
        assert_eq!(problem_of("no tag"), None);
        assert_eq!(update_line("x,6,Info [FLog::UpdateController] Update check thread: updateRequired TRUE"), Some("[FLog::UpdateController] Update check thread: updateRequired TRUE"));
        assert_eq!(update_line("x,6 [FLog::Output] RobloxChannel has been set to zaflag"), Some("[FLog::Output] RobloxChannel has been set to zaflag"));
        assert_eq!(update_line("x,6,Info [FLog::UpdateController] versionQueryUrl: https://x"), None);
    }

    #[test]
    fn stale_logs_are_old_or_huge_and_never_live() {
        let now = SystemTime::now();
        let hours = |h: u64| now - Duration::from_secs(h * 3600);
        let f = |name: &str, len: u64, at: SystemTime| (PathBuf::from(name), len, at);
        let live = PathBuf::from(r"L:\live_Player_last.log");
        let files = vec![
            f(r"L:\old_Player_last.log", 10, hours(25)),
            f(r"L:\new_Player_last.log", 10, hours(23)),
            f(r"L:\huge.LOG", CLEAN_BYTES + 1, hours(0)),
            f(r"L:\edge.log", CLEAN_BYTES, hours(0)),
            f(r"L:\old.txt", CLEAN_BYTES + 1, hours(48)),
            f(r"L:\future.log", 10, now + Duration::from_secs(3600)),
            (live.clone(), CLEAN_BYTES + 1, hours(48)),
        ];
        let picked: Vec<_> = stale(files, now, &[live]).into_iter().map(|(p, _)| p.display().to_string()).collect();
        assert_eq!(picked, [r"L:\old_Player_last.log", r"L:\huge.LOG"]);
    }

    #[test]
    fn clean_deletes_only_stale_logs() {
        let dir = temp_dir();
        std::fs::write(dir.join("big.log"), vec![b'x'; CLEAN_BYTES as usize + 1]).unwrap();
        std::fs::write(dir.join("small.log"), b"x").unwrap();
        std::fs::write(dir.join("live.log"), vec![b'x'; CLEAN_BYTES as usize + 1]).unwrap();
        let done = clean(&dir, &[dir.join("live.log")]);
        assert_eq!(done, crate::model::LogsCleaned { files: 1, bytes: CLEAN_BYTES + 1 });
        assert!(!dir.join("big.log").exists() && dir.join("small.log").exists() && dir.join("live.log").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_teleport_isnt_a_disconnect_until_it_fails() {
        let mut f = LogFacts::default();
        parse_line("x [FLog::UgcExperienceController] UgcExperienceController: doTeleport: joinScriptUrl = ...", &mut f);
        assert!(!f.teleporting, "only from inside a game");
        f.in_game = true;
        parse_line("x [FLog::UgcExperienceController] UgcExperienceController: doTeleport: joinScriptUrl = ...", &mut f);
        assert!(f.teleporting);
        parse_line("x [FLog::Network] Time to disconnect replication data: 0.1", &mut f);
        assert!(f.teleporting && f.disconnected && !f.left);
        parse_line("x [FLog::Output] ! Joining game '0d6c3f4e-5b2a-4c1d-9e8f-7a6b5c4d3e2f' place 1818 at 1.2.3.4", &mut f);
        assert!(!f.teleporting && !f.disconnected);
    }

    #[test]
    fn every_sample_line() {
        let samples = samples();
        assert!(samples.len() >= 20);
        for (key, l) in &samples {
            let mut want = LogFacts::default();
            match key.as_str() {
                "web_launch_btid" => want.browser_tracker_id = Some(BTID),
                "joining_game" => {
                    want.job_id = Some(JOB.into());
                    want.place_id = Some(PLACE);
                }
                "join_loadtime" => want.user_id = Some(USER),
                "replicator_created" => want.in_game = true,
                "time_to_disconnect" | "lost_connection" | "server_closed_reason" | "disconnection_notification" => {
                    want.disconnected = true
                }
                // Player: 285 (DisconnectClientInitiated): the player left.
                "client_disconnect_reason" => (want.disconnected, want.left) = (true, true),
                "singleton_event_signalled" => want.signalled = true,
                // Alone, a close request is the user's tray Exit.
                "close_main_window_request" | "update_required" | "tray_treatment" | "singleton_mutex_wait"
                | "singleton_mutex_loop" | "update_mode" | "udmux" | "client_disconnect" | "shutdown" | "leave_ugc"
                | "tray_user_exit" | "tray_spawned" => {}
                other => panic!("new fixture sample {other}: say here what it means"),
            }
            let mut got = parsed(&[l]);
            // Every disconnect keeps its line's text, from the marker on (reason_of).
            assert_eq!(got.reason.is_some(), want.disconnected, "{key}");
            got.reason = None;
            assert_eq!(got, want, "{key}");
        }
    }

    #[test]
    fn other_documented_lines() {
        let f = parsed(&[&line(&format!("[FLog::AppRemoteCheckStarterModule] BTID is overriden to {BTID}."))]);
        assert_eq!(f.browser_tracker_id, Some(BTID));
        assert!(parsed(&[&line("[FLog::Network] Sending disconnect with reason: 267")]).disconnected);
        assert!(parsed(&[&line("[FLog::Network] serverId: 10.9.2.90|51492")]).in_game);
        assert!(parsed(&[&line("[FLog::Something] reason: challengedByGcs")]).challenged);
        // Not a GUID: ignored.
        assert_eq!(parsed(&[&line("[FLog::Output] ! Joining game 'nope' place 1 at 1.2.3.4")]), LogFacts::default());
    }

    #[test]
    fn a_new_join_resets_the_session() {
        let s = samples();
        let mut f = parsed(&[&s["joining_game"], &s["replicator_created"], &s["lost_connection"]]);
        assert!(f.in_game && f.disconnected);
        parse_line(&s["joining_game"], &mut f);
        assert!(!f.in_game && !f.disconnected);
        assert_eq!(f.job_id.as_deref(), Some(JOB));
    }

    #[test]
    fn replacement_vs_tray_exit() {
        let s = samples();
        let (event, close) = (s["singleton_event_signalled"].as_str(), s["close_main_window_request"].as_str());
        assert!(parsed(&[event, close]).replaced);
        let tray_exit = line("[FLog::Systray] Systray menu item Exit is clicked");
        assert!(!parsed(&[&tray_exit, close, event]).replaced);
        assert!(!parsed(&[close]).replaced);
        assert!(!parsed(&[event]).replaced);
    }

    #[test]
    fn finds_the_log_by_tracker_id() {
        let dir = temp_dir();
        let now = SystemTime::now();
        let write = |name: &str, btid: i64, age: Duration| {
            let p = dir.join(name);
            let body = format!(
                "{}{}",
                line("[FLog::UpdateController] Update mode is chosen as BACKGROUND. Telemetry sent"),
                line(&format!("[FLog::LogWin32BTId] LogWin32BTId, App, web launch, websiteBTId is {btid}")),
            );
            std::fs::write(&p, body).unwrap();
            File::options().write(true).open(&p).unwrap().set_modified(now - age).unwrap();
            p
        };
        let since = now - Duration::from_secs(60);
        write("0.743.0.7431134_20261023T220008Z_Player_0A5C5_CrashHandler_last.log", BTID, Duration::ZERO);
        write("0.743.0.7431134_20261023T210008Z_Player_11111_last.log", BTID, Duration::from_secs(3600));
        write("0.743.0.7431134_20261023T220009Z_Player_22222_last.log", BTID + 1, Duration::ZERO);
        write("RobloxPlayerInstaller_EA24F.log", BTID, Duration::ZERO);
        assert_eq!(find_log(&dir, BTID, since), None);

        let older = write("0.743.0.7431134_20261023T220010Z_Player_33333_last.log", BTID, Duration::from_secs(30));
        assert_eq!(find_log(&dir, BTID, since), Some(older));
        let newer = write("0.743.0.7431134_20261023T220011Z_Player_44444_last.log", BTID, Duration::from_secs(2));
        assert_eq!(find_log(&dir, BTID, since), Some(newer));
        // Modified a little before `since`: within the slack.
        assert!(find_log(&dir, BTID + 1, now + Duration::from_secs(3)).is_some());
        assert_eq!(find_log(&dir.join("missing"), BTID, since), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn reads_appended_lines_only_when_complete() {
        let dir = temp_dir();
        let p = dir.join("0.743.0.7431134_20261023T220008Z_Player_8C675_last.log");
        let len = |p: &Path| std::fs::metadata(p).unwrap().len();
        std::fs::write(&p, line(&format!("[FLog::LogWin32BTId] LogWin32BTId, App, web launch, websiteBTId is {BTID}"))).unwrap();
        let mut f = LogFacts::default();
        let o1 = read_new(&p, 0, &mut f, |_, _| {}).unwrap();
        assert_eq!((o1, f.browser_tracker_id), (len(&p), Some(BTID)));

        let join = line(&format!("[FLog::Output] ! Joining game '{JOB}' place {PLACE} at 10.9.2.90"));
        let (head, tail) = join.split_at(70);
        let mut w = File::options().append(true).open(&p).unwrap();
        // A whole line with an invalid UTF-8 byte, then half of the next one.
        w.write_all(b"2026-10-19T09:08:04.000Z,1.0,0a50,6 \xff noise\n").unwrap();
        w.write_all(head.as_bytes()).unwrap();
        let o2 = read_new(&p, o1, &mut f, |_, _| {}).unwrap();
        assert_eq!(o2, len(&p) - head.len() as u64);
        assert_eq!(f.job_id, None);
        assert_eq!(read_new(&p, o2, &mut f, |_, _| {}).unwrap(), o2);

        w.write_all(tail.as_bytes()).unwrap();
        w.write_all(line("[FLog::Network] Replicator created: 000000007A26CF00").as_bytes()).unwrap();
        let o3 = read_new(&p, o2, &mut f, |_, _| {}).unwrap();
        assert_eq!(o3, len(&p));
        assert_eq!((f.job_id.as_deref(), f.place_id, f.in_game), (Some(JOB), Some(PLACE), true));
        drop(w);

        // Replaced by a shorter file: read from the start.
        std::fs::write(&p, line("[FLog::Network] Lost connection with reason : You have been kicked from the game")).unwrap();
        let o4 = read_new(&p, o3, &mut f, |_, _| {}).unwrap();
        assert_eq!(o4, len(&p));
        assert!(f.disconnected);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_disconnect_the_player_started_is_told_apart() {
        let kicked = parsed(&[&line("[FLog::Network] Sending disconnect with reason: 267")]);
        assert!(kicked.disconnected && !kicked.left);
        let shutdown = parsed(&[&line("[FLog::Network] Connection closed by server for reason: RCC: 288 (DisconnectServerShutdown)")]);
        assert!(shutdown.disconnected && !shutdown.left);
        for l in ["[FLog::Network] Disconnection Notification. Reason: 285", "Disconnected from server for reason: Player: 285 (DisconnectClientInitiated)"] {
            let left = parsed(&[&line(l)]);
            assert!(left.disconnected && left.left, "{l}");
        }
        assert!(!parsed(&[&line("[FLog::Network] Disconnection Notification. Reason: 2851")]).left);
        let elsewhere = parsed(&[&line("[FLog::Network] Disconnection Notification. Reason: 273")]);
        assert!(elsewhere.disconnected && elsewhere.left);
        // A 285 in the timestamp, seconds or thread id is not the reason.
        let prefixed = parsed(&["2026-10-17T18:16:31.285Z,285.100000,a285,6,Info [FLog::Network] Sending disconnect with reason: 267"]);
        assert!(prefixed.disconnected && !prefixed.left);
    }

    #[test]
    fn the_disconnect_reason_is_kept_as_plain_text() {
        let reason = |lines: &[&str]| {
            let mut f = LogFacts::default();
            lines.iter().for_each(|l| parse_line(&line(l), &mut f));
            f.reason
        };
        assert_eq!(reason(&["[FLog::Network] Sending disconnect with reason: 267"]).as_deref(), Some("Sending disconnect with reason: 267"));
        // A line naming no reason gives way to one that does, not the other way round.
        let both = ["[FLog::Network] Time to disconnect replication data: 0.01", "[FLog::Network] Disconnection Notification. Reason: 277"];
        assert_eq!(reason(&both).as_deref(), Some("Disconnection Notification. Reason: 277"));
        let shutdown = "[FLog::Network] Connection closed by server for reason: RCC: 288 (DisconnectServerShutdown)";
        assert_eq!(reason(&[shutdown, both[0]]).as_deref(), Some("Connection closed by server for reason: RCC: 288 (DisconnectServerShutdown)"));
        // Anything unusual ends it, and it is capped.
        let odd = format!("[FLog::Network] Lost connection with reason : kicked \"x\" {}", "y".repeat(400));
        assert_eq!(reason(&[&odd]).as_deref(), Some("Lost connection with reason : kicked"));
        let long = format!("[FLog::Network] Lost connection with reason : {}", "y".repeat(400));
        assert_eq!(reason(&[&long]).unwrap().len(), 160);
        // A new join forgets it.
        let mut f = parsed(&[&line("[FLog::Network] Sending disconnect with reason: 267")]);
        parse_line(&line(&format!("[FLog::Output] ! Joining game '{JOB}' place {PLACE} at 10.9.2.90")), &mut f);
        assert_eq!(f.reason, None);
    }
}

