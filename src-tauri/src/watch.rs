//! What RoALT looks at by itself while it runs, so trouble is known before a launch finds it:
//! every account's sign-in (`settings.sign_in_watch`: 90 s after start, then every 6 hours), only
//! with the vault unlocked, one account at a time.

use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::App;
use crate::commands;
use crate::model::AccountStatus;

const SIGN_IN_FIRST: Duration = Duration::from_secs(90);
const SIGN_IN_EVERY: Duration = Duration::from_secs(6 * 3600);
/// When a check couldn't run (switched off, locked, a launch under way): looked at again this soon.
const SIGN_IN_RETRY: Duration = Duration::from_secs(5 * 60);
/// A sign-in checked within this long isn't asked about again by the watch.
const JUST_CHECKED_MS: i64 = 10 * 60_000;

/// Runs for the app's lifetime.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(SIGN_IN_FIRST).await;
        loop {
            let wait = if sign_ins(&app).await { SIGN_IN_EVERY } else { SIGN_IN_RETRY };
            tokio::time::sleep(wait).await;
        }
    });
}

/// Checks the sign-in of every account not known dead already (`commands::validate_accounts`:
/// one that died is marked, and told to Discord). False when it didn't run now.
async fn sign_ins(app: &AppHandle) -> bool {
    let s = app.state::<App>();
    if !s.db.settings().is_ok_and(|v| v.sign_in_watch) || s.vault.is_locked() || s.bulk.any_busy() {
        return false;
    }
    // Not one checked a moment ago (the Accounts list checks once when it first shows).
    let stale = commands::now_ms() - JUST_CHECKED_MS;
    let ids: Vec<i64> = match s.db.accounts() {
        Ok(list) => list.into_iter().filter(|a| a.status != AccountStatus::Invalid && a.status_checked_at.is_none_or(|t| t < stale)).map(|a| a.id).collect(),
        Err(e) => {
            log::info!("sign-in watch: accounts unreadable: {e}");
            return false;
        }
    };
    if ids.is_empty() {
        return true;
    }
    log::debug!("sign-in watch: checking {} accounts", ids.len());
    commands::validate_accounts(app, &s, Some(ids)).await.inspect_err(|e| log::info!("sign-in watch: not checked: {e}")).is_ok()
}
