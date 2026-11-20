//! Roblox login / session windows on Linux (WebKitGTK). Cookie reading and writing go through
//! Tauri's webview cookie API; each window gets its own profile directory where the platform
//! supports it.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tauri::webview::Cookie;
use tauri::{AppHandle, Manager, Webview, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use zeroize::Zeroizing;

use crate::error::{AppError, Result};
use crate::roblox_api::endpoints;
use crate::vault::Secret;

const TMP_DIR: &str = "tmp-webview";
const LOGIN_DIR: &str = "webview-login";
const SESSIONS_DIR: &str = "webview-sessions";
/// In a kept profile while its window is open; still there at start-up → the profile may hold a
/// cookie or a sign-in left by a window that never closed normally.
const OPEN_MARK: &str = ".roalt-open";
const POLL: Duration = Duration::from_millis(700);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const HOME_PAGE: &str = "https://www.roblox.com/home";

static LOGIN_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Session windows from close request until their close path has finished with the profile.
static CLOSING: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// A sign-in to fill in for the user (a bulk import's `user:pass` line). Only ever put into the
/// login page's fields; never stored.
pub struct LoginFill {
    pub username: String,
    pub password: Secret,
}

/// What fills Roblox's login form (pure): on www.roblox.com's top frame only, once its fields
/// show (up to 20 s), each one still empty; the input events tell its React state. Never presses
/// Log In: the user does, after any captcha. Values are JSON-escaped.
fn fill_script(username: &str, password: &str) -> Zeroizing<String> {
    const BODY: &str = r#"if (location.hostname !== "www.roblox.com" || window.top !== window) return;
const set = (el, v) => {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(el, v);
  el.dispatchEvent(new Event("input", { bubbles: true }));
  el.dispatchEvent(new Event("change", { bubbles: true }));
};
let tries = 0;
const timer = setInterval(() => {
  const u = document.getElementById("login-username"), p = document.getElementById("login-password");
  if (u && p) {
    clearInterval(timer);
    if (!u.value) set(u, user);
    if (!p.value) set(p, pass);
    p.focus();
  } else if (++tries >= 100) clearInterval(timer);
}, 200);"#;
    let json = |s: &str| Zeroizing::new(serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into()));
    Zeroizing::new(format!("(() => {{\nconst user = {}, pass = {};\n{BODY}\n}})();", *json(username), *json(password)))
}

pub async fn capture_login(app: &AppHandle, data_dir: &Path, fill: Option<&LoginFill>) -> Result<Secret> {
    let slot = LoginSlot::take(app)?;
    let login = parse_url(endpoints::LOGIN_PAGE)?;
    let label = format!("login-{}", uuid::Uuid::new_v4());
    let profile = login_profile(data_dir);
    mark_open(&profile);
    // Blank first: the old cookie goes before anything loads from Roblox.
    let script = fill.map(|f| fill_script(&f.username, f.password.expose()));
    let mut builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(parse_url("about:blank")?))
        .title(tr!("Sign in to Roblox — RoALT"))
        .inner_size(480.0, 760.0)
        .center()
        .data_directory(profile.clone())
        .incognito(false)
        .devtools(cfg!(debug_assertions))
        .general_autofill_enabled(false)
        .on_navigation(allowed);
    if let Some(s) = &script {
        builder = builder.initialization_script(s.as_str());
    }
    let built = builder.build();
    let window = match built {
        Ok(w) => w,
        Err(e) => {
            mark_closed(&profile);
            return Err(e.into());
        }
    };
    // A Google / Apple sign-in left by an earlier window goes before the page loads.
    let cookie = if clear_auth_cookies(&window) {
        match window.navigate(login) {
            Ok(()) => wait_for_cookie(app.clone(), label.clone()).await,
            Err(e) => Err(e.into()),
        }
    } else {
        Err(AppError::Internal(tr!("Could not clear the previous Roblox sign-in. Try again.")))
    };
    // Keep the profile (a captcha solved in it stays solved), but only when its auth cookies are gone.
    let cleared = app.get_webview_window(&label).is_some() && clear_auth_cookies(&window);
    if cleared {
        mark_closed(&profile);
    }
    if let Err(e) = window.destroy() {
        log::debug!("could not destroy login window {label}: {e}");
    }
    drop(slot);
    cookie
}

