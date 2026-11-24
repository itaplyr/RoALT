//! Browse on Linux: roblox.com inside the RoALT window, as one stored account at a time. A child
//! webview (`LABEL`) of the main window on that account's kept profile (`webview::session_profile`),
//! with the account's `.ROBLOSECURITY` put in before the first page loads and navigation kept on
//! Roblox (and the Google / Apple sign-in pages), off Create, and off the real-money pages while
//! the purchase lock is on. It reaches no command: `main_only` refuses every one from a webview
//! other than the UI's. The UI places it over its own page (`browse_bounds`) and hides it under its
//! dialogs and menus. A link meant for a new window opens here; downloads are refused.
//!
//! WebKitGTK's child webviews land in the window's default `gtk::Box`, where wry's `set_bounds` is
//! a no-op, so the page is re-parented into a `gtk::Fixed` layered over the window with a
//! `gtk::Overlay` (see `embed`); `browse_bounds` moves it there.
//!
//! It closes (`close`) on an account switch, `browse_close`, the vault locking, quitting and the
//! account's removal: the cookie it ended with is kept when it still signs in as the same user
//! (Roblox rotates it, a sign-in again there fixes a dead one), then deleted from the profile.
//!
//! The types mirror `browse.rs` exactly (same `ts-rs` names) so `npm run bindings` on Linux still
//! generates them for the frontend.

use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
use gtk::prelude::*;
use serde::{Deserialize, Serialize};
use tauri::webview::{Cookie, NewWindowResponse, PageLoadEvent, WebviewBuilder};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, Webview, WebviewUrl};
use ts_rs::TS;
use url::Url;

use crate::App;
use crate::error::{AppError, Result};
use crate::vault::Secret;
use crate::webview;

const LABEL: &str = "browse";
const ROBLOX: &str = "https://www.roblox.com/";

/// Who Browse is open as, and the kept profile it uses.
struct Open {
    account_id: i64,
    user_id: i64,
    profile: std::path::PathBuf,
}

/// Held while Browse opens or closes, so one never runs into the other.
static OPEN: Mutex<Option<Open>> = Mutex::new(None);
/// From the moment Browse starts opening until it has closed: its profile is in use.
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Browse is open, or still opening or closing.
pub fn is_open() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

/// `Settings.lock_purchases`, as Browse's navigation check reads it (set when Browse opens and when
/// the setting changes; on by default).
static LOCK: AtomicBool = AtomicBool::new(true);

pub fn set_lock(on: bool) {
    LOCK.store(on, Ordering::Relaxed);
}

/// Roblox's real-money pages (pure): Robux and Premium upgrades, billing, payments.
fn money_page(url: &Url) -> bool {
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let path = url.path().to_ascii_lowercase();
    let under = |p: &str| path == p || path.starts_with(&format!("{p}/"));
    host == "billing.roblox.com"
        || (host == "apis.roblox.com" && under("/payments-gateway"))
        || (matches!(host.as_str(), "www.roblox.com" | "roblox.com") && (under("/upgrades") || under("/premium") || under("/robux")))
}

/// `browse://blocked`: what Browse just refused, for the UI to say. Linux can refuse Create and
/// money pages at navigation time (no request bodies to inspect), so `Robux` and `SignOut` are only
/// reachable on Windows; they stay for the shared frontend and bindings.
#[allow(dead_code)]
#[derive(Serialize, TS, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BrowseBlocked {
    Create,
    Robux,
    Money,
    SignOut,
}

/// Where Browse's page goes, in the main window's logical pixels (the UI's CSS pixels).
#[derive(Deserialize, TS, Clone, Copy, Debug)]
#[ts(export)]
pub struct PaneRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// `browse://page`: the page Browse finished loading.
#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BrowsePage {
    pub account_id: i64,
    /// Path and query on www.roblox.com, like "games/123/x" or "search/users?keyword=bob".
    pub path: String,
}

/// What the profile card shows (same shape as upstream). Linux has no page script to draw it, so
/// `browse_card` is a no-op; the type stays for the shared frontend and bindings.
#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PageCard {
    pub user_id: i64,
    pub title: String,
    pub toggle: String,
    pub rows: Vec<(String, String)>,
    pub note: Option<String>,
    pub top_title: String,
    pub top: Vec<(String, String)>,
}

/// Shows `card` on Browse's profile page (None: takes it away). No-op on Linux.
#[tauri::command]
pub fn browse_card(_app: AppHandle, _card: Option<PageCard>) -> Result<()> {
    Ok(())
}

