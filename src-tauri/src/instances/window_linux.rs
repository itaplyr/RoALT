//! Client-window controls on Linux. There is no portable global window API, so this talks to the
//! compositor that is actually running:
//!
//! * Hyprland through `hyprctl` (0.55+ Lua dispatches, older positional dispatches);
//! * Sway and Sway-compatible Wayland sessions through `swaymsg` (`SWAYSOCK`);
//! * X11 through `xdotool`, falling back to `wmctrl` where it can.
//!
//! An unrecognised Wayland compositor (KDE, GNOME and the like) is deliberately a no-op: X11 tools
//! under XWayland cannot see the native Wayland windows, so pretending otherwise would only move
//! the wrong thing. Previews never depend on any of this: they come from Cordial's dev-control
//! socket and keep working on every desktop, including `--headless`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::error::{AppError, Result};

#[derive(Clone)]
pub struct Thumb {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone)]
pub struct Shot {
    pub thumb: Arc<Thumb>,
    pub seq: u32,
    pub at: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Status {
    Live = 0,
    Minimized = 1,
    NoWindow = 2,
    Pending = 3,
    Unsupported = 4,
}

pub struct Preview {
    pub status: Status,
    pub hidden: bool,
    pub shot: Option<Shot>,
}

impl Preview {
    /// Width, height, status, 1 if hidden else 0, picture number and its age in ms (u32
    /// little-endian each; the size, number and age are 0 without a picture), then RGBA rows
    /// top-down, unless the picture is `known` already.
    pub fn to_bytes(&self, known: u32) -> Vec<u8> {
        let (w, h, seq, age) = self.shot.as_ref().map_or((0, 0, 0, 0), |s| {
            (s.thumb.width, s.thumb.height, s.seq, u32::try_from(s.at.elapsed().as_millis()).unwrap_or(u32::MAX))
        });
        let rgba = self.shot.as_ref().filter(|s| s.seq != known).map_or(&[][..], |s| &s.thumb.rgba);
        let mut b = Vec::with_capacity(24 + rgba.len());
        for v in [w, h, self.status as u32, u32::from(self.hidden), seq, age] {
            b.extend(v.to_le_bytes());
        }
        b.extend_from_slice(rgba);
        b
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Show {
    Focus,
    Minimize,
    Restore,
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

static SEQ: AtomicU32 = AtomicU32::new(1);

/// The newest frame Cordial can provide. A headless client has no host window, so its profile is
/// found from the process command line and the dev-control socket does the rest.
pub fn preview(pid: Option<u32>, max_w: u32) -> Preview {
    let Some(pid) = pid else {
        return Preview { status: Status::NoWindow, hidden: false, shot: None };
    };
    let Some(user) = super::process::profile_of(pid) else {
        return Preview { status: Status::NoWindow, hidden: false, shot: None };
    };
    let Some(socket) = crate::launcher::client::devctl_socket(user) else {
        return Preview { status: Status::NoWindow, hidden: false, shot: None };
    };
    match thumbnail_socket(&socket, max_w) {
        Some((thumb, _)) => {
            let seq = SEQ.fetch_add(1, Ordering::Relaxed);
            Preview {
                status: Status::Live,
                hidden: hidden_of(pid),
                shot: Some(Shot { thumb: Arc::new(thumb), seq, at: Instant::now() }),
            }
        }
        None => Preview { status: Status::Pending, hidden: false, shot: None },
    }
}

// ---------------------------------------------------------------------------------------------
// Compositor abstraction
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Compositor {
    Hyprland,
    Sway,
    X11,
}

/// Whether a program can be found on PATH, without spawning anything.
fn tool_exists(name: &str) -> bool {
    if name.contains('/') {
        return Path::new(name).is_file();
    }
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
}

fn wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v.eq_ignore_ascii_case("wayland"))
}

/// Which desktop controls are available. Hyprland and Sway are recognised by their own session
/// variables; X11 only when this is not a Wayland session, so an unknown Wayland compositor stays
/// a no-op instead of being poked through XWayland.
fn compositor() -> Option<Compositor> {
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() && tool_exists("hyprctl") {
        return Some(Compositor::Hyprland);
    }
    if std::env::var_os("SWAYSOCK").is_some() && tool_exists("swaymsg") {
        return Some(Compositor::Sway);
    }
    if wayland_session() {
        return None;
    }
    if std::env::var_os("DISPLAY").is_some() && (tool_exists("xdotool") || tool_exists("wmctrl")) {
        return Some(Compositor::X11);
    }
    None
}

/// A program's output if it ran successfully; None for a missing tool, a non-zero exit or a
/// spawn failure. Callers treat all of them as "this control is not available".
fn run(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn run_ok(program: &str, args: &[&str]) -> bool {
    run(program, args).is_some()
}

fn family(pid: u32) -> Vec<u32> {
    let mut family = vec![pid];
    family.extend(super::process::descendants(pid));
    family
}

/// The window that belongs to `pid` or one of the processes it started.
#[derive(Clone, Debug)]
enum Selector {
    /// Hyprland window selector, validated as `address:0x<hex>`.
    Hypr(String),
    /// A numeric pid used as a Sway `[pid=N]` criterion.
    Sway(u32),
    /// A numeric X11 window id.
    X11(u64),
}

#[derive(Clone, Debug)]
struct WindowInfo {
    selector: Selector,
    size: (i32, i32),
    /// Hyprland workspace id as a string; unused on the other compositors.
    workspace: Option<String>,
}

fn window_info(pid: u32) -> Option<WindowInfo> {
    match compositor()? {
        Compositor::Hyprland => hypr_window_info(pid),
        Compositor::Sway => sway_window_info(pid),
        Compositor::X11 => x11_window_info(pid),
    }
}

// ---------------------------------------------------------------------------------------------
// Hyprland
// ---------------------------------------------------------------------------------------------

fn hyprctl(args: &[&str]) -> Option<String> {
    run("hyprctl", args)
}

/// Hyprland 0.55+ dispatches Lua expressions; older releases took positional strings. Try the
/// Lua form first, then the legacy one, so this keeps working across a user's Hyprland update.
fn hypr_dispatch(lua: &str, legacy: Option<(&str, &str)>) -> bool {
    if hyprctl(&["dispatch", lua]).is_some_and(|s| !s.to_ascii_lowercase().contains("error")) {
        return true;
    }
    legacy.is_some_and(|(name, arg)| hyprctl(&["dispatch", name, arg]).is_some())
}

fn hypr_window_info(pid: u32) -> Option<WindowInfo> {
    let text = hyprctl(&["-j", "clients"])?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    // The window often belongs to a child Cordial started (and to cage for a nested run), not
    // to the PID RoALT tracks. Look at the whole process family, exact PID first.
    let family = family(pid);
    let w = v.as_array()?.iter().find(|w| {
        w.get("pid")
            .and_then(|p| p.as_u64())
            .and_then(|p| u32::try_from(p).ok())
            .is_some_and(|p| family.contains(&p))
    })?;
    let address = w.get("address")?.as_str()?.strip_prefix("0x").unwrap_or(w.get("address")?.as_str()?).to_owned();
    // Only a hexadecimal address reaches the dispatcher, never a window title.
    if address.is_empty() || !address.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let ws = w.get("workspace").and_then(|w| w.get("id")).and_then(|i| i.as_i64()).unwrap_or(1) as i32;
    let size = (
        w.get("size")?.as_array()?.first()?.as_i64()? as i32,
        w.get("size")?.as_array()?.get(1)?.as_i64()? as i32,
    );
    Some(WindowInfo { selector: Selector::Hypr(format!("address:0x{address}")), size, workspace: Some(ws.to_string()) })
}

fn hypr_active_workspace() -> Option<i32> {
    let text = hyprctl(&["-j", "activeworkspace"])?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(v.get("id")?.as_i64()? as i32)
}

fn hypr_focused_monitor() -> Option<(i32, i32, i32, i32)> {
    let text = hyprctl(&["-j", "monitors"])?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let mon = v.as_array()?.iter().find(|m| m.get("focused").and_then(|f| f.as_bool()) == Some(true))?;
    Some((
        mon.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
        mon.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
        mon.get("width").and_then(|v| v.as_i64()).unwrap_or(1280) as i32,
        mon.get("height").and_then(|v| v.as_i64()).unwrap_or(720) as i32,
    ))
}

fn hypr_focus(selector: &str) -> bool {
    hypr_dispatch(
        &format!(r#"hl.dsp.focus({{ window = "{selector}" }})"#),
        Some(("focuswindow", selector)),
    )
}

fn hypr_move_workspace(selector: &str, workspace: &str) -> bool {
    hypr_dispatch(
        &format!(r#"hl.dsp.window.move({{ workspace = "{workspace}", window = "{selector}" }})"#),
        Some(("movetoworkspacesilent", &format!("{workspace},{selector}"))),
    )
}

fn hypr_move(selector: &str, x: i32, y: i32) -> bool {
    hypr_dispatch(
        &format!(r#"hl.dsp.window.move({{ x = {x}, y = {y}, relative = false, window = "{selector}" }})"#),
        Some(("movewindowpixel", &format!("exact {x} {y},{selector}"))),
    )
}

fn hypr_resize(selector: &str, w: i32, h: i32) -> bool {
    hypr_dispatch(
        &format!(r#"hl.dsp.window.resize({{ x = {w}, y = {h}, relative = false, window = "{selector}" }})"#),
        Some(("resizewindowpixel", &format!("exact {w} {h},{selector}"))),
    )
}

// ---------------------------------------------------------------------------------------------
// Sway / i3-compatible Wayland
// ---------------------------------------------------------------------------------------------

fn sway_tree() -> Option<serde_json::Value> {
    let text = run("swaymsg", &["-t", "get_tree"])?;
    serde_json::from_str(&text).ok()
}

fn node_pid(node: &serde_json::Value) -> Option<u32> {
    node.get("pid").and_then(|p| p.as_u64()).and_then(|p| u32::try_from(p).ok())
}

fn find_node<'a>(node: &'a serde_json::Value, family: &[u32]) -> Option<&'a serde_json::Value> {
    if node_pid(node).is_some_and(|p| family.contains(&p)) {
        return Some(node);
    }
    for key in ["nodes", "floating_nodes"] {
        if let Some(children) = node.get(key).and_then(|v| v.as_array()) {
            for child in children {
                if let Some(found) = find_node(child, family) {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn sway_node(pid: u32) -> Option<serde_json::Value> {
    let tree = sway_tree()?;
    find_node(&tree, &family(pid)).cloned()
}

fn sway_window_info(pid: u32) -> Option<WindowInfo> {
    let node = sway_node(pid)?;
    let owner = node_pid(&node)?;
    let rect = node.get("rect")?;
    Some(WindowInfo {
        selector: Selector::Sway(owner),
        size: (rect.get("width")?.as_i64()? as i32, rect.get("height")?.as_i64()? as i32),
        workspace: None,
    })
}

fn sway_focused_output() -> Option<(i32, i32, i32, i32)> {
    let text = run("swaymsg", &["-t", "get_outputs"])?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let out = v.as_array()?.iter().find(|o| o.get("focused").and_then(|f| f.as_bool()) == Some(true))?;
    let rect = out.get("rect")?;
    Some((
        rect.get("x")?.as_i64()? as i32,
        rect.get("y")?.as_i64()? as i32,
        rect.get("width")?.as_i64()? as i32,
        rect.get("height")?.as_i64()? as i32,
    ))
}

fn find_focused(node: &serde_json::Value) -> Option<u64> {
    if node.get("focused").and_then(|f| f.as_bool()) == Some(true) && node_pid(node).is_some() {
        return node.get("id")?.as_u64();
    }
    for key in ["nodes", "floating_nodes"] {
        if let Some(children) = node.get(key).and_then(|v| v.as_array()) {
            for child in children {
                if let Some(id) = find_focused(child) {
                    return Some(id);
                }
            }
        }
    }
    None
}

fn sway_focused_id() -> Option<u64> {
    find_focused(&sway_tree()?)
}

/// Run one Sway command against the window whose pid is `pid`. The criterion is always numeric;
/// a window title never reaches sway's command language.
fn sway_sel(pid: u32, command: &str) -> bool {
    let Ok(out) = std::process::Command::new("swaymsg").arg(format!("[pid={pid}] {command}")).output() else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    // swaymsg can exit 0 while reporting a command error; treat that as "not done".
    let reply = format!(
        "{} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    !reply.to_ascii_lowercase().contains("error")
}

fn sway_hide(pid: u32) -> bool {
    sway_sel(pid, "move scratchpad")
}

fn sway_reveal(pid: u32) -> bool {
    sway_sel(pid, "move workspace current") || sway_sel(pid, "move container to workspace current")
}

fn sway_press_f13(pid: u32) -> bool {
    let mut presses: Vec<(&str, &[&str])> = Vec::new();
    if tool_exists("wtype") {
        presses.push(("wtype", &["-k", "F13"]));
    }
    if tool_exists("ydotool") {
        presses.push(("ydotool", &["key", "183:1", "183:0"]));
    }
    if presses.is_empty() {
        return false;
    }
    let previous = sway_focused_id();
    let target = sway_node(pid).and_then(|n| n.get("id").and_then(|i| i.as_u64()));
    if !sway_sel(pid, "focus") {
        return false;
    }
    std::thread::sleep(Duration::from_millis(30));
    let sent = presses.into_iter().any(|(tool, args)| run_ok(tool, args));
    if let Some(previous) = previous
        && Some(previous) != target
    {
        let _ = run_ok("swaymsg", &[&format!("[con_id={previous}] focus")]);
    }
    sent
}

// ---------------------------------------------------------------------------------------------
// X11 (xdotool, with wmctrl fallback)
// ---------------------------------------------------------------------------------------------

fn parse_hex_id(text: &str) -> Option<u64> {
    let text = text.trim();
    let digits = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")).unwrap_or(text);
    u64::from_str_radix(digits, 16).ok()
}

fn x11_search_pid(pid: u32) -> Option<u64> {
    let pid = pid.to_string();
    let out = run("xdotool", &["search", "--onlyvisible", "--pid", &pid])
        .or_else(|| run("xdotool", &["search", "--pid", &pid]))?;
    // Only a plain decimal id is accepted; xdotool prints one window id per line.
    out.lines().find_map(|line| line.trim().parse::<u64>().ok())
}

fn wmctrl_search_pid(pid: u32) -> Option<u64> {
    let out = run("wmctrl", &["-lp"])?;
    for line in out.lines() {
        let mut fields = line.split_whitespace();
        let (Some(id), Some(_desktop), Some(wpid)) = (fields.next(), fields.next(), fields.next()) else {
            continue;
        };
        if wpid.parse::<u32>().ok() == Some(pid) {
            return parse_hex_id(id);
        }
    }
    None
}

fn x11_size(id: u64) -> Option<(i32, i32)> {
    if tool_exists("xdotool")
        && let Some(out) = run("xdotool", &["getwindowgeometry", "--shell", &id.to_string()])
    {
        let mut width = None;
        let mut height = None;
        for line in out.lines() {
            if let Some(v) = line.strip_prefix("WIDTH=") {
                width = v.trim().parse::<i32>().ok();
            }
            if let Some(v) = line.strip_prefix("HEIGHT=") {
                height = v.trim().parse::<i32>().ok();
            }
        }
        if let (Some(w), Some(h)) = (width, height) {
            return Some((w, h));
        }
    }
    let out = run("wmctrl", &["-lG"])?;
    for line in out.lines() {
        let mut fields = line.split_whitespace();
        let (Some(idtxt), Some(_desktop), Some(_x), Some(_y), Some(w), Some(h)) =
            (fields.next(), fields.next(), fields.next(), fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if parse_hex_id(idtxt) != Some(id) {
            continue;
        }
        if let (Ok(w), Ok(h)) = (w.parse::<i32>(), h.parse::<i32>()) {
            return Some((w, h));
        }
    }
    None
}

fn x11_window_info(pid: u32) -> Option<WindowInfo> {
    for p in family(pid) {
        if let Some(id) = x11_search_pid(p).or_else(|| wmctrl_search_pid(p)) {
            let size = x11_size(id).unwrap_or((1280, 720));
            return Some(WindowInfo { selector: Selector::X11(id), size, workspace: None });
        }
    }
    None
}

fn x11_hex(id: u64) -> String {
    format!("0x{id:x}")
}

fn x11_focus(id: u64) -> bool {
    if run_ok("xdotool", &["windowactivate", &id.to_string()]) {
        return true;
    }
    run_ok("wmctrl", &["-i", "-a", &x11_hex(id)])
}

fn x11_resize(id: u64, w: i32, h: i32) -> bool {
    if run_ok("xdotool", &["windowsize", &id.to_string(), &w.to_string(), &h.to_string()]) {
        return true;
    }
    run_ok("wmctrl", &["-i", "-r", &x11_hex(id), "-e", &format!("0,-1,-1,{w},{h}")])
}

fn x11_move(id: u64, x: i32, y: i32) -> bool {
    if run_ok("xdotool", &["windowmove", &id.to_string(), &x.to_string(), &y.to_string()]) {
        return true;
    }
    run_ok("wmctrl", &["-i", "-r", &x11_hex(id), "-e", &format!("0,{x},{y},-1,-1")])
}

fn x11_hide(id: u64) -> bool {
    // The EWMH hidden hint is the safest hide; a plain unmap is the fallback.
    if tool_exists("wmctrl") && run_ok("wmctrl", &["-i", "-r", &x11_hex(id), "-b", "add,hidden"]) {
        return true;
    }
    run_ok("xdotool", &["windowunmap", &id.to_string()])
}

fn x11_reveal(id: u64, focus: bool) -> bool {
    let mut shown = false;
    if tool_exists("wmctrl") {
        shown |= run_ok("wmctrl", &["-i", "-r", &x11_hex(id), "-b", "remove,hidden"]);
    }
    if tool_exists("xdotool") {
        shown |= run_ok("xdotool", &["windowmap", &id.to_string()]);
    }
    if focus {
        shown |= x11_focus(id);
    }
    shown
}

fn x11_press_f13(id: u64) -> bool {
    let id = id.to_string();
    if run_ok("xdotool", &["key", "--window", &id, "F13"]) {
        return true;
    }
    // Some servers refuse the send-event form; fall back to a focused XTEST press and put the
    // previous window back.
    let previous = run("xdotool", &["getactivewindow"]).and_then(|v| v.trim().parse::<u64>().ok());
    if !run_ok("xdotool", &["windowactivate", &id]) {
        return false;
    }
    std::thread::sleep(Duration::from_millis(30));
    let sent = run_ok("xdotool", &["key", "F13"]);
    if let Some(previous) = previous
        && previous.to_string() != id
    {
        let _ = run_ok("xdotool", &["windowactivate", &previous.to_string()]);
    }
    sent
}

fn x11_screen() -> Option<(i32, i32, i32, i32)> {
    if let Some(out) = run("xdotool", &["getdisplaygeometry"]) {
        let mut fields = out.split_whitespace();
        if let (Some(w), Some(h)) = (fields.next(), fields.next())
            && let (Ok(w), Ok(h)) = (w.parse::<i32>(), h.parse::<i32>())
        {
            return Some((0, 0, w, h));
        }
    }
    // wmctrl -d work area: `... WA: 0,0 1920,1080 ...`
    let out = run("wmctrl", &["-d"])?;
    for line in out.lines() {
        let Some(rest) = line.split("WA:").nth(1) else { continue };
        let mut fields = rest.split_whitespace();
        let (Some(position), Some(size)) = (fields.next(), fields.next()) else { continue };
        let (Some(x), Some(y)) = split_pair::<i32>(position, ',') else { continue };
        let (Some(w), Some(h)) = split_pair::<i32>(size, 'x') else { continue };
        return Some((x, y, w, h));
    }
    None
}

fn split_pair<T: std::str::FromStr>(text: &str, sep: char) -> (Option<T>, Option<T>) {
    let mut fields = text.split(sep);
    (fields.next().and_then(|v| v.parse().ok()), fields.next().and_then(|v| v.parse().ok()))
}

// ---------------------------------------------------------------------------------------------
// Hidden windows
// ---------------------------------------------------------------------------------------------

/// Where each hidden window came from, so it can be put back. Hyprland has no minimize, so a
/// window is moved to a special workspace and its old workspace id is remembered. Sway uses the
/// scratchpad, X11 an EWMH hint or an unmap; both come back to where the compositor puts them.
#[derive(Clone, Debug)]
enum Saved {
    Hyprland(i32),
    Sway,
    X11,
}

static HIDDEN: Mutex<Vec<(u32, Saved)>> = Mutex::new(Vec::new());

fn hidden_of(pid: u32) -> bool {
    HIDDEN.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|(p, _)| *p == pid)
}

fn hidden_saved(pid: u32) -> Option<Saved> {
    HIDDEN.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(p, _)| *p == pid).map(|(_, s)| s.clone())
}

fn remember_hidden(pid: u32, saved: Saved) {
    let mut hidden = HIDDEN.lock().unwrap_or_else(|e| e.into_inner());
    hidden.retain(|(p, _)| *p != pid);
    hidden.push((pid, saved));
}

fn forget_hidden(pid: u32) {
    HIDDEN.lock().unwrap_or_else(|e| e.into_inner()).retain(|(p, _)| *p != pid);
}

// ---------------------------------------------------------------------------------------------
// Window operations
// ---------------------------------------------------------------------------------------------

fn hide(pid: u32) -> Result<()> {
    let Some(info) = window_info(pid) else {
        return Err(AppError::Invalid(tr!("That client has no window yet.")));
    };
    match &info.selector {
        Selector::Hypr(selector) => {
            let workspace = info.workspace.as_deref().and_then(|s| s.parse::<i32>().ok()).unwrap_or(1);
            if hypr_move_workspace(selector, "special:roalt-hidden") {
                remember_hidden(pid, Saved::Hyprland(workspace));
            }
        }
        Selector::Sway(owner) => {
            if sway_hide(*owner) {
                remember_hidden(pid, Saved::Sway);
            }
        }
        Selector::X11(id) => {
            if x11_hide(*id) {
                remember_hidden(pid, Saved::X11);
            }
        }
    }
    Ok(())
}

fn reveal(pid: u32, focus: bool) -> Result<()> {
    let Some(info) = window_info(pid) else {
        return Err(AppError::Invalid(tr!("That client has no window yet.")));
    };
    match &info.selector {
        Selector::Hypr(selector) => {
            let saved = match hidden_saved(pid) {
                Some(Saved::Hyprland(workspace)) => Some(workspace),
                _ => None,
            };
            let workspace = saved.or_else(hypr_active_workspace).unwrap_or(1);
            hypr_move_workspace(selector, &workspace.to_string());
            if focus {
                hypr_focus(selector);
            }
        }
        Selector::Sway(owner) => {
            sway_reveal(*owner);
            if focus {
                sway_sel(*owner, "focus");
            }
        }
        Selector::X11(id) => {
            x11_reveal(*id, focus);
        }
    }
    forget_hidden(pid);
    Ok(())
}

/// Sends a real F13 to the client's window. F13 is unbound in games, so this counts as input for
/// Roblox's idle timer without moving the character.
pub fn press_f13(pid: u32) -> bool {
    let Some(info) = window_info(pid) else { return false };
    match &info.selector {
        Selector::Hypr(selector) => hypr_dispatch(
            &format!(r#"hl.dsp.send_shortcut({{ mods = "", key = "code:191", window = "{selector}" }})"#),
            None,
        ),
        Selector::Sway(owner) => sway_press_f13(*owner),
        Selector::X11(id) => x11_press_f13(*id),
    }
}

/// Whether the process has a window on this desktop (a hidden one still counts).
pub fn has_window(pid: u32) -> bool {
    window_info(pid).is_some()
}

#[allow(dead_code)]
pub fn unpark_client(pid: u32) {
    let _ = reveal(pid, false);
}

pub fn show(pid: u32, what: Show) -> Result<()> {
    // An unknown Wayland compositor has no dependable window control: a no-op, not an error.
    if compositor().is_none() {
        return Ok(());
    }
    match what {
        Show::Focus => reveal(pid, true),
        Show::Minimize => hide(pid),
        Show::Restore => reveal(pid, false),
    }
}

#[allow(dead_code)]
pub fn show_all(pids: &[u32], what: Show) {
    for &pid in pids {
        if let Err(e) = show(pid, what) {
            log::debug!("show all: client {pid}: {e}");
        }
    }
}

pub fn hide_on_launch(pid: u32) {
    let _ = hide(pid);
}

pub fn recover() {}

pub fn sweep(_clients: &[(u32, Option<(u32, u32)>)]) {}

#[allow(dead_code)]
pub fn set_live_hidden_previews(_on: bool) {}

#[allow(dead_code)]
pub const MICRO: (u32, u32) = (320, 240);

static MICRO_OLD: Mutex<Vec<(u32, (i32, i32))>> = Mutex::new(Vec::new());

pub fn micro(_id: &str, pid: u32, on: bool, size: (u32, u32)) -> Result<(u32, u32)> {
    // An unknown Wayland compositor has no dependable window control: report the requested size
    // without pretending to have changed anything.
    if compositor().is_none() {
        return Ok(size);
    }
    let info = window_info(pid).ok_or_else(|| AppError::Invalid(tr!("That client has no window yet.")))?;
    let (w, h) = if on {
        MICRO_OLD.lock().unwrap_or_else(|e| e.into_inner()).push((pid, info.size));
        (size.0 as i32, size.1 as i32)
    } else {
        let old = MICRO_OLD
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .rev()
            .find(|(p, _)| *p == pid)
            .map(|(_, s)| *s);
        MICRO_OLD.lock().unwrap_or_else(|e| e.into_inner()).retain(|(p, _)| *p != pid);
        old.unwrap_or((size.0 as i32, size.1 as i32))
    };
    let ok = match &info.selector {
        Selector::Hypr(selector) => hypr_resize(selector, w, h),
        Selector::Sway(owner) => sway_sel(*owner, &format!("resize set {w} {h}")),
        Selector::X11(id) => x11_resize(*id, w, h),
    };
    if ok { Ok((w as u32, h as u32)) } else { Err(AppError::Invalid(tr!("That client's window could not be resized."))) }
}

/// Cells (x, y, w, h) for `n` windows in `area`: a square-ish grid, or fixed cells of `micro`
/// size. `+4` keeps adjacent windows from sharing a border.
fn layout(n: usize, area: (i32, i32, i32, i32), micro: Option<(u32, u32)>) -> Vec<(i32, i32, i32, i32)> {
    let (ax, ay, aw, ah) = area;
    if n == 0 || aw <= 0 || ah <= 0 {
        return Vec::new();
    }
    let cols = (n as f64).sqrt().ceil() as i32;
    let rows = ((n as i32) + cols - 1) / cols;
    let (cw, ch) = micro.map_or(((aw / cols).max(1), (ah / rows).max(1)), |(w, h)| (w as i32, h as i32));
    (0..n)
        .map(|i| {
            let (col, row) = (i as i32 % cols, i as i32 / cols);
            (ax + col * cw + 4, ay + row * ch + 4, cw, ch)
        })
        .collect()
}

pub fn tile(clients: &[(String, u32)], _near: usize, micro_size: Option<(u32, u32)>) -> Vec<String> {
    let Some(compositor) = compositor() else { return Vec::new() };
    let (area, active) = match compositor {
        Compositor::Hyprland => {
            let Some(area) = hypr_focused_monitor() else { return Vec::new() };
            (area, hypr_active_workspace())
        }
        Compositor::Sway => {
            let Some(area) = sway_focused_output() else { return Vec::new() };
            (area, None)
        }
        Compositor::X11 => {
            let Some(area) = x11_screen() else { return Vec::new() };
            (area, None)
        }
    };
    let visible: Vec<(&String, WindowInfo)> = clients
        .iter()
        .filter_map(|(id, pid)| {
            if hidden_of(*pid) {
                return None;
            }
            let info = window_info(*pid)?;
            if compositor == Compositor::Hyprland
                && let Some(active) = active
                && info.workspace.as_deref().and_then(|s| s.parse::<i32>().ok()) != Some(active)
            {
                return None;
            }
            Some((id, info))
        })
        .collect();
    let cells = layout(visible.len(), area, micro_size);
    let mut placed = Vec::new();
    for ((id, info), (x, y, cw, ch)) in visible.iter().zip(cells) {
        let moved = match &info.selector {
            Selector::Hypr(selector) => hypr_move(selector, x, y),
            Selector::Sway(owner) => sway_sel(*owner, &format!("move position {x} {y}")),
            Selector::X11(id) => x11_move(*id, x, y),
        };
        let resized = match &info.selector {
            Selector::Hypr(selector) => hypr_resize(selector, cw, ch),
            Selector::Sway(owner) => sway_sel(*owner, &format!("resize set {cw} {ch}")),
            Selector::X11(id) => x11_resize(*id, cw, ch),
        };
        if moved || resized {
            placed.push((*id).clone());
        }
    }
    placed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_fill_the_area_in_a_grid() {
        let area = (0, 40, 1920, 1000);
        assert_eq!(layout(0, area, None), []);
        assert_eq!(layout(1, area, None), [(4, 44, 1920, 1000)]);
        assert_eq!(layout(4, area, None)[3], (964, 544, 960, 500));
        let micro = layout(3, area, Some((320, 240)));
        assert_eq!(micro, [(4, 44, 320, 240), (324, 44, 320, 240), (4, 284, 320, 240)]);
        assert_eq!(layout(2, (0, 0, 100, 100), Some((300, 300))), [(4, 4, 300, 300), (304, 4, 300, 300)]);
        assert_eq!(layout(1, (0, 0, 0, 100), None), []);
    }

    #[test]
    fn window_ids_are_read_as_numbers_not_text() {
        assert_eq!(parse_hex_id("0x04000003"), Some(0x0400_0003));
        assert_eq!(parse_hex_id(" 4000003 "), Some(0x0400_0003));
        assert_eq!(parse_hex_id("not-a-window"), None);
        assert_eq!(split_pair::<i32>("1920,1080", ','), (Some(1920), Some(1080)));
        assert_eq!(split_pair::<i32>("1920x1080", 'x'), (Some(1920), Some(1080)));
        assert_eq!(split_pair::<i32>("bad", 'x'), (None, None));
    }
}