/// Closes an open sign-in window: its `capture_login` ends `Cancelled` (a bulk import's Stop).
pub fn close_login(app: &AppHandle) {
    for (label, w) in app.webview_windows() {
        if label.starts_with("login-")
            && let Err(e) = w.close()
        {
            log::debug!("could not close login window {label}: {e}");
        }
    }
}

pub async fn open_session(
    app: &AppHandle,
    data_dir: &Path,
    account_id: i64,
    title: &str,
    cookie: Secret,
    on_close: impl FnOnce(Option<Secret>) + Send + 'static,
) -> Result<()> {
    let home = parse_url(HOME_PAGE)?;
    let label = format!("session-{account_id}");
    // Still closing: the profile isn't free yet.
    if CLOSING.lock().unwrap_or_else(|e| e.into_inner()).contains(&label) {
        return Err(AppError::Invalid(tr!("That Roblox window is still closing. Try again in a moment.")));
    }
    // One profile, one window: its cookie jar is shared.
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.unminimize();
        let _ = w.set_focus();
        return Ok(());
    }
    let profile = session_profile(data_dir, account_id);
    // Marked with no window open: left by a crash or a removal that failed (maybe of an earlier
    // account with this id), so nothing of it is reused.
    if profile.join(OPEN_MARK).exists() {
        let p = profile.clone();
        let _ = tokio::task::spawn_blocking(move || remove_profile(&p)).await;
    }
    mark_open(&profile);
    // Blank first, so nothing loads from Roblox before the cookie is in place.
    let built = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(parse_url("about:blank")?))
        .title(title)
        .inner_size(1180.0, 800.0)
        .center()
        .data_directory(profile.clone())
        .incognito(false)
        .devtools(cfg!(debug_assertions))
        .general_autofill_enabled(false)
        .on_navigation(allowed)
        .build();
    let window = match built {
        Ok(w) => w,
        Err(e) => {
            mark_closed(&profile);
            return Err(e.into());
        }
    };
    let c = Cookie::build((".ROBLOSECURITY", cookie.expose().to_owned()))
        .domain(".roblox.com")
        .path("/")
        .secure(true)
        .http_only(true)
        .same_site(tauri::webview::cookie::SameSite::Lax)
        .build();
    if let Err(e) = window.set_cookie(c) {
        if let Err(e) = window.destroy() {
            log::debug!("could not destroy session window {label}: {e}");
        }
        mark_closed(&profile);
        return Err(AppError::Internal(tr!("Could not set Roblox's cookie: {why}", why = e)));
    }
    let (app2, title2) = (app.clone(), title.to_owned());
    let pending = Mutex::new(Some(move || end_session(&app2, &label, &title2, &profile, on_close)));
    window.on_window_event(move |event| {
        // `cookies()` deadlocks in here: keep the window and finish on another thread.
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            if let Some(end) = take_once(&pending) {
                tauri::async_runtime::spawn_blocking(end);
            }
        }
    });
    window.navigate(home)?;
    Ok(())
}

/// Marks `profile` as in use (created if needed). Logged, not an error: the window still works.
fn mark_open(profile: &Path) {
    if let Err(e) = std::fs::create_dir_all(profile).and_then(|()| std::fs::write(profile.join(OPEN_MARK), b"")) {
        log::warn!("could not mark browser profile {} as open: {e}", profile.display());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(profile, std::fs::Permissions::from_mode(0o700)) {
            log::warn!("could not restrict browser profile {}: {e}", profile.display());
        }
    }
}

/// After a normal close, once the cookies were deleted.
fn mark_closed(profile: &Path) {
    match std::fs::remove_file(profile.join(OPEN_MARK)) {
        Err(e) if e.kind() != ErrorKind::NotFound => log::warn!("could not unmark browser profile {}: {e}", profile.display()),
        _ => {}
    }
}

