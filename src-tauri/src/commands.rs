//! The IPC surface. Each command validates input, calls the modules and emits change events.

use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
#[cfg(windows)]
use tauri::{WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::error::{AppError, Result};
use crate::model::*;
use crate::storage::{AccountMeta, NewAccount};
use crate::vault::backup::{self, BackupAccount, BackupGroup, BackupPayload};
use crate::vault::{Secret, cookie_aad};
use crate::roblox_api::types::{AuthenticatedUser, GameDetails, Holding, UserPresence};
use crate::{App, webview};

pub fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn accounts_changed(app: &AppHandle) {
    let _ = app.emit("accounts://changed", ());
}

// ---------- vault ----------

// The lists the UI reads on every refresh are async (here and below): a sync command runs on the main
// thread, and one waiting on the DB lock (a backup, a tracker write) would freeze the window.
#[tauri::command(async)]
pub fn vault_status(s: State<'_, App>) -> Result<VaultStatus> {
    Ok(VaultStatus {
        mode: s.vault.mode(),
        locked: s.vault.is_locked(),
        accounts: s.db.accounts()?.len() as u32,
        auto_lock_minutes: s.db.settings()?.auto_lock_minutes,
        has_recovery: s.db.recovery_record()?.is_some(),
    })
}

#[tauri::command]
pub async fn vault_unlock(app: AppHandle, s: State<'_, App>, password: String) -> Result<()> {
    let rec = s.db.vault_record()?.ok_or(AppError::Internal("vault record missing".into()))?;
    let handle = app.clone();
    blocking(move || handle.state::<App>().vault.unlock(&rec, &password)).await??;
    after_unlock(&app, &s);
    Ok(())
}

fn after_unlock(app: &AppHandle, s: &App) {
    let later = std::mem::take(&mut *s.rotated_later.lock().unwrap_or_else(|e| e.into_inner()));
    later.into_iter().for_each(|(id, cookie)| s.store_rotated(id, cookie));
    crate::tray::refresh(app);
}

/// Forgot the password: `code` sets `new_password` and unlocks. The code is used up; returns the
/// fresh one, which the UI shows once. A vault without a code answers like a wrong code.
#[tauri::command]
pub async fn vault_recover(app: AppHandle, s: State<'_, App>, code: String, new_password: String) -> Result<String> {
    let recovery = s.db.recovery_record()?.ok_or(AppError::WrongPassword)?;
    let handle = app.clone();
    let new_code = blocking(move || -> Result<String> {
        let s = handle.state::<App>();
        let (rec, new_code, new_recovery) = crate::vault::recover(&recovery, &code, &new_password)?;
        s.db.save_vault_and_recovery(&rec, &new_recovery)?;
        s.vault.unlock(&rec, &new_password)?;
        Ok(new_code.expose().to_owned())
    })
    .await??;
    after_unlock(&app, &s);
    Ok(new_code)
}

/// A new recovery code, shown once by the UI. Replaces (revokes) the one before.
#[tauri::command]
pub async fn vault_recovery_create(app: AppHandle, s: State<'_, App>, current_password: String) -> Result<String> {
    let rec = s.db.vault_record()?.ok_or(AppError::Internal("vault record missing".into()))?;
    if rec.mode != VaultMode::Master {
        return Err(AppError::Invalid(tr!("No master password is set.")));
    }
    let handle = app.clone();
    blocking(move || -> Result<String> {
        let s = handle.state::<App>();
        s.vault.check_password(&rec, &current_password)?;
        let (code, recovery) = s.vault.seal_recovery()?;
        s.db.save_recovery_record(&recovery)?;
        Ok(code.expose().to_owned())
    })
    .await?
}

/// What a saved code file says. Refuses anything but a code, so the command that writes it can't
/// be used to put other content in a file.
fn recovery_file_text(code: &str) -> Result<String> {
    let code = crate::vault::master::normalize_code(code).ok_or_else(not_a_code)?;
    Ok(tr!(
        "RoALT recovery code: {code}\n\nIf you forget your master password, press Forgot password? on the unlock screen and enter this code. \
         It works once, for this Windows user on this PC.\n\nAnyone who finds this file on this PC can open your vault. Keep it on a USB stick, \
         a phone or paper.\n",
        code = code.expose()
    ))
}

fn not_a_code() -> AppError {
    AppError::Invalid(tr!("That isn't a recovery code."))
}

#[tauri::command]
pub async fn vault_recovery_save(path: String, code: String) -> Result<()> {
    crate::vault::backup::write_atomic(Path::new(&path), recovery_file_text(&code)?.as_bytes())
}

/// Copies a code as `account_copy_cookie` does a cookie: not into Windows clipboard history, and
/// cleared again.
#[tauri::command]
pub async fn vault_recovery_copy(app: AppHandle, s: State<'_, App>, code: String) -> Result<()> {
    let code = crate::vault::master::normalize_code(&code).ok_or_else(not_a_code)?;
    copy_secret(&app, &s, code)
}

/// Returns once locked: the UI shows the lock screen then, and an unlock can't come in between.
#[tauri::command]
pub async fn vault_lock(app: AppHandle) -> Result<()> {
    if app.state::<App>().vault.mode() == VaultMode::Master {
        blocking(move || lock_vault_now(&app)).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn vault_set_mode(
    app: AppHandle,
    s: State<'_, App>,
    mode: VaultMode,
    current_password: Option<String>,
    new_password: Option<String>,
) -> Result<()> {
    let rec = s.db.vault_record()?.ok_or(AppError::Internal("vault record missing".into()))?;
    // Otherwise setting a master password would be a way round the Hello check on reveal.
    if rec.mode == VaultMode::Dpapi && mode == VaultMode::Master && crate::vault::hello_available().await {
        reauth(&app, &s, None, &tr!("Confirm it's you to set a master password.")).await?;
    }
    let handle = app.clone();
    blocking(move || -> Result<()> {
        let s = handle.state::<App>();
        if rec.mode == VaultMode::Dpapi && mode == VaultMode::Master {
            // Rotate the 256-bit data key when upgrading from DPAPI to Master so a previously
            // captured DPAPI wrap can no longer decrypt secrets after the switch.
            let (new_key, new_rec) = s.vault.prepare_master_rotation(new_password.as_deref())?;
            let cookies: Vec<(i64, Vec<u8>)> = s
                .db
                .all_account_cookies()?
                .into_iter()
                .map(|(id, user_id, blob)| Ok((id, s.vault.reencrypt_with_key(&new_key, &cookie_aad(user_id), &blob)?)))
                .collect::<Result<_>>()?;
            let favourites: Vec<(i64, Vec<u8>)> = s
                .db
                .favourites()?
                .into_iter()
                .map(|row| Ok((row.id, s.vault.reencrypt_with_key(&new_key, FAVOURITE_AAD, &row.target)?)))
                .collect::<Result<_>>()?;
            let webhook_hex = crate::discord::reencrypt_webhook(&s, &new_key)?;
            s.db.commit_key_rotation(&new_rec, &cookies, &favourites, webhook_hex.as_deref())?;
            s.vault.commit_rotation(new_key, VaultMode::Master);
        } else {
            let new_rec = s.vault.rewrap(&rec, mode, current_password.as_deref(), new_password.as_deref())?;
            s.db.save_vault_record(&new_rec)?;
            s.vault.set_mode(mode);
        }
        Ok(())
    })
    .await??;
    crate::tray::refresh(&app);
    Ok(())
}

/// Called by the UI (throttled) on user input; drives auto-lock.
#[tauri::command]
pub fn app_activity(s: State<'_, App>) {
    s.vault.touch();
}

// ---------- accounts ----------

#[tauri::command(async)]
pub fn accounts_list(s: State<'_, App>) -> Result<Vec<AccountSummary>> {
    s.db.accounts()
}

/// Accepts a raw value, `.ROBLOSECURITY=value`, quoted or with a trailing `;`.
fn normalize_cookie(raw: &str) -> Result<Secret> {
    let v = raw.trim().trim_matches(|c| c == '"' || c == '\'').trim();
    let v = v.strip_prefix(".ROBLOSECURITY=").unwrap_or(v).trim_end_matches(';').trim();
    if v.is_empty() || v.len() > 8192 || !v.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(AppError::Invalid(tr!("That doesn't look like a .ROBLOSECURITY cookie.")));
    }
    Ok(Secret::new(v))
}

/// Validates a cookie that is not stored yet and saves it as a new or refreshed account.
async fn add_account(app: &AppHandle, s: &App, cookie: Secret) -> Result<AccountSummary> {
    let (user, rotated): (AuthenticatedUser, _) = s.api.authenticated_user(0, &cookie).await?;
    let cookie = rotated.unwrap_or(cookie);
    let blob = s.vault.encrypt(&cookie_aad(user.id), cookie.expose().as_bytes())?;
    let id = s.db.upsert_account(&NewAccount {
        user_id: user.id,
        username: &user.name,
        display_name: &user.display_name,
        cookie: &blob,
        status: AccountStatus::Valid,
        checked_at: Some(now_ms()),
    })?;
    #[cfg(not(windows))]
    crate::launcher::client::ensure_profile(user.id);
    accounts_changed(app);
    s.db.account(id)
}

#[tauri::command]
pub async fn account_add_cookie(app: AppHandle, s: State<'_, App>, cookie: String) -> Result<AccountSummary> {
    unlocked(&s)?;
    add_account(&app, &s, normalize_cookie(&cookie)?).await
}

/// Signs in through Roblox's own page. `username` and `password` (a bulk import's `user:pass`
/// line, both or neither) are filled in for the user, who still presses Log In; never stored.
#[tauri::command]
pub async fn account_add_login(app: AppHandle, s: State<'_, App>, username: Option<String>, password: Option<String>) -> Result<AccountSummary> {
    unlocked(&s)?;
    let fill = match (username, password.map(Secret::new)) {
        (None, None) => None,
        (Some(username), Some(password)) => Some(login_fill(username, password)?),
        _ => return Err(AppError::Invalid(tr!("Give both a username and a password, or neither."))),
    };
    let cookie = webview::capture_login(&app, &s.data_dir, fill.as_ref()).await?;
    add_account(&app, &s, cookie).await
}

/// What a `user:pass` line may fill in: a username, email or phone (1-254 characters, no spaces)
/// and a password of 1-256 characters.
fn login_fill(username: String, password: Secret) -> Result<webview::LoginFill> {
    let name = username.trim();
    if name.is_empty() || name.chars().count() > 254 || name.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(AppError::Invalid(tr!("That isn't a Roblox username: use 1 to 254 characters without spaces.")));
    }
    let len = password.expose().chars().count();
    if len == 0 || len > 256 {
        return Err(AppError::Invalid(tr!("The password must be 1 to 256 characters.")));
    }
    Ok(webview::LoginFill { username: name.to_owned(), password })
}

/// Closes the open Roblox sign-in window (a bulk import's Stop); its sign-in ends cancelled.
#[tauri::command]
pub fn account_login_cancel(app: AppHandle) {
    webview::close_login(&app);
}

/// The start of every .ROBLOSECURITY value.
const COOKIE_MARK: &str = "_|WARNING:-DO-NOT-SHARE-THIS.";

/// Every .ROBLOSECURITY value in pasted text (pure), once each: on any line, from the warning
/// to the next space, quote, `;`, `,` or `:` after it, so `user:pass:cookie` lines and plain
/// lists both work.
fn find_cookies(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let mut rest = line;
        while let Some(at) = rest.find(COOKIE_MARK) {
            let tail = &rest[at..];
            // The warning ends with "|_"; the value proper starts after it.
            let body = tail.find("|_").map_or(COOKIE_MARK.len(), |i| i + 2);
            let end = tail[body..].find(|c: char| !c.is_ascii_graphic() || "\"';,:".contains(c)).map_or(tail.len(), |i| body + i);
            let c = &tail[..end];
            if end > body && !out.iter().any(|o| o == c) {
                out.push(c.to_owned());
            }
            rest = &tail[end..];
        }
    }
    out
}

/// Adds each cookie in turn through the single-account path (checked with Roblox, a short
/// pause between them). A cookie for an account already stored refreshes it (`updated`); the
/// same cookie twice counts once.
async fn import_cookies(app: &AppHandle, s: &App, cookies: Vec<(Secret, ImportExtras)>) -> Result<ImportReport> {
    let mut report = ImportReport::default();
    let before: Vec<i64> = s.db.accounts()?.iter().map(|a| a.id).collect();
    for (i, (cookie, extras)) in cookies.into_iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        match add_account(app, s, cookie).await {
            Ok(a) => {
                if before.contains(&a.id) { report.updated += 1 } else { report.added += 1 }
                let patch = AccountPatch {
                    alias: Some(extras.alias.chars().take(64).collect()).filter(|v: &String| !v.trim().is_empty()),
                    notes: Some(extras.notes.chars().take(2000).collect()).filter(|v: &String| !v.trim().is_empty()),
                    ..Default::default()
                };
                if (patch.alias.is_some() || patch.notes.is_some()) && let Err(e) = s.db.update_account(a.id, &patch) {
                    log::warn!("import: details of account {} not kept: {e}", a.id);
                }
                // Added to the group it was in there; the groups it has here stay.
                let group: String = extras.group.trim().chars().take(64).collect();
                if !group.is_empty()
                    && let Err(e) = s.db.group_by_name_or_create(&group, None).and_then(|g| {
                        let mut ids = a.group_ids.clone();
                        if !ids.contains(&g) {
                            ids.push(g);
                        }
                        s.db.set_account_groups(a.id, &ids)
                    })
                {
                    log::warn!("import: group of account {} not kept: {e}", a.id);
                }
            }
            Err(AppError::Locked) => return Err(AppError::Locked),
            Err(e) => {
                log::info!("import: entry #{} was refused: {e}", i + 1);
                report.failed += 1;
                let who = if !extras.alias.trim().is_empty() {
                    extras.alias.trim().to_owned()
                } else {
                    tr!("Line {n}", n = i + 1)
                };
                report.failures.push(format!("{who}: {}", e.message()));
            }
        }
    }
    accounts_changed(app);
    Ok(report)
}

/// What an import keeps besides the cookie.
#[derive(Default)]
struct ImportExtras {
    alias: String,
    notes: String,
    /// The group to add it to, by name (made if missing); "" for none.
    group: String,
}

/// Every cookie found in pasted text (`find_cookies`), added one by one. `skipped`: repeats.
#[tauri::command]
pub async fn accounts_import_cookies(app: AppHandle, s: State<'_, App>, text: String) -> Result<ImportReport> {
    unlocked(&s)?;
    let found = find_cookies(&text);
    if found.is_empty() {
        return Err(AppError::Invalid(tr!("No .ROBLOSECURITY cookies found. Each starts with _|WARNING:-DO-NOT-SHARE-THIS.")));
    }
    let total = text.matches(COOKIE_MARK).count();
    let cookies = found.iter().map(|c| normalize_cookie(c).map(|c| (c, ImportExtras::default()))).collect::<Result<Vec<_>>>()?;
    let mut report = import_cookies(&app, &s, cookies).await?;
    report.skipped = total.saturating_sub(found.len()) as u32;
    Ok(report)
}

/// Roblox Account Manager's DPAPI entropy (ic3w0lf22/Roblox-Account-Manager, AccountManager.cs).
const RAM_ENTROPY: &[u8] = b"ROBLOX ACCOUNT MANAGER | :) | BROUGHT TO YOU BUY ic3w0lf";
/// Starts a RAM file saved with its own password (libsodium; not readable here).
const RAM_HEADER: &[u8] = b"Roblox Account Manager created by ic3w0lf22 @ github.com .......";

/// One account of RAM's AccountData.json.
#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RamAccount {
    #[serde(default)]
    security_token: Option<String>,
    #[serde(default)]
    alias: Option<String>,
    #[serde(default)]
    description: Option<String>,
    /// RAM's group, "Default" for an account in none (Account.cs: `Group { get; set; } = "Default"`).
    #[serde(default)]
    group: Option<String>,
}

/// RAM's accounts from its saved JSON (pure): the cookie (blank: None), alias, description and
/// group (RAM's "Default" is no group).
fn ram_accounts(json: &[u8]) -> Result<Vec<(Option<String>, ImportExtras)>> {
    let list: Vec<RamAccount> = serde_json::from_slice(json.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(json))
        .map_err(|e| {
            log::debug!("RAM AccountData.json unreadable: {:?} at {}:{}", e.classify(), e.line(), e.column());
            AppError::Invalid(tr!("That isn't a Roblox Account Manager AccountData.json."))
        })?;
    Ok(list
        .into_iter()
        .map(|a| {
            let cookie = a.security_token.filter(|t| !t.trim().is_empty());
            let group = a.group.filter(|g| g.trim() != "Default").unwrap_or_default();
            (cookie, ImportExtras { alias: a.alias.unwrap_or_default(), notes: a.description.unwrap_or_default(), group })
        })
        .collect())
}

/// Imports Roblox Account Manager's AccountData.json: plain, or DPAPI-encrypted by RAM for
/// this Windows user. One saved with a RAM password is refused (remove the password in RAM
/// first). Alias and Description become alias and notes.
#[tauri::command]
pub async fn accounts_import_ram(app: AppHandle, s: State<'_, App>, path: String) -> Result<ImportReport> {
    unlocked(&s)?;
    let meta = std::fs::metadata(&path).map_err(|e| {
        log::warn!("could not read RAM file metadata: {e}");
        AppError::Invalid(tr!("RoALT couldn't read that file."))
    })?;
    if meta.len() > 32 * 1024 * 1024 {
        return Err(AppError::Invalid(tr!("That file is too big to be an AccountData.json.")));
    }
    let data = zeroize::Zeroizing::new(std::fs::read(&path).map_err(|e| {
        log::warn!("could not read RAM file: {e}");
        AppError::Invalid(tr!("RoALT couldn't read that file."))
    })?);
    if data.starts_with(RAM_HEADER) {
        return Err(AppError::Invalid(tr!(
            "This file is locked with a Roblox Account Manager password. Remove the password in Roblox Account Manager, then import again."
        )));
    }
    let entries = match crate::vault::unprotect_with(&data, RAM_ENTROPY).inspect_err(|e| log::debug!("RAM file not DPAPI-readable: {e}")) {
        Ok(plain) => ram_accounts(&plain)?,
        Err(_) if data.trim_ascii_start().starts_with(b"[") || data.starts_with(b"\xEF\xBB\xBF") => ram_accounts(&data)?,
        Err(_) => {
            return Err(AppError::Invalid(tr!(
                "RoALT couldn't decrypt this file. It must come from Roblox Account Manager on this PC, signed in as the same Windows user."
            )));
        }
    };
    let mut cookies = Vec::new();
    let mut failed = 0;
    for (cookie, extras) in entries {
        match cookie.as_deref().map(normalize_cookie) {
            Some(Ok(c)) => cookies.push((c, extras)),
            _ => failed += 1,
        }
    }
    if failed > 0 {
        log::debug!("RAM import: {failed} entries with a blank or malformed cookie");
    }
    let mut report = import_cookies(&app, &s, cookies).await?;
    report.failed += failed;
    Ok(report)
}

/// A locked vault is for looking at nothing and changing nothing.
fn unlocked(s: &App) -> Result<()> {
    if s.vault.is_locked() { Err(AppError::Locked) } else { Ok(()) }
}

/// These run on the main thread: a list no UI can make would freeze the window.
fn not_too_many(ids: &[i64]) -> Result<()> {
    if ids.len() > 20_000 { Err(AppError::Internal("too many ids".into())) } else { Ok(()) }
}

#[tauri::command]
pub fn account_update(app: AppHandle, s: State<'_, App>, id: i64, mut patch: AccountPatch) -> Result<AccountSummary> {
    unlocked(&s)?;
    if let Some(p) = patch.profile.take() {
        crate::performance::check(&p, &s.db.settings()?)?;
        patch.profile = Some(crate::performance::per_client(p));
    }
    if let Some(a) = &patch.launch_args {
        crate::launcher::launch_args(a)?;
    }
    if let Some(f) = patch.fps_cap {
        check_fps(f)?;
    }
    if let Some(f) = &patch.custom_flags {
        crate::launcher::flags::check(f)?;
    }
    s.db.update_account(id, &patch)?;
    accounts_changed(&app);
    s.db.account(id)
}

#[tauri::command]
pub fn accounts_reorder(app: AppHandle, s: State<'_, App>, ids: Vec<i64>) -> Result<()> {
    unlocked(&s)?;
    not_too_many(&ids)?;
    s.db.reorder_accounts(&ids)?;
    accounts_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn accounts_remove(app: AppHandle, s: State<'_, App>, ids: Vec<i64>) -> Result<()> {
    unlocked(&s)?;
    not_too_many(&ids)?;
    s.db.remove_accounts(&ids)?;
    accounts_changed(&app);
    // Linux: a session window holds its profile; ask it to close so removal can wait for it.
    #[cfg(not(windows))]
    for id in &ids {
        if let Some(w) = app.get_webview_window(&format!("session-{id}")) {
            let _ = w.close();
        }
    }
    // Off this thread: Browse, open as one of them, closes first, and removal retries for seconds.
    let dir = s.data_dir.clone();
    std::thread::spawn(move || ids.into_iter().for_each(|id| webview::remove_account_profile(&app, &dir, id)));
    Ok(())
}

/// Checks each cookie with Roblox (one at a time; the API client throttles).
/// `None` checks every account. Returns the full, updated list.
#[tauri::command]
pub async fn accounts_validate(app: AppHandle, s: State<'_, App>, ids: Option<Vec<i64>>) -> Result<Vec<AccountSummary>> {
    validate_accounts(&app, &s, ids).await
}

/// `accounts_validate`, also what the sign-in watch runs (`watch`).
pub(crate) async fn validate_accounts(app: &AppHandle, s: &App, ids: Option<Vec<i64>>) -> Result<Vec<AccountSummary>> {
    let ids = match ids {
        Some(ids) => ids,
        None => s.db.accounts()?.iter().map(|a| a.id).collect(),
    };
    let (mut answered, mut last_err) = (0, None);
    for id in ids {
        // Removed while a long run was going: skip it.
        let (user_id, blob) = match s.db.account_cookie(id) {
            Err(AppError::NotFound) => continue,
            r => r?,
        };
        let cookie = s
            .vault
            .decrypt_secret(&cookie_aad(user_id), &blob)
            .inspect_err(|e| log::warn!("could not decrypt the cookie of account {id}: {e}"))?;
        let now = now_ms();
        let was = s.db.account(id).ok().map(|a| a.status);
        let stored = match s.api.authenticated_user(id, &cookie).await {
            Ok((u, _)) if u.id == user_id => {
                s.db.set_status(id, AccountStatus::Valid, now, Some(&u.name), Some(&u.display_name))
            }
            // The cookie signs in as someone else (e.g. an account switch in a browser).
            Ok(_) => s.db.set_status(id, AccountStatus::Invalid, now, None, None),
            Err(AppError::CookieInvalid | AppError::BoundSessionRequired) => {
                s.db.set_status(id, AccountStatus::Invalid, now, None, None)
            }
            Err(AppError::Challenge) => s.db.set_status(id, AccountStatus::Challenged, now, None, None),
            Err(e) => {
                log::warn!("could not validate account {id}: {e}");
                last_err = Some(e);
                continue;
            }
        };
        match stored {
            Err(AppError::NotFound) => continue,
            r => r?,
        }
        answered += 1;
        // Told to Discord when a check finds the sign-in newly dead, not each time it still is.
        if let Ok(a) = s.db.account(id)
            && a.status != AccountStatus::Valid
            && was.is_some_and(|w| w != a.status)
        {
            crate::discord::sign_in(app, id, a.status == AccountStatus::Challenged);
        }
    }
    accounts_changed(app);
    match last_err {
        Some(e) if answered == 0 => Err(e),
        _ => s.db.accounts(),
    }
}

/// Master mode asks for the password again; DPAPI mode asks Windows Hello.
async fn reauth(app: &AppHandle, s: &App, password: Option<String>, prompt: &str) -> Result<()> {
    match s.vault.mode() {
        VaultMode::Master => {
            let rec = s.db.vault_record()?.ok_or(AppError::Internal("vault record missing".into()))?;
            let pw = password.ok_or(AppError::WrongPassword)?;
            let handle = app.clone();
            blocking(move || handle.state::<App>().vault.check_password(&rec, &pw)).await?
        }
        VaultMode::Dpapi => {
            let window = app.get_webview_window("main").ok_or(AppError::Internal("no main window".into()))?;
            crate::vault::windows_hello(&window, prompt).await
        }
    }
}

fn reveal_prompt() -> String {
    tr!("Confirm it's you to reveal a Roblox cookie.")
}

/// Records a dead or challenged cookie on the account (true when `e` was one).
pub(crate) fn record_cookie_error(app: &AppHandle, s: &App, id: i64, e: &AppError) -> Result<bool> {
    let status = match e {
        AppError::CookieInvalid | AppError::BoundSessionRequired => AccountStatus::Invalid,
        AppError::Challenge => AccountStatus::Challenged,
        _ => return Ok(false),
    };
    log::info!("account {id} marked {status:?}: {e}");
    // Told to Discord when it turns bad, not on every check that finds it still so.
    let news = s.db.account(id).is_ok_and(|a| a.status != status);
    s.db.set_status(id, status, now_ms(), None, None)?;
    accounts_changed(app);
    if news {
        crate::discord::sign_in(app, id, status == AccountStatus::Challenged);
    }
    Ok(true)
}

pub(crate) fn stored_cookie(s: &App, id: i64) -> Result<Secret> {
    let (user_id, blob) = s.db.account_cookie(id)?;
    s.vault.decrypt_secret(&cookie_aad(user_id), &blob)
}

#[tauri::command]
pub async fn account_reveal_cookie(app: AppHandle, s: State<'_, App>, id: i64, password: Option<String>) -> Result<String> {
    reauth(&app, &s, password, &reveal_prompt()).await?;
    Ok(stored_cookie(&s, id)?.expose().to_owned())
}

/// Copies from Rust so the cookie never passes through the UI. `write_secret` keeps it out of
/// Windows clipboard history and cloud sync; the clipboard is cleared after the configured
/// delay, or at exit (`clear_copied_cookie`) if that comes first.
#[tauri::command]
pub async fn account_copy_cookie(app: AppHandle, s: State<'_, App>, id: i64, password: Option<String>) -> Result<()> {
    reauth(&app, &s, password, &reveal_prompt()).await?;
    copy_secret(&app, &s, stored_cookie(&s, id)?)
}

fn copy_secret(app: &AppHandle, s: &App, secret: Secret) -> Result<()> {
    let secs = s.db.settings()?.clipboard_clear_seconds.max(5);
    app.clipboard().write_secret(secret.expose().to_owned()).map_err(|e| AppError::Internal(e.to_string()))?;
    *s.copied.lock().unwrap_or_else(|e| e.into_inner()) = Some(secret);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(secs as u64)).await;
        clear_copied_cookie(&app);
    });
    Ok(())
}

