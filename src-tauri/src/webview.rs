//! Roblox login / session windows on kept WebView2 profiles: one for signing in, one per
//! account for its session window. A fresh profile each time looked like a never-seen browser
//! to Roblox and drew a captcha (403) on most sign-ins. `.ROBLOSECURITY` (the vault holds it)
//! and Google / Apple sign-ins are deleted when a window opens and again before it closes. A
//! window that never closed normally (crash, quit with it open) leaves an `OPEN_MARK` behind:
//! that profile is deleted whole at exit or the next start (`sweep_dirty_profiles`).

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tauri::webview::{Cookie, PlatformWebview};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use url::Url;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_COOKIE_SAME_SITE_KIND_LAX, ICoreWebView2, ICoreWebView2_2, ICoreWebView2CookieList, ICoreWebView2CookieManager,
    ICoreWebView2Settings4,
};
use windows::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject};
use windows::core::{Interface, PCWSTR, w};
use zeroize::Zeroizing;

use crate::error::{AppError, Result};
use crate::roblox_api::endpoints;
use crate::vault::Secret;

/// Where earlier versions put their throw-away profiles (still swept at start-up).
const TMP_DIR: &str = "tmp-webview";
const LOGIN_DIR: &str = "webview-login";
const SESSIONS_DIR: &str = "webview-sessions";
/// In a profile while its window is open; still there at start-up → the profile may hold a cookie.
const OPEN_MARK: &str = ".roalt-open";
const POLL: Duration = Duration::from_millis(700);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const HOME_PAGE: &str = "https://www.roblox.com/home";

/// Variables WebView2 reads before (and instead of) the options Tauri passes it.
const WEBVIEW2_VARS: [&str; 6] = [
    "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER",
    "WEBVIEW2_USER_DATA_FOLDER",
    "WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS",
    "WEBVIEW2_RELEASE_CHANNELS",
    "WEBVIEW2_CHANNEL_SEARCH_KIND",
    "WEBVIEW2_RELEASE_CHANNEL_PREFERENCE",
];

static LOGIN_ACTIVE: AtomicBool = AtomicBool::new(false);
/// Session windows from close request until their browser has exited: the kept profile is still
/// held, so the account's next window must wait.
static CLOSING: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Opens `https://www.roblox.com/login` in a `login-<uuid>` window on the kept sign-in profile
/// `<data_dir>/webview-login` (no devtools, no autofill, no context menu, no IPC capability),
/// with any `.ROBLOSECURITY` deleted before the page loads, so the last account's cookie can't
/// be taken for this one. A worker thread polls `cookies()` every ~700 ms until a non-empty
/// `.ROBLOSECURITY` appears, deletes it from the profile, closes the window and waits for its
/// browser process to exit. Closing the window first → `AppError::Cancelled`.
pub async fn capture_login(app: &AppHandle, data_dir: &Path) -> Result<Secret> {
    let slot = LoginSlot::take(app)?;
    let login = parse_url(endpoints::LOGIN_PAGE)?;
    let label = format!("login-{}", uuid::Uuid::new_v4());
    let profile = login_profile(data_dir);
    mark_open(&profile);
    // Blank first: the old cookie goes before anything loads from Roblox.
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(parse_url("about:blank")?))
        .title(tr!("Sign in to Roblox — RoALT"))
        .inner_size(480.0, 760.0)
        .center()
        .data_directory(profile.clone())
        .incognito(false)
        .devtools(cfg!(debug_assertions))
        .general_autofill_enabled(false)
        .on_navigation(allowed)
        .build()?;
    let (pid, cookie) = match harden(&window, None).await {
        // A Google / Apple sign-in left by an earlier window goes before the page loads.
        Ok(pid) => match (clear_cookies_async(&window, false).await, window.navigate(login)) {
            (_, Ok(())) => (Some(pid), wait_for_cookie(app.clone(), label.clone()).await),
            (_, Err(e)) => (Some(pid), Err(e.into())),
        },
        Err(e) => (None, Err(e)),
    };
    // Closed by the user: its cookies (a half-done Google sign-in too) can't be deleted any more, so
    // the whole profile goes once its browser has exited (kept marked if that fails: next start).
    let cleared = app.get_webview_window(&label).is_some() && clear_cookies_async(&window, true).await;
    if cleared {
        mark_closed(&profile);
    }
    if let Err(e) = window.destroy() {
        log::debug!("could not destroy login window {label}: {e}");
    }
    let _ = tauri::async_runtime::spawn_blocking(move || {
        if let Some(pid) = pid {
            wait_for_exit(pid);
        }
        if !cleared {
            remove_profile(&profile);
        }
    })
    .await;
    // Only now: the next sign-in shares this profile, which the old browser held until it exited.
    drop(slot);
    cookie
}

