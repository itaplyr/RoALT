//! Browse: roblox.com inside the RoALT window, as one stored account at a time. A child webview
//! (`LABEL`) of the main window on that account's kept profile (`webview::session_profile`), set
//! up as the sign-in window is (`webview::harden`: no context menu, accelerator keys or password
//! saving; the account's cookie put in before the first page loads; navigation kept on Roblox and
//! off Create, `webview::allowed`). It reaches no command: `main_only` refuses every one from a
//! webview other than the UI's. The UI places it over its own page (`browse_bounds`) and hides it
//! under its dialogs and menus. A link meant for a new window opens here; downloads are refused.
//!
//! It closes (`close`) on an account switch, `browse_close`, the vault locking, quitting and the
//! account's removal: the cookie it ended with is kept when it still signs in as the same user
//! (Roblox rotates it, a sign-in again there fixes a dead one), then deleted from the profile.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use tauri::webview::{NewWindowResponse, PageLoadEvent, WebviewBuilder};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, Webview, WebviewUrl};
use ts_rs::TS;
use url::Url;

use crate::App;
use crate::error::{AppError, Result};
use crate::vault::Secret;
use crate::webview;

const LABEL: &str = "browse";
const ROBLOX: &str = "https://www.roblox.com/";

/// Who Browse is open as, and what to wait for when it closes.
struct Open {
    account_id: i64,
    user_id: i64,
    /// Its WebView2 browser process: the profile stays locked until it has exited.
    pid: u32,
    profile: PathBuf,
}

/// Held while Browse opens or closes, so one never runs into the other.
static OPEN: Mutex<Option<Open>> = Mutex::new(None);
/// From the moment Browse starts opening until it has closed: its profile is in use.
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Browse is open, or still opening or closing.
pub fn is_open() -> bool {
    ACTIVE.load(Ordering::Acquire)
}

/// `Settings.lock_purchases`, as Browse's request handler reads it (set when Browse opens and when
/// the setting changes; on by default).
static LOCK: AtomicBool = AtomicBool::new(true);

pub fn set_lock(on: bool) {
    LOCK.store(on, Ordering::Relaxed);
}

/// What a page or request would spend.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Spend {
    /// A purchase at price 0: a free item.
    Free,
    Robux,
    /// Buying Robux or Premium, billing.
    Money,
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

/// What a request spends under the purchase lock (pure), for the account with Roblox user id `me`:
/// None for anything that spends nothing. A Robux purchase (`expectedPrice` in its body, the
/// website's own buy calls) is Free only at a price of 0. Anything else that costs Robux counts
/// whatever its body: creating or renaming a community, changing the username, a group payout, accepting a
/// trade (what it gives isn't in the call). A trade sent or countered spends when this account's
/// side offers Robux. A body it can't read counts as spending.
fn spend_of(method: &str, url: &Url, body: Option<&[u8]>, me: i64) -> Option<Spend> {
    use serde_json::Value;
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let path = url.path().to_ascii_lowercase();
    // Redeeming a gift card or a promo code adds Robux or credit: nothing is spent. Only those two
    // (turning credit into Robux, `/credit/redeem-robux`, spends).
    if host == "billing.roblox.com" && (path.ends_with("/gamecard/redeem") || path.ends_with("/promocodes/redeem")) {
        return None;
    }
    if money_page(url) {
        return Some(Spend::Money);
    }
    if !(method.eq_ignore_ascii_case("POST") || method.eq_ignore_ascii_case("PATCH")) {
        return None;
    }
    let json = || body.and_then(|b| serde_json::from_slice::<Value>(b).ok());
    match host.as_str() {
        // Renewing a private server's subscription spends each month; turning it off spends nothing.
        "games.roblox.com" if path.starts_with("/v1/vip-servers/") && path.ends_with("/subscription") => {
            let off = json().and_then(|v| v.get("active").and_then(Value::as_bool)) == Some(false);
            return Some(if off { Spend::Free } else { Spend::Robux });
        }
        // Creating a community, renaming one (100 Robux), paying out.
        "groups.roblox.com" if path == "/v1/groups/create" || path.ends_with("/name") || path.contains("/payouts") => return Some(Spend::Robux),
        "auth.roblox.com" if path.ends_with("/username") => return Some(Spend::Robux),
        "trades.roblox.com" if path.ends_with("/accept") => return Some(Spend::Robux),
        "trades.roblox.com" if path.ends_with("/send") || path.ends_with("/counter") => {
            // v2 names this account's side `senderOffer`; v1 lists both sides by user id.
            let mine = |v: Value| -> Option<i64> {
                if let Some(o) = v.get("senderOffer") {
                    return o.get("robux").and_then(Value::as_i64);
                }
                let offers = v.get("offers")?.as_array()?;
                offers.iter().find(|o| o.get("userId").and_then(Value::as_i64) == Some(me))?.get("robux").and_then(Value::as_i64)
            };
            // Only a readable offer of 0 Robux from this account passes.
            return if json().and_then(mine) == Some(0) { None } else { Some(Spend::Robux) };
        }
        _ => {}
    }
    let buys = matches!(host.as_str(), "economy.roblox.com" | "apis.roblox.com" | "catalog.roblox.com" | "games.roblox.com")
        && (path.contains("purchase") || (host == "games.roblox.com" && path.starts_with("/v1/games/vip-servers/")));
    if !buys {
        return None;
    }
    let price = json().and_then(|v| v.get("expectedPrice").and_then(Value::as_i64));
    Some(if price == Some(0) { Spend::Free } else { Spend::Robux })
}

