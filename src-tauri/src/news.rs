//! Home's News tab: RoALT's releases (GitHub), the DevForum's announcements and release notes, and
//! Roblox's newsroom. Fetched when the UI asks (at start and every 30 min); the sources load side by
//! side and one that fails is named in `failed` while the others still show. Every link is rebuilt
//! from an id or slug on the source's own host, so a page can't hand the UI some other address.

use std::time::Duration;

use reqwest::redirect::Policy;
use serde::Deserialize;

use crate::model::{News, NewsItem, NewsSource};

const RELEASES: &str = "https://api.github.com/repos/inspectsoftware/RoALT/releases?per_page=5";
const ANNOUNCEMENTS: &str = "https://devforum.roblox.com/c/updates/announcements/36.json";
const RELEASE_NOTES: &str = "https://devforum.roblox.com/c/updates/release-notes/62.json";
/// No feed: the page's article cards are read (`newsroom`).
const NEWSROOM: &str = "https://about.roblox.com/newsroom";
/// The newsroom page is about 0.6 MB.
const MAX_BODY: usize = 3_000_000;
/// Items kept from each source.
const PER_SOURCE: usize = 6;
/// A release's notes, cut beyond this many characters.
const MAX_NOTES: usize = 20_000;

/// Fetches every source at once; never fails as a whole.
pub async fn fetch() -> News {
    let http = match reqwest::Client::builder()
        .user_agent(concat!("RoALT/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            log::warn!("news: http client unavailable: {e}");
            return News { items: vec![], failed: vec![NewsSource::Roalt, NewsSource::Devforum, NewsSource::Newsroom] };
        }
    };
    let (releases, announcements, notes, newsroom) = tokio::join!(get(&http, RELEASES), get(&http, ANNOUNCEMENTS), get(&http, RELEASE_NOTES), get(&http, NEWSROOM));
    let mut items = Vec::new();
    let mut failed = Vec::new();
    let mut take = |source, got: Option<Vec<NewsItem>>| match got {
        Some(list) => items.extend(list),
        None => {
            if !failed.contains(&source) {
                failed.push(source);
            }
        }
    };
    take(NewsSource::Roalt, releases.as_deref().and_then(parse_releases));
    // One of the two DevForum categories is enough to show the DevForum.
    let forum: Vec<_> = [announcements, notes].iter().flatten().filter_map(|b| parse_devforum(b)).flatten().collect();
    take(NewsSource::Devforum, (!forum.is_empty()).then_some(forum));
    take(NewsSource::Newsroom, newsroom.as_deref().map(parse_newsroom).filter(|l| !l.is_empty()));
    // RFC 3339 and `YYYY-MM` sort together as text; a month-only item goes before that month's days.
    items.sort_by(|a, b| b.published.cmp(&a.published));
    News { items, failed }
}

