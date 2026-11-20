//! Rolimons' values for limiteds, only while `settings.rolimons_values` is on. While an account's
//! limiteds are counted, RoALT downloads Rolimons' public item list (one GET of
//! api.rolimons.com, at most every 30 minutes; no user id, name or cookie is sent) and keeps it in
//! the cache table. Rolimons isn't part of Roblox: values are their opinion, RAP stays Roblox's.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use reqwest::redirect::Policy;
use serde_json::Value;

use crate::model::RolimonsItem;
use crate::storage::Db;

const URL: &str = "https://api.rolimons.com/items/v2/itemdetails";
/// The list's row in the cache table, as downloaded.
const KEY: &str = "rolimons";
/// A list this old is downloaded again (when counting).
const FRESH_MS: i64 = 30 * 60_000;
/// After a failed download, none for this long.
const RETRY_MS: i64 = 5 * 60_000;
/// The list is about 200 KB.
const MAX_BODY: usize = 4_000_000;

/// Asset id → what Rolimons says about it.
pub type Items = HashMap<i64, RolimonsItem>;

#[derive(Default)]
struct State {
    /// When it was downloaded, and the list.
    items: Option<(i64, Arc<Items>)>,
    failed_at: i64,
}

/// Held across a download: two counts at once download once.
static STATE: LazyLock<tokio::sync::Mutex<State>> = LazyLock::new(Default::default);

/// The list RoALT holds (seeded from the cache table, at any age). With `fetch` (a count), one
/// older than 30 minutes is downloaded again first, unless a download failed in the last 5. None
/// when there is no list at all.
pub async fn items(db: &Db, now: i64, fetch: bool) -> Option<Arc<Items>> {
    items_in(&STATE, db, now, fetch, download).await
}

async fn items_in<F, Fut>(state: &tokio::sync::Mutex<State>, db: &Db, now: i64, fetch: bool, download: F) -> Option<Arc<Items>>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Option<String>>,
{
    let mut s = state.lock().await;
    if s.items.is_none()
        && let Ok(Some((body, at))) = db.cache_get(KEY, i64::MAX / 2, now)
        && let Some(items) = parse(&body)
    {
        s.items = Some((at, Arc::new(items)));
    }
    let stale = s.items.as_ref().is_none_or(|(at, _)| now - at >= FRESH_MS);
    if fetch && stale && now - s.failed_at >= RETRY_MS {
        match download().await.and_then(|body| parse(&body).map(|items| (body, items))) {
            Some((body, items)) => {
                if let Err(e) = db.cache_put(KEY, &body, now) {
                    log::warn!("rolimons: list not kept: {e}");
                }
                s.items = Some((now, Arc::new(items)));
            }
            None => s.failed_at = now,
        }
    }
    s.items.as_ref().map(|(_, items)| items.clone())
}

