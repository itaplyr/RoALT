//! Discord rich presence (`Settings.discord_presence` + `Settings.discord_rpc`): "Playing RoALT",
//! idling or with the game and how many alts run, plus optional Get RoALT / Join Discord buttons.
//!
//! Spoken to the Discord app on this PC over its local IPC pipe `\\.\pipe\discord-ipc-N`: each
//! frame is an opcode and a length (u32 LE each), then JSON. Every read and write uses overlapped
//! I/O with a 3 s timeout so a stalled Discord client can never hang this thread. Nothing goes
//! over the network from RoALT, and nothing happens while Discord isn't running.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::time::Duration;

use serde_json::{Value, json};
use tauri::{AppHandle, Manager};
use windows::Win32::Foundation::{CloseHandle, ERROR_IO_PENDING, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::{FILE_FLAG_OVERLAPPED, ReadFile, WriteFile};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};

use crate::App;
use crate::commands::{cached_game_name, now_ms};
use crate::model::{DiscordRpc, Instance, InstanceState};

/// The "RoALT" application on Discord's Developer Portal; its art asset `roalt` is the logo.
const CLIENT_ID: &str = "1565066775752741165";
const DOWNLOAD: &str = "https://github.com/inspectsoftware/RoALT/releases/latest";
/// The same invite as the app's (`src/features/donate/links.ts`).
const INVITE: &str = "https://discord.gg/roaltdev";
/// Discord allows about 5 activity updates per 20 s; one is sent only when it changed.
const TICK: Duration = Duration::from_secs(5);
/// Per-operation timeout on the local Discord IPC pipe (ms).
const IO_TIMEOUT_MS: u32 = 3_000;

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

/// Runs one overlapped pipe read or write with `IO_TIMEOUT_MS`, cancelling it on timeout so the
/// stack buffer is never touched after returning.
fn overlapped_io(pipe: &File, op: impl FnOnce(HANDLE, *mut OVERLAPPED) -> windows::core::Result<()>) -> io::Result<usize> {
    let h = HANDLE(pipe.as_raw_handle());
    // SAFETY: manual-reset event closed before returning; `CancelIoEx` + `GetOverlappedResult(wait=true)`
    // ensures the kernel is done with `ov` and the caller's buffer before this stack frame unwinds.
    unsafe {
        let ev = CreateEventW(None, true, false, None).map_err(|e| io::Error::other(e.to_string()))?;
        let mut ov = OVERLAPPED { hEvent: ev, ..Default::default() };
        let res = op(h, &mut ov);
        let out = match res {
            Ok(()) => {
                let mut n = 0u32;
                GetOverlappedResult(h, &ov, &mut n, false).map(|()| n as usize).map_err(|e| io::Error::other(e.to_string()))
            }
            Err(e) if e.code() == ERROR_IO_PENDING.to_hresult() => {
                if WaitForSingleObject(ev, IO_TIMEOUT_MS) == WAIT_OBJECT_0 {
                    let mut n = 0u32;
                    GetOverlappedResult(h, &ov, &mut n, false).map(|()| n as usize).map_err(|e| io::Error::other(e.to_string()))
                } else {
                    let _ = CancelIoEx(h, Some(&ov));
                    let mut n = 0u32;
                    let _ = GetOverlappedResult(h, &ov, &mut n, true);
                    Err(io::Error::new(io::ErrorKind::TimedOut, "Discord pipe timed out"))
                }
            }
            Err(e) => Err(io::Error::other(e.to_string())),
        };
        let _ = CloseHandle(ev);
        out
    }
}

fn write_all_timeout(pipe: &File, mut buf: &[u8]) -> io::Result<()> {
    while !buf.is_empty() {
        let n = overlapped_io(pipe, |h, ov| unsafe { WriteFile(h, Some(buf), None, Some(ov)) })?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "Discord pipe closed"));
        }
        buf = &buf[n..];
    }
    Ok(())
}

fn read_exact_timeout(pipe: &File, mut buf: &mut [u8]) -> io::Result<()> {
    while !buf.is_empty() {
        let n = overlapped_io(pipe, |h, ov| unsafe { ReadFile(h, Some(buf), None, Some(ov)) })?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "Discord pipe closed"));
        }
        buf = &mut buf[n..];
    }
    Ok(())
}