/// Marks `profile` as in use (created if needed). Logged, not an error: the window still works.
fn mark_open(profile: &Path) {
    if let Err(e) = std::fs::create_dir_all(profile).and_then(|()| std::fs::write(profile.join(OPEN_MARK), b"")) {
        log::warn!("could not mark browser profile {} as open: {e}", profile.display());
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
/// Only browser state goes (captchas may come back); the cookies are in the vault.
pub fn reset_profiles(app: &AppHandle, data_dir: &Path) -> bool {
    if browsing(app) {
        return false;
    }
    // All tried, even when one fails (a browser that is still exiting holds its folder). One by
    // one, so a profile that stays behind keeps its mark.
    let login = remove_profile(&login_profile(data_dir));
    let sessions = profile_dirs(data_dir).fold(true, |ok, p| remove_profile(&p) & ok);
    login && sessions
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
/// was asked to close: waits up to ~5 s for it and its browser to go first.
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

/// Opens `https://www.roblox.com/home` signed in as a stored account: a `session-<account_id>`
/// window titled `title` on the account's kept profile `<data_dir>/webview-sessions/<id>` (no
/// devtools in release, no IPC capability, same hardening as login). Already open → focused.
/// The cookie is injected through WebView2's own `ICoreWebView2CookieManager` with domain
/// `.roblox.com`, path `/`, HttpOnly, Secure, SameSite=Lax (never Tauri's `set_cookie`, which
/// drops the leading dot), before the first navigation. Returns once the window shows.
/// When the user closes it, `on_close` gets the profile's last non-empty `.ROBLOSECURITY`
/// (Roblox may have rotated it, or the user may have signed out / switched accounts), read
/// before the window is destroyed; then the cookie is deleted from the profile.
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
    // Still closing (its browser may outlive the window by seconds): the profile isn't free yet.
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
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(parse_url("about:blank")?))
        .title(title)
        .inner_size(1180.0, 800.0)
        .center()
        .data_directory(profile.clone())
        .incognito(false)
        .devtools(cfg!(debug_assertions))
        .general_autofill_enabled(false)
        .on_navigation(allowed)
        .build()?;
    let pid = match harden(&window, Some(cookie)).await {
        Ok(pid) => pid,
        Err(e) => {
            if let Err(e) = window.destroy() {
                log::debug!("could not destroy session window {label}: {e}");
            }
            return Err(e);
        }
    };
    // ponytail: a close in the instant before this handler exists skips on_close; the profile
    // keeps its OPEN_MARK and is deleted at the next start.
    let (app, title) = (app.clone(), title.to_owned());
    let pending = Mutex::new(Some(move || end_session(&app, &label, &title, pid, &profile, on_close)));
    window.on_window_event(move |event| {
        // `cookies()` deadlocks in here: keep the window and finish on another thread.
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            if let Some(end) = take_once(&pending) {
                tauri::async_runtime::spawn_blocking(end);
            }
        }
    });
    // A Google / Apple sign-in left in this profile goes before the page loads (the account's own
    // cookie, just put in, stays). After the close handler: this can take seconds.
    clear_cookies_async(&window, false).await;
    window.navigate(home)?;
    Ok(())
}

