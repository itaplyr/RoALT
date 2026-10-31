//! Launching one account into a target, the way roblox.com's Play button does:
//! resolve the target with that account's cookie, get an authentication ticket, build the
//! `roblox-player:` URI and start the client. Tickets and URIs are secrets: never logged.

#[cfg(windows)]
pub mod authenticode;
pub mod bulk;
#[cfg(windows)]
pub mod client;
#[cfg(not(windows))]
#[path = "client_linux.rs"]
pub mod client;
pub mod client_settings;
pub mod flags;
pub mod target;
#[cfg(windows)]
pub mod update;
#[cfg(not(windows))]
#[path = "update_linux.rs"]
pub mod update;
pub mod uri;

use serde_json::Value;
use uuid::Uuid;

use crate::App;
use crate::error::{AppError, Result};
use crate::model::{LaunchInfo, LaunchTarget};
use crate::roblox_api::Client;
use crate::roblox_api::types::{ShareLinkResolution, ShareLinkV2, ShareTarget};
use crate::vault::{Secret, cookie_aad};

/// Roblox's limit on launch data, in bytes.
const MAX_LAUNCH_DATA: usize = 200;

/// A target resolved for one account: what PlaceLauncher is asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Join {
    /// `RequestGame`.
    Game { place_id: i64, launch_data: Option<String> },
    /// `RequestGameJob` (`gameId` = job id).
    Job { place_id: i64, job_id: String },
    /// `RequestPrivateGame`; share-link and `privateServerLinkCode` joins send only the link code.
    Private { place_id: i64, link_code: Option<String>, access_code: Option<String> },
    /// `RequestFollowUser`.
    Follow { user_id: i64 },
}

impl Join {
    /// The place the client goes to, when known before it starts.
    pub fn place_id(&self) -> Option<i64> {
        match self {
            Join::Game { place_id, .. } | Join::Job { place_id, .. } | Join::Private { place_id, .. } => Some(*place_id),
            Join::Follow { .. } => None,
        }
    }
}

/// Resolves `target` with the account's own cookie into a `Join`:
/// - `ShareLink` → `POST apis.roblox.com/sharelinks/v1/resolve-link`. Server links use
///   `privateServerInviteData` (`placeId` + `linkCode`; universe → root place only when
///   `placeId` is missing). `Invalid` status → `AppError::Invalid("This share link is no
///   longer valid.")`; `Expired` still joins, as the website does, with a warning.
///   ExperienceInvite / NotificationExperienceInvite with an instance → `Job`; ExperienceDetails /
///   Affiliate / Event / Screenshot → the game (root place, launch data when given); Profile →
///   `Follow`; other types → `Invalid`.
/// - `ShareLinkV2` → `POST apis.roblox.com/deeplinks/v2/resolve`; `Experience` → `Game`
///   (`targetId` place, `customData` as launch data); other link types → `Invalid`.
/// - `Universe` → its root place (`games.roblox.com/v1/games?universeIds=`).
/// - Launch data over 200 bytes is dropped (Roblox's limit) with a warning.
///
/// The second value is a warning for the user (expired link, dropped launch data).
pub async fn resolve(
    api: &Client,
    account_id: i64,
    cookie: &Secret,
    target: &LaunchTarget,
) -> Result<(Join, Option<String>)> {
    let mut warning = None;
    let join = match target.clone() {
        LaunchTarget::Place { place_id, launch_data } => Join::Game { place_id, launch_data },
        LaunchTarget::Server { place_id, job_id } => Join::Job { place_id, job_id },
        LaunchTarget::PrivateLink { place_id, link_code } => {
            Join::Private { place_id, link_code: Some(link_code), access_code: None }
        }
        LaunchTarget::PrivateAccess { place_id, access_code } => {
            Join::Private { place_id, link_code: None, access_code: Some(access_code) }
        }
        LaunchTarget::ShareLink { code, link_type } => {
            let (join, expired) =
                share_join(api, api.resolve_share_link(account_id, cookie, &code, &link_type).await?).await?;
            warning = expired.then(expired_warning);
            join
        }
        LaunchTarget::ShareLinkV2 { code } => {
            let v = api.resolve_share_link_v2(account_id, cookie, &code).await?;
            warning = is_status(&v.status, "Expired").then(expired_warning);
            v2_join(v)?
        }
        LaunchTarget::Universe { universe_id } => {
            Join::Game { place_id: root_place(api, universe_id).await?, launch_data: None }
        }
        LaunchTarget::FollowUser { user_id } => Join::Follow { user_id },
    };
    let (join, dropped) = cap_launch_data(join);
    Ok((join, warning.or_else(|| dropped.then(|| tr!("The link's launch data is over Roblox's 200-byte limit and was left out.")))))
}