/// Roblox's Log Out (pure): it ends the very session RoALT keeps for the account (Browse runs on
/// the stored cookie), so it is never sent from Browse, lock or not.
fn signs_out(method: &str, url: &Url) -> bool {
    method.eq_ignore_ascii_case("POST")
        && url.host_str().is_some_and(|h| h.eq_ignore_ascii_case("auth.roblox.com"))
        && matches!(url.path().to_ascii_lowercase().as_str(), "/v1/logout" | "/v2/logout")
}

/// `browse://blocked`: what Browse just refused, for the UI to say.
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

/// The card RoALT shows on a player's profile in Browse (`browse_card.js`).
const CARD_JS: &str = include_str!("browse_card.js");

/// What the profile card shows, in the UI's language: label/value rows, a note, the most
/// valuable limiteds. Only text: the page script puts it in with `textContent`.
#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PageCard {
    pub user_id: i64,
    pub title: String,
    /// The collapse button's label.
    pub toggle: String,
    pub rows: Vec<(String, String)>,
    pub note: Option<String>,
    pub top_title: String,
    pub top: Vec<(String, String)>,
}

impl PageCard {
    /// Kept small whatever the UI sends.
    fn clipped(mut self) -> Self {
        let cut = |s: &mut String| *s = s.chars().take(120).collect();
        self.rows.truncate(20);
        self.top.truncate(10);
        for (a, b) in self.rows.iter_mut().chain(self.top.iter_mut()) {
            cut(a);
            cut(b);
        }
        for s in [&mut self.title, &mut self.toggle, &mut self.top_title] {
            cut(s);
        }
        if let Some(n) = &mut self.note {
            cut(n);
        }
        self
    }
}

