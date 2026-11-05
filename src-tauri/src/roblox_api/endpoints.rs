//! Every Roblox URL in one place (verified 2026-10-24, see docs/DESIGN.md).

pub const USERS_AUTHENTICATED: &str = "https://users.roblox.com/v1/users/authenticated";
pub const LOGIN_PAGE: &str = "https://www.roblox.com/login";
pub const WWW: &str = "https://www.roblox.com/";
/// `Origin` the website sends to auth.roblox.com and apis.roblox.com (`Referer` is `WWW`).
pub const WEB_ORIGIN: &str = "https://www.roblox.com";

// Account info. `{}` is filled by `fill`; each template is also its endpoint's throttle key.
pub const USER: &str = "https://users.roblox.com/v1/users/{}";
pub const HEADSHOTS: &str =
    "https://thumbnails.roblox.com/v1/users/avatar-headshot?userIds={}&size=150x150&format=Png&isCircular=false";
pub const CURRENCY: &str = "https://economy.roblox.com/v1/user/currency";
pub const PREMIUM: &str = "https://premiumfeatures.roblox.com/v1/users/{}/validate-membership";
pub const PROFILES: &str = "https://apis.roblox.com/user-profile-api/v1/user/profiles/get-profiles";
pub const FRIENDS_COUNT: &str = "https://friends.roblox.com/v1/users/{}/friends/count";
pub const FOLLOWERS_COUNT: &str = "https://friends.roblox.com/v1/users/{}/followers/count";
pub const FOLLOWINGS_COUNT: &str = "https://friends.roblox.com/v1/users/{}/followings/count";
pub const PRESENCE: &str = "https://presence.roblox.com/v1/presence/users";
pub const EMAIL: &str = "https://accountsettings.roblox.com/v1/email";
pub const TWO_STEP: &str = "https://twostepverification.roblox.com/v1/users/{}/configuration";

// Launching. Trailing slashes as the website sends them.
pub const CLIENT_ASSERTION: &str = "https://auth.roblox.com/v1/client-assertion/";
pub const AUTH_TICKET: &str = "https://auth.roblox.com/v1/authentication-ticket/";
pub const SHARE_LINK: &str = "https://apis.roblox.com/sharelinks/v1/resolve-link";
pub const SHARE_LINK_V2: &str = "https://apis.roblox.com/deeplinks/v2/resolve";
pub const GAMES: &str = "https://games.roblox.com/v1/games?universeIds={}";
pub const USERS_BY_NAME: &str = "https://users.roblox.com/v1/usernames/users";
pub const FRIENDS_ONLINE: &str = "https://friends.roblox.com/v1/users/{}/friends/online";

// Servers (query string appended by the caller).
pub const SERVERS_PUBLIC: &str = "https://games.roblox.com/v2/games/{}/servers/Public";
pub const SERVERS_PRIVATE: &str = "https://games.roblox.com/v1/games/{}/private-servers";
pub const PLACE_UNIVERSE: &str = "https://apis.roblox.com/universes/v1/places/{}/universe";
pub const PLACE_ICONS: &str =
    "https://thumbnails.roblox.com/v1/places/gameicons?placeIds={}&returnPolicy=PlaceHolder&size=150x150&format=Png&isCircular=false";

/// `template` with its `{}` replaced by `value`.
pub fn fill(template: &str, value: impl std::fmt::Display) -> String {
    template.replacen("{}", &value.to_string(), 1)
}

// Roblox client updates (no cookie). The installer URL is `{SETUP}{clientVersionUpload}-RobloxPlayerInstaller.exe`.
pub const CLIENT_VERSION: &str = "https://clientsettingscdn.roblox.com/v2/client-version/WindowsPlayer";
#[allow(dead_code)]
pub const SETUP: &str = "https://setup.rbxcdn.com/";