fn expired_warning() -> String {
    tr!("This share link has expired, so Roblox may refuse the join.")
}

const MAX_ARGS_LEN: usize = 512;
const MAX_ARGS: usize = 32;

/// Extra client arguments typed by the user, split on whitespace. Refused (`Invalid`): over
/// 512 characters or 32 arguments, quotes, control characters (newlines), `& | < > ^` (a
/// bootstrapper may hand them to a shell), and anything starting with `roblox-player:` (a
/// second launch URI). They go after the launch URI as typed; never logged.
pub fn launch_args(text: &str) -> Result<Vec<String>> {
    let invalid = |why: String| Err(AppError::Invalid(why));
    if text.chars().count() > MAX_ARGS_LEN {
        return invalid(tr!("Launch arguments can be at most {max} characters.", max = MAX_ARGS_LEN));
    }
    if let Some(c) = text.chars().find(|&c| c.is_control() && !matches!(c, ' ' | '\t') || "\"'&|<>^".contains(c)) {
        return invalid(if c.is_control() {
            tr!("Launch arguments can't contain line breaks or control characters.")
        } else {
            tr!("Launch arguments can't contain {char}.", char = c)
        });
    }
    let args: Vec<String> = text.split_whitespace().map(str::to_owned).collect();
    if args.len() > MAX_ARGS {
        return invalid(tr!("Launch arguments can be at most {max} words.", max = MAX_ARGS));
    }
    if args.iter().any(|a| a.to_ascii_lowercase().starts_with("roblox-player:")) {
        return invalid(tr!("Launch arguments can't contain another roblox-player: link."));
    }
    Ok(args)
}

/// The account's own launch arguments, else the default from settings (`launch_args`).
fn args_for(s: &App, account_id: i64) -> Result<Vec<String>> {
    let own = s.db.account(account_id)?.launch_args;
    launch_args(&own.map_or_else(|| s.db.settings().map(|v| v.launch_args), Ok)?)
}

/// One launch: resolve → `client-assertion` (failure ignored) → `authentication-ticket` →
/// URI with a fresh `browsertrackerid` → start the client (`update::launch_client` + `client::start`,
/// with the account's or the default launch arguments after the URI). Just before the client
/// starts, `fps_cap`, else the account's or the default FPS cap, and the lowest graphics when
/// `Settings.low_graphics` is on, go into Roblox's settings file (`client_settings`). It returns
/// once the client has started; a task keeps `client_settings::LOCK` until the client has read
/// the file and the user's own are back, so the next launch waits for that, not the caller.
/// `AppError::Locked` when the vault is locked; cookie errors as the API client maps them.
pub async fn launch(
    s: &App,
    account_id: i64,
    target: &LaunchTarget,
    fps_cap: Option<u32>,
    headless: bool,
) -> Result<LaunchInfo> {
    // First: the previous launch's client may still be reading the settings file, and this
    // launch's exe, ticket and start time should come after that wait, not before it.
    let held = client_settings::LOCK.lock().await;
    let cookie = account_cookie(s, account_id)?;
    // Before any cookie call: no ticket is fetched when Roblox isn't installed or the arguments
    // are refused.
    let (exe, production) = update::launch_client(s).await?;
    let args = args_for(s, account_id)?;
    let (join, warning) = resolve(&s.api, account_id, &cookie, target).await?;
    let assertion = s
        .api
        .client_assertion(account_id, &cookie)
        .await
        .inspect_err(|e| log::warn!("client assertion for account {account_id} failed, ticket fetched without it: {e}"))
        .ok();
    let ticket = s.api.authentication_ticket(account_id, &cookie, assertion.as_ref()).await?;
    let (browser_tracker_id, started_at) = (uri::new_browser_tracker_id(), crate::commands::now_ms());
    let place = uri::PlaceLaunch { join: &join, browser_tracker_id, join_attempt_id: Uuid::new_v4() };
    let settings = s.db.settings()?;
    let account = s.db.account(account_id)?;
    let fps = fps_cap.or(account.fps_cap).unwrap_or(settings.fps_cap);
    let undo = client_settings::apply(&s.db, fps, settings.low_graphics && !account.full_quality);
    let flags = match (settings.lighter_rendering && !account.full_quality, exe.bootstrapper) {
        (false, _) => None,
        (true, true) => {
            log::info!("lighter rendering skipped for account {account_id}: a bootstrapper manages Roblox's flags");
            None
        }
        (true, false) => flags::apply(&exe.path, &settings.render_flags),
    };
    let waiting = undo.is_some() || flags.is_some();
    let put_back = move || {
        if let Some(undo) = undo {
            undo.put_back();
        }
        if let Some(flags) = flags {
            flags.put_back();
        }
    };
    let since = std::time::SystemTime::now();
    let mobile = uri::mobile_join_url(&join);
    let started = client::start(
        &exe,
        &uri::player_uri(&ticket, started_at, &place, production),
        mobile.as_deref(),
        &args,
        account_id,
        account.user_id,
        &account.username,
        &account.display_name,
        &cookie,
        browser_tracker_id,
        headless,
        settings.keep_running_on_close,
        fps,
    );
    match &started {
        Ok((pid, pid_start)) if waiting => {
            // A bootstrapper's own process exits by design: only the log tells then.
            let client = (!exe.bootstrapper).then_some((*pid, *pid_start));
            tauri::async_runtime::spawn(async move {
                client_settings::wait_until_read(browser_tracker_id, since, client).await;
                if let Err(e) = tokio::task::spawn_blocking(put_back).await {
                    log::warn!("Roblox settings: putting the user's own back failed: {e}");
                }
                drop(held);
            });
        }
        _ => put_back(),
    }
    let (pid, pid_start) = started?;

    log::info!("launched account {account_id} (pid {pid}, bootstrapper: {}, {} extra argument(s))", exe.bootstrapper, args.len());
    Ok(LaunchInfo {
        account_id,
        pid,
        browser_tracker_id,
        place_id: join.place_id(),
        started_at,
        via_bootstrapper: exe.bootstrapper,
        warning,
        fps_cap,
        pid_start: Some(pid_start),
    })
}

