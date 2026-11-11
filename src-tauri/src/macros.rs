//! The user's AutoHotkey v2 macros, run against RoALT's clients. AutoHotkey isn't bundled: RoALT
//! runs the one the user installed (`find_ahk`). Each run is one AutoHotkey process per client,
//! fed the script on stdin with the client's window id and pid as its only arguments (never a
//! cookie or anything else of the account), inside a kill-on-close job: Stop, Kill all, the
//! client exiting, RoALT exiting or dying all end it. Only trusted macros run (`may_run`): typed
//! in RoALT, or imported and then reviewed. One macro per client at a time.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, MutexGuard};

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::App;
use crate::commands::{notice, now_ms};
use crate::error::{AppError, Result};
#[cfg(windows)]
use crate::instances::limits::{Job, kill_on_close};
use crate::model::{InstanceState, Limits, Macro, MacroRun};

/// AutoHotkey is a Windows program: Linux has no run to put in a job. `find_ahk` finds none, so
/// a run never starts; this keeps the type and the call sites compiling.
#[cfg(not(windows))]
struct Job;

#[cfg(not(windows))]
fn kill_on_close(_child: &Child) -> Result<Job> {
    Ok(Job)
}

/// How much of a run's stderr is kept for its error toast and the log.
const TAIL: usize = 4 << 10;
/// AutoHotkey's working folder, in the data folder.
const DIR: &str = "macros";

struct Run {
    info: MacroRun,
    client_pid: u32,
    ahk_pid: u32,
    /// Dropping it ends the AutoHotkey process.
    _job: Job,
}

/// The running macros, by instance id, and the macro hotkeys registered with Windows.
pub struct Runner {
    runs: Mutex<HashMap<String, Run>>,
    hotkeys: Mutex<Vec<(Shortcut, i64)>>,
}

impl Runner {
    pub fn new() -> Runner {
        Runner { runs: Mutex::new(HashMap::new()), hotkeys: Mutex::new(Vec::new()) }
    }

    fn runs(&self) -> MutexGuard<'_, HashMap<String, Run>> {
        self.runs.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn hotkeys(&self) -> MutexGuard<'_, Vec<(Shortcut, i64)>> {
        self.hotkeys.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Oldest first.
    pub fn list(&self) -> Vec<MacroRun> {
        let mut out: Vec<MacroRun> = self.runs().values().map(|r| r.info.clone()).collect();
        out.sort_by_key(|r| r.started_at);
        out
    }

    /// Instance ids of the clients running a macro (anti-AFK leaves them alone).
    pub fn busy(&self) -> Vec<String> {
        self.runs().keys().cloned().collect()
    }

    /// Ends the run on this client, if any (its job closes).
    pub fn stop(&self, instance_id: &str) {
        if self.runs().remove(instance_id).is_some() {
            log::info!("macro stopped on instance {instance_id}");
        }
    }

    /// Stops macro `id` everywhere; false when it wasn't running.
    pub fn stop_macro(&self, id: i64) -> bool {
        let mut runs = self.runs();
        let before = runs.len();
        runs.retain(|_, r| r.info.macro_id != id);
        before != runs.len()
    }

    /// Stop all, Kill all, the panic hotkey, RoALT exiting.
    pub fn stop_all(&self) {
        let mut runs = self.runs();
        if !runs.is_empty() {
            log::info!("stopping {} macro run(s)", runs.len());
        }
        runs.clear();
    }

    /// Runs whose client is gone (or came back under another pid) stop.
    pub fn sync(&self, clients: &[(String, u32, u64, Limits)]) {
        self.runs().retain(|id, r| clients.iter().any(|c| c.0 == *id && c.1 == r.client_pid));
    }

    /// The run of AutoHotkey `ahk_pid` on `instance_id` ended: forgotten. True when it was still
    /// listed, so it ended by itself rather than being stopped.
    fn ended(&self, instance_id: &str, ahk_pid: u32) -> bool {
        let mut runs = self.runs();
        let mine = runs.get(instance_id).is_some_and(|r| r.ahk_pid == ahk_pid);
        if mine {
            runs.remove(instance_id);
        }
        mine
    }

