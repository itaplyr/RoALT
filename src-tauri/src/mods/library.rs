//! The imported mods: each in `<mods>\library\<uuid>\` with RoALT's own `mod.json`; their order
//! (top first, the top one wins a file two mods change) and on/off beside the settings, under the
//! `mods` key. A mod never changes once imported: a new version is a new import.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::pack::{self, META_FILE, Meta};
use crate::error::{AppError, Result};
use crate::storage::Db;

const KEY: &str = "mods";
/// One change at a time (import, remove, order), so the list and the folders agree.
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
struct Ref {
    id: String,
    on: bool,
}

/// RoALT's `mod.json` in a mod's folder: what the pack said, and what was kept.
#[derive(Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Stored {
    #[serde(flatten)]
    meta: Meta,
    files: u32,
    bytes: u64,
    added: i64,
}

/// A mod as the Mods view lists it.
#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ModItem {
    pub id: String,
    pub name: String,
    pub author: String,
    pub version: String,
    pub description: String,
    pub on: bool,
    pub files: u32,
    #[ts(type = "number")]
    pub bytes: u64,
}

/// The Mods view's state.
#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ModsList {
    pub mods: Vec<ModItem>,
    /// The last launch with mods on (since RoALT started).
    pub last: Option<super::runtime::ModsLast>,
}

/// What an import kept.
#[derive(Serialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ModImported {
    pub name: String,
    pub files: u32,
    pub skipped: u32,
}

fn dir() -> PathBuf {
    super::root().join("library")
}

/// The list, top first. Unreadable is an error, never an empty list: `sweep` would delete every
/// folder no entry names, and the next import would save over the rest.
fn refs(db: &Db) -> Result<Vec<Ref>> {
    match db.setting_value(KEY)? {
        Some(j) => serde_json::from_str(&j).map_err(|e| AppError::Internal(format!("the modpack list is unreadable: {e}"))),
        None => Ok(Vec::new()),
    }
}

fn save(db: &Db, refs: &[Ref]) -> Result<()> {
    let json = serde_json::to_string(refs).map_err(|e| AppError::Internal(e.to_string()))?;
    db.set_setting_value(KEY, Some(&json))
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// A mod id from the UI as the folder name it is: a UUID, never a path.
fn id_of(id: &str) -> Result<String> {
    uuid::Uuid::parse_str(id).map(|u| u.to_string()).map_err(|_| AppError::NotFound)
}

fn stored(id: &str) -> Option<Stored> {
    serde_json::from_slice(&std::fs::read(dir().join(id).join(META_FILE)).ok()?).ok()
}

/// Every mod, top first; ones whose folder is gone are left out.
pub fn list(db: &Db) -> Result<Vec<ModItem>> {
    Ok(refs(db)?
        .into_iter()
        .filter_map(|r| {
            let s = stored(&r.id)?;
            Some(ModItem { id: r.id, name: s.meta.name, author: s.meta.author, version: s.meta.version, description: s.meta.description, on: r.on, files: s.files, bytes: s.bytes })
        })
        .collect())
}

/// The folders of the mods turned on, top first, with their ids: only those `list` shows, so none
/// is applied that can't be turned off.
pub fn enabled(db: &Db) -> Result<Vec<(String, PathBuf)>> {
    Ok(refs(db)?.into_iter().filter(|r| r.on && stored(&r.id).is_some()).map(|r| (r.id.clone(), dir().join(&r.id))).collect())
}

/// At start, before anything can import: half-made imports and folders no mod is, gone; mods
/// whose folder is gone, out of the list.
pub fn sweep(db: &Db) {
    let _g = lock();
    let Ok(mut refs) = refs(db) else { return };
    let before = refs.len();
    refs.retain(|r| dir().join(&r.id).join(META_FILE).is_file());
    if refs.len() != before && let Err(e) = save(db, &refs) {
        log::warn!("mods: couldn't drop missing mods from the list: {e}");
    }
    for e in std::fs::read_dir(dir()).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !refs.iter().any(|r| r.id == name) && let Err(err) = std::fs::remove_dir_all(e.path()) {
            log::warn!("mods: couldn't delete {name}: {err}");
        }
    }
}