/// Deletes the kept profiles a window never closed normally in (crash, quit with it open):
/// they may still hold a `.ROBLOSECURITY` or a Google / Apple sign-in. At start-up and exit.
pub fn sweep_dirty_profiles(data_dir: &Path) {
    for p in std::iter::once(login_profile(data_dir)).chain(profile_dirs(data_dir)) {
        if p.join(OPEN_MARK).exists() {
            log::info!("browser profile {} was left open: deleted", p.display());
            remove_profile(&p);
        }
    }
}

fn login_profile(data_dir: &Path) -> PathBuf {
    data_dir.join(LOGIN_DIR)
}

fn session_profile(data_dir: &Path, account_id: i64) -> PathBuf {
    data_dir.join(SESSIONS_DIR).join(account_id.to_string())
}

/// Deletes the sign-in profile and every session profile (blocking: removal retries), unless a
/// sign-in or session window is open, whose browser holds them: then nothing, and `false`.
pub fn reset_profiles(app: &AppHandle, data_dir: &Path) -> bool {
    if browsing(app) {
        return false;
    }
    let login_gone = remove_profile(&login_profile(data_dir));
    let mut sessions_gone = true;
    for p in profile_dirs(data_dir) {
        sessions_gone &= remove_profile(&p);
    }
    login_gone && sessions_gone
}

/// A sign-in or session window is open, or one is still closing: its browser holds a profile,
/// and a closing session may yet hand over a newer cookie.
pub fn browsing(app: &AppHandle) -> bool {
    let closing = !CLOSING.lock().unwrap_or_else(|e| e.into_inner()).is_empty();
    closing || LOGIN_ACTIVE.load(Ordering::Acquire) || app.webview_windows().keys().any(|l| l.starts_with("login-") || l.starts_with("session-"))
}

/// Waits (blocking, up to ~5 s) for session windows to finish closing: `end_session` stores the
/// last cookie each one had. Never on the main thread, which those closes need.
pub fn wait_sessions_closed(app: &AppHandle) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let open = || {
        !CLOSING.lock().unwrap_or_else(|e| e.into_inner()).is_empty() || app.webview_windows().keys().any(|l| l.starts_with("session-"))
    };
    while Instant::now() < deadline && open() {
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The folders in `webview-sessions` (none if it is missing).
fn profile_dirs(data_dir: &Path) -> impl Iterator<Item = PathBuf> {
    std::fs::read_dir(data_dir.join(SESSIONS_DIR)).into_iter().flatten().filter_map(|e| e.ok()).map(|e| e.path())
}

/// Deletes the session profiles of accounts that no longer exist (start-up, before any window),
/// so a later account can't get an old one's.
pub fn sweep_orphan_profiles(data_dir: &Path, account_ids: &[i64]) {
    for p in profile_dirs(data_dir) {
        let id = p.file_name().and_then(|n| n.to_str()).and_then(|n| n.parse::<i64>().ok());
        if !id.is_some_and(|id| account_ids.contains(&id)) {
            log::info!("browser profile {} has no account: deleted", p.display());
            remove_profile(&p);
        }
    }
}

/// Deletes a removed account's session profile (blocking; logged, never an error). Its window
/// was asked to close: waits up to ~5 s for it and its close path to go first.
pub fn remove_account_profile(app: &AppHandle, data_dir: &Path, account_id: i64) {
    let label = format!("session-{account_id}");
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline
        && (app.get_webview_window(&label).is_some() || CLOSING.lock().unwrap_or_else(|e| e.into_inner()).contains(&label))
    {
        std::thread::sleep(Duration::from_millis(200));
    }
    remove_profile(&session_profile(data_dir, account_id));
}

pub fn sweep_temp_profiles(data_dir: &Path) {
    let entries = match std::fs::read_dir(data_dir.join(TMP_DIR)) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return,
        Err(e) => {
            log::warn!("could not list old browser profiles: {e}");
            return;
        }
    };
    for path in entries.filter_map(|e| e.inspect_err(|e| log::debug!("could not read an old browser profile entry: {e}")).ok()).map(|e| e.path()) {
        let removed = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        if let Err(e) = removed {
            log::warn!("could not delete old browser profile {}: {e}", path.display());
        }
    }
}

