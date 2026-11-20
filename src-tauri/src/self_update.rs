//! RoALT's own updates (Roblox's are `launcher::update`).
//!
//! - Check: `GET api.github.com/repos/inspectsoftware/RoALT/releases/latest` (published releases
//!   only, never a draft or prerelease) at start and every 6 h while `self_update` is on,
//!   or on Check now. A `vX.Y.Z` tag newer than this build (`newer`: never the same or older)
//!   with the assets `RoALT-Setup-X.Y.Z.exe` and `.exe.sig` is an update; `app://update` says so.
//! - Install (`install`, only on the user's click): asks GitHub again, downloads both (https only,
//!   from github.com, redirected only to GitHub's asset hosts; ≤ 200 MB and ≤ 4 KB) into a fresh
//!   temp folder, write-locks the installer, reads it back through that handle and checks the
//!   ECDSA P-256 / SHA-256 signature of `RoALT-Setup-X.Y.Z.exe\n` followed by exactly those bytes
//!   against `PUBLIC_KEY` (Windows CNG): the version is signed too, so an older installer can't
//!   pass as a newer one. Only then, and if no launch or Roblox install started meanwhile, runs it
//!   silently with `/RELAUNCH` (RoALT.iss starts RoALT again afterwards) and
//!   quits, so its files can be replaced. The folder is left to the installer; the next start
//!   deletes it.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use reqwest::Url;
use reqwest::header::ACCEPT;
use reqwest::redirect::Policy;
use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager};
use windows::Win32::Security::Cryptography::{
    BCRYPT_ALG_HANDLE, BCRYPT_ECCPUBLIC_BLOB, BCRYPT_ECDSA_P256_ALGORITHM, BCRYPT_ECDSA_PUBLIC_P256_MAGIC, BCRYPT_FLAGS,
    BCRYPT_KEY_HANDLE, BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS, BCRYPT_SHA256_ALG_HANDLE, BCryptHash, BCryptImportKeyPair,
    BCryptOpenAlgorithmProvider, BCryptVerifySignature,
};
use windows::core::{Owned, PCWSTR};

use crate::App;
use crate::error::{AppError, Result};
use crate::launcher::update::{FILE_SHARE_READ, TempDir, blocking};
use crate::model::AppUpdate;

pub const UPDATE_EVENT: &str = "app://update";
const CURRENT: &str = env!("CARGO_PKG_VERSION");
/// An indev build (`build-installer.ps1 -Dev`): its code is newer than the published release its
/// version trails, so it never offers that release, which would replace it with older code.
const DEV: bool = option_env!("ROALT_DEV").is_some();
const LATEST: &str = "https://api.github.com/repos/inspectsoftware/RoALT/releases/latest";
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const MAX_JSON: u64 = 1_000_000;
const MAX_INSTALLER: u64 = 200_000_000;
const MAX_SIG: u64 = 4_096;
/// Where release assets may come from: github.com, then GitHub's redirect to its storage.
const ASSET_HOSTS: [&str; 3] = ["github.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com"];
const TEMP_PREFIX: &str = "roalt-update-";
/// Inno Setup's silent install; `/RELAUNCH` is RoALT.iss's own: start RoALT when done.
const INSTALLER_ARGS: [&str; 5] = ["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS", "/RELAUNCH"];
/// The release signing key (P-256, uncompressed X‖Y): its private half is the `ROALT_UPDATE_KEY`
/// secret of the release workflow, never in this repository.
const PUBLIC_KEY: &str = "75e2530761806230914528bcedce3565121808e8576bf27eb82864485f7e5cc3031aecb007e08d37cb04bd67809c9b06c901ce058794e536ea6b71a406908623";

#[derive(Default)]
pub struct SelfUpdater {
    /// The newer version the last check found.
    available: Mutex<Option<String>>,
    installing: AtomicBool,
}

impl SelfUpdater {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn installing(&self) -> bool {
        self.installing.load(Ordering::SeqCst)
    }
}

/// A newer release and where its signed installer is.
#[derive(Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub exe: String,
    pub sig: String,
}

