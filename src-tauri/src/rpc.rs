//! Discord rich presence (`Settings.discord_presence`): "Playing RoALT", idling or with the
//! game and how many alts run, plus Get RoALT / Join Discord buttons.
//!
//! Spoken to the Discord app on this PC over its local IPC pipe `\\.\pipe\discord-ipc-N`: each
//! frame is an opcode and a length (u32 LE each), then JSON. Nothing goes over the network from
//! RoALT, and nothing happens while Discord isn't running (the pipe is retried every tick).

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::time::Duration;

use serde_json::{Value, json};
use tauri::{AppHandle, Manager};

use crate::App;
use crate::commands::{cached_game_name, now_ms};
use crate::model::{Instance, InstanceState};

/// The "RoALT" application on Discord's Developer Portal; its art asset `roalt` is the logo.
const CLIENT_ID: &str = "1565066775752741165";
const DOWNLOAD: &str = "https://github.com/inspectsoftware/RoALT/releases/latest";
const INVITE: &str = "https://discord.gg/qcj68tHKCX";
/// Discord allows about 5 activity updates per 20 s; one is sent only when it changed.
const TICK: Duration = Duration::from_secs(5);

const HANDSHAKE: u32 = 0;
const FRAME: u32 = 1;
const CLOSE: u32 = 2;

/// One IPC frame (pure).
fn frame(op: u32, body: &Value) -> Vec<u8> {
    let json = body.to_string();
    let mut b = Vec::with_capacity(8 + json.len());
    b.extend(op.to_le_bytes());
    b.extend((json.len() as u32).to_le_bytes());
    b.extend(json.as_bytes());
    b
}

/// Reads one frame's opcode and JSON; a CLOSE (or anything unreadable) is an error.
fn read_frame(pipe: &mut File) -> std::io::Result<(u32, Value)> {
    let mut head = [0u8; 8];
    pipe.read_exact(&mut head)?;
    let op = u32::from_le_bytes(head[..4].try_into().unwrap());
    let len = u32::from_le_bytes(head[4..].try_into().unwrap()) as usize;
    if len > 64 * 1024 {
        return Err(std::io::Error::other("frame too large"));
    }
    let mut body = vec![0; len];
    pipe.read_exact(&mut body)?;
    let v = serde_json::from_slice(&body).unwrap_or_else(|e| {
        log::debug!("discord presence: unreadable frame: {e}");
        Value::Null
    });
    if op == CLOSE {
        return Err(std::io::Error::other(format!("closed: {v}")));
    }
    Ok((op, v))
}

/// The first Discord pipe that takes our handshake.
fn connect() -> Option<File> {
    (0..10).find_map(|n| {
        let mut pipe = OpenOptions::new().read(true).write(true).open(format!(r"\\.\pipe\discord-ipc-{n}")).ok()?;
        let shook = pipe
            .write_all(&frame(HANDSHAKE, &json!({ "v": 1, "client_id": CLIENT_ID })))
            .and_then(|()| read_frame(&mut pipe));
        if let Err(e) = shook {
            log::debug!("discord presence: handshake on pipe {n} failed: {e}");
            return None;
        }
        Some(pipe)
    })
}

/// Sets (or, with `Value::Null`, clears) the activity and reads Discord's answer.
fn send(pipe: &mut File, activity: &Value, nonce: u64) -> std::io::Result<()> {
    let cmd = json!({
        "cmd": "SET_ACTIVITY",
        "args": { "pid": std::process::id(), "activity": activity },
        "nonce": nonce.to_string(),
    });
    pipe.write_all(&frame(FRAME, &cmd))?;
    let (_, answer) = read_frame(pipe)?;
    if answer["evt"] == "ERROR" {
        log::info!("discord presence refused: {}", answer["data"]["message"]);
    }
    Ok(())
}

/// What the profile shows (pure): `details` and `state`. Idling until a client is in a game,
/// then the game most clients play (the first launched on a tie) and how many run.
fn status(instances: &[Instance], game_name: impl Fn(i64) -> Option<String>) -> (String, String) {
    let live: Vec<&Instance> = instances.iter().filter(|i| i.state != InstanceState::Exited).collect();
    let mut games: Vec<(i64, usize, i64)> = Vec::new(); // place, clients, first start
    for i in live.iter().filter(|i| i.state == InstanceState::InGame) {
        let Some(p) = i.place_id else { continue };
        match games.iter_mut().find(|g| g.0 == p) {
            Some(g) => {
                g.1 += 1;
                g.2 = g.2.min(i.started_at);
            }
            None => games.push((p, 1, i.started_at)),
        }
    }
    let Some(top) = games.iter().max_by(|a, b| a.1.cmp(&b.1).then(b.2.cmp(&a.2))) else {
        let state = if live.is_empty() {
            tr!("Managing Roblox alts")
        } else {
            trn!(live.len(), "{n} alt open", "{n} alts open")
        };
        return (tr!("Idling"), state);
    };
    let state = match games.len() {
        1 => trn!(live.len(), "Running {n} alt", "Running {n} alts"),
        n => trn!(live.len(), "{n} alt · {games} games", "{n} alts · {games} games", games = n),
    };
    let details = match game_name(top.0) {
        Some(name) => tr!("Playing {game}", game = name),
        None => tr!("Playing a Roblox game"),
    };
    (details, state)
}