pub fn clear_webview2_env() {}

struct LoginSlot;

impl LoginSlot {
    fn take(app: &AppHandle) -> Result<Self> {
        if !LOGIN_ACTIVE.swap(true, Ordering::AcqRel) {
            return Ok(Self);
        }
        if let Some(w) = app.webview_windows().into_iter().find_map(|(l, w)| l.starts_with("login-").then_some(w)) {
            let _ = w.unminimize();
            let _ = w.set_focus();
        }
        Err(AppError::Invalid(tr!("Finish the Roblox sign-in window that's already open.")))
    }
}

impl Drop for LoginSlot {
    fn drop(&mut self) {
        LOGIN_ACTIVE.store(false, Ordering::Release);
    }
}

pub fn log_process_failures(_window: &WebviewWindow) {}

/// Kept for the shared `set_main_shown`: WebKitGTK has no per-webview memory target to lower.
pub fn memory_target(_wv: &Webview, _low: bool) {}

fn parse_url(s: &str) -> Result<url::Url> {
    s.parse().map_err(|e: url::ParseError| AppError::Internal(e.to_string()))
}

fn take_once<T>(slot: &Mutex<Option<T>>) -> Option<T> {
    slot.lock().unwrap_or_else(|e| e.into_inner()).take()
}

fn end_session(app: &AppHandle, label: &str, title: &str, profile: &Path, on_close: impl FnOnce(Option<Secret>)) {
    CLOSING.lock().unwrap_or_else(|e| e.into_inner()).push(label.to_owned());
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.hide();
        let last = match window.cookies().map(pick_roblosecurity) {
            Ok(Some(c)) => Some(c),
            Ok(None) => {
                log::warn!("session window \"{title}\" closed signed out of Roblox; stored cookie not changed");
                None
            }
            Err(e) => {
                log::warn!("could not read the cookies of session window \"{title}\": {e}");
                None
            }
        };
        on_close(last);
        // The cookie is the vault's job: drop it from the kept profile, and keep the profile
        // (a captcha solved in it stays solved). If that fails, the mark stays and the next
        // start deletes the whole profile.
        if clear_auth_cookies(&window) {
            mark_closed(profile);
        }
        if let Err(e) = window.destroy() {
            log::debug!("could not destroy session window {label}: {e}");
        }
    }
    // No window here means its cookies couldn't be read or deleted: the mark stays, so the next
    // start deletes the whole profile (`sweep_dirty_profiles`) rather than trust it.
    CLOSING.lock().unwrap_or_else(|e| e.into_inner()).retain(|l| l != label);
}

/// Deletes every `.ROBLOSECURITY` and every Google / Apple sign-in cookie from a window's
/// profile; false when one couldn't be deleted (the profile is then swept the next start).
fn clear_auth_cookies(window: &WebviewWindow) -> bool {
    match window.cookies() {
        Ok(all) => {
            let mut ok = true;
            for c in all.into_iter().filter(|c| {
                c.domain().is_some_and(|d| (c.name() == ".ROBLOSECURITY" && is_roblox(d)) || is_sign_in_provider(d))
            }) {
                if let Err(e) = window.delete_cookie(c) {
                    log::warn!("could not delete an auth cookie from a window's profile: {e}");
                    ok = false;
                }
            }
            ok
        }
        Err(e) => {
            log::warn!("could not read a window's cookies to delete them: {e}");
            false
        }
    }
}

/// Where a Roblox window may go: Roblox over https, and the sign-in providers it hands off to.
fn allowed(url: &url::Url) -> bool {
    let ok = url.as_str() == "about:blank"
        || (url.scheme() == "https"
            && url.username().is_empty()
            && url.port().is_none()
            && url.host_str().is_some_and(|h| is_roblox(h) || is_sign_in_provider(h)));
    if !ok {
        log::debug!("Roblox window: navigation to another site refused");
    }
    ok
}

