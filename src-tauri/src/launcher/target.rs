//! Turns pasted text (game links, share links, roblox:// deep links, ids, usernames) into a
//! `ParsedInput`. Pure and local: nothing here touches the network.
//! The recorded cases: docs/research/fixtures/links-parse-cases.json.

use std::collections::HashMap;

use url::{Url, form_urlencoded};

use crate::model::{LaunchTarget, ParsedInput};

fn empty_why() -> String {
    tr!("Paste a game link, an ID or a username.")
}
fn not_roblox_why() -> String {
    tr!("That isn't a Roblox link.")
}
fn not_game_why() -> String {
    tr!("That link doesn't point to a game.")
}
fn place_why() -> String {
    tr!("That place ID isn't valid.")
}
fn user_why() -> String {
    tr!("That user ID isn't valid.")
}
fn universe_why() -> String {
    tr!("That game ID isn't valid.")
}
fn job_why() -> String {
    tr!("That server ID isn't valid.")
}
fn link_code_why() -> String {
    tr!("That private server link code isn't valid.")
}
fn access_code_why() -> String {
    tr!("That private server access code isn't valid.")
}
fn share_code_why() -> String {
    tr!("That share link's code isn't valid.")
}
fn share_type_missing_why() -> String {
    tr!("That share link is missing its type.")
}
fn share_type_why() -> String {
    tr!("That share link's type isn't known.")
}
fn username_why() -> String {
    tr!("That username isn't valid.")
}
fn launch_uri_why() -> String {
    tr!("Launch links carry a sign-in ticket and aren't accepted. Paste the game link instead.")
}
fn short_link_why() -> String {
    tr!("Open that ro.blox.com link in your browser, then paste the roblox.com link it goes to.")
}
fn nested_why() -> String {
    tr!("That ro.blox.com link wraps another ro.blox.com link.")
}

const WEB_HOSTS: [&str; 4] = ["roblox.com", "www.roblox.com", "web.roblox.com", "m.roblox.com"];
/// The web's share-link types; `true` = it can lead into a game.
const SHARE_TYPES: [(&str, bool); 12] = [
    ("Server", true),
    ("ExperienceInvite", true),
    ("NotificationExperienceInvite", true),
    ("ScreenshotInvite", true),
    ("ExperienceDetails", true),
    ("ExperienceAffiliate", true),
    ("ExperienceEvent", true),
    ("Profile", true),
    ("FriendInvite", false),
    ("AvatarItemDetails", false),
    ("AvatarItemAffiliate", false),
    ("ContentPost", false),
];
/// Lower-cased keys; the first non-empty value wins.
type Query = HashMap<String, String>;

/// Never panics; anything unusable is `Invalid` / `Unsupported` with a short user-facing reason.
pub fn parse(input: &str) -> ParsedInput {
    parse_at(input, 0)
}

fn parse_at(input: &str, depth: u8) -> ParsedInput {
    let s = input.trim().trim_matches(['<', '>', '"', '\'']).trim();
    if s.is_empty() {
        return invalid(empty_why());
    }
    if s.bytes().all(|b| b.is_ascii_digit()) {
        return id(s).map_or_else(|| invalid(place_why()), |place_id| place(place_id, None));
    }
    if is_uuid(s) {
        return ParsedInput::JobIdOnly { job_id: s.to_ascii_lowercase() };
    }
    if let Some(name) = s.strip_prefix('@') {
        return if is_username(name) { username(name) } else { invalid(username_why()) };
    }
    if is_username(s) {
        return username(s);
    }
    let Some((scheme, rest)) = s.split_once(':').filter(|(x, _)| is_scheme(x)) else {
        return web(&format!("https://{s}"), depth);
    };
    match scheme.to_ascii_lowercase().as_str() {
        "roblox" if rest.starts_with("//") => deeplink(&rest[2..]),
        "roblox-player" | "roblox-studio" | "roblox-studio-auth" => invalid(launch_uri_why()),
        "http" | "https" => web(s, depth),
        _ => invalid(not_roblox_why()),
    }
}

/// http(s) links; hosts are read the WHATWG way (`evil.com\@www.roblox.com` is evil.com).
fn web(s: &str, depth: u8) -> ParsedInput {
    let Ok(u) = Url::parse(s) else { return invalid(not_roblox_why()) };
    let host = u.host_str().unwrap_or("").trim_end_matches('.').to_ascii_lowercase();
    let q = query(u.query().unwrap_or(""));
    if host == "ro.blox.com" {
        return apps_flyer(&q, depth);
    }
    if !WEB_HOSTS.contains(&host.as_str()) {
        return invalid(not_roblox_why());
    }
    let low: Vec<String> = u.path().split('/').filter(|x| !x.is_empty()).map(str::to_ascii_lowercase).collect();
    let mut seg: Vec<&str> = low.iter().map(String::as_str).collect();
    if seg.len() >= 2 && is_locale(seg[0]) && ["games", "share", "share-links", "users"].contains(&seg[1]) {
        seg.remove(0);
    }
    match seg.as_slice() {
        ["games", "start", ..] => from_query(&q),
        ["games", p, ..] => id(p).map_or_else(|| invalid(place_why()), |place_id| with_place(place_id, &q, false)),
        ["share"] | ["share-links"] => share(&q),
        ["users", u, ..] => id(u).map_or_else(|| invalid(user_why()), follow),
        ["placeitem.aspx"] => q.get("id").and_then(|p| id(p)).map_or_else(|| invalid(place_why()), |p| place(p, None)),
        _ => unsupported(not_game_why()),
    }
}

