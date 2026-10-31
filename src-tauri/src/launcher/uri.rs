//! The `roblox-player:` URI and PlaceLauncher URL, byte-for-byte as roblox.com builds them
//! (docs/research/fixtures/authlaunch-web-launch-uri-spec.json).

use std::fmt::Write;

use uuid::Uuid;

use super::Join;
use crate::vault::Secret;

pub const PLACE_LAUNCHER: &str = "https://www.roblox.com/Game/PlaceLauncher.ashx";

pub struct PlaceLaunch<'a> {
    pub join: &'a Join,
    pub browser_tracker_id: i64,
    pub join_attempt_id: Uuid,
}

/// `https://www.roblox.com/Game/PlaceLauncher.ashx?request=<T>&browserTrackerId=<id>&...`
/// with the website's parameter order; keys and values encodeURIComponent-encoded, `%20` → `+`.
/// Game: placeId, [launchData], isPlayTogetherGame=false, joinAttemptId, joinAttemptOrigin=PlayButton.
/// Job: placeId, gameId, isPlayTogetherGame=false, joinAttemptId, joinAttemptOrigin=JoinInstance.
/// Private: placeId, [accessCode], [linkCode], joinAttemptId, joinAttemptOrigin=PlayButton.
/// Follow: userId, joinAttemptId, joinAttemptOrigin=JoinUser.
pub fn place_launcher_url(p: &PlaceLaunch) -> String {
    let some = |k, v: &Option<String>| v.as_deref().filter(|v| !v.is_empty()).map(|v| (k, v.to_owned()));
    let (request, mut params, origin) = match p.join {
        Join::Game { place_id, launch_data } => {
            let mut v = vec![("placeId", place_id.to_string())];
            v.extend(some("launchData", launch_data));
            v.push(("isPlayTogetherGame", "false".into()));
            ("RequestGame", v, "PlayButton")
        }
        Join::Job { place_id, job_id } => {
            let v = vec![("placeId", place_id.to_string()), ("gameId", job_id.clone()), ("isPlayTogetherGame", "false".into())];
            ("RequestGameJob", v, "JoinInstance")
        }
        Join::Private { place_id, link_code, access_code } => {
            let mut v = vec![("placeId", place_id.to_string())];
            v.extend(some("accessCode", access_code));
            v.extend(some("linkCode", link_code));
            ("RequestPrivateGame", v, "PlayButton")
        }
        Join::Follow { user_id } => ("RequestFollowUser", vec![("userId", user_id.to_string())], "JoinUser"),
    };
    params.push(("joinAttemptId", p.join_attempt_id.to_string()));
    params.push(("joinAttemptOrigin", origin.into()));
    let mut url = format!("{PLACE_LAUNCHER}?request={request}&browserTrackerId={}", p.browser_tracker_id);
    for (k, v) in params {
        let _ = write!(url, "&{}={}", encode(k), encode(&v).replace("%20", "+"));
    }
    url
}

/// `roblox-player:1+launchmode:play+gameinfo:<enc ticket>+launchtime:<ms>+placelauncherurl:<enc url>+
/// browsertrackerid:<id>+robloxLocale:en_us+gameLocale:en_us+LaunchExp:InApp`. `production` adds an
/// empty `channel:` before `LaunchExp`: the client then asks for production's version, whatever
/// channel Roblox left in the registry (research multi-instance M9). The result holds the ticket,
/// so it is a `Secret`.
pub fn player_uri(ticket: &Secret, launch_time_ms: i64, p: &PlaceLaunch, production: bool) -> Secret {
    let url = place_launcher_url(p);
    // Sized up front so no reallocation leaves a copy of the ticket behind.
    let mut s = String::with_capacity(3 * (ticket.expose().len() + url.len()) + 256);
    s.push_str("roblox-player:1+launchmode:play+gameinfo:");
    encode_into(&mut s, ticket.expose());
    let _ = write!(s, "+launchtime:{launch_time_ms}+placelauncherurl:");
    encode_into(&mut s, &url);
    let channel = if production { "channel:+" } else { "" };
    let _ = write!(s, "+browsertrackerid:{}+robloxLocale:en_us+gameLocale:en_us+{channel}LaunchExp:InApp", p.browser_tracker_id);
    Secret::new(s)
}

/// The engine's own mobile game-link form, `roblox://experiences/start?placeId=…`, carrying a
/// server when the target names one. Cordial passes this shape straight to the engine; the
/// desktop `roblox-player:` form has to be translated first and is more fragile. None for a
/// follow-user target, which has no place to name.
pub fn mobile_join_url(join: &Join) -> Option<String> {
    let place = join.place_id()?;
    let mut s = format!("roblox://experiences/start?placeId={place}");
    match join {
        Join::Game { launch_data, .. } => {
            if let Some(data) = launch_data.as_deref().filter(|d| !d.is_empty()) {
                let _ = write!(s, "&launchData={}", encode(data));
            }
        }
        Join::Job { job_id, .. } => {
            let _ = write!(s, "&gameInstanceId={}", encode(job_id));
        }
        Join::Private { link_code, access_code, .. } => {
            if let Some(code) = access_code.as_deref().filter(|c| !c.is_empty()) {
                let _ = write!(s, "&accessCode={}", encode(code));
            }
            if let Some(code) = link_code.as_deref().filter(|c| !c.is_empty()) {
                let _ = write!(s, "&linkCode={}", encode(code));
            }
        }
        Join::Follow { .. } => return None,
    }
    Some(s)
}

