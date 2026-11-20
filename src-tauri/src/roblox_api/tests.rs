use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;

use reqwest::header::HeaderName;

use super::*;
use crate::launcher::{Join, resolve};
use crate::model::{GameServer, LaunchTarget, ServerOrder, ServerPage};

fn fixture(name: &str) -> String {
    let path = format!("{}/../docs/research/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap()
}

/// The recorded `body` of a JSON fixture, as it came over the wire.
fn fixture_body(name: &str) -> Vec<u8> {
    let v: Value = serde_json::from_str(&fixture(name)).unwrap();
    serde_json::to_vec(&v["body"]).unwrap()
}

/// A `.headers.txt` capture (status line, then `name: value` lines).
fn fixture_headers(name: &str) -> HeaderMap {
    headers(&fixture(name).lines().filter_map(|l| l.split_once(": ")).collect::<Vec<_>>())
}

fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
    let mut h = HeaderMap::new();
    for (k, v) in pairs {
        h.append(HeaderName::from_bytes(k.as_bytes()).unwrap(), HeaderValue::from_str(v).unwrap());
    }
    h
}

#[test]
fn rate_limit_headers() {
    let ok = RateLimit::parse(&fixture_headers("acctinfo-users-authenticated-nocookie.headers.txt"));
    assert_eq!(ok, RateLimit { window: Some(60), remaining: Some(99), reset: Some(32), retry_after: None });
    let limited = RateLimit::parse(&fixture_headers("acctinfo-ratelimit-429-friends-list.headers.txt"));
    assert_eq!(limited, RateLimit { window: Some(60), remaining: Some(0), reset: Some(30), retry_after: Some(5) });
    let servers = RateLimit::parse(&fixture_headers("servers-v1-429.headers.txt"));
    assert_eq!((servers.remaining, servers.reset_secs()), (Some(0), Some(50)));

    // No reset header: the next clock-aligned window boundary.
    let hourly = RateLimit::parse(&headers(&[("x-ratelimit-limit", "750, 750;w=3600"), ("x-ratelimit-remaining", "749")]));
    assert_eq!(hourly.window, Some(3600));
    assert!((1..=3600).contains(&hourly.reset_secs().unwrap()));

    assert_eq!(RateLimit::parse(&headers(&[("x-ratelimit-limit", "5, 5;w=1, 100;w=60")])).window, Some(1));
    assert_eq!(RateLimit::parse(&headers(&[("retry-after", "Wed, 21 Oct 2026 07:28:00 GMT")])).retry_after, None);
    assert_eq!(RateLimit::parse(&HeaderMap::new()), RateLimit::default());
    assert_eq!(RateLimit::default().reset_secs(), None);
}

#[test]
fn set_cookie_rotation() {
    let new = ".ROBLOSECURITY=_|WARNING:-DO-NOT-SHARE-THIS.--x|_NEW; domain=.roblox.com; \
               expires=Mon, 24-Oct-2061 10:00:00 GMT; path=/; secure; HttpOnly";
    let h = headers(&[
        ("set-cookie", "__cf_bm=abc; HttpOnly; Path=/; Domain=roblox.com"),
        ("set-cookie", ".ROBLOSECURITY=; domain=.roblox.com; expires=Thu, 01-Jan-1970 00:00:00 GMT; path=/"),
        ("set-cookie", new),
        ("set-cookie", "rbx-ip2=1; domain=roblox.com; path=/"),
    ]);
    assert_eq!(rotated_cookie(&h).unwrap().expose(), "_|WARNING:-DO-NOT-SHARE-THIS.--x|_NEW");
    assert_eq!(rotated_cookie(&headers(&[("set-cookie", ".ROBLOSECURITY=_|WARNING:-DO-NOT-SHARE-THIS.PLAIN")])).unwrap().expose(), "_|WARNING:-DO-NOT-SHARE-THIS.PLAIN");

    for gone in [
        ".ROBLOSECURITY=; path=/",
        ".ROBLOSECURITY=deleted; Max-Age=0",
        ".ROBLOSECURITY=old; expires=Thu, 01 Jan 1970 00:00:00 GMT",
        "x.ROBLOSECURITY=other",
        "GuestData=UserID=-1; domain=.roblox.com",
        ".ROBLOSECURITY=deleted; expires=Fri, 31 Dec 1999 23:59:59 GMT",
        ".ROBLOSECURITY=x",
    ] {
        assert!(rotated_cookie(&headers(&[("set-cookie", gone)])).is_none(), "{gone}");
    }
    assert!(rotated_cookie(&HeaderMap::new()).is_none());

    // The site cookies Roblox sets on an anonymous www request are no rotation.
    let v: Value = serde_json::from_str(&fixture("authlaunch-www-xsrftoken-get.json")).unwrap();
    let set: Vec<(&str, &str)> =
        v["headers"]["set-cookie"].as_array().unwrap().iter().map(|c| ("set-cookie", c.as_str().unwrap())).collect();
    assert_eq!(set.len(), 3);
    assert!(rotated_cookie(&headers(&set)).is_none());
}

#[test]
fn error_classification() {
    let none = HeaderMap::new();
    let missing = fixture_body("acctinfo-users-authenticated-nocookie.json");
    assert_eq!(error_code(&missing), Some((9002, 0)));
    assert!(matches!(classify(StatusCode::UNAUTHORIZED, &none, &missing), AppError::CookieInvalid));
    let ticket = fixture_body("authlaunch-auth-ticket-nocookie-nocsrf.json");
    assert!(matches!(classify(StatusCode::UNAUTHORIZED, &none, &ticket), AppError::CookieInvalid));

    let bound = br#"{"errors":[{"code":9002,"subcode":2,"message":"Unauthorized"}]}"#;
    assert!(matches!(classify(StatusCode::UNAUTHORIZED, &none, bound), AppError::BoundSessionRequired));

    // Bare string, {Error: ..} and empty bodies: no code, still a plain 401.
    let bare = fixture_body("links-sharelinks-resolve-server-unauth-401.json");
    for body in [&bare[..], br#"{"Error":"Unauthorized"}"#, b"", b"<html>"] {
        assert_eq!(error_code(body), None);
        assert!(matches!(classify(StatusCode::UNAUTHORIZED, &none, body), AppError::CookieInvalid));
    }

    // auth.roblox.com's flat CSRF body; a 403 that reaches classify is a plain error.
    let csrf = fixture_body("authlaunch-csrf-login-v2-nocsrf.json");
    assert_eq!(error_code(&csrf), Some((0, 0)));
    assert!(matches!(classify(StatusCode::FORBIDDEN, &none, &csrf), AppError::Roblox(m) if m == "HTTP 403"));
    let challenge = headers(&[("rblx-challenge-id", "abc"), ("rblx-challenge-type", "captcha")]);
    assert!(matches!(classify(StatusCode::FORBIDDEN, &challenge, b"{}"), AppError::Challenge));

    let gone = fixture_body("acctinfo-presence-last-online-nocookie.json");
    assert!(matches!(classify(StatusCode::NOT_FOUND, &none, &gone), AppError::Roblox(m) if m == "HTTP 404"));
}

// End to end against a canned HTTP server on 127.0.0.1.

const USER: &str = r#"{"id":1,"name":"roblox","displayName":"Roblox","hasVerifiedBadge":true}"#;
const TICKET: &str = "https://auth.roblox.com/v1/authentication-ticket";

fn reply(status: &str, headers: &[&str], body: &str) -> String {
    let extra: String = headers.iter().map(|h| format!("{h}\r\n")).collect();
    format!(
        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n{extra}\r\n{body}",
        body.len()
    )
}

/// Answers one connection per response, in order, and passes on each raw request.
fn serve(responses: Vec<String>) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for resp in responses {
            let Ok((mut s, _)) = listener.accept() else { return };
            let _ = tx.send(read_request(&mut s));
            let _ = s.write_all(resp.as_bytes());
        }
    });
    (base, rx)
}

