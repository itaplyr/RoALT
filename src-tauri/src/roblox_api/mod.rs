//! The one typed client for every Roblox web API RoALT uses.
//!
//! - One `reqwest::Client`, no cookie jar: `.ROBLOSECURITY` is set per request.
//! - CSRF: the real request is sent with the cached token for that account; a 403 carrying
//!   `x-csrf-token` stores the new token and retries once (any host).
//! - Rate limits: per-route next-slot throttle fed by `x-ratelimit-*`; 429 waits for
//!   `x-ratelimit-reset` (not `retry-after`), exponential backoff from 1 s, max 3 tries.
//! - Cookie rotation: a `Set-Cookie: .ROBLOSECURITY=` on any response goes to the sink
//!   (stored accounts) and is returned to the caller.
//! - Errors: 401 → `CookieInvalid` (`9002/2` → `BoundSessionRequired`), 403 with
//!   `rblx-challenge-*` → `Challenge`, exhausted 429 → `RateLimited`, else `Roblox(..)`.

pub mod endpoints;
#[cfg(test)]
mod tests;
pub mod types;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use reqwest::header::{ACCEPT, COOKIE, HeaderMap, HeaderValue, ORIGIN, REFERER, RETRY_AFTER, SET_COOKIE};
use reqwest::{Method, StatusCode, Url};
use serde::de::DeserializeOwned;
use serde_json::Value;
use zeroize::Zeroizing;

use crate::error::{AppError, Result};
use crate::vault::Secret;
use types::{
    AuthenticatedUser, Data, FriendOnline, Game, GameDetails, Presences, PrivateServerList, Profiles, ServerList,
    ShareLinkResolution, ShareLinkV2, SocialCounts, Thumbnails, TwoStepConfig, UserDetails, UserPresence,
};

/// Called with `(account_id, new cookie)` when Roblox rotates a stored account's cookie.
pub type RotationSink = Arc<dyn Fn(i64, Secret) + Send + Sync>;

const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36";
const MAX_ATTEMPTS: u32 = 3;
const CSRF: &str = "x-csrf-token";
const CHALLENGE: &str = "rblx-challenge-id";
/// The response header that carries a launch ticket (a secret).
const TICKET_HEADER: &str = "rbx-authentication-ticket";
const HEADSHOT_RETRIES: u32 = 5;
#[cfg(not(test))]
const HEADSHOT_STEP: Duration = Duration::from_millis(500);
#[cfg(test)]
const HEADSHOT_STEP: Duration = Duration::from_millis(1);
/// 2-step `mediaType` names by enum value.
const MEDIA_TYPES: [&str; 8] =
    ["Email", "SMS", "Authenticator", "RecoveryCode", "SecurityKey", "CrossDevice", "Password", "Passkey"];

/// Headers, body and the rotated cookie (if any) of a 2xx answer.
type Reply = (HeaderMap, Vec<u8>, Option<Secret>);

/// What a route's last answer said about its rate-limit window.
struct Window {
    remaining: u64,
    reset_at: Instant,
}

pub struct Client {
    http: std::result::Result<reqwest::Client, String>,
    on_rotate: RotationSink,
    /// Replaces scheme and host of every URL (tests).
    base: Option<String>,
    csrf: Mutex<HashMap<i64, String>>,
    windows: Mutex<HashMap<&'static str, Window>>,
}

impl Client {
    pub fn new(on_rotate: RotationSink) -> Client {
        Client::with_base(on_rotate, None)
    }

