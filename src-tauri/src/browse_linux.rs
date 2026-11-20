//! Browse on Linux: the fork's kept-profile session window. Upstream's Browse is a WebView2 child
//! webview (`src/browse.rs`, Windows only); WebKitGTK has no equivalent harden path here, so
//! `browse_open` opens the account's roblox.com session in its own kept profile window instead
//! (`webview::open_session`). The page geometry, profile card and page events have no meaning for
//! a separate window, so those commands are no-ops.
//!
//! The types mirror `browse.rs` exactly (same `ts-rs` names) so `npm run bindings` on Linux still
//! generates them for the frontend.
#![allow(dead_code)]

use std::sync::atomic::{AtomicI64, Ordering};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use ts_rs::TS;

use crate::App;
use crate::error::{AppError, Result};

/// The account whose session window Browse opened, or -1: its label is `session-<id>`.
static OPEN_ACCOUNT: AtomicI64 = AtomicI64::new(-1);

/// Browse is open while one of these session windows is.
pub fn is_open() -> bool {
    OPEN_ACCOUNT.load(Ordering::Acquire) >= 0
}

/// `Settings.lock_purchases`: kept for the shared call sites.
pub fn set_lock(_on: bool) {}

/// `browse://blocked`: what Browse refused, for the UI to say (same shape as upstream).
#[derive(Serialize, TS, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BrowseBlocked {
    Create,
    Robux,
    Money,
    SignOut,
}

/// Where Browse's page goes, in the main window's logical pixels.
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
    pub path: String,
}

/// What the profile card shows (same shape as upstream).
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

/// Browse's page buttons.
#[derive(Deserialize, TS, Clone, Copy, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BrowseNav {
    Back,
    Forward,
    Reload,
}

/// `browse://play`: a Play or Join on Browse's page, for RoALT to launch.
#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BrowsePlay {
    pub account_id: i64,
    pub target: crate::model::LaunchTarget,
}

/// A separate window has no profile card; the UI's call succeeds and shows nothing.
#[tauri::command]
pub fn browse_card(_app: AppHandle, _card: Option<PageCard>) -> Result<()> {
    Ok(())
}

/// Opens `account_id`'s roblox.com session in its kept profile window (the Linux form of Browse).
#[tauri::command]
pub async fn browse_open(
    app: AppHandle,
    s: State<'_, App>,
    account_id: i64,
    _path: String,
    _rect: PaneRect,
) -> Result<()> {
    if s.vault.is_locked() {
        return Err(AppError::Locked);
    }
    let a = s.db.account(account_id)?;
    let cookie = crate::launcher::account_cookie(&s, account_id)?;
    let name = if a.alias.is_empty() { a.display_name.clone() } else { a.alias.clone() };
    let title = format!("{name} (@{}) — Roblox — RoALT", a.username);
    let user_id = a.user_id;
    OPEN_ACCOUNT.store(account_id, Ordering::Release);
    let handle = app.clone();
    let result = crate::webview::open_session(&app, &s.data_dir, account_id, &title, cookie, move |last| {
        OPEN_ACCOUNT.store(-1, Ordering::Release);
        if let Some(last) = last {
            crate::commands::keep_session_cookie(&handle, account_id, user_id, last);
        }
    })
    .await;
    if result.is_err() {
        OPEN_ACCOUNT.store(-1, Ordering::Release);
    }
    result
}

/// A separate window places itself.
#[tauri::command]
pub fn browse_bounds(_app: AppHandle, _rect: PaneRect, _shown: bool) -> Result<()> {
    Ok(())
}

pub fn window_shown(_app: &AppHandle, _shown: bool) {}

/// A separate window has its own navigation; the UI's call succeeds and does nothing.
#[tauri::command]
pub fn browse_nav(_app: AppHandle, _action: BrowseNav) -> Result<()> {
    Ok(())
}

#[tauri::command]
pub async fn browse_close(app: AppHandle) -> Result<()> {
    close(&app);
    Ok(())
}

/// Closes the session window Browse opened, if any.
pub fn close(app: &AppHandle) {
    use tauri::Manager;
    let id = OPEN_ACCOUNT.swap(-1, Ordering::AcqRel);
    if id >= 0
        && let Some(w) = app.get_webview_window(&format!("session-{id}"))
        && let Err(e) = w.close()
    {
        log::debug!("could not close Browse's session window for account {id}: {e}");
    }
    // Its close keeps the account's newest cookie: let it finish before Browse can open again.
    if id >= 0 {
        crate::webview::wait_sessions_closed(app);
    }
}

pub fn close_then(app: &AppHandle, then: impl FnOnce()) {
    close(app);
    then();
}

pub fn close_if(app: &AppHandle, account_id: i64) {
    if OPEN_ACCOUNT.load(Ordering::Acquire) == account_id {
        close(app);
    }
}