/// Browse's page buttons.
#[derive(Deserialize, TS, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BrowseNav {
    Back,
    Forward,
    Reload,
}

/// A path on www.roblox.com as a page Browse may show (pure): the same host, allowed, never Create.
fn page_url(path: &str) -> Result<Url> {
    let refused = || AppError::Invalid(tr!("That page isn't available in Browse."));
    let url = Url::parse(ROBLOX).and_then(|base| base.join(path.trim_start_matches('/'))).map_err(|_| refused())?;
    if url.host_str() != Some("www.roblox.com") || !webview::allowed(&url) || webview::creating(&url) || (LOCK.load(Ordering::Relaxed) && money_page(&url)) {
        return Err(refused());
    }
    Ok(url)
}

/// What the UI shows for a page: its path and query on Roblox (pure).
fn path_of(url: &Url) -> String {
    let path = url.path().trim_start_matches('/');
    url.query().map_or_else(|| path.to_owned(), |q| format!("{path}?{q}"))
}

/// Shows www.roblox.com/`path` as `account_id` at `rect` (first opening it, or moving it over from
/// another account). Returns once the page is on its way.
#[tauri::command]
pub async fn browse_open(app: AppHandle, s: State<'_, App>, account_id: i64, path: String, rect: PaneRect) -> Result<()> {
    if s.vault.is_locked() {
        return Err(AppError::Locked);
    }
    // The lock gates money pages in `page_url` as well as navigation: read the setting first so a
    // disabled lock does not refuse a page the session would allow.
    if let Ok(v) = s.db.settings() {
        set_lock(v.lock_purchases);
    }
    let url = page_url(&path)?;
    let a = s.db.account(account_id)?;
    let cookie = crate::launcher::account_cookie(&s, account_id)?;
    let data_dir = s.data_dir.clone();
    // Off the async threads: making the webview waits for the main thread, and its cookies are read
    // or written on this blocking thread.
    tauri::async_runtime::spawn_blocking(move || open(&app, &data_dir, a.id, a.user_id, cookie, url, rect)).await?
}

/// The UI shows Browse's page (`browse_bounds`). The main window going to the tray or minimizing
/// hides the page too; coming back shows it again only if so.
static PANE_SHOWN: AtomicBool = AtomicBool::new(false);
/// The main window is on screen, as last told by `set_main_shown` through `window_shown`.
static MAIN_SHOWN: AtomicBool = AtomicBool::new(true);
/// The child is inside the overlaid `gtk::Fixed` and may be shown. Until `embed` has re-parented it
/// it still sits in the window's vertical box, where showing it would lay it out over the UI.
static EMBEDDED: AtomicBool = AtomicBool::new(false);
/// Browse is closing: its cookie read can take seconds, during which a late `browse_bounds` must not
/// reveal it and `is_open()` (ACTIVE) must stay true so quitting still waits for the cookie write.
static CLOSING: AtomicBool = AtomicBool::new(false);

/// Moves Browse's page to `rect`, and shows or hides it (under a dialog, on another view).
#[tauri::command]
pub fn browse_bounds(app: AppHandle, rect: PaneRect, shown: bool) -> Result<()> {
    // Closing (its cookie read takes seconds): a late bounds must not reveal the closing page.
    if CLOSING.load(Ordering::Relaxed) || !ACTIVE.load(Ordering::Acquire) {
        return Ok(());
    }
    // Still in the window's vertical box (embed racing this): record the wish only.
    PANE_SHOWN.store(shown, Ordering::Relaxed);
    if !EMBEDDED.load(Ordering::Acquire) {
        return Ok(());
    }
    let Some(wv) = app.get_webview(LABEL) else { return Ok(()) };
    place(&wv, rect);
    if shown && MAIN_SHOWN.load(Ordering::Relaxed) { wv.show()? } else { wv.hide()? }
    Ok(())
}

/// The main window went to the tray or minimized (`shown` false), or is back.
pub fn window_shown(app: &AppHandle, shown: bool) {
    MAIN_SHOWN.store(shown, Ordering::Relaxed);
    if CLOSING.load(Ordering::Relaxed) || !ACTIVE.load(Ordering::Acquire) || !EMBEDDED.load(Ordering::Acquire) {
        return;
    }
    let Some(wv) = app.get_webview(LABEL) else { return };
    if let Err(e) = if shown && PANE_SHOWN.load(Ordering::Relaxed) { wv.show() } else { wv.hide() } {
        log::debug!("Browse: could not follow the main window: {e}");
    }
    webview::memory_target(&wv, !shown);
}