    /// Starts `m` with AutoHotkey `ahk` on the client `client_pid` of instance `instance_id`.
    fn start(&self, app: &AppHandle, ahk: &Path, dir: &Path, m: &Macro, instance_id: &str, client_pid: u32) -> Result<()> {
        let Some((hwnd, _)) = crate::instances::process::main_window(client_pid) else { return Err(not_in_game()) };
        #[cfg(windows)]
        let hwnd = hwnd.0 as usize;
        #[cfg(not(windows))]
        let hwnd = hwnd as usize;
        let mut runs = self.runs();
        if runs.contains_key(instance_id) {
            return Err(AppError::Invalid(tr!("That client is already running a macro. Stop it first.")));
        }
        std::fs::create_dir_all(dir)?;
        // `*`: the script comes on stdin, which AutoHotkey reads to the end before it runs any of it,
        // so the job below is in place first.
        // shortcut: `*` and the directives appended below are unconfirmed on a real AutoHotkey v2; run one before release.
        let mut child = Command::new(ahk)
            .args(["/ErrorStdOut=UTF-8", "/CP65001", "*", &format!("{:#x}", hwnd), &client_pid.to_string()])
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| AppError::Invalid(tr!("AutoHotkey didn't start: {why}", why = e)))?;
        let job = kill_on_close(&child).inspect_err(|_| {
            let _ = child.kill();
        })?;
        let ahk_pid = child.id();
        let info = MacroRun { instance_id: instance_id.to_owned(), macro_id: m.id, name: m.name.clone(), started_at: now_ms() };
        runs.insert(instance_id.to_owned(), Run { info, client_pid, ahk_pid, _job: job });
        drop(runs);
        log::info!("macro {} started on instance {instance_id} (AutoHotkey pid {ahk_pid})", m.id);
        // At the end, so the script's own line numbers stay right in its errors.
        let script = format!("{}\n#SingleInstance Off\n#NoTrayIcon\n", m.body);
        let (handle, id, name) = (app.clone(), instance_id.to_owned(), m.name.clone());
        std::thread::spawn(move || supervise(&handle, child, &script, &id, ahk_pid, &name));
        emit_runs(app);
        Ok(())
    }
}

impl Default for Runner {
    fn default() -> Self {
        Self::new()
    }
}

/// Feeds a run its script, keeps the last `TAIL` of its stderr and waits for it. A run that ended
/// by itself with an error is a toast with the first line of what it said.
fn supervise(app: &AppHandle, mut child: Child, script: &str, instance_id: &str, ahk_pid: u32, name: &str) {
    if let Some(mut stdin) = child.stdin.take()
        && let Err(e) = stdin.write_all(script.as_bytes())
    {
        log::debug!("macro on instance {instance_id}: script not written: {e}");
    }
    // On its own thread: a process the script started can inherit stderr and keep it open after
    // AutoHotkey exits, until the job (dropped by `ended`) ends it.
    let (tx, rx) = std::sync::mpsc::channel();
    if let Some(mut err) = child.stderr.take() {
        std::thread::spawn(move || {
            let mut tail = Vec::new();
            let mut buf = [0u8; 1024];
            while let Ok(n @ 1..) = err.read(&mut buf) {
                tail.extend_from_slice(&buf[..n]);
                tail.drain(..tail.len().saturating_sub(TAIL));
            }
            let _ = tx.send(tail);
        });
    }
    let status = child.wait();
    let s = app.state::<App>();
    let by_itself = s.macros.ended(instance_id, ahk_pid);
    emit_runs(app);
    let failed = !status.as_ref().is_ok_and(|st| st.success());
    log::info!("macro on instance {instance_id} ended ({status:?}){}", if by_itself { "" } else { ", stopped" });
    if !(by_itself && failed) {
        return;
    }
    let tail = rx.recv_timeout(std::time::Duration::from_millis(500)).unwrap_or_default();
    let said = String::from_utf8_lossy(&tail);
    if !said.trim().is_empty() {
        log::warn!("macro {name} on instance {instance_id} said: {said}");
    }
    let message = match said.lines().map(str::trim).find(|l| !l.is_empty()) {
        Some(line) => tr!("Macro {name} stopped with an error: {why}", name = name, why = line.chars().take(300).collect::<String>()),
        None => tr!("Macro {name} stopped with an error.", name = name),
    };
    notice(app, true, message);
}

fn emit_runs(app: &AppHandle) {
    let _ = app.emit("macros://runs", app.state::<App>().macros.list());
}

fn not_in_game() -> AppError {
    AppError::Invalid(tr!("That client isn't in a game."))
}