    fn with_base(on_rotate: RotationSink, base: Option<String>) -> Client {
        let accept = HeaderValue::from_static("application/json, text/plain, */*");
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .default_headers(HeaderMap::from_iter([(ACCEPT, accept)]))
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|e| e.without_url().to_string())
            .inspect_err(|e| log::error!("Roblox http client unavailable: {e}"));
        Client { http, on_rotate, base, csrf: Mutex::default(), windows: Mutex::default() }
    }

    /// `GET users.roblox.com/v1/users/authenticated`, the only reliable cookie check.
    /// `account_id` 0 = cookie not stored yet (no sink call). Returns the user and, if Roblox
    /// rotated the cookie during the call, the new value.
    pub async fn authenticated_user(&self, account_id: i64, cookie: &Secret) -> Result<(AuthenticatedUser, Option<Secret>)> {
        let url = endpoints::USERS_AUTHENTICATED;
        let (_, body, rotated) = self.request(Method::GET, url, url, None, account_id, Some(cookie)).await?;
        Ok((json(&body)?, rotated))
    }

    /// `GET users.roblox.com/v1/users/{id}` (public). Sent with the account's cookie when
    /// one is given (higher limit than anonymous: 30/min).
    pub async fn user_details(&self, account_id: i64, cookie: Option<&Secret>, user_id: i64) -> Result<UserDetails> {
        self.get(endpoints::USER, &endpoints::fill(endpoints::USER, user_id), account_id, cookie).await
    }

    /// Avatar headshot URLs, 150x150 PNG, no cookie. ≤100 ids per call (chunks larger
    /// lists); re-requests only the `Pending` ids up to 5 times (0.5 s, 1 s, … steps).
    /// Ids that end Blocked / Error / still Pending, or with an empty `imageUrl`, are absent.
    pub async fn headshots(&self, user_ids: &[i64]) -> Result<HashMap<i64, String>> {
        let mut out = HashMap::new();
        for chunk in valid_ids(user_ids).chunks(100) {
            let mut pending = chunk.to_vec();
            for retry in 0..=HEADSHOT_RETRIES {
                if retry > 0 {
                    tokio::time::sleep(HEADSHOT_STEP * retry).await;
                }
                let url = endpoints::fill(endpoints::HEADSHOTS, csv(&pending));
                // A failed retry keeps what earlier answers already resolved.
                let page: Thumbnails = match self.get(endpoints::HEADSHOTS, &url, 0, None).await {
                    Ok(p) => p,
                    Err(e) if retry > 0 => {
                        log::debug!("headshots: retry {retry} for {} pending id(s) failed: {e}", pending.len());
                        break;
                    }
                    Err(e) => return Err(e),
                };
                pending = sort_thumbnails(page, &mut out);
                if pending.is_empty() {
                    break;
                }
            }
        }
        Ok(out)
    }

    /// `GET economy.roblox.com/v1/user/currency` → `robux` of the cookie's own account.
    pub async fn robux(&self, account_id: i64, cookie: &Secret) -> Result<i64> {
        let url = endpoints::CURRENCY;
        field(self.get(url, url, account_id, Some(cookie)).await?, "robux")
    }

    /// Legacy Premium: `GET premiumfeatures.roblox.com/v1/users/{id}/validate-membership`
    /// (body is a bare JSON boolean). Marked for removal by Roblox; callers treat errors as unknown.
    pub async fn premium(&self, account_id: i64, cookie: &Secret, user_id: i64) -> Result<bool> {
        self.get(endpoints::PREMIUM, &endpoints::fill(endpoints::PREMIUM, user_id), account_id, Some(cookie)).await
    }

    /// Roblox Plus via `POST apis.roblox.com/user-profile-api/v1/user/profiles/get-profiles`
    /// `{userIds, fields:["hasRobloxSubscription"]}`, no cookie, ≤200 ids per call. A row
    /// without the field (unknown user, renamed field) is absent: never guess `false`.
    pub async fn plus(&self, user_ids: &[i64]) -> Result<HashMap<i64, bool>> {
        let (url, mut out) = (endpoints::PROFILES, HashMap::new());
        for chunk in valid_ids(user_ids).chunks(200) {
            let body = serde_json::json!({ "userIds": chunk, "fields": ["hasRobloxSubscription"] });
            let (_, bytes, _) = self.request(Method::POST, url, url, Some(&body), 0, None).await?;
            let page: Profiles = json(&bytes)?;
            let (rows, before) = (page.profile_details.len(), out.len());
            out.extend(page.profile_details.into_iter().filter_map(|p| Some((p.user_id, p.has_roblox_subscription?))));
            if out.len() - before < rows {
                log::debug!("plus: {} of {rows} profile(s) without hasRobloxSubscription", rows - (out.len() - before));
            }
        }
        Ok(out)
    }

    /// `friends.roblox.com/v1/users/{id}/{friends,followers,followings}/count` (public).
    pub async fn social_counts(&self, user_id: i64) -> Result<SocialCounts> {
        let count = move |route: &'static str| async move {
            field::<i64>(self.get(route, &endpoints::fill(route, user_id), 0, None).await?, "count")
        };
        let (friends, followers, following) = tokio::try_join!(
            count(endpoints::FRIENDS_COUNT),
            count(endpoints::FOLLOWERS_COUNT),
            count(endpoints::FOLLOWINGS_COUNT)
        )?;
        Ok(SocialCounts { friends, followers, following })
    }

    /// `POST presence.roblox.com/v1/presence/users {userIds}` as the viewer (its cookie),
    /// ≤50 ids per call (chunks), ids ≤ 0 dropped and duplicates removed before sending.
    pub async fn presence(&self, account_id: i64, cookie: &Secret, user_ids: &[i64]) -> Result<Vec<UserPresence>> {
        let (url, mut out) = (endpoints::PRESENCE, Vec::new());
        for chunk in valid_ids(user_ids).chunks(50) {
            let body = serde_json::json!({ "userIds": chunk });
            let (_, bytes, _) = self.request(Method::POST, url, url, Some(&body), account_id, Some(cookie)).await?;
            out.extend(json::<Presences>(&bytes)?.user_presences);
        }
        Ok(out)
    }

    /// `GET accountsettings.roblox.com/v1/email` → `verified`. The address is not kept.
    pub async fn email_verified(&self, account_id: i64, cookie: &Secret) -> Result<bool> {
        let url = endpoints::EMAIL;
        field(self.get(url, url, account_id, Some(cookie)).await?, "verified")
    }

    /// `GET twostepverification.roblox.com/v1/users/{id}/configuration` → names of the
    /// enabled methods ("Email", "SMS", "Authenticator", "SecurityKey", "Passkey", …).
    /// `mediaType` may arrive as a number or a string.
    pub async fn two_step_methods(&self, account_id: i64, cookie: &Secret, user_id: i64) -> Result<Vec<String>> {
        let url = endpoints::fill(endpoints::TWO_STEP, user_id);
        let config: TwoStepConfig = self.get(endpoints::TWO_STEP, &url, account_id, Some(cookie)).await?;
        Ok(enabled_methods(config))
    }

    /// A GET whose JSON answer is `T`.
    async fn get<T: DeserializeOwned>(
        &self,
        route: &'static str,
        url: &str,
        account_id: i64,
        cookie: Option<&Secret>,
    ) -> Result<T> {
        let (_, body, _) = self.request(Method::GET, route, url, None, account_id, cookie).await?;
        json(&body)
    }

    /// `GET auth.roblox.com/v1/client-assertion/` → the whole JSON answer (`{clientAssertion}`),
    /// sent as the body of the ticket request. Callers ignore failures, as the website does.
    pub async fn client_assertion(&self, account_id: i64, cookie: &Secret) -> Result<Value> {
        let url = endpoints::CLIENT_ASSERTION;
        self.get(url, url, account_id, Some(cookie)).await
    }

    /// `POST auth.roblox.com/v1/authentication-ticket/` (JSON body = the assertion, or `{}`),
    /// with Referer/Origin www.roblox.com; the ticket is the `rbx-authentication-ticket`
    /// response header (missing → `Roblox("no ticket")`). Fetched right before each launch.
    pub async fn authentication_ticket(&self, account_id: i64, cookie: &Secret, assertion: Option<&Value>) -> Result<Secret> {
        let (url, empty) = (endpoints::AUTH_TICKET, Value::Object(Default::default()));
        let body = assertion.unwrap_or(&empty);
        let (headers, _, _) = self.request(Method::POST, url, url, Some(body), account_id, Some(cookie)).await?;
        let ticket = headers.get(TICKET_HEADER).and_then(|v| v.to_str().ok()).filter(|t| !t.is_empty());
        if ticket.is_none() {
            log::warn!("account {account_id}: auth ticket header missing or unreadable");
        }
        ticket.map(Secret::new).ok_or_else(|| AppError::Roblox("no ticket".into()))
    }

    /// `POST apis.roblox.com/sharelinks/v1/resolve-link {linkId, linkType}` (cookie + CSRF).
    /// The answer has one `*Data` object per link type; see types::ShareLinkResolution.
    pub async fn resolve_share_link(&self, account_id: i64, cookie: &Secret, code: &str, link_type: &str) -> Result<ShareLinkResolution> {
        let (url, body) = (endpoints::SHARE_LINK, serde_json::json!({ "linkId": code, "linkType": link_type }));
        let (_, bytes, _) = self.request(Method::POST, url, url, Some(&body), account_id, Some(cookie)).await?;
        json(&bytes)
    }

    /// `POST apis.roblox.com/deeplinks/v2/resolve {linkId}` (cookie + CSRF) for `?v=v2` links.
    pub async fn resolve_share_link_v2(&self, account_id: i64, cookie: &Secret, code: &str) -> Result<ShareLinkV2> {
        let (url, body) = (endpoints::SHARE_LINK_V2, serde_json::json!({ "linkId": code }));
        let (_, bytes, _) = self.request(Method::POST, url, url, Some(&body), account_id, Some(cookie)).await?;
        json(&bytes)
    }

    /// `GET games.roblox.com/v1/games?universeIds=<csv ≤50>` (no cookie) → universe id →
    /// (root place id, game name). Unknown universes are absent.
    pub async fn universe_root_places(&self, universe_ids: &[i64]) -> Result<HashMap<i64, (i64, String)>> {
        Ok(self.games(universe_ids).await?.into_iter().map(|g| (g.universe_id, (g.root_place_id, g.name))).collect())
    }

    /// `POST users.roblox.com/v1/usernames/users {usernames ≤200, excludeBannedUsers:true}`
    /// (no cookie). Matches previous usernames too; unknown names are absent.
    pub async fn users_by_name(&self, names: &[String]) -> Result<Vec<AuthenticatedUser>> {
        let (url, mut out) = (endpoints::USERS_BY_NAME, Vec::new());
        for chunk in names.chunks(200) {
            let body = serde_json::json!({ "usernames": chunk, "excludeBannedUsers": true });
            let (_, bytes, _) = self.request(Method::POST, url, url, Some(&body), 0, None).await?;
            out.extend(json::<Data<AuthenticatedUser>>(&bytes)?.data);
        }
        Ok(out)
    }

    /// `GET friends.roblox.com/v1/users/{id}/friends/online` with the account's cookie:
    /// friends online right now with their presence (PascalCase `UserPresenceType` string,
    /// `gameInstanceId`), see research account-info A14.
    pub async fn friends_online(&self, account_id: i64, cookie: &Secret, user_id: i64) -> Result<Vec<FriendOnline>> {
        let url = endpoints::fill(endpoints::FRIENDS_ONLINE, user_id);
        Ok(self.get::<Data<FriendOnline>>(endpoints::FRIENDS_ONLINE, &url, account_id, Some(cookie)).await?.data)
    }

    /// `GET games.roblox.com/v2/games/{placeId}/servers/Public?orderBy=..&excludeFullGames=..&limit=..&cursor=..`
    /// (`endpoints::SERVERS_PUBLIC`; the website's own list). `limit` must be 10, 25, 50 or 100
    /// (anything else → `Invalid` without a call). `cookie` optional; Recommended / BestLatency
    /// without one get Roblox's 400 code 7 → `Invalid("Pick an account to sort by ...")`.
    /// Parsing is lenient (only id / maxPlayers / playing required, unknown fields ignored,
    /// rows with a missing id dropped); the cursor is URL-encoded and passed through opaque.
    #[allow(clippy::too_many_arguments)] // one per query parameter, as the IPC command has them
    pub async fn servers_public(
        &self,
        account_id: i64,
        cookie: Option<&Secret>,
        place_id: i64,
        order: crate::model::ServerOrder,
        exclude_full: bool,
        limit: u8,
        cursor: Option<&str>,
    ) -> Result<crate::model::ServerPage> {
        use crate::model::{GameServer, ServerOrder, ServerPage};
        if ![10, 25, 50, 100].contains(&limit) {
            return Err(AppError::Invalid(tr!("A server page holds 10, 25, 50 or 100 servers.")));
        }
        let order_by = match order {
            ServerOrder::OccupancyDesc => "OccupancyDesc",
            ServerOrder::OccupancyAsc => "OccupancyAsc",
            ServerOrder::Recommended => "Recommended",
            ServerOrder::BestLatency => "BestLatency",
        };
        let route = endpoints::SERVERS_PUBLIC;
        let mut url = format!(
            "{}?orderBy={order_by}&excludeFullGames={exclude_full}&limit={limit}",
            endpoints::fill(route, place_id)
        );
        if let Some(c) = cursor.filter(|c| !c.is_empty()) {
            url.push_str("&cursor=");
            url.push_str(&crate::launcher::uri::encode(c));
        }
        let list: ServerList = self.get(route, &url, account_id, cookie).await?;
        if list.data.is_none() {
            log::debug!("servers_public: place {place_id} answer has no data");
        }
        let rows = list.data.unwrap_or_default();
        let total = rows.len();
        let servers: Vec<GameServer> = rows.into_iter().filter_map(|r| {
            Some(GameServer {
                // A job id goes into a launch URI: GUIDs only (DESIGN §6).
                id: r.id.filter(|id| crate::launcher::target::is_uuid(id))?,
                playing: r.playing?,
                max_players: r.max_players?,
                fps: r.fps,
                ping: r.ping,
                friend_count: r.friend_count,
            })
        }).collect();
        if servers.len() < total {
            log::debug!("servers_public: place {place_id}: {} of {total} row(s) dropped", total - servers.len());
        }
        Ok(ServerPage { servers, next_cursor: list.next_page_cursor.filter(|c| !c.is_empty()) })
    }

    /// `GET apis.roblox.com/universes/v1/places/{placeId}/universe` (no cookie): the place's
    /// universe; `{"universeId": null}` (unknown place) → None, as is an id ≤ 0 (no call).
    pub async fn place_universe(&self, place_id: i64) -> Result<Option<i64>> {
        if place_id <= 0 {
            return Ok(None);
        }
        let route = endpoints::PLACE_UNIVERSE;
        field(self.get(route, &endpoints::fill(route, place_id), 0, None).await?, "universeId")
    }

    /// `GET games.roblox.com/v1/games?universeIds=` (no cookie, ≤50 per call, the existing
    /// `GAMES` endpoint) with everything `GameInfo` needs: id, rootPlaceId, name, creator.name,
    /// playing, visits, maxPlayers, createVipServersAllowed. Lenient: missing or null fields
    /// default; unknown universes, and rows without id or rootPlaceId, are absent.
    pub async fn games(&self, universe_ids: &[i64]) -> Result<Vec<GameDetails>> {
        let mut out = Vec::new();
        for chunk in valid_ids(universe_ids).chunks(50) {
            let url = endpoints::fill(endpoints::GAMES, csv(chunk));
            let page: Data<Game> = self.get(endpoints::GAMES, &url, 0, None).await?;
            let (rows, before) = (page.data.len(), out.len());
            out.extend(page.data.into_iter().filter_map(|g| {
                Some(GameDetails {
                    universe_id: g.id?,
                    root_place_id: g.root_place_id?,
                    name: g.name.unwrap_or_default(),
                    creator: g.creator.and_then(|c| c.name).unwrap_or_default(),
                    playing: g.playing.unwrap_or_default(),
                    visits: g.visits.unwrap_or_default(),
                    max_players: g.max_players.unwrap_or_default(),
                    private_servers: g.create_vip_servers_allowed.unwrap_or_default(),
                })
            }));
            if out.len() - before < rows {
                log::debug!("games: {} of {rows} row(s) without id or rootPlaceId", rows - (out.len() - before));
            }
        }
        Ok(out)
    }

    /// `GET thumbnails.roblox.com/v1/places/gameicons?placeIds=..` (no cookie, `PLACE_ICONS`):
    /// place id → image URL for `Completed` icons (matched by `targetId`, not by order).
    pub async fn place_icons(&self, place_ids: &[i64]) -> Result<HashMap<i64, String>> {
        let mut out = HashMap::new();
        for chunk in valid_ids(place_ids).chunks(100) {
            let url = endpoints::fill(endpoints::PLACE_ICONS, csv(chunk));
            sort_thumbnails(self.get(endpoints::PLACE_ICONS, &url, 0, None).await?, &mut out);
        }
        Ok(out)
    }

    /// `GET games.roblox.com/v1/games/{placeId}/private-servers?cursor=..` with the account's
    /// cookie (Roblox only answers signed in). Lenient parsing: `data[]` rows with `vipServerId`,
    /// `name`, `owner{id,name}`, `playing`, `maxPlayers`, `id` (job id, may be absent),
    /// `accessCode` (only present when this account can join), `nextPageCursor`,
    /// `gameJoinRestricted`. Job ids and access codes that aren't GUIDs are dropped.
    pub async fn private_servers(
        &self,
        account_id: i64,
        cookie: &Secret,
        place_id: i64,
        cursor: Option<&str>,
    ) -> Result<crate::model::PrivateServerPage> {
        use crate::launcher::target::is_uuid;
        use crate::model::{PrivateServer, PrivateServerPage};
        let route = endpoints::SERVERS_PRIVATE;
        // The query the website sends.
        let mut url = endpoints::fill(route, place_id) + "?sortOrder=Desc&excludeFullGames=false";
        if let Some(c) = cursor.filter(|c| !c.is_empty()) {
            url.push_str("&cursor=");
            url.push_str(&crate::launcher::uri::encode(c));
        }
        let list: PrivateServerList = self.get(route, &url, account_id, Some(cookie)).await?;
        let rows = list.data.unwrap_or_default();
        let total = rows.len();
        let (mut bad_job, mut bad_code) = (0usize, 0usize);
        let servers: Vec<PrivateServer> = rows.into_iter().filter_map(|r| {
            let (owner_id, owner_name) = r.owner.map_or((None, None), |o| (o.id, o.name));
            bad_job += usize::from(r.id.as_deref().is_some_and(|id| !is_uuid(id)));
            bad_code += usize::from(r.access_code.as_deref().is_some_and(|c| !is_uuid(c)));
            Some(PrivateServer {
                vip_server_id: r.vip_server_id?,
                name: r.name.unwrap_or_default(),
                owner_id,
                owner_name,
                playing: r.playing.unwrap_or_default(),
                max_players: r.max_players.unwrap_or_default(),
                job_id: r.id.filter(|id| is_uuid(id)),
                access_code: r.access_code.filter(|code| is_uuid(code)),
            })
        }).collect();
        if servers.len() < total || bad_job + bad_code > 0 {
            log::debug!(
                "private_servers: place {place_id}: {} of {total} row(s) without vipServerId, {bad_job} job id(s) and {bad_code} access code(s) not GUIDs",
                total - servers.len()
            );
        }
        Ok(PrivateServerPage {
            servers,
            next_cursor: list.next_page_cursor.filter(|c| !c.is_empty()),
            join_restricted: list.game_join_restricted.unwrap_or_default(),
        })
    }

    /// `GET clientsettingscdn.roblox.com/v2/client-version/WindowsPlayer` (no cookie) → the live
    /// `clientVersionUpload` (`version-` + 16 hex digits; anything else → `Roblox(..)`).
    pub async fn client_version(&self) -> Result<String> {
        let route = endpoints::CLIENT_VERSION;
        let (_, body, _) = self.request(Method::GET, route, route, None, 0, None).await?;
        client_version_of(&body)
    }

    /// `client_version` of a release channel (Roblox puts some PCs on a test channel).
    pub async fn client_version_on(&self, channel: &str) -> Result<String> {
        let url = format!("{}/channel/{channel}", endpoints::CLIENT_VERSION);
        let (_, body, _) = self.request(Method::GET, endpoints::CLIENT_VERSION, &url, None, 0, None).await?;
        client_version_of(&body)
    }

    /// Downloads `url` (no cookie, no redirects) in at most 5 minutes; refused when it is over
    /// `max` bytes (by `Content-Length` before, or while reading). `progress(received, total)`
    /// after each chunk.
    pub async fn download(&self, url: &str, max: u64, mut progress: impl FnMut(u64, Option<u64>)) -> Result<Vec<u8>> {
        let http = self.http.as_ref().map_err(|e| AppError::Internal(tr!("HTTP client unavailable: {why}", why = e)))?;
        let (url, _) = self.target(url)?;
        let mut resp = http.get(url).timeout(Duration::from_secs(300)).send().await.map_err(network)?;
        if !resp.status().is_success() {
            log::warn!("download: HTTP {}", resp.status().as_u16());
            return Err(AppError::Roblox(format!("download: HTTP {}", resp.status().as_u16())));
        }
        let too_big = || {
            log::warn!("download: over {max} bytes, refused");
            AppError::Roblox(format!("download over {} MB", max / 1_000_000))
        };
        let total = resp.content_length();
        if total.is_some_and(|t| t > max) {
            return Err(too_big());
        }
        let mut out = Vec::with_capacity(total.unwrap_or(0) as usize);
        while let Some(chunk) = resp.chunk().await.map_err(network)? {
            if (out.len() + chunk.len()) as u64 > max {
                return Err(too_big());
            }
            out.extend_from_slice(&chunk);
            progress(out.len() as u64, total);
        }
        Ok(out)
    }

    /// One API call with throttle, CSRF retry, 429 backoff and cookie rotation.
    /// `route` keys the throttle (one per endpoint, not per URL). No cookie: no `Cookie`
    /// header, CSRF cached as account 0, no rotation.
    async fn request(
        &self,
        method: Method,
        route: &'static str,
        url: &str,
        body: Option<&Value>,
        account_id: i64,
        cookie: Option<&Secret>,
    ) -> Result<Reply> {
        let account_id = if cookie.is_some() { account_id } else { 0 };
        let http = self.http.as_ref().map_err(|e| AppError::Internal(tr!("HTTP client unavailable: {why}", why = e)))?;
        let (url, web) = self.target(url)?;
        let (mut rotated, mut csrf_retried, mut attempts) = (None::<Secret>, false, 0);
        loop {
            self.wait_turn(route).await?;
            let current = rotated.as_ref().or(cookie);
            let mut req = http.request(method.clone(), url.clone());
            if let Some(current) = current {
                req = req.header(COOKIE, cookie_header(current)?);
            }
            if web {
                req = req.header(REFERER, endpoints::WWW).header(ORIGIN, endpoints::WEB_ORIGIN);
            }
            let token = lock(&self.csrf).get(&account_id).cloned();
            if (method != Method::GET || csrf_retried)
                && let Some(token) = token
            {
                req = req.header(CSRF, token);
            }
            if let Some(body) = body {
                req = req.json(body);
            }

            let mut resp = req.send().await.map_err(network)?;
            let (status, headers) = (resp.status(), resp.headers().clone());
            let limits = RateLimit::parse(&headers);
            if let (Some(remaining), Some(reset)) = (limits.remaining, limits.reset_secs()) {
                self.set_window(route, remaining, reset);
            }
            if let Some(current) = current
                && let Some(new) = rotated_cookie(&headers).filter(|new| new.expose() != current.expose())
            {
                if account_id != 0 {
                    log::debug!("account {account_id}: cookie rotated by Roblox");
                    (self.on_rotate)(account_id, new.clone());
                }
                rotated = Some(new);
            }
            // After the headers: a body that fails must not lose a rotated cookie.
            let mut bytes = Vec::new();
            while let Some(chunk) = resp.chunk().await.map_err(network)? {
                if bytes.len() + chunk.len() > MAX_BODY {
                    return Err(AppError::Roblox(tr!("Roblox's answer was too large.")));
                }
                bytes.extend_from_slice(&chunk);
            }

            if status.is_success() {
                return Ok((headers, bytes, rotated));
            }
            if status == StatusCode::TOO_MANY_REQUESTS {
                attempts += 1;
                let wait = limits.reset.or(limits.retry_after).unwrap_or(1 << (attempts - 1)).max(1);
                self.set_window(route, 0, wait);
                if attempts >= MAX_ATTEMPTS || wait > MAX_WAIT_SECS {
                    log::warn!("rate limited on {route} {attempts} times, giving up");
                    return Err(AppError::RateLimited(wait));
                }
                log::info!("rate limited on {route}, retrying in {wait} s");
                continue;
            }
            if status == StatusCode::FORBIDDEN
                && !csrf_retried
                && !headers.contains_key(CHALLENGE)
                && let Some(token) = headers.get(CSRF).and_then(|v| v.to_str().ok())
            {
                lock(&self.csrf).insert(account_id, token.to_owned());
                csrf_retried = true;
                continue;
            }
            // The server list's "Guest users are not allowed." (Recommended / BestLatency).
            if route == endpoints::SERVERS_PUBLIC && status == StatusCode::BAD_REQUEST && error_code(&bytes) == Some((7, 0)) {
                return Err(AppError::Invalid(tr!(
                    "Pick an account to sort by Recommended or Best latency: Roblox only does that for a signed-in account."
                )));
            }
            let e = classify(status, &headers, &bytes);
            log::warn!("{route}: HTTP {status}, code {:?}: {e}", error_code(&bytes));
            if status == StatusCode::FORBIDDEN {
                // Whose refusal it is (the API's or a CDN's page), without the body.
                let header = |name| headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or("-");
                log::warn!(
                    "{route}: 403 details: CSRF retried {csrf_retried}, x-csrf-token sent back {}, challenge {}, content-type {}, server {}",
                    headers.contains_key(CSRF),
                    headers.contains_key(CHALLENGE),
                    header("content-type"),
                    header("server"),
                );
            }
            return Err(e);
        }
    }

    /// The URL to call and whether it gets the website's `Referer` / `Origin`.
    fn target(&self, url: &str) -> Result<(Url, bool)> {
        let bad = |e: url::ParseError| {
            log::error!("bad Roblox URL: {e}");
            AppError::Internal("bad Roblox URL".into())
        };
        let mut u = Url::parse(url).map_err(bad)?;
        let web = matches!(u.host_str(), Some("auth.roblox.com" | "apis.roblox.com"));
        if let Some(base) = &self.base {
            u = Url::parse(&format!("{base}{}", &u[url::Position::BeforePath..])).map_err(bad)?;
        }
        Ok((u, web))
    }

    /// Waits while `route`'s window is used up, else takes one request from it.
    /// A longer wait than `MAX_WAIT_SECS` fails instead: callers hold launch locks.
    async fn wait_turn(&self, route: &'static str) -> Result<()> {
        loop {
            let wait = {
                let mut windows = lock(&self.windows);
                let now = Instant::now();
                match windows.get_mut(route) {
                    Some(w) if now < w.reset_at && w.remaining == 0 => w.reset_at - now,
                    Some(w) if now < w.reset_at => {
                        w.remaining -= 1;
                        return Ok(());
                    }
                    _ => return Ok(()),
                }
            };
            if wait.as_secs() > MAX_WAIT_SECS {
                return Err(AppError::RateLimited(wait.as_secs()));
            }
            tokio::time::sleep(wait).await;
        }
    }

    fn set_window(&self, route: &'static str, remaining: u64, reset_secs: u64) {
        // The value is whatever the header says; the longest real window is an hour.
        let reset_at = Instant::now() + Duration::from_secs(reset_secs.min(3600));
        lock(&self.windows).insert(route, Window { remaining, reset_at });
    }
}