/// Browse's page buttons, through the page's own history.
#[tauri::command]
pub fn browse_nav(app: AppHandle, action: BrowseNav) -> Result<()> {
    let Some(wv) = app.get_webview(LABEL) else { return Ok(()) };
    let js = match action {
        BrowseNav::Back => "history.back()",
        BrowseNav::Forward => "history.forward()",
        BrowseNav::Reload => "location.reload()",
    };
    wv.eval(js)?;
    Ok(())
}

/// Closes Browse, keeping the cookie it ended with (`close`).
#[tauri::command]
pub async fn browse_close(app: AppHandle) -> Result<()> {
    tauri::async_runtime::spawn_blocking(move || close(&app)).await?;
    Ok(())
}

/// Logical pixels, clamped to at least one as GTK sizes are unsigned in effect.
#[cfg(target_os = "linux")]
fn pixels(rect: PaneRect) -> (i32, i32, i32, i32) {
    (
        rect.x.round() as i32,
        rect.y.round() as i32,
        rect.width.max(1.0).round() as i32,
        rect.height.max(1.0).round() as i32,
    )
}

/// Moves the page's widget inside the `gtk::Fixed` it was embedded into. Queued to the main thread.
fn place(wv: &Webview, rect: PaneRect) {
    #[cfg(target_os = "linux")]
    {
        let (x, y, width, height) = pixels(rect);
        if let Err(e) = wv.with_webview(move |pv| {
            let w: gtk::Widget = pv.inner().upcast();
            if let Some(parent) = w.parent()
                && let Some(fixed) = parent.downcast_ref::<gtk::Fixed>()
            {
                fixed.move_(&w, x, y);
                w.set_size_request(width, height);
            }
        }) {
            log::debug!("Browse: could not move the page: {e}");
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = wv.set_position(LogicalPosition::new(rect.x, rect.y));
        let _ = wv.set_size(LogicalSize::new(rect.width.max(1.0), rect.height.max(1.0)));
    }
}

/// The `gtk::Fixed` Browse lives in, installing the overlay around the window's content the first
/// time. Runs on the GTK main thread (from `with_webview`). The fixed is kept on the window's
/// `ApplicationWindow` with `set_data` so both `embed` and `place` can find it without a static.
#[cfg(target_os = "linux")]
fn browse_fixed(gtk_window: &gtk::ApplicationWindow) -> gtk::Fixed {
    const KEY: &str = "roalt-browse-fixed";
    // SAFETY: main thread only; the key is ours and only ever holds a `gtk::Fixed` we put there.
    if let Some(ptr) = unsafe { gtk_window.data::<gtk::Fixed>(KEY) } {
        return unsafe { ptr.as_ref() }.clone();
    }
    let fixed = gtk::Fixed::new();
    fixed.set_halign(gtk::Align::Fill);
    fixed.set_valign(gtk::Align::Fill);
    fixed.set_hexpand(true);
    fixed.set_vexpand(true);
    // Wrap the window's current content in an overlay so Browse can float over it.
    let content = gtk_window.child();
    let overlay = gtk::Overlay::new();
    if let Some(content) = &content {
        if let Some(container) = content.parent().and_then(|p| p.downcast::<gtk::Container>().ok()) {
            container.remove(content);
        }
        overlay.set_child(Some(content));
    }
    overlay.add_overlay(&fixed);
    // GtkOverlay gives each overlay child its own window covering the whole window, and raises it
    // on every allocation. Without this the fixed's window swallows every click meant for the UI;
    // passing input through it leaves the page's own (descendant) window receiving its clicks.
    overlay.set_overlay_pass_through(&fixed, true);
    gtk_window.add(&overlay);
    // A window-level `show_all` (tao does this when restoring from the tray) recurses into every
    // visible child. Without this it would show the page's webview again, overriding `wv.hide()`.
    // `no-show-all` keeps the fixed out of that recursion; an explicit `wv.show()` still works.
    fixed.set_no_show_all(true);
    // Show the frame and the layer, but not recursively: the child webview is still in the box and
    // stays hidden until the UI asks for it (a `show_all` here would reveal it).
    overlay.show();
    fixed.show();
    // SAFETY: as above; the window keeps the fixed (and drops it with itself).
    unsafe { gtk_window.set_data(KEY, fixed.clone()) };
    fixed
}

/// Re-parents the freshly built child webview out of the window's vertical box and into the
/// overlaid `gtk::Fixed`, where `place` can move it. Waits (up to ~3 s) for the main thread to do
/// it, so the caller knows whether Browse can be shown at all.
fn embed(wv: &Webview, rect: PaneRect) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let (x, y, width, height) = pixels(rect);
        let (tx, rx) = std::sync::mpsc::channel::<bool>();
        wv.with_webview(move |pv| {
            let w: gtk::Widget = pv.inner().upcast();
            let ok = if let Some(toplevel) = w.toplevel()
                && let Some(gtk_window) = toplevel.downcast_ref::<gtk::ApplicationWindow>()
            {
                let fixed = browse_fixed(gtk_window);
                // The vertical box wry added it to does not place children freely: take it out.
                if let Some(parent) = w.parent()
                    && let Some(container) = parent.downcast_ref::<gtk::Container>()
                {
                    container.remove(&w);
                }
                fixed.put(&w, x, y);
                w.set_size_request(width, height);
                EMBEDDED.store(true, Ordering::Release);
                if PANE_SHOWN.load(Ordering::Relaxed) && MAIN_SHOWN.load(Ordering::Relaxed) {
                    w.show_all();
                } else {
                    // Hidden until the UI shows it: Browse may be gone, or a dialog up, by now.
                    w.hide();
                }
                true
            } else {
                log::warn!("Browse: could not find the main GTK window to embed the page in");
                false
            };
            let _ = tx.send(ok);
        })
        .map_err(|e| AppError::Internal(e.to_string()))?;
        match rx.recv_timeout(Duration::from_secs(3)) {
            Ok(true) => Ok(()),
            Ok(false) => Err(AppError::Internal("the page could not be embedded".into())),
            Err(_) => Err(AppError::Internal("embedding the page timed out".into())),
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        wv.set_position(LogicalPosition::new(rect.x, rect.y))?;
        wv.set_size(LogicalSize::new(rect.width.max(1.0), rect.height.max(1.0)))?;
        EMBEDDED.store(true, Ordering::Release);
        Ok(())
    }
}

