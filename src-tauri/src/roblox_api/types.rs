//! Response types. Lenient: unknown fields are ignored.

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticatedUser {
    pub id: i64,
    pub name: String,
    pub display_name: String,
}

/// `GET users.roblox.com/v1/users/{id}`.
#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UserDetails {
    pub id: i64,
    pub name: String,
    pub display_name: String,
    #[serde(default)]
    pub description: String,
    /// ISO 8601.
    pub created: Option<String>,
    #[serde(default)]
    pub is_banned: bool,
    #[serde(default)]
    pub has_verified_badge: bool,
}

/// Friends / followers / following counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SocialCounts {
    pub friends: i64,
    pub followers: i64,
    pub following: i64,
}

/// One row of `POST presence.roblox.com/v1/presence/users`.
#[derive(Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UserPresence {
    pub user_id: i64,
    pub user_presence_type: i64,
    pub last_location: Option<String>,
    pub place_id: Option<i64>,
    pub root_place_id: Option<i64>,
    pub game_id: Option<String>,
    pub universe_id: Option<i64>,
}

/// The `{data:[..]}` envelope of list answers.
#[derive(Deserialize, Debug)]
pub struct Data<T> {
    pub data: Vec<T>,
}

/// A row of `games.roblox.com/v1/games?universeIds=` as sent. Anything may be null or missing.
/// Rows without id or rootPlaceId are dropped by the caller.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Game {
    pub id: Option<i64>,
    pub root_place_id: Option<i64>,
    pub name: Option<String>,
    pub creator: Option<Party>,
    pub playing: Option<i64>,
    pub visits: Option<i64>,
    pub max_players: Option<i32>,
    pub create_vip_servers_allowed: Option<bool>,
    pub favorited_count: Option<i64>,
    /// ISO 8601.
    pub created: Option<String>,
    pub updated: Option<String>,
    /// The old single genre ("All" for most); `genre_l1` replaces it.
    pub genre: Option<String>,
    #[serde(rename = "genre_l1")]
    pub genre_l1: Option<String>,
    #[serde(rename = "genre_l2")]
    pub genre_l2: Option<String>,
    /// "MorphToR6", "MorphToR15" or "PlayerChoice"; an int has been seen too (then not shown).
    pub universe_avatar_type: Option<serde_json::Value>,
    pub copying_allowed: Option<bool>,
}

/// A game's `creator` or a private server's `owner`.
#[derive(Deserialize, Debug)]
pub struct Party {
    pub id: Option<i64>,
    pub name: Option<String>,
    /// "User" or "Group" (a game's creator).
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

/// A row of `games.roblox.com/v1/games?universeIds=` with what the server browser shows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GameDetails {
    pub universe_id: i64,
    pub root_place_id: i64,
    pub name: String,
    pub creator: String,
    pub playing: i64,
    pub visits: i64,
    pub max_players: i32,
    pub private_servers: bool,
    pub favourites: i64,
    pub created: Option<String>,
    pub updated: Option<String>,
    pub genre: Option<String>,
    pub subgenre: Option<String>,
    /// The owning group, when a group made it.
    pub group_id: Option<i64>,
    pub avatar_type: Option<String>,
    pub copying: Option<bool>,
}