/// The most a call waits for its rate-limit window before failing with `RateLimited`.
const MAX_WAIT_SECS: u64 = 60;
/// The largest answer read. Real ones are far smaller; a failed allocation would end the app.
const MAX_BODY: usize = 16 << 20;
const COOKIE_MARK: &str = "_|WARNING:-DO-NOT-SHARE-THIS.";

/// The `x-ratelimit-*` and `retry-after` headers of one answer.
#[derive(Debug, Default, PartialEq)]
struct RateLimit {
    /// Window length in seconds, the `w=` of `x-ratelimit-limit: 20, 20;w=60`.
    window: Option<u64>,
    remaining: Option<u64>,
    /// Seconds until the window resets.
    reset: Option<u64>,
    retry_after: Option<u64>,
}

impl RateLimit {
    fn parse(h: &HeaderMap) -> RateLimit {
        let get = |name: &str| h.get(name).and_then(|v| v.to_str().ok());
        RateLimit {
            window: get("x-ratelimit-limit").and_then(|v| v.split_once("w=")).and_then(|(_, w)| first_number(w)),
            remaining: get("x-ratelimit-remaining").and_then(first_number),
            reset: get("x-ratelimit-reset").and_then(first_number),
            retry_after: get(RETRY_AFTER.as_str()).and_then(first_number),
        }
    }