/// Clears the clipboard if it still holds the last copied cookie.
pub fn clear_copied_cookie(app: &AppHandle) {
    let s = app.state::<App>();
    let Some(cookie) = s.copied.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
    match app.clipboard().read_text() {
        Ok(t) if t == cookie.expose() => {
            if let Err(e) = app.clipboard().clear() {
                log::warn!("could not clear the copied cookie from the clipboard: {e}");
            }
        }
        Ok(_) => {}
        Err(e) => log::warn!("could not read the clipboard to clear the copied cookie: {e}"),
    }
}

/// "Export cookies": the chosen accounts' (all with None) cookies as `username:cookie` lines in a
/// file under `<data dir>\exports`, selected in Explorer and deleted after
/// `cookie_export::DELETE_AFTER`. Same check as revealing one cookie. A locked vault fails it all;
/// an account whose cookie can't be read is skipped.
#[tauri::command]
pub async fn accounts_export_cookies(app: AppHandle, s: State<'_, App>, account_ids: Option<Vec<i64>>, password: Option<String>) -> Result<CookieExport> {
    reauth(&app, &s, password, &tr!("Confirm it's you to export Roblox cookies.")).await?;
    let mut found = Vec::new();
    let mut skipped = 0u32;
    for a in s.db.accounts()?.into_iter().filter(|a| account_ids.as_ref().is_none_or(|ids| ids.contains(&a.id))) {
        match stored_cookie(&s, a.id) {
            Ok(cookie) => found.push((a.username, cookie)),
            Err(e @ AppError::Locked) => return Err(e),
            Err(e) => {
                log::warn!("cookie export: account {} skipped: {e}", a.id);
                skipped += 1;
            }
        }
    }
    if found.is_empty() {
        return Err(crate::cookie_export::none_to_export());
    }
    let path = crate::cookie_export::write(&s.data_dir, &crate::cookie_export::lines(&found))?;
    log::info!("cookie export: {} account(s) written, {skipped} skipped", found.len());
    reveal(&app, &path);
    let forget = path.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(crate::cookie_export::DELETE_AFTER).await;
        crate::cookie_export::forget(&forget);
    });
    Ok(CookieExport {
        path: path.display().to_string(),
        count: found.len() as u32,
        skipped,
        deletes_at: now_ms() + crate::cookie_export::DELETE_AFTER.as_millis() as i64,
    })
}

/// Deletes the cookie exports this run wrote (never another file).
#[tauri::command]
pub fn cookie_export_delete() -> u32 {
    crate::cookie_export::delete_all() as u32
}

/// Selects the newest cookie export in Explorer again.
#[tauri::command]
pub fn cookie_export_reveal(app: AppHandle) -> Result<()> {
    let path = crate::cookie_export::newest().ok_or_else(|| AppError::Invalid(tr!("The exported file is gone.")))?;
    reveal(&app, &path);
    Ok(())
}

/// Opens Explorer with `path` selected; logged, not an error (the path is shown anyway).
fn reveal(app: &AppHandle, path: &std::path::Path) {
    use tauri_plugin_opener::OpenerExt;
    if let Err(e) = app.opener().reveal_item_in_dir(path) {
        log::warn!("could not show {} in Explorer: {e}", path.display());
    }
}

// ---------- account info ----------

const HEADSHOT_TTL_MS: i64 = 6 * 60 * 60 * 1000;

fn presence_of(p: UserPresence) -> Presence {
    Presence {
        kind: PresenceKind::from_code(p.user_presence_type),
        last_location: p.last_location,
        place_id: p.place_id,
        root_place_id: p.root_place_id,
        universe_id: p.universe_id,
        game_id: p.game_id,
    }
}

/// Each account's Robux as its info panel last read it, whatever its age: (account id, Robux).
/// Nothing is asked of Roblox; an account whose panel was never opened is left out.
#[tauri::command(async)]
pub fn accounts_robux(s: State<'_, App>) -> Result<Vec<(i64, i64)>> {
    let now = now_ms();
    let robux = |user_id: i64| {
        let (json, _) = s.db.cache_get(&format!("info:{user_id}"), i64::MAX / 2, now).ok()??;
        serde_json::from_str::<serde_json::Value>(&json).ok()?.get("robux")?.as_i64()
    };
    Ok(s.db.accounts()?.into_iter().filter_map(|a| Some((a.id, robux(a.user_id)?))).collect())
}

/// The info panel. Served from the cache unless `refresh` or older than `cache_minutes`.
/// Each part is fetched on its own; parts Roblox doesn't answer are listed in `unavailable`.
#[tauri::command]
pub async fn account_info(app: AppHandle, s: State<'_, App>, id: i64, refresh: bool) -> Result<AccountInfo> {
    let a = s.db.account(id)?;
    let key = format!("info:{}", a.user_id);
    let ttl = s.db.settings()?.cache_minutes as i64 * 60_000;
    if !refresh
        && let Some((json, _)) = s.db.cache_get(&key, ttl, now_ms())?
        && let Ok(mut info) = serde_json::from_str::<AccountInfo>(&json).inspect_err(|e| {
            log::debug!("cached info of account {id} unreadable: {:?} at {}:{}", e.classify(), e.line(), e.column())
        })
    {
        // Presence goes stale long before the rest: re-ask (self-view) once a minute at most.
        if now_ms() - info.fetched_at > 60_000
            && let Ok(rows) = s.api.presence(id, &stored_cookie(&s, id)?, &[a.user_id]).await
        {
            info.presence = rows.into_iter().find(|p| p.user_id == a.user_id).map(presence_of);
        }
        return Ok(info);
    }
    let cookie = stored_cookie(&s, id)?;
    let uid = a.user_id;
    let one = [uid];
    let (details, heads, robux, premium, plus, counts, presence, email, two_step) = tokio::join!(
        s.api.user_details(id, Some(&cookie), uid),
        s.api.headshots(&one),
        s.api.robux(id, &cookie),
        s.api.premium(id, &cookie, uid),
        s.api.plus(&one),
        s.api.social_counts(uid),
        s.api.presence(id, &cookie, &one),
        s.api.email_verified(id, &cookie),
        s.api.two_step_methods(id, &cookie, uid),
    );
    let mut unavailable = Vec::new();
    let mut part = |name: &str, ok: bool| {
        if !ok {
            unavailable.push(name.to_owned());
        }
    };
    part("profile", details.is_ok());
    part("avatar", heads.as_ref().is_ok_and(|h| h.contains_key(&uid)));
    part("robux", robux.is_ok());
    part("friends", counts.is_ok());
    part("presence", presence.as_ref().is_ok_and(|p| !p.is_empty()));
    part("email", email.is_ok());
    part("2-step", two_step.is_ok());
    let subscription = match (plus.as_ref().ok().and_then(|p| p.get(&uid).copied()), premium.as_ref().ok().copied()) {
        (Some(true), _) => Some(Subscription::Plus),
        (_, Some(true)) => Some(Subscription::Premium),
        (Some(false), _) => Some(Subscription::None),
        (None, _) => None,
    };
    part("subscription", subscription.is_some());

    // Robux needs the cookie, so it is the probe: a dead or challenged cookie is reported (and
    // recorded) instead of showing a half-empty panel; so is being entirely offline. Neither
    // is cached. Public parts still answer anonymously when the cookie is dead.
    let robux = match robux {
        Ok(r) => Some(r),
        Err(e) => {
            if record_cookie_error(&app, &s, id, &e)? || unavailable.len() == 8 {
                return Err(e);
            }
            None
        }
    };
    let details = details.ok();
    let counts = counts.ok();
    let info = AccountInfo {
        account_id: id,
        user_id: uid,
        username: details.as_ref().map_or(a.username.clone(), |d| d.name.clone()),
        display_name: details.as_ref().map_or(a.display_name.clone(), |d| d.display_name.clone()),
        description: details.as_ref().map(|d| d.description.clone()).unwrap_or_default(),
        created: details.as_ref().and_then(|d| d.created.clone()),
        is_banned: details.as_ref().is_some_and(|d| d.is_banned),
        verified_badge: details.as_ref().is_some_and(|d| d.has_verified_badge),
        headshot_url: heads.ok().and_then(|mut h| h.remove(&uid)),
        robux,
        subscription,
        friends: counts.map(|c| c.friends),
        followers: counts.map(|c| c.followers),
        following: counts.map(|c| c.following),
        email_verified: email.ok(),
        two_step_methods: two_step.ok(),
        presence: presence.ok().and_then(|p| p.into_iter().find(|p| p.user_id == uid)).map(presence_of),
        fetched_at: now_ms(),
        unavailable,
    };
    let json = serde_json::to_string(&info).map_err(|e| AppError::Internal(e.to_string()))?;
    s.db.cache_put(&key, &json, info.fetched_at)?;
    if let Some(url) = &info.headshot_url {
        s.db.cache_put(&format!("headshot:{uid}"), url, info.fetched_at)?;
    }
    Ok(info)
}

/// Avatar URLs for every account (cached 6 h); one batched call for the missing ones.
#[tauri::command]
pub async fn accounts_headshots(s: State<'_, App>) -> Result<Vec<Headshot>> {
    let now = now_ms();
    let accounts = s.db.accounts()?;
    let mut out = Vec::new();
    let mut missing = Vec::new();
    for a in &accounts {
        match s.db.cache_get(&format!("headshot:{}", a.user_id), HEADSHOT_TTL_MS, now)? {
            Some((url, _)) => out.push(Headshot { account_id: a.id, url }),
            None => missing.push(a.user_id),
        }
    }
    if !missing.is_empty() {
        // Offline or rate limited: still show the avatars we have.
        match s.api.headshots(&missing).await {
            Ok(urls) => {
                for a in accounts.iter().filter(|a| missing.contains(&a.user_id)) {
                    if let Some(url) = urls.get(&a.user_id) {
                        s.db.cache_put(&format!("headshot:{}", a.user_id), url, now)?;
                        out.push(Headshot { account_id: a.id, url: url.clone() });
                    }
                }
            }
            Err(e) => log::warn!("could not fetch avatars: {e}"),
        }
    }
    Ok(out)
}

/// Presence of the given accounts (the rows on screen), each asked with its own cookie:
/// Roblox hides friends-only presence and game details from other viewers. Accounts whose
/// cookie isn't valid, or whose call fails, are left out.
#[tauri::command]
pub async fn accounts_presence(s: State<'_, App>, ids: Vec<i64>) -> Result<Vec<AccountPresence>> {
    let mut out = Vec::new();
    for a in s.db.accounts()?.into_iter().filter(|a| ids.contains(&a.id) && a.status == AccountStatus::Valid) {
        let cookie = match stored_cookie(&s, a.id) {
            Ok(c) => c,
            Err(AppError::Locked) => continue,
            Err(e) => {
                log::debug!("presence: cookie of account {} unusable: {e}", a.id);
                continue;
            }
        };
        match s.api.presence(a.id, &cookie, &[a.user_id]).await {
            Ok(rows) => out.extend(
                rows.into_iter()
                    .find(|p| p.user_id == a.user_id)
                    .map(|p| AccountPresence { account_id: a.id, presence: presence_of(p) }),
            ),
            Err(e) => log::info!("presence of account {} unavailable: {e}", a.id),
        }
    }
    Ok(out)
}

/// The most accounts one "make friends" run takes.
pub const BEFRIEND_MAX: usize = 20;

/// Friends the given valid accounts with each other: for each pair, the first sends a request
/// and the second accepts it, each with its own cookie. Pairs already friends are skipped; a
/// pending request is just accepted. A captcha stops that account sending (never solved here,
/// it can still accept); a dead cookie marks it invalid. Everything else counts as failed and the run goes on.
#[tauri::command]
pub async fn accounts_befriend(app: AppHandle, s: State<'_, App>, ids: Vec<i64>) -> Result<FriendReport> {
    use crate::roblox_api::types::FriendStatus;
    unlocked(&s)?;
    // (account id, Roblox user id)
    let accts: Vec<(i64, i64)> = s
        .db
        .accounts()?
        .into_iter()
        .filter(|a| ids.contains(&a.id) && a.status == AccountStatus::Valid)
        .map(|a| (a.id, a.user_id))
        .collect();
    // Pairs grow with the square: 20 accounts are 190 pairs, already minutes of Roblox calls.
    // shortcut: no progress events, add them if a capped run still feels stuck.
    if accts.len() > BEFRIEND_MAX {
        return Err(AppError::Invalid(tr!("Make friends takes up to {max} accounts at a time.", max = BEFRIEND_MAX)));
    }
    // Read right before each call, not once: Roblox may rotate a cookie mid-run (the store gets the new one).
    let cookie = |id: i64| stored_cookie(&s, id).map_err(|e| (id, e));
    let api = &s.api;
    // `by` accepts `from`'s request.
    let accept = |by: i64, from_user: i64| async move {
        let c = cookie(by)?;
        api.accept_friend(by, &c, from_user).await.map_err(|e| (by, e))
    };
    let mut report = FriendReport::default();
    // Dead cookies make no more calls; a captcha only stops new requests (accepting still works).
    let mut dead = std::collections::HashSet::new();
    let mut captcha = std::collections::HashSet::new();
    type Ids = std::collections::HashSet<i64>;
    let note = |report: &mut FriendReport, dead: &mut Ids, captcha: &mut Ids, id: i64, e: AppError| -> Result<()> {
        if matches!(e, AppError::Challenge) {
            if captcha.insert(id) {
                report.challenged.push(id);
            }
        } else if matches!(e, AppError::CookieInvalid | AppError::BoundSessionRequired) {
            dead.insert(id);
            record_cookie_error(&app, &s, id, &e)?;
        } else {
            log::info!("befriend: account {id}: {e}");
        }
        Ok(())
    };
    for (i, &(a_id, a_user)) in accts.iter().enumerate() {
        // A run is activity: minutes of calls must not auto-lock under it. A lock by hand still stops it.
        unlocked(&s)?;
        s.vault.touch();
        let rest = &accts[i + 1..];
        let others: Vec<i64> = rest.iter().map(|b| b.1).collect();
        let statuses = if dead.contains(&a_id) {
            None
        } else {
            let asked = async {
                let c = cookie(a_id)?;
                api.friend_statuses(a_id, &c, a_user, &others).await.map_err(|e| (a_id, e))
            };
            match asked.await {
                Ok(m) => Some(m),
                Err((id, e)) => {
                    note(&mut report, &mut dead, &mut captcha, id, e)?;
                    None
                }
            }
        };
        let Some(statuses) = statuses else {
            report.failed += rest.len() as u32;
            continue;
        };
        for &(b_id, b_user) in rest {
            let status = statuses.get(&b_user).copied().unwrap_or(FriendStatus::NotFriends);
            let step = befriend_step(status, dead.contains(&a_id), captcha.contains(&a_id), dead.contains(&b_id));
            let done = match step {
                BefriendStep::Skip => {
                    report.already += 1;
                    continue;
                }
                BefriendStep::Fail => {
                    report.failed += 1;
                    continue;
                }
                BefriendStep::Accept { by_b: false } => accept(a_id, b_user).await,
                BefriendStep::Accept { by_b: true } => accept(b_id, a_user).await,
                BefriendStep::RequestThenAccept => {
                    let sent = async {
                        let c = cookie(a_id)?;
                        api.request_friendship(a_id, &c, b_user).await.map_err(|e| (a_id, e))
                    };
                    match sent.await {
                        Ok(()) => accept(b_id, a_user).await,
                        Err(e) => Err(e),
                    }
                }
            };
            match done {
                Ok(()) => report.added += 1,
                Err((id, e)) => {
                    report.failed += 1;
                    note(&mut report, &mut dead, &mut captcha, id, e)?;
                }
            }
        }
    }
    Ok(report)
}

/// What "make friends" does for one pair (A earlier in the list, B later).
#[derive(Debug, PartialEq, Eq)]
enum BefriendStep {
    /// Already friends.
    Skip,
    /// The account that would make the call is blocked.
    Fail,
    /// Accept the pending request: by B when A asked, else by A.
    Accept { by_b: bool },
    /// A sends a request, B accepts it.
    RequestThenAccept,
}

/// Picks the step for one pair. A dead cookie blocks every call; a captcha only blocks sending.
fn befriend_step(status: crate::roblox_api::types::FriendStatus, a_dead: bool, a_captcha: bool, b_dead: bool) -> BefriendStep {
    use crate::roblox_api::types::FriendStatus;
    match status {
        FriendStatus::Friends => BefriendStep::Skip,
        // B asked A: A accepts.
        FriendStatus::RequestReceived if a_dead => BefriendStep::Fail,
        FriendStatus::RequestReceived => BefriendStep::Accept { by_b: false },
        // A asked B: B accepts.
        FriendStatus::RequestSent if b_dead => BefriendStep::Fail,
        FriendStatus::RequestSent => BefriendStep::Accept { by_b: true },
        FriendStatus::NotFriends if a_dead || a_captcha || b_dead => BefriendStep::Fail,
        FriendStatus::NotFriends => BefriendStep::RequestThenAccept,
    }
}

#[cfg(test)]
mod befriend_tests {
    use super::{BefriendStep as S, befriend_step};
    use crate::roblox_api::types::FriendStatus as F;

    #[test]
    fn friends_always_skip() {
        for (a, c, b) in [(false, false, false), (true, true, true), (false, true, false)] {
            assert_eq!(befriend_step(F::Friends, a, c, b), S::Skip);
        }
    }

    #[test]
    fn received_is_accepted_by_a() {
        assert_eq!(befriend_step(F::RequestReceived, false, false, false), S::Accept { by_b: false });
        // A captcha on A or a dead B does not stop A accepting.
        assert_eq!(befriend_step(F::RequestReceived, false, true, false), S::Accept { by_b: false });
        assert_eq!(befriend_step(F::RequestReceived, false, false, true), S::Accept { by_b: false });
        assert_eq!(befriend_step(F::RequestReceived, true, false, false), S::Fail);
    }

    #[test]
    fn sent_is_accepted_by_b() {
        assert_eq!(befriend_step(F::RequestSent, false, false, false), S::Accept { by_b: true });
        assert_eq!(befriend_step(F::RequestSent, true, false, false), S::Accept { by_b: true });
        assert_eq!(befriend_step(F::RequestSent, false, true, false), S::Accept { by_b: true });
        assert_eq!(befriend_step(F::RequestSent, false, false, true), S::Fail);
    }

    #[test]
    fn new_request_needs_both_and_no_captcha_on_a() {
        assert_eq!(befriend_step(F::NotFriends, false, false, false), S::RequestThenAccept);
        assert_eq!(befriend_step(F::NotFriends, true, false, false), S::Fail);
        assert_eq!(befriend_step(F::NotFriends, false, true, false), S::Fail);
        assert_eq!(befriend_step(F::NotFriends, false, false, true), S::Fail);
    }
}

/// Blocking (never on an async thread): keeps `last`, the cookie a Browse page ended with, when it
/// still signs in as `user_id` (Roblox may have rotated it, or the user signed in again or solved
/// a challenge there) and marks the account valid; a sign-out or account switch there is logged
/// and the stored cookie left as it was.
pub(crate) fn keep_session_cookie(app: &AppHandle, id: i64, user_id: i64, last: Secret) {
    tauri::async_runtime::block_on(async move {
        let s = app.state::<App>();
        // The row may have been removed (and its id reused) while Browse was open.
        match s.db.account(id) {
            Ok(a) if a.user_id == user_id => {}
            Ok(_) | Err(AppError::NotFound) => return,
            Err(e) => {
                log::debug!("Browse for account {id}: could not read the account: {e}");
                return;
            }
        }
        match s.api.authenticated_user(0, &last).await {
            Ok((u, rotated)) if u.id == user_id => {
                s.store_rotated(id, rotated.unwrap_or(last));
                if let Err(e) = s.db.set_status(id, AccountStatus::Valid, now_ms(), Some(&u.name), Some(&u.display_name)) {
                    log::warn!("could not mark account {id} valid after Browse: {e}");
                }
            }
            Ok(_) => log::warn!("Browse for account {id} ended signed in as another user; cookie not changed"),
            Err(e) => log::warn!("Browse for account {id} ended with an unusable cookie ({e}); cookie not changed"),
        }
        accounts_changed(app);
    });
}

// ---------- launching ----------

/// Local only: what pasted text points at.
#[tauri::command]
pub fn target_parse(input: String) -> ParsedInput {
    crate::launcher::target::parse(&input)
}

/// The game or server of the last `roalt://` link RoALT was opened with, until Play takes it.
static PENDING_LINK: std::sync::Mutex<Option<LaunchTarget>> = std::sync::Mutex::new(None);

/// Acts on the first link in a command line. A `roalt:` link naming a public game or server, or
/// a `roblox:` link to anything Play can join (while RoALT opens Roblox's links), is kept and
/// the UI told: any web page can open these, so they only ever fill in Play, never launch. A
/// `roblox-player:` link (the website's Play button) starts the client at once
/// (`links::open`), or with `Settings.ask_browser_links` fills in Play too, so the account is
/// picked there. False when the main window should stay as it is.
pub fn open_link(app: &AppHandle, args: impl IntoIterator<Item = String>) -> bool {
    for arg in args {
        let scheme = arg.split_once(':').map(|(s, _)| s.to_ascii_lowercase());
        match scheme.as_deref() {
            Some("roblox-player") => {
                // Unreadable settings: as before, the browser's account.
                let ask = app.state::<App>().db.settings().is_ok_and(|v| v.ask_browser_links);
                if let Some(t) = crate::launcher::uri::target_of_player_uri(&arg).filter(|_| ask && arg.len() <= 8192) {
                    *PENDING_LINK.lock().unwrap_or_else(|e| e.into_inner()) = Some(t);
                    let _ = app.emit("app://open-link", ());
                    return true;
                }
                crate::launcher::links::open(app, &arg);
                return false;
            }
            Some(scheme @ ("roalt" | "roblox")) if arg.len() <= 2048 => {
                match crate::launcher::target::parse(&arg) {
                    ParsedInput::Target { target: t } if scheme == "roblox" || matches!(t, LaunchTarget::Place { .. } | LaunchTarget::Server { .. }) => {
                        *PENDING_LINK.lock().unwrap_or_else(|e| e.into_inner()) = Some(t);
                        let _ = app.emit("app://open-link", ());
                    }
                    _ => log::info!("ignored a {scheme}: link that names no game or server"),
                }
                return true;
            }
            _ => {}
        }
    }
    true
}