/// Reads one frame's opcode and JSON; a CLOSE (or anything unreadable) is an error.
fn read_frame(pipe: &File) -> io::Result<(u32, Value)> {
    let mut head = [0u8; 8];
    read_exact_timeout(pipe, &mut head)?;
    let op = u32::from_le_bytes(head[..4].try_into().unwrap());
    let len = u32::from_le_bytes(head[4..].try_into().unwrap()) as usize;
    if len > 64 * 1024 {
        return Err(io::Error::other("frame too large"));
    }
    let mut body = vec![0; len];
    read_exact_timeout(pipe, &mut body)?;
    let v = serde_json::from_slice(&body).unwrap_or_else(|e| {
        log::debug!("discord presence: unreadable frame: {e}");
        Value::Null
    });
    if op == CLOSE {
        return Err(io::Error::other(format!("closed: {v}")));
    }
    Ok((op, v))
}

/// The first Discord pipe that takes our handshake.
fn connect() -> Option<File> {
    (0..10).find_map(|n| {
        let pipe = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(FILE_FLAG_OVERLAPPED.0)
            .open(format!(r"\\.\pipe\discord-ipc-{n}"))
            .ok()?;
        let shook = write_all_timeout(&pipe, &frame(HANDSHAKE, &json!({ "v": 1, "client_id": CLIENT_ID })))
            .and_then(|()| read_frame(&pipe));
        if let Err(e) = shook {
            log::debug!("discord presence: handshake on pipe {n} failed: {e}");
            return None;
        }
        Some(pipe)
    })
}

/// Sets (or, with `Value::Null`, clears) the activity and reads Discord's answer.
fn send(pipe: &File, activity: &Value, nonce: u64) -> io::Result<()> {
    let cmd = json!({
        "cmd": "SET_ACTIVITY",
        "args": { "pid": std::process::id(), "activity": activity },
        "nonce": nonce.to_string(),
    });
    write_all_timeout(pipe, &frame(FRAME, &cmd))?;
    let (_, answer) = read_frame(pipe)?;
    if answer["evt"] == "ERROR" {
        log::info!("discord presence refused: {}", answer["data"]["message"]);
    }
    Ok(())
}

/// Substitutes `{game}` and `{count}` in a custom RPC line (capped at Discord's 128-char limit).
fn render_custom(template: &str, game: &str, count: usize) -> String {
    template
        .trim()
        .replace("{game}", game)
        .replace("{count}", &count.to_string())
        .chars()
        .take(128)
        .collect()
}

/// What the profile shows (pure), respecting `DiscordRpc` options.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Presence {
    details: String,
    state: Option<String>,
    show_elapsed: bool,
    show_buttons: bool,
}

fn status(instances: &[Instance], cfg: &DiscordRpc, game_name: impl Fn(i64) -> Option<String>) -> Presence {
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
    let top = games.iter().max_by(|a, b| a.1.cmp(&b.1).then(b.2.cmp(&a.2)));
    let resolved_game = top.and_then(|t| game_name(t.0));
    let game_label = resolved_game.as_deref().unwrap_or("Roblox");

    let auto_details = match top {
        None => tr!("Idling"),
        Some(_) if !cfg.show_game => tr!("Playing a Roblox game"),
        Some(_) => match &resolved_game {
            Some(name) => tr!("Playing {game}", game = name),
            None => tr!("Playing a Roblox game"),
        },
    };
    let auto_state = if !cfg.show_count {
        None
    } else if top.is_none() {
        Some(if live.is_empty() {
            tr!("Managing Roblox alts")
        } else {
            trn!(live.len(), "{n} alt open", "{n} alts open")
        })
    } else {
        Some(match games.len() {
            1 => trn!(live.len(), "Running {n} alt", "Running {n} alts"),
            n => trn!(live.len(), "{n} alt · {games} games", "{n} alts · {games} games", games = n),
        })
    };

    let details = if !cfg.custom_details.trim().is_empty() {
        render_custom(&cfg.custom_details, game_label, live.len())
    } else {
        auto_details
    };
    let state = if !cfg.custom_state.trim().is_empty() {
        Some(render_custom(&cfg.custom_state, game_label, live.len()))
    } else {
        auto_state
    };
    Presence { details, state, show_elapsed: cfg.show_elapsed, show_buttons: cfg.show_buttons }
}

/// The full activity for `Presence`, with the time that status began.
fn activity(p: &Presence, since_ms: i64) -> Value {
    let mut obj = json!({
        "details": p.details,
        "assets": { "large_image": "roalt", "large_text": format!("RoALT · {}", tr!("Managing Roblox alts")) },
    });
    if let Some(state) = &p.state {
        obj["state"] = json!(state);
    }
    if p.show_elapsed {
        obj["timestamps"] = json!({ "start": since_ms / 1000 });
    }
    if p.show_buttons {
        obj["buttons"] = json!([
            { "label": tr!("Get RoALT"), "url": DOWNLOAD },
            { "label": tr!("Join Discord"), "url": INVITE },
        ]);
    }
    obj
}