    /// `x-ratelimit-reset`, else the next window boundary (Roblox aligns windows to the clock).
    fn reset_secs(&self) -> Option<u64> {
        self.reset.or_else(|| {
            let w = self.window.filter(|&w| w > 0)?;
            let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
            Some(w - now % w)
        })
    }
}

/// The leading integer of `"20, 20;w=60"`-style values.
fn first_number(v: &str) -> Option<u64> {
    v.split([',', ';']).next()?.trim().parse().ok()
}

/// A new `.ROBLOSECURITY` among the `Set-Cookie` headers, last one wins. Deletions
/// (empty value, `Max-Age=0`, a 1970 expiry) are not a rotation.
fn rotated_cookie(h: &HeaderMap) -> Option<Secret> {
    let parse = |v: &str| {
        let mut parts = v.split(';');
        let value = parts.next()?.trim().strip_prefix(".ROBLOSECURITY=")?.trim();
        // Only what is shaped like one may replace the stored cookie: "deleted" isn't.
        let deleted = !value.starts_with(COOKIE_MARK)
            || value.len() > 8192
            || !value.bytes().all(|b| b.is_ascii_graphic())
            || parts.any(|a| {
                let a = a.trim().to_ascii_lowercase();
                a == "max-age=0" || a.starts_with("max-age=-") || (a.starts_with("expires=") && a.contains("1970"))
            });
        (!deleted).then(|| Secret::new(value))
    };
    h.get_all(SET_COOKIE).iter().filter_map(|v| v.to_str().ok()).filter_map(parse).next_back()
}