/// Locked: no new runs (running ones go on). Untrusted: never, until reviewed.
pub fn may_run(m: &Macro, locked: bool) -> Result<()> {
    if locked {
        return Err(AppError::Locked);
    }
    if !m.trusted {
        return Err(AppError::Invalid(tr!("Review {name} on the Macros page before it runs.", name = m.name)));
    }
    Ok(())
}

/// AutoHotkey v2's AutoHotkey64.exe: `custom` (Settings), else where its installer put it (the
/// registry's InstallDir, then Program Files, then the per-user Programs folder).
#[cfg(windows)]
pub fn find_ahk(custom: &str) -> Option<PathBuf> {
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    let reg = |root, key| crate::launcher::client::reg_string(root, key, Some("InstallDir")).map(PathBuf::from);
    let env = |var, sub| std::env::var_os(var).map(|d| PathBuf::from(d).join(sub));
    let roots = [
        reg(HKEY_CURRENT_USER, r"SOFTWARE\AutoHotkey"),
        reg(HKEY_LOCAL_MACHINE, r"SOFTWARE\AutoHotkey"),
        reg(HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\AutoHotkey"),
        env("ProgramFiles", "AutoHotkey"),
        env("LOCALAPPDATA", r"Programs\AutoHotkey"),
    ];
    find_in(custom, roots.into_iter().flatten())
}

/// AutoHotkey is a Windows program (it runs under Wine at best): Linux finds none, so the Macros
/// page reports it as not installed and no run starts here.
#[cfg(not(windows))]
pub fn find_ahk(_custom: &str) -> Option<PathBuf> {
    None
}

/// The first file of `custom`, then `<root>\v2\AutoHotkey64.exe` and `<root>\AutoHotkey64.exe`
/// of each root.
#[cfg_attr(not(windows), allow(dead_code))]
fn find_in(custom: &str, roots: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    let custom = Some(PathBuf::from(custom.trim())).filter(|p| !p.as_os_str().is_empty());
    custom.into_iter().chain(roots.into_iter().flat_map(|r| [r.join(r"v2\AutoHotkey64.exe"), r.join("AutoHotkey64.exe")])).find(|p| p.is_file())
}

fn ahk(s: &App) -> Result<PathBuf> {
    find_ahk(&s.db.settings()?.ahk_path).ok_or_else(|| AppError::Invalid(tr!("Install AutoHotkey v2 to use macros.")))
}

/// A macro file's text: UTF-8 with or without a BOM, or UTF-16 with its BOM. Anything else, or
/// text with NULs (UTF-16 without a BOM), is refused.
pub fn decode(bytes: &[u8]) -> Result<String> {
    let utf16 = |b: &[u8], be: bool| {
        if !b.len().is_multiple_of(2) {
            return None;
        }
        let units: Vec<u16> = b.as_chunks::<2>().0.iter().map(|&c| if be { u16::from_be_bytes(c) } else { u16::from_le_bytes(c) }).collect();
        String::from_utf16(&units).ok()
    };
    let text = match bytes {
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8(rest.to_vec()).ok(),
        [0xFF, 0xFE, rest @ ..] => utf16(rest, false),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, true),
        _ => String::from_utf8(bytes.to_vec()).ok(),
    };
    let text = text.filter(|t| !t.contains('\0')).ok_or_else(|| AppError::Invalid(tr!("RoALT can't read that file as text. Save it as UTF-8 and import it again.")))?;
    check_text(&text)?;
    Ok(text)
}

/// Refuses source with bidi controls or invisible characters (a leading BOM aside): the review
/// must show exactly what runs. A zero-width joiner between two emoji (👨‍💻) is fine.
pub fn check_text(body: &str) -> Result<()> {
    let hidden = |c: char| {
        matches!(c,
            '\u{00AD}' | '\u{034F}' | '\u{061C}' | '\u{115F}'..='\u{1160}' | '\u{17B4}'..='\u{17B5}' | '\u{180B}'..='\u{180F}'
            | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206F}'
            | '\u{3164}' | '\u{FEFF}' | '\u{FFA0}' | '\u{FFF9}'..='\u{FFFB}' | '\u{E0000}'..='\u{E007F}')
    };
    let emoji = |c: Option<char>| c.is_some_and(|c| matches!(c, '\u{2600}'..='\u{27BF}' | '\u{FE0F}' | '\u{1F000}'..='\u{10FFFF}'));
    let chars: Vec<char> = body.strip_prefix('\u{FEFF}').unwrap_or(body).chars().collect();
    let joins_emoji = |i: usize| chars[i] == '\u{200D}' && emoji(i.checked_sub(1).map(|j| chars[j])) && emoji(chars.get(i + 1).copied());
    if (0..chars.len()).any(|i| hidden(chars[i]) && !joins_emoji(i)) {
        return Err(AppError::Invalid(tr!("This macro has invisible or text-direction characters. Remove them first.")));
    }
    Ok(())
}