/// Play's target from a `roalt://` link, once.
#[tauri::command]
pub fn take_pending_link() -> Option<LaunchTarget> {
    PENDING_LINK.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// A Roblox username (current or previous) → the user.
#[tauri::command]
pub async fn user_lookup(s: State<'_, App>, name: String) -> Result<UserRef> {
    let name = name.trim().trim_start_matches('@').to_owned();
    if name.is_empty() {
        return Err(AppError::Invalid(tr!("Type a Roblox username.")));
    }
    let found = s.api.users_by_name(std::slice::from_ref(&name)).await?;
    let u = found.into_iter().next().ok_or_else(|| AppError::Invalid(tr!("No Roblox user is called \"{name}\".", name = name)))?;
    Ok(UserRef { user_id: u.id, username: u.name, display_name: u.display_name })
}

/// Launches one account and starts following its client. `fps_cap` is this launch's own
/// (`launch_fps`).
#[tauri::command]
pub async fn launch(app: AppHandle, s: State<'_, App>, account_id: i64, target: LaunchTarget, fps_cap: Option<u32>) -> Result<LaunchInfo> {
    let fps_cap = launch_fps(fps_cap)?;
    // Launches go strictly one at a time.
    let _single = s.bulk.begin_single_launch()?;
    Ok(launch_tracked(&app, &s, account_id, target, None, fps_cap, true, None).await?.0)
}

/// Starts following a client as `info.account_id`'s (`played`: `Tracker::register_played`).
pub(crate) fn track(s: &App, info: &LaunchInfo, target: LaunchTarget, played: bool) -> Instance {
    let instance = if played { s.tracker.register_played(info, target) } else { s.tracker.register(info, target) };
    // What was last set on one of the account's clients is this one's too (a rejoin or restart
    // comes through here as well), else what its groups give: the limiter gives it them once its
    // process is found.
    if let Some(p) = s.db.effective(info.account_id).ok().and_then(|a| a.client_limits) {
        s.tracker.set_limits(&instance.id, p.limits);
        s.tracker.set_anti_afk(&instance.id, p.anti_afk);
    }
    instance
}

/// Why launches are refused while `roblox_install` runs.
pub(crate) fn installing() -> String {
    tr!("Roblox is being installed. Launch again once it is done.")
}

/// Whether the launcher a launch would use is Mocktail now (`launcher::client::find`, the same
/// choice `launcher::launch` makes). Windows has no Mocktail backend.
#[cfg(not(windows))]
fn mocktail_choice() -> bool {
    crate::launcher::client::find()
        .is_ok_and(|exe| exe.kind == crate::launcher::client::LauncherKind::Mocktail)
}

#[cfg(windows)]
fn mocktail_choice() -> bool {
    false
}

/// `mocktail_choice` for the status surface. `find` logs and stats launchers, and the UI asks for
/// the multi-instance status every few seconds, so the reported choice is settled once per run; a
/// launch asks `mocktail_choice` again, so one installed mid-run still counts there.
#[cfg(not(windows))]
fn mocktail_selected() -> bool {
    static SELECTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SELECTED.get_or_init(mocktail_choice)
}

#[cfg(windows)]
fn mocktail_selected() -> bool {
    false
}

/// Whether a Mocktail client is running now (`instances::process::mocktail_running`, which reads
/// the same `/proc` view the launch backend refuses on). Windows has no Mocktail backend.
#[cfg(not(windows))]
fn mocktail_client_running() -> bool {
    crate::instances::process::mocktail_running()
}

#[cfg(windows)]
fn mocktail_client_running() -> bool {
    false
}

/// Mocktail runs one Roblox client at a time unless the user has enabled multi-instance: with it
/// on, every launch gets its own copied launcher and isolated engine-data root. Refused before
/// the launch cooldown, so a launch that can't happen doesn't hold a slot.
pub(crate) fn mocktail_launch_guard(multi_instance: bool) -> Result<()> {
    if !multi_instance && mocktail_choice() && mocktail_client_running() {
        return Err(AppError::Invalid(tr!(
            "Mocktail supports one Roblox client at a time. Close the running client first."
        )));
    }
    Ok(())
}

/// One launch as every caller does it: the launch cooldown (`Bulk::cooldown`: waits until the
/// launch delay, `delay_ms` or else the setting's, less its 1 s jitter has passed since the
/// previous launch; a launch that fails before its client starts gives its slot back); stopped
/// by Kill all (`Bulk::kill`) while waiting, and a client started meanwhile is closed; the multi-instance
/// lock first (it has to be held before this client starts, or the next one replaces it), and
/// so the shared login files while protection is on; a dead or challenged cookie recorded on
/// the account; the client tracked. An account whose client is
/// still running is refused (a second client would sign it out of the first).
/// `hold`: refuse when an update would close running clients (a relaunch that already checked
/// that before closing its client doesn't ask again).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn launch_tracked(
    app: &AppHandle,
    s: &App,
    account_id: i64,
    target: LaunchTarget,
    delay_ms: Option<u32>,
    fps_cap: Option<u32>,
    hold: bool,
    headless: Option<bool>,
) -> Result<(LaunchInfo, Instance)> {
    let kills = s.bulk.kills();
    let killed = || AppError::Invalid(tr!("Launch cancelled by Stop all."));
    // Claimed until the client is tracked, so a launch from another path (rejoin, tray,
    // batch) can't start the same account meanwhile.
    let user_id = s.db.account(account_id).ok().map(|a| a.user_id);
    let Some(_claim) = s.tracker.claim_for(account_id, user_id) else {
        let name = s.db.account(account_id).map(|a| crate::launcher::bulk::name_of(&a)).ok().filter(|n| !n.is_empty());
        let name = name.unwrap_or_else(|| tr!("This account"));
        return Err(AppError::Busy(tr!("{name} is already running. Close its client first.", name = name)));
    };
    if let Some(why) = crate::network::blocked(s) {
        return Err(AppError::Wait(why));
    }
    let _busy = s.bulk.begin_launch();
    // After `_busy`: an install starting now sees this launch, one already running is seen here.
    if s.update.installing() {
        return Err(AppError::Wait(installing()));
    }
    if hold {
        if let Some(why) = crate::launcher::update::update_hold(s, 0).await {
            return Err(AppError::Wait(why));
        }
        // The hold may have found Roblox's update isn't rolled out here: the warnings go.
        crate::launcher::update::publish(app);
    }
    let v = s.db.settings()?;
    // Mocktail replaces the client it already has instead of starting a second one: refuse now,
    // before the launch gap is waited out. Multi-instance switches to per-account copies.
    mocktail_launch_guard(v.multi_instance)?;
    let gap = s.bulk.launch_gap(delay_ms.unwrap_or(v.launch_delay_ms), Instant::now());
    let (wait, slot) = s.bulk.cooldown(gap);
    if !wait.is_zero() {
        log::debug!("launch of account {account_id} waits {wait:?} (launch gap {gap:?})");
    }
    tokio::time::sleep(wait).await;
    if s.bulk.kills() != kills {
        s.bulk.release(slot);
        return Err(killed());
    }
    retake_multi(s, &v);
    // Holds the shared files until the client is tracked (the poll releases them when idle).
    let launching = s.multi.launching(v.multi_instance && v.protect_shared_login);
    let info = match crate::launcher::launch(s, account_id, &target, fps_cap, headless.unwrap_or(v.headless)).await {
        Ok(info) => info,
        Err(e) => {
            log::warn!("launch failed for account {account_id}: {e}");
            s.bulk.release(slot);
            if e.is_forbidden() {
                s.bulk.refused();
            }
            record_cookie_error(app, s, account_id, &e)?;
            return Err(e);
        }
    };
    let instance = track(s, &info, target, false);
    drop(launching);
    instances_changed(app);
    // Kill all pressed while the ticket was fetched: this client goes too.
    if s.bulk.kills() != kills {
        if let Some((pid, started)) = s.tracker.mark_closing(&instance.id) {
            match tauri::async_runtime::spawn_blocking(move || {
                crate::instances::process::close(pid, started, true, Duration::ZERO)
            })
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(e)) => log::warn!("could not close client {pid} started during kill all: {e}"),
                Err(e) => log::warn!("close of client {pid} started during kill all failed: {e}"),
            }
        }
        return Err(killed());
    }
    Ok((info, instance))
}

// ---------- M7: client, data folder, notices ----------

/// Shows a message to the user (a toast; the window may be hidden in the tray).
pub fn notice(app: &AppHandle, error: bool, message: impl Into<String>) {
    let message = message.into();
    if error {
        log::warn!("notice: {message}");
    } else {
        log::info!("notice: {message}");
    }
    let _ = app.emit("app://notice", Notice { error, message });
}

/// The Roblox client RoALT would start (`launcher::client::find`; not installed → path None),
/// its version and Roblox's latest (`launcher::update`).
#[tauri::command]
pub fn roblox_client(app: AppHandle) -> RobloxClient {
    crate::launcher::update::publish(&app)
}

/// The programs that can open Roblox's links, and which does now (`launcher::links`).
#[tauri::command(async)]
pub fn roblox_launchers() -> Vec<RobloxLauncher> {
    crate::launcher::links::list()
}

/// Makes the launcher at `path` (one `roblox_launchers` lists) open Roblox's links; RoALT then
/// launches through it. Returns the list again.
#[tauri::command(async)]
pub fn roblox_launcher_set(app: AppHandle, s: State<'_, App>, path: String) -> Result<Vec<RobloxLauncher>> {
    crate::launcher::links::set(&s, &path)?;
    crate::launcher::update::publish(&app);
    Ok(crate::launcher::links::list())
}

/// Downloads Roblox's installer, checks it is signed by Roblox Corporation and runs it
/// (`launcher::update::install`); progress on `roblox://install`. Refused while a client runs.
#[tauri::command]
pub async fn roblox_install(app: AppHandle) -> Result<RobloxClient> {
    crate::launcher::update::install(&app).await
}

/// RoALT's version and the newer release the last check found (no network).
#[tauri::command]
pub fn self_update_status(s: State<'_, App>) -> AppUpdate {
    crate::self_update::status(&s)
}

/// Home's News tab: RoALT's releases, the DevForum and Roblox's newsroom (`news`). Nothing is asked
/// while `settings.news` is off or the settings can't be read.
#[tauri::command]
pub async fn news(s: State<'_, App>) -> Result<crate::model::News> {
    if !s.db.settings().is_ok_and(|v| v.news) {
        return Ok(crate::model::News::default());
    }
    Ok(crate::news::fetch().await)
}

/// RoALT's sponsor and its supporters' names, from roalt.dev; empty when it can't be read. The UI
/// asks only for News with `settings.show_sponsor` on, or with Settings › About open.
#[tauri::command]
pub async fn extras() -> crate::model::Extras {
    crate::news::extras().await
}

/// Pings RoALT's online counter and returns how many people use RoALT now and how many accounts
/// they're playing (`online`). None, and nothing sent, while `settings.online_count` is off or the
/// settings can't be read.
#[tauri::command]
pub async fn online_ping(s: State<'_, App>) -> Result<Option<crate::online::Online>> {
    if !s.db.settings().is_ok_and(|v| v.online_count) {
        return Ok(None);
    }
    Ok(crate::online::ping(s.tracker.clients().len()).await)
}

/// Asks GitHub for a newer RoALT now (Check now), whatever `settings.self_update` says.
#[tauri::command]
pub async fn self_update_check(app: AppHandle) -> Result<AppUpdate> {
    crate::self_update::check(&app).await
}

/// Downloads the newer RoALT's installer, checks its signature, runs it and quits
/// (`self_update::install`; the installer starts RoALT again).
#[tauri::command]
pub async fn self_update_install(app: AppHandle) -> Result<()> {
    crate::self_update::install(&app).await
}

/// Where the data lives now (see `main::data_dir`).
#[tauri::command]
pub fn data_dir_info(s: State<'_, App>) -> DataDir {
    DataDir {
        path: s.data_dir.display().to_string(),
        overridden: crate::data_dir_overridden(),
    }
}

/// Opens the data folder in Explorer (opener plugin, Rust side: no capability needed).
#[tauri::command]
pub fn data_dir_open(app: AppHandle, s: State<'_, App>) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_path(s.data_dir.display().to_string(), None::<&str>).map_err(|e| AppError::Internal(e.to_string()))
}

/// Opens RoALT's own log folder (data folder\logs) in Explorer.
#[tauri::command]
pub fn logs_open(app: AppHandle, s: State<'_, App>) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = s.data_dir.join("logs");
    std::fs::create_dir_all(&dir)?;
    app.opener().open_path(dir.display().to_string(), None::<&str>).map_err(|e| AppError::Internal(e.to_string()))
}

/// The last `lines` lines of `logs/roalt.log` (at most 10 000): the Debug tab's history.
#[tauri::command]
pub async fn debug_tail(s: State<'_, App>, lines: u32) -> Result<Vec<String>> {
    let path = s.data_dir.join("logs").join("roalt.log");
    blocking(move || tail(&path, lines.min(10_000) as usize)).await?
}

/// The last `n` lines of a text file, read from its last 4 MB (pure but for the read).
fn tail(path: &Path, n: usize) -> Result<Vec<String>> {
    use std::io::{Read, Seek, SeekFrom};
    const WINDOW: u64 = 4 << 20;
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let len = f.metadata()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(WINDOW)))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf);
    let all: Vec<&str> = text.lines().collect();
    // A window that starts mid-file starts mid-line: drop that partial first line.
    let skip = usize::from(len > WINDOW && !all.is_empty());
    let all = &all[skip..];
    Ok(all[all.len().saturating_sub(n)..].iter().map(|l| l.to_string()).collect())
}

/// A line from the UI (uncaught errors, failed invokes) into the log, target "ui".
#[tauri::command]
pub fn debug_log(level: String, message: String) {
    let level = match level.as_str() {
        "error" => log::Level::Error,
        "warn" => log::Level::Warn,
        "info" => log::Level::Info,
        _ => log::Level::Debug,
    };
    let mut message = message;
    if message.len() > 4096 {
        let mut end = 4096;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        message.truncate(end);
    }
    log::log!(target: "ui", level, "{message}");
}

/// Deletes Roblox client logs over a day old or 50 MB now (`logwatch::clean`), never a running
/// client's.
#[tauri::command]
pub async fn logs_clean(s: State<'_, App>) -> Result<LogsCleaned> {
    let Some(dir) = crate::instances::logwatch::logs_dir() else {
        log::debug!("logs clean: no Roblox logs folder");
        return Ok(LogsCleaned::default());
    };
    let keep = s.tracker.live_logs();
    blocking(move || crate::instances::logwatch::clean(&dir, &keep)).await
}

/// "Reset Roblox browser data": `webview::reset_profiles`; false while a Roblox window is open.
#[tauri::command]
pub async fn browser_data_reset(app: AppHandle, s: State<'_, App>) -> Result<bool> {
    let dir = s.data_dir.clone();
    blocking(move || webview::reset_profiles(&app, &dir)).await
}

const BACKGROUND_KEY: &str = "background";
/// 16 MB: an animated GIF is often bigger than a picture.
const BACKGROUND_MAX: u64 = 16 * 1024 * 1024;

/// PNG, JPEG, WebP or GIF, by its first bytes (pure).
fn is_image(b: &[u8]) -> bool {
    b.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a])
        || b.starts_with(&[0xff, 0xd8, 0xff])
        || b.starts_with(b"GIF8")
        || (b.len() >= 12 && b.starts_with(b"RIFF") && &b[8..12] == b"WEBP")
}

/// Keeps a copy of the picked image (up to 16 MB) in the database as the window's background.
#[tauri::command]
pub async fn background_set(s: State<'_, App>, path: String) -> Result<()> {
    // Off the async thread: reading and hex-encoding 16 MB takes a while.
    let hex = blocking(move || -> Result<String> {
        if std::fs::metadata(&path)?.len() > BACKGROUND_MAX {
            return Err(AppError::Invalid(tr!("Pick an image of 16 MB or less.")));
        }
        let bytes = std::fs::read(&path)?;
        if !is_image(&bytes) {
            return Err(AppError::Invalid(tr!("Pick a PNG, JPEG, WebP or GIF image.")));
        }
        Ok(crate::discord::hex(&bytes))
    })
    .await??;
    s.db.set_setting_value(BACKGROUND_KEY, Some(&hex))
}

#[tauri::command]
pub fn background_clear(s: State<'_, App>) -> Result<()> {
    s.db.set_setting_value(BACKGROUND_KEY, None)
}

/// The background image's bytes; empty when none is set.
#[tauri::command]
pub async fn background_get(s: State<'_, App>) -> Result<tauri::ipc::Response> {
    let hex = s.db.setting_value(BACKGROUND_KEY)?;
    let bytes = blocking(move || {
        hex.and_then(|h| {
            let b = crate::discord::unhex(&h);
            if b.is_none() {
                log::warn!("stored background isn't valid hex; showing none");
            }
            b
        })
        .unwrap_or_default()
    })
    .await?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// Seconds until the shutdown (pure): `in_secs`, else until `at_ms` (rounded up); 1 minute to 7 days.
fn shutdown_secs(at_ms: Option<i64>, in_secs: Option<u32>, now_ms: i64) -> Result<u32> {
    let secs = match (in_secs, at_ms) {
        (Some(s), _) => i64::from(s),
        (None, Some(at)) => (at - now_ms + 999).div_euclid(1000),
        (None, None) => return Err(AppError::Invalid(tr!("Pick when to shut down."))),
    };
    if !(60..=7 * 24 * 3600).contains(&secs) {
        return Err(AppError::Invalid(tr!("The shutdown must be 1 minute to 7 days from now.")));
    }
    Ok(secs as u32)
}

/// Runs the platform's shutdown command without a window: its exit code and what it printed.
#[cfg(windows)]
fn shutdown_exe(args: &[&str]) -> Result<(i32, String)> {
    use std::os::windows::process::CommandExt;
    let root = std::env::var_os("SystemRoot").map(std::path::PathBuf::from).unwrap_or_else(|| r"C:\Windows".into());
    let out = std::process::Command::new(root.join(r"System32\shutdown.exe")).args(args).creation_flags(0x0800_0000).output()?;
    let text = String::from_utf8_lossy(&out.stderr).trim().to_owned();
    let code = out.status.code().unwrap_or(-1);
    if code != 0 {
        log::warn!("shutdown.exe {} exited {code}: {}", args.join(" "), text.chars().take(300).collect::<String>());
    }
    Ok((code, text))
}

/// `shutdown` from util-linux on Linux; `-h +N` schedules minutes ahead, `-c` cancels.
#[cfg(not(windows))]
fn shutdown_exe(args: &[&str]) -> Result<(i32, String)> {
    let out = std::process::Command::new("shutdown").args(args).output()?;
    let text = String::from_utf8_lossy(&out.stderr).trim().to_owned();
    let code = out.status.code().unwrap_or(-1);
    if code != 0 {
        log::warn!("shutdown {} exited {code}: {}", args.join(" "), text.chars().take(300).collect::<String>());
    }
    Ok((code, text))
}

/// The settings-table row with the scheduled shutdown's deadline (Unix ms), so RoALT still knows
/// it (and can cancel it) after a restart; Windows keeps the timer either way.
const SHUTDOWN_KEY: &str = "shutdown_at";

/// The deadline of a shutdown scheduled before RoALT restarted, if it is still ahead (at start).
pub fn pending_shutdown(db: &crate::storage::Db) -> Option<i64> {
    let v = db.setting_value(SHUTDOWN_KEY).inspect_err(|e| log::warn!("could not read the scheduled shutdown: {e}")).ok().flatten()?;
    v.parse::<i64>().inspect_err(|e| log::warn!("scheduled shutdown deadline {v:?} unreadable: {e}")).ok().filter(|&at| at > now_ms())
}

/// Tells Windows to shut down in `in_secs` or at `at_ms` (`shutdown /s /t`, no admin rights;
/// Windows then force-closes every app). Replaces one RoALT scheduled. `close_clients`: RoALT's
/// clients are asked to close 30 s before (while RoALT keeps running). Returns the deadline
/// (Unix ms), stored (`SHUTDOWN_KEY`) until it is called off or passes.
#[tauri::command]
pub fn shutdown_schedule(app: AppHandle, s: State<'_, App>, at_ms: Option<i64>, in_secs: Option<u32>, close_clients: bool) -> Result<Option<i64>> {
    let secs = shutdown_secs(at_ms, in_secs, now_ms())?;
    let mut at = s.shutdown.lock().unwrap_or_else(|e| e.into_inner());
    if at.take().is_some() {
        #[cfg(windows)]
        shutdown_exe(&["/a"])?;
        #[cfg(not(windows))]
        shutdown_exe(&["-c"])?;
    }
    #[cfg(windows)]
    let (code, text) = shutdown_exe(&["/s", "/t", &secs.to_string()])?;
    #[cfg(not(windows))]
    let (code, text) = shutdown_exe(&["-h", &format!("+{}", secs.div_ceil(60))])?;
    if code != 0 {
        // 1190: a shutdown is already scheduled (by another app or by hand).
        return Err(AppError::Invalid(tr!("Windows wouldn't schedule the shutdown: {text}", text = text)));
    }
    let deadline = now_ms() + i64::from(secs) * 1000;
    *at = Some(deadline);
    if let Err(e) = s.db.set_setting_value(SHUTDOWN_KEY, Some(&deadline.to_string())) {
        log::warn!("the shutdown's deadline wasn't stored: {e}");
    }
    log::info!("shutdown scheduled in {secs} s");
    if close_clients {
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_secs(u64::from(secs.saturating_sub(30)))).await;
            let s = app.state::<App>();
            if *s.shutdown.lock().unwrap_or_else(|e| e.into_inner()) == Some(deadline)
                && let Err(e) = close_running(&s).await
            {
                log::warn!("could not close the clients before the shutdown: {e}");
            }
        });
    }
    Ok(Some(deadline))
}

/// Calls off the shutdown RoALT scheduled (`shutdown /a`).
#[tauri::command]
pub fn shutdown_cancel(s: State<'_, App>) -> Result<Option<i64>> {
    let mut at = s.shutdown.lock().unwrap_or_else(|e| e.into_inner());
    if at.is_some() {
        // 1116: none is scheduled any more (called off outside RoALT).
        #[cfg(windows)]
        let (code, text) = shutdown_exe(&["/a"])?;
        #[cfg(not(windows))]
        let (code, text) = shutdown_exe(&["-c"])?;
        if code != 0 && code != 1116 {
            return Err(AppError::Invalid(tr!("Windows wouldn't call off the shutdown: {text}", text = text)));
        }
        log::info!("shutdown called off");
    }
    *at = None;
    s.db.set_setting_value(SHUTDOWN_KEY, None)?;
    Ok(None)
}

/// When the shutdown RoALT scheduled is due (Unix ms); None when there is none.
#[tauri::command]
pub fn shutdown_status(s: State<'_, App>) -> Option<i64> {
    *s.shutdown.lock().unwrap_or_else(|e| e.into_inner())
}

/// Copies the database into `path` (a new folder or one without a roalt.db; not the current
/// folder or inside it) with SQLite's `VACUUM INTO` (consistent while open), records it in
/// `%LOCALAPPDATA%\RoALT\location.json` as `{"dataDir": path, "movedFrom": current}` and
/// restarts RoALT, all with the database locked so no write lands in the old copy after the
/// copy. The next start deletes the old folder's roalt.db, roalt.db-wal and roalt.db-shm once
/// the new database opened, and drops `movedFrom`. Returns only on failure, after deleting the
/// database files it may have left in `path`. Refused for test runs (`ROALT_DATA_DIR`), when
/// `path` already has a roalt.db, and while a launch, batch or auto-rejoin runs (the restart
/// would cut it off). Async: the copy can take a moment.
#[tauri::command]
pub async fn data_dir_move(app: AppHandle, s: State<'_, App>, path: String) -> Result<()> {
    if crate::data_dir_overridden() {
        return Err(AppError::Invalid(tr!("A test run's data folder (ROALT_DATA_DIR) can't be moved.")));
    }
    if s.bulk.any_busy() {
        return Err(AppError::Invalid(tr!("Wait for the running launch, batch or auto-rejoin to finish, then move the data folder.")));
    }
    // Their browser profiles stay in the old folder, which nothing sweeps after the move.
    if crate::webview::browsing(&app) {
        return Err(AppError::Invalid(tr!("Close the sign-in and Roblox browser windows first, then move the data folder.")));
    }
    let target = crate::move_target(&s.data_dir, &path)?;
    let handle = app.clone();
    let target_for_log = target.clone();
    blocking(move || -> Result<()> {
        let s = handle.state::<App>();
        // Only a folder that holds nothing of anyone's: this removes what it inherits for all inside.
        let empty = std::fs::read_dir(&target).map_or(true, |mut d| d.next().is_none());
        std::fs::create_dir_all(&target)?;
        if empty {
            crate::restrict_to_user(&target);
        }
        // In the copy: the exit handler still writes, but to the old database, the next start deletes.
        s.tracker.persist(&s.db, true);
        let now = now_ms();
        if let Err(e) = s.db.add_playtime(&s.tracker.take_playtime(now, true), now) {
            log::warn!("could not record playtime before the move: {e}");
        }
        let location = crate::Location { data_dir: target.clone(), moved_from: Some(s.data_dir.clone()) };
        let db_dst = target.join(crate::DB_FILE);
        let failed = s.db.copy_to(&db_dst, || crate::write_location(&location).map_err(AppError::from));
        if failed.is_ok() {
            // When moving into a non-empty folder, still restrict RoALT's own database file.
            if !empty {
                crate::restrict_file_to_user(&db_dst);
            }
            // An export's 10-minute timer dies with the restart, and the next start sweeps only the new
            // folder: a cookie file left in the old one would stay there for good.
            crate::cookie_export::sweep(&s.data_dir);
            return Ok(());
        }
        // A half-written copy would make a retry to the same folder refused.
        for f in crate::DB_FILES {
            if let Err(e) = std::fs::remove_file(target.join(f))
                && e.kind() != std::io::ErrorKind::NotFound
            {
                log::debug!("could not delete the half-written {f}: {e}");
            }
        }
        failed
    })
    .await??;
    // Not inside copy_to: a restart waits for the exit handler, which needs the database's lock.
    log::info!("database copied to {}; restarting to use it", target_for_log.display());
    app.request_restart();
    Ok(())
}

// ---------- bulk ----------

/// Starts a batch in the background (see `launcher::bulk::start`); returns its id.
#[tauri::command]
pub fn bulk_launch(app: AppHandle, request: BulkRequest) -> Result<String> {
    crate::launcher::bulk::start(app, request)
}

/// Stops the running batch after its current step.
#[tauri::command]
pub fn bulk_cancel(s: State<'_, App>, batch_id: String) -> Result<()> {
    s.bulk.cancel(&batch_id)
}

/// The running or last batch (None before the first), for a view that opens later.
#[tauri::command]
pub fn bulk_current(s: State<'_, App>) -> Option<BulkProgress> {
    s.bulk.current()
}

// ---------- servers ----------

/// The game behind a place: name, creator, players, icon, whether it sells private servers.
/// The game's name from what the app cached (`game_info`), however old; never asks Roblox.
pub(crate) fn cached_game_name(s: &App, place_id: i64) -> Option<String> {
    s.db.cache_get(&format!("game:{place_id}"), i64::MAX / 2, now_ms())
        .ok()
        .flatten()
        .and_then(|(json, _)| {
            serde_json::from_str::<GameInfo>(&json).inspect_err(|e| log::debug!("cached game {place_id} unreadable: {e}")).ok()
        })
        .map(|g| g.name)
}

/// Cached for `settings.cache_minutes` (key `game:<place_id>`, JSON of `GameInfo`). No cookie.
/// `NotFound` when Roblox knows no such place (no universe) or no such game.
#[tauri::command]
pub async fn game_info(s: State<'_, App>, place_id: i64) -> Result<GameInfo> {
    let key = format!("game:{place_id}");
    let ttl = s.db.settings()?.cache_minutes as i64 * 60_000;
    if let Some((json, _)) = s.db.cache_get(&key, ttl, now_ms())?
        && let Ok(info) = serde_json::from_str::<GameInfo>(&json).inspect_err(|e| log::debug!("cached game {place_id} unreadable: {e}"))
    {
        return Ok(info);
    }
    // The place icon needs no universe, so it is asked alongside; it is optional.
    let (universe, icons) = tokio::join!(s.api.place_universe(place_id), s.api.place_icons(std::slice::from_ref(&place_id)));
    let universe = universe?.ok_or(AppError::NotFound)?;
    let game = s.api.games(&[universe]).await?.into_iter().find(|g| g.universe_id == universe).ok_or(AppError::NotFound)?;
    let info = game_info_of(place_id, game, icons.ok().and_then(|mut i| i.remove(&place_id)));
    // Without its icon (failed or still being made) it is asked for again next time.
    if info.icon_url.is_some() {
        let json = serde_json::to_string(&info).map_err(|e| AppError::Internal(e.to_string()))?;
        s.db.cache_put(&key, &json, now_ms())?;
    }
    Ok(info)
}