/// Shows `card` on Browse's profile page (None: takes it away). Ignored when Browse is closed,
/// or the page shows another profile by now (the script checks).
#[tauri::command]
pub fn browse_card(app: AppHandle, card: Option<PageCard>) -> Result<()> {
    let Some(wv) = app.get_webview(LABEL) else { return Ok(()) };
    let json = serde_json::to_string(&card.map(PageCard::clipped)).map_err(|e| AppError::Internal(e.to_string()))?;
    // JSON is a JS expression: no string of the card's can become code.
    wv.eval(format!("window.__roaltCard&&window.__roaltCard({json})"))?;
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

/// A path on www.roblox.com as a page Browse may show (pure): the same host, and `allowed`
/// (never Create).
fn page_url(path: &str) -> Result<Url> {
    let refused = || AppError::Invalid(tr!("That page isn't available in Browse."));
    let url = Url::parse(ROBLOX).and_then(|base| base.join(path.trim_start_matches('/'))).map_err(|_| refused())?;
    if url.host_str() != Some("www.roblox.com") || !webview::allowed(&url) {
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
    let url = page_url(&path)?;
    let a = s.db.account(account_id)?;
    let cookie = crate::launcher::account_cookie(&s, account_id)?;
    let data_dir = s.data_dir.clone();
    // Off the async threads: making the webview waits for the main thread, closing one waits for
    // its browser process.
    tauri::async_runtime::spawn_blocking(move || open(&app, &data_dir, a.id, a.user_id, cookie, url, rect)).await?
}

/// The UI shows Browse's page (`browse_bounds`). The main window going to the tray or minimizing
/// hides the page too; coming back shows it again only if so.
static PANE_SHOWN: AtomicBool = AtomicBool::new(false);

/// Moves Browse's page to `rect`, and shows or hides it (under a dialog, on another view).
#[tauri::command]
pub fn browse_bounds(app: AppHandle, rect: PaneRect, shown: bool) -> Result<()> {
    let Some(wv) = app.get_webview(LABEL) else { return Ok(()) };
    place(&wv, rect)?;
    PANE_SHOWN.store(shown, Ordering::Relaxed);
    if shown && crate::main_shown() { wv.show()? } else { wv.hide()? }
    Ok(())
}

/// The main window went to the tray or minimized (`shown` false), or is back.
pub fn window_shown(app: &AppHandle, shown: bool) {
    let Some(wv) = app.get_webview(LABEL) else { return };
    if let Err(e) = if shown && PANE_SHOWN.load(Ordering::Relaxed) { wv.show() } else { wv.hide() } {
        log::debug!("Browse: could not follow the main window: {e}");
    }
    webview::memory_target(&wv, !shown);
}

#[tauri::command]
pub fn browse_nav(app: AppHandle, action: BrowseNav) -> Result<()> {
    let Some(wv) = app.get_webview(LABEL) else { return Ok(()) };
    wv.with_webview(move |pv| {
        // SAFETY: main thread (with_webview), live controller.
        let r = unsafe {
            pv.controller().CoreWebView2().and_then(|core| match action {
                BrowseNav::Back => core.GoBack(),
                BrowseNav::Forward => core.GoForward(),
                BrowseNav::Reload => core.Reload(),
            })
        };
        if let Err(e) = r {
            log::debug!("Browse: {action:?} failed: {e}");
        }
    })?;
    Ok(())
}

/// Closes Browse, keeping the cookie it ended with (`close`).
#[tauri::command]
pub async fn browse_close(app: AppHandle) -> Result<()> {
    tauri::async_runtime::spawn_blocking(move || close(&app)).await?;
    Ok(())
}

fn place(wv: &Webview, rect: PaneRect) -> Result<()> {
    wv.set_position(LogicalPosition::new(rect.x, rect.y))?;
    wv.set_size(LogicalSize::new(rect.width.max(1.0), rect.height.max(1.0)))?;
    Ok(())
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
        place(&wv, rect)?;
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
    let (links, pages, refused) = (app.clone(), app.clone(), app.clone());
    // Blank first, so nothing loads from Roblox before the cookie is in place.
    let builder = WebviewBuilder::new(LABEL, WebviewUrl::External(webview::parse_url("about:blank")?))
        .data_directory(profile.clone())
        .incognito(false)
        .devtools(cfg!(debug_assertions))
        .general_autofill_enabled(false)
        .on_navigation(move |url| {
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
            // A new window or tab would be out of RoALT's hands: the link opens here instead.
            if webview::allowed(&url)
                && let Some(wv) = links.get_webview(LABEL)
                && let Err(e) = wv.navigate(url)
            {
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
    let wv = window.add_child(builder, LogicalPosition::new(rect.x, rect.y), LogicalSize::new(rect.width.max(1.0), rect.height.max(1.0)))?;
    let set_up = || -> Result<u32> {
        // Hidden until the UI shows it: Browse may be gone, or a dialog up, by the time this returns.
        PANE_SHOWN.store(false, Ordering::Relaxed);
        wv.hide()?;
        // On this blocking thread: not an async one, so waiting here is allowed.
        let card = zeroize::Zeroizing::new(CARD_JS.to_owned());
        let pid = tauri::async_runtime::block_on(webview::harden(&wv, Some(cookie), Some(card)))?;
        // A Google / Apple sign-in left in this profile goes before the page loads (the account's
        // own cookie, just put in, stays).
        webview::clear_cookies(&wv, false);
        watch_play(&wv, app.clone(), account_id)?;
        watch_page(&wv, app.clone(), account_id)?;
        guard_purchases(&wv, app.clone(), user_id)?;
        wv.navigate(url)?;
        Ok(pid)
    };
    let pid = set_up().inspect_err(|_| {
        // Half set up (no purchase lock, maybe): it goes. The profile stays marked: the next start
        // deletes it.
        let _ = wv.close();
    })?;
    watch_cookie(app.clone(), account_id, user_id, pid);
    Ok(Open { account_id, user_id, pid, profile })
}

/// Play or Join on a Roblox page starts the client through a `roblox-player:` link: Browse never
/// lets it out (the system would start Roblox past RoALT, with the page's own ticket). It is
/// cancelled, and its target goes to the UI (`browse://play`), which launches the account through
/// RoALT. Top-frame links show in NavigationStarting (which `harden` cancels), others in
/// LaunchingExternalUriScheme (WebView2 1.0.1518+), which also keeps every other app's link in.
fn watch_play(wv: &Webview, app: AppHandle, account_id: i64) -> Result<()> {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_18;
    use webview2_com::{LaunchingExternalUriSchemeEventHandler, NavigationStartingEventHandler, take_pwstr};
    use windows::core::{Interface, PWSTR};
    wv.with_webview(move |pv| {
        let (top, other) = (app.clone(), app);
        // SAFETY: main thread (with_webview), live controller; the handlers get live args, and
        // `take_pwstr` frees the strings they hand out.
        let r = unsafe {
            pv.controller().CoreWebView2().and_then(|core| {
                let mut token = Default::default();
                let nav = NavigationStartingEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args {
                        let mut uri = PWSTR::null();
                        args.Uri(&mut uri)?;
                        play(&top, account_id, &take_pwstr(uri));
                    }
                    Ok(())
                }));
                core.add_NavigationStarting(&nav, &mut token)?;
                let ext = LaunchingExternalUriSchemeEventHandler::create(Box::new(move |_, args| {
                    if let Some(args) = args {
                        args.SetCancel(true)?;
                        let mut uri = PWSTR::null();
                        args.Uri(&mut uri)?;
                        play(&other, account_id, &take_pwstr(uri));
                    }
                    Ok(())
                }));
                core.cast::<ICoreWebView2_18>()?.add_LaunchingExternalUriScheme(&ext, &mut token)
            })
        };
        if let Err(e) = r {
            log::warn!("Browse: Play buttons in frames won't launch (WebView2 too old?): {e}");
        }
    })?;
    Ok(())
}

/// Roblox moves between many of its pages in place (a search result to a profile): no page load
/// says so, only WebView2's SourceChanged. Each new address goes to the UI as `browse://page`, as a
/// finished load does, so the toolbar knows a game or profile is up.
fn watch_page(wv: &Webview, app: AppHandle, account_id: i64) -> Result<()> {
    use webview2_com::{SourceChangedEventHandler, take_pwstr};
    use windows::core::PWSTR;
    wv.with_webview(move |pv| {
        // SAFETY: main thread (with_webview), live controller; the handler gets the live webview,
        // and `take_pwstr` frees the string it hands out.
        let r = unsafe {
            pv.controller().CoreWebView2().and_then(|core| {
                let mut token = Default::default();
                let changed = SourceChangedEventHandler::create(Box::new(move |sender, _| {
                    if let Some(core) = sender {
                        let mut uri = PWSTR::null();
                        core.Source(&mut uri)?;
                        if let Ok(url) = Url::parse(&take_pwstr(uri))
                            && url.host_str() == Some("www.roblox.com")
                        {
                            let _ = app.emit("browse://page", BrowsePage { account_id, path: path_of(&url) });
                        }
                    }
                    Ok(())
                }));
                core.add_SourceChanged(&changed, &mut token)
            })
        };
        if let Err(e) = r {
            log::warn!("Browse: pages Roblox opens in place won't show their details button: {e}");
        }
    })?;
    Ok(())
}

/// The purchase lock on Browse's page: while `LOCK` is on, a request that would spend (`spend_of`,
/// for user `me`) is answered with an empty 403 before it leaves, so Roblox's page shows its own
/// "failed". Log Out (`signs_out`) is answered so always. Only the hosts Roblox spends or signs
/// out through are watched; a body is read only for POST and PATCH while the lock is on.
fn guard_purchases(wv: &Webview, app: AppHandle, me: i64) -> Result<()> {
    use webview2_com::Microsoft::Web::WebView2::Win32::{COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL, ICoreWebView2_2};
    use webview2_com::{WebResourceRequestedEventHandler, take_pwstr};
    use windows::core::{HSTRING, Interface, PWSTR, w};
    wv.with_webview(move |pv| {
        // SAFETY: main thread (with_webview), live controller; the handler gets live args, and
        // `take_pwstr` frees the strings it hands out.
        let r = unsafe {
            pv.controller().CoreWebView2().and_then(|core| {
                let env = core.cast::<ICoreWebView2_2>()?.Environment()?;
                // Not www: its pages are refused before they load (`on_navigation`), and it serves most requests.
                for host in ["economy", "apis", "games", "catalog", "billing", "groups", "auth", "trades"] {
                    core.AddWebResourceRequestedFilter(&HSTRING::from(format!("https://{host}.roblox.com/*")), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)?;
                }
                let handler = WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let req = args.Request()?;
                    let (mut m, mut u) = (PWSTR::null(), PWSTR::null());
                    req.Method(&mut m)?;
                    req.Uri(&mut u)?;
                    let (method, uri) = (take_pwstr(m), take_pwstr(u));
                    let Ok(url) = Url::parse(&uri) else { return Ok(()) };
                    let refused = if signs_out(&method, &url) {
                        Some(BrowseBlocked::SignOut)
                    } else if LOCK.load(Ordering::Relaxed) {
                        let posts = method.eq_ignore_ascii_case("POST") || method.eq_ignore_ascii_case("PATCH");
                        let body = if posts { req.Content().ok().map(|s| read_stream(&s)) } else { None };
                        match spend_of(&method, &url, body.as_deref(), me) {
                            Some(Spend::Robux) => Some(BrowseBlocked::Robux),
                            Some(Spend::Money) => Some(BrowseBlocked::Money),
                            Some(Spend::Free) | None => None,
                        }
                    } else {
                        None
                    };
                    if let Some(why) = refused {
                        let refusal = env.CreateWebResourceResponse(None, 403, w!("Forbidden"), w!("Content-Type: application/json"))?;
                        args.SetResponse(&refusal)?;
                        log::info!("Browse: refused {} {} ({why:?})", method, url.path());
                        let _ = app.emit("browse://blocked", why);
                    }
                    Ok(())
                }));
                let mut token = Default::default();
                core.add_WebResourceRequested(&handler, &mut token)
            })
        };
        if let Err(e) = r {
            log::warn!("Browse: the purchase lock couldn't be set up: {e}");
        }
    })?;
    Ok(())
}