/// The body of a 2xx answer, at most `MAX_BODY` bytes, as UTF-8; None (logged) otherwise.
async fn get(http: &reqwest::Client, url: &str) -> Option<String> {
    let host = url.split('/').nth(2).unwrap_or(url);
    let mut resp = http.get(url).send().await.map_err(|e| log::info!("news: {host} unreachable: {}", e.without_url())).ok()?;
    if !resp.status().is_success() {
        log::info!("news: {host} answered HTTP {}", resp.status().as_u16());
        return None;
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|e| log::info!("news: {host} cut off: {}", e.without_url())).ok()? {
        if out.len() + chunk.len() > MAX_BODY {
            log::info!("news: {host} sent more than {MAX_BODY} bytes");
            return None;
        }
        out.extend_from_slice(&chunk);
    }
    String::from_utf8(out).inspect_err(|_| log::info!("news: {host} sent text that isn't UTF-8")).ok()
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    published_at: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// `x.y.z`, digits and dots only.
fn plain_version(v: &str) -> bool {
    let parts: Vec<_> = v.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.len() <= 9 && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Published releases with a `vX.Y.Z` tag, newest first as GitHub lists them. None when unreadable.
fn parse_releases(body: &str) -> Option<Vec<NewsItem>> {
    let list: Vec<Release> = serde_json::from_str(body).inspect_err(|e| log::info!("news: GitHub's answer unreadable: {e}")).ok()?;
    Some(
        list.into_iter()
            .filter(|r| !r.draft && !r.prerelease)
            .filter_map(|r| {
                let version = r.tag_name.strip_prefix('v').filter(|v| plain_version(v))?.to_owned();
                Some(NewsItem {
                    id: format!("roalt:{version}"),
                    source: NewsSource::Roalt,
                    // By the tag, as the update offer names it (a release's name can differ: v0.9.2 is "RoALT 0.9.1a").
                    title: format!("RoALT {version}"),
                    summary: r.body.unwrap_or_default().chars().take(MAX_NOTES).collect(),
                    url: format!("https://github.com/inspectsoftware/RoALT/releases/tag/v{version}"),
                    published: r.published_at?,
                    version: Some(version),
                })
            })
            .take(PER_SOURCE)
            .collect(),
    )
}

#[derive(Deserialize)]
struct Category {
    topic_list: TopicList,
}

#[derive(Deserialize)]
struct TopicList {
    topics: Vec<Topic>,
}

#[derive(Deserialize)]
struct Topic {
    id: u64,
    title: String,
    slug: String,
    created_at: String,
    #[serde(default)]
    excerpt: Option<String>,
}

fn plain_slug(s: &str) -> bool {
    !s.is_empty() && s.len() <= 200 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A category's newest topics, without its "About the … category" post. None when unreadable.
fn parse_devforum(body: &str) -> Option<Vec<NewsItem>> {
    let c: Category = serde_json::from_str(body).inspect_err(|e| log::info!("news: DevForum's answer unreadable: {e}")).ok()?;
    let mut topics: Vec<_> = c.topic_list.topics.into_iter().filter(|t| !t.slug.starts_with("about-the-")).collect();
    // Listed by last reply; News goes by when each was posted.
    topics.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Some(
        topics
            .into_iter()
            .take(PER_SOURCE)
            .map(|t| NewsItem {
                id: format!("devforum:{}", t.id),
                source: NewsSource::Devforum,
                title: t.title.trim().to_owned(),
                summary: t.excerpt.as_deref().map(text).unwrap_or_default(),
                url: format!("https://devforum.roblox.com/t/{}/{}", if plain_slug(&t.slug) { &t.slug } else { "topic" }, t.id),
                published: t.created_at,
                version: None,
            })
            .collect(),
    )
}

/// The newsroom page's article cards, in page order: `<a href="/newsroom/YYYY/MM/slug" …>` holding
/// an `<h3>` title. Cards that don't read so are skipped; an empty list means the page changed.
fn parse_newsroom(html: &str) -> Vec<NewsItem> {
    const OPEN: &str = "<a href=\"/newsroom/";
    let mut out: Vec<NewsItem> = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find(OPEN) {
        rest = &rest[i + OPEN.len()..];
        let Some(end) = rest.find('"') else { break };
        let parts: Vec<_> = rest[..end].split('/').collect();
        let card = &rest[..rest.find("</a>").unwrap_or(rest.len())];
        let [year, month, slug] = parts[..] else { continue };
        let digits = |s: &str, n: usize| s.len() == n && s.bytes().all(|b| b.is_ascii_digit());
        if !digits(year, 4) || !digits(month, 2) || !plain_slug(slug) {
            continue;
        }
        let title = card.find("<h3").and_then(|h| {
            let s = &card[h..];
            Some(text(&s[s.find('>')? + 1..s.find("</h3>")?]))
        });
        let Some(title) = title.filter(|t| !t.is_empty()) else { continue };
        let id = format!("newsroom:{year}/{month}/{slug}");
        if out.iter().any(|n| n.id == id) {
            continue;
        }
        out.push(NewsItem {
            id,
            source: NewsSource::Newsroom,
            title,
            summary: String::new(),
            url: format!("https://about.roblox.com/newsroom/{year}/{month}/{slug}"),
            published: format!("{year}-{month}"),
            version: None,
        });
        if out.len() == PER_SOURCE {
            break;
        }
    }
    out
}

/// HTML to plain text: tags dropped, the common entities decoded, runs of white space made one space.
fn text(html: &str) -> String {
    let mut plain = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                plain.push(' ');
            }
            _ if !in_tag => plain.push(c),
            _ => {}
        }
    }
    let mut out = String::with_capacity(plain.len());
    let mut rest = plain.as_str();
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let decoded = rest.find(';').filter(|&e| e <= 10).and_then(|e| entity(&rest[1..e]).map(|c| (c, e)));
        match decoded {
            Some((c, e)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn entity(name: &str) -> Option<char> {
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "hellip" => '…',
        "rsquo" => '’',
        "lsquo" => '‘',
        "rdquo" => '”',
        "ldquo" => '“',
        "mdash" => '—',
        "ndash" => '–',
        _ => {
            let n = name.strip_prefix('#')?;
            let code = match n.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => n.parse().ok()?,
            };
            char::from_u32(code)?
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn releases() {
        let body = r###"[
            {"tag_name":"v0.10.0","name":"RoALT 0.9.9a","body":"## New\n- Macros","published_at":"2026-11-05T10:00:00Z"},
            {"tag_name":"v0.9.9","name":"","body":null,"published_at":"2026-11-04T10:00:00Z","draft":true},
            {"tag_name":"v0.9.8-rc.1","published_at":"2026-11-03T10:00:00Z"},
            {"tag_name":"v0.9.7","name":"  ","published_at":"2026-11-02T10:00:00Z","prerelease":true},
            {"tag_name":"0.9.6","published_at":"2026-11-01T10:00:00Z"},
            {"tag_name":"v0.9.5","published_at":null},
            {"tag_name":"v0.9.4","name":null,"published_at":"2026-10-30T10:00:00Z"}
        ]"###;
        let items = parse_releases(body).unwrap();
        assert_eq!(items.len(), 2, "drafts, prereleases, odd tags and undated ones are left out");
        assert_eq!(items[0].title, "RoALT 0.10.0", "by the tag, not the release's name");
        assert_eq!(items[0].summary, "## New\n- Macros");
        assert_eq!(items[0].url, "https://github.com/inspectsoftware/RoALT/releases/tag/v0.10.0");
        assert_eq!(items[0].version.as_deref(), Some("0.10.0"));
        assert_eq!(items[1].id, "roalt:0.9.4");
        assert!(parse_releases("<html>").is_none());
    }

    #[test]
    fn devforum() {
        let body = r#"{"topic_list":{"topics":[
            {"id":1,"title":"About the Announcements category","slug":"about-the-announcements-category","created_at":"2021-01-01T08:00:00.000Z","pinned":true},
            {"id":30,"title":"Older","slug":"older","created_at":"2026-11-01T00:00:00.000Z","excerpt":"Hi &amp; bye&hellip;"},
            {"id":40,"title":" Newer ","slug":"newer-one","created_at":"2026-11-04T00:00:00.000Z"},
            {"id":50,"title":"Odd slug","slug":"../../evil","created_at":"2026-10-01T00:00:00.000Z"}
        ]}}"#;
        let items = parse_devforum(body).unwrap();
        assert_eq!(items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["devforum:40", "devforum:30", "devforum:50"]);
        assert_eq!(items[0].title, "Newer");
        assert_eq!(items[0].url, "https://devforum.roblox.com/t/newer-one/40");
        assert_eq!(items[1].summary, "Hi & bye…");
        assert_eq!(items[2].url, "https://devforum.roblox.com/t/topic/50", "a slug that isn't plain is not used");
        assert!(parse_devforum("{}").is_none());
    }

    #[test]
    fn newsroom() {
        let html = r#"<a href="/newsroom/2026/10/the-hollow" class="card"><h4>Community</h4><h3 class="t">The Hollow: New &amp; Spooky</h3></a>
            <a href="/newsroom/2026/10/the-hollow" class="card"><h3>Again</h3></a>
            <a href="/newsroom/2026/9/bad-month" class="card"><h3>Bad</h3></a>
            <a href="/newsroom/2026/09/no-title" class="card"><img src="x"></a>
            <a href="/newsroom/2026/09/Upper" class="card"><h3>Upper</h3></a>
            <a href="/newsroom/2026/09/fine-one" class="card"><h3> Fine <em>one</em> </h3></a>
            <a href="/newsroom/category/x" class="card"><h3>Category</h3></a>"#;
        let items = parse_newsroom(html);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "The Hollow: New & Spooky");
        assert_eq!(items[0].url, "https://about.roblox.com/newsroom/2026/10/the-hollow");
        assert_eq!(items[0].published, "2026-10");
        assert_eq!(items[1].title, "Fine one");
        assert!(parse_newsroom("<html></html>").is_empty());
    }

    /// Against the real sites (the newsroom is read from its page, which can change):
    /// `cargo test news::tests::live -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn live() {
        let news = fetch().await;
        for i in &news.items {
            println!("{:?} {} | {} | {}", i.source, i.published, i.title, i.url);
        }
        assert!(news.failed.is_empty(), "failed: {:?}", news.failed);
    }

    #[test]
    fn plain_text() {
        assert_eq!(text("a&nbsp;b &#39;c&#x27; &bogus; & d"), "a b 'c' &bogus; & d");
        assert_eq!(text("<p>Hi</p><p>there</p>"), "Hi there");
        assert_eq!(text("&#xFFFFFFFF;"), "&#xFFFFFFFF;");
    }
}