/// Roblox's Charts lists with their games' icons, for Browse's Popular. Cached for
/// `settings.cache_minutes` (key `popular`). No cookie.
#[tauri::command]
pub async fn games_popular(s: State<'_, App>) -> Result<Vec<GameSort>> {
    const KEY: &str = "popular";
    // One per run, as the website keeps one per visit.
    static SESSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let ttl = s.db.settings()?.cache_minutes as i64 * 60_000;
    if let Some((json, _)) = s.db.cache_get(KEY, ttl, now_ms())?
        && let Ok(sorts) = serde_json::from_str::<Vec<GameSort>>(&json).inspect_err(|e| log::debug!("cached popular games unreadable: {e}"))
    {
        return Ok(sorts);
    }
    let mut sorts = s.api.popular_games(SESSION.get_or_init(|| uuid::Uuid::new_v4().to_string())).await?;
    let mut ids: Vec<i64> = sorts.iter().flat_map(|s| s.games.iter().map(|g| g.universe_id)).collect();
    ids.sort_unstable();
    ids.dedup();
    // Icons are a nicety: the lists show without them.
    let (icons, complete) = s.api.game_icons(&ids).await;
    for g in sorts.iter_mut().flat_map(|s| s.games.iter_mut()) {
        g.icon_url = icons.get(&g.universe_id).cloned();
    }
    // Only a full answer is kept: without lists or some icons, the next visit asks again (as `game_info`).
    if !sorts.is_empty() && complete {
        let json = serde_json::to_string(&sorts).map_err(|e| AppError::Internal(e.to_string()))?;
        s.db.cache_put(KEY, &json, now_ms())?;
    }
    Ok(sorts)
}

/// A game's details panel: its numbers, votes, passes and badges. Cached for
/// `settings.cache_minutes` (key `gamestats:<universe_id>`), only when every part answered. No
/// cookie. `NotFound` when Roblox knows no such game.
#[tauri::command]
pub async fn game_stats(s: State<'_, App>, universe_id: i64) -> Result<GameStats> {
    let key = format!("gamestats:{universe_id}");
    let ttl = s.db.settings()?.cache_minutes as i64 * 60_000;
    if let Some((json, _)) = s.db.cache_get(&key, ttl, now_ms())?
        && let Ok(stats) = serde_json::from_str::<GameStats>(&json).inspect_err(|e| log::debug!("cached game stats {universe_id} unreadable: {e}"))
    {
        return Ok(stats);
    }
    let one = [universe_id];
    let (game, votes, passes, badges, age, archived, places) = tokio::join!(
        s.api.games(&one),
        s.api.game_votes(universe_id),
        s.api.game_passes(universe_id),
        s.api.game_badges(universe_id),
        s.api.age_rating(universe_id),
        s.api.universe_archived(universe_id),
        s.api.universe_places(universe_id)
    );
    let g = game?.into_iter().find(|g| g.universe_id == universe_id).ok_or(AppError::NotFound)?;
    let part = |what: &str, e: &AppError| log::info!("game {universe_id}: {what} not loaded: {e}");
    let votes = votes.inspect_err(|e| part("votes", e)).ok();
    let passes = passes.inspect_err(|e| part("passes", e)).ok();
    let badges = badges.inspect_err(|e| part("badges", e)).ok();
    let age = age.inspect_err(|e| part("age rating", e)).ok();
    // After the game row: only it says which group made it (7 calls a minute, hence the cache).
    let group_members = match g.group_id {
        // Not waited for long: past its 7 a minute the window would hold the whole panel back.
        Some(id) => match tokio::time::timeout(std::time::Duration::from_secs(3), s.api.group_members(id)).await {
            Ok(r) => r.inspect_err(|e| part("group", e)).ok().flatten(),
            Err(_) => None,
        },
        None => None,
    };
    let complete = votes.as_ref().is_some_and(Option::is_some)
        && passes.as_ref().is_some_and(|l| l.complete)
        && badges.as_ref().is_some_and(|l| l.complete)
        && age.is_some();
    let votes = votes.flatten();
    let stats = GameStats {
        universe_id,
        playing: g.playing,
        visits: g.visits,
        favourites: g.favourites,
        max_players: g.max_players,
        created: g.created,
        updated: g.updated,
        genre: g.genre,
        subgenre: g.subgenre,
        up_votes: votes.map(|v| v.0),
        down_votes: votes.map(|v| v.1),
        more_passes: passes.as_ref().is_some_and(|l| l.more),
        more_badges: badges.as_ref().is_some_and(|l| l.more),
        passes: passes.map(|l| l.items),
        badges: badges.map(|l| l.items),
        age_rating: age.flatten(),
        avatar_type: g.avatar_type,
        copying: g.copying,
        places: places.inspect_err(|e| part("places", e)).ok().filter(|l| !l.more).map(|l| l.items.len() as i64),
        servers: servers_estimate(g.playing, g.max_players),
        group_members,
        archived: archived.inspect_err(|e| part("universe", e)).ok().flatten(),
    };
    // A part missing or cut short is asked for again next time (as `games_popular`).
    if complete {
        let json = serde_json::to_string(&stats).map_err(|e| AppError::Internal(e.to_string()))?;
        s.db.cache_put(&key, &json, now_ms())?;
    }
    Ok(stats)
}

/// About how many servers `playing` players fill (rounded up); None when the size is unknown.
fn servers_estimate(playing: i64, max_players: i32) -> Option<i64> {
    let size = i64::from(max_players);
    (size > 0).then(|| (playing.max(0) + size - 1) / size)
}

/// A player's profile for Browse's player details: who they are, their friends and followers,
/// groups, Roblox badges and games (no cookie; cached `player:<user_id>` for
/// `settings.cache_minutes`, only when every part answered), and where they are now, asked each
/// time as `account_id` (the Browse account: Roblox tells a friend more).
#[tauri::command]
pub async fn player_stats(s: State<'_, App>, account_id: i64, user_id: i64) -> Result<PlayerStats> {
    if user_id <= 0 {
        return Err(AppError::NotFound);
    }
    let cookie = stored_cookie(&s, account_id).inspect_err(|e| log::debug!("player {user_id}: no cookie for presence: {e}")).ok();
    let presence = async {
        let c = cookie.as_ref()?;
        let p = s.api.presence(account_id, c, &[user_id]).await.inspect_err(|e| log::debug!("player {user_id}: presence not loaded: {e}")).ok()?;
        p.into_iter().find(|p| p.user_id == user_id).map(|p| PlayerPresence {
            kind: p.user_presence_type,
            place: p.last_location.filter(|l| p.user_presence_type == 2 && !l.is_empty()),
        })
    };
    let key = format!("player:{user_id}");
    let ttl = s.db.settings()?.cache_minutes as i64 * 60_000;
    if let Some((json, _)) = s.db.cache_get(&key, ttl, now_ms())?
        && let Ok(mut stats) = serde_json::from_str::<PlayerStats>(&json).inspect_err(|e| log::debug!("cached player {user_id} unreadable: {e}"))
    {
        stats.presence = presence.await;
        return Ok(stats);
    }
    let one = [user_id];
    let (user, counts, groups, badges, games, headshot, presence) = tokio::join!(
        // Anonymous: a profile is public, and the Browse account's cookie may be dead.
        s.api.user_details(0, None, user_id),
        s.api.social_counts(user_id),
        s.api.user_groups(user_id),
        s.api.roblox_badges(user_id),
        s.api.user_games(user_id),
        s.api.headshots(&one),
        presence
    );
    let u = user?;
    let part = |what: &str, e: &AppError| log::info!("player {user_id}: {what} not loaded: {e}");
    let counts = counts.inspect_err(|e| part("counts", e)).ok();
    let groups = groups.inspect_err(|e| part("groups", e)).ok();
    let badges = badges.inspect_err(|e| part("badges", e)).ok();
    let games = games.inspect_err(|e| part("games", e)).ok();
    let headshot_url = headshot.inspect_err(|e| part("headshot", e)).ok().and_then(|mut h| h.remove(&user_id));
    let complete = counts.is_some() && groups.is_some() && badges.is_some() && games.as_ref().is_some_and(|l| l.complete);
    let mut stats = PlayerStats {
        user_id,
        name: u.name,
        display_name: u.display_name,
        verified: u.has_verified_badge,
        banned: u.is_banned,
        created: u.created,
        headshot_url,
        friends: counts.map(|c| c.friends),
        followers: counts.map(|c| c.followers),
        following: counts.map(|c| c.following),
        presence: None,
        groups,
        badges,
        more_games: games.as_ref().is_some_and(|l| l.more),
        games: games.map(|l| l.items),
    };
    if complete {
        let json = serde_json::to_string(&stats).map_err(|e| AppError::Internal(e.to_string()))?;
        s.db.cache_put(&key, &json, now_ms())?;
    }
    stats.presence = presence;
    Ok(stats)
}

/// A player's limiteds for Browse's player details, read as `account_id` (the Browse account):
/// the count kept from less than `settings.cache_minutes` ago, else (or with `refresh`) a new one,
/// as `account_value` counts (same key `value:<user_id>`: one of your own accounts shares it).
/// A hidden inventory is `private`, and nothing is kept.
#[tauri::command]
pub async fn player_value(app: AppHandle, s: State<'_, App>, account_id: i64, user_id: i64, refresh: bool) -> Result<PlayerValue> {
    if user_id <= 0 {
        return Err(AppError::NotFound);
    }
    let ttl = s.db.settings()?.cache_minutes as i64 * 60_000;
    let fresh = if refresh { None } else { kept_holdings(&s, user_id, ttl)? };
    let counted = fresh.is_none();
    let held = match fresh {
        Some(h) => h,
        None => {
            let cookie = stored_cookie(&s, account_id)?;
            // Opening a profile never queues behind another count (Count again does).
            match count_limiteds(&s, account_id, &cookie, user_id, refresh, || true).await {
                Ok(Some(h)) => h,
                Ok(None) => match kept_holdings(&s, user_id, i64::MAX / 2)? {
                    Some(h) => h,
                    None => return Ok(PlayerValue { private: false, busy: true, value: None }),
                },
                Err(e) if crate::roblox_api::is_private_inventory(&e) => return Ok(PlayerValue { private: true, busy: false, value: None }),
                Err(e) => {
                    record_cookie_error(&app, &s, account_id, &e)?;
                    return Err(e);
                }
            }
        }
    };
    let rolimons = if s.db.settings()?.rolimons_values { crate::rolimons::items(&s.db, now_ms(), counted).await } else { None };
    Ok(PlayerValue { private: false, busy: false, value: Some(value_of(&held, rolimons.as_deref())) })
}

/// What `account_value` keeps per account (key `value:<user_id>`): what it holds, not what it's
/// worth, so Rolimons values come and go with their setting without a recount.
#[derive(Serialize, Deserialize)]
struct Holdings {
    fetched_at: i64,
    /// Stopped at the page cap: more copies than were read.
    partial: bool,
    items: Vec<Holding>,
}

/// One count at a time, whichever view starts it: Roblox throttles inventory hard.
static COUNTING: std::sync::LazyLock<tokio::sync::Mutex<()>> = std::sync::LazyLock::new(Default::default);

/// The count kept for `user_id`, if younger than `max_age` ms.
fn kept_holdings(s: &App, user_id: i64, max_age: i64) -> Result<Option<Holdings>> {
    let Some((json, _)) = s.db.cache_get(&format!("value:{user_id}"), max_age, now_ms())? else { return Ok(None) };
    Ok(serde_json::from_str(&json).inspect_err(|e| log::debug!("kept limiteds of user {user_id} unreadable: {e}")).ok())
}

/// Counts `user_id`'s limiteds now as `account_id` (its cookie; one count at a time: `wait` for
/// one running, else None). One cut short by Roblox gives way to the count kept before it; else
/// the new one is kept when `keep()` still says so (a cut-short one too, when it's all there is:
/// marked partial).
async fn count_limiteds(s: &App, account_id: i64, cookie: &Secret, user_id: i64, wait: bool, keep: impl Fn() -> bool) -> Result<Option<Holdings>> {
    let _one = if wait {
        COUNTING.lock().await
    } else {
        match COUNTING.try_lock() {
            Ok(g) => g,
            Err(_) => return Ok(None),
        }
    };
    let list = s.api.collectibles(account_id, cookie, user_id).await?;
    if !list.complete
        && let Some(old) = kept_holdings(s, user_id, i64::MAX / 2)?
    {
        log::info!("user {user_id}: count cut short, the last one stays");
        return Ok(Some(old));
    }
    let held = Holdings { fetched_at: now_ms(), partial: list.more, items: list.items };
    if keep() {
        let json = serde_json::to_string(&held).map_err(|e| AppError::Internal(e.to_string()))?;
        s.db.cache_put(&format!("value:{user_id}"), &json, held.fetched_at)?;
    }
    Ok(Some(held))
}

/// The account's limiteds as last counted (None: never), or, with `refresh`, counted now: its
/// whole inventory read with its own cookie, a page a second (`Client::collectibles`), one count
/// at a time. One cut short by Roblox gives way to the count kept before it (its own `fetched_at`
/// says it's older), or, with none, is kept and shown as partial. With `settings.rolimons_values` on, Rolimons' values are added (their list is
/// downloaded only by a count). Without `refresh` nothing is sent anywhere.
#[tauri::command]
pub async fn account_value(app: AppHandle, s: State<'_, App>, id: i64, refresh: bool) -> Result<Option<AccountValue>> {
    let a = s.db.account(id)?;
    let held = if refresh {
        let cookie = stored_cookie(&s, id)?;
        // Not kept if the account was removed while it was counted: nothing of it comes back.
        let still_here = || s.db.account(id).is_ok_and(|now| now.user_id == a.user_id);
        match count_limiteds(&s, id, &cookie, a.user_id, true, still_here).await {
            Ok(Some(h)) => h,
            // Never: a count that waits always counts.
            Ok(None) => return Ok(None),
            Err(e) => {
                record_cookie_error(&app, &s, id, &e)?;
                return Err(e);
            }
        }
    } else {
        let Some(h) = kept_holdings(&s, a.user_id, i64::MAX / 2)? else { return Ok(None) };
        h
    };
    let rolimons = if s.db.settings()?.rolimons_values { crate::rolimons::items(&s.db, now_ms(), refresh).await } else { None };
    Ok(Some(value_of(&held, rolimons.as_deref())))
}

/// Browse › Limiteds: every limited Rolimons lists. Only with `settings.rolimons_values` on (None
/// otherwise). A list older than 30 minutes is downloaded again first (`rolimons::list`); one that
/// can't be has `fetched_at` None and no items.
#[tauri::command]
pub async fn market_items(s: State<'_, App>) -> Result<Option<Market>> {
    if !s.db.settings()?.rolimons_values {
        return Ok(None);
    }
    let Some((at, items)) = crate::rolimons::list(&s.db, now_ms(), true).await else {
        return Ok(Some(Market { fetched_at: None, items: Vec::new() }));
    };
    Ok(Some(Market { fetched_at: Some(at), items: items.iter().map(|(id, item)| (*id, item.clone())).collect() }))
}

/// Item pictures for Browse › Limiteds (no cookie): asset id and image URL, for those Roblox has
/// ready. At most 200 ids a call.
#[tauri::command]
pub async fn asset_thumbnails(s: State<'_, App>, mut ids: Vec<i64>) -> Result<Vec<(i64, String)>> {
    ids.truncate(200);
    Ok(s.api.asset_thumbnails(&ids).await?.into_iter().collect())
}

// ---------- mods ----------

/// The Mods view: every mod (top first) and the last launch's result.
#[tauri::command(async)]
pub fn mods_list(s: State<'_, App>) -> Result<crate::mods::library::ModsList> {
    Ok(crate::mods::library::ModsList { mods: crate::mods::library::list(&s.db)?, last: crate::mods::runtime::last() })
}

/// Imports the zip at `path` (picked in the file dialog) as a new mod, on and at the top.
#[tauri::command]
pub async fn mods_import(app: AppHandle, path: String) -> Result<crate::mods::library::ModImported> {
    crate::launcher::update::blocking(move || crate::mods::library::import(&app.state::<App>().db, Path::new(&path), now_ms())).await
}

/// Writes the mod as a zip at `path` (picked in the save dialog).
#[tauri::command]
pub async fn mods_export(id: String, path: String) -> Result<()> {
    crate::launcher::update::blocking(move || crate::mods::library::export(&id, Path::new(&path))).await
}

/// The file name the save dialog suggests for an export.
#[tauri::command(async)]
pub fn mods_export_name(id: String) -> Result<String> {
    crate::mods::library::export_name(&id)
}

#[tauri::command(async)]
pub fn mods_remove(s: State<'_, App>, id: String) -> Result<()> {
    crate::mods::library::remove(&s.db, &id)
}

#[tauri::command(async)]
pub fn mods_set_on(s: State<'_, App>, id: String, on: bool) -> Result<()> {
    crate::mods::library::set_on(&s.db, &id, on)
}

/// The new order, top first (the top mod wins a file two change).
#[tauri::command(async)]
pub fn mods_reorder(s: State<'_, App>, ids: Vec<String>) -> Result<()> {
    if ids.len() > 1000 {
        return Err(AppError::Internal("too many ids".into()));
    }
    crate::mods::library::reorder(&s.db, &ids)
}

/// Limiteds shown at most (`AccountValue::top`).
const TOP_ITEMS: usize = 10;

/// What `held` is worth (pure): RAP of every copy and, with Rolimons' list, each copy at its value
/// (its RAP when Rolimons doesn't value it), and the most valuable items.
fn value_of(held: &Holdings, rolimons: Option<&crate::rolimons::Items>) -> AccountValue {
    let mut items: Vec<LimitedItem> = held
        .items
        .iter()
        .map(|h| LimitedItem {
            asset_id: h.asset_id,
            name: h.name.clone(),
            copies: h.copies,
            rap: h.rap,
            rolimons: rolimons.and_then(|r| r.get(&h.asset_id)).cloned(),
        })
        .collect();
    let worth = |i: &LimitedItem| i.rolimons.as_ref().and_then(|r| r.value).unwrap_or(i.rap);
    let total = |each: &dyn Fn(&LimitedItem) -> i64| items.iter().map(|i| each(i).saturating_mul(i64::from(i.copies))).fold(0i64, i64::saturating_add);
    let (rap, value) = (total(&|i| i.rap), rolimons.map(|_| total(&worth)));
    let count = items.iter().map(|i| i.copies).fold(0u32, u32::saturating_add);
    items.sort_by(|a, b| worth(b).cmp(&worth(a)).then(b.copies.cmp(&a.copies)).then_with(|| a.name.cmp(&b.name)));
    items.truncate(TOP_ITEMS);
    AccountValue { count, rap, value, top: items, partial: held.partial, fetched_at: held.fetched_at }
}

fn game_info_of(place_id: i64, g: GameDetails, icon_url: Option<String>) -> GameInfo {
    GameInfo {
        place_id,
        universe_id: g.universe_id,
        root_place_id: g.root_place_id,
        name: g.name,
        creator: g.creator,
        playing: g.playing,
        visits: g.visits,
        max_players: g.max_players,
        icon_url,
        private_servers: g.private_servers,
    }
}

/// The place a target leads to, for the server browser: its own place, a universe's root
/// place, or where a share link points (resolved with `account_id`'s cookie, which share links
/// need). `Invalid` for targets without a game (a user to follow).
#[tauri::command]
pub async fn target_place(app: AppHandle, s: State<'_, App>, target: LaunchTarget, account_id: Option<i64>) -> Result<i64> {
    let no_game = || AppError::Invalid(tr!("That link doesn't lead to a game."));
    match &target {
        LaunchTarget::Place { place_id, .. }
        | LaunchTarget::Server { place_id, .. }
        | LaunchTarget::PrivateLink { place_id, .. }
        | LaunchTarget::PrivateAccess { place_id, .. } => Ok(*place_id),
        LaunchTarget::Universe { universe_id } => crate::launcher::root_place(&s.api, *universe_id).await,
        LaunchTarget::ShareLink { .. } | LaunchTarget::ShareLinkV2 { .. } => {
            let id = account_id.ok_or_else(|| AppError::Invalid(tr!("Pick an account first: share links open as an account.")))?;
            let cookie = stored_cookie(&s, id)?;
            match crate::launcher::resolve(&s.api, id, &cookie, &target).await {
                Ok((join, _)) => join.place_id().ok_or_else(no_game),
                Err(e) => {
                    record_cookie_error(&app, &s, id, &e)?;
                    Err(e)
                }
            }
        }
        LaunchTarget::FollowUser { .. } => Err(no_game()),
    }
}

/// One page of the place's private servers as `account_id` sees them (its cookie; joinable ones
/// carry an access code). `Locked` when the vault is locked.
#[tauri::command]
pub async fn private_servers_list(
    app: AppHandle,
    s: State<'_, App>,
    place_id: i64,
    account_id: i64,
    cursor: Option<String>,
) -> Result<PrivateServerPage> {
    let cookie = stored_cookie(&s, account_id)?;
    let page = s.api.private_servers(account_id, &cookie, place_id, cursor.as_deref()).await;
    if let Err(e) = &page {
        record_cookie_error(&app, &s, account_id, e)?;
    }
    page
}

/// One page of a place's public servers (`cursor` from the previous page). With `account_id`,
/// asked with that account's cookie (needed for Recommended / BestLatency; a guest gets 3
/// requests a minute). `limit` is 10, 25, 50 or 100.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // one per IPC argument
pub async fn servers_list(
    app: AppHandle,
    s: State<'_, App>,
    place_id: i64,
    order: ServerOrder,
    exclude_full: bool,
    limit: u8,
    cursor: Option<String>,
    account_id: Option<i64>,
) -> Result<ServerPage> {
    let cookie = account_id.map(|id| stored_cookie(&s, id)).transpose()?;
    let id = account_id.unwrap_or(0);
    let page = s.api.servers_public(id, cookie.as_ref(), place_id, order, exclude_full, limit, cursor.as_deref()).await;
    if let (Err(e), Some(id)) = (&page, account_id) {
        record_cookie_error(&app, &s, id, e)?;
    }
    page
}

/// A failed Roblox action of `account_id`, for the UI: a dead cookie is recorded on the account.
/// A challenge on one Robux, settings or avatar action is that action's (like befriending's), so
/// the account isn't marked Challenged and stays usable.
fn action_error(app: &AppHandle, s: &App, account_id: i64, e: AppError) -> AppError {
    if matches!(e, AppError::Challenge) {
        return AppError::Invalid(tr!("Roblox wants a verification for this: do it on roblox.com, then try again."));
    }
    match record_cookie_error(app, s, account_id, &e) {
        Err(stored) => stored,
        Ok(_) => e,
    }
}

/// The account's newest 25 Robux transactions of one kind.
#[tauri::command]
pub async fn robux_transactions(app: AppHandle, s: State<'_, App>, account_id: i64, kind: RobuxKind) -> Result<Vec<RobuxTransaction>> {
    unlocked(&s)?;
    let user_id = s.db.account(account_id)?.user_id;
    let cookie = stored_cookie(&s, account_id)?;
    s.api.transactions(account_id, &cookie, user_id, kind).await.map_err(|e| action_error(&app, &s, account_id, e))
}

/// What came in, went out and is pending in the account's last month of Robux.
#[tauri::command]
pub async fn robux_totals(app: AppHandle, s: State<'_, App>, account_id: i64) -> Result<RobuxTotals> {
    unlocked(&s)?;
    let user_id = s.db.account(account_id)?.user_id;
    let cookie = stored_cookie(&s, account_id)?;
    s.api.transaction_totals(account_id, &cookie, user_id).await.map_err(|e| action_error(&app, &s, account_id, e))
}

// ---------- instances ----------

pub fn instances_changed(app: &AppHandle) {
    let _ = app.emit("instances://changed", ());
    crate::tray::refresh(app);
}

#[tauri::command]
pub fn instances_list(s: State<'_, App>) -> Vec<Instance> {
    s.tracker.list()
}

/// Asks one client to close (graceful unless `force`); not treated as an unexpected exit.
#[tauri::command]
pub async fn instance_close(s: State<'_, App>, id: String, force: bool) -> Result<()> {
    s.tracker.get(&id).ok_or(AppError::NotFound)?;
    // Closed by the user: a relaunch waiting on it is over.
    s.tracker.end_relaunch(&id);
    // Its process already gone: the next poll ends the entry.
    let Some(target) = s.tracker.mark_closing(&id) else {
        return Ok(());
    };
    let account_id = s.tracker.get(&id).map(|i| i.account_id);
    let r = close_all(vec![target], force, grace(&s))
        .await?
        .pop()
        .unwrap_or(Ok(()));
    if r.is_err() {
        s.tracker.clear_closing(&id);
    } else if let Some(account_id) = account_id {
        remove_secrets_if_stopped(&s, account_id);
    }
    r
}

/// Deletes an account's plaintext session once none of its clients is live: Cordial's profile
/// cookie/identity and Mocktail's per-account `roblox.cookie`, both removed by
/// `launcher::client::remove_profile_secrets` (the path logic stays in the launcher lane). The
/// encrypted vault remains the source for the next launch.
///
/// `process::profiles_by_user` knows both backends: a running Mocktail is read from its
/// `MOCKTAIL_AUTH_ROOT`, so an account whose client is still up keeps its plaintext session.
pub(crate) fn remove_secrets_if_stopped(s: &App, account_id: i64) {
    let Some(user_id) = s.db.account(account_id).ok().map(|a| a.user_id) else {
        return;
    };
    match crate::instances::process::profiles_by_user() {
        Some(users) if !users.contains(&user_id) => {
            crate::launcher::client::remove_profile_secrets(user_id)
        }
        Some(_) => {}
        None => log::debug!("kept Cordial profile {user_id}: could not scan running profiles"),
    }
}

/// Clear every stopped account's plaintext Cordial session. Called at start and exit; a profile
/// with a live Cordial process is left alone, so a client RoALT keeps running stays signed in.
pub(crate) fn remove_all_stopped_secrets(s: &App) {
    if let Ok(accounts) = s.db.accounts() {
        for a in accounts {
            remove_secrets_if_stopped(s, a.id);
        }
    }
}