/// Keeps the presence in step on its own thread (pipe I/O is bounded by `IO_TIMEOUT_MS`).
pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let mut pipe: Option<File> = None;
        // What Discord shows now: None when cleared or not connected.
        let mut shown: Option<Presence> = None;
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
            let (on, cfg) = settings.map(|v| (v.discord_presence, v.discord_rpc)).unwrap_or_default();
            let want = on.then(|| status(&s.tracker.list(), &cfg, |p| cached_game_name(&s, p)));
            if want.as_ref().map(|w| &w.details) != shown.as_ref().map(|w| &w.details) {
                since = now_ms();
            }
            if want != shown || (want.is_some() && pipe.is_none()) {
                if pipe.is_none() && want.is_some() {
                    pipe = connect();
                }
                if let Some(p) = pipe.as_ref() {
                    nonce += 1;
                    let payload = want.as_ref().map_or(Value::Null, |w| activity(w, since));
                    match send(p, &payload, nonce) {
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
            micro_size: None,
            applied_limits: Limits::default(),
            held: false,
        }
    }

    fn name(p: i64) -> Option<String> {
        (p == 10).then(|| "Blox Fruits".into())
    }

    #[test]
    fn idles_until_a_client_is_in_a_game() {
        let def = DiscordRpc::default();
        let p = status(&[], &def, name);
        assert_eq!((p.details.as_str(), p.state.as_deref()), ("Idling", Some("Managing Roblox alts")));
        let joining = [inst(InstanceState::Joining, None, 1), inst(InstanceState::Exited, Some(10), 2)];
        let p = status(&joining, &def, name);
        assert_eq!((p.details.as_str(), p.state.as_deref()), ("Idling", Some("1 alt open")));
    }

    #[test]
    fn shows_the_game_and_the_alt_count() {
        let def = DiscordRpc::default();
        let one = [inst(InstanceState::InGame, Some(10), 1)];
        let p = status(&one, &def, name);
        assert_eq!((p.details.as_str(), p.state.as_deref()), ("Playing Blox Fruits", Some("Running 1 alt")));
        let three = [inst(InstanceState::InGame, Some(10), 1), inst(InstanceState::InGame, Some(10), 2), inst(InstanceState::Starting, None, 3)];
        let p = status(&three, &def, name);
        assert_eq!((p.details.as_str(), p.state.as_deref()), ("Playing Blox Fruits", Some("Running 3 alts")));
    }

    #[test]
    fn picks_the_busiest_game_then_the_first_launched() {
        let def = DiscordRpc::default();
        let mixed = [inst(InstanceState::InGame, Some(20), 1), inst(InstanceState::InGame, Some(10), 2), inst(InstanceState::InGame, Some(10), 3)];
        let p = status(&mixed, &def, name);
        assert_eq!((p.details.as_str(), p.state.as_deref()), ("Playing Blox Fruits", Some("3 alts · 2 games")));
        let tie = [inst(InstanceState::InGame, Some(10), 5), inst(InstanceState::InGame, Some(20), 1)];
        assert_eq!(status(&tie, &def, name).details, "Playing a Roblox game", "place 20 started first; its name isn't cached");
    }

    #[test]
    fn respects_custom_rpc_settings() {
        let one = [inst(InstanceState::InGame, Some(10), 1), inst(InstanceState::InGame, Some(10), 2)];
        let hidden = DiscordRpc { show_game: false, show_count: false, show_elapsed: false, show_buttons: false, ..Default::default() };
        let p = status(&one, &hidden, name);
        assert_eq!((p.details.as_str(), p.state.as_deref()), ("Playing a Roblox game", None));
        let a = activity(&p, 5_000);
        assert!(a.get("state").is_none() && a.get("timestamps").is_none() && a.get("buttons").is_none());

        let custom = DiscordRpc {
            custom_details: "Grinding {game}".into(),
            custom_state: "{count} accounts active".into(),
            ..Default::default()
        };
        let p = status(&one, &custom, name);
        assert_eq!((p.details.as_str(), p.state.as_deref()), ("Grinding Blox Fruits", Some("2 accounts active")));
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
        let p = status(&[], &DiscordRpc::default(), name);
        let a = activity(&p, 5_000);
        assert_eq!(a["timestamps"]["start"], 5);
        assert_eq!(a["buttons"][0]["label"], "Get RoALT");
        assert_eq!(a["buttons"][1]["url"], INVITE);
    }
}