/// The account's decrypted cookie (`Locked` when the vault is locked).
/// Would `launch` get past Roblox now? Everything it asks for before starting a client, the login
/// ticket too (unused, it just expires). For a relaunch, before the old client is closed for it.
pub async fn launch_check(s: &App, account_id: i64, target: &LaunchTarget) -> Result<()> {
    let cookie = account_cookie(s, account_id)?;
    client::find()?;
    args_for(s, account_id)?;
    resolve(&s.api, account_id, &cookie, target).await?;
    let assertion = s.api.client_assertion(account_id, &cookie).await.ok();
    s.api.authentication_ticket(account_id, &cookie, assertion.as_ref()).await.map(drop)
}

pub(crate) fn account_cookie(s: &App, account_id: i64) -> Result<Secret> {
    let (user_id, blob) = s.db.account_cookie(account_id)?;
    s.vault.decrypt_secret(&cookie_aad(user_id), &blob)
}

enum ShareKind {
    Server,
    Invite,
    Game,
    Profile,
}

/// The filled `*Data` object of a share-link answer, mapped as `resolve` describes, and
/// whether the link has expired.
async fn share_join(api: &Client, r: ShareLinkResolution) -> Result<(Join, bool)> {
    let filled = [
        (ShareKind::Server, r.private_server_invite_data),
        (ShareKind::Invite, r.experience_invite_data),
        (ShareKind::Invite, r.notification_experience_invite_data),
        (ShareKind::Game, r.screenshot_invite_data),
        (ShareKind::Game, r.experience_details_invite_data),
        (ShareKind::Game, r.experience_affiliate_data),
        (ShareKind::Game, r.experience_event_data),
        (ShareKind::Profile, r.profile_link_resolution_response_data),
    ];
    let Some((kind, t)) = filled.into_iter().find_map(|(kind, t)| Some((kind, t?))) else {
        return Err(AppError::Invalid(tr!("This share link doesn't lead to a game or a player.")));
    };
    if is_status(&t.status, "Invalid") {
        return Err(no_longer_valid());
    }
    // As the website does, an invite joins its server only while it is Valid.
    let job = match kind {
        ShareKind::Invite if t.status.is_none() || is_status(&t.status, "Valid") => code(t.instance_id.clone()),
        _ => None,
    };
    let expired = is_status(&t.status, "Expired");
    let join = match (kind, job) {
        (ShareKind::Server, _) => Join::Private {
            place_id: share_place(api, &t).await?,
            link_code: Some(code(t.link_code).ok_or_else(no_longer_valid)?),
            access_code: None,
        },
        (ShareKind::Invite, Some(job_id)) => Join::Job { place_id: share_place(api, &t).await?, job_id },
        (ShareKind::Profile, _) => Join::Follow { user_id: t.user_id.ok_or_else(no_longer_valid)? },
        (ShareKind::Invite | ShareKind::Game, _) => {
            let data = t.launch_data.clone().filter(|d| !d.is_empty());
            let launch_data = data.or_else(|| text(t.join_data.as_ref()?.get("launchData")?));
            Join::Game { place_id: share_place(api, &t).await?, launch_data }
        }
    };
    Ok((join, expired))
}