/// Imports the zip at `zip` as a new mod, on and at the top.
pub fn import(db: &Db, zip: &Path, now: i64) -> Result<ModImported> {
    let _g = lock();
    super::ensure_root()?;
    let id = uuid::Uuid::new_v4().to_string();
    let staging = dir().join(format!("{id}.partial"));
    std::fs::create_dir_all(&staging)?;
    let done = (|| {
        let got = pack::unzip(zip, &staging)?;
        if got.files == 0 {
            return Err(AppError::Invalid(tr!("No content files found. A modpack needs content, PlatformContent or ExtraContent folders.")));
        }
        let mut meta = got.meta;
        if meta.name.is_empty() {
            meta.name = zip.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            meta = meta.clean();
        }
        let s = Stored { meta, files: got.files, bytes: got.bytes, added: now };
        std::fs::write(staging.join(META_FILE), serde_json::to_vec(&s).map_err(|e| AppError::Internal(e.to_string()))?)?;
        crate::launcher::client_settings::retry(|| std::fs::rename(&staging, dir().join(&id)))?;
        Ok(ModImported { name: s.meta.name, files: got.files, skipped: got.skipped })
    })();
    match done {
        Ok(got) => {
            let mut all = refs(db)?;
            all.insert(0, Ref { id: id.clone(), on: true });
            save(db, &all)?;
            log::info!("mods: imported {id} ({} files, {} skipped)", got.files, got.skipped);
            Ok(got)
        }
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            Err(e)
        }
    }
}

pub fn remove(db: &Db, id: &str) -> Result<()> {
    let id = id_of(id)?;
    let _g = lock();
    let mut all = refs(db)?;
    all.retain(|r| r.id != id);
    save(db, &all)?;
    // The copies hold copies of its files, not links: deleting it never pulls a file from a running client.
    if let Err(e) = std::fs::remove_dir_all(dir().join(&id)) {
        log::warn!("mods: couldn't delete {id}'s folder: {e}");
    }
    Ok(())
}

pub fn set_on(db: &Db, id: &str, on: bool) -> Result<()> {
    let id = id_of(id)?;
    let _g = lock();
    let mut all = refs(db)?;
    all.iter_mut().find(|r| r.id == id).ok_or(AppError::NotFound)?.on = on;
    save(db, &all)
}

/// The new order, top first, of the mods the view lists. Ones it doesn't list (a folder gone or
/// damaged since) keep their place after them; an unknown or repeated id refuses the order.
pub fn reorder(db: &Db, ids: &[String]) -> Result<()> {
    let _g = lock();
    let all = refs(db)?;
    let ids: Vec<String> = ids.iter().map(|i| id_of(i)).collect::<Result<_>>()?;
    let mut next: Vec<Ref> = Vec::with_capacity(all.len());
    for id in &ids {
        match all.iter().find(|r| &r.id == id) {
            Some(r) if !next.contains(r) => next.push(r.clone()),
            _ => return Err(AppError::Invalid(tr!("The modpack list changed. Try again."))),
        }
    }
    next.extend(all.iter().filter(|r| !ids.contains(&r.id)).cloned());
    save(db, &next)
}

/// Writes the mod as a zip at `dest`, with a `mod.json` of only what the pack said.
pub fn export(id: &str, dest: &Path) -> Result<()> {
    let id = id_of(id)?;
    let s = stored(&id).ok_or(AppError::NotFound)?;
    pack::zip_to(&dir().join(&id), &s.meta, dest)
}

/// The file name an export suggests.
pub fn export_name(id: &str) -> Result<String> {
    let id = id_of(id)?;
    Ok(pack::file_name(&stored(&id).ok_or(AppError::NotFound)?.meta.name))
}