/// `experience-guidelines/get-age-recommendation`.
#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AgeRecommendation {
    pub age_recommendation_details: Option<AgeDetails>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AgeDetails {
    pub summary: Option<AgeSummary>,
    pub descriptor_usages: Vec<DescriptorUsage>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AgeSummary {
    pub age_recommendation: Option<AgeLabel>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AgeLabel {
    /// "Mild", "Moderate", "Minimal", "Restricted".
    pub display_name: Option<String>,
    pub minimum_age: Option<i64>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DescriptorUsage {
    pub descriptor: Option<Descriptor>,
    /// false: the game was checked for it and doesn't have it.
    pub contains: Option<bool>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Descriptor {
    /// "Violence (Repeated/Mild)".
    pub display_name: Option<String>,
}

/// `develop.roblox.com/v1/universes/{id}`.
#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct UniverseRow {
    pub is_archived: Option<bool>,
}

/// A page of ids (`develop.roblox.com/v1/universes/{id}/places`).
#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct IdPage {
    pub next_page_cursor: Option<String>,
    pub data: Option<Vec<IdRow>>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
pub struct IdRow {
    pub id: Option<i64>,
}

/// `groups.roblox.com/v1/groups/{id}`.
#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct GroupRow {
    pub member_count: Option<i64>,
}

/// A row of `groups.roblox.com/v2/users/{id}/groups/roles`.
#[derive(Deserialize, Debug, Default)]
#[serde(default)]
pub struct GroupRoleRow {
    pub group: Option<GroupInfo>,
    pub role: Option<RoleInfo>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct GroupInfo {
    pub id: Option<i64>,
    pub name: Option<String>,
    pub member_count: Option<i64>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(default)]
pub struct RoleInfo {
    pub name: Option<String>,
}

/// A row of `accountinformation.roblox.com/v1/users/{id}/roblox-badges` (a bare array).
#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct RobloxBadgeRow {
    pub id: Option<i64>,
    pub name: Option<String>,
    pub image_url: Option<String>,
}

/// `games.roblox.com/v2/users/{id}/games`.
#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct UserGamePage {
    pub next_page_cursor: Option<String>,
    pub data: Option<Vec<UserGameRow>>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct UserGameRow {
    pub id: Option<i64>,
    pub name: Option<String>,
    pub root_place: Option<IdRow>,
    pub place_visits: Option<i64>,
}

/// `games.roblox.com/v1/games/votes?universeIds=`.
#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct VoteRow {
    pub id: Option<i64>,
    pub up_votes: Option<i64>,
    pub down_votes: Option<i64>,
}

/// `apis.roblox.com/game-passes/v1/universes/{id}/game-passes`: an empty `nextPageToken` is the end.
#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PassPage {
    pub game_passes: Option<Vec<PassRow>>,
    pub next_page_token: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct PassRow {
    pub id: Option<i64>,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub is_for_sale: Option<bool>,
    /// Null when off sale.
    pub price: Option<i64>,
}

/// `badges.roblox.com/v1/universes/{id}/badges`.
#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct BadgePage {
    pub next_page_cursor: Option<String>,
    pub data: Option<Vec<BadgeRow>>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct BadgeRow {
    pub id: Option<i64>,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub enabled: Option<bool>,
    pub statistics: Option<BadgeStats>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct BadgeStats {
    pub past_day_awarded_count: Option<i64>,
    pub awarded_count: Option<i64>,
    /// A fraction (0.001 is 0.1 %), despite the name.
    pub win_rate_percentage: Option<f64>,
}

/// `inventory.roblox.com/v1/users/{id}/assets/collectibles`: one row per copy.
#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CollectiblePage {
    pub next_page_cursor: Option<String>,
    pub data: Option<Vec<CollectibleRow>>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct CollectibleRow {
    pub asset_id: Option<i64>,
    pub name: Option<String>,
    pub recent_average_price: Option<i64>,
}

/// A list read page by page. `more`: pages were left unread (the cap, or a later page failed);
/// `complete`: nothing failed (stopping at the cap is still complete).
#[derive(Debug, Clone, PartialEq)]
pub struct Listed<T> {
    pub items: Vec<T>,
    pub more: bool,
    pub complete: bool,
}

/// One limited an account holds: its copies and RAP each (`Client::collectibles`).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Holding {
    pub asset_id: i64,
    pub name: String,
    pub copies: u32,
    pub rap: i64,
}

/// `apis.roblox.com/explore-api/v1/get-sorts`: the Charts' lists. Lenient: everything optional.
#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ExploreSorts {
    pub sorts: Vec<ExploreSort>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ExploreSort {
    /// "Games" for a list of games (others: filters, banners).
    pub content_type: String,
    pub sort_id: String,
    pub sort_display_name: String,
    pub games: Vec<ExploreGame>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ExploreGame {
    pub universe_id: Option<i64>,
    pub root_place_id: Option<i64>,
    pub name: String,
    pub player_count: i64,
    pub total_up_votes: i64,
    pub total_down_votes: i64,
    /// An ad: left out.
    pub is_sponsored: bool,
}

/// `thumbnails.roblox.com/v1/users/avatar-headshot` (and `/v1/batch`).
#[derive(Deserialize, Debug)]
pub struct Thumbnails {
    pub data: Vec<Thumbnail>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Thumbnail {
    pub target_id: i64,
    /// Completed, Pending, Blocked, Error, InReview, TemporarilyUnavailable.
    pub state: String,
    /// `""` while Pending; Blocked may carry a placeholder.
    pub image_url: Option<String>,
}

/// `get-profiles`. Roblox silently drops fields it doesn't know, row by row.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Profiles {
    #[serde(default)]
    pub profile_details: Vec<Profile>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub user_id: i64,
    pub has_roblox_subscription: Option<bool>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Presences {
    pub user_presences: Vec<UserPresence>,
}

/// `GET twostepverification.roblox.com/v1/users/{id}/configuration`.
#[derive(Deserialize, Debug)]
pub struct TwoStepConfig {
    #[serde(default)]
    pub methods: Vec<TwoStepMethod>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TwoStepMethod {
    /// An enum int (0 Email … 7 Passkey) or its name.
    pub media_type: serde_json::Value,
    #[serde(default)]
    pub enabled: bool,
}

/// `sharelinks/v1/resolve-link`: one `*Data` object is filled, by link type. Unknown fields
/// are ignored; every field is optional because Roblox omits them freely.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ShareLinkResolution {
    pub private_server_invite_data: Option<ShareTarget>,
    pub experience_invite_data: Option<ShareTarget>,
    pub notification_experience_invite_data: Option<ShareTarget>,
    pub screenshot_invite_data: Option<ShareTarget>,
    pub experience_details_invite_data: Option<ShareTarget>,
    pub experience_affiliate_data: Option<ShareTarget>,
    pub experience_event_data: Option<ShareTarget>,
    pub profile_link_resolution_response_data: Option<ShareTarget>,
}

/// The union of the fields the website reads from any share-link payload.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ShareTarget {
    /// Valid | Expired | Invalid | InviterNotInExperience | ...
    pub status: Option<String>,
    pub place_id: Option<i64>,
    pub universe_id: Option<i64>,
    pub link_code: Option<String>,
    pub instance_id: Option<String>,
    pub launch_data: Option<String>,
    pub user_id: Option<i64>,
    /// ExperienceAffiliate / ExperienceEvent keep launch data here.
    pub join_data: Option<serde_json::Value>,
}

/// `deeplinks/v2/resolve`.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ShareLinkV2 {
    /// Experience | StudioEdit | ...
    pub link_type: Option<String>,
    pub status: Option<String>,
    pub target_id: Option<i64>,
    pub custom_data: Option<serde_json::Value>,
}

/// A row of `friends/v1/users/{id}/friends/statuses`. `status` is an int (0-3) in the spec but
/// strings have been seen; both are read by `FriendStatus::of`.
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct FriendStatusRow {
    pub id: i64,
    #[serde(default)]
    pub status: serde_json::Value,
}

/// How a stored account stands with another user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FriendStatus {
    NotFriends,
    Friends,
    /// This account asked the other one.
    RequestSent,
    /// The other one asked this account.
    RequestReceived,
}