/// A random 16-digit browser tracker id, unique per launch (the client log names it, which is
/// how a client is tied back to its account). Kept at or below 2^53 - 1 so JavaScript holds it exactly.
pub fn new_browser_tracker_id() -> i64 {
    const LOW: i64 = 1_000_000_000_000_000;
    LOW + (Uuid::new_v4().as_u128() % (8 * LOW as u128)) as i64
}

/// JavaScript's `encodeURIComponent`.
pub(crate) fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    encode_into(&mut out, s);
    out
}

fn encode_into(out: &mut String, s: &str) {
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    fn launch(join: &Join) -> PlaceLaunch<'_> {
        PlaceLaunch {
            join,
            browser_tracker_id: 1234567890,
            join_attempt_id: Uuid::parse_str("11111111-2222-4333-8444-555555555555").unwrap(),
        }
    }

    #[test]
    fn website_examples() {
        let path = format!("{}/../docs/research/fixtures/authlaunch-web-launch-uri-spec.json", env!("CARGO_MANIFEST_DIR"));
        let spec: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let joins = [
            ("RequestGame", Join::Game { place_id: 1818, launch_data: None }),
            ("RequestGameJob", Join::Job { place_id: 1818, job_id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".into() }),
            (
                "RequestPrivateGame_linkCode",
                Join::Private { place_id: 1818, link_code: Some("12345678901234567890123456789012".into()), access_code: None },
            ),
            ("RequestFollowUser", Join::Follow { user_id: 1 }),
        ];
        assert_eq!(spec["examples"].as_object().unwrap().len(), joins.len());
        let ticket = Secret::new("<TICKET>");
        for (name, join) in &joins {
            let uri = player_uri(&ticket, 1792839000000, &launch(join), false);
            assert_eq!(uri.expose(), spec["examples"][name].as_str().unwrap(), "{name}");
            let production = player_uri(&ticket, 1792839000000, &launch(join), true);
            assert_eq!(production.expose(), uri.expose().replace("+LaunchExp:", "+channel:+LaunchExp:"), "{name}");
        }
    }

    #[test]
    fn encodes_like_the_website() {
        let join = Join::Game { place_id: 1, launch_data: Some("a b+c&d=e/é!~*'()".into()) };
        let url = place_launcher_url(&launch(&join));
        assert!(url.contains("&placeId=1&launchData=a+b%2Bc%26d%3De%2F%C3%A9!~*'()&isPlayTogetherGame=false&"), "{url}");
        let uri = player_uri(&Secret::new("a+b c"), 0, &launch(&join), false);
        assert!(uri.expose().contains("+gameinfo:a%2Bb%20c+launchtime:0+"));
        assert!(uri.expose().contains("launchData%3Da%2Bb%252Bc%2526d"));
    }

    #[test]
    fn private_access_code_comes_before_link_code_and_empty_ones_are_dropped() {
        let both = Join::Private { place_id: 1, link_code: Some("2".into()), access_code: Some("x".into()) };
        assert!(place_launcher_url(&launch(&both)).contains("placeId=1&accessCode=x&linkCode=2&joinAttemptId="));
        let empty = Join::Private { place_id: 1, link_code: Some(String::new()), access_code: None };
        assert!(place_launcher_url(&launch(&empty)).contains("placeId=1&joinAttemptId="));
    }

    #[test]
    fn mobile_links_carry_the_server() {
        let job = Join::Job { place_id: 1, job_id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".into() };
        assert_eq!(
            mobile_join_url(&job).as_deref(),
            Some("roblox://experiences/start?placeId=1&gameInstanceId=aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
        );
        let private = Join::Private { place_id: 2, link_code: Some("123".into()), access_code: None };
        assert_eq!(mobile_join_url(&private).as_deref(), Some("roblox://experiences/start?placeId=2&linkCode=123"));
        assert_eq!(mobile_join_url(&Join::Follow { user_id: 1 }), None);
    }

    #[test]
    fn browser_tracker_ids_are_16_random_digits() {
        let ids: Vec<i64> = (0..1000).map(|_| new_browser_tracker_id()).collect();
        assert!(ids.iter().all(|id| (1_000_000_000_000_000..=8_999_999_999_999_999).contains(id)));
        assert!(ids.iter().all(|id| id.to_string().len() == 16));
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), ids.len());
    }
}