/// Closes one client because its account's run reached `Settings.close_after_h`: as the user's own
/// close, so nothing brings it back; the user is told why it went.
pub(crate) async fn close_timed(app: &AppHandle, id: &str) -> Result<()> {
    let s = app.state::<App>();
    let Some(i) = s.tracker.get(id) else { return Ok(()) };
    s.tracker.end_relaunch(id);
    let Some(target) = s.tracker.mark_closing(id) else { return Ok(()) };
    let r = close_all(vec![target], false, grace(&s)).await?.pop().unwrap_or(Ok(()));
    if r.is_err() {
        // Still running: the next check tries again.
        s.tracker.clear_closing(id);
    } else {
        let name = s.db.account(i.account_id).map_or_else(|_| tr!("An account"), |a| crate::launcher::bulk::name_of(&a));
        notice(app, false, tr!("{name} was closed: its run reached the time set in Settings.", name = name));
    }
    instances_changed(app);
    r
}

/// Moves a playing account's client to `target` (asked first in the UI): its client is closed, then
/// the account launches again. Into the same game it was in (no particular server), it hops to the
/// smallest other public server (`bulk::smallest_other`), or as Roblox matches it when there is
/// none; into the very server it is in, refused. The launch is checked before anything is closed,
/// so a refused one keeps the client. `fps_cap`: this launch's own, else the old client's.
#[tauri::command]
pub async fn instance_move(
    app: AppHandle,
    s: State<'_, App>,
    id: String,
    target: LaunchTarget,
    fps_cap: Option<u32>,
) -> Result<LaunchInfo> {
    let fps_cap = launch_fps(fps_cap)?;
    let _single = s.bulk.begin_single_launch()?;
    let kills = s.bulk.kills();
    let inst = s.tracker.get(&id).ok_or(AppError::NotFound)?;
    if let Some(why) = crate::network::blocked(&s) {
        return Err(AppError::Wait(why));
    }
    let target = hop_target(&s, &inst, target).await?;
    if let Some(why) = crate::launcher::update::update_hold(&s, 1).await {
        return Err(AppError::Wait(why));
    }
    crate::launcher::launch_check(&s, inst.account_id, &target).await?;
    s.tracker.end_relaunch(&id);
    if let Some(pid) = s.tracker.mark_closing(&id) {
        if let Err(e) = close_all(vec![pid], false, grace(&s)).await?.pop().unwrap_or(Ok(())) {
            s.tracker.clear_closing(&id);
            return Err(e);
        }
        // Still live in the tracker until its next poll ends it, and a launch then is refused.
        let deadline = Instant::now() + Duration::from_secs(5);
        while s.tracker.get(&id).is_some_and(|i| i.state != InstanceState::Exited) && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
    // Kill all pressed while the old client closed: no new one.
    if s.bulk.kills() != kills {
        return Err(AppError::Invalid(tr!("Launch cancelled by Stop all.")));
    }
    log::info!("moving account {} from instance {id}", inst.account_id);
    Ok(launch_tracked(&app, &s, inst.account_id, target, None, fps_cap.or(inst.fps_cap), false, None).await?.0)
}

/// Where `instance_move` sends `inst`: the smallest other public server when `target` is the game
/// it is in; the target itself otherwise (and when the client hasn't reached a server yet).
async fn hop_target(s: &App, inst: &Instance, target: LaunchTarget) -> Result<LaunchTarget> {
    let (Some(place), Some(job)) = (inst.place_id, inst.job_id.as_deref()) else { return Ok(target) };
    let cookie = crate::launcher::account_cookie(s, inst.account_id)?;
    let (join, _) = crate::launcher::resolve(&s.api, inst.account_id, &cookie, &target).await?;
    match join {
        crate::launcher::Join::Job { job_id, .. } if job_id.eq_ignore_ascii_case(job) => {
            Err(AppError::Invalid(tr!("This account is already in that server.")))
        }
        crate::launcher::Join::Game { place_id, launch_data: None } if place_id == place => {
            let servers = crate::launcher::bulk::server_page(&s.api, inst.account_id, &cookie, place, crate::model::ServerOrder::OccupancyAsc).await?;
            Ok(crate::launcher::bulk::smallest_other(&servers, job).map_or(target, |job_id| LaunchTarget::Server { place_id, job_id }))
        }
        _ => Ok(target),
    }
}

/// How long a closed client gets before it is terminated (`settings.close_grace_s`).
fn grace(s: &App) -> Duration {
    Duration::from_secs(u64::from(
        s.db.settings().inspect_err(|e| log::debug!("could not read settings, close grace 5 s: {e}")).map_or(5, |v| v.close_grace_s),
    ))
}

/// Closes the processes at once (each `process::close` waits up to `grace`), so the whole
/// thing takes one grace period, not one per client. Results in input order.
async fn close_all(targets: Vec<(u32, Option<u64>)>, force: bool, grace: Duration) -> Result<Vec<Result<()>>> {
    blocking(move || {
        std::thread::scope(|sc| {
            let jobs: Vec<_> = targets
                .into_iter()
                .map(|(pid, t0)| sc.spawn(move || crate::instances::process::close(pid, t0, force, grace)))
                .collect();
            jobs.into_iter()
                .map(|j| j.join().unwrap_or_else(|_| Err(AppError::Internal("close panicked".into()))))
                .collect()
        })
    }).await
}

#[tauri::command]
pub async fn instances_close_all(s: State<'_, App>) -> Result<()> {
    close_running(&s).await
}

/// Asks every running client to close (also the tray's "Close all clients").
pub(crate) async fn close_running(s: &App) -> Result<()> {
    s.tracker.cancel_relaunches();
    let running: Vec<Instance> = s
        .tracker
        .list()
        .into_iter()
        .filter(|i| i.state != InstanceState::Exited)
        .collect();
    let mut ids = Vec::new();
    let mut targets = Vec::new();
    for i in &running {
        if let Some(target) = s.tracker.mark_closing(&i.id) {
            ids.push((i.id.clone(), i.account_id));
            targets.push(target);
        }
    }
    for ((id, account_id), r) in ids.iter().zip(close_all(targets, false, grace(s)).await?) {
        if let Err(e) = r {
            log::warn!("could not close Roblox client of instance {id}: {e}");
            s.tracker.clear_closing(id);
        } else {
            remove_secrets_if_stopped(s, *account_id);
        }
    }
    Ok(())
}

/// Logical processors (for auto-spread; affinity masks cover at most 64).
pub fn cores() -> u32 {
    std::thread::available_parallelism().map_or(1, |n| n.get() as u32)
}

/// A launch's own FPS cap: None or 0 for none (the account's or the default applies), else
/// `launcher::client_settings::RANGE`.
pub(crate) fn launch_fps(f: Option<u32>) -> Result<Option<u32>> {
    let f = f.filter(|&f| f != 0);
    if let Some(f) = f {
        check_fps(f)?;
    }
    Ok(f)
}

/// An FPS cap the user can set: 0 (none, or the default for an account) or `launcher::client_settings::RANGE`.
fn check_fps(f: u32) -> Result<()> {
    if f != 0 && !crate::launcher::client_settings::RANGE.contains(&f) {
        return Err(AppError::Invalid(tr!("The FPS cap must be 0 (off) or 5 to 360.")));
    }
    Ok(())
}

/// Limits the user can set: a CPU cap of 1-100 %, a trim every 0-1440 minutes, at least one core.
fn check_limits(l: &Limits) -> Result<()> {
    if l.cpu_cap.is_some_and(|c| !(1..=100).contains(&c)) {
        return Err(AppError::Invalid(tr!("The CPU limit must be between 1 and 100 %.")));
    }
    if l.trim_every_min.is_some_and(|m| m > 24 * 60) {
        return Err(AppError::Invalid(tr!("Trimming can be at most once a day apart.")));
    }
    if l.affinity == Some(0) {
        return Err(AppError::Invalid(tr!("Pick at least one CPU core.")));
    }
    Ok(())
}

/// Sets one client's own limits (CPU cap, efficiency mode, trim interval, cores); they win
/// over the adaptive manager's and fill in the defaults (`limits::plan`). A client still behind
/// its bootstrapper gets them once it is found.
#[tauri::command]
pub async fn instance_set_limits(app: AppHandle, s: State<'_, App>, id: String, limits: Limits) -> Result<()> {
    check_limits(&limits)?;
    let (prev, client) = s.tracker.set_limits(&id, limits).ok_or(AppError::NotFound)?;
    instances_changed(&app);
    let kept = id.clone();
    if client.is_none() {
        remember_client(&app, &s, &kept);
        return Ok(());
    }
    let app2 = app.clone();
    let r = blocking(move || {
        let s = app2.state::<App>();
        let v = s.db.settings().unwrap_or_else(|e| {
            log::warn!("could not read settings for the limits of {id}; using defaults: {e}");
            Default::default()
        });
        let held = s.tracker.throttle(v.mem_throttle.mb).0;
        let plan = crate::instances::limits::plan_now(&s, &v, &s.tracker.clients(), &held);
        match plan.into_iter().find(|c| c.0 == id) {
            Some((_, pid, t0, l)) => s.limiter.apply(&id, pid, t0, l).map_err(|e| (id, e)),
            None => Ok(()),
        }
    }).await?;
    if let Err((id, e)) = r {
        s.tracker.set_limits(&id, prev);
        instances_changed(&app);
        return Err(e);
    }
    remember_client(&app, &s, &kept);
    Ok(())
}

/// Applies `limits` to every currently running client without overwriting each account's saved
/// `client_limits`. Returns how many running clients were updated.
#[tauri::command]
pub async fn instances_apply_limits(app: AppHandle, s: State<'_, App>, limits: Limits) -> Result<u32> {
    check_limits(&limits)?;
    let ids: Vec<String> = s
        .tracker
        .list()
        .into_iter()
        .filter(|i| i.state != InstanceState::Exited)
        .map(|i| i.id)
        .collect();
    let mut updated = 0u32;
    for id in &ids {
        if s.tracker.set_limits(id, limits).is_some() {
            updated += 1;
        }
    }
    instances_changed(&app);
    let app2 = app.clone();
    blocking(move || {
        let s = app2.state::<App>();
        let v = s.db.settings().unwrap_or_default();
        let held = s.tracker.throttle(v.mem_throttle.mb).0;
        let plan = crate::instances::limits::plan_now(&s, &v, &s.tracker.clients(), &held);
        for (id, pid, t0, l) in plan {
            if let Err(e) = s.limiter.apply(&id, pid, t0, l) {
                log::warn!("bulk limits: could not apply to {id}: {e}");
            }
        }
    })
    .await?;
    Ok(updated)
}

/// Keeps what client `id` has now (its limits, its anti-AFK) for its account's next client.
fn remember_client(app: &AppHandle, s: &App, id: &str) {
    let Some(i) = s.tracker.get(id) else { return };
    let prefs = crate::model::ClientPrefs { limits: i.limits, anti_afk: i.anti_afk };
    // Nothing of its own to keep when this is what its clients start with anyway: its groups', or
    // the defaults. Kept when it differs, also when it is the defaults themselves: that is how an
    // account opts out of its group's limits.
    let inherited = s.db.account(i.account_id).and_then(|a| {
        let groups = s.db.groups()?;
        Ok(crate::storage::with_group_defaults(AccountSummary { client_limits: None, ..a }, &groups).client_limits.unwrap_or_default())
    });
    let keep = if inherited.is_ok_and(|base| base == prefs) { None } else { Some(&prefs) };
    match s.db.set_client_limits(i.account_id, keep) {
        // The account's Overrides tab shows it.
        Ok(()) => accounts_changed(app),
        Err(e) => log::warn!("the limits of account {} aren't remembered: {e}", i.account_id),
    }
}

/// Opts one running client in or out of anti-AFK.
#[tauri::command]
pub fn instance_set_anti_afk(app: AppHandle, s: State<'_, App>, id: String, enabled: bool) -> Result<()> {
    if !s.tracker.set_anti_afk(&id, enabled) {
        return Err(AppError::NotFound);
    }
    instances_changed(&app);
    remember_client(&app, &s, &id);
    Ok(())
}

/// Takes control of one running client ("Take control") or gives it back: RoALT leaves a held
/// client alone (`Instance::held`). Taking it also puts its window back to its size, unmutes it and
/// brings it to the front.
#[tauri::command]
pub async fn instance_take_control(app: AppHandle, s: State<'_, App>, id: String, on: bool) -> Result<()> {
    if !s.tracker.set_held(&id, on) {
        return Err(AppError::NotFound);
    }
    // A relaunch waiting for it ends now.
    crate::instances::rejoin::retry_now();
    instances_changed(&app);
    let Some(pid) = client_pid(&s, &id)?.filter(|_| on) else { return Ok(()) };
    if s.tracker.get(&id).is_some_and(|i| i.micro) {
        let id2 = id.clone();
        blocking(move || crate::instances::window::micro(&id2, pid, false, (0, 0))).await??;
        s.tracker.set_micro(&id, None);
        instances_changed(&app);
    }
    // Not awaited: it may wait seconds for the client's audio.
    tauri::async_runtime::spawn_blocking(move || crate::instances::audio::set_mute(pid, false));
    blocking(move || crate::instances::window::show(pid, crate::instances::window::Show::Focus)).await?
}

/// Pages one client's working set out now ("Trim now").
#[tauri::command]
pub async fn instance_trim(app: AppHandle, s: State<'_, App>, id: String) -> Result<(u64, u64)> {
    s.tracker.get(&id).ok_or(AppError::NotFound)?;
    let (_, pid, t0, _) = s
        .tracker
        .clients()
        .into_iter()
        .find(|c| c.0 == id)
        .ok_or_else(|| AppError::Invalid(tr!("That client hasn't started yet, or has closed.")))?;
    blocking(move || app.state::<App>().limiter.logged_trim("by hand", pid, t0)).await?
}

/// The pid of the running client `id` once it is known to be the client itself (None while
/// only its bootstrapper is seen); NotFound when there is no such instance.
fn client_pid(s: &App, id: &str) -> Result<Option<u32>> {
    s.tracker.get(id).ok_or(AppError::NotFound)?;
    Ok(s.tracker.clients().into_iter().find(|c| c.0 == id).map(|c| c.1))
}

fn not_started() -> AppError {
    AppError::Invalid(tr!("That client hasn't started yet, or has closed."))
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| AppError::Internal(e.to_string()))
}

/// One client's preview as raw bytes, a newer picture asked at most `max_w` (16..=1280) pixels wide,
/// none for 0 (previews Off, between stills): a 24-byte header (`window::Preview::to_bytes`), then
/// the picture's RGBA unless `known` is its number already. Answers at once with the newest picture there is.
#[tauri::command]
pub async fn instance_thumbnail(s: State<'_, App>, id: String, max_w: u32, known: u32) -> Result<tauri::ipc::Response> {
    let pid = client_pid(&s, &id)?;
    let max_w = (max_w > 0).then(|| max_w.clamp(16, 1280));
    let preview = blocking(move || crate::instances::window::preview(pid, max_w)).await?;
    Ok(tauri::ipc::Response::new(preview.to_bytes(known)))
}

/// Frame-free previews (`instances::thumbs`), where live ones would get Windows' yellow frame: each
/// client in `places` drawn over its card, the others taken away.
#[cfg(windows)]
#[tauri::command]
pub fn instance_thumbs(app: AppHandle, s: State<'_, App>, places: Vec<crate::instances::thumbs::ThumbPlace>) -> Result<()> {
    let w = app.get_webview_window("main").ok_or(AppError::NotFound)?;
    // Tauri's HWND comes from its own `windows` version.
    let dest = windows::Win32::Foundation::HWND(w.hwnd()?.0 as _);
    let window_of = |id: &str| {
        let pid = client_pid(&s, id).ok().flatten()?;
        crate::instances::process::main_window(pid).filter(|&(_, minimized)| !minimized).map(|(h, _)| h)
    };
    crate::instances::thumbs::place(dest, w.scale_factor()?, &places, window_of);
    Ok(())
}

/// Linux previews are live captures of the client window (`instances::capture`), with no DWM
/// thumbnails to place: nothing to do.
#[cfg(not(windows))]
#[tauri::command]
pub fn instance_thumbs(_app: AppHandle, _s: State<'_, App>, places: Vec<serde_json::Value>) -> Result<()> {
    let _ = places;
    Ok(())
}

/// Linux: whether this instance has a window on the host desktop. A headless client runs inside
/// `cage`, so it has none; a windowed one keeps its window even while hidden.
#[cfg(not(windows))]
fn instance_has_window(s: &App, id: &str) -> bool {
    let pid = s.tracker.get(id).map(|i| i.pid).unwrap_or(0);
    pid != 0 && crate::instances::window::has_window(pid)
}

/// Linux: switch one running client between the host-window and headless forms. A headless
/// client's window lives inside `cage` and cannot be adopted by the host, so this closes the
/// current process and relaunches the same account/target in the requested form. Windowed to
/// headless is the only way to actually reclaim its render/window memory.
#[cfg(not(windows))]
async fn set_instance_headless(app: &AppHandle, s: &App, id: &str, headless: bool) -> Result<()> {
    let inst = s.tracker.get(id).ok_or(AppError::NotFound)?;
    let (account_id, target) = (inst.account_id, inst.target.clone());
    if let Some((pid, t0)) = s.tracker.mark_closing(id) {
        let _ = close_all(vec![(pid, t0)], false, grace(s)).await?;
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    while std::time::Instant::now() < deadline {
        if s.tracker.get(id).is_none_or(|i| i.state == InstanceState::Exited) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    launch_tracked(app, s, account_id, target, None, None, true, Some(headless)).await?;
    Ok(())
}

/// Brings one client's window to the front, minimizes or restores it. On Linux, minimizing a
/// windowed client relaunches it headless; restoring/clicking a headless client relaunches it
/// with a window.
#[tauri::command]
pub async fn instance_show(app: AppHandle, s: State<'_, App>, id: String, what: crate::instances::window::Show) -> Result<()> {
    #[cfg(not(windows))]
    {
        let has = instance_has_window(&s, &id);
        match what {
            crate::instances::window::Show::Minimize if has => {
                set_instance_headless(&app, &s, &id, true).await
            }
            crate::instances::window::Show::Minimize => Ok(()),
            crate::instances::window::Show::Focus | crate::instances::window::Show::Restore if has => {
                let pid = client_pid(&s, &id)?.ok_or_else(not_started)?;
                blocking(move || crate::instances::window::show(pid, what)).await?
            }
            crate::instances::window::Show::Focus | crate::instances::window::Show::Restore => {
                set_instance_headless(&app, &s, &id, false).await
            }
        }
    }
    #[cfg(windows)]
    {
        let pid = client_pid(&s, &id)?.ok_or_else(not_started)?;
        blocking(move || crate::instances::window::show(pid, what)).await?
    }
}

/// Minimizes or restores every running client's window (focus is for one client). On Linux,
/// minimize switches every windowed client to headless and restore brings every headless client
/// back to a window; already-headless/already-windowed clients are left alone.
#[tauri::command]
pub async fn instances_show_all(app: AppHandle, s: State<'_, App>, what: crate::instances::window::Show) -> Result<()> {
    if what == crate::instances::window::Show::Focus {
        return Err(AppError::Invalid(tr!("Only one client can be in front.")));
    }
    #[cfg(not(windows))]
    {
        let ids: Vec<String> = s
            .tracker
            .list()
            .into_iter()
            .filter(|i| i.state != InstanceState::Exited)
            .map(|i| i.id)
            .collect();
        for id in ids {
            let has = instance_has_window(&s, &id);
            match what {
                crate::instances::window::Show::Minimize if has => {
                    set_instance_headless(&app, &s, &id, true).await?;
                }
                crate::instances::window::Show::Restore if !has => {
                    set_instance_headless(&app, &s, &id, false).await?;
                }
                _ => {}
            }
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        let pids: Vec<u32> = s.tracker.clients().into_iter().map(|c| c.1).collect();
        // One without a window yet has nothing to show or hide.
        blocking(move || crate::instances::window::show_all(&pids, what)).await
    }
}

/// Makes one client's window small (`window::MICRO` client area) or puts it back as it was.
/// Returns the client area it got: Roblox may keep it bigger than asked.
#[tauri::command]
pub async fn instance_micro(app: AppHandle, s: State<'_, App>, id: String, on: bool, size: Option<(u32, u32)>) -> Result<(u32, u32)> {
    let pid = client_pid(&s, &id)?.ok_or_else(not_started)?;
    let id2 = id.clone();
    let v = s.db.settings()?;
    // Its account's own performance profile can set the tiny size.
    let own = s.tracker.get(&id).and_then(|i| s.db.account(i.account_id).ok()?.profile);
    let v = crate::performance::overlay(&v, own.as_ref());
    let wanted = size.unwrap_or((v.micro_width, v.micro_height));
    check_window_size(wanted)?;
    let size = blocking(move || crate::instances::window::micro(&id2, pid, on, wanted)).await??;
    s.tracker.set_micro(&id, on.then_some(wanted));
    instances_changed(&app);
    Ok(size)
}

/// A size a window can be given: small enough for lots of clients, none of it zero.
fn check_window_size((w, h): (u32, u32)) -> Result<()> {
    if !(16..=7680).contains(&w) || !(16..=4320).contains(&h) {
        return Err(AppError::Invalid(tr!("A window can be 16 to 7680 pixels wide and 16 to 4320 high.")));
    }
    Ok(())
}

/// Lays every running client's window out in a grid on the monitor RoALT is on, `micro`-sized
/// or as big as all fit (`window::tile`; minimized ones are left alone). How many it laid out.
#[tauri::command]
pub async fn instances_tile(app: AppHandle, s: State<'_, App>, micro: bool) -> Result<usize> {
    let v = s.db.settings()?;
    let layout_on = v.window_layout != WindowLayout::Off;
    // With a window layout on, the accounts the user plays stay out of it here too: they take no cell.
    let played = if layout_on { crate::instances::limits::played(&s) } else { Vec::new() };
    let account_of = |id: &str| s.tracker.get(id).map(|i| i.account_id);
    // Held ones (Take control) stay where the user has them, layout or not.
    let left_alone = |id: &str| s.tracker.get(id).is_some_and(|i| i.held || played.contains(&i.account_id));
    let clients: Vec<(String, u32)> = s.tracker.clients().into_iter().filter(|c| !left_alone(&c.0)).map(|c| (c.0, c.1)).collect();
    // As a number: a window handle can't cross threads.
    #[cfg(windows)]
    let near = app.get_webview_window("main").and_then(|w| w.hwnd().ok()).map_or(0, |h| h.0 as isize);
    #[cfg(not(windows))]
    let near = app.get_webview_window("main").map_or(0, |_| 0);
    let size = micro.then_some((v.micro_width, v.micro_height));
    // A re-layout still placing stops: Tile places every window now.
    s.tracker.layout_gen.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    #[cfg(windows)]
    let cells = blocking(move || crate::instances::window::tile(&clients, windows::Win32::Foundation::HWND(near as *mut _), size)).await?;
    // Linux's `tile` reports the clients it placed, not an optional placed size: a tiled client is
    // micro-sized (or not) together, as the whole grid was asked.
    #[cfg(not(windows))]
    let cells: Vec<(String, Option<(u32, u32)>)> =
        blocking(move || crate::instances::window::tile(&clients, near, size).into_iter().map(|id| (id, size)).collect()).await?;
    #[cfg(windows)]
    for (id, placed) in &cells {
        // None is a cell that failed: keep what the client had.
        if let Some(small) = placed {
            s.tracker.set_micro(id, *small);
        }
    }
    #[cfg(not(windows))]
    for (id, placed) in &cells {
        s.tracker.set_micro(id, *placed);
    }
    if layout_on {
        // This cell order is the layout's now (a failed cell keeps its client's slot, so the rest
        // stay matched): later clients go after these, not over them.
        s.tracker.set_slots(cells.iter().filter_map(|(id, _)| account_of(id)).collect(), !micro && v.window_layout == WindowLayout::Fit);
        // Clients Tile left alone (minimized, still loading) keep a slot, after these. The count
        // then differs from the grid's, so the next Fit arrival lays every window out again.
        let mut rest: Vec<i64> = s
            .tracker
            .list()
            .into_iter()
            .filter(|i| i.state != crate::model::InstanceState::Exited && !played.contains(&i.account_id))
            .map(|i| i.account_id)
            .collect();
        // A running batch's accounts not started yet keep their list order too.
        for a in s.tracker.pending_slots() {
            if !played.contains(&a) && !rest.contains(&a) {
                rest.push(a);
            }
        }
        s.tracker.reserve_slots(&rest, false);
    }
    instances_changed(&app);
    #[cfg(windows)]
    let laid_out = cells.iter().filter(|c| c.1.is_some()).count();
    #[cfg(not(windows))]
    let laid_out = cells.len();
    Ok(laid_out)
}

/// How long the layout preview stays on screen.
#[cfg(windows)]
const PREVIEW_FOR: Duration = Duration::from_secs(6);
/// Bumped by each preview: an older one's timer leaves a newer window alone.
#[cfg(windows)]
static PREVIEW_ROUND: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Draws the window layout's spots for launching `account_ids` (in list order) over RoALT's monitor
/// for a few seconds: numbered (launch order) with the account's name, spots of clients already
/// running marked `running` (the caller's translated word). The same slots and cells a launch gets
/// (`Tracker::preview_slots`, `window::slot_rect`). The window is see-through and click-through,
/// takes no focus and has no IPC: what it draws comes in with it.
#[cfg(windows)]
#[tauri::command]
pub async fn layout_preview(app: AppHandle, s: State<'_, App>, account_ids: Vec<i64>, running: String) -> Result<()> {
    use std::sync::atomic::Ordering;
    use tauri::{PhysicalPosition, PhysicalSize};
    let v = s.db.settings()?;
    if v.window_layout == WindowLayout::Off {
        return Err(AppError::Invalid(tr!("Pick Fit or Tiny to see where the windows go.")));
    }
    // As in a launch: the accounts the user plays keep their window (but still launch in their turn).
    let accounts = s.db.accounts()?;
    let played = crate::instances::limits::played(&s);
    let ids: Vec<i64> = account_ids.iter().copied().filter(|id| accounts.iter().any(|a| a.id == *id) && !played.contains(id)).collect();
    let main = app.get_webview_window("main").ok_or(AppError::Internal("no main window".into()))?;
    let area = crate::instances::window::work_area(main.hwnd()?).ok_or(AppError::Internal("monitor unreadable".into()))?;
    let (slots, live) = s.tracker.preview_slots(&ids);
    let frame = crate::instances::frame_size(s.tracker.clients().iter().map(|c| c.1));
    let name = |id: i64| accounts.iter().find(|a| a.id == id).map(crate::launcher::bulk::name_of).unwrap_or_default();
    let cells: Vec<serde_json::Value> = slots
        .iter()
        .enumerate()
        .filter_map(|(slot, &account)| {
            let ((x, y, w, h), _) = crate::instances::window::slot_rect(v.window_layout, slot, slots.len(), area, frame, (v.micro_width, v.micro_height))?;
            // The place in the real launch order, which counts full-quality accounts too.
            let order = ids.contains(&account).then(|| account_ids.iter().position(|&a| a == account).map(|i| i + 1)).flatten();
            // Physical px from the monitor's work area: the page scales them by its own DPI.
            Some(serde_json::json!({ "x": x - area.0, "y": y - area.1, "w": w, "h": h, "order": order, "name": name(account), "running": order.is_none() && live.contains(&account) }))
        })
        .collect();
    if cells.is_empty() {
        return Err(AppError::Invalid(tr!("Pick accounts to see where their windows go.")));
    }
    let data = serde_json::json!({ "cells": cells, "running": running });
    close_layout_previews(&app);
    // A label of its own: a destroyed window's label stays taken until the event loop has ended it.
    let label = format!("{PREVIEW}-{}", PREVIEW_ROUND.fetch_add(1, Ordering::Relaxed) + 1);
    // In before the page's own scripts: it renders this instead of the app.
    let mut builder = WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("index.html".into()));
    if let Some(args) = crate::cdp_args() {
        builder = builder.additional_browser_args(&args);
    }
    let w = builder
        .initialization_script(format!("window.__ROALT_LAYOUT_PREVIEW__ = {data};"))
        .title("RoALT")
        // The main window's WebView2 profile: none of its own outside RoALT's data folder.
        .data_directory(s.data_dir.join("webview"))
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        // Never the foreground window: RoALT keeps the keyboard.
        .focusable(false)
        .resizable(false)
        .visible(false)
        // Only once the page is in (and see-through): before, it would cover the screen in the app's background.
        .on_page_load(|w, p| {
            if p.event() == tauri::webview::PageLoadEvent::Finished {
                let _ = w.show();
            }
        })
        .build()?;
    // Gone after its time whatever happens below.
    let timer = (app.clone(), label);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(PREVIEW_FOR).await;
        if let Some(w) = timer.0.get_webview_window(&timer.1) {
            let _ = w.destroy();
        }
    });
    // Not placed or not click-through: never shown (it would sit over the screen taking clicks).
    let placed = w
        .set_position(PhysicalPosition::new(area.0, area.1))
        .and_then(|()| w.set_size(PhysicalSize::new(area.2.max(1) as u32, area.3.max(1) as u32)))
        .and_then(|()| w.set_ignore_cursor_events(true));
    if let Err(e) = placed {
        let _ = w.destroy();
        return Err(e.into());
    }
    Ok(())
}