/// Deletes leftover `tmp-webview/*` profiles from earlier runs.
pub fn sweep_temp_profiles(data_dir: &Path) {
    let entries = match std::fs::read_dir(data_dir.join(TMP_DIR)) {
        Ok(entries) => entries,
        Err(e) if e.kind() == ErrorKind::NotFound => return,
        Err(e) => {
            log::warn!("could not list old browser profiles: {e}");
            return;
        }
    };
    let entries = entries.filter_map(|e| e.inspect_err(|e| log::debug!("could not read an old browser profile entry: {e}")).ok());
    for path in entries.map(|e| e.path()) {
        let removed = if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        if let Err(e) = removed {
            log::warn!("could not delete old browser profile {}: {e}", path.display());
        }
    }
}

/// Removes `WEBVIEW2_*` override variables so they cannot redirect or merge profiles.
/// Called first thing in `main`, before any thread starts.
pub fn clear_webview2_env() {
    for name in WEBVIEW2_VARS {
        // SAFETY: runs first thing in `main`, before any other thread exists, so nothing can
        // read the environment at the same time.
        unsafe { std::env::remove_var(name) };
    }
}

/// One login window at a time (a double click must not open two).
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

/// Turns off what Tauri does not expose, adds `cookie` as `.ROBLOSECURITY` if given, and
/// returns the profile's browser process id.
async fn harden(window: &WebviewWindow, cookie: Option<Secret>) -> Result<u32> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    window.with_webview(move |pv| {
        let r = harden_webview(&pv, cookie.as_ref()).map_err(|e| AppError::Internal(tr!("WebView2 setup failed: {why}", why = e)));
        let _ = tx.send(r);
    })?;
    // The closure is dropped unrun if the window is already gone.
    rx.await.map_err(|_| {
        log::debug!("webview setup skipped: window {} already gone", window.label());
        AppError::Cancelled
    })?
}

fn harden_webview(pv: &PlatformWebview, cookie: Option<&Secret>) -> windows::core::Result<u32> {
    // SAFETY: COM calls on the main thread (with_webview), on live interfaces from WebView2;
    // `value` outlives the call that reads it.
    unsafe {
        let wv = pv.controller().CoreWebView2()?;
        let settings = wv.Settings()?;
        settings.SetAreDefaultContextMenusEnabled(false)?;
        let settings = settings.cast::<ICoreWebView2Settings4>()?;
        settings.SetAreBrowserAcceleratorKeysEnabled(false)?;
        settings.SetIsPasswordAutosaveEnabled(false)?;
        // As well as on_navigation: Tauri lets through any address it can't parse, and a page on
        // such a host panics Tauri's IPC, which ends the app.
        let guard = webview2_com::NavigationStartingEventHandler::create(Box::new(|_, args| {
            if let Some(args) = args {
                let mut uri = windows::core::PWSTR::null();
                // A live args interface; `take_pwstr` frees the string it hands out.
                args.Uri(&mut uri)?;
                if !allowed_uri(&webview2_com::take_pwstr(uri)) {
                    args.SetCancel(true)?;
                }
            }
            Ok(())
        }));
        let mut token = Default::default();
        wv.add_NavigationStarting(&guard, &mut token)?;
        // A crash can leave the last sign-in in a kept profile: never start with one.
        delete_auth_cookie(&wv)?;
        if let Some(cookie) = cookie {
            // WebView2's own manager: `.roblox.com` keeps its dot (Tauri's `set_cookie` drops it).
            let value = wide(cookie.expose());
            let mgr = wv.cast::<ICoreWebView2_2>()?.CookieManager()?;
            let c = mgr.CreateCookie(w!(".ROBLOSECURITY"), PCWSTR(value.as_ptr()), w!(".roblox.com"), w!("/"))?;
            c.SetIsHttpOnly(true)?;
            c.SetIsSecure(true)?;
            c.SetSameSite(COREWEBVIEW2_COOKIE_SAME_SITE_KIND_LAX)?;
            mgr.AddOrUpdateCookie(&c)?;
        }
        let mut pid = 0;
        wv.BrowserProcessId(&mut pid)?;
        Ok(pid)
    }
}

