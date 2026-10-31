//! Client-window controls on Linux. Wayland has no portable global window list, and X11
//! controls would need a separate dependency and per-compositor handling. Thumbnails, tiling,
//! micro windows and focus/minimize are therefore not available yet.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::Mutex;

use crate::error::{AppError, Result};

#[derive(Clone)]
pub struct Thumb {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Show {
    Focus,
    Minimize,
    Restore,
}

pub fn thumbnail(_pid: u32, _max_w: u32) -> Option<(Thumb, bool)> {
    None
}

/// A preview through Cordial's dev-control socket. It asks the running engine to write its next
/// presented frame to a file, so it also works for a headless client (`--headless`), which has no
/// window to capture.
pub fn thumbnail_socket(socket: &Path, max_w: u32) -> Option<(Thumb, bool)> {
    let path = std::env::temp_dir().join(format!("roalt-shot-{}.png", uuid::Uuid::new_v4()));
    let mut stream = UnixStream::connect(socket).ok()?;
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).ok()?;
    stream.set_write_timeout(Some(std::time::Duration::from_secs(5))).ok()?;
    writeln!(stream, "screenshot {}", path.display()).ok()?;
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply).ok()?;
    if !reply.trim_start().starts_with("ok") {
        let _ = std::fs::remove_file(&path);
        log::debug!("Cordial screenshot refused: {}", reply.trim());
        return None;
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline && !path.is_file() {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let bytes = std::fs::read(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).ok()?;
    let (w, h) = (img.width(), img.height());
    let target_w = max_w.clamp(16, 1280).min(w.max(1));
    let target_h = ((h as u64 * target_w as u64) / w.max(1) as u64).max(1) as u32;
    let resized = img.resize(target_w, target_h, image::imageops::FilterType::Triangle).to_rgba8();
    Some((Thumb { width: resized.width(), height: resized.height(), rgba: resized.into_raw() }, false))
}

/// Where each hidden window came from. Hyprland has no minimize, so a window is moved to a
/// special workspace; this remembers the workspace to put it back on.
static HIDDEN: Mutex<Vec<(u32, i32)>> = Mutex::new(Vec::new());

fn hyprctl(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("hyprctl").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn window_for(pid: u32) -> Option<(String, i32)> {
    let text = hyprctl(&["-j", "clients"])?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    // The window often belongs to a child Cordial started (and to cage for a nested run), not
    // to the PID RoALT tracks. Look at the whole process family, exact PID first.
    let mut family = vec![pid];
    family.extend(super::process::descendants(pid));
    let w = v.as_array()?.iter().find(|w| {
        w.get("pid").and_then(|p| p.as_u64()).and_then(|p| u32::try_from(p).ok()).is_some_and(|p| family.contains(&p))
    })?;
    let address = w.get("address")?.as_str()?.strip_prefix("0x").unwrap_or(w.get("address")?.as_str()?).to_owned();
    let ws = w.get("workspace").and_then(|w| w.get("id")).and_then(|i| i.as_i64()).unwrap_or(1) as i32;
    Some((format!("address:0x{address}"), ws))
}

fn active_workspace() -> Option<i32> {
    let text = hyprctl(&["-j", "activeworkspace"])?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(v.get("id")?.as_i64()? as i32)
}

fn hidden_workspace_of(pid: u32) -> Option<i32> {
    HIDDEN.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(p, _)| *p == pid).map(|(_, ws)| *ws)
}

fn hide(pid: u32) -> Result<()> {
    let Some((selector, ws)) = window_for(pid) else {
        return Err(AppError::Invalid(tr!("That client has no window yet.")));
    };
    {
        let mut hidden = HIDDEN.lock().unwrap_or_else(|e| e.into_inner());
        hidden.retain(|(p, _)| *p != pid);
        hidden.push((pid, ws));
    }
    hyprctl(&["dispatch", "movetoworkspacesilent", &format!("special:roalt-hidden,{selector}")]);
    Ok(())
}

fn reveal(pid: u32, focus: bool) -> Result<()> {
    let Some((selector, _)) = window_for(pid) else {
        return Err(AppError::Invalid(tr!("That client has no window yet.")));
    };
    let ws = hidden_workspace_of(pid).or_else(active_workspace).unwrap_or(1);
    hyprctl(&["dispatch", "movetoworkspacesilent", &format!("{ws},{selector}")]);
    HIDDEN.lock().unwrap_or_else(|e| e.into_inner()).retain(|(p, _)| *p != pid);
    if focus {
        hyprctl(&["dispatch", "focuswindow", &selector]);
    }
    Ok(())
}

/// Whether the process has a window on this desktop (a hidden one still counts).
pub fn has_window(pid: u32) -> bool {
    window_for(pid).is_some()
}

pub fn unpark_client(pid: u32) {
    let _ = reveal(pid, false);
}

pub fn show(pid: u32, what: Show, _park_it: bool) -> Result<()> {
    match what {
        Show::Focus => reveal(pid, true),
        Show::Minimize => hide(pid),
        Show::Restore => reveal(pid, false),
    }
}

pub const MICRO: (u32, u32) = (320, 240);

pub fn minimize_quietly(pid: u32, _park_it: bool) {
    let _ = hide(pid);
}

pub fn micro(_id: &str, _pid: u32, _on: bool, size: (u32, u32)) -> Result<(u32, u32)> {
    // Hyprland can resize a window, but restoring its exact previous geometry needs the same
    // placement bookkeeping as the Windows path; the dashboard preview and hide/show controls
    // work without it.
    Ok(size)
}

pub fn tile(_clients: &[(String, u32)], _near: usize, _size: Option<(u32, u32)>) -> Vec<String> {
    Vec::new()
}