/// A macro file's name (without extension) and text; its size is checked before it is read.
pub fn read_file(path: &Path) -> Result<(String, String)> {
    // UTF-16 takes up to twice the bytes; the decoded text is held to the limit when saved.
    if std::fs::metadata(path)?.len() > 2 * crate::storage::MAX_MACRO as u64 {
        return Err(AppError::Invalid(tr!("A macro can be at most {max} KiB.", max = crate::storage::MAX_MACRO >> 10)));
    }
    let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Ok((name, decode(&std::fs::read(path)?)?))
}

/// `key` ("" for none) as a macro hotkey: a key combination that isn't the panic hotkey or
/// another macro's (`id`'s own is fine).
pub fn check_hotkey(key: &str, id: Option<i64>, macros: &[Macro], panic: &str) -> Result<()> {
    if key.is_empty() {
        return Ok(());
    }
    let k = crate::commands::parse_hotkey(key)?;
    let same = |other: &str| other.parse::<Shortcut>().is_ok_and(|o| o == k);
    if same(panic) {
        return Err(AppError::Invalid(tr!("{key} is already the panic hotkey.", key = key)));
    }
    match macros.iter().find(|m| Some(m.id) != id && same(&m.hotkey)) {
        Some(m) => Err(AppError::Invalid(tr!("{key} is already the hotkey of {name}.", key = key, name = m.name))),
        None => Ok(()),
    }
}

/// Windows gives RoALT `key` ("" for none), unless it is already one of RoALT's macro hotkeys.
pub fn can_register(app: &AppHandle, key: &str) -> Result<()> {
    let Ok(k) = key.parse::<Shortcut>() else { return Ok(()) };
    if app.state::<App>().macros.hotkeys().iter().any(|(h, _)| *h == k) {
        return Ok(());
    }
    let gs = app.global_shortcut();
    gs.register(k).map_err(|e| {
        log::warn!("could not register the macro hotkey {key}: {e}");
        AppError::Invalid(tr!("Windows wouldn't give RoALT {key}: another app probably uses it. Pick another.", key = key))
    })?;
    if let Err(e) = gs.unregister(k) {
        log::warn!("could not unregister the macro hotkey {key} after trying it: {e}");
    }
    Ok(())
}

/// Registers every trusted macro's hotkey anew (at start, after a save, a delete, a trust or a
/// restore). One Windows won't give is logged and left out.
pub fn sync_hotkeys(app: &AppHandle) {
    let s = app.state::<App>();
    let gs = app.global_shortcut();
    // Not held while Windows is asked: a key pressed meanwhile looks the list up (`hotkey`).
    let old = std::mem::take(&mut *s.macros.hotkeys());
    for (k, id) in old {
        if let Err(e) = gs.unregister(k) {
            log::warn!("could not unregister the hotkey of macro {id}: {e}");
        }
    }
    let macros = match s.db.macros() {
        Ok(m) => m,
        Err(e) => return log::warn!("no macro hotkeys: could not read the macros: {e}"),
    };
    let mut held = Vec::new();
    for m in macros.iter().filter(|m| m.trusted && !m.hotkey.is_empty()) {
        match m.hotkey.parse::<Shortcut>().map(|k| (k, gs.register(k))) {
            Ok((k, Ok(()))) => held.push((k, m.id)),
            Ok((_, Err(e))) => log::warn!("could not register the hotkey {} of macro {}: {e}", m.hotkey, m.id),
            Err(_) => log::warn!("macro {} has no usable hotkey: {:?}", m.id, m.hotkey),
        }
    }
    *s.macros.hotkeys() = held;
}

/// What a pressed global shortcut is.
#[derive(Debug, PartialEq)]
pub enum Pressed {
    Macro(i64),
    Panic,
    /// Neither: a macro's key pressed while the hotkeys re-sync.
    Ignore,
}

