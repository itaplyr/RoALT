//! "Mute clients on launch" on Linux: sets the PipeWire/PulseAudio sink-input of `pid` muted or
//! not. The settings file is untouched; only the running client's own stream is changed. Uses
//! `pactl`, which is present on both PulseAudio and PipeWire desktops.

use std::process::Command;
use std::time::Duration;

/// A client can open its audio stream a moment after it reaches a game: tries this often.
const TRIES: u32 = 5;
const STEP: Duration = Duration::from_secs(2);

/// Mutes (or unmutes) every sink-input owned by `pid`; true when one was set. Failures are logged.
pub fn set_mute(pid: u32, mute: bool) -> bool {
    for attempt in 1..=TRIES {
        match set_once(pid, mute) {
            Ok(n) if n > 0 => return true,
            Ok(_) => {}
            Err(e) => log::debug!("client {pid}: setting muted: {mute} failed (try {attempt}): {e}"),
        }
        if attempt < TRIES {
            std::thread::sleep(STEP);
        }
    }
    false
}

fn set_once(pid: u32, mute: bool) -> Result<usize, String> {
    let out = Command::new("pactl")
        .args(["--format=json", "list", "sink-inputs"])
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("pactl list exited {:?}", out.status.code()));
    }
    let inputs: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let want = pid.to_string();
    let mut changed = 0;
    for input in inputs.as_array().into_iter().flatten() {
        let owner = input
            .get("properties")
            .and_then(|p| p.get("application.process.id"))
            .and_then(|v| v.as_str());
        if owner != Some(want.as_str()) {
            continue;
        }
        let Some(index) = input.get("index").and_then(|v| v.as_u64()) else { continue };
        let ok = Command::new("pactl")
            .args(["set-sink-input-mute", &index.to_string(), if mute { "1" } else { "0" }])
            .status()
            .map_err(|e| e.to_string())?
            .success();
        if ok {
            changed += 1;
        } else {
            return Err(format!("pactl could not set sink-input {index}"));
        }
    }
    Ok(changed)
}
