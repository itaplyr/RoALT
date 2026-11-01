//! A report to send when something goes wrong: versions, the performance settings, and the end of
//! RoALT's log and of the Roblox client logs that matter, cleaned line by line (`redact`). Built
//! only when asked for, and it only goes where the user copies or saves it.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::instances::logwatch::SENSITIVE;

/// For the clipboard (a chat message takes little more) and for a file.
pub const FOR_CLIPBOARD: usize = 60 * 1024;
pub const FOR_FILE: usize = 500 * 1024;
const ROALT_LINES: usize = 500;
const ROBLOX_LINES: usize = 300;
/// How much of the end of a log is read.
const TAIL_BYTES: u64 = 4 << 20;
/// More words that mark a line not to share, on top of what the log watcher never reports.
const ALSO_SENSITIVE: [&str; 5] = ["authorization", "bearer", "joinscript", "webhook", "password"];

/// `line` with what shouldn't leave this PC masked; None when all of it must stay (pure).
pub fn redact(line: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    if SENSITIVE.iter().chain(&ALSO_SENSITIVE).any(|w| lower.contains(w)) {
        return None;
    }
    let mut out = mask_user_folder(line);
    out = mask_after(&out, "userid:");
    out = mask_after(&out, "sid:");
    Some(mask_uuids(&mask_ipv4(&mask_queries(&out))))
}

/// Ids shaped 8-4-4-4-12 to `…`: a job id with its place id is enough to find that exact server.
fn mask_uuids(line: &str) -> String {
    let is_uuid = |s: &[u8]| s.len() == 36 && s.iter().enumerate().all(|(i, c)| if matches!(i, 8 | 13 | 18 | 23) { *c == b'-' } else { c.is_ascii_hexdigit() });
    let (mut out, mut rest) = (String::new(), line);
    while rest.len() >= 36 {
        match (0..=rest.len() - 36).find(|&i| is_uuid(&rest.as_bytes()[i..i + 36])) {
            Some(i) => {
                out.push_str(&rest[..i]);
                out.push('…');
                rest = &rest[i + 36..];
            }
            None => break,
        }
    }
    out + rest
}

/// Where `key` (lowercase ASCII) first is in `text`, ignoring ASCII case.
fn find_ascii_ci(text: &str, key: &str) -> Option<usize> {
    text.as_bytes().windows(key.len()).position(|w| w.eq_ignore_ascii_case(key.as_bytes()))
}

/// `C:\Users\name\x` to `C:\Users\…\x`, for every such path.
fn mask_user_folder(line: &str) -> String {
    let (mut out, mut rest) = (String::new(), line);
    // Only as far as the next match: the whole line for every match is quadratic on a long one.
    while let Some(i) = find_ascii_ci(rest, r"\users\") {
        let (head, tail) = rest.split_at(i + r"\users\".len());
        out.push_str(head);
        // `{:?}` doubles the backslashes.
        let sep = tail.len() - tail.trim_start_matches(['\\', '/']).len();
        out.push_str(&tail[..sep]);
        out.push('…');
        let tail = &tail[sep..];
        // A user name can have spaces, so a space doesn't end it: masking too much is the safe way.
        rest = &tail[tail.find(['\\', '/', '"', ',']).unwrap_or(tail.len())..];
    }
    out + rest
}

/// What follows `key` (case-insensitive), up to a space or comma, masked.
fn mask_after(line: &str, key: &str) -> String {
    let (mut out, mut rest) = (String::new(), line);
    while let Some(i) = find_ascii_ci(rest, key) {
        let (head, tail) = rest.split_at(i + key.len());
        out.push_str(head);
        out.push('…');
        rest = &tail[tail.find([' ', ',']).unwrap_or(tail.len())..];
    }
    out + rest
}

/// A URL's query (`?…`) dropped: tokens travel there.
fn mask_queries(line: &str) -> String {
    let (mut out, mut rest) = (String::new(), line);
    while let Some(i) = rest.find("http") {
        let end = rest[i..].find([' ', '"', '\'']).map_or(rest.len(), |e| i + e);
        let url = &rest[i..end];
        out.push_str(&rest[..i]);
        out.push_str(url.find('?').map_or(url, |q| &url[..=q]));
        if url.contains('?') {
            out.push('…');
        }
        rest = &rest[end..];
    }
    out + rest
}

/// Dotted quads to `x.x.x.x`; a version like 0.745.0.7451137 has too long a part to be one.
fn mask_ipv4(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(start) = rest.find(|c: char| c.is_ascii_digit()) {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let len = rest.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(rest.len());
        let token = &rest[..len];
        let parts: Vec<&str> = token.split('.').collect();
        let is_ip = parts.len() == 4 && parts.iter().all(|p| (1..=3).contains(&p.len()) && p.parse::<u16>().is_ok_and(|n| n <= 255));
        out.push_str(if is_ip { "x.x.x.x" } else { token });
        rest = &rest[len..];
    }
    out + rest
}

/// The last `max` lines of the file, each through `redact` and with `names` masked; empty when it
/// can't be read.
pub fn clean_tail(path: &Path, max: usize, names: &[String]) -> Vec<String> {
    let read = || -> std::io::Result<String> {
        let mut f = std::fs::File::open(path)?;
        let len = f.metadata()?.len();
        f.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES)))?;
        let mut bytes = Vec::new();
        f.read_to_end(&mut bytes)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    };
    let text = read().inspect_err(|e| log::debug!("diagnostics: {} not read: {e}", path.display())).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(max)..].iter().filter_map(|l| redact(l)).map(|l| mask_names(l, names)).collect()
}