/// A request body, up to 64 KB (a buy call is a few hundred bytes), rewound after for the request.
///
/// # Safety
/// A live stream, on the thread WebView2 handed it out on.
unsafe fn read_stream(s: &windows::Win32::System::Com::IStream) -> Vec<u8> {
    use windows::Win32::System::Com::STREAM_SEEK_SET;
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    // SAFETY: as the function's contract; `buf` outlives each call that fills it.
    unsafe {
        while out.len() < 64 * 1024 {
            let mut read = 0u32;
            if s.Read(buf.as_mut_ptr().cast(), buf.len() as u32, Some(&mut read)).is_err() || read == 0 {
                break;
            }
            out.extend_from_slice(&buf[..read as usize]);
        }
        let _ = s.Seek(0, STREAM_SEEK_SET, None);
    }
    out
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
    static LAST: Mutex<Option<(std::time::Instant, String)>> = Mutex::new(None);
    if !uri.get(..14).is_some_and(|p| p.eq_ignore_ascii_case("roblox-player:")) {
        return;
    }
    let Some(target) = crate::launcher::uri::target_of_player_uri(uri) else {
        log::info!("Browse: a Roblox launch link RoALT can't read was ignored");
        return;
    };
    let key = format!("{target:?}");
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if last.as_ref().is_some_and(|(at, k)| *k == key && at.elapsed() < std::time::Duration::from_secs(3)) {
        return;
    }
    *last = Some((std::time::Instant::now(), key));
    let _ = app.emit("browse://play", BrowsePlay { account_id, target });
}

