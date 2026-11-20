//! The tray icon (`tray-icon` feature): left click shows the window; its tooltip counts the running
//! clients; the menu has Show RoALT, Favourites (those with `in_tray`), the window tools (Tile,
//! Minimize all, Restore all), Play all again, Anti-AFK, Close all clients, Kill all Roblox, Lock
//! vault (master-password mode), Quit.

use std::sync::{Mutex, MutexGuard};

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

use crate::App;
use crate::commands::{self, notice};
use crate::error::{AppError, Result};
use crate::instances::window::Show;
use crate::model::{AccountStatus, AntiAfk, BulkRequest, InstanceState, SameServer, SettingsPatch, VaultMode};

const TRAY: &str = "main";

/// What the menu shows; it is rebuilt only when this changes.
#[derive(PartialEq)]
struct View {
    /// In-tray favourites in order: id and name (names are stored in plain text).
    favourites: Vec<(i64, String)>,
    locked: bool,
    master: bool,
    /// How many clients run (the tooltip says so).
    running: usize,
    /// Some client ended and is still listed: Play all again has something to start.
    ended: bool,
    /// Anti-AFK is on (its tick).
    afk: bool,
    /// The menu's language (`i18n::current`).
    lang: &'static str,
}

/// The view the current menu was built from. Only touched on the main thread.
static SHOWN: Mutex<Option<View>> = Mutex::new(None);

fn shown() -> MutexGuard<'static, Option<View>> {
    SHOWN.lock().unwrap_or_else(|e| e.into_inner())
}

/// Builds the tray icon (the app's window icon, tooltip "RoALT") and its menu; menu events go
/// to `on_menu`. Called once from setup.
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let view = view(app);
    let mut tray = TrayIconBuilder::with_id(TRAY)
        .tooltip(tooltip(&view))
        .menu(&menu(app, &view)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| on_menu(app, e.id().as_ref()))
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    *shown() = Some(view);
    Ok(())
}

/// Rebuilds the menu: favourites in tray order by name (ids `fav:<id>`; disabled with the label
/// "Unlock RoALT to launch" while the vault is locked; "No favourites" when none), "Close all
/// clients" (disabled when none run), "Kill all Roblox" (always: it also ends clients RoALT
/// didn't start), "Lock vault" (only in master-password mode, disabled
/// while locked). Called after favourites change, the vault locks or unlocks, and clients
/// start or end. Never fails loudly: errors are logged.
pub fn refresh(app: &AppHandle) {
    let handle = app.clone();
    // Menus live on the main thread; queueing there also keeps refreshes in order and never
    // blocks the caller.
    let queued = app.run_on_main_thread(move || {
        let view = view(&handle);
        if shown().as_ref() == Some(&view) {
            return;
        }
        let Some(tray) = handle.tray_by_id(TRAY) else {
            log::warn!("tray icon missing; menu not updated");
            return;
        };
        if let Err(e) = tray.set_tooltip(Some(tooltip(&view))) {
            log::debug!("could not update the tray tooltip: {e}");
        }
        match menu(&handle, &view).and_then(|m| tray.set_menu(Some(m))) {
            Ok(()) => *shown() = Some(view),
            Err(e) => log::warn!("could not update the tray menu: {e}"),
        }
    });
    if let Err(e) = queued {
        log::warn!("could not update the tray menu: {e}");
    }
}

fn view(app: &AppHandle) -> View {
    let s = app.state::<App>();
    let favourites = match s.db.favourites() {
        Ok(rows) => rows.into_iter().filter(|f| f.in_tray).map(|f| (f.id, f.name)).collect(),
        Err(e) => {
            log::warn!("tray: could not read favourites: {e}");
            Vec::new()
        }
    };
    let list = s.tracker.list();
    View {
        favourites,
        locked: s.vault.is_locked(),
        master: s.vault.mode() == VaultMode::Master,
        running: s.tracker.clients().len(),
        ended: list.iter().any(|i| i.state == InstanceState::Exited),
        afk: s.db.settings().is_ok_and(|v| v.anti_afk.enabled),
        lang: crate::i18n::current(),
    }
}