/// The list's body, or None (logged).
async fn download() -> Option<String> {
    let http = reqwest::Client::builder()
        .user_agent(concat!("RoALT/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build()
        .inspect_err(|e| log::warn!("rolimons: http client unavailable: {e}"))
        .ok()?;
    let mut resp = http.get(URL).send().await.map_err(|e| log::info!("rolimons: unreachable: {}", e.without_url())).ok()?;
    if !resp.status().is_success() {
        log::info!("rolimons: answered HTTP {}", resp.status().as_u16());
        return None;
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| log::info!("rolimons: cut off: {}", e.without_url())).ok()? {
        if out.len() + chunk.len() > MAX_BODY {
            log::info!("rolimons: sent more than {MAX_BODY} bytes");
            return None;
        }
        out.extend_from_slice(&chunk);
    }
    String::from_utf8(out).inspect_err(|_| log::info!("rolimons: sent text that isn't UTF-8")).ok()
}

/// `{success, items: {"<assetId>": [name, acronym, rap, value, default value, demand, trend,
/// projected, hyped, rare, ..]}}`, read by position: -1 is "none", flags are 1 or -1. Rows too
/// short are left out; None when it isn't a list, or most rows are unreadable (the format changed).
fn parse(body: &str) -> Option<Items> {
    let v: Value = serde_json::from_str(body).inspect_err(|e| log::info!("rolimons: list unreadable: {e}")).ok()?;
    if v.get("success").and_then(Value::as_bool) != Some(true) {
        log::info!("rolimons: the list says it failed");
        return None;
    }
    let rows = v.get("items")?.as_object()?;
    let items: Items = rows.iter().filter_map(|(id, row)| Some((id.parse::<i64>().ok()?, item(row.as_array()?)?))).collect();
    if items.len() * 2 < rows.len() {
        log::warn!("rolimons: {} of {} rows unreadable (format changed?)", rows.len() - items.len(), rows.len());
        return None;
    }
    Some(items)
}

fn item(row: &[Value]) -> Option<RolimonsItem> {
    if row.len() < 10 {
        return None;
    }
    let int = |i: usize| row[i].as_i64();
    let scale = |i: usize| int(i).filter(|n| (0..=4).contains(n)).map(|n| n as u8);
    Some(RolimonsItem {
        value: int(3).filter(|&n| n >= 0),
        demand: scale(5),
        trend: scale(6),
        projected: int(7) == Some(1),
        hyped: int(8) == Some(1),
        rare: int(9) == Some(1),
        acronym: row[1].as_str().filter(|s| !s.is_empty()).map(str::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = r#"{"success":true,"item_count":3,"items":{
        "1029025":["The Classic ROBLOX Fedora","CF",431023,450000,450000,4,2,-1,1,1,1],
        "1365767":["Valkyrie Helm","",263297,-1,263297,-1,9,1,-1,-1],
        "7":["Too short",""]}}"#;

    #[test]
    fn parse_reads_rows_by_position() {
        let items = parse(LIST).unwrap();
        assert_eq!(items.len(), 2, "the short row is left out");
        assert_eq!(
            items[&1029025],
            RolimonsItem { value: Some(450000), demand: Some(4), trend: Some(2), projected: false, hyped: true, rare: true, acronym: Some("CF".into()) }
        );
        let helm = &items[&1365767];
        assert_eq!((helm.value, helm.demand, helm.trend, helm.projected, helm.acronym.as_deref()), (None, None, None, true, None), "-1, out of range, empty");
        assert_eq!(parse(r#"{"success":false,"items":{}}"#), None);
        assert_eq!(parse(r#"{"success":true,"items":{"1":[1],"2":[2],"3":["a","",1,1,1,1,1,1,1,1]}}"#), None, "mostly unreadable");
        assert_eq!(parse("<html>"), None);
    }

    async fn never() -> Option<String> {
        panic!("downloaded when it shouldn't")
    }
    async fn fails() -> Option<String> {
        None
    }
    async fn fresh() -> Option<String> {
        Some(LIST.replace("431023", "777777"))
    }

    #[tokio::test]
    async fn items_without_fetch_never_calls_out() {
        let db = Db::open_in_memory().unwrap();
        let state = tokio::sync::Mutex::new(State::default());
        assert!(items_in(&state, &db, 1_000_000, false, never).await.is_none(), "nothing kept, nothing fetched");
        db.cache_put(KEY, LIST, 0).unwrap();
        let state = tokio::sync::Mutex::new(State::default());
        let now = 10 * FRESH_MS;
        assert_eq!(items_in(&state, &db, now, false, never).await.map(|i| i.len()), Some(2), "a kept list at any age");
        // A count: the old list is downloaded again; a failed download keeps it and waits.
        assert_eq!(items_in(&state, &db, now, true, fails).await.map(|i| i.len()), Some(2));
        assert!(items_in(&state, &db, now + 60_000, true, never).await.is_some(), "no retry within 5 minutes");
        assert!(items_in(&state, &db, now + RETRY_MS, true, fresh).await.is_some());
        assert!(items_in(&state, &db, now + RETRY_MS + 1, true, never).await.is_some(), "fresh for 30 minutes");
        assert!(db.cache_get(KEY, 1, now + RETRY_MS).unwrap().is_some_and(|(b, _)| b.contains("777777")), "kept in the cache table");
    }

    /// Against Rolimons: `cargo test rolimons::tests::live -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn live() {
        let items = parse(&download().await.unwrap()).unwrap();
        println!("rolimons: {} items; fedora {:?}", items.len(), items.get(&1029025));
        assert!(items.len() > 1000);
    }
}