fn cookie_header(cookie: &Secret) -> Result<HeaderValue> {
    let raw = Zeroizing::new(format!(".ROBLOSECURITY={}", cookie.expose()));
    let mut v = HeaderValue::from_str(&raw).map_err(|_| {
        log::warn!("cookie has bytes not allowed in a header");
        AppError::CookieInvalid
    })?;
    v.set_sensitive(true);
    Ok(v)
}

/// Maps a final non-2xx answer. Never quotes the body: it may echo what was sent.
fn classify(status: StatusCode, headers: &HeaderMap, body: &[u8]) -> AppError {
    match status {
        StatusCode::UNAUTHORIZED if error_code(body) == Some((9002, 2)) => AppError::BoundSessionRequired,
        StatusCode::UNAUTHORIZED => AppError::CookieInvalid,
        StatusCode::FORBIDDEN if headers.contains_key(CHALLENGE) => AppError::Challenge,
        s => AppError::Roblox(format!("HTTP {}", s.as_u16())),
    }
}

/// `(code, subcode)` of `{errors:[{..}]}` or a flat `{code, ..}` body. Bare strings,
/// `{Error: ..}` and empty bodies have none.
fn error_code(body: &[u8]) -> Option<(i64, i64)> {
    let v: Value = serde_json::from_slice(body).ok()?;
    let e = v.get("errors").and_then(|e| e.get(0)).unwrap_or(&v);
    Some((e.get("code")?.as_i64()?, e.get("subcode").and_then(Value::as_i64).unwrap_or(0)))
}