/// Deletes `.ROBLOSECURITY` from the profile, as a domain cookie and as any host-only one.
///
/// # Safety
/// A live `wv`, on the main thread (inside `with_webview`).
unsafe fn delete_auth_cookie(wv: &ICoreWebView2) -> windows::core::Result<()> {
    // SAFETY: as the function's contract.
    unsafe {
        let mgr = wv.cast::<ICoreWebView2_2>()?.CookieManager()?;
        mgr.DeleteCookiesWithDomainAndPath(w!(".ROBLOSECURITY"), w!(".roblox.com"), w!("/"))?;
        mgr.DeleteCookies(w!(".ROBLOSECURITY"), w!("https://www.roblox.com/"))
    }
}

/// Sign-in providers Roblox hands off to; their sessions are the user's own and must not stay
/// in a kept profile.
const SIGN_IN_PROVIDERS: [&str; 2] = ["google.com", "apple.com"];

fn is_sign_in_provider(domain: &str) -> bool {
    let d = domain.trim_start_matches('.').to_ascii_lowercase();
    SIGN_IN_PROVIDERS.iter().any(|p| d == *p || d.ends_with(&format!(".{p}")))
}

/// Deletes the Google / Apple cookies in `list`.
///
/// # Safety
/// Live interfaces, on the main thread (WebView2 calls the GetCookies handler there).
unsafe fn delete_sign_in_providers(mgr: &ICoreWebView2CookieManager, list: &ICoreWebView2CookieList) -> windows::core::Result<()> {
    // SAFETY: as the function's contract; `take_pwstr` frees the string it hands out.
    unsafe {
        let mut n = 0;
        list.Count(&mut n)?;
        for i in 0..n {
            let c = list.GetValueAtIndex(i)?;
            let mut domain = windows::core::PWSTR::null();
            c.Domain(&mut domain)?;
            if is_sign_in_provider(&webview2_com::take_pwstr(domain)) {
                mgr.DeleteCookie(&c)?;
            }
        }
        Ok(())
    }
}

/// Deletes the Google / Apple cookies of a window's profile, and `.ROBLOSECURITY` too when `auth`
/// (not while a session window opens: harden just put the account's in). Blocking, ≤ 5 s; true
/// only when every delete worked — a profile is marked clean on nothing less.
fn clear_cookies(window: &WebviewWindow, auth: bool) -> bool {
    let (tx, rx) = std::sync::mpsc::channel();
    let label = window.label().to_owned();
    let r = window.with_webview(move |pv| {
        let done = tx.clone();
        // SAFETY: main thread (with_webview), live controller.
        let started = unsafe {
            pv.controller().CoreWebView2().and_then(|wv| {
                if auth {
                    delete_auth_cookie(&wv)?;
                }
                let mgr = wv.cast::<ICoreWebView2_2>()?.CookieManager()?;
                let m = mgr.clone();
                let handler = webview2_com::GetCookiesCompletedHandler::create(Box::new(move |hr, list| {
                    // WebView2 calls this on the main thread with a live list (the outer SAFETY).
                    let ok = match (hr, list) {
                        (Ok(()), Some(list)) => delete_sign_in_providers(&m, &list)
                            .inspect_err(|e| log::warn!("could not delete Google / Apple cookies from a Roblox window's profile: {e}"))
                            .is_ok(),
                        (r, _) => {
                            log::warn!("could not list a Roblox window's cookies: {:?}", r.err());
                            false
                        }
                    };
                    let _ = done.send(ok);
                    Ok(())
                }));
                // A null URI lists every cookie of the profile.
                mgr.GetCookies(PCWSTR::null(), &handler)
            })
        };
        if let Err(e) = started {
            log::warn!("could not delete cookies from window {label}'s profile: {e}");
            let _ = tx.send(false);
        }
    });
    if let Err(e) = r {
        log::warn!("could not delete cookies from a Roblox window's profile: {e}");
    }
    matches!(rx.recv_timeout(Duration::from_secs(5)), Ok(true))
}