/// Blocking (never on the main thread): the webview needs it free.
fn open(app: &AppHandle, data_dir: &Path, account_id: i64, user_id: i64, cookie: Secret, url: Url, rect: PaneRect) -> Result<()> {
    let mut slot = OPEN.lock().unwrap_or_else(|e| e.into_inner());
    // Waited behind a lock's close: the vault is locked now, and nothing opens over its screen.
    if app.state::<App>().vault.is_locked() {
        return Err(AppError::Locked);
    }
    if slot.as_ref().is_some_and(|o| o.account_id == account_id)
        && let Some(wv) = app.get_webview(LABEL)
    {
        // Shown by the UI's next bounds, once Browse is still on screen with nothing over it.
        place(&wv, rect);
        // Back on Browse with its page still there: it stays as it was, not loaded again.
        if wv.url().ok().as_ref() != Some(&url) {
            wv.navigate(url)?;
        }
        return Ok(());
    }
    if let Some(o) = slot.take() {
        end(app, o, true);
    }
    ACTIVE.store(true, Ordering::Release);
    match create(app, data_dir, account_id, user_id, cookie, url, rect) {
        Ok(o) => {
            *slot = Some(o);
            Ok(())
        }
        Err(e) => {
            ACTIVE.store(false, Ordering::Release);
            Err(e)
        }
    }
}

