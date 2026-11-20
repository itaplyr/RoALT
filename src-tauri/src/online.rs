//! How many people use RoALT now, and how many accounts they're playing, shown beside the Discord
//! button while `settings.online_count` is on. Each ping sends only a random id made once per
//! launch and how many Roblox clients RoALT is running to RoALT's counter (a Cloudflare Worker,
//! `server/online-counter`), which answers how many ids it saw lately, their clients added up,
//! and when to ping next.

use std::sync::LazyLock;

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

const ONLINE_URL: &str = "https://roalt-online.roalt-counterlive.workers.dev/ping";

/// One per launch: the counter tells runs apart, never people.
static LAUNCH_ID: LazyLock<Uuid> = LazyLock::new(Uuid::new_v4);

/// The counter's answer, passed on to the UI as it is.
#[derive(Serialize, Deserialize, TS, Clone, Debug, PartialEq, Eq)]
#[ts(export)]
pub struct Online {
    /// RoALTs running now.
    pub online: u32,
    /// Their Roblox clients added up; None from a counter older than this field.
    #[serde(default)]
    pub playing: Option<u32>,
    /// Seconds until the next ping, as the counter asks; None from a counter older than this
    /// field (then every 5 minutes). The UI keeps it within 1 to 60 minutes.
    #[serde(default)]
    pub every: Option<u32>,
}

/// Tells the counter this RoALT runs `playing` clients. The counter's answer, or None (logged)
/// when it can't be reached or read.
pub async fn ping(playing: usize) -> Option<Online> {
    let http = crate::http::client("online")?;
    let sent = http.post(ONLINE_URL).json(&serde_json::json!({ "id": *LAUNCH_ID, "playing": playing })).send().await;
    // The answer is under 100 bytes.
    parse(&crate::http::text("online", sent, 10_000).await?)
}

fn parse(body: &str) -> Option<Online> {
    serde_json::from_str(body).inspect_err(|e| log::info!("online: counter's answer unreadable: {e}")).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answer() {
        assert_eq!(parse(r#"{"online":128,"playing":540,"every":300}"#), Some(Online { online: 128, playing: Some(540), every: Some(300) }));
        assert_eq!(parse(r#"{"online":128}"#), Some(Online { online: 128, playing: None, every: None }), "an older counter");
        assert_eq!(parse(r#"{"online":-1}"#), None);
        assert_eq!(parse("Not found"), None);
        assert_eq!(serde_json::json!({ "id": *LAUNCH_ID })["id"].as_str().map(str::len), Some(36), "a plain hyphenated uuid");
    }

    /// Against the deployed counter: `cargo test online::tests::live -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn live() {
        let n = ping(0).await;
        println!("online: {n:?}");
        assert!(n.is_some_and(|n| n.online >= 1));
    }
}