/// `clear_cookies` off the async thread it would block.
async fn clear_cookies_async(window: &WebviewWindow, auth: bool) -> bool {
    let window = window.clone();
    tauri::async_runtime::spawn_blocking(move || clear_cookies(&window, auth)).await.unwrap_or(false)
}

/// Logs WebView2 renderer/GPU/browser process failures of `window` (a white screen otherwise
/// leaves no trace).
pub fn log_process_failures(window: &WebviewWindow) {
    let label = window.label().to_string();
    let r = window.with_webview(move |pv| {
        use webview2_com::ProcessFailedEventHandler;
        let label = label.clone();
        let handler = ProcessFailedEventHandler::create(Box::new(move |_, args| {
            let mut kind = Default::default();
            if let Some(args) = args {
                // SAFETY: a live args interface handed to the event handler.
                unsafe { args.ProcessFailedKind(&mut kind)? };
            }
            log::error!("WebView2 process failed in window \"{label}\" (kind {})", kind.0);
            Ok(())
        }));
        let mut token = Default::default();
        // SAFETY: COM call on the main thread (with_webview) on a live controller.
        let added = unsafe { pv.controller().CoreWebView2().and_then(|wv| wv.add_ProcessFailed(&handler, &mut token)) };
        if let Err(e) = added {
            log::warn!("could not watch WebView2 process failures: {e}");
        }
    });
    if let Err(e) = r {
        log::warn!("could not watch WebView2 process failures: {e}");
    }
}

/// NUL-terminated UTF-16 for a `PCWSTR`, zeroed on drop.
fn wide(s: &str) -> Zeroizing<Vec<u16>> {
    Zeroizing::new(s.encode_utf16().chain([0]).collect())
}

fn parse_url(s: &str) -> Result<Url> {
    s.parse().map_err(|e: url::ParseError| AppError::Internal(e.to_string()))
}

/// The first close request gets the close path; clicks while it runs get nothing.
fn take_once<T>(slot: &Mutex<Option<T>>) -> Option<T> {
    slot.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// Blocking (see `poll_cookie`): reads the profile's cookie while the window still exists,
/// hands it to `on_close`, deletes it from the profile, then destroys the window.
fn end_session(app: &AppHandle, label: &str, title: &str, pid: u32, profile: &Path, on_close: impl FnOnce(Option<Secret>)) {
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
        // Unmarked before the window goes: once it is destroyed a new window for this account can
        // open on the same profile, and its mark must not be removed by this one.
        if clear_cookies(&window, true) {
            mark_closed(profile);
        }
        if let Err(e) = window.destroy() {
            log::debug!("could not destroy session window {label}: {e}");
        }
    }
    wait_for_exit(pid);
    CLOSING.lock().unwrap_or_else(|e| e.into_inner()).retain(|l| l != label);
}

async fn wait_for_cookie(app: AppHandle, label: String) -> Result<Secret> {
    tauri::async_runtime::spawn_blocking(move || poll_cookie(&app, &label)).await?
}