/// The full activity for `status`, with the time that status began.
fn activity((details, state): &(String, String), since_ms: i64) -> Value {
    json!({
        "details": details,
        "state": state,
        "timestamps": { "start": since_ms / 1000 },
        "assets": { "large_image": "roalt", "large_text": format!("RoALT · {}", tr!("Managing Roblox alts")) },
        "buttons": [
            { "label": tr!("Get RoALT"), "url": DOWNLOAD },
            { "label": tr!("Join Discord"), "url": INVITE },
        ],
    })
}

/// Keeps the presence in step on its own thread (pipe I/O blocks).
pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let mut pipe: Option<File> = None;
        // What Discord shows now: None when cleared or not connected.
        let mut shown: Option<(String, String)> = None;
        let mut since = now_ms();
        let mut nonce = 0u64;
        let mut settings_failed = false;
        loop {
            let s = app.state::<App>();
            let settings = s.db.settings();
            match (&settings, settings_failed) {
                (Err(e), false) => log::debug!("discord presence: settings unreadable, presence off: {e}"),
                (Ok(_), true) => log::debug!("discord presence: settings readable again"),
                _ => {}
            }
            settings_failed = settings.is_err();
            let on = settings.map(|v| v.discord_presence).unwrap_or(false);
            let want = on.then(|| status(&s.tracker.list(), |p| cached_game_name(&s, p)));
            if want.as_ref().map(|w| &w.0) != shown.as_ref().map(|w| &w.0) {
                since = now_ms();
            }
            if want != shown || (want.is_some() && pipe.is_none()) {
                if pipe.is_none() && want.is_some() {
                    pipe = connect();
                }
                if let Some(p) = pipe.as_mut() {
                    nonce += 1;
                    let activity = want.as_ref().map_or(Value::Null, |w| activity(w, since));
                    match send(p, &activity, nonce) {
                        Ok(()) => shown = want.clone(),
                        Err(e) => {
                            log::info!("discord presence: {e}");
                            pipe = None;
                            shown = None;
                        }
                    }
                }
                if want.is_none() {
                    // Off: cleared above, then let go of Discord.
                    pipe = None;
                    shown = None;
                }
            }
            std::thread::sleep(TICK);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LaunchTarget, Limits};

    fn inst(state: InstanceState, place: Option<i64>, started_at: i64) -> Instance {
        Instance {
            id: started_at.to_string(),
            account_id: 1,
            pid: 1,
            state,
            target: LaunchTarget::Place { place_id: place.unwrap_or(1), launch_data: None },
            place_id: place,
            job_id: None,
            started_at,
            ended_at: None,
            via_bootstrapper: false,
            note: None,
            rejoins: 0,
            limits: Limits::default(),
            anti_afk: true,
            afk_at: None,
            throttled: false,
            micro: false,
            fps_cap: None,
            relaunching: false,
            relaunch_problem: None,
            sign_in_problem: false,
            applied_limits: Limits::default(),
        }
    }

    fn name(p: i64) -> Option<String> {
        (p == 10).then(|| "Blox Fruits".into())
    }

    #[test]
    fn idles_until_a_client_is_in_a_game() {
        assert_eq!(status(&[], name), ("Idling".into(), "Managing Roblox alts".into()));
        let joining = [inst(InstanceState::Joining, None, 1), inst(InstanceState::Exited, Some(10), 2)];
        assert_eq!(status(&joining, name), ("Idling".into(), "1 alt open".into()));
    }

    #[test]
    fn shows_the_game_and_the_alt_count() {
        let one = [inst(InstanceState::InGame, Some(10), 1)];
        assert_eq!(status(&one, name), ("Playing Blox Fruits".into(), "Running 1 alt".into()));
        let three = [inst(InstanceState::InGame, Some(10), 1), inst(InstanceState::InGame, Some(10), 2), inst(InstanceState::Starting, None, 3)];
        assert_eq!(status(&three, name), ("Playing Blox Fruits".into(), "Running 3 alts".into()));
    }

    #[test]
    fn picks_the_busiest_game_then_the_first_launched() {
        let mixed = [inst(InstanceState::InGame, Some(20), 1), inst(InstanceState::InGame, Some(10), 2), inst(InstanceState::InGame, Some(10), 3)];
        assert_eq!(status(&mixed, name), ("Playing Blox Fruits".into(), "3 alts · 2 games".into()));
        let tie = [inst(InstanceState::InGame, Some(10), 5), inst(InstanceState::InGame, Some(20), 1)];
        assert_eq!(status(&tie, name).0, "Playing a Roblox game", "place 20 started first; its name isn't cached");
    }

    #[test]
    fn frames_are_opcode_length_json() {
        let f = frame(FRAME, &json!({ "a": 1 }));
        assert_eq!(&f[..4], &1u32.to_le_bytes());
        assert_eq!(&f[4..8], &7u32.to_le_bytes());
        assert_eq!(&f[8..], br#"{"a":1}"#);
    }

    #[test]
    fn activity_has_both_buttons() {
        let a = activity(&("Idling".into(), "Managing Roblox alts".into()), 5_000);
        assert_eq!(a["timestamps"]["start"], 5);
        assert_eq!(a["buttons"][0]["label"], "Get RoALT");
        assert_eq!(a["buttons"][1]["url"], INVITE);
    }
}
