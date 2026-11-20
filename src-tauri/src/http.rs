//! Plain HTTPS for the hosts that aren't Roblox's API: News' sources, Rolimons and RoALT's counter.

use std::time::Duration;

use reqwest::redirect::Policy;

/// HTTPS only, no redirects, named `RoALT/<version>`. None (logged under `who`) when it can't be built.
pub fn client(who: &str) -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("RoALT/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build()
        .inspect_err(|e| log::warn!("{who}: http client unavailable: {e}"))
        .ok()
}

/// The body of a 2xx answer, at most `max` bytes, as UTF-8; None (logged under `who`) otherwise.
pub async fn text(who: &str, sent: reqwest::Result<reqwest::Response>, max: usize) -> Option<String> {
    let mut resp = sent.map_err(|e| log::info!("{who}: unreachable: {}", e.without_url())).ok()?;
    if !resp.status().is_success() {
        log::info!("{who}: answered HTTP {}", resp.status().as_u16());
        return None;
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| log::info!("{who}: cut off: {}", e.without_url())).ok()? {
        if out.len() + chunk.len() > max {
            log::info!("{who}: sent more than {max} bytes");
            return None;
        }
        out.extend_from_slice(&chunk);
    }
    String::from_utf8(out).inspect_err(|_| log::info!("{who}: sent text that isn't UTF-8")).ok()
}