fn is_sign_in_provider(domain: &str) -> bool {
    let d = domain.trim_start_matches('.').to_ascii_lowercase();
    d == "google.com" || d.ends_with(".google.com") || d == "apple.com" || d.ends_with(".apple.com")
}

async fn wait_for_cookie(app: AppHandle, label: String) -> Result<Secret> {
    tauri::async_runtime::spawn_blocking(move || poll_cookie(&app, &label)).await?
}

fn poll_cookie(app: &AppHandle, label: &str) -> Result<Secret> {
    let deadline = Instant::now() + LOGIN_TIMEOUT;
    let mut failures = 0;
    while Instant::now() < deadline {
        let Some(window) = app.get_webview_window(label) else {
            return Err(AppError::Cancelled);
        };
        app.state::<crate::App>().vault.touch();
        match window.cookies() {
            Ok(all) => {
                failures = 0;
                if let Some(cookie) = pick_roblosecurity(all) {
                    return Ok(cookie);
                }
            }
            Err(e) => {
                failures += 1;
                log::debug!("login window {label}: reading cookies failed ({failures}/5): {e}");
                if failures == 5 {
                    return Err(e.into());
                }
            }
        }
        std::thread::sleep(POLL);
    }
    Err(AppError::Invalid(tr!("The Roblox sign-in window timed out after 15 minutes. Try again.")))
}

fn pick_roblosecurity(cookies: Vec<Cookie<'static>>) -> Option<Secret> {
    cookies
        .into_iter()
        .find(|c| c.name() == ".ROBLOSECURITY" && !c.value().is_empty() && c.domain().is_some_and(is_roblox))
        .map(|c| Secret::new(c.value()))
}

fn is_roblox(domain: &str) -> bool {
    let d = domain.trim_start_matches('.').to_ascii_lowercase();
    d == "roblox.com" || d.ends_with(".roblox.com")
}

/// Child processes can hold files a moment longer, so retry; a final failure is logged and
/// returns false (gone or never there: true). What is left is marked open, so the next sweep
/// tries again (the mark may have been among the files already deleted).
fn remove_profile(dir: &Path) -> bool {
    for attempt in 1..=5 {
        match std::fs::remove_dir_all(dir) {
            Ok(()) => return true,
            Err(e) if e.kind() == ErrorKind::NotFound => return true,
            Err(e) if attempt == 5 => log::warn!("could not delete browser profile {}: {e}", dir.display()),
            Err(_) => std::thread::sleep(Duration::from_millis(500)),
        }
    }
    if dir.is_dir() {
        let _ = std::fs::write(dir.join(OPEN_MARK), b"");
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cookie(name: &str, value: &str, domain: &str) -> Cookie<'static> {
        Cookie::build((name.to_owned(), value.to_owned())).domain(domain.to_owned()).build()
    }

    fn picked(cookies: Vec<Cookie<'static>>) -> Option<String> {
        pick_roblosecurity(cookies).map(|s| s.expose().to_owned())
    }

    #[test]
    fn picks_only_a_non_empty_roblox_cookie() {
        let jar = vec![
            cookie(".ROBLOSECURITY", "fake", "evil.com"),
            cookie(".ROBLOSECURITY", "", ".roblox.com"),
            cookie(".ROBLOSECURITY", "good", ".roblox.com"),
        ];
        assert_eq!(picked(jar), Some("good".into()));
        assert_eq!(picked(vec![cookie(".ROBLOSECURITY", "x", "roblox.com.evil.com")]), None);
        assert_eq!(picked(vec![Cookie::new(".ROBLOSECURITY", "no-domain")]), None);
    }

    #[test]
    fn close_path_runs_once() {
        let pending = Mutex::new(Some(7));
        assert_eq!(take_once(&pending), Some(7));
        assert_eq!(take_once(&pending), None);
    }
}