/// A transport error, named by host only (no path or query).
fn network(e: reqwest::Error) -> AppError {
    let what = if e.is_timeout() {
        tr!("timed out")
    } else if e.is_connect() {
        tr!("could not connect")
    } else {
        tr!("connection failed")
    };
    let host = e.url().and_then(Url::host_str).map(str::to_owned);
    // The cause chain (TLS, DNS, refused...) without the URL.
    let e = e.without_url();
    let mut why = e.to_string();
    let mut src = std::error::Error::source(&e);
    while let Some(s) = src {
        why.push_str(&format!(": {s}"));
        src = s.source();
    }
    log::warn!("Roblox request to {} failed: {why}", host.as_deref().unwrap_or("?"));
    match host {
        Some(host) => AppError::Network(format!("{what} ({host})")),
        None => AppError::Network(what),
    }
}

/// `clientVersionUpload` of a client-version answer, only when it is a plain version name.
fn client_version_of(body: &[u8]) -> Result<String> {
    let v: String = field(json(body)?, "clientVersionUpload")?;
    if crate::launcher::update::is_version(&v) {
        Ok(v)
    } else {
        log::debug!("client version: unexpected clientVersionUpload {:?}", v.chars().take(64).collect::<String>());
        Err(unreadable())
    }
}

fn json<T: DeserializeOwned>(body: &[u8]) -> Result<T> {
    serde_json::from_slice(body).map_err(|e| {
        // The category and place only: the message can quote the body.
        log::warn!("unreadable Roblox answer: {:?} error at line {} column {}", e.classify(), e.line(), e.column());
        unreadable()
    })
}