impl FriendStatus {
    /// Unknown values count as not friends: the worst case is one request Roblox refuses.
    pub fn of(v: &serde_json::Value) -> FriendStatus {
        let n = match v {
            serde_json::Value::Number(n) => n.as_i64(),
            serde_json::Value::String(s) => ["NotFriends", "Friends", "RequestSent", "RequestReceived"]
                .iter()
                .position(|x| x.eq_ignore_ascii_case(s))
                .map(|i| i as i64),
            _ => None,
        };
        match n {
            Some(1) => FriendStatus::Friends,
            Some(2) => FriendStatus::RequestSent,
            Some(3) => FriendStatus::RequestReceived,
            _ => FriendStatus::NotFriends,
        }
    }
}

/// A row of `friends/v1/users/{id}/friends/online`.
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct FriendOnline {
    pub id: i64,
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "userPresence")]
    pub presence: Option<FriendPresence>,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub struct FriendPresence {
    /// "Online" | "InGame" | "InStudio" | ...
    pub user_presence_type: Option<String>,
    #[serde(rename = "lastLocation")]
    pub last_location: Option<String>,
    #[serde(rename = "placeId")]
    pub place_id: Option<i64>,
    #[serde(rename = "rootPlaceId")]
    pub root_place_id: Option<i64>,
    #[serde(rename = "gameInstanceId")]
    pub game_instance_id: Option<String>,
    #[serde(rename = "universeId")]
    pub universe_id: Option<i64>,
}

/// `games.roblox.com/v2/games/{placeId}/servers/Public`. `data` may be null.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ServerList {
    pub data: Option<Vec<ServerRow>>,
    pub next_page_cursor: Option<String>,
}

/// A row of the server list. Roblox may leave anything out; rows without id / maxPlayers /
/// playing are dropped by the caller.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ServerRow {
    pub id: Option<String>,
    pub max_players: Option<i32>,
    pub playing: Option<i32>,
    pub fps: Option<f64>,
    pub ping: Option<i64>,
    pub friend_count: Option<i32>,
}

/// `games.roblox.com/v1/games/{placeId}/private-servers` (EXPERIMENTAL; no signed-in recording
/// exists, so this follows the documented shape and accepts anything missing). No `Debug`:
/// rows carry access codes.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivateServerList {
    pub data: Option<Vec<PrivateServerRow>>,
    pub next_page_cursor: Option<String>,
    pub game_join_restricted: Option<bool>,
}

/// `GET games.roblox.com/v1/vip-servers/{id}` as its owner sees it. Unrecorded signed in, so
/// everything is optional. Its `joinCode` and `link` are join secrets and have no field here, so
/// they are never read; no `Debug` all the same.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VipServerResponse {
    pub name: Option<String>,
    pub active: Option<bool>,
    pub subscription: Option<VipSubscription>,
    pub permissions: Option<VipPermissions>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VipSubscription {
    /// Auto-renew is on.
    pub active: Option<bool>,
    pub expired: Option<bool>,
    pub expiration_date: Option<String>,
    pub price: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VipPermissions {
    pub friends_allowed: Option<bool>,
    pub users: Option<Vec<VipUser>>,
}

/// A player the owner allowed in. Rows without `id` are dropped by the caller.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VipUser {
    pub id: Option<i64>,
    pub name: Option<String>,
    pub display_name: Option<String>,
}

/// Rows without `vipServerId` are dropped by the caller. `accessCode` is a join secret.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivateServerRow {
    pub vip_server_id: Option<i64>,
    pub name: Option<String>,
    pub owner: Option<Party>,
    pub playing: Option<i32>,
    pub max_players: Option<i32>,
    /// The job id while it runs.
    pub id: Option<String>,
    pub access_code: Option<String>,
}