/// Layout preview windows' labels start so (no capability: `capabilities/main.json` is for "main" only).
const PREVIEW: &str = "layout-preview";

/// Linux has no window layout to preview (the compositor owns placement): a no-op, so the UI's
/// call succeeds and the launch goes on.
#[cfg(not(windows))]
#[tauri::command]
pub async fn layout_preview(_app: AppHandle, _s: State<'_, App>, _account_ids: Vec<i64>, _running: String) -> Result<()> {
    Ok(())
}

/// "Stop preview": the layout preview goes before its time.
#[tauri::command]
pub fn layout_preview_stop(app: AppHandle) {
    close_layout_previews(&app);
}

/// Ends every layout preview on screen (a new one replaces it; RoALT's window closing ends it too).
pub fn close_layout_previews(app: &AppHandle) {
    for (label, w) in app.webview_windows() {
        if label.starts_with(PREVIEW) {
            let _ = w.destroy();
        }
    }
}

/// CPU, memory and IO of each running client (sampled at most every 2 s).
#[tauri::command]
pub async fn instances_metrics(app: AppHandle) -> Result<Vec<ClientMetrics>> {
    blocking(move || {
        let s = app.state::<App>();
        s.metrics.sample(&s.tracker.clients())
    }).await
}

/// When the bandwidth policy was last applied (the boot it was applied in, as fetched_at).
const QOS_KEY: &str = "qos:applied";

pub(crate) fn bandwidth(s: &App) -> Result<BandwidthStatus> {
    let kbps = s.db.settings()?.bandwidth_kbps;
    let now = now_ms();
    let boot = crate::instances::qos::boot_ms(now);
    let applied = s.db.cache_get(QOS_KEY, i64::MAX / 2, now)?.is_some_and(|(_, at)| crate::instances::qos::applied_this_boot(at, boot));
    #[cfg(windows)]
    let active = kbps > 0 && applied;
    // There is no Linux traffic policy yet: never claim a limit is enforced.
    #[cfg(not(windows))]
    let active = false;
    let _ = applied;
    Ok(BandwidthStatus { kbps, active })
}

/// The Roblox bandwidth limit and whether it is in force (no admin rights needed).
#[tauri::command]
pub fn bandwidth_status(s: State<'_, App>) -> Result<BandwidthStatus> {
    bandwidth(&s)
}

/// Sets the outbound limit for all Roblox clients (kbit/s; 0 removes it) as a Windows QoS
/// policy: Windows asks for admin rights first (`instances::qos::apply`). Saved only once set.
#[tauri::command]
pub async fn bandwidth_apply(s: State<'_, App>, kbps: u32) -> Result<BandwidthStatus> {
    crate::instances::qos::script(kbps)?;
    blocking(move || crate::instances::qos::apply(kbps)).await??;
    {
        let _change = s.db.settings_change();
        let mut v = s.db.settings()?;
        v.bandwidth_kbps = kbps;
        s.db.save_settings(&v)?;
    }
    s.db.cache_put(QOS_KEY, "", crate::instances::qos::boot_ms(now_ms()))?;
    bandwidth(&s)
}

/// How busy the whole PC is, for the verdict on Home.
#[tauri::command]
pub async fn system_usage(app: AppHandle) -> Result<Option<crate::model::SystemUsage>> {
    blocking(move || app.state::<App>().metrics.system()).await
}

/// Stops trying to bring this client back (the user's "Stop trying").
#[tauri::command]
pub fn instance_stop_relaunch(app: AppHandle, s: State<'_, App>, id: String) {
    s.tracker.end_relaunch(&id);
    // Its task ends now, so that it can't take over from one started by a later trouble.
    crate::instances::rejoin::retry_now();
    instances_changed(&app);
}

/// Relaunches waiting to try again do so now (the user's "Try now").
#[tauri::command]
pub fn instances_retry_relaunch() {
    crate::instances::rejoin::retry_now();
}

#[tauri::command]
pub fn instances_clear_ended(app: AppHandle, s: State<'_, App>) {
    s.tracker.clear_ended();
    instances_changed(&app);
}

/// The multi-instance status as it really is. Mocktail starts one Roblox client at a time, so its
/// backend never reports multi-client as active, whatever the setting says. Windows and Cordial
/// keep the lock's own answer.
fn multi_status(s: &App, v: &Settings) -> MultiInstanceStatus {
    let mut status = s.multi.status(v.multi_instance, v.protect_shared_login);
    if mocktail_selected() && !v.multi_instance {
        status.active = false;
        status.blocked_by.clear();
    }
    status
}

/// Off the main thread: while the lock is wanted but not held, the status may read the system's
/// handle table (`Multi::status`), and the UI asks every 5 s.
#[tauri::command]
pub async fn multi_instance_status(app: AppHandle) -> Result<MultiInstanceStatus> {
    blocking(move || {
        let s = app.state::<App>();
        let v = s.db.settings()?;
        retake_multi(&s, &v);
        Ok(multi_status(&s, &v))
    }).await?
}

/// Takes the lock when it is wanted but not held and Roblox has since closed (no-op otherwise).
pub(crate) fn retake_multi(s: &App, v: &Settings) {
    if v.multi_instance && !s.multi.is_active() && s.multi.enable().is_ok() {
        protect_running(s, v);
    }
}

/// Locks the shared login files at once when protection is on and clients already run
/// (RoALT's, or ones from before a restart).
fn protect_running(s: &App, v: &Settings) {
    if v.protect_shared_login && (s.multi.in_flight() || s.tracker.any_running() || s.multi.has_survivors()) {
        s.multi.lock_shared();
    }
}

/// Turns multi-instance on or off (saved). Turning it on while Roblox runs saves the setting
/// but reports why the lock isn't held yet; it is taken once Roblox has closed (status check,
/// next launch, next start) or after `roblox_close_all`.
#[tauri::command]
pub async fn multi_instance_set(app: AppHandle, enabled: bool) -> Result<MultiInstanceStatus> {
    // Off the main thread, as `multi_instance_status`.
    blocking(move || {
        let s = app.state::<App>();
        let change = s.db.settings_change();
        let mut v = s.db.settings()?;
        v.multi_instance = enabled;
        s.db.save_settings(&v)?;
        drop(change);
        if enabled {
            match s.multi.enable() {
                Ok(()) => protect_running(&s, &v),
                Err(e) => log::info!("multi-instance not active yet: {e}"),
            }
        } else {
            s.multi.disable();
        }
        Ok(multi_status(&s, &v))
    }).await?
}

/// Takes the multi-instance lock from the running Roblox (the UI asks first): closes Roblox's
/// singleton event inside each client (`Multi::take_over`); the clients keep running.
#[tauri::command]
pub async fn multi_instance_take_over(app: AppHandle) -> Result<MultiInstanceStatus> {
    blocking(move || {
        let s = app.state::<App>();
        let v = s.db.settings()?;
        if !v.multi_instance {
            return Err(AppError::Invalid(tr!("Turn on multi-instance first.")));
        }
        s.multi.take_over()?;
        protect_running(&s, &v);
        follow_outside(&app, &s);
        Ok(multi_status(&s, &v))
    }).await?
}

/// Follows each running client RoALT doesn't follow yet as the account its log names, window left
/// alone, so a taken-over client shows in Overview.
fn follow_outside(app: &AppHandle, s: &App) {
    use crate::instances::{LOG_SKEW_SECS, logwatch, process};
    let (Some(dir), Ok(accounts)) = (logwatch::logs_dir(), s.db.accounts()) else { return };
    let (mut followed, mut left) = (0, 0);
    for p in process::roblox_processes().into_iter().filter(|p| !s.tracker.follows(p.pid)) {
        let mut facts = logwatch::LogFacts::default();
        let read = logwatch::log_started_at(&dir, p.start_time, LOG_SKEW_SECS)
            .and_then(|log| logwatch::read_new(&log, 0, &mut facts, |_, _| {}).ok().map(|offset| (log, offset)));
        let account = facts.user_id.and_then(|u| accounts.iter().find(|a| a.user_id == u));
        let found = read.is_some();
        let (Some((log, offset)), Some(account), Some(place_id)) = (read, account, facts.place_id) else {
            let (joined, known) = (facts.user_id.is_some(), account.is_some());
            log::info!("take-over: client {} not followed (log found: {found}, in a game: {joined}, RoALT's account: {known})", p.pid);
            left += 1;
            continue;
        };
        let info = LaunchInfo {
            account_id: account.id,
            pid: p.pid,
            browser_tracker_id: facts.browser_tracker_id.unwrap_or(0),
            place_id: Some(place_id),
            started_at: i64::try_from(p.start_time * 1000).unwrap_or(0),
            via_bootstrapper: false,
            warning: None,
            fps_cap: None,
            pid_start: Some(p.start_time),
        };
        let inst = track(s, &info, LaunchTarget::Place { place_id, launch_data: None }, true);
        s.tracker.bind_log(&inst.id, log, offset, facts);
        s.multi.adopt(&[(p.pid, p.start_time)]);
        log::info!("take-over: client {} is account {}'s: followed as instance {}", p.pid, account.id, inst.id);
        followed += 1;
    }
    if followed > 0 {
        instances_changed(app);
    }
    if left > 0 {
        notice(app, false, tr!("A Roblox client that isn't in a game yet, or plays a Roblox user RoALT doesn't have, isn't shown in Overview."));
    }
}

/// Closes every Cordial client, tracked or not, then takes the multi-instance lock if it is
/// enabled. On Linux this deliberately leaves Sober and Wine clients alone.
#[tauri::command]
pub async fn roblox_close_all(s: State<'_, App>) -> Result<MultiInstanceStatus> {
    close_roblox(&s, false, false).await
}

/// Panic button (sidebar, tray "Kill all Roblox"): the running batch stops after its current
/// step, every client counts as closed by the user (no rejoin or restart follows, also for ones
/// waiting their turn), and every Roblox process is terminated at once, tracked or not.
/// The close question's "Keep running in the tray" (`app://close-ask`).
#[tauri::command]
pub fn window_to_tray(window: tauri::Window) {
    crate::hide_main(&window);
}

/// The close question's "Quit": as the tray's Quit.
#[tauri::command]
pub fn app_quit(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
pub async fn panic_kill(s: State<'_, App>) -> Result<MultiInstanceStatus> {
    kill_all(&s).await
}

/// Makes `key` the system-wide panic hotkey (Kill all) in place of `old`; "" for none. A key
/// Windows won't give (another app holds it) is an error, and `old` stays (if it was registered:
/// one refused at start is tried again when saved unchanged).
pub fn set_panic_hotkey(app: &AppHandle, old: &str, key: &str) -> Result<()> {
    swap_hotkey(app, old, key, "panic")?;
    *PANIC_KEY.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = if key.is_empty() { None } else { parse_hotkey(key).ok() };
    Ok(())
}

/// The panic key, as registered. The shortcut handler kills all only for this one: a key
/// pressed while it is being swapped in isn't this one, and does nothing.
static PANIC_KEY: std::sync::Mutex<Option<tauri_plugin_global_shortcut::Shortcut>> = std::sync::Mutex::new(None);

pub fn is_panic_key(key: &tauri_plugin_global_shortcut::Shortcut) -> bool {
    PANIC_KEY.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref() == Some(key)
}


fn swap_hotkey(app: &AppHandle, old: &str, key: &str, what: &str) -> Result<()> {
    use tauri_plugin_global_shortcut::GlobalShortcutExt;
    let parse = parse_hotkey;
    let new = if key.is_empty() { None } else { Some(parse(key)?) };
    let gs = app.global_shortcut();
    let old = parse(old).ok().filter(|&o| gs.is_registered(o));
    if new == old {
        return Ok(());
    }
    if let Some(o) = old
        && let Err(e) = gs.unregister(o)
    {
        log::warn!("could not unregister the old {what} hotkey: {e}");
    }
    if let Some(n) = new
        && let Err(e) = gs.register(n)
    {
        if let Some(o) = old
            && let Err(e) = gs.register(o)
        {
            log::warn!("could not re-register the old {what} hotkey; none is bound: {e}");
        }
        log::warn!("could not register the {what} hotkey {key}: {e}");
        return Err(AppError::Invalid(tr!("Windows wouldn't give RoALT {key}: another app probably uses it. Pick another.", key = key)));
    }
    Ok(())
}

/// A hotkey as Windows takes it (the panic hotkey).
pub(crate) fn parse_hotkey(k: &str) -> Result<tauri_plugin_global_shortcut::Shortcut> {
    k.parse().map_err(|_| AppError::Invalid(tr!("\"{k}\" isn't a key combination, like Ctrl+Shift+F12.", k = k.chars().take(32).collect::<String>())))
}

pub(crate) async fn kill_all(s: &App) -> Result<MultiInstanceStatus> {
    log::info!("kill all Roblox");
    s.bulk.kill();
    s.tracker.mark_all_closing();
    // First, so that the list of clients taken next has anything they just started.
    for r in close_all(s.tracker.bootstrappers(), true, Duration::ZERO).await? {
        if let Err(e) = r {
            log::warn!("could not close a bootstrapper: {e}");
        }
    }
    close_roblox(s, true, true).await
}

/// `keep_closing` (Kill all): a client that couldn't be closed still counts as closed by the
/// user, so it is never rejoined or restarted when it ends later.
async fn close_roblox(s: &App, force: bool, keep_closing: bool) -> Result<MultiInstanceStatus> {
    s.tracker.cancel_relaunches();
    for i in s.tracker.list() {
        s.tracker.mark_closing(&i.id);
    }
    let procs = crate::instances::process::cordial_processes();
    let targets = procs.iter().map(|p| (p.pid, Some(p.start_time))).collect();
    for (p, r) in procs.iter().zip(close_all(targets, force, grace(s)).await?) {
        if let Err(e) = r {
            log::warn!("could not close Roblox process {}: {e}", p.pid);
            // Still running: its exit stays unexpected (not after Kill all).
            for i in s.tracker.list().iter().filter(|i| !keep_closing && i.pid == p.pid && i.state != InstanceState::Exited) {
                s.tracker.clear_closing(&i.id);
            }
        }
    }
    for i in s.tracker.list() {
        remove_secrets_if_stopped(s, i.account_id);
    }
    let v = s.db.settings()?;
    retake_multi(s, &v);
    Ok(multi_status(s, &v))
}

/// Clients that crashed, were disconnected or hung, newest first (the last 200).
/// What a diagnostics report reads: the header now, the logs named for a background thread.
fn diagnostics_sources(s: &App) -> Result<crate::diagnostics::Sources> {
    let v = s.db.settings()?;
    // Whether the lock is held and, when it isn't, why: what multi-instance reports turn on.
    let multi = multi_status(s, &v);
    let lock = multi.message.map(|m| format!(" ({m}; blocked by {:?})", multi.blocked_by)).unwrap_or_default();
    #[cfg(windows)]
    let head = {
        use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;
        let reg = |value| crate::launcher::client::reg_string(HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", Some(value)).unwrap_or_default();
        format!(
            "RoALT {} on {} {} (build {})\nMulti-instance: {}, lock held: {}{}\nPerformance: {}\n",
            env!("CARGO_PKG_VERSION"),
            reg("ProductName"),
            reg("DisplayVersion"),
            reg("CurrentBuildNumber"),
            v.multi_instance,
            multi.active,
            lock,
            crate::performance::of(&v, None, now_ms()),
        )
    };
    #[cfg(not(windows))]
    let head = {
        let os = std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|t| t.lines().find_map(|l| l.strip_prefix("PRETTY_NAME=").map(|v| v.trim_matches('"').to_owned())))
            .unwrap_or_else(|| std::env::consts::OS.to_owned());
        format!(
            "RoALT {} on {} ({})\nMulti-instance: {}, lock held: {}{}\nPerformance: {}\n",
            env!("CARGO_PKG_VERSION"),
            os,
            std::env::consts::ARCH,
            v.multi_instance,
            multi.active,
            lock,
            crate::performance::of(&v, None, now_ms()),
        )
    };
    let mut names: Vec<String> = s
        .db
        .accounts()?
        .into_iter()
        .flat_map(|a| [a.username, a.display_name, a.alias, a.user_id.to_string()])
        .collect();
    names.extend(s.db.favourites()?.into_iter().map(|f| f.name));
    let mut roblox_logs = s.tracker.live_logs();
    for crash in s.db.crashes()?.iter().filter(|c| c.has_log).take(5) {
        if let Ok(path) = s.db.crash_log(crash.id) {
            let path = std::path::PathBuf::from(path);
            if !roblox_logs.contains(&path) {
                roblox_logs.push(path);
            }
        }
    }
    roblox_logs.truncate(5);
    Ok(crate::diagnostics::Sources { head, roalt_log: s.data_dir.join("logs").join("roalt.log"), roblox_logs, names })
}

#[tauri::command]
pub async fn diagnostics_text(s: State<'_, App>) -> Result<String> {
    let src = diagnostics_sources(&s)?;
    blocking(move || crate::diagnostics::report(&src, crate::diagnostics::FOR_CLIPBOARD)).await
}

#[tauri::command]
pub async fn diagnostics_save(s: State<'_, App>, path: String) -> Result<()> {
    let src = diagnostics_sources(&s)?;
    let text = blocking(move || crate::diagnostics::report(&src, crate::diagnostics::FOR_FILE)).await?;
    crate::vault::backup::write_atomic(Path::new(&path), text.as_bytes())
}

#[tauri::command(async)]
pub fn server_history(s: State<'_, App>, account_id: i64) -> Result<Vec<crate::model::ServerVisit>> {
    s.db.servers(account_id)
}

#[tauri::command(async)]
pub fn server_history_recent(s: State<'_, App>) -> Result<Vec<crate::model::PlayedServer>> {
    s.db.recent_servers()
}

#[tauri::command(async)]
pub fn crashes_list(s: State<'_, App>) -> Result<Vec<Crash>> {
    s.db.crashes()
}

/// Opens a recorded crash's client log in the default app (opener plugin, Rust side). The path
/// comes from the database, never from the UI.
#[tauri::command]
pub fn crash_open_log(app: AppHandle, s: State<'_, App>, id: i64) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    let path = s.db.crash_log(id)?;
    if !Path::new(&path).is_file() {
        return Err(AppError::Invalid(tr!("That client log is gone; Roblox deletes old logs.")));
    }
    app.opener().open_path(path, None::<&str>).map_err(|e| AppError::Internal(e.to_string()))
}

/// The accounts a favourite launches: its own, then the members of its groups as they are now
/// (`FavouriteOptions::group_ids`), each once.
pub(crate) fn favourite_accounts(s: &App, f: &Favourite) -> Result<Vec<i64>> {
    let mut ids = f.account_ids.clone();
    if !f.options.group_ids.is_empty() {
        let members = s.db.accounts()?.into_iter().filter(|a| a.group_ids.iter().any(|g| f.options.group_ids.contains(g)));
        for a in members {
            if !ids.contains(&a.id) {
                ids.push(a.id);
            }
        }
    }
    Ok(ids)
}
const FAVOURITE_AAD: &[u8] = b"roalt.favourite";

pub(crate) fn favourite_of(s: &App, row: crate::storage::FavouriteRow) -> Result<Favourite> {
    let json = s.vault.decrypt(FAVOURITE_AAD, &row.target)?;
    let target = serde_json::from_slice(&json).map_err(|e| {
        log::warn!("favourite {} target unreadable: {:?} at {}:{}", row.id, e.classify(), e.line(), e.column());
        AppError::Crypto
    })?;
    Ok(Favourite {
        id: row.id,
        name: row.name,
        target,
        account_ids: row.account_ids,
        in_tray: row.in_tray,
        sort_order: row.sort_order,
        options: row.options,
    })
}

#[tauri::command(async)]
pub fn favourites_list(s: State<'_, App>) -> Result<Vec<Favourite>> {
    s.db.favourites()?.into_iter().map(|r| favourite_of(&s, r)).collect()
}

#[tauri::command]
pub fn favourite_save(app: AppHandle, s: State<'_, App>, input: FavouriteInput) -> Result<Favourite> {
    not_too_many(&input.account_ids)?;
    let json = zeroize::Zeroizing::new(serde_json::to_vec(&input.target).map_err(|e| AppError::Internal(e.to_string()))?);
    let blob = s.vault.encrypt(FAVOURITE_AAD, &json)?;
    if let Some(o) = &input.options {
        if let Some(fps) = o.fps_cap {
            check_fps(fps)?;
        }
        not_too_many(&o.group_ids)?;
    }
    if let Some(o) = &input.options {
        // Its global key is tried before anything is saved: one that can't be had is the error,
        // and nothing changed. A key it already had is not judged again.
        let had = match input.id {
            Some(id) => s.db.favourites()?.into_iter().find(|f| f.id == id).map(|f| f.options.hotkey).unwrap_or_default(),
            None => String::new(),
        };
        let key = o.hotkey.trim();
        let changed: &[&str] = if key.is_empty() || key == had.trim() { &[] } else { &[key] };
        crate::hotkeys::try_change(&app, None, Some((input.id, key)), changed)?;
    }
    let saved = s.db.save_favourite(input.id, &input.name, &blob, &input.account_ids, input.in_tray).and_then(|id| {
        if let Some(o) = &input.options {
            s.db.set_favourite_options(id, o)?;
        }
        Ok(id)
    });
    // The keys as stored now: with a new favourite's id, or (the save failed) as they were.
    crate::hotkeys::sync(&app);
    let id = saved?;
    crate::tray::refresh(&app);
    let row = s.db.favourites()?.into_iter().find(|f| f.id == id).ok_or(AppError::NotFound)?;
    favourite_of(&s, row)
}

#[tauri::command]
pub fn favourite_remove(app: AppHandle, s: State<'_, App>, id: i64) -> Result<()> {
    unlocked(&s)?;
    s.db.remove_favourite(id)?;
    // Its key, if it had one, is free again.
    crate::hotkeys::sync(&app);
    crate::tray::refresh(&app);
    Ok(())
}

#[tauri::command]
pub fn favourites_reorder(app: AppHandle, s: State<'_, App>, ids: Vec<i64>) -> Result<()> {
    unlocked(&s)?;
    not_too_many(&ids)?;
    s.db.reorder_favourites(&ids)?;
    crate::tray::refresh(&app);
    Ok(())
}

fn presence_kind(s: Option<&str>) -> PresenceKind {
    match s {
        Some("InGame") => PresenceKind::InGame,
        Some("Online") => PresenceKind::Online,
        Some("InStudio") => PresenceKind::Studio,
        Some("Offline") => PresenceKind::Offline,
        None => PresenceKind::Unknown,
        Some(other) => {
            log::debug!("unrecognised friend presence {other:?}");
            PresenceKind::Unknown
        }
    }
}

/// Who `account_id` can join: its friends online on Roblox, then the other stored accounts
/// with a valid cookie (each asked about itself). Asked when the picker opens, not polled.
#[tauri::command]
pub async fn join_candidates(app: AppHandle, s: State<'_, App>, account_id: i64) -> Result<Vec<JoinCandidate>> {
    let me = s.db.account(account_id)?;
    let cookie = stored_cookie(&s, account_id)?;
    // Friends are optional: a transient failure still offers the other stored accounts.
    let friends = match s.api.friends_online(account_id, &cookie, me.user_id).await {
        Ok(f) => f,
        Err(e) if record_cookie_error(&app, &s, account_id, &e)? => return Err(e),
        Err(e) => {
            log::info!("friends online for account {account_id} failed: {e}");
            Vec::new()
        }
    };
    let mut out: Vec<JoinCandidate> = friends
        .into_iter()
        .map(|f| {
            let p = f.presence.unwrap_or_default();
            JoinCandidate {
                user_id: f.id,
                username: f.name,
                display_name: f.display_name,
                account_id: None,
                presence: Presence {
                    kind: presence_kind(p.user_presence_type.as_deref()),
                    last_location: p.last_location,
                    place_id: p.place_id,
                    root_place_id: p.root_place_id,
                    universe_id: p.universe_id,
                    game_id: p.game_instance_id,
                },
            }
        })
        .collect();
    for a in s.db.accounts()?.into_iter().filter(|a| a.id != account_id && a.status == AccountStatus::Valid) {
        let c = match stored_cookie(&s, a.id) {
            Ok(c) => c,
            Err(e) => {
                log::debug!("join candidates: cookie of account {} unusable: {e}", a.id);
                continue;
            }
        };
        let Ok(rows) = s.api.presence(a.id, &c, &[a.user_id]).await else { continue };
        let Some(p) = rows.into_iter().find(|p| p.user_id == a.user_id) else { continue };
        out.retain(|f| f.user_id != a.user_id);
        out.push(JoinCandidate {
            user_id: a.user_id,
            username: a.username,
            display_name: a.display_name,
            account_id: Some(a.id),
            presence: presence_of(p),
        });
    }
    Ok(out)
}

/// Every account's time in each place, the most played first (Browse's Playtime).
#[tauri::command(async)]
pub fn playtime_list(s: State<'_, App>) -> Result<Vec<PlaytimeRow>> {
    s.db.playtime()
}
/// Every account's time in games per local day (the last 90 days).
#[tauri::command(async)]
pub fn playtime_days(s: State<'_, App>) -> Result<Vec<PlaytimeDay>> {
    s.db.playtime_days()
}

/// One CSV line (RFC 4180). A cell a spreadsheet would run as a formula (it starts with = + - @)
/// gets a leading ': a crash's reason comes from Roblox's log.
fn csv_line<S: AsRef<str>>(cells: &[S]) -> String {
    let cell = |c: &S| {
        let c = c.as_ref();
        let c = if c.starts_with(['=', '+', '-', '@']) { format!("'{c}") } else { c.to_owned() };
        if c.contains([',', '"', '\n', '\r']) { format!("\"{}\"", c.replace('"', "\"\"")) } else { c }
    };
    cells.iter().map(cell).collect::<Vec<_>>().join(",") + "\r\n"
}

/// A CSV file at `path` (the user picked it): `head`, then `rows`. With a BOM, for Excel.
fn write_csv<const N: usize>(path: &str, head: [&str; N], rows: &[[String; N]]) -> Result<()> {
    let text = rows.iter().fold(format!("\u{feff}{}", csv_line(&head)), |text, row| text + &csv_line(row));
    crate::vault::backup::write_atomic(Path::new(path), text.as_bytes())
}

/// Writes the crash history as a CSV file. Its headings stay English, as a script would expect.
#[tauri::command]
pub async fn crashes_export(s: State<'_, App>, path: String) -> Result<()> {
    write_csv(&path, ["When", "Account", "Kind", "Seconds", "Place ID", "Reason", "Verdict"], &s.db.crash_cells()?)
}

/// Writes the playtime by day as a CSV file.
#[tauri::command]
pub async fn playtime_export(s: State<'_, App>, path: String) -> Result<()> {
    write_csv(&path, ["Day", "Account", "Minutes"], &s.db.playtime_day_cells()?)
}

/// Locks the vault; Browse closes first (it holds a live roblox.com session). Its close stores the
/// cookie Roblox may have rotated there, which needs the vault unlocked. Always in the background:
/// Browse may be opening at that very moment, and its close can't run on the main thread or a
/// runtime worker.
pub fn lock_vault(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || lock_vault_now(&app));
}