fn create(app: &AppHandle, data_dir: &Path, account_id: i64, user_id: i64, cookie: Secret, url: Url, rect: PaneRect) -> Result<Open> {
    let profile = webview::session_profile(data_dir, account_id);
    // Marked with nothing open: left by a crash or a removal that failed (maybe of an earlier
    // account with this id), so nothing of it is reused.
    if profile.join(webview::OPEN_MARK).exists() {
        webview::remove_profile(&profile);
    }
    webview::mark_open(&profile);
    let window = app.get_window("main").ok_or_else(|| AppError::Internal("the main window is missing".into()))?;
    if let Ok(v) = app.state::<App>().db.settings() {
        set_lock(v.lock_purchases);
    }
    let (links, pages, refused, plays, plays_new) = (app.clone(), app.clone(), app.clone(), app.clone(), app.clone());
    // Blank first, so nothing loads from Roblox before the cookie is in place.
    let builder = WebviewBuilder::new(LABEL, WebviewUrl::External(webview::parse_url("about:blank")?))
        .data_directory(profile.clone())
        .incognito(false)
        .devtools(cfg!(debug_assertions))
        .general_autofill_enabled(false)
        .on_navigation(move |url| {
            // A Play or Join link: never let it out (the system would start Roblox past RoALT);
            // its target goes to the UI instead, which launches the account through RoALT.
            if url.scheme().eq_ignore_ascii_case("roblox-player") {
                play(&plays, account_id, url.as_str());
                return false;
            }
            let why = if webview::creating(url) {
                Some(BrowseBlocked::Create)
            } else if LOCK.load(Ordering::Relaxed) && money_page(url) {
                Some(BrowseBlocked::Money)
            } else {
                None
            };
            if let Some(why) = why {
                let _ = refused.emit("browse://blocked", why);
                return false;
            }
            webview::allowed(url)
        })
        .on_new_window(move |url, _| {
            if url.scheme().eq_ignore_ascii_case("roblox-player") {
                // A Play link opened in a new tab is the same hand-off as one followed in place.
                play(&plays_new, account_id, url.as_str());
            } else if webview::allowed(&url)
                && let Some(wv) = links.get_webview(LABEL)
                && let Err(e) = wv.navigate(url)
            {
                // A new window or tab would be out of RoALT's hands: the link opens here instead.
                log::debug!("Browse: a link for a new window didn't open: {e}");
            }
            NewWindowResponse::Deny
        })
        .on_download(|_, _| false)
        .on_page_load(move |_, p| {
            if p.event() == PageLoadEvent::Finished && p.url().host_str() == Some("www.roblox.com") {
                // Browsing is using RoALT: the master password's auto-lock waits.
                pages.state::<App>().vault.touch();
                let _ = pages.emit("browse://page", BrowsePage { account_id, path: path_of(p.url()) });
            }
        });
    EMBEDDED.store(false, Ordering::Release);
    let wv = window.add_child(builder, LogicalPosition::new(rect.x, rect.y), LogicalSize::new(rect.width.max(1.0), rect.height.max(1.0)))?;
    let set_up = || -> Result<()> {
        // Hidden until the UI shows it: Browse may be gone, or a dialog up, by the time this returns.
        PANE_SHOWN.store(false, Ordering::Relaxed);
        wv.hide()?;
        // The account's cookie goes in before anything loads from Roblox.
        let c = Cookie::build((".ROBLOSECURITY", cookie.expose().to_owned()))
            .domain(".roblox.com")
            .path("/")
            .secure(true)
            .http_only(true)
            .same_site(tauri::webview::cookie::SameSite::Lax)
            .build();
        wv.set_cookie(c)?;
        // A Google / Apple sign-in left in this profile goes before the page loads (the account's
        // own cookie, just put in, stays).
        webview::clear_cookies(&wv, false);
        // Out of wry's vertical box and into the overlay's fixed, where it can be placed freely.
        if let Err(e) = embed(&wv, rect) {
            // A pane that cannot be placed over the window is broken: `set_up`'s error path closes
            // it. The profile stays marked (its cookie store now holds the account's cookie), so the
            // next open or the start sweep deletes it instead of reusing it.
            log::error!("Browse: could not show the page inside the RoALT window: {e}");
            return Err(AppError::Internal("Could not show Roblox inside the RoALT window.".into()));
        }
        wv.navigate(url)?;
        Ok(())
    };
    set_up().inspect_err(|_| {
        // Half set up (no purchase lock, maybe): it goes. The profile stays marked: the next start
        // deletes it.
        let _ = wv.close();
    })?;
    Ok(Open { account_id, user_id, profile })
}

/// `browse://play`: a Play or Join on Browse's page, for RoALT to launch.
#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BrowsePlay {
    pub account_id: i64,
    pub target: crate::model::LaunchTarget,
}

/// Hands a `roblox-player:` link's target to the UI, once: a page may try twice (a frame, then the
/// top). Never logs the link: it carries a launch ticket.
fn play(app: &AppHandle, account_id: i64, uri: &str) {
    static LAST: Mutex<Option<(Instant, String)>> = Mutex::new(None);
    let Some(target) = crate::launcher::uri::target_of_player_uri(uri) else {
        log::info!("Browse: a Roblox launch link RoALT can't read was ignored");
        return;
    };
    let key = format!("{target:?}");
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if last.as_ref().is_some_and(|(at, k)| *k == key && at.elapsed() < Duration::from_secs(3)) {
        return;
    }
    *last = Some((Instant::now(), key));
    let _ = app.emit("browse://play", BrowsePlay { account_id, target });
}

