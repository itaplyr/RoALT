//! What a client that ended badly leaves behind on Linux, written into RoALT's log (so into the
//! diagnostics report too): how long it really lived, what the tracker knew of it at the end, and
//! the last lines of its log cleaned through `diagnostics::redact`.
//!
//! Linux has no crash dumps or Application event log here, so this path is only the client's own
//! log. It is read-only: the file is opened, its last `TAIL_BYTES` are read, and nothing is
//! written, moved or started.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use super::tracker::Ended;
use crate::diagnostics::redact;

/// How much of the end of a client log is read.
const TAIL_BYTES: u64 = 32 * 1024;
/// How many of its last non-empty lines are logged.
const TAIL_LINES: usize = 12;
/// Roblox's lines can be very long.
const MAX_LINE: usize = 300;
/// The tracker's `clues` are a sentence, but cap them like a log line.
const MAX_CLUES: usize = 500;
/// Characters across all of the client log's lines, so one pathological line can't flood the log.
const MAX_REPORT_CHARS: usize = 6 * 1024;

/// Logs the whole autopsy. Blocking (it reads a file): called on a thread of its own.
pub fn report(c: &Ended) {
    let id = &c.instance_id;
    let lived = c.died_after_ms.map_or_else(|| "unknown".to_owned(), |ms| format!("{} s", ms / 1000));
    let reason = c.reason.as_deref().map_or_else(|| "none".to_owned(), |r| safe(r, MAX_LINE));
    log::warn!(
        "autopsy {id}: {} of account {} ended after {} s (seen gone after {lived}); reason: {reason}",
        c.kind.as_str(),
        c.account_id,
        c.uptime_ms / 1000,
    );
    let place = c.place_id.map_or_else(|| "unknown".to_owned(), |p| p.to_string());
    log::info!("autopsy {id}: pid {}, place {place}, clues: {}", c.pid, safe(&c.clues, MAX_CLUES));
    match c.log.as_deref() {
        Some(path) => log_tail(id, path),
        None => log::info!("autopsy {id}: no client log names this launch"),
    }
}

/// The last lines of the client log `path`, if it can be read.
fn log_tail(id: &str, path: &Path) {
    // Only the name: the full path can carry the user's home directory.
    let name = path.file_name().map_or_else(|| "a client log".to_owned(), |n| n.to_string_lossy().into_owned());
    match read_tail(path) {
        Ok((text, len, partial)) => {
            log::info!("autopsy {id}: client log {name}, {} KB", len >> 10);
            let lines = tail_lines(&text, TAIL_LINES, partial);
            if lines.is_empty() {
                log::info!("autopsy {id}:   (no readable, non-sensitive lines)");
                return;
            }
            let mut used = 0;
            for line in lines {
                used += line.chars().count() + 1;
                if used > MAX_REPORT_CHARS {
                    log::info!("autopsy {id}:   … further lines truncated");
                    break;
                }
                log::info!("autopsy {id}:   {line}");
            }
        }
        Err(e) => log::info!("autopsy {id}: client log {name} named, but could not be read: {e}"),
    }
}

/// The last `TAIL_BYTES` of the file as text, its length, and whether it starts mid-line. Missing,
/// unreadable or non-regular files are an error, never a panic.
fn read_tail(path: &Path) -> std::io::Result<(String, u64, bool)> {
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::with_capacity(TAIL_BYTES as usize);
    file.take(TAIL_BYTES).read_to_end(&mut bytes)?;
    Ok((String::from_utf8_lossy(&bytes).into_owned(), len, start > 0))
}

/// The last `max` non-empty lines of `text`, newest last, each through `redact` and cut to
/// `MAX_LINE`. A line `redact` refuses (it names a secret) is left out. `partial` drops the
/// first line, which starts mid-line when the tail didn't begin at the file's start.
fn tail_lines(text: &str, max: usize, partial: bool) -> Vec<String> {
    let mut lines: Vec<&str> = text.lines().collect();
    if partial && !lines.is_empty() {
        lines.remove(0);
    }
    let mut out: Vec<String> = lines
        .iter()
        .rev()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() { None } else { redact(line) }
        })
        .take(max)
        .map(|line| shortened(&line).to_owned())
        .collect();
    out.reverse();
    out
}

/// `text` through `redact`, with a refused line replaced rather than logged (pure).
fn safe(text: &str, max: usize) -> String {
    match redact(text) {
        Some(clean) => shortened_to(&clean, max).to_owned(),
        None => "…".to_owned(),
    }
}

fn shortened(line: &str) -> &str {
    shortened_to(line, MAX_LINE)
}

fn shortened_to(line: &str, max: usize) -> &str {
    &line[..line.char_indices().nth(max).map_or(line.len(), |(i, _)| i)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_lines_and_drops_empty_ones() {
        let text = "old\n\n  \nnewer\nnewest\n";
        assert_eq!(tail_lines(text, 2, false), ["newer", "newest"]);
        assert_eq!(tail_lines(text, 10, false), ["old", "newer", "newest"]);
        assert!(tail_lines("", 5, false).is_empty());
    }

    #[test]
    fn drops_the_partial_first_line_of_a_tail() {
        assert_eq!(tail_lines("partial\nfull\nlast\n", 5, true), ["full", "last"]);
        assert_eq!(tail_lines("only\n", 5, false), ["only"]);
    }

    #[test]
    fn secrets_are_never_logged() {
        let text = "gameinfo:abc\nsafe line\nticket=xyz\nCookie: .ROBLOSECURITY=abc\nlast\n";
        assert_eq!(tail_lines(text, 10, false), ["safe line", "last"]);
        assert_eq!(safe("gameinfo:abc", MAX_LINE), "…");
        assert_eq!(safe("Joining at 128.116.1.2 now", MAX_LINE), "Joining at x.x.x.x now");
    }

    #[test]
    fn long_lines_are_cut_on_a_character() {
        let long = "é".repeat(MAX_LINE + 5);
        assert_eq!(shortened("short"), "short");
        assert_eq!(shortened(&long).chars().count(), MAX_LINE);
    }

    #[test]
    fn a_missing_log_is_an_error_not_a_panic() {
        assert!(read_tail(Path::new("/nonexistent/roalt-autopsy-test.log")).is_err());
    }
}