fn read_request(s: &mut TcpStream) -> String {
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let (mut buf, mut chunk) = (Vec::new(), [0; 4096]);
    while let Ok(n @ 1..) = s.read(&mut chunk) {
        buf.extend_from_slice(&chunk[..n]);
        let text = String::from_utf8_lossy(&buf).into_owned();
        if let Some((head, body)) = text.split_once("\r\n\r\n") {
            let len = head
                .lines()
                .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap()))
                .unwrap_or(0);
            if body.len() >= len {
                return text;
            }
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// Case-insensitive "request contains this header line".
fn has(req: &str, line: &str) -> bool {
    req.to_ascii_lowercase().contains(&line.to_ascii_lowercase())
}

type Rotations = Arc<Mutex<Vec<(i64, String)>>>;

fn client(base: &str) -> (Client, Rotations) {
    let seen = Rotations::default();
    let sink = seen.clone();
    let on_rotate: RotationSink = Arc::new(move |id, c: Secret| sink.lock().unwrap().push((id, c.expose().to_owned())));
    (Client::with_base(on_rotate, Some(base.into())), seen)
}

#[tokio::test]
async fn authenticated_user_success() {
    let (base, reqs) = serve(vec![reply("200 OK", &[], USER)]);
    let (c, rotations) = client(&base);
    let (user, rotated) = c.authenticated_user(5, &Secret::new("OLD")).await.unwrap();
    assert_eq!(user, AuthenticatedUser { id: 1, name: "roblox".into(), display_name: "Roblox".into() });
    assert!(rotated.is_none() && rotations.lock().unwrap().is_empty());

    let req = reqs.recv().unwrap();
    assert!(req.starts_with("GET /v1/users/authenticated HTTP/1.1\r\n"), "{req}");
    assert!(has(&req, "\r\ncookie: .ROBLOSECURITY=OLD\r\n"));
    assert!(has(&req, "\r\nuser-agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64)"));
    // GET on users.roblox.com: no CSRF token, no website Origin.
    assert!(!has(&req, "x-csrf-token") && !has(&req, "\r\norigin:"));
}

#[tokio::test]
async fn csrf_token_is_stored_and_retried_once() {
    let csrf_403 = reply(
        "403 Forbidden",
        &["x-csrf-token: TOK1", "set-cookie: .ROBLOSECURITY=_|WARNING:-DO-NOT-SHARE-THIS.NEW; domain=.roblox.com; path=/"],
        r#"{"code":0,"message":"Token Validation Failed"}"#,
    );
    let (base, reqs) = serve(vec![csrf_403, reply("200 OK", &[], "{}"), reply("200 OK", &[], "{}")]);
    let (c, _) = client(&base);
    let (old, body) = (Secret::new("OLD"), serde_json::json!({}));

    let (_, _, rotated) = c.request(Method::POST, "ticket", TICKET, Some(&body), 3, Some(&old)).await.unwrap();
    assert_eq!(rotated.unwrap().expose(), "_|WARNING:-DO-NOT-SHARE-THIS.NEW");
    let (first, retry) = (reqs.recv().unwrap(), reqs.recv().unwrap());
    assert!(!has(&first, "x-csrf-token"));
    assert!(has(&retry, "\r\nx-csrf-token: TOK1\r\n"));
    // The retry already carries the cookie Roblox rotated in the 403.
    assert!(has(&retry, "\r\ncookie: .ROBLOSECURITY=_|WARNING:-DO-NOT-SHARE-THIS.NEW\r\n"));
    for r in [&first, &retry] {
        assert!(r.starts_with("POST /v1/authentication-ticket HTTP/1.1\r\n"));
        assert!(has(r, "\r\nreferer: https://www.roblox.com/\r\n") && has(r, "\r\norigin: https://www.roblox.com\r\n"));
        assert!(has(r, "\r\ncontent-type: application/json\r\n") && r.ends_with("\r\n\r\n{}"));
    }

    // The cached token goes out with the account's next call straight away.
    c.request(Method::POST, "ticket", TICKET, Some(&body), 3, Some(&old)).await.unwrap();
    assert!(has(&reqs.recv().unwrap(), "\r\nx-csrf-token: TOK1\r\n"));
}

#[tokio::test]
async fn csrf_retry_happens_only_once() {
    let csrf_403 = reply("403 Forbidden", &["x-csrf-token: TOK"], "{}");
    let (base, _reqs) = serve(vec![csrf_403.clone(), csrf_403]);
    let (c, _) = client(&base);
    let r = c.request(Method::POST, "ticket", TICKET, None, 3, Some(&Secret::new("OLD"))).await;
    assert!(matches!(r, Err(AppError::Roblox(m)) if m == "HTTP 403"));
}

#[tokio::test]
async fn rotated_cookie_reaches_sink_and_caller() {
    let set = [
        "set-cookie: __cf_bm=x; Path=/; Domain=roblox.com",
        "set-cookie: .ROBLOSECURITY=_|WARNING:-DO-NOT-SHARE-THIS.NEW; domain=.roblox.com; expires=Mon, 24-Oct-2061 10:00:00 GMT; path=/; secure; HttpOnly",
    ];
    let ok = reply("200 OK", &set, USER);
    let (base, _reqs) = serve(vec![ok.clone(), ok.clone(), ok]);
    let (c, rotations) = client(&base);

    let (_, rotated) = c.authenticated_user(7, &Secret::new("OLD")).await.unwrap();
    assert_eq!(rotated.unwrap().expose(), "_|WARNING:-DO-NOT-SHARE-THIS.NEW");
    assert_eq!(*rotations.lock().unwrap(), vec![(7, "_|WARNING:-DO-NOT-SHARE-THIS.NEW".to_string())]);

    // A cookie that is not stored yet (id 0) comes back but never reaches the sink.
    let (_, rotated) = c.authenticated_user(0, &Secret::new("OLD")).await.unwrap();
    assert_eq!(rotated.unwrap().expose(), "_|WARNING:-DO-NOT-SHARE-THIS.NEW");
    assert_eq!(rotations.lock().unwrap().len(), 1);

    // Roblox repeating the current value is no rotation.
    let (_, rotated) = c.authenticated_user(7, &Secret::new("_|WARNING:-DO-NOT-SHARE-THIS.NEW")).await.unwrap();
    assert!(rotated.is_none());
    assert_eq!(rotations.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn rate_limited_then_success() {
    let limited = reply(
        "429 Too Many Requests",
        &["x-ratelimit-limit: 20, 20;w=60", "x-ratelimit-remaining: 0", "x-ratelimit-reset: 1", "retry-after: 5"],
        r#"{"errors":[{"code":0,"message":""}]}"#,
    );
    let (base, _reqs) = serve(vec![limited, reply("200 OK", &[], USER)]);
    let (c, _) = client(&base);
    let start = Instant::now();
    let (user, _) = c.authenticated_user(1, &Secret::new("OLD")).await.unwrap();
    assert_eq!(user.id, 1);
    // Waited for x-ratelimit-reset (1 s), not retry-after (5 s).
    assert!((Duration::from_millis(900)..Duration::from_secs(4)).contains(&start.elapsed()));
}

#[tokio::test]
async fn rate_limit_gives_up_after_three_tries() {
    let limited = reply("429 Too Many Requests", &["x-ratelimit-reset: 1"], "");
    let (base, _reqs) = serve(vec![limited.clone(), limited.clone(), limited]);
    let (c, _) = client(&base);
    assert!(matches!(c.authenticated_user(1, &Secret::new("OLD")).await, Err(AppError::RateLimited(1))));
}

#[tokio::test]
async fn unauthorized_is_cookie_invalid_or_bound_session() {
    let missing = String::from_utf8(fixture_body("acctinfo-users-authenticated-nocookie.json")).unwrap();
    let bound = r#"{"errors":[{"code":9002,"subcode":2,"message":"Unauthorized"}]}"#;
    let (base, _reqs) = serve(vec![reply("401 Unauthorized", &[], &missing), reply("401 Unauthorized", &[], bound)]);
    let (c, _) = client(&base);
    let cookie = Secret::new("OLD");
    assert!(matches!(c.authenticated_user(1, &cookie).await, Err(AppError::CookieInvalid)));
    assert!(matches!(c.authenticated_user(1, &cookie).await, Err(AppError::BoundSessionRequired)));
}

#[tokio::test]
async fn challenge_is_not_retried() {
    let challenge = reply(
        "403 Forbidden",
        &["rblx-challenge-id: abc", "rblx-challenge-type: captcha", "rblx-challenge-metadata: e30=", "x-csrf-token: TOK"],
        "{}",
    );
    // One canned answer: a retry would hit a closed port and fail as Network.
    let (base, _reqs) = serve(vec![challenge]);
    let (c, _) = client(&base);
    let url = "https://apis.roblox.com/sharelinks/v1/resolve-link";
    let r = c.request(Method::POST, "share", url, None, 1, Some(&Secret::new("OLD"))).await;
    assert!(matches!(r, Err(AppError::Challenge)));
}

#[tokio::test]
async fn network_error_names_host_only() {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let (c, _) = client(&format!("http://127.0.0.1:{port}"));
    match c.authenticated_user(1, &Secret::new("OLD")).await {
        Err(AppError::Network(m)) => assert!(!m.contains("/v1") && !m.contains("OLD"), "{m}"),
        r => panic!("{r:?}"),
    }
}

#[tokio::test]
async fn used_up_window_delays_the_next_call() {
    let (c, _) = client("http://127.0.0.1:1");
    c.set_window("route", 0, 1);
    let start = Instant::now();
    c.wait_turn("route").await.unwrap();
    assert!(start.elapsed() >= Duration::from_millis(900));

    c.set_window("route", 1, 60);
    let start = Instant::now();
    c.wait_turn("route").await.unwrap();
    assert!(start.elapsed() < Duration::from_millis(500));
    assert_eq!(lock(&c.windows)["route"].remaining, 0);
}

// Account info: recorded answers.

/// The recorded HTTP status of a JSON fixture.
fn fixture_status(name: &str) -> StatusCode {
    let v: Value = serde_json::from_str(&fixture(name)).unwrap();
    StatusCode::from_bytes(v["_status"].as_str().unwrap().split(' ').nth(1).unwrap().as_bytes()).unwrap()
}

fn is_http(e: AppError, code: &str) -> bool {
    matches!(e, AppError::Roblox(m) if m == format!("HTTP {code}"))
}

#[test]
fn user_details_fixture() {
    let d: UserDetails = json(&fixture_body("acctinfo-users-user-1.json")).unwrap();
    assert_eq!((d.id, d.name.as_str(), d.display_name.as_str()), (1, "Roblox", "Roblox"));
    assert_eq!(d.created.as_deref(), Some("2006-02-27T21:06:40.3Z"));
    assert!(!d.is_banned && d.has_verified_badge && d.description.starts_with("Welcome"));
    let invalid = "acctinfo-users-user-invalid.json";
    assert!(is_http(classify(fixture_status(invalid), &HeaderMap::new(), &fixture_body(invalid)), "404"));
}

#[test]
fn headshot_fixtures() {
    let sort = |name: &str| {
        let mut out = HashMap::new();
        let pending = sort_thumbnails(json(&fixture_body(name)).unwrap(), &mut out);
        let mut ids: Vec<i64> = out.keys().copied().collect();
        ids.sort_unstable();
        (ids, pending, out)
    };
    let (ids, pending, out) = sort("acctinfo-thumbnails-headshot-1-150-png.json");
    assert_eq!((ids, pending), (vec![1, 156], vec![]));
    assert!(out[&156].starts_with("https://tr.rbxcdn.com/30DAY-AvatarHeadshot-635A2AA0"));
    // Unknown ids are simply left out; /v1/batch rows carry extra fields.
    assert_eq!(sort("acctinfo-thumbnails-headshot-invalid-and-nonexistent.json").0, [1]);
    assert_eq!(sort("acctinfo-thumbnails-headshot-1-420-webp-circular.json").0, [1]);
    assert_eq!(sort("acctinfo-thumbnails-batch-headshot.json").0, [1, 156]);

    // Pending rows have "" URLs; Blocked rows (empty or a placeholder URL) are never kept.
    let (ids, pending, _) = sort("acctinfo-thumbnails-headshot-pending-first-request.json");
    assert_eq!((ids, pending), (vec![], vec![4123400, 4123401, 4123402]));
    let (ids, pending, _) = sort("acctinfo-thumbnails-headshot-pending-after-retry.json");
    assert_eq!((ids, pending), (vec![4123400, 4123401, 4123402], vec![]));
    assert_eq!(sort("acctinfo-thumbnails-headshot-blocked-states.json").0, [3000000]);

    for name in ["acctinfo-thumbnails-headshot-101ids.json", "acctinfo-thumbnails-headshot-badsize.json"] {
        assert!(is_http(classify(fixture_status(name), &HeaderMap::new(), &fixture_body(name)), "400"), "{name}");
    }
}

#[test]
fn social_count_fixtures() {
    let count = |name: &str| field::<i64>(json(&fixture_body(name)).unwrap(), "count").unwrap();
    assert_eq!(count("acctinfo-friends-count-1.json"), 0);
    assert_eq!(count("acctinfo-friends-followers-count-1.json"), 16821485);
    assert_eq!(count("acctinfo-friends-followings-count-1.json"), 0);
    // A missing field or a non-object answer is unreadable, never a default 0.
    assert!(field::<i64>(serde_json::json!({ "total": 3 }), "count").is_err());
    assert!(field::<i64>(Value::Bool(true), "count").is_err());
}

#[test]
fn presence_fixtures() {
    let rows = |name: &str| json::<Presences>(&fixture_body(name)).unwrap().user_presences;
    let three = rows("acctinfo-presence-users-1-156-nocookie.json");
    assert_eq!(three.iter().map(|p| p.user_id).collect::<Vec<_>>(), [1, 156, 261]);
    let offline = UserPresence {
        user_id: 1,
        user_presence_type: 0,
        last_location: Some("Website".into()),
        place_id: None,
        root_place_id: None,
        game_id: None,
        universe_id: None,
    };
    assert_eq!(three[0], offline);
    assert!(rows("acctinfo-presence-users-empty.json").is_empty());
    // Unknown ids still get an offline row.
    assert_eq!(rows("acctinfo-presence-users-nonexistent.json")[0].user_id, 999999999999);

    let too_many = "acctinfo-presence-users-51ids-error.json";
    assert_eq!(fixture_status(too_many), StatusCode::BAD_REQUEST);
    assert_eq!(error_code(&fixture_body(too_many)), Some((3, 0)));
}

#[test]
fn plus_fixtures() {
    let rows = |name: &str| json::<Profiles>(&fixture_body(name)).unwrap().profile_details;
    let two: Vec<_> = rows("acctinfo-userprofile-get-profiles-nocookie.json")
        .into_iter()
        .map(|p| (p.user_id, p.has_roblox_subscription))
        .collect();
    assert_eq!(two, [(1, Some(true)), (156, Some(true))]);
    // Over 200 ids (or only unknown fields): an empty 200, not an error.
    assert!(rows("acctinfo-userprofile-get-profiles-201ids-silent-empty.json").is_empty());
}

#[test]
fn two_step_and_cookie_only_fixtures() {
    // The swagger's UserConfiguration (no recording with a cookie exists).
    let config = br#"{"primaryMediaType":2,"methods":[
        {"mediaType":0,"enabled":true,"updated":"2026-01-02T03:04:05Z"},{"mediaType":2,"enabled":true,"updated":null},
        {"mediaType":3,"enabled":true},{"mediaType":1,"enabled":false}]}"#;
    assert_eq!(enabled_methods(json(config).unwrap()), ["Email", "Authenticator"]);
    assert!(enabled_methods(json(b"{}").unwrap()).is_empty());

    // Without a cookie 2SV answers 400, the others 401.
    let tsv = "acctinfo-twostepverification-configuration-1-nocookie.json";
    assert!(is_http(classify(fixture_status(tsv), &HeaderMap::new(), &fixture_body(tsv)), "400"));
    for name in [
        "acctinfo-economy-user-currency-nocookie.json",
        "acctinfo-premiumfeatures-validate-membership-1-nocookie.json",
        "acctinfo-accountsettings-email-nocookie.json",
    ] {
        let e = classify(fixture_status(name), &HeaderMap::new(), &fixture_body(name));
        assert!(matches!(e, AppError::CookieInvalid), "{name}");
    }
}

// Account info end to end.

fn ok(body: &str) -> String {
    reply("200 OK", &[], body)
}

fn first_line(req: &str) -> &str {
    req.lines().next().unwrap()
}

fn body_of(req: &str) -> Value {
    serde_json::from_str(req.split_once("\r\n\r\n").unwrap().1).unwrap()
}

const HEADSHOT_QUERY: &str = "&size=150x150&format=Png&isCircular=false HTTP/1.1";

#[tokio::test]
async fn headshots_re_request_only_pending() {
    let first = r#"{"data":[
        {"targetId":1,"state":"Completed","imageUrl":"https://tr.rbxcdn.com/a"},
        {"targetId":2,"state":"Pending","imageUrl":""},
        {"targetId":3,"state":"Blocked","imageUrl":"https://tr.rbxcdn.com/placeholder"},
        {"targetId":4,"state":"Pending","imageUrl":""}]}"#;
    let second = r#"{"data":[{"targetId":2,"state":"Completed","imageUrl":"https://t6.rbxcdn.com/b"},
        {"targetId":4,"state":"Pending","imageUrl":""}]}"#;
    let third = r#"{"data":[{"targetId":4,"state":"Completed","imageUrl":""}]}"#;
    let (base, reqs) = serve(vec![ok(first), ok(second), ok(third)]);
    let (c, _) = client(&base);

    let urls = c.headshots(&[3, 1, 2, 4, 2, 0]).await.unwrap();
    let want = HashMap::from([(1, "https://tr.rbxcdn.com/a".to_string()), (2, "https://t6.rbxcdn.com/b".to_string())]);
    assert_eq!(urls, want);
    for ids in ["1,2,3,4", "2,4", "4"] {
        let req = reqs.recv().unwrap();
        assert_eq!(first_line(&req), format!("GET /v1/users/avatar-headshot?userIds={ids}{HEADSHOT_QUERY}"));
        assert!(!has(&req, "\r\ncookie:"));
    }
}