/// Closes Browse (blocking: never on the main thread, where `cookies()` would deadlock), keeping
/// the cookie it ended with when it still signs in as the same user.
pub fn close(app: &AppHandle) {
    close_then(app, || {});
}

/// `close`, then `then` before Browse can open again (the vault locking).
pub fn close_then(app: &AppHandle, then: impl FnOnce()) {
    let mut slot = OPEN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(o) = slot.take() {
        end(app, o, true);
    }
    ACTIVE.store(false, Ordering::Release);
    then();
}

/// Closes Browse if it is open as `account_id`, whose row is gone: nothing of it is kept.
pub fn close_if(app: &AppHandle, account_id: i64) {
    let mut slot = OPEN.lock().unwrap_or_else(|e| e.into_inner());
    if slot.as_ref().is_some_and(|o| o.account_id == account_id) {
        if let Some(o) = slot.take() {
            end(app, o, false);
        }
        ACTIVE.store(false, Ordering::Release);
    }
}

/// Reads the cookie the page ended with (`keep`: hands it to `keep_session_cookie`), deletes it and
/// the Google / Apple sign-ins from the profile, then closes the page.
fn end(app: &AppHandle, o: Open, keep: bool) {
    // First: a late `browse_bounds`/`window_shown` during the slow cookie read must not show the
    // closing page. ACTIVE stays true so `is_open()` keeps the quit path waiting for the cookie
    // write; only CLOSING changes now.
    CLOSING.store(true, Ordering::Release);
    // A restore while this runs (its cookie check takes seconds) must not show the closing page.
    PANE_SHOWN.store(false, Ordering::Relaxed);
    if let Some(wv) = app.get_webview(LABEL) {
        let _ = wv.hide();
        if keep {
            match wv.cookies().map(webview::pick_roblosecurity) {
                Ok(Some(c)) => crate::commands::keep_session_cookie(app, o.account_id, o.user_id, c),
                Ok(None) => log::warn!("Browse closed signed out of Roblox; account {}'s cookie not changed", o.account_id),
                Err(e) => log::warn!("could not read Browse's cookies for account {}: {e}", o.account_id),
            }
        }
        // Unmarked before the page goes: once it is closed a new one can open on this profile, and
        // its mark must not be removed by this one.
        if webview::clear_cookies(&wv, true) {
            webview::mark_closed(&o.profile);
        }
        if let Err(e) = wv.close() {
            log::debug!("could not close Browse: {e}");
        }
    }
    // The page is gone: Browse is no longer open (the quit path may stop waiting) and a later open
    // is not gated by a stale closing flag.
    ACTIVE.store(false, Ordering::Release);
    CLOSING.store(false, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_stay_on_roblox_and_off_create() {
        for (path, want) in [
            ("home", "https://www.roblox.com/home"),
            ("/charts", "https://www.roblox.com/charts"),
            ("search/users?keyword=bob", "https://www.roblox.com/search/users?keyword=bob"),
            ("games/123/x", "https://www.roblox.com/games/123/x"),
        ] {
            assert_eq!(page_url(path).unwrap().as_str(), want, "{path}");
        }
        for bad in ["https://evil.com/", "develop", "create/x", "http://www.roblox.com/home", "javascript:alert(1)"] {
            assert!(page_url(bad).is_err(), "{bad}");
        }
        // The purchase lock is on by default: a money page is refused before navigation too.
        assert!(page_url("upgrades/robux").is_err(), "a money page is refused up front");
        assert_eq!(page_url("//evil.com/x").unwrap().host_str(), Some("www.roblox.com"), "a scheme-relative path stays on Roblox");
    }

    #[test]
    fn path_of_keeps_the_query() {
        assert_eq!(path_of(&Url::parse("https://www.roblox.com/discover?Keyword=obby").unwrap()), "discover?Keyword=obby");
        assert_eq!(path_of(&Url::parse("https://www.roblox.com/home").unwrap()), "home");
    }

    #[test]
    fn money_pages_are_the_real_money_ones() {
        let u = |s: &str| Url::parse(s).unwrap();
        assert!(money_page(&u("https://www.roblox.com/upgrades/robux")));
        assert!(money_page(&u("https://www.roblox.com/premium/membership")));
        assert!(money_page(&u("https://billing.roblox.com/v1/x")));
        assert!(money_page(&u("https://apis.roblox.com/payments-gateway/v1/y")));
        assert!(!money_page(&u("https://www.roblox.com/premiumfeatures")));
        assert!(!money_page(&u("https://www.roblox.com/catalog")));
    }
}
