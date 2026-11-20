//! The global keys besides the panic key (`commands`): show or hide RoALT, tile and
//! minimize the clients (`settings.*_hotkey`), and one per favourite (`FavouriteOptions::hotkey`).
//! Each does what its tray menu item does (`tray::on_menu`), so a key is kept with that item's id.
//! All unset by default: a global key is taken from every other app.
//!
//! A key that can't be had (another app holds it, or it is also RoALT's panic key) is off
//! and the others still work. A change that sets such a key is refused (`try_change`).

use std::sync::Mutex;

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::App;
use crate::commands::parse_hotkey;
use crate::error::{AppError, Result};
use crate::model::Settings;

/// The keys registered now, each with the tray menu id it runs. Never held while Windows is asked:
/// that waits for the main thread, where the shortcut handler reads this (`id_of`).
static KEYS: Mutex<Vec<(Shortcut, String)>> = Mutex::new(Vec::new());
/// One change at a time (`set`).
static CHANGING: Mutex<()> = Mutex::new(());

/// A key that is off: as written, as Windows reads it (if it does), and why.
type Off = (String, Option<Shortcut>, AppError);

/// What `key` runs, if it is one of these.
pub fn id_of(key: &Shortcut) -> Option<String> {
    KEYS.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(k, _)| k == key).map(|(_, id)| id.clone())
}

/// The keys `v` and the favourites ask for, as (key, tray menu id); unset ones left out. `fav`: a
/// favourite's key as it is about to be saved, in place of its stored one (no id: a new favourite,
/// whose key runs nothing until `sync` gives it its id).
fn wanted(s: &App, v: &Settings, fav: Option<(Option<i64>, &str)>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = [(&v.toggle_hotkey, "toggle"), (&v.tile_hotkey, "tile"), (&v.minimize_hotkey, "minimize-all")]
        .into_iter()
        .map(|(k, id)| (k.trim().to_owned(), id.to_owned()))
        .collect();
    match s.db.favourites() {
        Ok(rows) => {
            let saving = fav.and_then(|(id, _)| id);
            out.extend(rows.into_iter().filter(|f| Some(f.id) != saving).map(|f| (f.options.hotkey.trim().to_owned(), format!("fav:{}", f.id))));
        }
        Err(e) => log::warn!("hotkeys: favourites unreadable, their keys are off: {e}"),
    }
    if let Some((id, key)) = fav {
        out.push((key.trim().to_owned(), id.map_or_else(|| "fav:new".to_owned(), |id| format!("fav:{id}"))));
    }
    out.retain(|(k, _)| !k.is_empty());
    out
}

/// The panic key of `v`: never given to anything here.
fn reserved(v: &Settings) -> Vec<Shortcut> {
    parse_hotkey(v.panic_hotkey.trim()).ok().into_iter().collect()
}

/// Registers what the stored settings and favourites ask for, as far as it goes. Returns why each
/// key that is off is off (already logged).
pub fn sync(app: &AppHandle) -> Vec<AppError> {
    let s = app.state::<App>();
    let v = match s.db.settings() {
        Ok(v) => v,
        Err(e) => {
            log::warn!("hotkeys: settings unreadable, the keys stay as they are: {e}");
            return Vec::new();
        }
    };
    let off = set(app, &wanted(&s, &v, None), &reserved(&v));
    off.into_iter()
        .map(|(key, _, why)| {
            log::warn!("hotkeys: {key} is off: {why}");
            why
        })
        .collect()
}

/// Tries a change before it is saved: `v` in place of the stored settings, or `fav` in place of a
/// favourite's stored key. `changed`: the keys this change sets. If one of them can't be had, the
/// keys go back to the stored ones and that is the error; a key that was off before stays off and
/// is only logged.
pub fn try_change(app: &AppHandle, v: Option<&Settings>, fav: Option<(Option<i64>, &str)>, changed: &[&str]) -> Result<()> {
    let s = app.state::<App>();
    let stored;
    let v = match v {
        Some(v) => v,
        None => {
            stored = s.db.settings()?;
            &stored
        }
    };
    let changed: Vec<(&str, Option<Shortcut>)> = changed.iter().map(|c| (c.trim(), parse_hotkey(c.trim()).ok())).collect();
    let mut refused = None;
    for (key, parsed, why) in set(app, &wanted(&s, v, fav), &reserved(v)) {
        if changed.iter().any(|(c, p)| *c == key || (parsed.is_some() && *p == parsed)) {
            refused.get_or_insert(why);
        } else {
            log::warn!("hotkeys: {key} is off: {why}");
        }
    }
    match refused {
        Some(why) => {
            sync(app);
            Err(why)
        }
        None => Ok(()),
    }
}

/// Makes `keys` the registered ones, as far as it goes, and returns the ones that are off: not a
/// key combination, asked for twice (the first asker keeps it), one of `reserved`, or refused by
/// Windows (another app holds it). A key that stays is never let go of in between.
fn set(app: &AppHandle, keys: &[(String, String)], reserved: &[Shortcut]) -> Vec<Off> {
    let _one = CHANGING.lock().unwrap_or_else(|e| e.into_inner());
    let mut off: Vec<Off> = Vec::new();
    let mut new: Vec<(Shortcut, String, &str)> = Vec::new();
    for (key, id) in keys {
        match parse_hotkey(key) {
            Err(e) => off.push((key.clone(), None, e)),
            Ok(k) if reserved.contains(&k) || new.iter().any(|(n, ..)| *n == k) => {
                off.push((key.clone(), Some(k), AppError::Invalid(tr!("{key} is set for two things. Give each its own key.", key = key))));
            }
            Ok(k) => new.push((k, id.clone(), key)),
        }
    }
    let gs = app.global_shortcut();
    let held = KEYS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    for (k, _) in held.iter().filter(|(k, _)| !new.iter().any(|(n, ..)| n == k)) {
        if let Err(e) = gs.unregister(*k) {
            log::warn!("hotkeys: could not unregister a key: {e}");
        }
    }
    new.retain(|(k, _, key)| {
        if held.iter().any(|(h, _)| h == k) {
            return true;
        }
        match gs.register(*k) {
            Ok(()) => true,
            Err(e) => {
                log::warn!("hotkeys: could not register {key}: {e}");
                off.push(((*key).to_owned(), Some(*k), AppError::Invalid(tr!("Windows wouldn't give RoALT {key}: another app probably uses it. Pick another.", key = *key))));
                false
            }
        }
    });
    *KEYS.lock().unwrap_or_else(|e| e.into_inner()) = new.into_iter().map(|(k, id, _)| (k, id)).collect();
    off
}