#[tokio::test]
async fn headshots_give_up_after_five_retries() {
    let pending = ok(r#"{"data":[{"targetId":8,"state":"Pending","imageUrl":""}]}"#);
    let (base, reqs) = serve(vec![pending; 6]);
    let (c, _) = client(&base);
    // A seventh call would hit a closed port and fail as Network.
    assert!(c.headshots(&[8]).await.unwrap().is_empty());
    assert_eq!(reqs.iter().count(), 6);
}

#[tokio::test]
async fn plus_row_without_the_field_is_absent() {
    let body = r#"{"profileDetails":[{"userId":1,"hasRobloxSubscription":true},
        {"userId":2,"hasRobloxSubscription":false},{"userId":3,"names":{"username":null}},
        {"userId":4,"hasRobloxSubscription":null}],"errors":[]}"#;
    let (base, reqs) = serve(vec![ok(body)]);
    let (c, _) = client(&base);
    assert_eq!(c.plus(&[1, 2, 3, 4]).await.unwrap(), HashMap::from([(1, true), (2, false)]));

    let req = reqs.recv().unwrap();
    assert_eq!(first_line(&req), "POST /user-profile-api/v1/user/profiles/get-profiles HTTP/1.1");
    assert_eq!(body_of(&req), serde_json::json!({ "userIds": [1, 2, 3, 4], "fields": ["hasRobloxSubscription"] }));
    assert!(!has(&req, "\r\ncookie:"));
}

