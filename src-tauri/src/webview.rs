//! Roblox pages on kept WebView2 profiles: the sign-in window, and each account's own profile
//! for Browse (`browse`, Roblox inside the RoALT window). A fresh profile each time looked like a
//! never-seen browser to Roblox and drew a captcha (403) on most sign-ins. `.ROBLOSECURITY` (the
//! vault holds it) and Google / Apple sign-ins are deleted when a page opens and again before it
//! closes. One that never closed normally (crash, quit with it open) leaves an `OPEN_MARK` behind:
//! that profile is deleted whole at exit or the next start (`sweep_dirty_profiles`).

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tauri::webview::{Cookie, PlatformWebview};
use tauri::{AppHandle, Manager, Webview, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use url::Url;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_COOKIE_SAME_SITE_KIND_LAX, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL, ICoreWebView2,
    ICoreWebView2_2, ICoreWebView2_19, ICoreWebView2CookieList, ICoreWebView2CookieManager, ICoreWebView2Settings4,
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
pub(crate) const OPEN_MARK: &str = ".roalt-open";
const POLL: Duration = Duration::from_millis(700);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(15 * 60);

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

/// Opens `https://www.roblox.com/login` in a `login-<uuid>` window on the kept sign-in profile
/// `<data_dir>/webview-login` (no devtools, no autofill, no context menu, no IPC capability),
/// with any `.ROBLOSECURITY` deleted before the page loads, so the last account's cookie can't
/// be taken for this one. `fill`: its username and password go into the form (`fill_script`).
/// A worker thread polls `cookies()` every ~700 ms until a non-empty
/// `.ROBLOSECURITY` appears, deletes it from the profile, closes the window and waits for its
/// browser process to exit. Closing the window first → `AppError::Cancelled`.
pub async fn capture_login(app: &AppHandle, data_dir: &Path, fill: Option<&LoginFill>) -> Result<Secret> {
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
    let script = fill.map(|f| fill_script(&f.username, f.password.expose()));
    let (pid, cookie) = match harden(window.as_ref(), None, script).await {
        // A Google / Apple sign-in left by an earlier window goes before the page loads.
        Ok(pid) => match (clear_cookies_async(window.as_ref(), false).await, window.navigate(login)) {
            (_, Ok(())) => (Some(pid), wait_for_cookie(app.clone(), label.clone()).await),
            (_, Err(e)) => (Some(pid), Err(e.into())),
        },
        Err(e) => (None, Err(e)),
    };
    // Closed by the user: its cookies (a half-done Google sign-in too) can't be deleted any more, so
    // the whole profile goes once its browser has exited (kept marked if that fails: next start).
    let cleared = app.get_webview_window(&label).is_some() && clear_cookies_async(window.as_ref(), true).await;
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

/// Marks `profile` as in use (created if needed). Logged, not an error: the window still works.
pub(crate) fn mark_open(profile: &Path) {
    if let Err(e) = std::fs::create_dir_all(profile).and_then(|()| std::fs::write(profile.join(OPEN_MARK), b"")) {
        log::warn!("could not mark browser profile {} as open: {e}", profile.display());
    }
}

/// After a normal close, once the cookies were deleted.
pub(crate) fn mark_closed(profile: &Path) {
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

/// An account's kept profile, the one Browse uses as that account.
pub(crate) fn session_profile(data_dir: &Path, account_id: i64) -> PathBuf {
    data_dir.join(SESSIONS_DIR).join(account_id.to_string())
}

/// Deletes the sign-in profile and every session profile (blocking: removal retries), unless the
/// sign-in window or Browse is open, whose browser holds one: then nothing, and `false`.
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

/// The sign-in window or Browse is open (or Browse is still closing): its browser holds a
/// profile, and Browse may yet hand over a newer cookie.
pub fn browsing(app: &AppHandle) -> bool {
    crate::browse::is_open() || signing_in(app)
}

/// The sign-in window is open: the cookie it captures is encrypted as it closes, which needs the
/// vault open (auto-lock waits for it).
pub fn signing_in(app: &AppHandle) -> bool {
    LOGIN_ACTIVE.load(Ordering::Acquire) || app.webview_windows().keys().any(|l| l.starts_with("login-"))
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

/// Deletes a removed account's session profile (blocking; logged, never an error). Browse, open
/// as that account, closes first (its cookie isn't kept: the account is gone).
pub fn remove_account_profile(app: &AppHandle, data_dir: &Path, account_id: i64) {
    crate::browse::close_if(app, account_id);
    remove_profile(&session_profile(data_dir, account_id));
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

/// Turns off what Tauri does not expose, adds `cookie` as `.ROBLOSECURITY` and `script` to
/// every page if given, and returns the profile's browser process id.
pub(crate) async fn harden(window: &Webview, cookie: Option<Secret>, script: Option<Zeroizing<String>>) -> Result<u32> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    window.with_webview(move |pv| {
        let script = script.map(|s| wide(&s));
        let r = harden_webview(&pv, cookie.as_ref(), script.as_deref().map(Vec::as_slice))
            .map_err(|e| AppError::Internal(tr!("WebView2 setup failed: {why}", why = e)));
        let _ = tx.send(r);
    })?;
    // The closure is dropped unrun if the window is already gone.
    rx.await.map_err(|_| {
        log::debug!("webview setup skipped: window {} already gone", window.label());
        AppError::Cancelled
    })?
}

fn harden_webview(pv: &PlatformWebview, cookie: Option<&Secret>, script: Option<&[u16]>) -> windows::core::Result<u32> {
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
        if let Some(script) = script {
            // NUL-terminated (`wide`); registered before the first navigation, so the login page has it.
            let done = webview2_com::AddScriptToExecuteOnDocumentCreatedCompletedHandler::create(Box::new(|_, _| Ok(())));
            wv.AddScriptToExecuteOnDocumentCreated(PCWSTR(script.as_ptr()), &done)?;
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

/// Deletes the Google / Apple cookies of a page's profile, and `.ROBLOSECURITY` too when `auth`
/// (not while Browse opens: harden just put the account's in). Blocking, ≤ 5 s; true only when
/// every delete worked — a profile is marked clean on nothing less.
pub(crate) fn clear_cookies(window: &Webview, auth: bool) -> bool {
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
async fn clear_cookies_async(window: &Webview, auth: bool) -> bool {
    let window = window.clone();
    tauri::async_runtime::spawn_blocking(move || clear_cookies(&window, auth)).await.unwrap_or(false)
}

/// Lets WebView2 trim `wv`'s memory while it is out of sight (`low`), or back to usual. Best effort:
/// runtimes before 1.0.2210 can't.
pub fn memory_target(wv: &Webview, low: bool) {
    let level = if low { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW } else { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL };
    let r = wv.with_webview(move |pv| {
        // SAFETY: COM calls on the main thread (with_webview) on a live controller.
        let r = unsafe { pv.controller().CoreWebView2().and_then(|core| core.cast::<ICoreWebView2_19>()).and_then(|core| core.SetMemoryUsageTargetLevel(level)) };
        if let Err(e) = r {
            log::debug!("WebView2 memory target not set: {e}");
        }
    });
    if let Err(e) = r {
        log::debug!("WebView2 memory target not set: {e}");
    }
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

pub(crate) fn parse_url(s: &str) -> Result<Url> {
    s.parse().map_err(|e: url::ParseError| AppError::Internal(e.to_string()))
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
pub(crate) fn pick_roblosecurity(cookies: Vec<Cookie<'static>>) -> Option<Secret> {
    cookies
        .into_iter()
        .find(|c| c.name() == ".ROBLOSECURITY" && !c.value().is_empty() && c.domain().is_some_and(is_roblox))
        .map(|c| Secret::new(c.value()))
}

/// Where a Roblox page may go: Roblox over https, and the sign-in providers it hands off to.
/// A Tauri window's page can crash the app through its IPC with a crafted host name, so it stays
/// off anything else. Never Create (`creating`): building games is not what RoALT is for.
pub(crate) fn allowed(url: &Url) -> bool {
    let ok = url.as_str() == "about:blank"
        || (url.scheme() == "https"
            && url.username().is_empty()
            && url.port().is_none()
            && url.host_str().is_some_and(|h| is_roblox(h) || is_sign_in_provider(h))
            && !creating(url));
    if !ok {
        log::debug!("Roblox window: navigation to another site refused");
    }
    ok
}

/// Roblox's Create pages: the Creator Hub and Studio's web pages.
pub(crate) fn creating(url: &Url) -> bool {
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let path = url.path().to_ascii_lowercase();
    host == "create.roblox.com"
        || ((host == "www.roblox.com" || host == "roblox.com") && ["/develop", "/create", "/build"].iter().any(|p| path == *p || path.starts_with(&format!("{p}/"))))
}

fn allowed_uri(uri: &str) -> bool {
    uri.parse().is_ok_and(|u| allowed(&u))
}

fn is_roblox(domain: &str) -> bool {
    let d = domain.trim_start_matches('.').to_ascii_lowercase();
    d == "roblox.com" || d.ends_with(".roblox.com")
}

/// A profile folder stays locked until its browser process has exited (≤ 5 s here).
pub(crate) fn wait_for_exit(pid: u32) {
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
pub(crate) fn remove_profile(dir: &Path) -> bool {
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
            "https://create.roblox.com/dashboard/creations",
            "https://www.roblox.com/develop",
            "https://www.roblox.com/create/x",
            "https://roblox.com/build",
        ] {
            assert!(!go(u), "{u}");
        }
        assert!(go("https://www.roblox.com/developer-terms") && go("https://www.roblox.com/creators"), "only Create's own pages");
    }

    #[test]
    fn session_helpers() {
        assert_eq!(*wide("aZ_9"), [97, 90, 95, 57, 0]);
        assert_eq!(*wide(""), [0]);
        assert_eq!(parse_url("https://www.roblox.com/home").unwrap().host_str(), Some("www.roblox.com"));
        assert!(parse_url("about:blank").is_ok());
    }

    #[test]
    fn fill_script_escapes_what_it_puts_in() {
        let s = fill_script("bob", "p\"a\\s</script>\n'x");
        assert!(s.contains(r#"const user = "bob", pass = "p\"a\\s</script>\n'x";"#), "{}", *s);
        assert!(s.contains(r#"location.hostname !== "www.roblox.com""#) && s.contains("window.top !== window"));
        assert!(!s.contains("login-button"), "never presses Log In");
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