#[derive(Deserialize)]
struct Latest {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// `X.Y.Z` (numbers without leading zeros), optionally `-prerelease`, as an ordering key: a
/// prerelease sorts before its release. None for anything else (build metadata included).
fn version_key(s: &str) -> Option<(u64, u64, u64, bool)> {
    let (core, pre) = match s.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (s, None),
    };
    if pre.is_some_and(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')) {
        return None;
    }
    let num = |p: &str| {
        (!p.is_empty() && p.len() <= 9 && p.bytes().all(|b| b.is_ascii_digit()) && (p == "0" || !p.starts_with('0')))
            .then(|| p.parse().ok())
            .flatten()
    };
    let mut parts = core.split('.');
    let key = (num(parts.next()?)?, num(parts.next()?)?, num(parts.next()?)?, pre.is_none());
    parts.next().is_none().then_some(key)
}

/// The version of `tag` (`vX.Y.Z`, no prerelease) when it is newer than `current`; never the same
/// or an older one, nor anything when `current` can't be read.
pub fn newer(current: &str, tag: &str) -> Option<String> {
    let v = tag.strip_prefix('v')?;
    let new = version_key(v).filter(|k| k.3)?;
    (new > version_key(current)?).then(|| v.to_owned())
}

/// `url` is https on the default port, without credentials, on one of `hosts`.
fn allowed(url: &Url, hosts: &[&str]) -> bool {
    url.scheme() == "https"
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.host_str().is_some_and(|h| hosts.contains(&h))
}

/// The update in a releases/latest answer for a build at `current`: None when there is nothing
/// newer; Err when there is, but without its installer and signature on github.com.
pub fn pick(body: &[u8], current: &str) -> Result<Option<Release>> {
    let l: Latest = serde_json::from_slice(body).map_err(|e| {
        log::debug!("update: GitHub answer unreadable: {e}");
        failed(tr!("GitHub's answer couldn't be read."))
    })?;
    let Some(version) = newer(current, &l.tag_name).filter(|_| !l.draft && !l.prerelease) else {
        return Ok(None);
    };
    let url = |name: String| {
        l.assets
            .iter()
            .find(|a| a.name == name)
            .map(|a| a.browser_download_url.clone())
            .filter(|u| {
                let ok = Url::parse(u).is_ok_and(|u| allowed(&u, &ASSET_HOSTS[..1]));
                if !ok {
                    log::debug!("update: asset {name} has a URL that isn't allowed");
                }
                ok
            })
    };
    match (url(format!("RoALT-Setup-{version}.exe")), url(format!("RoALT-Setup-{version}.exe.sig"))) {
        (Some(exe), Some(sig)) => Ok(Some(Release { version, exe, sig })),
        _ => Err(failed(tr!("RoALT {version} is out, but without a signed installer. Get it from GitHub Releases.", version = version))),
    }
}

/// Standard base64 with `=` padding (surrounding whitespace ignored); None when malformed.
fn base64(text: &str) -> Option<Vec<u8>> {
    let s = text.trim().as_bytes();
    if !s.len().is_multiple_of(4) {
        return None;
    }
    let val = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    };
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let last = s.len() / 4;
    for (i, chunk) in s.chunks(4).enumerate() {
        let pad = chunk.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 || (pad > 0 && i + 1 != last) {
            return None;
        }
        let mut n = 0u32;
        for &c in &chunk[..4 - pad] {
            n = n << 6 | u32::from(val(c)?);
        }
        n <<= 6 * pad as u32;
        out.extend_from_slice(&n.to_be_bytes()[1..4 - pad]);
    }
    Some(out)
}

fn public_key() -> [u8; 64] {
    let mut k = [0; 64];
    for (i, b) in k.iter_mut().enumerate() {
        *b = u8::from_str_radix(&PUBLIC_KEY[2 * i..2 * i + 2], 16).expect("PUBLIC_KEY is 64 bytes of hex");
    }
    k
}