/// "RoALT", whether locked, and how many clients run while any does.
fn tooltip(v: &View) -> String {
    match (v.locked, v.running) {
        (true, 0) => format!("RoALT · {}", tr!("Locked")),
        (true, n) => format!("RoALT · {} · {}: {}", tr!("Locked"), tr!("Roblox clients"), n),
        (false, 0) => "RoALT".into(),
        (false, n) => format!("RoALT · {}: {}", tr!("Roblox clients"), n),
    }
}

fn menu(app: &AppHandle, v: &View) -> tauri::Result<Menu<Wry>> {
    let item = |id: &str, text: &str, enabled: bool| MenuItem::with_id(app, id, text, enabled, None::<&str>);
    let favourites = Submenu::new(app, tr!("Favourites"), true)?;
    if v.favourites.is_empty() {
        favourites.append(&MenuItem::new(app, tr!("No favourites"), false, None::<&str>)?)?;
    } else if v.locked {
        favourites.append(&MenuItem::new(app, tr!("Unlock RoALT to launch"), false, None::<&str>)?)?;
    } else {
        for (id, name) in &v.favourites {
            // A single `&` would mark a shortcut key in a Windows menu.
            favourites.append(&item(&format!("fav:{id}"), &name.replace('&', "&&"), true)?)?;
        }
    }
    let running = v.running > 0;
    let menu = Menu::with_items(
        app,
        &[
            &item("show", &tr!("Show RoALT"), true)?,
            &favourites,
            &PredefinedMenuItem::separator(app)?,
            &item("tile", &tr!("Tile windows"), running)?,
            &item("minimize-all", &tr!("Minimize all"), running)?,
            &item("restore-all", &tr!("Restore all"), running)?,
            // The window does the starting (its launch queue and its rules); locked, it can't.
            &item("play-again", &tr!("Play all again"), v.ended && !v.locked)?,
            &CheckMenuItem::with_id(app, "afk", tr!("Anti-AFK"), true, v.afk, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &item("close-all", &tr!("Close all clients"), running)?,
            &item("kill-all", &tr!("Stop all Roblox"), true)?,
        ],
    )?;
    if v.master {
        menu.append(&item("lock", &tr!("Lock vault"), !v.locked)?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&item("quit", &tr!("Quit RoALT"), true)?)?;
    Ok(menu)
}

/// Shows, unminimizes and focuses the main window (it may be hidden in the tray).
pub fn show_main(app: &AppHandle) {
    // The page first, so the window doesn't show a stale frame.
    crate::set_main_shown(app, true);
    if let Some(w) = app.get_webview_window("main") {
        for r in [w.show(), w.unminimize(), w.set_focus()] {
            if let Err(e) = r {
                log::debug!("tray: could not show the main window: {e}");
            }
        }
    }
}

/// Menu ids: "show" → show, unminimize and focus the main window; "close-all" →
/// `commands::close_running`; "kill-all" → `commands::kill_all`
/// (no confirmation: it is the panic button); "lock" → `commands::lock_vault`;
/// "quit" → exit the app; "fav:<id>" → `launch_favourite`.
pub(crate) fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "show" => show_main(app),
        // A global key (`hotkeys`): the window when it isn't in front, the tray when it is.
        "toggle" => match app.get_webview_window("main") {
            Some(w) if w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false) => {
                if let Err(e) = w.hide() {
                    log::warn!("could not hide the main window to the tray: {e}");
                }
                crate::set_main_shown(app, false);
            }
            _ => show_main(app),
        },
        "close-all" => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = commands::close_running(&app.state::<App>()).await {
                    notice(&app, true, tr!("Couldn't close the clients: {why}", why = e.message()));
                }
            });
        }
        "kill-all" => kill_all(app),
        "tile" | "minimize-all" | "restore-all" => {
            let (app, id) = (app.clone(), id.to_owned());
            tauri::async_runtime::spawn(async move {
                let s = app.state::<App>();
                let done = match id.as_str() {
                    "tile" => commands::instances_tile(app.clone(), s, false).await.map(drop),
                    "minimize-all" => commands::instances_show_all(app.clone(), s, Show::Minimize).await,
                    _ => commands::instances_show_all(app.clone(), s, Show::Restore).await,
                };
                if let Err(e) = done {
                    notice(&app, true, e.message());
                }
            });
        }
        // The page starts them, as its own Play all again does.
        "play-again" => {
            let _ = app.emit("app://play-again", ());
        }
        "afk" => {
            let s = app.state::<App>();
            let set = s.db.settings().and_then(|v| {
                let anti_afk = Some(AntiAfk { enabled: !v.anti_afk.enabled, ..v.anti_afk });
                commands::set_settings(app, &s, SettingsPatch { anti_afk, ..Default::default() })
            });
            match set {
                // The page shows the switch too.
                Ok(_) => drop(app.emit("settings://changed", ())),
                Err(e) => notice(app, true, e.message()),
            }
            // Also when it failed: Windows ticked the item by itself.
            *shown() = None;
            refresh(app);
        }
        "lock" if app.state::<App>().vault.mode() == VaultMode::Master => commands::lock_vault(app),
        "quit" => app.exit(0),
        _ => {
            if let Some(fav) = id.strip_prefix("fav:").and_then(|n| n.parse().ok()) {
                launch_favourite(app, fav);
            } else {
                log::debug!("tray: unknown menu id {id}");
            }
        }
    }
}