/// `pressed` among the macro hotkeys, then the panic hotkey `panic`.
pub fn classify(pressed: &Shortcut, macros: &[(Shortcut, i64)], panic: &str) -> Pressed {
    if let Some(&(_, id)) = macros.iter().find(|(k, _)| k == pressed) {
        return Pressed::Macro(id);
    }
    if panic.parse::<Shortcut>().is_ok_and(|p| p == *pressed) { Pressed::Panic } else { Pressed::Ignore }
}

/// A pressed global shortcut: a macro's is toggled off this thread, the panic hotkey kills all.
/// When the settings can't be read, an unmatched key is taken as the panic hotkey.
pub fn hotkey(app: &AppHandle, key: &Shortcut) {
    let s = app.state::<App>();
    let panic = s.db.settings().map(|st| st.panic_hotkey);
    let what = match &panic {
        Ok(p) => classify(key, &s.macros.hotkeys(), p),
        Err(_) => match classify(key, &s.macros.hotkeys(), "") {
            Pressed::Ignore => Pressed::Panic,
            m => m,
        },
    };
    match what {
        Pressed::Macro(id) => {
            let app = app.clone();
            tauri::async_runtime::spawn_blocking(move || toggle(&app, id));
        }
        Pressed::Panic => crate::tray::kill_all(app),
        Pressed::Ignore => log::debug!("a hotkey that is no longer RoALT's was pressed; ignored"),
    }
}

/// Running anywhere: stopped everywhere. Else started on every client in a game, but not on Full
/// quality accounts or clients already running a macro.
fn toggle(app: &AppHandle, id: i64) {
    let s = app.state::<App>();
    if s.macros.stop_macro(id) {
        return;
    }
    let m = match s.db.macro_by_id(id) {
        Ok(m) => m,
        Err(e) => return log::warn!("macro hotkey: could not read macro {id}: {e}"),
    };
    if let Err(e) = may_run(&m, s.vault.is_locked()) {
        return notice(app, true, e.message());
    }
    let full = s.db.full_quality_accounts().unwrap_or_default();
    let busy = s.macros.busy();
    let targets: Vec<(String, u32)> = in_game(&s)
        .into_iter()
        .filter(|(id, _, account)| !full.contains(account) && !busy.contains(id))
        .map(|(id, pid, _)| (id, pid))
        .collect();
    match start_on(app, &s, &m, &targets) {
        (0, Some(e)) => notice(app, true, e.message()),
        (0, None) => notice(app, true, tr!("No client in a game can run {name} now.", name = m.name)),
        _ => {}
    }
}

/// Clients in a game: (instance id, client pid, account id).
fn in_game(s: &App) -> Vec<(String, u32, i64)> {
    s.tracker
        .clients()
        .into_iter()
        .filter_map(|(id, pid, ..)| s.tracker.get(&id).filter(|i| i.state == InstanceState::InGame).map(|i| (id, pid, i.account_id)))
        .collect()
}

/// Starts `m` on each (instance id, client pid): how many started, and the first refusal.
fn start_on(app: &AppHandle, s: &App, m: &Macro, targets: &[(String, u32)]) -> (usize, Option<AppError>) {
    let ahk = match ahk(s) {
        Ok(p) => p,
        Err(e) => return (0, Some(e)),
    };
    let dir = s.data_dir.join(DIR);
    let mut started = 0;
    let mut first = None;
    for (id, pid) in targets {
        match s.macros.start(app, &ahk, &dir, m, id, *pid) {
            Ok(()) => started += 1,
            Err(e) => {
                log::info!("macro {} not started on instance {id}: {e}", m.id);
                first.get_or_insert(e);
            }
        }
    }
    (started, first)
}

/// `macro_run`: macro `id` on these clients (each must be in a game). Err when any didn't start.
pub fn run(app: &AppHandle, s: &App, id: i64, instance_ids: &[String]) -> Result<()> {
    let m = s.db.macro_by_id(id)?;
    may_run(&m, s.vault.is_locked())?;
    let live = in_game(s);
    let targets = instance_ids
        .iter()
        .map(|i| live.iter().find(|c| c.0 == *i).map(|c| (c.0.clone(), c.1)).ok_or_else(not_in_game))
        .collect::<Result<Vec<_>>>()?;
    match start_on(app, s, &m, &targets) {
        (_, Some(e)) => Err(e),
        _ => Ok(()),
    }
}