/// True only when `sig` (IEEE P1363 r‖s, 64 bytes) is a valid ECDSA P-256 signature of the
/// SHA-256 of `data` by `public` (uncompressed X‖Y), checked with Windows CNG.
fn verify(public: &[u8; 64], data: &[u8], sig: &[u8]) -> bool {
    if sig.len() != 64 {
        return false;
    }
    // BCRYPT_ECCKEY_BLOB { dwMagic, cbKey } and then X, Y.
    let mut blob = Vec::with_capacity(72);
    blob.extend_from_slice(&BCRYPT_ECDSA_PUBLIC_P256_MAGIC.to_le_bytes());
    blob.extend_from_slice(&32u32.to_le_bytes());
    blob.extend_from_slice(public);
    let mut hash = [0u8; 32];
    // SAFETY: every buffer is a live slice passed with its own length; the algorithm and key
    // handles are only used while their `Owned` wrappers (closed on drop, key first) live.
    unsafe {
        let st = BCryptHash(BCRYPT_SHA256_ALG_HANDLE, None, data, &mut hash);
        if st.is_err() {
            log::warn!("update signature: BCryptHash failed: {:#010x}", st.0);
            return false;
        }
        let mut alg = BCRYPT_ALG_HANDLE::default();
        let st = BCryptOpenAlgorithmProvider(&mut alg, BCRYPT_ECDSA_P256_ALGORITHM, PCWSTR::null(), BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0));
        if st.is_err() {
            log::warn!("update signature: BCryptOpenAlgorithmProvider failed: {:#010x}", st.0);
            return false;
        }
        let alg = Owned::new(alg);
        let mut key = BCRYPT_KEY_HANDLE::default();
        let st = BCryptImportKeyPair(*alg, None, BCRYPT_ECCPUBLIC_BLOB, &mut key, &blob, 0);
        if st.is_err() {
            log::warn!("update signature: BCryptImportKeyPair failed: {:#010x}", st.0);
            return false;
        }
        let key = Owned::new(key);
        BCryptVerifySignature(*key, None, &hash, sig, BCRYPT_FLAGS(0)).is_ok()
    }
}

/// What the release workflow signs for `version`: its installer's name and a newline, then the
/// installer's bytes (appended by the caller).
fn signed_prefix(version: &str) -> Vec<u8> {
    format!("RoALT-Setup-{version}.exe\n").into_bytes()
}

/// Writes `bytes` to the new file `path`, reopens it so nothing can write or delete it while the
/// handle lives, and checks the signature of `version` and what the file holds, read back through
/// that handle: the bytes checked are the bytes that run. The handle is returned to keep the lock.
fn write_verified(path: &Path, version: &str, bytes: &[u8], sig: &[u8], key: &[u8; 64]) -> Result<File> {
    File::create_new(path)?.write_all(bytes)?;
    let mut pinned = OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(path)?;
    let mut signed = signed_prefix(version);
    signed.reserve(bytes.len());
    pinned.read_to_end(&mut signed)?;
    if !verify(key, &signed, sig) {
        return Err(AppError::Invalid(tr!("The downloaded RoALT installer was not run: its signature didn't check out.")));
    }
    Ok(pinned)
}

fn failed(why: String) -> AppError {
    AppError::Internal(why)
}

/// No path or query (asset URLs carry signed tokens): what went wrong, with the host.
fn network(e: reqwest::Error) -> AppError {
    let e = e.without_url();
    log::warn!("update: GitHub request failed: {e}");
    let what = if e.is_timeout() {
        tr!("timed out")
    } else if e.is_connect() {
        tr!("could not connect")
    } else {
        tr!("connection failed")
    };
    failed(tr!("Couldn't reach GitHub ({what}).", what = what))
}

fn http() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("RoALT/", env!("CARGO_PKG_VERSION")))
        .https_only(true)
        .redirect(Policy::custom(|a| {
            if a.previous().len() >= 5 {
                a.error("too many redirects")
            } else if allowed(a.url(), &ASSET_HOSTS) {
                a.follow()
            } else {
                a.stop()
            }
        }))
        .connect_timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| {
            log::warn!("update: http client unavailable: {}", e.without_url());
            failed(tr!("RoALT's HTTP client couldn't start."))
        })
}