/// One top-level field of a JSON object answer; missing or mistyped is unreadable.
fn field<T: DeserializeOwned>(mut v: Value, name: &str) -> Result<T> {
    serde_json::from_value(v.get_mut(name).map(Value::take).unwrap_or_default()).map_err(|e| {
        log::warn!("unreadable Roblox answer: field \"{name}\" is missing or {:?}", e.classify());
        unreadable()
    })
}

fn unreadable() -> AppError {
    AppError::Roblox("unreadable response".into())
}

/// Positive ids, each once (Roblox fails a whole presence batch on one id ≤ 0).
fn valid_ids(ids: &[i64]) -> Vec<i64> {
    let mut ids: Vec<i64> = ids.iter().copied().filter(|&id| id > 0).collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn csv(ids: &[i64]) -> String {
    ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
}

/// Keeps Completed rows with a URL in `out` (by `targetId`); returns the ids still Pending.
fn sort_thumbnails(page: Thumbnails, out: &mut HashMap<i64, String>) -> Vec<i64> {
    let mut pending = Vec::new();
    for t in page.data {
        match (t.state.as_str(), t.image_url) {
            // The UI loads these as images: Roblox's CDN only.
            ("Completed", Some(url)) if is_rbxcdn(&url) => {
                out.insert(t.target_id, url);
            }
            ("Pending", _) => pending.push(t.target_id),
            (state, _) => log::debug!("thumbnail for {} dropped ({:?})", t.target_id, state.chars().take(64).collect::<String>()),
        }
    }
    pending
}

/// An https URL on *.rbxcdn.com.
fn is_rbxcdn(url: &str) -> bool {
    Url::parse(url).is_ok_and(|u| u.scheme() == "https" && u.host_str().is_some_and(|h| h.ends_with(".rbxcdn.com")))
}

/// Names of the enabled 2-step methods; recovery codes and the password don't count.
fn enabled_methods(config: TwoStepConfig) -> Vec<String> {
    let name = |v: &Value| match v {
        Value::Number(n) => MEDIA_TYPES.get(usize::try_from(n.as_u64()?).ok()?).copied(),
        Value::String(s) => MEDIA_TYPES.iter().find(|m| m.eq_ignore_ascii_case(s)).copied(),
        _ => None,
    };
    config
        .methods
        .iter()
        .filter(|m| m.enabled)
        .filter_map(|m| {
            let n = name(&m.media_type);
            if n.is_none() {
                log::debug!("2-step: unknown mediaType {:?}", m.media_type.to_string().chars().take(64).collect::<String>());
            }
            n
        })
        .filter(|n| !matches!(*n, "RecoveryCode" | "Password"))
        .map(String::from)
        .collect()
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}