/// `deeplinks/v2/resolve`: only `Experience` links launch a game.
fn v2_join(v: ShareLinkV2) -> Result<Join> {
    if is_status(&v.status, "Invalid") {
        return Err(no_longer_valid());
    }
    match (v.link_type.as_deref(), v.target_id) {
        (Some("Experience"), Some(place_id)) => {
            Ok(Join::Game { place_id, launch_data: v.custom_data.as_ref().and_then(text) })
        }
        _ => Err(AppError::Invalid(tr!("This share link doesn't lead to a game."))),
    }
}

/// The payload's place, else its universe's root place.
async fn share_place(api: &Client, t: &ShareTarget) -> Result<i64> {
    match (t.place_id, t.universe_id) {
        (Some(place_id), _) => Ok(place_id),
        (None, Some(universe_id)) => root_place(api, universe_id).await,
        (None, None) => Err(no_longer_valid()),
    }
}

pub(crate) async fn root_place(api: &Client, universe_id: i64) -> Result<i64> {
    let mut found = api.universe_root_places(&[universe_id]).await?;
    let place = found.remove(&universe_id).map(|(place_id, _)| place_id);
    place.ok_or_else(|| AppError::Invalid(tr!("This game no longer exists.")))
}

/// Drops launch data over Roblox's 200-byte limit (true when it did). Its content is never logged.
fn cap_launch_data(join: Join) -> (Join, bool) {
    match join {
        Join::Game { place_id, launch_data: Some(d) } if d.len() > MAX_LAUNCH_DATA => {
            log::warn!("launch data of {} bytes dropped (limit {MAX_LAUNCH_DATA})", d.len());
            (Join::Game { place_id, launch_data: None }, true)
        }
        join => (join, false),
    }
}

/// A link code or job id from Roblox, kept only if it is a plain token (letters, digits, `-`).
fn code(v: Option<String>) -> Option<String> {
    v.filter(|c| !c.is_empty() && c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-'))
}

/// Launch data as Roblox hands it over: a string as is, other JSON as its text.
fn text(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.clone()).filter(|s| !s.is_empty()),
        v => Some(v.to_string()),
    }
}

fn is_status(status: &Option<String>, want: &str) -> bool {
    status.as_deref().is_some_and(|s| s.eq_ignore_ascii_case(want))
}

fn no_longer_valid() -> AppError {
    AppError::Invalid(tr!("This share link is no longer valid."))
}

#[cfg(test)]
mod tests {
    use super::launch_args;

    #[test]
    fn launch_args_are_plain_words() {
        assert_eq!(launch_args("").unwrap(), Vec::<String>::new());
        assert_eq!(launch_args("  --fps 60\t-x  ").unwrap(), ["--fps", "60", "-x"]);
        assert_eq!(launch_args(r"-path=C:\Games\x %TEMP%").unwrap().len(), 2);
        for bad in ["a\"b", "it's", "a & b", "a|b", "a > b", "a < b", "a^b", "a\nb", "a\rb", "x\u{0}"] {
            assert!(launch_args(bad).is_err(), "{bad:?}");
        }
        assert!(launch_args("ROBLOX-PLAYER:1+launchmode:play").is_err());
        assert!(launch_args("-x roblox-player:1").is_err());
        assert!(launch_args("--not-roblox-player:1").is_ok(), "only a word starting with it");
        assert!(launch_args(&"x ".repeat(33)).is_err(), "33 words");
        assert!(launch_args(&"x".repeat(513)).is_err());
        assert!(launch_args(&"x".repeat(512)).is_ok());
        // The message never echoes what was typed, beyond the one refused character.
        let e = launch_args("secret&").unwrap_err().to_string();
        assert!(!e.contains("secret") && e.contains('&'), "{e}");
    }
}