/// `lock_vault`, blocking until locked (never on the main thread or a runtime worker). The vault
/// locks before Browse can open again: a `browse_open` waiting meanwhile then finds it locked.
/// Clears any copied cookie and unencrypted rotated cookies still held in memory.
pub(crate) fn lock_vault_now(app: &AppHandle) {
    // Linux: session windows hold a live roblox.com session. Close them and let their close store
    // the cookie Roblox may have rotated (needs the vault unlocked) before locking.
    #[cfg(not(windows))]
    {
        for (label, w) in app.webview_windows() {
            if label.starts_with("session-")
                && let Err(e) = w.close()
            {
                log::warn!("could not close session window {label} on lock: {e}");
            }
        }
        crate::webview::wait_sessions_closed(app);
    }
    clear_copied_cookie(app);
    crate::browse::close_then(app, || {
        let s = app.state::<App>();
        s.rotated_later.lock().unwrap_or_else(|e| e.into_inner()).clear();
        s.vault.lock();
    });
    let _ = app.emit("vault://locked", ());
    crate::tray::refresh(app);
}

// ---------- discord ----------

/// Whether a webhook is set, and its id's last digits (never the URL). `Locked` while locked.
#[tauri::command]
pub fn discord_get(s: State<'_, App>) -> Result<DiscordStatus> {
    crate::discord::status(&s)
}

/// Stores a Discord webhook URL in the vault (only discord.com / discordapp.com webhooks);
/// None or "" removes it.
#[tauri::command]
pub fn discord_set(s: State<'_, App>, url: Option<String>) -> Result<DiscordStatus> {
    crate::discord::set(&s, url.as_deref())
}

/// Posts a test message to the webhook.
#[tauri::command]
pub async fn discord_test(s: State<'_, App>) -> Result<()> {
    crate::discord::test(&s).await
}

// ---------- groups ----------

#[tauri::command(async)]
pub fn groups_list(s: State<'_, App>) -> Result<Vec<Group>> {
    s.db.groups()
}

/// A group's defaults, by the rules of an account's own overrides and a client's limits.
fn check_group_defaults(d: &GroupDefaults) -> Result<()> {
    if let Some(f) = d.fps_cap {
        check_fps(f)?;
    }
    if let Some(a) = &d.launch_args {
        crate::launcher::launch_args(a)?;
    }
    if let Some(p) = &d.client {
        check_limits(&p.limits)?;
    }
    Ok(())
}
#[tauri::command]
pub fn group_save(app: AppHandle, s: State<'_, App>, input: GroupInput) -> Result<Group> {
    unlocked(&s)?;
    if let Some(d) = &input.defaults {
        check_group_defaults(d)?;
    }
    let g = s.db.save_group(&input)?;
    accounts_changed(&app);
    Ok(g)
}

#[tauri::command]
pub fn group_remove(app: AppHandle, s: State<'_, App>, id: i64) -> Result<()> {
    unlocked(&s)?;
    s.db.remove_group(id)?;
    accounts_changed(&app);
    Ok(())
}

#[tauri::command]
pub fn group_clear_overrides(app: AppHandle, s: State<'_, App>, id: i64) -> Result<u32> {
    unlocked(&s)?;
    let n = s.db.clear_group_overrides(id)?;
    accounts_changed(&app);
    Ok(n)
}

// ---------- backup ----------

const MIN_BACKUP_PASSWORD: usize = 12;

#[tauri::command]
pub async fn backup_export(
    app: AppHandle,
    s: State<'_, App>,
    path: String,
    password: String,
    current_password: Option<String>,
) -> Result<u32> {
    if password.chars().count() < MIN_BACKUP_PASSWORD {
        return Err(AppError::Invalid(tr!("Use at least {min} characters for the backup password.", min = MIN_BACKUP_PASSWORD)));
    }
    // Exporting hands out every cookie, so it needs the same check as reveal.
    reauth(&app, &s, current_password, &tr!("Confirm it's you to export your Roblox accounts.")).await?;
    let groups = s.db.groups()?;
    let name_of = |gid: i64| groups.iter().find(|g| g.id == gid).map(|g| g.name.clone());
    let mut accounts = Vec::new();
    // By user id: ids don't survive a move to another PC.
    let mut user_of = std::collections::HashMap::new();
    for a in s.db.accounts()? {
        user_of.insert(a.id, a.user_id);
        let cookie = stored_cookie(&s, a.id)?;
        accounts.push(BackupAccount {
            user_id: a.user_id,
            username: a.username,
            display_name: a.display_name,
            alias: a.alias,
            notes: a.notes,
            color: a.color,
            favourite: a.favourite,
            sort_order: a.sort_order,
            groups: a.group_ids.iter().filter_map(|g| name_of(*g)).collect(),
            cookie: cookie.expose().to_owned().into(),
            allow_disconnect: Some(a.allow_disconnect),
            launch_args: a.launch_args,
            discord_muted: a.discord_muted,
            fps_cap: a.fps_cap,
            full_quality: a.full_quality,
            main: a.main,
            custom_flags: Some(a.custom_flags),
            // Empty: the account has none, and an import removes the one it has there.
            profile: Some(a.profile.unwrap_or_default()),
        });
    }
    let mut favourites = Vec::new();
    for row in s.db.favourites()? {
        let f = favourite_of(&s, row)?;
        // Not its key (another PC may not have it free), and its groups by name: their ids are this PC's.
        let options = crate::model::FavouriteOptions { same_server: f.options.same_server, fps_cap: f.options.fps_cap, ..Default::default() };
        let in_groups = groups.iter().filter(|g| f.options.group_ids.contains(&g.id)).map(|g| g.name.clone()).collect();
        favourites.push(backup::BackupFavourite {
            name: f.name,
            target: f.target,
            user_ids: f.account_ids.iter().filter_map(|id| user_of.get(id).copied()).collect(),
            in_tray: f.in_tray,
            options,
            groups: in_groups,
        });
    }
    let count = accounts.len() as u32;
    let payload = BackupPayload {
        version: 1,
        exported_at: now_ms(),
        accounts,
        groups: groups
            .iter()
            .map(|g| BackupGroup {
                name: g.name.clone(),
                color: g.color.clone(),
                sort_order: g.sort_order,
                defaults: (g.defaults != GroupDefaults::default()).then(|| g.defaults.clone()),
            })
            .collect(),
        favourites,
        settings: Some(serde_json::to_value(s.db.settings()?).map_err(|e| AppError::Internal(e.to_string()))?),
        discord_webhook: crate::discord::stored_url(&s),
    };
    blocking(move || backup::write(Path::new(&path), &password, &payload)).await??;
    Ok(count)
}

/// What a backup entry must be before any of the file is applied.
fn check_backup(p: &backup::BackupPayload) -> Result<()> {
    let bad = |t: &str| t.chars().count() > 64;
    let wrong = p.version != 1
        || p.accounts.iter().any(|a| a.user_id <= 0 || bad(&a.username) || bad(&a.display_name))
        || p.groups.iter().any(|g| bad(&g.name))
        || p.favourites.iter().any(|f| f.name.is_empty() || bad(&f.name) || f.groups.len() > 200 || f.groups.iter().any(|g| bad(g)));
    if wrong {
        return Err(AppError::Invalid(tr!("This isn't a RoALT backup.")));
    }
    for a in &p.accounts {
        if let Some(args) = &a.launch_args {
            crate::launcher::launch_args(args)?;
        }
        if let Some(fps) = a.fps_cap {
            check_fps(fps)?;
        }
        if let Some(f) = &a.custom_flags {
            crate::launcher::flags::check(f)?;
        }
    }
    for d in p.groups.iter().filter_map(|g| g.defaults.as_ref()) {
        check_group_defaults(d)?;
    }
    p.accounts.iter().try_for_each(|a| crate::storage::check_meta(&AccountMeta { alias: a.alias.clone(), notes: a.notes.clone(), color: a.color.clone(), favourite: a.favourite }))
}

/// The backup's settings and Discord webhook onto this PC, through the checks a change in the UI
/// gets (`set_settings`, `discord::set`): the file may be anyone's. False when they couldn't be
/// applied.
fn restore_settings(app: &AppHandle, s: &App, payload: &BackupPayload) -> bool {
    let Some(mut json) = payload.settings.clone() else { return false };
    let apply = |json: serde_json::Value| serde_json::from_value::<SettingsPatch>(json).map_err(|e| AppError::Internal(e.to_string())).and_then(|p| set_settings_on_main(app, p));
    // A global key another program holds on this PC must not cost every other setting.
    let done = apply(json.clone()).or_else(|e| {
        log::info!("backup settings refused ({e}); trying without its global keys");
        if let Some(o) = json.as_object_mut() {
            for key in ["panicHotkey", "toggleHotkey", "tileHotkey", "minimizeHotkey"] {
                o.remove(key);
            }
        }
        apply(json)
    });
    if let Err(e) = &done {
        log::warn!("backup settings not restored: {e}");
    }
    if let Some(url) = &payload.discord_webhook
        && let Err(e) = crate::discord::set(s, Some(url.as_str()))
    {
        log::warn!("backup webhook not restored: {e}");
    }
    done.is_ok()
}

/// `set_settings` on the main thread, where `settings_set` and the tray run it. Off it, it would hold
/// the settings lock while waiting for the main thread to register a global key, and a change made
/// there meanwhile would wait for that lock: both stuck.
fn set_settings_on_main(app: &AppHandle, patch: SettingsPatch) -> Result<Settings> {
    let (tx, rx) = std::sync::mpsc::channel();
    let handle = app.clone();
    app.run_on_main_thread(move || _ = tx.send(set_settings(&handle, &handle.state::<App>(), patch)))
        .map_err(|e| AppError::Internal(e.to_string()))?;
    rx.recv().map_err(|e| AppError::Internal(e.to_string()))?
}