/// Blocking: `cookies()` deadlocks on the main thread and in event handlers.
fn poll_cookie(app: &AppHandle, label: &str) -> Result<Secret> {
    let deadline = Instant::now() + LOGIN_TIMEOUT;
    let mut failures = 0;
    while Instant::now() < deadline {
        // Also how we notice the user closing the window.
        let Some(window) = app.get_webview_window(label) else {
            return Err(AppError::Cancelled);
        };
        // Signing in (2-step codes, captchas) is activity: don't auto-lock under it, or the
        // captured cookie could not be encrypted.
        app.state::<crate::App>().vault.touch();
        // Not `cookies_for_url`: it panics if the window goes away mid-call.
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

/// The first non-empty `.ROBLOSECURITY` from roblox.com (Tauri strips the leading dot).
fn pick_roblosecurity(cookies: Vec<Cookie<'static>>) -> Option<Secret> {
    cookies
        .into_iter()
        .find(|c| c.name() == ".ROBLOSECURITY" && !c.value().is_empty() && c.domain().is_some_and(is_roblox))
        .map(|c| Secret::new(c.value()))
}

/// Where a Roblox window may go: Roblox over https, and the sign-in providers it hands off to.
/// A Tauri window's page can crash the app through its IPC with a crafted host name, so it stays
/// off anything else.
fn allowed(url: &Url) -> bool {
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

fn allowed_uri(uri: &str) -> bool {
    uri.parse().is_ok_and(|u| allowed(&u))
}

fn is_roblox(domain: &str) -> bool {
    let d = domain.trim_start_matches('.').to_ascii_lowercase();
    d == "roblox.com" || d.ends_with(".roblox.com")
}

/// A profile folder stays locked until its browser process has exited (≤ 5 s here).
fn wait_for_exit(pid: u32) {
    // SAFETY: the handle is used only here and closed before returning.
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            if WaitForSingleObject(h, 5_000) == WAIT_TIMEOUT {
                log::debug!("browser process {pid} still running after 5 s");
            }
            let _ = CloseHandle(h);
        }
    }
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
    fn profiles_left_open_are_swept() {
        let d = std::env::temp_dir().join(format!("roalt-sweep-{}", uuid::Uuid::new_v4()));
        let (left, clean) = (session_profile(&d, 1), session_profile(&d, 2));
        mark_open(&left);
        mark_open(&clean);
        mark_closed(&clean);
        mark_open(&login_profile(&d));
        sweep_dirty_profiles(&d);
        assert!(!left.exists() && !login_profile(&d).exists(), "left-open profiles deleted");
        assert!(clean.exists() && !clean.join(OPEN_MARK).exists(), "a clean profile stays");
        assert!(remove_profile(&d) && remove_profile(&d), "a missing folder counts as removed");
    }

    #[test]
    fn a_profile_left_behind_stays_marked() {
        use std::os::windows::fs::OpenOptionsExt;
        let d = std::env::temp_dir().join(format!("roalt-held-{}", uuid::Uuid::new_v4()));
        let p = session_profile(&d, 1);
        mark_open(&p);
        let held = std::fs::OpenOptions::new().create(true).truncate(true).write(true).share_mode(0).open(p.join("Cookies")).unwrap();
        assert!(!remove_profile(&p), "a held file keeps the folder");
        assert!(p.join(OPEN_MARK).exists(), "and its mark, for the next sweep");
        drop(held);
        assert!(remove_profile(&d));
    }

    #[test]
    fn profiles_of_removed_accounts_are_swept() {
        let d = std::env::temp_dir().join(format!("roalt-orphan-{}", uuid::Uuid::new_v4()));
        let (kept, gone) = (session_profile(&d, 1), session_profile(&d, 2));
        std::fs::create_dir_all(&kept).unwrap();
        std::fs::create_dir_all(&gone).unwrap();
        sweep_orphan_profiles(&d, &[1]);
        assert!(kept.exists() && !gone.exists());
        assert!(remove_profile(&d));
    }

    #[test]
    fn sign_in_providers_by_domain() {
        assert!(is_sign_in_provider(".google.com") && is_sign_in_provider("accounts.google.com") && is_sign_in_provider("appleid.apple.com"));
        assert!(!is_sign_in_provider("roblox.com") && !is_sign_in_provider("notgoogle.com"));
    }

    #[test]
    fn profiles_are_kept_outside_the_swept_folder() {
        let d = Path::new("D");
        assert_eq!(login_profile(d), d.join("webview-login"));
        assert_eq!(session_profile(d, 7), d.join("webview-sessions").join("7"));
        assert!(!login_profile(d).starts_with(d.join(TMP_DIR)) && !session_profile(d, 7).starts_with(d.join(TMP_DIR)));
    }

    #[test]
    fn picks_only_a_non_empty_roblox_cookie() {
        let jar = vec![
            cookie(".ROBLOSECURITY", "fake", "evil.com"),
            cookie(".ROBLOSECURITY", "", ".roblox.com"),
            cookie("GuestData", "guest", ".roblox.com"),
            cookie(".ROBLOSECURITY", "good", ".roblox.com"),
        ];
        assert_eq!(picked(jar), Some("good".into()));
        assert_eq!(picked(vec![cookie(".ROBLOSECURITY", "w", "www.roblox.com")]), Some("w".into()));
        assert_eq!(picked(vec![cookie(".ROBLOSECURITY", "x", "notroblox.com")]), None);
        assert_eq!(picked(vec![cookie(".ROBLOSECURITY", "x", "roblox.com.evil.com")]), None);
        assert_eq!(picked(vec![Cookie::new(".ROBLOSECURITY", "no-domain")]), None);
        assert_eq!(picked(vec![cookie(".roblosecurity", "x", "roblox.com")]), None);
        assert_eq!(picked(vec![]), None);
    }

    #[test]
    fn roblox_windows_stay_on_roblox() {
        let go = allowed_uri;
        for u in ["about:blank", "https://www.roblox.com/home", "https://roblox.com/", "https://appleid.apple.com/x", "https://accounts.google.com/"] {
            assert!(go(u), "{u}");
        }
        for u in [
            "http://www.roblox.com/",
            "http://tauri.localhost/",
            "https://tauri.localhost/",
            "http://x:y@tauri.localhost:1234/",
            "https://evil.com/",
            "https://roblox.com.evil.com/",
            "https://x@www.roblox.com/",
            "https://www.roblox.com:8443/",
            "file:///C:/x",
            "http://xn--a.localhost:1/",
        ] {
            assert!(!go(u), "{u}");
        }
    }

    #[test]
    fn session_helpers() {
        assert_eq!(*wide("aZ_9"), [97, 90, 95, 57, 0]);
        assert_eq!(*wide(""), [0]);
        assert_eq!(parse_url(HOME_PAGE).unwrap().host_str(), Some("www.roblox.com"));
        assert!(parse_url("about:blank").is_ok());
    }

    #[test]
    fn close_path_runs_once() {
        let pending = Mutex::new(Some(7));
        assert_eq!(take_once(&pending), Some(7));
        assert_eq!(take_once(&pending), None);
    }

    #[test]
    fn sweep_removes_leftover_profiles() {
        let root = std::env::temp_dir().join(format!("roalt-sweep-{}", uuid::Uuid::new_v4()));
        let tmp = root.join(TMP_DIR);
        std::fs::create_dir_all(tmp.join("a").join("EBWebView")).unwrap();
        std::fs::write(tmp.join("a").join("EBWebView").join("Cookies"), b"x").unwrap();
        std::fs::create_dir_all(tmp.join("b")).unwrap();
        std::fs::write(tmp.join("stray.tmp"), b"x").unwrap();
        sweep_temp_profiles(&root);
        assert_eq!(std::fs::read_dir(&tmp).unwrap().count(), 0);
        sweep_temp_profiles(&root.join("missing"));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn clears_webview2_overrides() {
        for name in WEBVIEW2_VARS {
            // SAFETY: nothing else in the test binary reads these variables, and std serialises
            // its own environment access on Windows.
            unsafe { std::env::set_var(name, "x") };
        }
        clear_webview2_env();
        assert!(WEBVIEW2_VARS.iter().all(|n| std::env::var_os(n).is_none()));
    }
}