/// GETs `url` within `timeout`; refused over `max` bytes (by `Content-Length`, or while reading).
async fn get(http: &reqwest::Client, url: &str, accept: &'static str, max: u64, timeout: Duration) -> Result<Vec<u8>> {
    let mut resp = http.get(url).header(ACCEPT, accept).timeout(timeout).send().await.map_err(network)?;
    if !resp.status().is_success() {
        log::warn!("update: GitHub answered HTTP {}", resp.status().as_u16());
        return Err(failed(tr!("GitHub answered HTTP {status}.", status = resp.status().as_u16())));
    }
    let too_big = || {
        log::warn!("update: GitHub sent more than {max} bytes");
        failed(tr!("GitHub sent more than {kb} KB.", kb = max / 1_000))
    };
    if resp.content_length().is_some_and(|t| t > max) {
        return Err(too_big());
    }
    let mut out = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(network)? {
        if (out.len() + chunk.len()) as u64 > max {
            return Err(too_big());
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

/// The newer release on GitHub, if any (never for an indev build, `DEV`).
async fn latest(http: &reqwest::Client) -> Result<Option<Release>> {
    if DEV {
        return Ok(None);
    }
    let body = get(http, LATEST, "application/vnd.github+json", MAX_JSON, Duration::from_secs(20)).await?;
    pick(&body, CURRENT)
}

/// This build's version and the newer one found last (no network).
pub fn status(s: &App) -> AppUpdate {
    AppUpdate { current: CURRENT.into(), available: lock(&s.self_update.available).clone() }
}

/// Asks GitHub now.
pub async fn check(app: &AppHandle) -> Result<AppUpdate> {
    let available = latest(&http()?).await?.map(|r| r.version);
    let s = app.state::<App>();
    *lock(&s.self_update.available) = available;
    Ok(status(&s))
}

/// After the switch changed: off forgets (and hides) what was found, on checks at once.
pub fn switched(app: &AppHandle, on: bool) {
    if !on {
        *lock(&app.state::<App>().self_update.available) = None;
        let _ = app.emit(UPDATE_EVENT, status(&app.state::<App>()));
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match check(&app).await {
            Ok(u) => {
                let _ = app.emit(UPDATE_EVENT, &u);
            }
            Err(e) => log::info!("RoALT update check failed: {e}"),
        }
    });
}

/// Tells Discord of `version` once (which one was told is kept, so a restart doesn't say it again).
fn told_discord(app: &AppHandle, version: &str) {
    const KEY: &str = "discordToldUpdate";
    let db = &app.state::<App>().db;
    if db.setting_value(KEY).ok().flatten().as_deref() == Some(version) {
        return;
    }
    // Not queued (no webhook, switched off, vault locked): the next check tries again.
    if crate::discord::notify(app, crate::discord::Event::Update { version: version.to_owned() })
        && let Err(e) = db.set_setting_value(KEY, Some(version))
    {
        log::debug!("the told update isn't remembered: {e}");
    }
}

/// Deletes what earlier updates left in %TEMP%, then checks at start and every 6 hours
/// while `settings.self_update` is on; `app://update` when there is a newer version.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // First thing at start: no delay before the first check.
        tauri::async_runtime::spawn_blocking(sweep);
        loop {
            let on = app
                .state::<App>()
                .db
                .settings()
                .inspect_err(|e| log::warn!("update check skipped, settings unreadable: {e}"))
                .is_ok_and(|v| v.self_update);
            if on {
                match check(&app).await {
                    Ok(u) if u.available.is_some() => {
                        let _ = app.emit(UPDATE_EVENT, &u);
                        told_discord(&app, u.available.as_deref().unwrap_or_default());
                    }
                    Ok(_) => {}
                    Err(e) => log::info!("RoALT update check failed: {e}"),
                }
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

fn sweep() {
    let dirs = match std::fs::read_dir(std::env::temp_dir()) {
        Ok(d) => d,
        Err(e) => {
            log::debug!("update: temp dir unreadable: {e}");
            return;
        }
    };
    for d in dirs.flatten().filter(|d| d.file_name().to_string_lossy().starts_with(TEMP_PREFIX)) {
        if let Err(e) = std::fs::remove_dir_all(d.path()) {
            log::info!("could not delete {}: {e}", d.path().display());
        }
    }
}

/// Downloads, verifies and runs the newer release's installer, then quits (see the module notes).
/// Refused while a launch, batch, auto-rejoin or Roblox install runs, and when nothing is newer.
pub async fn install(app: &AppHandle) -> Result<()> {
    let s = app.state::<App>();
    s.self_update
        .installing
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .map_err(|_| AppError::Invalid(tr!("RoALT is already being updated.")))?;
    let result = run_install(&s).await;
    match &result {
        Ok(()) => app.exit(0),
        Err(e) => {
            log::warn!("RoALT update failed: {e}");
            s.self_update.installing.store(false, Ordering::SeqCst);
        }
    }
    result
}

/// Refused while a launch, batch, auto-rejoin or Roblox install runs: RoALT quits for the installer.
fn idle(s: &App) -> Result<()> {
    if s.bulk.any_busy() {
        return Err(AppError::Invalid(tr!("Wait for the running launch, batch or auto-rejoin to finish, then update RoALT.")));
    }
    if s.update.installing() {
        return Err(AppError::Invalid(tr!("Wait for Roblox's installer to finish, then update RoALT.")));
    }
    Ok(())
}

async fn run_install(s: &App) -> Result<()> {
    idle(s)?;
    let http = http()?;
    let release = latest(&http).await?.ok_or_else(|| AppError::Invalid(tr!("RoALT is up to date.")))?;
    *lock(&s.self_update.available) = Some(release.version.clone());
    let refused = || AppError::Invalid(tr!("The downloaded RoALT installer was not run: its signature file is malformed."));
    let sig = get(&http, &release.sig, "application/octet-stream", MAX_SIG, Duration::from_secs(30)).await?;
    let sig = std::str::from_utf8(&sig).ok().and_then(base64).filter(|s| s.len() == 64).ok_or_else(refused)?;
    let bytes = get(&http, &release.exe, "application/octet-stream", MAX_INSTALLER, Duration::from_secs(600)).await?;

    let dir = TempDir::new(TEMP_PREFIX)?;
    let exe = dir.0.join(format!("RoALT-Setup-{}.exe", release.version));
    let (checked, version) = (exe.clone(), release.version.clone());
    let pinned = blocking(move || write_verified(&checked, &version, &bytes, &sig, &public_key())).await?;
    // Again: one may have started during the download, and quitting now would cut it off.
    idle(s)?;
    log::info!("running RoALT {}'s installer (signature checked)", release.version);
    Command::new(&exe)
        .args(INSTALLER_ARGS)
        .current_dir(&dir.0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| AppError::Internal(tr!("Could not start RoALT's installer: {why}", why = e)))?;
    // The installer runs from the folder: kept (write-locked until RoALT exits) for the next start
    // to delete.
    std::mem::forget((pinned, dir));
    Ok(())
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    #[test]
    fn versions() {
        for (current, tag, want) in [
            ("0.5.1", "v0.5.2", Some("0.5.2")),
            ("0.5.1", "v0.6.0", Some("0.6.0")),
            ("0.5.1", "v1.0.0", Some("1.0.0")),
            ("0.9.9", "v0.10.0", Some("0.10.0")),
            ("0.6.0-beta.1", "v0.6.0", Some("0.6.0")),
            ("0.5.1", "v0.5.1", None),
            ("0.5.1", "v0.5.0", None),
            ("0.10.0", "v0.9.9", None),
            ("0.6.0", "v0.6.0-beta.2", None),
            ("0.5.1", "v0.7.0-rc.1", None),
            ("0.5.1", "0.6.0", None),
            ("0.5.1", "V0.6.0", None),
            ("0.5.1", "v0.6", None),
            ("0.5.1", "v0.6.0.1", None),
            ("0.5.1", "v0.06.0", None),
            ("0.5.1", "v0.6.0+build", None),
            ("0.5.1", "v 0.6.0", None),
            ("0.5.1", "v0.6.x", None),
            ("0.5.1", "v-1.6.0", None),
            ("0.5.1", "v99999999999.0.0", None),
            ("", "v0.6.0", None),
            ("garbage", "v0.6.0", None),
        ] {
            assert_eq!(newer(current, tag).as_deref(), want, "{current} → {tag}");
        }
        assert!(version_key(CURRENT).is_some(), "this build's own version reads");
    }

    fn fixture() -> Value {
        let path = format!("{}/../docs/research/fixtures/github-releases-latest.json", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn with_sig(mut v: Value) -> Value {
        let mut sig = v["assets"][0].clone();
        sig["name"] = "RoALT-Setup-0.5.1.exe.sig".into();
        sig["browser_download_url"] = "https://github.com/inspectsoftware/RoALT/releases/download/v0.5.1/RoALT-Setup-0.5.1.exe.sig".into();
        v["assets"].as_array_mut().unwrap().push(sig);
        v
    }

    fn body(v: &Value) -> Vec<u8> {
        serde_json::to_vec(v).unwrap()
    }

    /// A recorded releases/latest answer (v0.5.1, which has no `.sig` yet).
    #[test]
    fn picks_the_signed_installer() {
        let recorded = fixture();
        assert_eq!(pick(&body(&recorded), "0.5.1").unwrap(), None, "same version");
        assert_eq!(pick(&body(&recorded), "0.6.0").unwrap(), None, "older");
        let e = pick(&body(&recorded), "0.5.0").unwrap_err().to_string();
        assert!(e.contains("without a signed installer"), "{e}");

        let signed = with_sig(recorded);
        assert_eq!(
            pick(&body(&signed), "0.5.0").unwrap(),
            Some(Release {
                version: "0.5.1".into(),
                exe: "https://github.com/inspectsoftware/RoALT/releases/download/v0.5.1/RoALT-Setup-0.5.1.exe".into(),
                sig: "https://github.com/inspectsoftware/RoALT/releases/download/v0.5.1/RoALT-Setup-0.5.1.exe.sig".into(),
            })
        );
        for flag in ["draft", "prerelease"] {
            let mut v = signed.clone();
            v[flag] = true.into();
            assert_eq!(pick(&body(&v), "0.5.0").unwrap(), None, "{flag}");
        }
        // Asset names must match exactly, and come from github.com over https.
        for (field, value) in [
            ("name", "roalt-setup-0.5.1.exe"),
            ("browser_download_url", "http://github.com/inspectsoftware/RoALT/releases/download/v0.5.1/RoALT-Setup-0.5.1.exe"),
            ("browser_download_url", "https://evil.example/RoALT-Setup-0.5.1.exe"),
            ("browser_download_url", "https://github.com.evil.example/RoALT-Setup-0.5.1.exe"),
            ("browser_download_url", "https://github.com:8443/RoALT-Setup-0.5.1.exe"),
            ("browser_download_url", "https://user@github.com/RoALT-Setup-0.5.1.exe"),
            ("browser_download_url", "https://objects.githubusercontent.com/RoALT-Setup-0.5.1.exe"),
        ] {
            let mut v = signed.clone();
            v["assets"][0][field] = value.into();
            assert!(pick(&body(&v), "0.5.0").is_err(), "{field} = {value}");
        }
        assert!(pick(b"<html>", "0.5.0").is_err());
    }

    #[test]
    fn redirect_hosts() {
        let ok = |u: &str| allowed(&Url::parse(u).unwrap(), &ASSET_HOSTS);
        assert!(ok("https://github.com/a"));
        assert!(ok("https://objects.githubusercontent.com/a?sig=x"));
        assert!(ok("https://release-assets.githubusercontent.com/a"));
        assert!(!ok("http://release-assets.githubusercontent.com/a"));
        assert!(!ok("https://githubusercontent.com/a"));
        assert!(!ok("https://evil.objects.githubusercontent.com/a"));
        assert!(!ok("https://api.github.com/a"));
    }

    #[test]
    fn base64_decodes_strictly() {
        assert_eq!(base64("").unwrap(), b"");
        assert_eq!(base64("Zg==").unwrap(), b"f");
        assert_eq!(base64("Zm8=").unwrap(), b"fo");
        assert_eq!(base64("Zm9v").unwrap(), b"foo");
        assert_eq!(base64(" Zm9vYmFy\r\n").unwrap(), b"foobar");
        assert_eq!(base64("+/+/").unwrap(), [0xfb, 0xff, 0xbf]);
        for bad in ["Zg=", "Zg", "Z===", "Zg==Zg==", "Zm 9v", "Zm-v", "Zm_v", "A=AA"] {
            assert_eq!(base64(bad), None, "{bad:?}");
        }
    }

    /// A throwaway key made with Node for this test only (crypto.generateKeyPairSync("ec",
    /// P-256), crypto.sign("sha256", MESSAGE, { dsaEncoding: "ieee-p1363" })), as release.yml
    /// signs `RoALT-Setup-0.6.0.exe\n` + DATA.
    const TEST_KEY: &str = "1bb1dec977d50faa29ab7620751690b318bd88e35f0b2d851d74f6440b34eacdc64b254b1782dede62cfc67f513e25d2f426e29e26e95c23a50d09d981be94e7";
    const MESSAGE: &[u8] = b"RoALT-Setup-0.6.0.exe\nRoALT self-update test vector";
    const DATA: &[u8] = b"RoALT self-update test vector";
    const SIGNATURE: &str = "LB7tyBAyRL8yzNJpASkt8mSei6PK6hRkdGh0YjzcF9UDsTqFmE2x7R9ejg5theR7L4bEFxa8O45e62NRjaySWg==";

    fn test_key() -> [u8; 64] {
        let mut k = [0; 64];
        for (i, b) in k.iter_mut().enumerate() {
            *b = u8::from_str_radix(&TEST_KEY[2 * i..2 * i + 2], 16).unwrap();
        }
        k
    }

    #[test]
    fn signatures() {
        let (key, sig) = (test_key(), base64(SIGNATURE).unwrap());
        assert_eq!([signed_prefix("0.6.0"), DATA.to_vec()].concat(), MESSAGE);
        assert!(verify(&key, MESSAGE, &sig));
        assert!(!verify(&key, b"RoALT-Setup-0.6.0.exe\nRoALT self-update test vectoR", &sig), "other data");
        assert!(!verify(&key, DATA, &sig), "without the version");
        let mut flipped = sig.clone();
        flipped[10] ^= 1;
        assert!(!verify(&key, MESSAGE, &flipped), "changed signature");
        assert!(!verify(&key, MESSAGE, &sig[..63]), "short signature");
        assert!(!verify(&key, MESSAGE, &[0; 64]), "zero signature");
        assert!(!verify(&public_key(), MESSAGE, &sig), "the release key didn't sign it");
        let mut other = key;
        other[63] ^= 1;
        assert!(!verify(&other, MESSAGE, &sig), "not a point on the curve");
    }

    #[test]
    fn the_file_is_checked_as_written_and_stays_locked() {
        let dir = TempDir::new("roalt-update-test-").unwrap();
        let sig = base64(SIGNATURE).unwrap();
        let exe = dir.0.join("setup.exe");
        assert!(write_verified(&exe, "0.6.0", DATA, &sig, &public_key()).is_err(), "signed by the test key, not the release key");
        assert!(write_verified(&exe, "0.6.0", DATA, &sig, &test_key()).is_err(), "never over an existing file");
        let renamed = dir.0.join("renamed.exe");
        assert!(write_verified(&renamed, "0.9.0", DATA, &sig, &test_key()).is_err(), "0.6.0's installer offered as 0.9.0");

        let good = dir.0.join("good.exe");
        let pinned = write_verified(&good, "0.6.0", DATA, &sig, &test_key()).unwrap();
        assert!(OpenOptions::new().write(true).open(&good).is_err(), "no writer while pinned");
        assert!(std::fs::remove_file(&good).is_err(), "no delete while pinned");
        drop(pinned);
        assert!(OpenOptions::new().write(true).open(&good).is_ok());
    }
}