/// `settings`: also restore the backup's settings and Discord webhook over this PC's.
#[tauri::command]
pub async fn backup_import(app: AppHandle, s: State<'_, App>, path: String, password: String, on_conflict: ConflictPolicy, settings: bool) -> Result<ImportReport> {
    unlocked(&s)?;
    let payload = blocking(move || backup::read(Path::new(&path), &password)).await??;
    // The file comes from anyone, and its password with it: all of it is checked before anything
    // is written, so a bad entry leaves nothing half done.
    check_backup(&payload)?;
    let here = s.db.groups()?;
    for g in &payload.groups {
        let id = s.db.group_by_name_or_create(&g.name, g.color.as_deref())?;
        // Its defaults come along, unless the group here already has some of its own.
        if let Some(d) = &g.defaults
            && here.iter().find(|h| h.id == id).is_none_or(|h| h.defaults == GroupDefaults::default())
        {
            s.db.set_group_defaults(id, d)?;
        }
    }
    let mut report = ImportReport::default();
    for a in &payload.accounts {
        let existing = s.db.account_id_for_user(a.user_id)?;
        if existing.is_some() && on_conflict == ConflictPolicy::Skip {
            report.skipped += 1;
            continue;
        }
        let Ok(mut cookie) = normalize_cookie(&a.cookie) else {
            report.failed += 1;
            continue;
        };
        // Over a stored cookie only if this one really signs in as that user: else it would
        // destroy the only copy. Anything else (other user, dead cookie, no network) keeps the row.
        if existing.is_some() {
            match s.api.authenticated_user(0, &cookie).await {
                Ok((u, rotated)) if u.id == a.user_id => cookie = rotated.unwrap_or(cookie),
                _ => {
                    report.failed += 1;
                    continue;
                }
            }
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        let blob = s.vault.encrypt(&cookie_aad(a.user_id), cookie.expose().as_bytes())?;
        let id = s.db.upsert_account(&NewAccount {
            user_id: a.user_id,
            username: &a.username,
            display_name: &a.display_name,
            cookie: &blob,
            status: AccountStatus::Unknown,
            checked_at: None,
        })?;
        #[cfg(not(windows))]
        crate::launcher::client::ensure_profile(a.user_id);
        s.db.set_meta(id, &AccountMeta { alias: a.alias.clone(), notes: a.notes.clone(), color: a.color.clone(), favourite: a.favourite })?;
        let gids = a.groups.iter().map(|n| s.db.group_by_name_or_create(n, None)).collect::<Result<Vec<_>>>()?;
        s.db.set_account_groups(id, &gids)?;
        // A backup from before 0.7 never held these: what the account has now stays.
        if a.allow_disconnect.is_some() {
            s.db.update_account(id, &AccountPatch {
                allow_disconnect: a.allow_disconnect,
                launch_args: Some(a.launch_args.clone().unwrap_or_default()),
                discord_muted: Some(a.discord_muted),
                fps_cap: Some(a.fps_cap.unwrap_or(0)),
                full_quality: Some(a.full_quality),
                custom_flags: a.custom_flags.clone(),
                // One this PC's Settings would refuse is left out: what the account has now stays.
                profile: a.profile.clone().filter(|p| s.db.settings().is_ok_and(|v| crate::performance::check(p, &v).is_ok())).map(crate::performance::per_client),
                ..Default::default()
            })?;
        }
        // Only ever marks: a backup's non-MAIN row doesn't unmark this PC's MAIN.
        if a.main {
            s.db.update_account(id, &AccountPatch { main: Some(true), ..Default::default() })?;
        }
        if existing.is_some() { report.updated += 1 } else { report.added += 1 }
    }
    // Favourites this PC doesn't have yet, by name.
    let have: Vec<String> = s.db.favourites()?.into_iter().map(|f| f.name).collect();
    for f in payload.favourites.iter().filter(|f| !have.contains(&f.name)) {
        let ids: Vec<i64> = f.user_ids.iter().filter_map(|u| s.db.account_id_for_user(*u).ok().flatten()).collect();
        let json = zeroize::Zeroizing::new(serde_json::to_vec(&f.target).map_err(|e| AppError::Internal(e.to_string()))?);
        let blob = s.vault.encrypt(FAVOURITE_AAD, &json)?;
        let id = s.db.save_favourite(None, &f.name, &blob, &ids, f.in_tray)?;
        // As saved, when it passes the checks a save in the UI gets; else the defaults.
        let o = crate::model::FavouriteOptions {
            same_server: f.options.same_server,
            fps_cap: f.options.fps_cap.filter(|&fps| check_fps(fps).is_ok()),
            // Its groups as they are called here (the backup's own were made above).
            group_ids: f.groups.iter().filter_map(|n| s.db.group_by_name_or_create(n, None).ok()).collect(),
            ..Default::default()
        };
        s.db.set_favourite_options(id, &o)?;
    }
    if settings && payload.settings.is_some() {
        report.settings = Some(restore_settings(&app, &s, &payload));
    }
    crate::tray::refresh(&app);
    accounts_changed(&app);
    Ok(report)
}

// ---------- settings ----------

#[tauri::command(async)]
pub fn settings_get(s: State<'_, App>) -> Result<Settings> {
    s.db.settings()
}

/// The current performance settings as a profile (JSON), for a file or a share code.
#[tauri::command]
pub fn performance_profile(s: State<'_, App>, preview_fps: Option<u32>) -> Result<String> {
    Ok(crate::performance::of(&s.db.settings()?, preview_fps, now_ms()))
}

#[tauri::command]
pub async fn performance_export(s: State<'_, App>, path: String, preview_fps: Option<u32>) -> Result<()> {
    let text = crate::performance::of(&s.db.settings()?, preview_fps, now_ms());
    crate::vault::backup::write_atomic(Path::new(&path), text.as_bytes())
}

/// The profile in the file at `path`, checked; nothing is saved (the UI shows it, then saves it
/// through `settings_set`).
#[tauri::command]
pub async fn performance_read(s: State<'_, App>, path: String) -> Result<crate::performance::Profile> {
    let refused = || AppError::Invalid(tr!("This isn't a RoALT performance profile."));
    if std::fs::metadata(&path)?.len() > crate::performance::MAX_BYTES as u64 {
        return Err(refused());
    }
    let text = std::fs::read_to_string(&path).map_err(|_| refused())?;
    crate::performance::parse(&text, &s.db.settings()?)
}

/// As `performance_read`, for a profile's text (a pasted share code, decoded by the UI).
#[tauri::command]
pub fn performance_parse(s: State<'_, App>, text: String) -> Result<crate::performance::Profile> {
    crate::performance::parse(&text, &s.db.settings()?)
}

/// One benchmark at a time: each holds a blocking thread for about 22 s.
static BENCHING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Rounds after the warm-up, and the time between them: over the metrics' 1.9 s cache.
const BENCH_ROUNDS: usize = 8;
const BENCH_GAP: Duration = Duration::from_millis(2500);

/// Watches the clients in a game for about 22 s (nothing is launched or saved) and says what this
/// PC could turn on for `target` clients like them (default: as many as are in a game).
#[tauri::command]
pub async fn performance_benchmark(app: AppHandle, target: Option<u32>) -> Result<crate::performance::Benchmark> {
    use std::sync::atomic::Ordering;
    if BENCHING.swap(true, Ordering::SeqCst) {
        return Err(AppError::Invalid(tr!("A measurement is already running.")));
    }
    let r = blocking(move || benchmark(&app.state::<App>(), target)).await;
    BENCHING.store(false, Ordering::SeqCst);
    r?
}

fn benchmark(s: &App, target: Option<u32>) -> Result<crate::performance::Benchmark> {
    use crate::performance::{Bench, Benchmark, Caps, LONG_RUN_MS, Round, measured, suggest};
    let no_game = || AppError::Invalid(tr!("Start a client and let it reach a game first."));
    let in_game = || s.tracker.list().into_iter().filter(|i| i.state == InstanceState::InGame).collect::<Vec<_>>();
    if in_game().is_empty() {
        return Err(no_game());
    }
    let full_quality = s.db.full_quality_accounts()?;
    let mut rounds = Vec::new();
    // Round 0 warms up: the first CPU and GPU readings are None, and the GPU counters open slowly once.
    for i in 0..=BENCH_ROUNDS {
        if i > 0 {
            std::thread::sleep(BENCH_GAP);
        }
        let games = in_game();
        let free = s.tracker.free(&full_quality, now_ms());
        let front = crate::instances::process::foreground_pid();
        let all = s.metrics.sample(&s.tracker.clients());
        let system = s.metrics.system();
        let Some(system) = system.filter(|_| i > 0) else { continue };
        rounds.push(Round {
            games: all
                .iter()
                .filter(|m| games.iter().any(|g| g.id == m.instance_id))
                .map(|m| (m.clone(), front != Some(m.pid) && !free.contains(&m.instance_id)))
                .collect(),
            clients_mb: all.iter().map(|m| m.memory_mb).sum(),
            clients_cpu: all.iter().filter_map(|m| m.cpu_pct).sum(),
            system,
        });
    }
    rounds.retain(|r| !r.games.is_empty());
    let last = rounds.last().ok_or_else(no_game)?;
    let list = in_game();
    let cap_of = |id: &str| list.iter().find(|i| i.id == id).and_then(|i| i.applied_limits.cpu_cap);
    // No refusal only means none was tried, unless some client holds a cap. Only Windows can
    // refuse a cap; Linux applies what it is given.
    #[cfg(windows)]
    let caps = if !s.limiter.uncapped().is_empty() {
        Caps::Refused
    } else if list.iter().any(|i| i.applied_limits.cpu_cap.is_some()) {
        Caps::Works
    } else {
        Caps::Unknown
    };
    #[cfg(not(windows))]
    let caps = if list.iter().any(|i| i.applied_limits.cpu_cap.is_some()) { Caps::Works } else { Caps::Unknown };
    let week = now_ms() - 7 * 24 * 3_600_000;
    let crashes: Vec<Crash> = s.db.crashes()?.into_iter().filter(|c| c.at >= week).collect();
    let bench = Bench {
        under_limits: last.games.iter().any(|(m, bg)| *bg && cap_of(&m.instance_id).is_some()),
        caps,
        mem_restarts: crashes.iter().filter(|c| c.kind == CrashKind::Mem).count() as u32,
        long_crashes: crashes.iter().filter(|c| matches!(c.kind, CrashKind::Crash | CrashKind::Hang) && c.uptime_ms >= LONG_RUN_MS).count() as u32,
        ..measured(&rounds, cores())
    };
    // `suggest` keeps only suggestions that pass `performance::check`.
    let advice = suggest(&bench, &s.db.settings()?, target);
    Ok(Benchmark { bench, advice })
}

/// This PC's processor, memory and graphics card, and about how many clients each preset fits, by
/// specs alone (nothing is launched). Running clients are left out of what the rest of the PC uses.
#[tauri::command]
pub async fn performance_specs(app: AppHandle) -> Result<crate::performance::Specs> {
    blocking(move || {
        let s = app.state::<App>();
        // Windows' registry; Linux reads the same name from `/proc/cpuinfo`.
        #[cfg(windows)]
        let cpu = {
            use windows::Win32::System::Registry::HKEY_LOCAL_MACHINE;
            crate::launcher::client::reg_string(HKEY_LOCAL_MACHINE, r"HARDWARE\DESCRIPTION\System\CentralProcessor\0", Some("ProcessorNameString")).unwrap_or_default()
        };
        #[cfg(not(windows))]
        let cpu = std::fs::read_to_string("/proc/cpuinfo")
            .ok()
            .and_then(|t| {
                t.lines().find_map(|l| {
                    let (key, value) = l.split_once(':')?;
                    key.trim().eq_ignore_ascii_case("model name").then(|| value.trim().to_owned())
                })
            })
            .unwrap_or_default();
        let threads = cores();
        let all = s.metrics.sample(&s.tracker.clients());
        let system = s.metrics.system().ok_or_else(|| AppError::Internal("memory status unreadable".into()))?;
        let other_mb = (system.mem_used_mb - all.iter().map(|m| m.memory_mb).sum::<f64>()).max(0.0);
        let other_cores = system.cpu_pct.map_or(0.0, |c| (c - all.iter().filter_map(|m| m.cpu_pct).sum::<f64>()).max(0.0)) * f64::from(threads) / 100.0;
        #[cfg(windows)]
        let (gpu, vram_mb) = crate::instances::gpu::adapter().map_or((None, 0.0), |(name, mb)| (Some(name), mb));
        #[cfg(not(windows))]
        let (gpu, vram_mb): (Option<String>, f64) = (None, 0.0);
        Ok(crate::performance::Specs {
            cpu: cpu.trim().to_owned(),
            threads,
            ram_mb: system.mem_total_mb,
            integrated: gpu.is_some() && crate::performance::integrated(vram_mb),
            gpu,
            vram_mb,
            rooms: crate::performance::spec_rooms(system.mem_total_mb, other_mb, threads, other_cores),
        })
    })
    .await?
}

/// The performance part of a settings change, checked and applied to `v` (no side effects): also
/// what a performance profile is checked with (`performance::parse`).
pub(crate) fn apply_performance(v: &mut Settings, patch: &SettingsPatch) -> Result<()> {
    if let Some(f) = patch.fps_cap {
        check_fps(f)?;
        v.fps_cap = f;
    }
    if let Some(l) = patch.low_graphics { v.low_graphics = l }
    if let Some(l) = patch.lighter_rendering { v.lighter_rendering = l }
    if let Some(r) = patch.render_flags { v.render_flags = r }
    if let Some(h) = patch.headless { v.headless = h }
    if let Some(k) = patch.keep_running_on_close { v.keep_running_on_close = k }
    if let Some(m) = patch.minimize_on_launch { v.minimize_on_launch = m }
    if let Some(t) = patch.tiny_on_launch { v.tiny_on_launch = t }
    if patch.micro_width.is_some() || patch.micro_height.is_some() {
        let size = (patch.micro_width.unwrap_or(v.micro_width), patch.micro_height.unwrap_or(v.micro_height));
        check_window_size(size)?;
        (v.micro_width, v.micro_height) = size;
    }
    if patch.headless_width.is_some() || patch.headless_height.is_some() {
        let size = (patch.headless_width.unwrap_or(v.headless_width), patch.headless_height.unwrap_or(v.headless_height));
        if !(320..=7680).contains(&size.0) || !(240..=4320).contains(&size.1) {
            return Err(AppError::Invalid(tr!("A headless resolution can be 320 to 7680 pixels wide and 240 to 4320 high.")));
        }
        (v.headless_width, v.headless_height) = size;
    }
    if let Some(m) = patch.mem_restart_mb {
        if m != 0 && !(512..=65_536).contains(&m) {
            return Err(AppError::Invalid(tr!("The memory limit must be 0 (off) or 512 to 65 536 MB.")));
        }
        v.mem_restart_mb = m;
    }
    if let Some(t) = patch.mem_throttle {
        if t.mb != 0 && !(512..=65_536).contains(&t.mb) {
            return Err(AppError::Invalid(tr!("The memory throttle must be 0 (off) or 512 to 65 536 MB.")));
        }
        if !(1..=100).contains(&t.cpu_cap) {
            return Err(AppError::Invalid(tr!("The throttle's CPU limit must be 1 to 100 %.")));
        }
        v.mem_throttle = t;
    }
    if let Some(m) = patch.trim_every_min {
        if m > 24 * 60 {
            return Err(AppError::Invalid(tr!("Trimming can be at most once a day apart.")));
        }
        v.trim_every_min = m;
    }
    if let Some(a) = patch.auto_spread { v.auto_spread = a }
    if let Some(f) = patch.fair_cap { v.fair_cap = f }
    if let Some(a) = patch.adaptive {
        check_limits(&a.background)?;
        v.adaptive = a;
    }
    Ok(())
}

/// The corner roundness choices, % of the design's radii.
const CORNERS: [u8; 5] = [0, 50, 100, 150, 200];
/// At most this many colours in a look (the UI has about 20 ids).
const LOOK_MAX: usize = 64;

/// `#rrggbb` (pure).
fn is_colour(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// A look as it may be stored: few colours, short plain ids, each a `#rrggbb` (lower-cased). The
/// ids aren't checked against the UI's list: one it doesn't know is ignored there (pure).
fn checked_look(mut l: crate::model::Look) -> Result<crate::model::Look> {
    let id_ok = |k: &str| (1..=24).contains(&k.len()) && k.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if l.colors.len() > LOOK_MAX || !l.colors.iter().all(|(k, c)| id_ok(k) && is_colour(c)) {
        return Err(AppError::Invalid(tr!("That theme has a colour RoALT can't use.")));
    }
    for c in l.colors.values_mut() {
        c.make_ascii_lowercase();
    }
    Ok(l)
}

#[tauri::command]
pub fn settings_set(app: AppHandle, s: State<'_, App>, patch: SettingsPatch) -> Result<Settings> {
    set_settings(&app, &s, patch)
}

/// Checks `patch`, saves it and makes it take effect (`settings_set`; also a backup's settings and
/// the tray's Anti-AFK tick).
pub(crate) fn set_settings(app: &AppHandle, s: &App, patch: SettingsPatch) -> Result<Settings> {
    if patch.auto_lock_minutes.is_some() || patch.lock_on_session_lock.is_some() {
        unlocked(s)?;
    }
    let change = s.db.settings_change();
    let mut v = s.db.settings()?;
    apply_performance(&mut v, &patch)?;
    if let Some(t) = patch.theme { v.theme = t }
    if let Some(l) = &patch.language {
        if !(l.is_empty() || crate::i18n::LANGS.contains(&l.as_str())) {
            return Err(AppError::Invalid(tr!("That language isn't available.")));
        }
        v.language = l.clone();
    }
    if let Some(a) = patch.accent {
        if !(a.is_empty() || is_colour(&a)) {
            return Err(AppError::Invalid(tr!("The accent must be a #rrggbb colour.")));
        }
        v.accent = a.to_ascii_lowercase();
    }
    if let Some(l) = patch.look { v.look = checked_look(l)? }
    if let Some(c) = patch.corners {
        if !CORNERS.contains(&c) {
            return Err(AppError::Invalid(tr!("Corners must be 0, 50, 100, 150 or 200 %.")));
        }
        v.corners = c;
    }
    if let Some(d) = patch.background_dim {
        if d > 95 {
            return Err(AppError::Invalid(tr!("The background dim must be 0 to 95 %.")));
        }
        v.background_dim = d;
    }
    if let Some(m) = patch.background_motion { v.background_motion = m }
    if let Some(d) = patch.density { v.density = d }
    if let Some(m) = patch.auto_lock_minutes {
        if m > 24 * 60 {
            return Err(AppError::Invalid(tr!("Auto-lock can be at most 24 hours.")));
        }
        v.auto_lock_minutes = m;
    }
    if let Some(l) = patch.lock_on_session_lock { v.lock_on_session_lock = l }
    if let Some(c) = patch.clipboard_clear_seconds {
        if !(5..=300).contains(&c) {
            return Err(AppError::Invalid(tr!("Clipboard clearing must be between 5 and 300 seconds.")));
        }
        v.clipboard_clear_seconds = c;
    }
    if let Some(c) = patch.cache_minutes {
        if !(1..=24 * 60).contains(&c) {
            return Err(AppError::Invalid(tr!("Cache time must be between 1 minute and 24 hours.")));
        }
        v.cache_minutes = c;
    }
    if let Some(r) = patch.auto_rejoin { v.auto_rejoin = r }
    if let Some(r) = patch.auto_restart_on_crash { v.auto_restart_on_crash = r }
    if let Some(n) = patch.breaker_max {
        if !(1..=20).contains(&n) {
            return Err(AppError::Invalid(tr!("The restart limit must be between 1 and 20.")));
        }
        v.breaker_max = n;
    }
    if let Some(m) = patch.breaker_window_min {
        if !(1..=240).contains(&m) {
            return Err(AppError::Invalid(tr!("The restart window must be between 1 minute and 4 hours.")));
        }
        v.breaker_window_min = m;
    }
    if let Some(g) = patch.close_grace_s {
        if !(1..=60).contains(&g) {
            return Err(AppError::Invalid(tr!("The close grace must be between 1 and 60 seconds.")));
        }
        v.close_grace_s = g;
    }
    if let Some(a) = patch.launch_args {
        crate::launcher::launch_args(&a)?;
        v.launch_args = a.trim().to_owned();
    }
    if let Some(f) = patch.custom_flags {
        crate::launcher::flags::check(&f)?;
        v.custom_flags = f;
    }
    if let Some(c) = patch.custom_flags_scope { v.custom_flags_scope = c }
    if let Some(l) = patch.lock_purchases { v.lock_purchases = l }
    if let Some(m) = patch.mods_on { v.mods_on = m }
    if let Some(m) = patch.mods_ack { v.mods_ack = m }
    if let Some(o) = patch.onboarded { v.onboarded = o }
    if let Some(l) = patch.tab_layout { v.tab_layout = l }
    if let Some(c) = patch.close_to_tray { v.close_to_tray = c }
    if let Some(s) = patch.skip_splash { v.skip_splash = s }
    if let Some(p) = patch.protect_main { v.protect_main = p }
    if let Some(m) = patch.mute_on_launch { v.mute_on_launch = m }
    if let Some(l) = patch.window_layout { v.window_layout = l }
    if let Some(a) = patch.anti_afk {
        // Roblox disconnects after 20 idle minutes.
        if !(1..=19).contains(&a.interval_min) {
            return Err(AppError::Invalid(tr!("Anti-AFK must press a key every 1 to 19 minutes.")));
        }
        v.anti_afk = a;
    }
    if let Some(mut d) = patch.discord {
        if d.status_every_min != 0 && !(5..=24 * 60).contains(&d.status_every_min) {
            return Err(AppError::Invalid(tr!("The status summary must be off (0) or every 5 minutes to 24 hours.")));
        }
        d.ping = crate::discord::parse_ping(&d.ping)?;
        v.discord = d;
    }
    if let Some(a) = patch.auto_update { v.auto_update = a }
    if let Some(a) = patch.ask_browser_links { v.ask_browser_links = a }
    let self_update = patch.self_update.filter(|&u| u != v.self_update);
    if let Some(u) = self_update { v.self_update = u }
    if let Some(p) = patch.discord_presence { v.discord_presence = p }
    if let Some(mut r) = patch.discord_rpc {
        r.custom_details = r.custom_details.trim().chars().take(128).collect();
        r.custom_state = r.custom_state.trim().chars().take(128).collect();
        v.discord_rpc = r;
    }
    if let Some(o) = patch.online_count { v.online_count = o }
    if let Some(o) = patch.news { v.news = o }
    if let Some(o) = patch.show_sponsor { v.show_sponsor = o }
    if let Some(o) = patch.sign_in_watch { v.sign_in_watch = o }
    for (hours, to) in [(patch.restart_every_h, &mut v.restart_every_h), (patch.close_after_h, &mut v.close_after_h)] {
        if let Some(h) = hours {
            if h > 7 * 24 {
                return Err(AppError::Invalid(tr!("The timer must be off (0) or 1 to 168 hours.")));
            }
            *to = h;
        }
    }
    if let Some(r) = patch.rolimons_values { v.rolimons_values = r }
    if let Some(c) = patch.clean_logs { v.clean_logs = c }
    if let Some(d) = patch.launch_delay_ms {
        if !(2_000..=60_000).contains(&d) {
            return Err(AppError::Invalid(tr!("The launch delay must be between 2 and 60 seconds.")));
        }
        v.launch_delay_ms = d;
    }
    // Multi-instance itself goes through multi_instance_set (it has to take or drop the lock).
    let protect = patch.protect_shared_login.filter(|&p| p != v.protect_shared_login);
    if let Some(p) = protect { v.protect_shared_login = p }
    let had = v.panic_hotkey.clone();
    // Not a key one of RoALT's other things has (`hotkeys`): each key does one thing.
    if let Some(k) = patch.panic_hotkey.as_deref()
        && parse_hotkey(k.trim()).is_ok_and(|p| crate::hotkeys::id_of(&p).is_some())
    {
        return Err(AppError::Invalid(tr!("{key} is set for two things. Give each its own key.", key = k.trim())));
    }
    // Last: it takes effect at once, so only once everything else is valid; put back if
    // anything after fails, so Windows and the settings agree.
    if let Some(k) = patch.panic_hotkey {
        set_panic_hotkey(app, &had, k.trim())?;
        v.panic_hotkey = k.trim().to_owned();
    }
    if let Some(a) = patch.allow_public_networks { v.allow_public_networks = a }
    // The other global keys, tried before the save: one Windows won't give is refused, and the ones
    // there stay.
    let keys = [(&patch.toggle_hotkey, &mut v.toggle_hotkey), (&patch.tile_hotkey, &mut v.tile_hotkey), (&patch.minimize_hotkey, &mut v.minimize_hotkey)];
    let (mut rekey, mut changed) = (false, Vec::new());
    for (new, to) in keys {
        if let Some(k) = new {
            if k.trim() != to.as_str() {
                changed.push(k.trim());
            }
            *to = k.trim().to_owned();
            rekey = true;
        }
    }
    if rekey && let Err(e) = crate::hotkeys::try_change(app, Some(&v), None, &changed) {
        if v.panic_hotkey != had
            && let Err(e) = set_panic_hotkey(app, &v.panic_hotkey, &had)
        {
            log::warn!("could not roll back the panic hotkey: {e}");
        }
        return Err(e);
    }
    let rest = s.db.save_settings(&v);
    if rest.is_err() {
        // As saved before: the keys follow the settings that are still there.
        if rekey {
            crate::hotkeys::sync(app);
        }
        if v.panic_hotkey != had
            && let Err(e) = set_panic_hotkey(app, &v.panic_hotkey, &had)
        {
            log::warn!("could not roll back the panic hotkey after a failed save: {e}");
        }
    }
    drop(change);
    rest?;
    // Once saved: a refused change never drops the shared-login lock.
    match protect {
        Some(false) => s.multi.release_shared(),
        Some(true) if v.multi_instance => protect_running(s, &v),
        _ => {}
    }
    // Once saved: a refused change never loosens the lock Browse is under.
    crate::browse::set_lock(v.lock_purchases);
    if patch.language.is_some() {
        crate::i18n::set(&v.language);
    }
    // Its language, and its Anti-AFK tick (nothing is rebuilt when neither changed).
    crate::tray::refresh(app);
    if let Some(u) = self_update {
        crate::self_update::switched(app, u);
    }
    Ok(v)
}

// ---------- network ----------

/// Whether a Public network is connected (`network`); app://network sends changes.
#[tauri::command]
pub fn network_status(s: State<'_, App>) -> NetworkStatus {
    s.network.get()
}

#[cfg(test)]
mod tests {
    #[test]
    fn tail_keeps_the_last_lines() {
        let path = std::env::temp_dir().join(format!("roalt-tail-{}.log", uuid::Uuid::new_v4()));
        assert!(crate::commands::tail(&path, 5).unwrap().is_empty());
        std::fs::write(&path, "a\nb\nc\nd\n").unwrap();
        assert_eq!(crate::commands::tail(&path, 2).unwrap(), ["c", "d"]);
        assert_eq!(crate::commands::tail(&path, 10).unwrap(), ["a", "b", "c", "d"]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn recovery_file_holds_only_a_code() {
        assert!(super::recovery_file_text("abc def 234").unwrap().contains("ABC-DEF-234"));
        for other in ["", "ABC-DEF-23", "ABC-DEF-230", "cookie=_|WARNING:-DO-NOT-SHARE", "ABC-DEF-234\nand more"] {
            assert!(matches!(super::recovery_file_text(other), Err(crate::error::AppError::Invalid(_))), "{other:?}");
        }
    }

    use super::{GameDetails, GameInfo, find_cookies, game_info_of, is_image, normalize_cookie, ram_accounts, shutdown_secs};

    #[test]
    fn servers_estimate_rounds_up_and_needs_a_size() {
        use super::servers_estimate;
        assert_eq!(servers_estimate(0, 12), Some(0));
        assert_eq!(servers_estimate(1, 12), Some(1));
        assert_eq!(servers_estimate(24, 12), Some(2));
        assert_eq!(servers_estimate(25, 12), Some(3));
        assert_eq!(servers_estimate(25, 0), None);
        assert_eq!(servers_estimate(-3, 12), Some(0));
    }

    /// What Rolimons says about an item it values at `value`.
    fn roli(value: Option<i64>) -> crate::model::RolimonsItem {
        crate::model::RolimonsItem {
            name: String::new(),
            rap: None,
            value,
            demand: Some(2),
            trend: None,
            projected: false,
            hyped: false,
            rare: false,
            acronym: None,
        }
    }

    #[test]
    fn value_of_adds_copies_and_falls_back_to_rap() {
        use super::{Holdings, TOP_ITEMS, value_of};
        use crate::roblox_api::types::Holding;
        let hold = |asset_id, copies, rap| Holding { asset_id, name: format!("item {asset_id}"), copies, rap };
        let held = Holdings { fetched_at: 5, partial: false, items: vec![hold(1, 2, 100), hold(2, 1, 1_000), hold(3, 1, 50)] };

        let v = value_of(&held, None);
        assert_eq!((v.count, v.rap, v.value, v.fetched_at, v.partial), (4, 1_250, None, 5, false), "RAP of every copy; no value without Rolimons");
        assert_eq!(v.top.iter().map(|i| i.asset_id).collect::<Vec<_>>(), [2, 1, 3], "by worth each");

        // Item 1 valued above its RAP, item 2 not valued (its RAP counts), item 3 unknown to Rolimons.
        let list = std::collections::HashMap::from([(1, roli(Some(900))), (2, roli(None))]);
        let v = value_of(&held, Some(&list));
        assert_eq!(v.value, Some(2 * 900 + 1_000 + 50));
        assert_eq!(v.rap, 1_250, "RAP stays Roblox's");
        assert_eq!(v.top.iter().map(|i| i.asset_id).collect::<Vec<_>>(), [2, 1, 3]);
        assert!(v.top[2].rolimons.is_none() && v.top[1].rolimons.as_ref().is_some_and(|r| r.value == Some(900)));

        let many = Holdings { fetched_at: 0, partial: true, items: (1..=15).map(|i| hold(i, 1, i * 10)).collect() };
        let v = value_of(&many, None);
        assert_eq!((v.top.len(), v.top[0].asset_id, v.count, v.partial), (TOP_ITEMS, 15, 15, true));
        assert!(!crate::model::Settings::default().rolimons_values, "Rolimons is off until turned on");
    }

    #[test]
    fn looks_keep_only_plain_colours() {
        use super::{checked_look, LOOK_MAX};
        use crate::model::Look;
        let look = |pairs: &[(&str, &str)]| Look { colors: pairs.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect(), ..Look::default() };
        let ok = checked_look(look(&[("ground", "#0A1B2C"), ("fg-2", "#ffffff")])).unwrap();
        assert_eq!(ok.colors["ground"], "#0a1b2c", "stored lower-case");
        for bad in [("ground", "red"), ("ground", "#12345"), ("ground", "#1234567"), ("Ground", "#000000"), ("", "#000000"), ("a b", "#000000"), ("--ground", "#000000x")] {
            assert!(checked_look(look(&[bad])).is_err(), "{bad:?}");
        }
        let many: Vec<(String, String)> = (0..=LOOK_MAX).map(|i| (format!("t{i}"), "#000000".to_string())).collect();
        assert!(checked_look(Look { colors: many.into_iter().collect(), ..Look::default() }).is_err(), "too many colours");
        assert!(checked_look(Look::default()).is_ok(), "no colours is the base");
    }

    #[test]
    fn knows_images_by_their_bytes() {
        assert!(is_image(&[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n', 0]));
        assert!(is_image(&[0xff, 0xd8, 0xff, 0xe0]));
        assert!(is_image(b"GIF89a"));
        assert!(is_image(b"RIFF\0\0\0\0WEBPVP8 "));
        assert!(!is_image(b"RIFF\0\0\0\0WAVE"));
        assert!(!is_image(b"<svg></svg>"));
        assert!(!is_image(b""));
    }

    const W: &str = "_|WARNING:-DO-NOT-SHARE-THIS.--Sharing-this-will-allow-someone-to-log-in-as-you-and-to-steal-your-ROBUX-and-items.|_";

    #[test]
    fn finds_cookies_in_pasted_lists() {
        let (a, b) = (format!("{W}ABC123"), format!("{W}CAEaAh+/x="));
        let text = format!("user:pass:{a}
  {b}  
.ROBLOSECURITY={a};
\"{b}\", other
nothing here
{W}
{a} {b}");
        assert_eq!(find_cookies(&text), [a.clone(), b.clone()]);
        assert_eq!(find_cookies(&format!("{a}:user:pass")), std::slice::from_ref(&a));
        assert_eq!(find_cookies(&format!("{a}\u{201d}\u{200b}")), [a], "a smart quote or zero-width space ends it");
        assert!(find_cookies("no cookies").is_empty());
    }

    /// A backup carries `Settings` and restores them as a `SettingsPatch` (`restore_settings`):
    /// every setting must read back as its patch field, or the whole restore is refused.
    #[test]
    fn stored_settings_read_as_a_patch() {
        let json = serde_json::to_value(crate::model::Settings::default()).unwrap();
        let patch: crate::model::SettingsPatch = serde_json::from_value(json).unwrap();
        assert_eq!(patch.fps_cap, Some(crate::model::Settings::default().fps_cap));
        assert_eq!(patch.discord.map(|d| d.sign_in), Some(true));
    }

    #[test]
    fn reads_roblox_account_manager_json() {
        let json = br#"[{"SecurityToken":"tok","Username":"u","Alias":"farm","Description":"note","Group":"Default"},{"SecurityToken":"","Alias":null}]"#;
        let list = ram_accounts(json).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!((list[0].0.as_deref(), list[0].1.alias.as_str(), list[0].1.notes.as_str()), (Some("tok"), "farm", "note"));
        assert!(list[1].0.is_none() && list[1].1.alias.is_empty());
        assert!(list[0].1.group.is_empty() && list[1].1.group.is_empty(), "RAM's Default, or none, is no group");
        let grouped = ram_accounts(br#"[{"SecurityToken":"tok","Group":"Farm 1"}]"#).unwrap();
        assert_eq!(grouped[0].1.group, "Farm 1");
        let bom = [b"\xEF\xBB\xBF".as_slice(), b"[]"].concat();
        assert!(ram_accounts(&bom).unwrap().is_empty());
        assert!(ram_accounts(b"{}").is_err());
    }

    #[test]
    fn shutdown_delays() {
        let now = 1_000_000_000_000;
        assert_eq!(shutdown_secs(None, Some(3600), now).unwrap(), 3600);
        // `in_secs` wins; a time is rounded up to the next second.
        assert_eq!(shutdown_secs(Some(now + 5_000_000), Some(60), now).unwrap(), 60);
        assert_eq!(shutdown_secs(Some(now + 90_001), None, now).unwrap(), 91);
        for bad in [(None, None), (None, Some(59)), (Some(now - 1000), None), (Some(now + 59_000), None), (None, Some(7 * 24 * 3600 + 1))] {
            assert!(shutdown_secs(bad.0, bad.1, now).is_err(), "{bad:?}");
        }
        assert_eq!(shutdown_secs(None, Some(7 * 24 * 3600), now).unwrap(), 7 * 24 * 3600);
    }

    #[test]
    fn the_suggested_panic_hotkey_parses() {
        assert!(crate::model::Settings::default().panic_hotkey.is_empty(), "off until the user picks one");
        let k = "Ctrl+Shift+F12";
        assert!(k.parse::<tauri_plugin_global_shortcut::Shortcut>().is_ok(), "{k}");
        assert!("Ctrl+Shift+".parse::<tauri_plugin_global_shortcut::Shortcut>().is_err());
    }

    #[test]
    fn game_info_keeps_the_asked_place() {
        let g = GameDetails {
            universe_id: 994732206,
            root_place_id: 2753915549,
            name: "Blox Fruits".into(),
            creator: "Gamer Robot Inc".into(),
            playing: 193096,
            visits: 65079604670,
            max_players: 12,
            private_servers: true,
            ..Default::default()
        };
        let info = game_info_of(7449423635, g, Some("https://tr.rbxcdn.com/i".into()));
        let want = GameInfo {
            place_id: 7449423635,
            universe_id: 994732206,
            root_place_id: 2753915549,
            name: "Blox Fruits".into(),
            creator: "Gamer Robot Inc".into(),
            playing: 193096,
            visits: 65079604670,
            max_players: 12,
            icon_url: Some("https://tr.rbxcdn.com/i".into()),
            private_servers: true,
        };
        assert_eq!(info, want);
        // What the cache stores reads back the same.
        assert_eq!(serde_json::from_str::<GameInfo>(&serde_json::to_string(&info).unwrap()).unwrap(), want);
    }

    #[test]
    fn cookie_input_forms() {
        for raw in ["abc123", " abc123 ", "\"abc123\"", ".ROBLOSECURITY=abc123;", "'.ROBLOSECURITY=abc123'"] {
            assert_eq!(normalize_cookie(raw).unwrap().expose(), "abc123", "{raw}");
        }
        assert!(normalize_cookie("").is_err());
        assert!(normalize_cookie("has space").is_err());
        assert!(normalize_cookie(".ROBLOSECURITY=").is_err());
    }

    #[test]
    fn csv_cells_are_quoted_and_never_formulas() {
        assert_eq!(super::csv_line(&["a", "b c", ""]), "a,b c,\r\n");
        assert_eq!(super::csv_line(&["say \"hi\", you", "two\nlines"]), "\"say \"\"hi\"\", you\",\"two\nlines\"\r\n");
        assert_eq!(super::csv_line(&["=1+1", "-5", "@x", "+1", "a=b"]), "'=1+1,'-5,'@x,'+1,a=b\r\n");
    }
}