/// The user's account and favourite names to `…` (shorter ones would mask ordinary words).
fn mask_names(line: String, names: &[String]) -> String {
    names.iter().filter(|n| n.chars().count() >= 3).fold(line, |l, n| l.replace(n.as_str(), "…"))
}

/// The sections under `max` bytes in all: each gets an equal share and keeps its newest lines
/// (pure). A section using less leaves its share to no one: simple, and it stays under.
fn compose(head: &str, sections: &[(String, Vec<String>)], max: usize) -> String {
    let share = max.saturating_sub(head.len()) / sections.len().max(1);
    let mut out = head.to_owned();
    for (title, lines) in sections {
        let (mut kept, mut used) = (Vec::new(), title.len() + 6);
        for l in lines.iter().rev() {
            used += l.len() + 1;
            if used > share {
                break;
            }
            kept.push(l.as_str());
        }
        kept.reverse();
        out.push_str(&format!("\n== {title}\n"));
        for l in kept {
            out.push_str(l);
            out.push('\n');
        }
    }
    out
}

/// Everything the report is made of: read once on a background thread.
pub struct Sources {
    pub head: String,
    pub roalt_log: std::path::PathBuf,
    pub roblox_logs: Vec<std::path::PathBuf>,
    /// Account and favourite names, kept out of the logs' text.
    pub names: Vec<String>,
}

pub fn report(src: &Sources, max: usize) -> String {
    let mut sections = vec![("RoALT log".to_owned(), clean_tail(&src.roalt_log, ROALT_LINES, &src.names))];
    for (n, log) in src.roblox_logs.iter().enumerate() {
        sections.push((format!("Roblox log {}", n + 1), clean_tail(log, ROBLOX_LINES, &src.names)));
    }
    compose(&src.head, &sections, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_secrets_and_names_out() {
        for bad in [
            "x gameinfo:abc",
            "x authentication ticket here",
            "Cookie: .ROBLOSECURITY=abc",
            "placelauncherurl:https://x",
            "Authorization: Bearer abc",
            "posting to https://discord.com/api/webhooks/1/token",
        ] {
            assert_eq!(redact(bad), None, "{bad}");
        }
        assert_eq!(redact(r"load C:\Users\Marco\AppData\Local\Roblox\x.log").unwrap(), r"load C:\Users\…\AppData\Local\Roblox\x.log");
        assert_eq!(redact(r"a C:\USERS\Bob\x b C:\Users\Amy").unwrap(), r"a C:\USERS\…\x b C:\Users\…");
        assert_eq!(redact("load time userid:123456, sid:abc-def, place 7").unwrap(), "load time userid:…, sid:…, place 7");
        assert_eq!(redact("GET https://games.roblox.com/v1/x?id=5&t=abc failed").unwrap(), "GET https://games.roblox.com/v1/x?… failed");
        assert_eq!(redact("Joining at 128.116.1.2 now").unwrap(), "Joining at x.x.x.x now");
        assert_eq!(redact("client 0.745.0.7451137 started").unwrap(), "client 0.745.0.7451137 started", "a version stays");
        assert_eq!(redact("plain line").unwrap(), "plain line");
        assert_eq!(redact(r"load C:\Users\Marco Rossi\x").unwrap(), r"load C:\Users\…\x", "a name with a space");
        assert_eq!(
            redact("! Joining game '0d6c3f4e-5b2a-4c1d-9e8f-7a6b5c4d3e2f' place 7 at 1.2.3.4").unwrap(),
            "! Joining game '…' place 7 at x.x.x.x"
        );
        assert_eq!(mask_names("MyAlt is running".into(), &["MyAlt".into(), "a".into()]), "… is running");
    }

    /// Over this PC's real Roblox logs: `cargo test -- --ignored redacts_real_logs --nocapture`.
    #[test]
    #[ignore]
    fn redacts_real_logs() {
        let dir = crate::instances::logwatch::logs_dir().unwrap();
        let (mut total, mut kept) = (0, 0);
        for e in std::fs::read_dir(dir).unwrap().flatten() {
            let bytes = std::fs::read(e.path()).unwrap_or_default();
            for line in String::from_utf8_lossy(&bytes).lines() {
                total += 1;
                let Some(clean) = redact(line) else { continue };
                kept += 1;
                let lower = clean.to_ascii_lowercase();
                for bad in ["gameinfo", "roblosecurity", "ticket", "cookie", r"\users\n\"] {
                    assert!(!lower.contains(bad), "{bad} in: {clean}");
                }
            }
        }
        println!("REDACTED {kept} of {total} lines kept");
        assert!(total > 0);
    }

    #[test]
    fn stays_under_the_limit_keeping_the_newest_lines() {
        let lines: Vec<String> = (0..1000).map(|i| format!("line {i}")).collect();
        let out = compose("head\n", &[("a".into(), lines.clone()), ("b".into(), lines)], 2000);
        assert!(out.len() <= 2000, "{}", out.len());
        assert!(out.starts_with("head\n") && out.contains("line 999") && !out.contains("line 0\n"));
        assert!(compose("h", &[], 10).starts_with('h'));
    }
}