#[tokio::test]
async fn presence_drops_bad_ids_and_chunks_at_50() {
    let page = |ids: std::ops::RangeInclusive<i64>| {
        let row = |id| format!(r#"{{"userPresenceType":2,"placeId":5,"gameId":"g","universeId":9,"userId":{id}}}"#);
        let rows: Vec<String> = ids.map(row).collect();
        ok(&format!(r#"{{"userPresences":[{}]}}"#, rows.join(",")))
    };
    let (base, reqs) = serve(vec![page(1..=50), page(51..=60)]);
    let (c, _) = client(&base);
    let cookie = Secret::new("OLD");

    let mut ids: Vec<i64> = (1..=60).rev().collect();
    ids.extend([0, -5, 7, 60]);
    let rows = c.presence(4, &cookie, &ids).await.unwrap();
    assert_eq!(rows.iter().map(|p| p.user_id).collect::<Vec<_>>(), (1..=60).collect::<Vec<_>>());
    assert_eq!((rows[59].universe_id, rows[59].root_place_id), (Some(9), None));

    for want in [1..=50, 51..=60] {
        let req = reqs.recv().unwrap();
        assert_eq!(first_line(&req), "POST /v1/presence/users HTTP/1.1");
        assert!(has(&req, "\r\ncookie: .ROBLOSECURITY=OLD\r\n"));
        assert_eq!(body_of(&req), serde_json::json!({ "userIds": want.collect::<Vec<i64>>() }));
    }
    // Nothing valid to ask about: no call at all (a third would fail as Network).
    assert!(c.presence(4, &cookie, &[0, -1]).await.unwrap().is_empty());
}

#[tokio::test]
async fn premium_is_a_bare_boolean() {
    let (base, reqs) = serve(vec![ok("true"), ok("false"), ok(r#"{"isPremium":true}"#)]);
    let (c, _) = client(&base);
    let cookie = Secret::new("OLD");
    assert!(c.premium(2, &cookie, 77).await.unwrap());
    assert!(!c.premium(2, &cookie, 77).await.unwrap());
    assert!(matches!(c.premium(2, &cookie, 77).await, Err(AppError::Roblox(m)) if m == "unreadable response"));

    let req = reqs.recv().unwrap();
    assert_eq!(first_line(&req), "GET /v1/users/77/validate-membership HTTP/1.1");
    assert!(has(&req, "\r\ncookie: .ROBLOSECURITY=OLD\r\n"));
}

#[tokio::test]
async fn public_calls_send_no_cookie() {
    // A Set-Cookie on an anonymous answer is no rotation either.
    let count = reply("200 OK", &["set-cookie: .ROBLOSECURITY=_|WARNING:-DO-NOT-SHARE-THIS.NEW; path=/"], r#"{"count":7}"#);
    let (base, reqs) = serve(vec![count.clone(), count.clone(), count, ok(USER)]);
    let (c, rotations) = client(&base);

    let counts = c.social_counts(1).await.unwrap();
    assert_eq!(counts, SocialCounts { friends: 7, followers: 7, following: 7 });
    let mut lines: Vec<String> = (0..3)
        .map(|_| {
            let req = reqs.recv().unwrap();
            assert!(!has(&req, "\r\ncookie:"), "{req}");
            first_line(&req).to_owned()
        })
        .collect();
    lines.sort();
    let want = ["followers", "followings", "friends"].map(|k| format!("GET /v1/users/1/{k}/count HTTP/1.1"));
    assert_eq!(lines, want);

    assert_eq!(c.user_details(9, None, 1).await.unwrap().display_name, "Roblox");
    let req = reqs.recv().unwrap();
    assert_eq!(first_line(&req), "GET /v1/users/1 HTTP/1.1");
    assert!(!has(&req, "\r\ncookie:"));
    assert!(rotations.lock().unwrap().is_empty());
}

#[tokio::test]
async fn robux_email_and_details_use_the_cookie() {
    let email = r#"{"emailAddress":"r***@example.com","verified":true,"canBypassPasswordForEmailUpdate":false}"#;
    let (base, reqs) = serve(vec![ok(r#"{"robux":1234}"#), ok(email), ok(USER)]);
    let (c, _) = client(&base);
    let cookie = Secret::new("OLD");
    assert_eq!(c.robux(2, &cookie).await.unwrap(), 1234);
    assert!(c.email_verified(2, &cookie).await.unwrap());
    assert_eq!(c.user_details(2, Some(&cookie), 1).await.unwrap().id, 1);

    for path in ["/v1/user/currency", "/v1/email", "/v1/users/1"] {
        let req = reqs.recv().unwrap();
        assert_eq!(first_line(&req), format!("GET {path} HTTP/1.1"));
        assert!(has(&req, "\r\ncookie: .ROBLOSECURITY=OLD\r\n"));
    }
}

#[tokio::test]
async fn two_step_maps_int_and_string_media_types() {
    let body = r#"{"primaryMediaType":2,"methods":[
        {"mediaType":0,"enabled":true},{"mediaType":"Authenticator","enabled":true},
        {"mediaType":1,"enabled":false},{"mediaType":3,"enabled":true},{"mediaType":"Password","enabled":true},
        {"mediaType":"securitykey","enabled":true},{"mediaType":7,"enabled":true},
        {"mediaType":99,"enabled":true},{"mediaType":-1,"enabled":true},{"mediaType":"Pigeon","enabled":true}]}"#;
    let (base, reqs) = serve(vec![ok(body)]);
    let (c, _) = client(&base);
    let methods = c.two_step_methods(3, &Secret::new("OLD"), 42).await.unwrap();
    assert_eq!(methods, ["Email", "Authenticator", "SecurityKey", "Passkey"]);

    let req = reqs.recv().unwrap();
    assert_eq!(first_line(&req), "GET /v1/users/42/configuration HTTP/1.1");
    assert!(has(&req, "\r\ncookie: .ROBLOSECURITY=OLD\r\n"));
}

// Launching end to end: the API calls and `launcher::resolve` on top of them.

const LINK_CODE: &str = "67771413747491684286858666824242";
const JOB: &str = "8d3e4f5a-1b2c-4d5e-8f90-123456789abc";

fn csrf_403() -> String {
    reply("403 Forbidden", &["x-csrf-token: TOK"], r#"{"errors":[{"code":0,"message":"XSRF token invalid"}]}"#)
}

fn share(link_type: &str) -> LaunchTarget {
    LaunchTarget::ShareLink { code: "ce65b2c80e8dae48866fe2f919966557".into(), link_type: link_type.into() }
}

/// `resolve` against canned answers, as account 4 with cookie "OLD" (the warning is dropped).
async fn resolve_with(answers: Vec<String>, target: LaunchTarget) -> (Result<Join>, mpsc::Receiver<String>) {
    let (r, reqs) = resolve_warn(answers, target).await;
    (r.map(|(join, _)| join), reqs)
}

async fn resolve_warn(
    answers: Vec<String>,
    target: LaunchTarget,
) -> (Result<(Join, Option<String>)>, mpsc::Receiver<String>) {
    let (base, reqs) = serve(answers);
    let (c, _) = client(&base);
    (resolve(&c, 4, &Secret::new("OLD"), &target).await, reqs)
}

fn invalid(r: Result<Join>, want: &str) -> bool {
    matches!(r, Err(AppError::Invalid(m)) if m == want)
}

#[tokio::test]
async fn ticket_comes_from_the_header_after_a_csrf_retry() {
    let assertion = r#"{"clientAssertion":"ASSERT.abc"}"#;
    let ticket = reply("200 OK", &["rbx-authentication-ticket: TICKET-123"], "");
    let (base, reqs) = serve(vec![ok(assertion), csrf_403(), ticket]);
    let (c, _) = client(&base);
    let cookie = Secret::new("OLD");

    let assertion = c.client_assertion(3, &cookie).await.ok();
    let ticket = c.authentication_ticket(3, &cookie, assertion.as_ref()).await.unwrap();
    assert_eq!(ticket.expose(), "TICKET-123");
    assert!(!format!("{ticket:?}").contains("TICKET"));

    let get = reqs.recv().unwrap();
    assert_eq!(first_line(&get), "GET /v1/client-assertion/ HTTP/1.1");
    assert!(has(&get, "\r\ncookie: .ROBLOSECURITY=OLD\r\n"));
    let (first, retry) = (reqs.recv().unwrap(), reqs.recv().unwrap());
    assert!(!has(&first, "x-csrf-token") && has(&retry, "\r\nx-csrf-token: TOK\r\n"));
    for r in [&first, &retry] {
        assert_eq!(first_line(r), "POST /v1/authentication-ticket/ HTTP/1.1");
        assert!(has(r, "\r\ncookie: .ROBLOSECURITY=OLD\r\n") && has(r, "\r\ncontent-type: application/json\r\n"));
        assert!(has(r, "\r\nreferer: https://www.roblox.com/\r\n") && has(r, "\r\norigin: https://www.roblox.com\r\n"));
        // The assertion answer is the ticket request's body, unchanged.
        assert_eq!(body_of(r), serde_json::json!({ "clientAssertion": "ASSERT.abc" }));
    }
}

#[tokio::test]
async fn ticket_without_assertion_sends_empty_object() {
    let missing = String::from_utf8(fixture_body("authlaunch-client-assertion-nocookie.json")).unwrap();
    let ticket = reply("200 OK", &["rbx-authentication-ticket: T2"], "{}");
    let (base, reqs) = serve(vec![reply("401 Unauthorized", &[], &missing), ticket, ok("{}")]);
    let (c, _) = client(&base);
    let cookie = Secret::new("OLD");

    // A failed assertion is ignored, as the website does.
    let assertion = c.client_assertion(3, &cookie).await.ok();
    assert!(assertion.is_none());
    assert_eq!(c.authentication_ticket(3, &cookie, None).await.unwrap().expose(), "T2");
    reqs.recv().unwrap();
    assert_eq!(body_of(&reqs.recv().unwrap()), serde_json::json!({}));

    // A 2xx without the header is no ticket.
    let r = c.authentication_ticket(3, &cookie, None).await;
    assert!(matches!(r, Err(AppError::Roblox(m)) if m == "no ticket"));
}

#[tokio::test]
async fn server_share_link_joins_privately_with_its_link_code() {
    let answer = format!(
        r#"{{"experienceInviteData":null,"privateServerInviteData":{{"status":"Valid","ownerUserId":45348281,
        "universeId":6430220996,"privateServerId":1630945839,"linkCode":"{LINK_CODE}","placeId":18980972074}},
        "experienceAffiliateData":null}}"#
    );
    let (join, reqs) = resolve_with(vec![csrf_403(), ok(&answer)], share("Server")).await;
    let want = Join::Private { place_id: 18980972074, link_code: Some(LINK_CODE.into()), access_code: None };
    assert_eq!(join.unwrap(), want);

    let (first, retry) = (reqs.recv().unwrap(), reqs.recv().unwrap());
    assert!(has(&retry, "\r\nx-csrf-token: TOK\r\n"));
    for r in [&first, &retry] {
        assert_eq!(first_line(r), "POST /sharelinks/v1/resolve-link HTTP/1.1");
        assert!(has(r, "\r\ncookie: .ROBLOSECURITY=OLD\r\n") && has(r, "\r\norigin: https://www.roblox.com\r\n"));
        let body = serde_json::json!({ "linkId": "ce65b2c80e8dae48866fe2f919966557", "linkType": "Server" });
        assert_eq!(body_of(r), body);
    }
}

#[tokio::test]
async fn share_link_status_invalid_fails_and_expired_still_joins() {
    let gone = ok(r#"{"privateServerInviteData":{"status":"Invalid"}}"#);
    let (join, _) = resolve_with(vec![gone], share("Server")).await;
    assert!(invalid(join, "This share link is no longer valid."));

    let expired = format!(r#"{{"privateServerInviteData":{{"status":"Expired","placeId":5,"linkCode":"{LINK_CODE}"}}}}"#);
    let (r, _) = resolve_warn(vec![ok(&expired)], share("Server")).await;
    let (join, warning) = r.unwrap();
    assert_eq!(join, Join::Private { place_id: 5, link_code: Some(LINK_CODE.into()), access_code: None });
    assert!(warning.unwrap().contains("expired"));

    // A server link without a usable link code, and types that are no game.
    let no_code = ok(r#"{"privateServerInviteData":{"status":"Valid","placeId":5,"linkCode":"1+placelauncherurl:x"}}"#);
    let (join, _) = resolve_with(vec![no_code], share("Server")).await;
    assert!(invalid(join, "This share link is no longer valid."));
    let friend = ok(r#"{"friendInviteData":{"status":"Valid","senderUserId":1},"privateServerInviteData":null}"#);
    let (join, _) = resolve_with(vec![friend], share("FriendInvite")).await;
    assert!(invalid(join, "This share link doesn't lead to a game or a player."));
}

#[tokio::test]
async fn experience_invite_joins_its_server_while_valid() {
    let valid = format!(r#"{{"experienceInviteData":{{"status":"Valid","placeId":1818,"instanceId":"{JOB}","inviterId":9}}}}"#);
    let (join, _) = resolve_with(vec![ok(&valid)], share("ExperienceInvite")).await;
    assert_eq!(join.unwrap(), Join::Job { place_id: 1818, job_id: JOB.into() });

    // Expired (or the inviter left): the website opens the game instead.
    let left = format!(r#"{{"experienceInviteData":{{"status":"InviterNotInExperience","placeId":1818,"instanceId":"{JOB}"}}}}"#);
    let (join, _) = resolve_with(vec![ok(&left)], share("ExperienceInvite")).await;
    assert_eq!(join.unwrap(), Join::Game { place_id: 1818, launch_data: None });

    // Notification invites carry the inviter's server the same way.
    let note = format!(r#"{{"notificationExperienceInviteData":{{"status":"Valid","placeId":1818,"instanceId":"{JOB}","launchData":"x"}}}}"#);
    let (join, _) = resolve_with(vec![ok(&note)], share("NotificationExperienceInvite")).await;
    assert_eq!(join.unwrap(), Join::Job { place_id: 1818, job_id: JOB.into() });
    let old = format!(r#"{{"notificationExperienceInviteData":{{"status":"Expired","placeId":1818,"instanceId":"{JOB}","launchData":"x"}}}}"#);
    let (r, _) = resolve_warn(vec![ok(&old)], share("NotificationExperienceInvite")).await;
    assert_eq!(r.unwrap(), (Join::Game { place_id: 1818, launch_data: Some("x".into()) }, Some(EXPIRED_TEXT.into())));
}

const EXPIRED_TEXT: &str = "This share link has expired, so Roblox may refuse the join.";

#[tokio::test]
async fn experience_details_resolve_the_universe_to_its_root_place() {
    let details = ok(r#"{"experienceDetailsInviteData":{"status":"Valid","universeId":994732206}}"#);
    let games = ok(&fixture("meta-games-by-universe.json"));
    let (join, reqs) = resolve_with(vec![details, games], share("ExperienceDetails")).await;
    assert_eq!(join.unwrap(), Join::Game { place_id: 2753915549, launch_data: None });

    reqs.recv().unwrap();
    let req = reqs.recv().unwrap();
    assert_eq!(first_line(&req), "GET /v1/games?universeIds=994732206 HTTP/1.1");
    assert!(!has(&req, "\r\ncookie:"));

    // Affiliate links keep their launch data in joinData; an unknown universe is no game.
    let affiliate = ok(r#"{"experienceAffiliateData":{"status":"Valid","universeId":7,"joinData":{"launchData":"ref=abc"}}}"#);
    let root = ok(r#"{"data":[{"id":7,"rootPlaceId":70,"name":"Seven"}]}"#);
    let (join, _) = resolve_with(vec![affiliate, root], share("ExperienceAffiliate")).await;
    assert_eq!(join.unwrap(), Join::Game { place_id: 70, launch_data: Some("ref=abc".into()) });
    let (join, _) = resolve_with(vec![ok(r#"{"data":[]}"#)], LaunchTarget::Universe { universe_id: 8 }).await;
    assert!(invalid(join, "This game no longer exists."));
}

#[tokio::test]
async fn profile_link_follows_the_user() {
    let profile = ok(r#"{"profileLinkResolutionResponseData":{"status":"Valid","userId":156}}"#);
    let (join, _) = resolve_with(vec![profile], share("Profile")).await;
    assert_eq!(join.unwrap(), Join::Follow { user_id: 156 });
}

#[tokio::test]
async fn v2_experience_link_is_a_game_with_custom_data() {
    let v2 = ok(r#"{"linkType":"Experience","status":"Valid","targetId":1818,"customData":{"k":"v"}}"#);
    let target = LaunchTarget::ShareLinkV2 { code: "abc123".into() };
    let (join, reqs) = resolve_with(vec![csrf_403(), v2], target.clone()).await;
    assert_eq!(join.unwrap(), Join::Game { place_id: 1818, launch_data: Some(r#"{"k":"v"}"#.into()) });
    reqs.recv().unwrap();
    let req = reqs.recv().unwrap();
    assert_eq!(first_line(&req), "POST /deeplinks/v2/resolve HTTP/1.1");
    assert!(has(&req, "\r\nx-csrf-token: TOK\r\n") && has(&req, "\r\ncookie: .ROBLOSECURITY=OLD\r\n"));
    assert_eq!(body_of(&req), serde_json::json!({ "linkId": "abc123" }));

    let text = ok(r#"{"linkType":"Experience","targetId":1818,"customData":"plain"}"#);
    let (join, _) = resolve_with(vec![text], target.clone()).await;
    assert_eq!(join.unwrap(), Join::Game { place_id: 1818, launch_data: Some("plain".into()) });
    let studio = ok(r#"{"linkType":"StudioEdit","status":"Valid","targetId":1818}"#);
    let (join, _) = resolve_with(vec![studio], target).await;
    assert!(invalid(join, "This share link doesn't lead to a game."));
}

#[tokio::test]
async fn direct_targets_need_no_call_and_long_launch_data_is_dropped() {
    let (c, _) = client("http://127.0.0.1:1");
    let cookie = Secret::new("OLD");
    let place = |d: &str| LaunchTarget::Place { place_id: 1818, launch_data: Some(d.into()) };
    let at_limit = "x".repeat(200);
    assert_eq!(
        resolve(&c, 4, &cookie, &place(&at_limit)).await.unwrap(),
        (Join::Game { place_id: 1818, launch_data: Some(at_limit) }, None)
    );
    let (over, warning) = resolve(&c, 4, &cookie, &place(&"x".repeat(201))).await.unwrap();
    assert_eq!(over, Join::Game { place_id: 1818, launch_data: None });
    assert!(warning.unwrap().contains("200-byte"));

    let server = LaunchTarget::Server { place_id: 1, job_id: JOB.into() };
    assert_eq!(resolve(&c, 4, &cookie, &server).await.unwrap().0, Join::Job { place_id: 1, job_id: JOB.into() });
    let access = LaunchTarget::PrivateAccess { place_id: 1, access_code: JOB.into() };
    let want = Join::Private { place_id: 1, link_code: None, access_code: Some(JOB.into()) };
    assert_eq!(resolve(&c, 4, &cookie, &access).await.unwrap().0, want);
    let (follow, _) = resolve(&c, 4, &cookie, &LaunchTarget::FollowUser { user_id: 2 }).await.unwrap();
    assert_eq!((follow.place_id(), follow), (None, Join::Follow { user_id: 2 }));
}

#[tokio::test]
async fn users_by_name_excludes_banned_and_sends_no_cookie() {
    let found = String::from_utf8(fixture_body("authlaunch-csrf-users-v1-usernames-nocsrf.json")).unwrap();
    let (base, reqs) = serve(vec![ok(&found)]);
    let (c, _) = client(&base);
    let users = c.users_by_name(&["Roblox".to_string()]).await.unwrap();
    assert_eq!(users, [AuthenticatedUser { id: 1, name: "Roblox".into(), display_name: "Roblox".into() }]);

    let req = reqs.recv().unwrap();
    assert_eq!(first_line(&req), "POST /v1/usernames/users HTTP/1.1");
    assert_eq!(body_of(&req), serde_json::json!({ "usernames": ["Roblox"], "excludeBannedUsers": true }));
    assert!(!has(&req, "\r\ncookie:"));
    assert!(c.users_by_name(&[]).await.unwrap().is_empty());
}

#[tokio::test]
async fn friend_statuses_read_ints_and_strings() {
    let body = r#"{"data":[{"id":2,"status":1},{"id":3,"status":"RequestSent"},{"id":4,"status":3},{"id":5,"status":"Weird"},{"id":6}]}"#;
    let (base, reqs) = serve(vec![ok(body)]);
    let (c, _) = client(&base);
    let s = c.friend_statuses(4, &Secret::new("OLD"), 1, &[6, 5, 4, 3, 2, 2, 0]).await.unwrap();
    assert_eq!(s[&2], FriendStatus::Friends);
    assert_eq!(s[&3], FriendStatus::RequestSent);
    assert_eq!(s[&4], FriendStatus::RequestReceived);
    assert_eq!((s[&5], s[&6]), (FriendStatus::NotFriends, FriendStatus::NotFriends));
    let req = reqs.recv().unwrap();
    assert_eq!(first_line(&req), "GET /v1/users/1/friends/statuses?userIds=2,3,4,5,6 HTTP/1.1");
    assert!(has(&req, "\r\ncookie: .ROBLOSECURITY=OLD\r\n"));
}

#[tokio::test]
async fn friend_request_and_accept_send_csrf() {
    let (base, reqs) = serve(vec![csrf_403(), ok("{}"), ok("{}")]);
    let (c, _) = client(&base);
    let cookie = Secret::new("OLD");
    c.request_friendship(4, &cookie, 156).await.unwrap();
    c.accept_friend(4, &cookie, 261).await.unwrap();
    let (_, retried, accept) = (reqs.recv().unwrap(), reqs.recv().unwrap(), reqs.recv().unwrap());
    assert_eq!(first_line(&retried), "POST /v1/users/156/request-friendship HTTP/1.1");
    assert!(has(&retried, "\r\nx-csrf-token: TOK\r\n"));
    assert_eq!(first_line(&accept), "POST /v1/users/261/accept-friend-request HTTP/1.1");
    assert!(has(&accept, "\r\nx-csrf-token: TOK\r\n") && has(&accept, "\r\ncookie: .ROBLOSECURITY=OLD\r\n"));
}

#[tokio::test]
async fn friend_request_captcha_is_a_challenge() {
    let captcha = reply("403 Forbidden", &["rblx-challenge-id: abc", "rblx-challenge-type: captcha", "x-csrf-token: TOK"], "{}");
    let (base, _reqs) = serve(vec![captcha]);
    let (c, _) = client(&base);
    assert!(matches!(c.request_friendship(4, &Secret::new("OLD"), 156).await, Err(AppError::Challenge)));
}

#[tokio::test]
async fn friends_online_reads_pascal_case_presence() {
    let body = format!(
        r#"{{"data":[{{"id":156,"name":"builderman","displayName":"Builder","sortScore":1.5,"userPresence":{{
        "UserPresenceType":"InGame","UserLocationType":"Game","lastLocation":"Crossroads","placeId":1818,
        "rootPlaceId":1818,"gameInstanceId":"{JOB}","universeId":13058,"lastOnline":"2026-10-24T10:00:00Z"}}}},
        {{"id":261,"name":"shedletsky","displayName":"Shedletsky","userPresence":{{"UserPresenceType":"Online"}}}},
        {{"id":7,"name":"x","displayName":"X"}}]}}"#
    );
    let missing = String::from_utf8(fixture_body("acctinfo-friends-online-1-nocookie.json")).unwrap();
    let (base, reqs) = serve(vec![ok(&body), reply("401 Unauthorized", &[], &missing)]);
    let (c, _) = client(&base);
    let cookie = Secret::new("OLD");

    let friends = c.friends_online(4, &cookie, 1).await.unwrap();
    assert_eq!(friends.iter().map(|f| f.id).collect::<Vec<_>>(), [156, 261, 7]);
    let p = friends[0].presence.clone().unwrap();
    assert_eq!((p.user_presence_type.as_deref(), p.game_instance_id.as_deref()), (Some("InGame"), Some(JOB)));
    assert_eq!((p.place_id, p.root_place_id, p.universe_id), (Some(1818), Some(1818), Some(13058)));
    assert_eq!((friends[0].display_name.as_str(), p.last_location.as_deref()), ("Builder", Some("Crossroads")));
    assert_eq!(friends[1].presence.as_ref().unwrap().user_presence_type.as_deref(), Some("Online"));
    assert!(friends[2].presence.is_none());

    let req = reqs.recv().unwrap();
    assert_eq!(first_line(&req), "GET /v1/users/1/friends/online HTTP/1.1");
    assert!(has(&req, "\r\ncookie: .ROBLOSECURITY=OLD\r\n"));
    assert!(matches!(c.friends_online(4, &cookie, 1).await, Err(AppError::CookieInvalid)));
}

/// Live check of the public (cookie-less) endpoints against roblox.com, user 1 ("Roblox").
/// Not run by default: `cargo test live_public -- --ignored --nocapture`.
#[tokio::test]
#[ignore = "talks to roblox.com"]
async fn live_public_endpoints() {
    let c = Client::new(Arc::new(|_, _| panic!("no cookie was sent, nothing can rotate")));
    let user = c.user_details(0, None, 1).await.unwrap();
    assert_eq!((user.id, user.name.as_str()), (1, "Roblox"));
    assert!(user.created.is_some());
    let heads = c.headshots(&[1, 156]).await.unwrap();
    assert!(heads.get(&1).is_some_and(|u| u.starts_with("https://") && u.contains(".rbxcdn.com/")), "{heads:?}");
    let plus = c.plus(&[1, 156]).await.unwrap();
    assert!(plus.contains_key(&1), "{plus:?}");
    let counts = c.social_counts(1).await.unwrap();
    assert!(counts.followers > 1_000_000, "{counts:?}");
    let games = c.universe_root_places(&[383310974]).await.unwrap();
    assert_eq!(games.get(&383310974).map(|g| g.0), Some(920587237), "{games:?}");
    let found = c.users_by_name(&["Roblox".to_owned()]).await.unwrap();
    assert_eq!(found.first().map(|u| u.id), Some(1));
    println!("user={user:?}\nheads={heads:?}\nplus={plus:?}\ncounts={counts:?}\ngames={games:?}");
}

// Server list.

const SERVERS: &str = "GET /v2/games/920587237/servers/Public";

#[tokio::test]
async fn servers_public_sends_the_website_query_and_the_cookie() {
    let (base, reqs) = serve(vec![ok(&fixture("servers-v2-public-limit25.json"))]);
    let (c, _) = client(&base);
    let cookie = Secret::new("OLD");
    let cursor = "GioKKAgj+/x==";
    let page = c.servers_public(3, Some(&cookie), 920587237, ServerOrder::OccupancyDesc, true, 25, Some(cursor)).await.unwrap();
    assert_eq!(page.servers.len(), 25);
    assert_eq!(page.next_cursor.as_deref(), Some("GioKKAgjEiQ4YzBlNzgyYi05ZjlhLTQ0MjItOGZmMC03MTcwNjEzOTg1YWJwAQ=="));

    let req = reqs.recv().unwrap();
    let want = format!("{SERVERS}?orderBy=OccupancyDesc&excludeFullGames=true&limit=25&cursor=GioKKAgj%2B%2Fx%3D%3D HTTP/1.1");
    assert_eq!(first_line(&req), want);
    assert!(has(&req, "\r\ncookie: .ROBLOSECURITY=OLD\r\n"));
}

#[tokio::test]
async fn servers_public_maps_each_order_and_sends_no_cookie_for_guests() {
    let empty = ok(r#"{"previousPageCursor":null,"nextPageCursor":null,"data":[]}"#);
    let (base, reqs) = serve(vec![empty.clone(), empty.clone(), empty]);
    let (c, _) = client(&base);
    let cookie = Secret::new("OLD");
    let asks = [
        (ServerOrder::OccupancyAsc, None, false, 10, "OccupancyAsc&excludeFullGames=false&limit=10"),
        (ServerOrder::Recommended, Some(&cookie), true, 50, "Recommended&excludeFullGames=true&limit=50"),
        (ServerOrder::BestLatency, Some(&cookie), false, 100, "BestLatency&excludeFullGames=false&limit=100"),
    ];
    for (order, cookie, exclude_full, limit, query) in asks {
        // An empty cursor is no cursor.
        let page = c.servers_public(3, cookie, 920587237, order, exclude_full, limit, Some("")).await.unwrap();
        assert_eq!(page, ServerPage { servers: vec![], next_cursor: None });
        let req = reqs.recv().unwrap();
        assert_eq!(first_line(&req), format!("{SERVERS}?orderBy={query} HTTP/1.1"));
        assert_eq!(has(&req, "\r\ncookie:"), cookie.is_some(), "{req}");
    }
}

#[tokio::test]
async fn servers_public_rejects_other_page_sizes_without_a_call() {
    let (base, reqs) = serve(vec![]);
    let (c, _) = client(&base);
    for limit in [0, 1, 20, 99, 200] {
        let r = c.servers_public(0, None, 920587237, ServerOrder::OccupancyDesc, true, limit, None).await;
        assert!(matches!(r, Err(AppError::Invalid(_))), "{limit}");
    }
    assert!(reqs.try_recv().is_err());
}

#[tokio::test]
async fn servers_public_guest_sorts_say_to_pick_an_account() {
    let guest = fixture("servers-v2-public-bestlatency-unauth.json");
    let bad_limit = fixture("servers-v1-invalid-limit20.json");
    let (base, _reqs) = serve(vec![reply("400 Bad Request", &[], &guest), reply("400 Bad Request", &[], &bad_limit)]);
    let (c, _) = client(&base);
    let r = c.servers_public(0, None, 920587237, ServerOrder::BestLatency, true, 100, None).await;
    assert!(matches!(r, Err(AppError::Invalid(m)) if m.starts_with("Pick an account to sort by")));
    // Any other 400 stays a plain Roblox error.
    let r = c.servers_public(0, None, 920587237, ServerOrder::OccupancyDesc, true, 100, None).await;
    assert!(is_http(r.unwrap_err(), "400"));

    // Code 7 means "guest" only on the server list.
    let (base, _reqs) = serve(vec![reply("400 Bad Request", &[], &guest)]);
    let (c, _) = client(&base);
    assert!(is_http(c.presence(1, &Secret::new("OLD"), &[1]).await.unwrap_err(), "400"));
}

#[test]
fn server_list_parsing_is_lenient() {
    // A real page: guests get fps and friendCount but no ping; players / playerTokens ignored.
    let list: ServerList = json(fixture("servers-v2-public-occupancyasc-limit100.json").as_bytes()).unwrap();
    let rows = list.data.unwrap();
    assert_eq!(rows.len(), 100);
    let first = &rows[0];
    assert_eq!((first.id.as_deref(), first.playing, first.max_players), (Some("037b10e2-cec9-45cd-a836-2929124af417"), Some(1), Some(35)));
    assert_eq!((first.fps, first.ping, first.friend_count), (Some(60.008926), None, Some(0)));
}

#[tokio::test]
async fn servers_public_drops_rows_without_id_or_counts() {
    let body = r#"{"nextPageCursor":"","gameJoinRestricted":false,"data":[
        {"id":"aaaaaaaa-0000-4000-8000-000000000001","maxPlayers":20,"playing":3,"fps":null,"ping":87,
         "friendCount":2,"pingSignalLevel":"Low","isOfficiallyLicensed":true,"players":[{"id":1}]},
        {"maxPlayers":20,"playing":3},
        {"id":null,"maxPlayers":20,"playing":3},
        {"id":"","maxPlayers":20,"playing":3},
        {"id":"aaaaaaaa-0000-4000-8000-000000000002","playing":3},
        {"id":"aaaaaaaa-0000-4000-8000-000000000003","maxPlayers":20},
        {"id":"aaaaaaaa-0000-4000-8000-000000000004","maxPlayers":8,"playing":0}]}"#;
    let (base, _reqs) = serve(vec![ok(body), ok(r#"{"data":null}"#)]);
    let (c, _) = client(&base);
    let page = c.servers_public(0, None, 920587237, ServerOrder::OccupancyDesc, false, 10, None).await.unwrap();
    let full = GameServer {
        id: "aaaaaaaa-0000-4000-8000-000000000001".into(),
        playing: 3,
        max_players: 20,
        fps: None,
        ping: Some(87),
        friend_count: Some(2),
    };
    let bare = GameServer {
        id: "aaaaaaaa-0000-4000-8000-000000000004".into(),
        playing: 0,
        max_players: 8,
        fps: None,
        ping: None,
        friend_count: None,
    };
    assert_eq!(page, ServerPage { servers: vec![full, bare], next_cursor: None });
    let empty = c.servers_public(0, None, 920587237, ServerOrder::OccupancyDesc, false, 10, None).await.unwrap();
    assert_eq!(empty, ServerPage { servers: vec![], next_cursor: None });
}

// Game info and private servers.

#[tokio::test]
async fn place_universe_reads_the_id_or_null() {
    let (base, reqs) = serve(vec![
        ok(&fixture("servers-games-fc-universe-2753.json")),
        ok(&fixture("servers-games-fc-universe-invalid.json")),
    ]);
    let (c, _) = client(&base);
    assert_eq!(c.place_universe(2753915549).await.unwrap(), Some(994732206));
    let req = reqs.recv().unwrap();
    assert_eq!(first_line(&req), "GET /universes/v1/places/2753915549/universe HTTP/1.1");
    assert!(!has(&req, "\r\ncookie:"));
    assert_eq!(c.place_universe(1).await.unwrap(), None);
    // No such place id at all: no call (a third would fail as Network).
    assert_eq!(c.place_universe(0).await.unwrap(), None);
    assert_eq!(c.place_universe(-5).await.unwrap(), None);
}

#[tokio::test]
async fn games_read_the_browser_fields_leniently() {
    let partial = r#"{"data":[{"id":7,"rootPlaceId":70,"name":null,"creator":null,"playing":null,
        "createVipServersAllowed":true,"universeAvatarType":3},
        {"rootPlaceId":80,"name":"no id"},{"id":9,"name":"no root place"}]}"#;
    let (base, reqs) = serve(vec![ok(&fixture("servers-games-fc-games-2u.json")), ok(partial)]);
    let (c, _) = client(&base);

    let games = c.games(&[994732206, 383310974, 994732206, 0]).await.unwrap();
    let blox = GameDetails {
        universe_id: 994732206,
        root_place_id: 2753915549,
        name: "Blox Fruits".into(),
        creator: "Gamer Robot Inc".into(),
        playing: 193096,
        visits: 65079604670,
        max_players: 12,
        private_servers: false,
        favourites: 20119446,
        created: Some("2019-01-16T03:29:19.433Z".into()),
        updated: Some("2026-10-22T17:28:11.337Z".into()),
        genre: Some("RPG".into()),
        subgenre: Some("Action RPG".into()),
        group_id: Some(4372130),
        avatar_type: Some("MorphToR15".into()),
        copying: Some(false),
    };
    assert_eq!(games[0], blox);
    assert_eq!((games[1].root_place_id, games[1].creator.as_str(), games[1].max_players), (920587237, "Uplift Games", 35));
    let req = reqs.recv().unwrap();
    assert_eq!(first_line(&req), "GET /v1/games?universeIds=383310974,994732206 HTTP/1.1");
    assert!(!has(&req, "\r\ncookie:"));

    // Nulls default, rows without id or root place are dropped.
    let bare = GameDetails { universe_id: 7, root_place_id: 70, private_servers: true, ..Default::default() };
    assert_eq!(c.games(&[7, 8, 9]).await.unwrap(), [bare]);
    assert!(c.games(&[]).await.unwrap().is_empty());
}

#[tokio::test]
async fn games_chunk_at_50_and_root_places_build_on_them() {
    let (base, reqs) = serve(vec![ok(r#"{"data":[{"id":1,"rootPlaceId":10,"name":"One"}]}"#), ok(r#"{"data":[]}"#)]);
    let (c, _) = client(&base);
    let ids: Vec<i64> = (1..=51).collect();
    assert_eq!(c.universe_root_places(&ids).await.unwrap(), HashMap::from([(1, (10, "One".to_string()))]));
    let want = |r: std::ops::RangeInclusive<i64>| format!("GET /v1/games?universeIds={} HTTP/1.1", csv(&r.collect::<Vec<_>>()));
    assert_eq!(first_line(&reqs.recv().unwrap()), want(1..=50));
    assert_eq!(first_line(&reqs.recv().unwrap()), want(51..=51));
}

#[tokio::test]
async fn place_icons_match_by_target_id_and_keep_only_completed() {
    let mixed = r#"{"data":[{"targetId":30,"state":"Completed","imageUrl":"https://tr.rbxcdn.com/c"},
        {"targetId":10,"state":"Pending","imageUrl":""},{"targetId":20,"state":"Blocked","imageUrl":"https://tr.rbxcdn.com/p"},
        {"targetId":40,"state":"Completed","imageUrl":""},{"targetId":50,"state":"Error","imageUrl":null}]}"#;
    let (base, reqs) = serve(vec![ok(&fixture("servers-games-fc-place-gameicons.json")), ok(mixed)]);
    let (c, _) = client(&base);

    let icons = c.place_icons(&[920587237, 2753915549]).await.unwrap();
    assert_eq!(icons.len(), 2);
    assert!(icons[&2753915549].starts_with("https://tr.rbxcdn.com/180DAY-774ec145"));
    assert!(icons[&920587237].starts_with("https://tr.rbxcdn.com/180DAY-4dd71502"));
    let req = reqs.recv().unwrap();
    let query = "returnPolicy=PlaceHolder&size=150x150&format=Png&isCircular=false";
    assert_eq!(first_line(&req), format!("GET /v1/places/gameicons?placeIds=920587237,2753915549&{query} HTTP/1.1"));
    assert!(!has(&req, "\r\ncookie:"));

    // Pending is not retried here: no icon is shown instead.
    let icons = c.place_icons(&[10, 20, 30, 40, 50]).await.unwrap();
    assert_eq!(icons, HashMap::from([(30, "https://tr.rbxcdn.com/c".to_string())]));
}

const ACCESS: &str = "0f1e2d3c-4b5a-4968-8776-655443322110";

#[tokio::test]
async fn private_servers_send_the_cookie_and_keep_only_guid_secrets() {
    use crate::model::{PrivateServer, PrivateServerPage};
    let body = format!(
        r#"{{"previousPageCursor":null,"nextPageCursor":"NEXT+/=","gameJoinRestricted":true,"data":[
        {{"id":"{JOB}","maxPlayers":12,"playing":3,"playerTokens":["t"],"players":[],"fps":59.9,"ping":40,
          "name":"Mine","vipServerId":111,"accessCode":"{ACCESS}","owner":{{"id":5,"name":"me","displayName":"Me"}}}},
        {{"maxPlayers":12,"playing":0,"name":"Closed","vipServerId":222,"owner":{{"id":6,"name":"friend"}}}},
        {{"id":"not-a-guid","name":"Odd","vipServerId":333,"accessCode":"1+placelauncherurl:x"}},
        {{"name":"No vip id","accessCode":"{ACCESS}"}}]}}"#
    );
    let last = r#"{"nextPageCursor":"","data":null}"#;
    let (base, reqs) = serve(vec![ok(&body), ok(last)]);
    let (c, _) = client(&base);
    let cookie = Secret::new("OLD");

    let page = c.private_servers(4, &cookie, 920587237, Some("abc+/=")).await.unwrap();
    let server = |vip_server_id, name: &str, owner: Option<(i64, &str)>, (playing, max_players), job: Option<&str>, code: Option<&str>| {
        PrivateServer {
            vip_server_id,
            name: name.into(),
            owner_id: owner.map(|o| o.0),
            owner_name: owner.map(|o| o.1.into()),
            playing,
            max_players,
            job_id: job.map(Into::into),
            access_code: code.map(Into::into),
        }
    };
    let want = vec![
        server(111, "Mine", Some((5, "me")), (3, 12), Some(JOB), Some(ACCESS)),
        server(222, "Closed", Some((6, "friend")), (0, 12), None, None),
        server(333, "Odd", None, (0, 0), None, None),
    ];
    assert_eq!(page, PrivateServerPage { servers: want, next_cursor: Some("NEXT+/=".into()), join_restricted: true });
    let req = reqs.recv().unwrap();
    assert_eq!(first_line(&req), "GET /v1/games/920587237/private-servers?sortOrder=Desc&excludeFullGames=false&cursor=abc%2B%2F%3D HTTP/1.1");
    assert!(has(&req, "\r\ncookie: .ROBLOSECURITY=OLD\r\n"));

    // First page: no query at all; an empty next cursor is the last page.
    let page = c.private_servers(4, &cookie, 920587237, Some("")).await.unwrap();
    assert_eq!(page, PrivateServerPage { servers: vec![], next_cursor: None, join_restricted: false });
    assert_eq!(first_line(&reqs.recv().unwrap()), "GET /v1/games/920587237/private-servers?sortOrder=Desc&excludeFullGames=false HTTP/1.1");
}

#[tokio::test]
async fn private_servers_without_a_valid_cookie_is_cookie_invalid() {
    let (base, _reqs) = serve(vec![reply("401 Unauthorized", &[], &fixture("private-servers-unauth.json"))]);
    let (c, _) = client(&base);
    let r = c.private_servers(4, &Secret::new("OLD"), 920587237, None).await;
    assert!(matches!(r, Err(AppError::CookieInvalid)));
}

#[tokio::test]
async fn vip_server_reads_leniently_and_never_keeps_secrets() {
    use crate::model::{AllowedUser, PrivateServerSettings};
    let body = format!(
        r#"{{"id":9,"name":"Mine","game":{{"id":7,"name":"G","rootPlace":{{"id":70}}}},"joinCode":"{ACCESS}","active":true,
        "link":"https://www.roblox.com/share?code={ACCESS}","accessCode":"{ACCESS}",
        "subscription":{{"active":false,"expired":false,"expirationDate":"2026-12-01T00:00:00Z","price":100,"canRenew":true}},
        "permissions":{{"clanAllowed":false,"friendsAllowed":true,"users":[{{"id":3,"name":"alt","displayName":"Alt"}},{{"name":"no id"}}]}}}}"#
    );
    let (base, reqs) = serve(vec![ok(&body), ok("{}")]);
    let (c, _) = client(&base);
    let cookie = Secret::new("OLD");

    let s = c.vip_server(4, &cookie, 9).await.unwrap();
    let want = PrivateServerSettings {
        vip_server_id: 9,
        name: "Mine".into(),
        active: true,
        friends_allowed: true,
        allowed_users: vec![AllowedUser { user_id: 3, username: "alt".into(), display_name: "Alt".into() }],
        renews: false,
        expired: false,
        expires: Some("2026-12-01T00:00:00Z".into()),
        price: Some(100),
    };
    assert_eq!(s, want);
    assert!(!serde_json::to_string(&s).unwrap().contains(ACCESS));
    let req = reqs.recv().unwrap();
    assert_eq!(first_line(&req), "GET /v1/vip-servers/9 HTTP/1.1");
    assert!(has(&req, "\r\ncookie: .ROBLOSECURITY=OLD\r\n"));

    // Nothing but the id known: everything off / empty.
    let bare = c.vip_server(4, &cookie, 9).await.unwrap();
    assert_eq!(bare, PrivateServerSettings { name: "".into(), active: false, friends_allowed: false, allowed_users: vec![], expires: None, price: None, ..want });
}

#[tokio::test]
async fn vip_server_update_sends_only_the_set_parts_after_a_csrf_retry() {
    use crate::model::PrivateServerPatch;
    let (base, reqs) = serve(vec![csrf_403(), ok("{}"), ok("{}"), ok("{}")]);
    let (c, _) = client(&base);
    let cookie = Secret::new("OLD");

    let patch = PrivateServerPatch {
        name: Some("  New name ".into()),
        new_join_code: Some(true),
        users_to_add: Some(vec![3, 0, 3]),
        users_to_remove: Some(vec![]),
        ..Default::default()
    };
    c.vip_server_update(4, &cookie, 9, &patch).await.unwrap();
    let (first, retried, permissions) = (reqs.recv().unwrap(), reqs.recv().unwrap(), reqs.recv().unwrap());
    assert!(!has(&first, "x-csrf-token"));
    assert_eq!(first_line(&retried), "PATCH /v1/vip-servers/9 HTTP/1.1");
    assert!(has(&retried, "\r\nx-csrf-token: TOK\r\n"));
    assert_eq!(body_of(&retried), serde_json::json!({ "name": "New name", "newJoinCode": true }));
    assert_eq!(first_line(&permissions), "PATCH /v1/vip-servers/9/permissions HTTP/1.1");
    assert_eq!(body_of(&permissions), serde_json::json!({ "usersToAdd": [3] }));

    // Only the server part set: no permissions call. `newJoinCode: false` is no change.
    let patch = PrivateServerPatch { active: Some(false), new_join_code: Some(false), ..Default::default() };
    c.vip_server_update(4, &cookie, 9, &patch).await.unwrap();
    let req = reqs.recv().unwrap();
    assert_eq!((first_line(&req), body_of(&req)), ("PATCH /v1/vip-servers/9 HTTP/1.1", serde_json::json!({ "active": false })));
}

#[tokio::test]
async fn vip_server_update_without_changes_or_with_a_blank_name_makes_no_call() {
    use crate::model::PrivateServerPatch;
    // No canned answer: any call fails as Network.
    let (base, _reqs) = serve(vec![]);
    let (c, _) = client(&base);
    let cookie = Secret::new("OLD");
    let empty = PrivateServerPatch { users_to_add: Some(vec![0]), new_join_code: Some(false), ..Default::default() };
    c.vip_server_update(4, &cookie, 9, &empty).await.unwrap();
    let blank = PrivateServerPatch { name: Some(" \t ".into()), friends_allowed: Some(true), ..Default::default() };
    assert!(matches!(c.vip_server_update(4, &cookie, 9, &blank).await, Err(AppError::Invalid(_))));
    assert!(matches!(c.vip_server_buy(4, &cookie, 7, "  ", 100, ACCESS).await, Err(AppError::Invalid(_))));
    assert!(matches!(c.vip_server_buy(4, &cookie, 7, "Mine", 100, "").await, Err(AppError::Internal(_))));
}

#[tokio::test]
async fn vip_server_subscription_and_buy_bodies() {
    let (base, reqs) = serve(vec![ok("{}"), ok("{}"), ok(&format!(r#"{{"id":9,"joinCode":"{ACCESS}"}}"#))]);
    let (c, _) = client(&base);
    let cookie = Secret::new("OLD");

    c.vip_server_subscription(4, &cookie, 9, true, 100).await.unwrap();
    c.vip_server_subscription(4, &cookie, 9, false, 100).await.unwrap();
    for active in [true, false] {
        let req = reqs.recv().unwrap();
        assert_eq!(first_line(&req), "PATCH /v1/vip-servers/9/subscription HTTP/1.1");
        assert_eq!(body_of(&req), serde_json::json!({ "active": active, "price": 100 }));
    }

    c.vip_server_buy(4, &cookie, 7, " Mine ", 100, ACCESS).await.unwrap();
    let req = reqs.recv().unwrap();
    assert_eq!(first_line(&req), "POST /v1/games/vip-servers/7 HTTP/1.1");
    assert!(has(&req, "\r\ncookie: .ROBLOSECURITY=OLD\r\n"));
    let want = serde_json::json!({ "name": "Mine", "expectedPrice": 100, "isPurchaseConfirmed": true, "idempotencyKey": ACCESS });
    assert_eq!(body_of(&req), want);
}

#[tokio::test]
async fn vip_server_refusals_map_to_cookie_invalid_and_forbidden() {
    let not_owner = || reply("403 Forbidden", &[], r#"{"errors":[{"code":0,"message":"Authorization has been denied for this request."}]}"#);
    let (base, _reqs) = serve(vec![reply("401 Unauthorized", &[], &fixture("private-servers-unauth.json")), not_owner(), not_owner()]);
    let (c, _) = client(&base);
    let cookie = Secret::new("OLD");
    assert!(matches!(c.vip_server(4, &cookie, 9).await, Err(AppError::CookieInvalid)));
    // A 403 without a CSRF token, even with Roblox's text: not the owner (the command words it so).
    assert!(c.vip_server_subscription(4, &cookie, 9, false, 100).await.unwrap_err().is_forbidden());
    assert!(c.vip_server(4, &cookie, 9).await.unwrap_err().is_forbidden());
}

#[tokio::test]
async fn vip_server_refusals_quote_robloxs_own_text() {
    let (base, _reqs) = serve(vec![
        reply("400 Bad Request", &[], r#"{"errors":[{"code":12,"message":"The price\n is wrong."}]}"#),
        reply("400 Bad Request", &[], "{}"),
        reply("403 Forbidden", &[], r#"{"errors":[{"code":2,"message":"Not friends."}]}"#),
        reply("500 Internal Server Error", &[], r#"{"errors":[{"message":"Oops"}]}"#),
    ]);
    let (c, _) = client(&base);
    let cookie = Secret::new("OLD");
    let text = |r: crate::error::Result<()>| match r {
        Err(AppError::Invalid(m)) => m,
        other => panic!("{:?}", other.err()),
    };
    assert!(text(c.vip_server_buy(4, &cookie, 7, "Mine", 99, ACCESS).await).ends_with("The price is wrong."));
    // No text: the purchase still reads as refused, not as "HTTP 400".
    assert!(text(c.vip_server_buy(4, &cookie, 7, "Mine", 99, ACCESS).await).contains("charged nothing"));
    let patch = crate::model::PrivateServerPatch { users_to_add: Some(vec![5]), ..Default::default() };
    assert!(text(c.vip_server_update(4, &cookie, 9, &patch).await).ends_with("Not friends."));
    // A 5xx may have gone through: it stays a Roblox error for the UI to treat as unsure.
    assert!(matches!(c.vip_server_buy(4, &cookie, 7, "Mine", 100, ACCESS).await, Err(AppError::Roblox(_))));
}

#[test]
fn images_only_from_roblox_cdn() {
    assert!(super::is_rbxcdn("https://tr.rbxcdn.com/180DAY-abc/150/150/Image/Png/noFilter"));
    for url in ["http://tr.rbxcdn.com/x", "https://rbxcdn.com.evil.example/x", "https://evil.example/tr.rbxcdn.com", "", "javascript:alert(1)"] {
        assert!(!super::is_rbxcdn(url), "{url}");
    }
}

#[test]
fn client_version_answer() {
    for name in ["multiinstance-client-version-windowsplayer.json", "multiinstance-client-version-windowsplayer-channel-live.json"] {
        assert_eq!(client_version_of(fixture(name).as_bytes()).unwrap(), "version-9ac8595712b447a7", "{name}");
    }
    for bad in [
        r#"{"clientVersionUpload":"version-9AC8595712B447A7"}"#,
        r#"{"clientVersionUpload":"version-9ac8595712b447a"}"#,
        r#"{"clientVersionUpload":"../../evil"}"#,
        r#"{"clientVersionUpload":""}"#,
        r#"{"version":"0.744.0.7441080"}"#,
        "not json",
    ] {
        assert!(client_version_of(bad.as_bytes()).is_err(), "{bad}");
    }
}

#[test]
fn a_huge_rate_limit_window_neither_panics_nor_stalls() {
    let c = Client::new(Arc::new(|_, _| {}));
    c.set_window("r", 0, u64::MAX);
    let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
    assert!(matches!(rt.block_on(c.wait_turn("r")), Err(AppError::RateLimited(_))));
}

#[test]
fn popular_keeps_game_lists_without_ads() {
    // Trimmed from a live get-sorts answer (2026-11-08): a filters row, two game lists.
    let page: types::ExploreSorts = serde_json::from_str(
        r#"{"sorts":[
            {"contentType":"Filters","sortId":"filters_v5","sortDisplayName":"","filters":[]},
            {"contentType":"Games","sortId":"top-trending","sortDisplayName":"Top Trending","games":[
                {"universeId":994732206,"rootPlaceId":2753915549,"name":"Blox Fruits","playerCount":338925,"totalUpVotes":10,"totalDownVotes":2,"isSponsored":false},
                {"universeId":5,"rootPlaceId":6,"name":"An ad","playerCount":1,"isSponsored":true},
                {"universeId":null,"rootPlaceId":7,"name":"No id"}
            ]},
            {"contentType":"Games","sortId":"empty","sortDisplayName":"Only ads","games":[{"universeId":8,"rootPlaceId":9,"isSponsored":true}]}
        ],"nextSortsPageToken":"x"}"#,
    )
    .unwrap();
    let sorts = popular_of(page);
    assert_eq!(sorts.len(), 1, "filters and lists left empty go");
    assert_eq!((sorts[0].id.as_str(), sorts[0].name.as_str()), ("top-trending", "Top Trending"));
    assert_eq!(sorts[0].games.len(), 1, "no ads, no rows without ids");
    let g = &sorts[0].games[0];
    assert_eq!((g.universe_id, g.root_place_id, g.playing, g.up_votes, g.down_votes), (994732206, 2753915549, 338925, 10, 2));
    assert_eq!(g.icon_url, None);
}

#[tokio::test]
async fn game_votes_match_by_universe() {
    let votes = r#"{"data":[{"id":9,"upVotes":1,"downVotes":2},{"id":383310974,"upVotes":9623620,"downVotes":1516142}]}"#;
    let (base, reqs) = serve(vec![ok(votes), ok(r#"{"data":[]}"#)]);
    let (c, _) = client(&base);
    assert_eq!(c.game_votes(383310974).await.unwrap(), Some((9623620, 1516142)));
    let req = reqs.recv().unwrap();
    assert_eq!(first_line(&req), "GET /v1/games/votes?universeIds=383310974 HTTP/1.1");
    assert!(!has(&req, "\r\ncookie:"));
    assert_eq!(c.game_votes(383310974).await.unwrap(), None, "a universe Roblox doesn't list");
}

#[tokio::test]
async fn game_passes_page_by_token_with_icons() {
    let first = r#"{"gamePasses":[{"id":1,"name":"vip","displayName":"VIP","isForSale":true,"price":480},
        {"id":2,"name":"Old","isForSale":false,"price":null},{"id":4,"name":"Priced off sale","isForSale":false,"price":50},
        {"name":"no id","price":5}],"nextPageToken":"id_2z/+="}"#;
    let second = r#"{"gamePasses":[{"id":3,"displayName":"Griffin","isForSale":true,"price":99}],"nextPageToken":""}"#;
    let icons = r#"{"data":[{"targetId":3,"state":"Completed","imageUrl":"https://tr.rbxcdn.com/3"},
        {"targetId":1,"state":"Completed","imageUrl":"https://tr.rbxcdn.com/1"}]}"#;
    let (base, reqs) = serve(vec![ok(first), ok(second), ok(icons)]);
    let (c, _) = client(&base);
    let list = c.game_passes(383310974).await.unwrap();
    let pass = |id, name: &str, price, icon: Option<&str>| crate::model::GamePass { id, name: name.into(), price, icon_url: icon.map(Into::into) };
    assert_eq!(
        list.items,
        [pass(1, "VIP", Some(480), Some("https://tr.rbxcdn.com/1")), pass(2, "Old", None, None), pass(4, "Priced off sale", None, None), pass(3, "Griffin", Some(99), Some("https://tr.rbxcdn.com/3"))]
    );
    assert!(!list.more && list.complete);
    let req = reqs.recv().unwrap();
    assert_eq!(first_line(&req), "GET /game-passes/v1/universes/383310974/game-passes?passView=Full&pageSize=100 HTTP/1.1");
    assert!(has(&req, "\r\norigin: https://www.roblox.com") && !has(&req, "\r\ncookie:"));
    let req = reqs.recv().unwrap();
    assert_eq!(first_line(&req), "GET /game-passes/v1/universes/383310974/game-passes?passView=Full&pageSize=100&pageToken=id_2z%2F%2B%3D HTTP/1.1");
    assert_eq!(first_line(&reqs.recv().unwrap()), "GET /v1/game-passes?gamePassIds=1,2,3,4&size=150x150&format=Png HTTP/1.1");
}

#[tokio::test]
async fn badges_stop_at_the_cap_and_keep_win_rate_as_fraction() {
    let page = |n: u32| {
        ok(&format!(
            r#"{{"nextPageCursor":"c{n}","data":[{{"id":{n},"name":"b{n}","displayName":"Badge {n}","enabled":true,
                "statistics":{{"pastDayAwardedCount":3,"awardedCount":1563733,"winRatePercentage":0.0015}}}},
                {{"id":{},"name":"off","enabled":false}},{{"id":{},"name":"bare"}}]}}"#,
            n + 100,
            n + 200
        ))
    };
    let mut responses: Vec<String> = (1..=5).map(page).collect();
    responses.push(ok(r#"{"data":[]}"#));
    let (base, reqs) = serve(responses);
    let (c, _) = client(&base);
    let list = c.game_badges(1).await.unwrap();
    assert!(list.more && list.complete, "stopped at the cap: more, but nothing failed");
    assert_eq!(list.items.len(), 10, "5 pages of an enabled badge and a bare one; disabled ones left out");
    let b = &list.items[0];
    assert_eq!((b.id, b.name.as_str(), b.awarded, b.awarded_yesterday, b.win_rate), (1, "Badge 1", 1563733, 3, 0.0015));
    let bare = &list.items[1];
    assert_eq!((bare.name.as_str(), bare.awarded, bare.win_rate), ("bare", 0, 0.0));
    assert_eq!(first_line(&reqs.recv().unwrap()), "GET /v1/universes/1/badges?limit=100&sortOrder=Asc HTTP/1.1");
    assert_eq!(first_line(&reqs.recv().unwrap()), "GET /v1/universes/1/badges?limit=100&sortOrder=Asc&cursor=c1 HTTP/1.1");
}

#[tokio::test]
async fn a_later_page_failing_keeps_what_was_read() {
    let first = r#"{"nextPageCursor":"next","data":[{"assetId":1029025,"name":"Fedora","recentAveragePrice":431023}]}"#;
    let (base, _reqs) = serve(vec![ok(first), reply("500 Internal Server Error", &[], "{}"), reply("500 Internal Server Error", &[], "{}")]);
    let (c, _) = client(&base);
    let cookie = Secret::new("C");
    let list = c.collectibles(3, &cookie, 156).await.unwrap();
    assert_eq!(list.items.len(), 1);
    assert!(list.more && !list.complete, "cut short: not complete");
    assert!(c.collectibles(3, &cookie, 156).await.is_err(), "the first page failing is the error");
}

#[tokio::test]
async fn collectibles_send_the_cookie_and_add_up_copies() {
    let first = r#"{"nextPageCursor":"21748508_1_79","data":[
        {"userAssetId":1,"assetId":1029025,"name":"The Classic ROBLOX Fedora","recentAveragePrice":431023},
        {"userAssetId":2,"assetId":1365767,"name":"Valkyrie Helm","recentAveragePrice":null},
        {"userAssetId":3,"name":"no asset id","recentAveragePrice":5}]}"#;
    let second = r#"{"nextPageCursor":null,"data":[{"userAssetId":4,"assetId":1029025,"name":"The Classic ROBLOX Fedora","recentAveragePrice":431023}]}"#;
    let (base, reqs) = serve(vec![ok(first), ok(second)]);
    let (c, _) = client(&base);
    let list = c.collectibles(3, &Secret::new("C"), 156).await.unwrap();
    assert!(!list.more && list.complete);
    assert_eq!(
        list.items,
        [
            Holding { asset_id: 1029025, name: "The Classic ROBLOX Fedora".into(), copies: 2, rap: 431023 },
            Holding { asset_id: 1365767, name: "Valkyrie Helm".into(), copies: 1, rap: 0 },
        ]
    );
    let req = reqs.recv().unwrap();
    assert_eq!(first_line(&req), "GET /v1/users/156/assets/collectibles?limit=100&sortOrder=Asc HTTP/1.1");
    assert!(has(&req, "\r\ncookie: .ROBLOSECURITY=C"));
    assert_eq!(first_line(&reqs.recv().unwrap()), "GET /v1/users/156/assets/collectibles?limit=100&sortOrder=Asc&cursor=21748508_1_79 HTTP/1.1");
}

#[tokio::test]
async fn a_hidden_inventory_is_told_apart() {
    let hidden = r#"{"errors":[{"code":11,"message":"You don't have permissions to view the specified user's inventory."}]}"#;
    let (base, _reqs) = serve(vec![reply("403 Forbidden", &[], hidden), reply("500 Internal Server Error", &[], "{}"), reply("500 Internal Server Error", &[], "{}")]);
    let (c, _) = client(&base);
    let e = c.collectibles(3, &Secret::new("C"), 2).await.unwrap_err();
    assert!(super::is_private_inventory(&e), "{e:?}");
    let e = c.collectibles(3, &Secret::new("C"), 2).await.unwrap_err();
    assert!(!super::is_private_inventory(&e), "a failure is not a hidden inventory: {e:?}");
}

#[tokio::test]
async fn age_rating_posts_the_universe_and_keeps_what_it_contains() {
    let body = r#"{"ageRecommendationDetails":{"summary":{"ageRecommendation":{"displayName":"Mild","minimumAge":9}},
        "descriptorUsages":[{"descriptor":{"displayName":"Violence (Repeated/Mild)"},"contains":true},
        {"descriptor":{"displayName":"Blood"},"contains":false},{"descriptor":null}]}}"#;
    let (base, reqs) = serve(vec![ok(body), ok("{}")]);
    let (c, _) = client(&base);
    let r = c.age_rating(994732206).await.unwrap().unwrap();
    assert_eq!((r.label.as_str(), r.minimum_age, r.descriptors), ("Mild", Some(9), vec!["Violence (Repeated/Mild)".to_string()]));
    let req = reqs.recv().unwrap();
    assert!(first_line(&req).starts_with("POST /experience-guidelines-api/experience-guidelines/get-age-recommendation"));
    assert!(has(&req, "\r\norigin: https://www.roblox.com") && req.contains(r#"{"universeId":"994732206"}"#));
    assert_eq!(c.age_rating(1).await.unwrap(), None, "no rating: none");
}

#[tokio::test]
async fn a_players_groups_badges_and_games_read_leniently() {
    let groups = r#"{"data":[{"group":{"id":9,"name":"Small","memberCount":10,"hasVerifiedBadge":false},"role":{"id":1,"name":"Member","rank":1}},
        {"group":{"id":8,"name":"Big","memberCount":1000,"hasVerifiedBadge":true},"role":{"id":2,"name":"Owner","rank":255}},{"group":null}]}"#;
    let badges = r#"[{"id":1,"name":"Veteran","description":"old","imageUrl":"https://images.rbxcdn.com/v.png"},{"id":2,"name":"Odd","imageUrl":"https://evil.example/x.png"},{"name":"no id"}]"#;
    let games = r#"{"nextPageCursor":null,"data":[{"id":5,"name":"Obby","rootPlace":{"id":50},"placeVisits":24854},{"id":6,"name":"no place"}]}"#;
    let (base, reqs) = serve(vec![ok(groups), ok(badges), ok(games)]);
    let (c, _) = client(&base);
    let g = c.user_groups(156).await.unwrap();
    assert_eq!(g.iter().map(|g| (g.name.as_str(), g.role.as_str(), g.members)).collect::<Vec<_>>(), [("Big", "Owner", 1000), ("Small", "Member", 10)]);
    assert_eq!(first_line(&reqs.recv().unwrap()), "GET /v2/users/156/groups/roles HTTP/1.1");
    let b = c.roblox_badges(156).await.unwrap();
    assert_eq!(b.iter().map(|b| (b.id, b.image_url.is_some())).collect::<Vec<_>>(), [(1, true), (2, false)], "icons only from Roblox's CDN");
    let list = c.user_games(156).await.unwrap();
    assert!(list.complete && !list.more);
    assert_eq!(list.items, [crate::model::PlayerGame { universe_id: 5, place_id: 50, name: "Obby".into(), visits: 24854 }]);
}
#[tokio::test]
async fn an_icon_still_being_made_leaves_the_answer_incomplete() {
    let pending = r#"{"data":[{"targetId":1,"state":"Completed","imageUrl":"https://tr.rbxcdn.com/1"},{"targetId":2,"state":"Pending","imageUrl":""}]}"#;
    let (base, _reqs) = serve(vec![ok(pending), ok(r#"{"data":[{"targetId":1,"state":"Completed","imageUrl":"https://tr.rbxcdn.com/1"}]}"#)]);
    let (c, _) = client(&base);
    let (icons, complete) = c.game_icons(&[1, 2]).await;
    assert_eq!((icons.len(), complete), (1, false), "asked again next time, not kept");
    assert_eq!(c.game_icons(&[1]).await, (HashMap::from([(1, "https://tr.rbxcdn.com/1".to_string())]), true));
}