/// A client of a non-Full-quality account reached a game with auto-run on: the first trusted
/// "Run on launch" macro by name starts on it. Not while the vault is locked.
pub fn auto_run(app: &AppHandle, s: &App, instance_id: &str, pid: u32) {
    if s.vault.is_locked() {
        return log::info!("auto-run: the vault is locked; no macro on instance {instance_id}");
    }
    let m = match s.db.macros() {
        Ok(list) => list.into_iter().find(|m| m.run_on_launch && m.trusted),
        Err(e) => return log::warn!("auto-run: could not read the macros: {e}"),
    };
    if let Some(m) = m
        && let (0, Some(e)) = start_on(app, s, &m, &[(instance_id.to_owned(), pid)])
    {
        notice(app, true, tr!("Macro {name} didn't start: {why}", name = m.name, why = e.message()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn macro_with(id: i64, hotkey: &str, trusted: bool) -> Macro {
        Macro { id, name: format!("m{id}"), body: String::new(), trusted, hotkey: hotkey.into(), run_on_launch: false, created_at: 0, updated_at: 0 }
    }

    #[test]
    fn decodes_utf8_and_utf16_by_bom_only() {
        let utf16 = |be: bool, bom: bool| {
            let mut out: Vec<u8> = if bom { if be { vec![0xFE, 0xFF] } else { vec![0xFF, 0xFE] } } else { Vec::new() };
            for u in "Send 'ä'".encode_utf16() {
                out.extend(if be { u.to_be_bytes() } else { u.to_le_bytes() });
            }
            out
        };
        assert_eq!(decode("Send 'ä'".as_bytes()).unwrap(), "Send 'ä'");
        assert_eq!(decode(b"\xEF\xBB\xBFSend 'x'").unwrap(), "Send 'x'");
        assert_eq!(decode(&utf16(false, true)).unwrap(), "Send 'ä'");
        assert_eq!(decode(&utf16(true, true)).unwrap(), "Send 'ä'");
        assert_eq!(decode(b"").unwrap(), "");
        let mut odd = utf16(false, true);
        odd.push(b'x');
        let bad = [utf16(false, false), odd, b"Send\0'x'".to_vec(), b"\xFF\xFE\x00\xD8".to_vec(), b"caf\xE9".to_vec(), b"\xEF\xBB\xBF\xC3".to_vec()];
        for b in bad {
            assert!(matches!(decode(&b), Err(AppError::Invalid(_))), "{b:?}");
        }
    }

    #[test]
    fn finds_autohotkey_v2_first() {
        let tmp = std::env::temp_dir().join(format!("roalt-ahk-{}", uuid::Uuid::new_v4()));
        let touch = |p: &Path| {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
        };
        let (a, b) = (tmp.join("a"), tmp.join("b"));
        let roots = || [tmp.join("none"), a.clone(), b.clone()];
        assert_eq!(find_in("", roots()), None);
        touch(&b.join(r"v2\AutoHotkey64.exe"));
        touch(&a.join("AutoHotkey64.exe"));
        assert_eq!(find_in("", roots()), Some(a.join("AutoHotkey64.exe")), "the first root that has one");
        touch(&a.join(r"v2\AutoHotkey64.exe"));
        assert_eq!(find_in("", roots()), Some(a.join(r"v2\AutoHotkey64.exe")), "v2 before the root's own");
        let custom = tmp.join(r"mine\AHK.exe");
        assert_eq!(find_in(&custom.display().to_string(), roots()), Some(a.join(r"v2\AutoHotkey64.exe")), "a picked file that is gone");
        touch(&custom);
        assert_eq!(find_in(&format!(" {} ", custom.display()), roots()), Some(custom.clone()));
        // A folder named like it isn't it.
        std::fs::create_dir_all(tmp.join(r"c\v2\AutoHotkey64.exe")).unwrap();
        assert_eq!(find_in("", [tmp.join("c")]), None);
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn hotkeys_never_clash() {
        let macros = [macro_with(1, "Ctrl+Shift+F1", true), macro_with(2, "", true)];
        assert!(check_hotkey("", None, &macros, "Ctrl+Shift+F12").is_ok());
        assert!(check_hotkey("Ctrl+Shift+F2", None, &macros, "Ctrl+Shift+F12").is_ok());
        assert!(check_hotkey("shift+control+F1", Some(1), &macros, "").is_ok(), "its own");
        for (key, id) in [("control+shift+F1", None), ("Ctrl+Shift+F1", Some(2)), ("Ctrl+Shift+F12", Some(1)), ("Shift+Ctrl+F12", None), ("nonsense", None)] {
            assert!(matches!(check_hotkey(key, id, &macros, "Ctrl+Shift+F12"), Err(AppError::Invalid(_))), "{key}");
        }
    }

    #[test]
    fn refuses_hidden_and_bidi_characters() {
        assert!(check_text("Send 'ä'\n\tSleep 10").is_ok());
        assert!(check_text("\u{FEFF}Send 'x'").is_ok(), "a leading BOM");
        assert!(check_text("Send \"gg 👨\u{200D}💻 🏳\u{FE0F}\u{200D}🌈\"").is_ok(), "emoji joined by ZWJ");
        let hidden = ['\u{202A}', '\u{202E}', '\u{2066}', '\u{2069}', '\u{200B}', '\u{200D}', '\u{200F}', '\u{FEFF}', '\u{00AD}', '\u{061C}', '\u{2060}', '\u{3164}', '\u{E0041}'];
        for c in hidden {
            assert!(matches!(check_text(&format!("Send 'a{c}b'")), Err(AppError::Invalid(_))), "{:x}", c as u32);
        }
        assert!(matches!(check_text("\u{FEFF}\u{FEFF}x"), Err(AppError::Invalid(_))), "a second BOM");
        assert!(matches!(decode("x\u{202E}y".as_bytes()), Err(AppError::Invalid(_))), "on import too");
    }

    #[test]
    fn pressed_keys_are_a_macro_the_panic_key_or_nothing() {
        let k = |s: &str| s.parse::<Shortcut>().unwrap();
        let macros = [(k("Ctrl+Shift+F1"), 1), (k("Ctrl+Shift+F2"), 2)];
        assert_eq!(classify(&k("shift+control+F2"), &macros, "Ctrl+Shift+F12"), Pressed::Macro(2));
        assert_eq!(classify(&k("Ctrl+Shift+F12"), &macros, "Shift+Ctrl+F12"), Pressed::Panic);
        assert_eq!(classify(&k("Ctrl+Shift+F12"), &[], "Ctrl+Shift+F12"), Pressed::Panic);
        // A macro key mid re-sync, with the panic hotkey off or another key.
        assert_eq!(classify(&k("Ctrl+Shift+F1"), &[], ""), Pressed::Ignore);
        assert_eq!(classify(&k("Ctrl+Shift+F1"), &[], "Ctrl+Shift+F12"), Pressed::Ignore);
    }

    #[cfg(windows)]
    fn run(r: &Runner, instance_id: &str, client_pid: u32, ahk_pid: u32) {
        let info = MacroRun { instance_id: instance_id.into(), macro_id: 1, name: "m".into(), started_at: 0 };
        let job = crate::instances::limits::new_job().unwrap();
        r.runs().insert(instance_id.into(), Run { info, client_pid, ahk_pid, _job: job });
    }

    #[cfg(windows)]
    #[test]
    fn runs_end_only_for_their_own_process() {
        let r = Runner::new();
        run(&r, "a", 10, 100);
        assert!(!r.ended("a", 999), "a newer run's process");
        assert!(!r.ended("b", 100));
        assert_eq!(r.busy(), ["a"]);
        assert!(r.ended("a", 100));
        assert!(r.busy().is_empty());
        assert!(!r.ended("a", 100), "already stopped");
    }

    #[cfg(windows)]
    #[test]
    fn runs_stop_when_their_client_is_gone() {
        let r = Runner::new();
        run(&r, "a", 10, 100);
        run(&r, "b", 20, 200);
        run(&r, "c", 30, 300);
        // "b" came back under another pid, "c" is gone.
        r.sync(&[("a".into(), 10, 0, Limits::default()), ("b".into(), 21, 0, Limits::default())]);
        assert_eq!(r.busy(), ["a"]);
    }

    #[test]
    fn only_trusted_macros_run_and_not_while_locked() {
        assert!(may_run(&macro_with(1, "", true), false).is_ok());
        assert!(matches!(may_run(&macro_with(1, "", false), false), Err(AppError::Invalid(_))));
        assert!(matches!(may_run(&macro_with(1, "", true), true), Err(AppError::Locked)));
    }
}