/// `commands::kill_all` in the background (tray menu, panic hotkey); a failure is a notice.
pub fn kill_all(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = commands::kill_all(&app.state::<App>()).await {
            notice(&app, true, tr!("Couldn't kill every Roblox client: {why}", why = e.message()));
        }
    });
}

/// Launches a favourite from the tray with its stored accounts that have a valid cookie: one →
/// `commands::launch_tracked` (the favourite marked used); several → `launcher::bulk::start`
/// with the launch delay and `SameServer::PickFirst`; none → show the window with an
/// `app://notice` saying to pick accounts for it in RoALT. Errors become `app://notice`
/// (error), also when the window is hidden.
fn launch_favourite(app: &AppHandle, id: i64) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = launch(&app, id).await {
            notice(&app, true, tr!("Couldn't launch that favourite: {why}", why = e.message()));
        }
    });
}

async fn launch(app: &AppHandle, id: i64) -> Result<()> {
    let s = app.state::<App>();
    let row = s.db.favourites()?.into_iter().find(|f| f.id == id).ok_or(AppError::NotFound)?;
    let fav = commands::favourite_of(&s, row)?;
    let valid: Vec<i64> = commands::favourite_accounts(&s, &fav)?
        .into_iter()
        .filter(|&a| match s.db.account(a) {
            Ok(a) => a.status == AccountStatus::Valid,
            Err(e) => {
                log::warn!("tray: could not read account {a} of favourite {id}: {e}");
                false
            }
        })
        .collect();
    // Skip members already running so refilling dead accounts from the tray works in one click.
    let ids: Vec<i64> = valid.iter().copied().filter(|&a| !s.tracker.has_live_client(a)).collect();
    if ids.is_empty() && !valid.is_empty() {
        notice(app, false, tr!("Every account of \"{name}\" is already running.", name = fav.name));
        return Ok(());
    }
    match ids.len() {
        0 => {
            show_main(app);
            notice(app, false, tr!("No account of \"{name}\" can launch. Pick accounts for it in RoALT.", name = fav.name));
        }
        1 => {
            // Without the multi-instance lock it would end the client that runs.
            if !s.multi.is_active() && s.tracker.list().iter().any(|i| i.account_id != ids[0] && i.state != crate::model::InstanceState::Exited) {
                show_main(app);
                notice(app, false, tr!("\"{name}\" wasn't started: another client is running and multi-instance is off.", name = fav.name));
                return Ok(());
            }
            let _single = s.bulk.begin_single_launch()?;
            commands::launch_tracked(app, &s, ids[0], fav.target, None, fav.options.fps_cap, true, None).await?;
        }
        _ => {
            let delay_ms = s.db.settings()?.launch_delay_ms;
            let req = BulkRequest {
                account_ids: ids,
                target: fav.target,
                delay_ms,
                same_server: fav.options.same_server.unwrap_or(SameServer::PickFirst),
                fps_cap: fav.options.fps_cap,
            };
            crate::launcher::bulk::start(app.clone(), req)?;
        }
    }
    Ok(())
}