/// While this page stays open: once a minute, a cookie that changed (Roblox rotated it, or the
/// user signed in again there) is kept at once (`keep_session_cookie`), not only when it closes.
fn watch_cookie(app: AppHandle, account_id: i64, user_id: i64, pid: u32) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(60));
            // Closed, or open again as another account or page since: this watch is over.
            let still = OPEN.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|o| o.pid == pid && o.account_id == account_id);
            let Some(wv) = app.get_webview(LABEL).filter(|_| still) else { return };
            let Ok(Some(now)) = wv.cookies().map(webview::pick_roblosecurity) else { continue };
            let s = app.state::<App>();
            let stored = crate::launcher::account_cookie(&s, account_id);
            if !stored.is_ok_and(|c| c.expose() == now.expose()) && !s.vault.is_locked() {
                crate::commands::keep_session_cookie(&app, account_id, user_id, now);
            }
        }
    });
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

/// Reads the cookie the page ended with (`keep`: hands it to `keep_session_cookie`), deletes it
/// from the profile, closes the page and waits for its browser to exit.
fn end(app: &AppHandle, o: Open, keep: bool) {
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
    webview::wait_for_exit(o.pid);
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
        assert_eq!(page_url("//evil.com/x").unwrap().host_str(), Some("www.roblox.com"), "a scheme-relative path stays on Roblox");
    }

    #[test]
    fn the_purchase_lock_stops_spending_not_free_items() {
        const ME: i64 = 42;
        let spend = |m: &str, url: &str, body: Option<&[u8]>| spend_of(m, &Url::parse(url).unwrap(), body, ME);
        let buy = "https://economy.roblox.com/v1/purchases/products/123";
        assert_eq!(spend("POST", buy, Some(br#"{"expectedCurrency":1,"expectedPrice":0,"expectedSellerId":1}"#)), Some(Spend::Free));
        assert_eq!(spend("POST", buy, Some(br#"{"expectedCurrency":1,"expectedPrice":75}"#)), Some(Spend::Robux));
        assert_eq!(spend("POST", buy, None), Some(Spend::Robux), "no body read: spending");
        assert_eq!(spend("POST", buy, Some(b"not json")), Some(Spend::Robux));
        let item = "https://apis.roblox.com/marketplace-purchasing/v1/items/abc/purchase-item";
        assert_eq!(spend("POST", item, Some(br#"{"expectedPrice":0}"#)), Some(Spend::Free));
        assert_eq!(spend("POST", item, Some(br#"{"expectedPrice":20}"#)), Some(Spend::Robux));
        assert_eq!(spend("POST", "https://economy.roblox.com/v2/user-products/9/purchase", Some(br#"{"expectedPrice":500}"#)), Some(Spend::Robux));
        assert_eq!(spend("POST", "https://games.roblox.com/v1/games/vip-servers/42", Some(br#"{"name":"x","expectedPrice":100}"#)), Some(Spend::Robux));
        let sub = "https://games.roblox.com/v1/vip-servers/7/subscription";
        assert_eq!(spend("PATCH", sub, Some(br#"{"active":true,"price":100}"#)), Some(Spend::Robux));
        assert_eq!(spend("PATCH", sub, Some(br#"{"active":false}"#)), Some(Spend::Free));
        // Robux spent without a buy call: a community, a new username, a payout, a trade.
        assert_eq!(spend("POST", "https://groups.roblox.com/v1/groups/create", Some(b"--multipart--")), Some(Spend::Robux));
        assert_eq!(spend("PATCH", "https://groups.roblox.com/v1/groups/9/name", Some(br#"{"name":"x"}"#)), Some(Spend::Robux), "a rename costs 100");
        assert_eq!(spend("PATCH", "https://groups.roblox.com/v1/groups/9/description", Some(b"{}")), None);
        assert_eq!(spend("POST", "https://groups.roblox.com/v1/groups/9/payouts", None), Some(Spend::Robux));
        assert_eq!(spend("POST", "https://groups.roblox.com/v1/groups/9/payouts/recurring", None), Some(Spend::Robux));
        assert_eq!(spend("POST", "https://auth.roblox.com/v2/username", Some(br#"{"username":"x","password":"y"}"#)), Some(Spend::Robux));
        assert_eq!(spend("POST", "https://auth.roblox.com/v1/usernames/validate", Some(b"{}")), None);
        assert_eq!(spend("POST", "https://trades.roblox.com/v1/trades/5/accept", None), Some(Spend::Robux), "what an accept gives isn't in the call");
        let v1 = |mine: i64, theirs: i64| format!(r#"{{"offers":[{{"userId":7,"userAssetIds":[],"robux":{theirs}}},{{"userId":{ME},"userAssetIds":[1],"robux":{mine}}}]}}"#);
        assert_eq!(spend("POST", "https://trades.roblox.com/v1/trades/send", Some(v1(50, 0).as_bytes())), Some(Spend::Robux));
        assert_eq!(spend("POST", "https://trades.roblox.com/v1/trades/send", Some(v1(0, 50).as_bytes())), None, "only Robux this account gives");
        assert_eq!(spend("POST", "https://trades.roblox.com/v1/trades/5/counter", Some(v1(10, 0).as_bytes())), Some(Spend::Robux));
        assert_eq!(spend("POST", "https://trades.roblox.com/v2/trades/send", Some(br#"{"senderOffer":{"robux":0},"recipientOffer":{"robux":99}}"#)), None);
        assert_eq!(spend("POST", "https://trades.roblox.com/v2/trades/send", Some(br#"{"senderOffer":{"robux":5}}"#)), Some(Spend::Robux));
        assert_eq!(spend("POST", "https://trades.roblox.com/v1/trades/send", Some(b"?")), Some(Spend::Robux), "unreadable: spending");
        for page in ["https://www.roblox.com/upgrades/robux", "https://www.roblox.com/premium/membership", "https://billing.roblox.com/v1/x", "https://apis.roblox.com/payments-gateway/v1/y"] {
            assert_eq!(spend("GET", page, None), Some(Spend::Money), "{page}");
        }
        assert_eq!(spend("POST", "https://billing.roblox.com/v1/gamecard/redeem", Some(br#"{"pinCode":"1"}"#)), None, "a gift card adds, spends nothing");
        assert_eq!(spend("POST", "https://billing.roblox.com/v1/promocodes/redeem", Some(b"{}")), None);
        assert_eq!(spend("POST", "https://billing.roblox.com/v1/credit/redeem-robux", Some(b"{}")), Some(Spend::Money), "credit into Robux spends it");
        assert_eq!(spend("GET", buy, None), None, "only buying spends");
        assert_eq!(spend("POST", "https://friends.roblox.com/v1/users/1/request-friendship", None), None);
        assert_eq!(spend("POST", "https://apis.roblox.com/universes/v1/x", Some(b"{}")), None);
        let u = |s: &str| Url::parse(s).unwrap();
        assert!(!money_page(&u("https://www.roblox.com/premiumfeatures")) && !money_page(&u("https://www.roblox.com/catalog")));
        assert!(signs_out("POST", &u("https://auth.roblox.com/v2/logout")) && signs_out("post", &u("https://auth.roblox.com/v1/logout")));
        assert!(!signs_out("GET", &u("https://auth.roblox.com/v2/logout")) && !signs_out("POST", &u("https://auth.roblox.com/v2/login")));
    }

    #[test]
    fn path_of_keeps_the_query() {
        assert_eq!(path_of(&Url::parse("https://www.roblox.com/discover?Keyword=obby").unwrap()), "discover?Keyword=obby");
        assert_eq!(path_of(&Url::parse("https://www.roblox.com/home").unwrap()), "home");
    }
}