/// ro.blox.com (AppsFlyer): the real link is in the query; short links are never fetched.
fn apps_flyer(q: &Query, depth: u8) -> ParsedInput {
    if depth > 0 {
        return invalid(nested_why());
    }
    match get(q, &["af_dp", "deep_link_value", "af_web_dp"]) {
        Some(inner) => parse_at(inner, depth + 1),
        None => unsupported(short_link_why()),
    }
}

/// `rest` = what follows `roblox://`. The legacy form `roblox://placeId=1&..` has no `?`.
fn deeplink(rest: &str) -> ParsedInput {
    let (mut path, mut raw) = rest.split_once('?').unwrap_or((rest, ""));
    if raw.is_empty() && path.contains('=') {
        (path, raw) = ("", path.trim_end_matches('/'));
    }
    let q = query(raw);
    match path.trim_matches('/').to_ascii_lowercase().as_str() {
        "" | "experiences/start" => from_query(&q),
        "navigation/share_links" => share(&q),
        "navigation/game_details" => q
            .get("gameid")
            .and_then(|g| id(g))
            .map_or_else(|| invalid(universe_why()), |universe_id| target(LaunchTarget::Universe { universe_id })),
        "navigation/profile" => q.get("userid").and_then(|u| id(u)).map_or_else(|| invalid(user_why()), follow),
        _ => unsupported(not_game_why()),
    }
}

/// `/games/start` and the roblox:// launch forms.
fn from_query(q: &Query) -> ParsedInput {
    match get(q, &["placeid", "id"]) {
        Some(p) => id(p).map_or_else(|| invalid(place_why()), |place_id| with_place(place_id, q, true)),
        None if q.contains_key("userid") => q.get("userid").and_then(|u| id(u)).map_or_else(|| invalid(user_why()), follow),
        None => invalid(place_why()),
    }
}

/// Link code, then access code, then job id, as the website's own handler. A `/games/{id}` page
/// (`start == false`) takes neither an access code nor launch data.
fn with_place(place_id: i64, q: &Query, start: bool) -> ParsedInput {
    if let Some(c) = get(q, &["linkcode", "privateserverlinkcode"]) {
        if !is_digits(c, 64) {
            return invalid(link_code_why());
        }
        return target(LaunchTarget::PrivateLink { place_id, link_code: c.to_owned() });
    }
    if start && let Some(a) = q.get("accesscode") {
        if !is_uuid(a) {
            return invalid(access_code_why());
        }
        return target(LaunchTarget::PrivateAccess { place_id, access_code: a.to_ascii_lowercase() });
    }
    if let Some(j) = q.get("gameinstanceid") {
        if !is_uuid(j) {
            return invalid(job_why());
        }
        return target(LaunchTarget::Server { place_id, job_id: j.to_ascii_lowercase() });
    }
    // Over-long data is kept here; resolve drops it at launch and says so.
    let launch_data = q.get("launchdata").filter(|_| start);
    place(place_id, launch_data.cloned())
}

fn share(q: &Query) -> ParsedInput {
    let Some(code) = q.get("code").filter(|c| is_code(c)) else { return invalid(share_code_why()) };
    if q.get("v").is_some_and(|v| v.eq_ignore_ascii_case("v2")) {
        return target(LaunchTarget::ShareLinkV2 { code: code.clone() });
    }
    let Some(t) = q.get("type") else { return invalid(share_type_missing_why()) };
    match SHARE_TYPES.iter().find(|(name, _)| name.eq_ignore_ascii_case(t)) {
        None => invalid(share_type_why()),
        Some((_, false)) => unsupported(not_game_why()),
        Some((name, true)) => target(LaunchTarget::ShareLink { code: code.clone(), link_type: (*name).to_owned() }),
    }
}

/// Values are percent-decoded once. `+` stays a literal plus, as in Roblox's own parser (no
/// form decoding). Empty values count as absent.
fn query(raw: &str) -> Query {
    let mut q = Query::new();
    for (k, v) in form_urlencoded::parse(raw.replace('+', "%2B").as_bytes()) {
        if !v.is_empty() {
            q.entry(k.to_lowercase()).or_insert_with(|| v.into_owned());
        }
    }
    q
}

fn get<'a>(q: &'a Query, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|k| q.get(*k)).map(String::as_str)
}

/// A positive id that fits an i64, digits only.
fn id(s: &str) -> Option<i64> {
    if !is_digits(s, 19) {
        return None;
    }
    s.parse().ok().filter(|n| *n > 0)
}

fn is_digits(s: &str, max: usize) -> bool {
    (1..=max).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit())
}

pub(crate) fn is_uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        })
}

fn is_code(s: &str) -> bool {
    (1..=128).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn is_username(s: &str) -> bool {
    (3..=20).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn is_scheme(s: &str) -> bool {
    s.bytes().next().is_some_and(|b| b.is_ascii_alphabetic())
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"+-.".contains(&b))
}

/// `xx` or `xx-yyyy` (already lower-cased).
fn is_locale(s: &str) -> bool {
    let (lang, region) = match s.split_once('-') {
        Some((l, r)) => (l, Some(r)),
        None => (s, None),
    };
    lang.len() == 2
        && lang.bytes().all(|b| b.is_ascii_alphabetic())
        && region.is_none_or(|r| (2..=4).contains(&r.len()) && r.bytes().all(|b| b.is_ascii_alphanumeric()))
}

fn target(target: LaunchTarget) -> ParsedInput {
    ParsedInput::Target { target }
}

fn place(place_id: i64, launch_data: Option<String>) -> ParsedInput {
    target(LaunchTarget::Place { place_id, launch_data })
}

fn follow(user_id: i64) -> ParsedInput {
    target(LaunchTarget::FollowUser { user_id })
}

fn username(name: &str) -> ParsedInput {
    ParsedInput::Username { name: name.to_owned() }
}

fn invalid(reason: String) -> ParsedInput {
    ParsedInput::Invalid { reason }
}

fn unsupported(reason: String) -> ParsedInput {
    ParsedInput::Unsupported { reason }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    /// The fixture's short reason codes, as the user-facing text.
    fn reason(code: &str) -> String {
        match code {
            "empty" => empty_why(),
            "host" => not_roblox_why(),
            "placeId" => place_why(),
            "linkCode" => link_code_why(),
            "jobId" => job_why(),
            "share type missing" => share_type_missing_why(),
            "not a game link" => not_game_why(),
            "AppsFlyer short link without af_dp/deep_link_value/af_web_dp" => short_link_why(),
            "launch URI (carries auth ticket) not accepted as input" => launch_uri_why(),
            other => panic!("unmapped reason {other}"),
        }
    }

    fn expected(v: &Value) -> ParsedInput {
        let n = |k: &str| v[k].as_i64().unwrap();
        let s = |k: &str| v[k].as_str().unwrap().to_owned();
        match v["kind"].as_str().unwrap() {
            "Place" => place(n("placeId"), v["launchData"].as_str().map(Into::into)),
            "Instance" => target(LaunchTarget::Server { place_id: n("placeId"), job_id: s("jobId") }),
            "PrivateServerLinkCode" => {
                target(LaunchTarget::PrivateLink { place_id: n("placeId"), link_code: s("linkCode") })
            }
            "PrivateServerAccessCode" => {
                target(LaunchTarget::PrivateAccess { place_id: n("placeId"), access_code: s("accessCode") })
            }
            "ShareLink" => target(LaunchTarget::ShareLink { code: s("code"), link_type: s("linkType") }),
            "ShareLinkV2" => target(LaunchTarget::ShareLinkV2 { code: s("code") }),
            "Universe" => target(LaunchTarget::Universe { universe_id: n("universeId") }),
            "FollowUser" | "User" => follow(n("userId")),
            "JobIdOnly" => ParsedInput::JobIdOnly { job_id: s("jobId") },
            "Invalid" => invalid(reason(&s("reason"))),
            "Unsupported" => unsupported(reason(&s("reason"))),
            other => panic!("unknown kind {other}"),
        }
    }

    #[test]
    fn recorded_cases() {
        let path = format!("{}/../docs/research/fixtures/links-parse-cases.json", env!("CARGO_MANIFEST_DIR"));
        let v: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let cases = v["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 38);
        for c in cases {
            let input = c["input"].as_str().unwrap();
            assert_eq!(parse(input), expected(&c["expected"]), "{input}");
        }
    }

    fn data(input: &str) -> Option<String> {
        match parse(input) {
            ParsedInput::Target { target: LaunchTarget::Place { launch_data, .. } } => launch_data,
            other => panic!("{input}: {other:?}"),
        }
    }

    #[test]
    fn launch_data_is_decoded_once_and_plus_is_literal() {
        assert_eq!(data("roblox://placeId=1&launchData=a+b").as_deref(), Some("a+b"));
        let web = "https://www.roblox.com/games/start?placeId=1&launchData=a+b%2Bc%20d";
        assert_eq!(data(web).as_deref(), Some("a+b+c d"));
        assert_eq!(data("roblox://placeId=1&launchData=a%2520b").as_deref(), Some("a%20b"));
        assert_eq!(data("roblox.com/games/start?placeId=1&launchData=a:b").as_deref(), Some("a:b"));
    }

    #[test]
    fn long_launch_data_is_kept_for_resolve_to_drop() {
        let at = |d: &str| data(&format!("roblox://placeId=1&launchData={d}"));
        assert_eq!(at(&"a".repeat(201)), Some("a".repeat(201)));
        assert_eq!(at(&"%41".repeat(200)), Some("A".repeat(200)));
    }

    #[test]
    fn verifier_fixes() {
        let link = |place_id, c: &str| target(LaunchTarget::PrivateLink { place_id, link_code: c.into() });
        let cases = [
            // Empty values count as absent.
            ("roblox://experiences/start?placeId=1&accessCode=&gameInstanceId=", place(1, None)),
            ("https://www.roblox.com/games/1/x?privateServerLinkCode=&gameInstanceId=", place(1, None)),
            ("roblox://experiences/start?placeId=&id=5", place(5, None)),
            ("https://www.roblox.com/games/start?placeId=&userId=156", follow(156)),
            // WHATWG hosts.
            ("https://evil.com\\@www.roblox.com/games/1", invalid(not_roblox_why())),
            ("https://www.roblox.com@evil.com/games/1", invalid(not_roblox_why())),
            ("https://WWW.ROBLOX.COM./games/1", place(1, None)),
            ("https://www.roblox.com:443/games/1", place(1, None)),
            // Legacy place page.
            ("https://www.roblox.com/PlaceItem.aspx?id=920587237", place(920587237, None)),
            ("https://www.roblox.com/PlaceItem.aspx?id=x", invalid(place_why())),
            // A linkCode on a game page is kept, not dropped.
            ("https://www.roblox.com/games/1/x?linkCode=123", link(1, "123")),
            ("https://www.roblox.com/games/1/x?linkCode=12a", invalid(link_code_why())),
            ("https://www.roblox.com/games/1/x?accessCode=8d5f1b3e-6a0c-4f7e-9b2d-1c3e5a7f9b0d", place(1, None)),
        ];
        for (input, want) in cases {
            assert_eq!(parse(input), want, "{input}");
        }
    }

    #[test]
    fn other_forms() {
        let cases = [
            ("0", invalid(place_why())),
            ("9223372036854775807", place(i64::MAX, None)),
            ("9223372036854775808", invalid(place_why())),
            ("https://www.roblox.com/en-us/games/1", place(1, None)),
            ("roblox://navigation/profile?userId=156", follow(156)),
            ("roblox://navigation/game_details", invalid(universe_why())),
            ("roblox://navigation/home", unsupported(not_game_why())),
            ("roblox:placeId=1", invalid(not_roblox_why())),
            ("ROBLOX-STUDIO:1+launchmode:edit", invalid(launch_uri_why())),
            ("ftp://www.roblox.com/games/1", invalid(not_roblox_why())),
            ("roblox://experiences/start?placeId=1&accessCode=nope", invalid(access_code_why())),
            ("https://www.roblox.com/share?code=a&type=Bogus", invalid(share_type_why())),
            ("https://www.roblox.com/share?code=a!b&type=Server", invalid(share_code_why())),
            ("https://www.roblox.com/users/x/profile", invalid(user_why())),
            ("https://ro.blox.com/Ebh5?af_dp=https%3A%2F%2Fro.blox.com%2FEbh5%3Faf_dp%3D1", invalid(nested_why())),
            ("https://ro.blox.com/Ebh5?af_dp=&deep_link_value=roblox%3A%2F%2FplaceId%3D7", place(7, None)),
        ];
        for (input, want) in cases {
            assert_eq!(parse(input), want, "{input}");
        }
    }

    #[test]
    fn usernames() {
        let name = |n: &str| ParsedInput::Username { name: n.into() };
        let cases = [
            ("@builderman", name("builderman")),
            ("  builderman ", name("builderman")),
            ("Roblox_Dev1", name("Roblox_Dev1")),
            ("@1234", name("1234")),
            ("abc", name("abc")),
            ("@ab", invalid(username_why())),
            ("@builder-man", invalid(username_why())),
            ("@", invalid(username_why())),
            ("123456", place(123456, None)),
            ("ab", invalid(not_roblox_why())),
            ("abcdefghijklmnopqrstu", invalid(not_roblox_why())),
        ];
        for (input, want) in cases {
            assert_eq!(parse(input), want, "{input}");
        }
    }
}
