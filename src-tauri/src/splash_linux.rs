//! Start-up splash on Linux. The window opens small and grows to the saved state after the UI
//! animation (or a 10 s fallback). Positioning uses Tauri's own window API instead of Win32.

use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, LogicalSize, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};
use tauri_plugin_window_state::{StateFlags, WindowExt};

const SPLASH: (f64, f64) = (440.0, 300.0);
pub const DEFAULT: (f64, f64) = (1280.0, 800.0);
pub const MIN: (f64, f64) = (960.0, 600.0);
const FALLBACK: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

struct Target {
    rect: Rect,
    maximized: bool,
}

#[derive(Default)]
pub struct Pending(Mutex<Option<Target>>);

pub fn start(app: &AppHandle, w: &WebviewWindow, _skip_splash: bool) -> tauri::Result<()> {
    w.restore_state(StateFlags::all() - StateFlags::VISIBLE)?;
    let maximized = w.is_maximized()?;
    if maximized {
        w.unmaximize()?;
    }
    let scale = w.scale_factor()?;
    let (pos, size) = (w.outer_position()?, w.outer_size()?);
    let mut rect = Rect { x: pos.x, y: pos.y, w: size.width as i32, h: size.height as i32 };
    let monitor = w.current_monitor()?.or(w.primary_monitor()?);
    if f64::from(rect.w) < MIN.0 * scale || f64::from(rect.h) < MIN.1 * scale {
        let (rw, rh) = ((DEFAULT.0 * scale) as i32, (DEFAULT.1 * scale) as i32);
        rect = match &monitor {
            Some(m) => centered(area(m), rw, rh),
            None => Rect { w: rw, h: rh, ..rect },
        };
    }
    if maximized && let Some(m) = &monitor {
        rect = area(m);
    }
    let s = centered(rect, (SPLASH.0 * scale) as i32, (SPLASH.1 * scale) as i32);
    w.set_size(PhysicalSize::new(s.w as u32, s.h as u32))?;
    w.set_position(PhysicalPosition::new(s.x, s.y))?;
    *app.state::<Pending>().0.lock().unwrap_or_else(|e| e.into_inner()) = Some(Target { rect, maximized });
    w.show()?;
    let _ = w.set_focus();
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(FALLBACK);
        grow(&handle, true);
    });
    Ok(())
}

fn centered(on: Rect, w: i32, h: i32) -> Rect {
    Rect { x: on.x + (on.w - w) / 2, y: on.y + (on.h - h) / 2, w, h }
}

fn area(m: &tauri::Monitor) -> Rect {
    let a = m.work_area();
    Rect { x: a.position.x, y: a.position.y, w: a.size.width as i32, h: a.size.height as i32 }
}

fn grow(app: &AppHandle, fallback: bool) {
    let Some(target) = app.state::<Pending>().0.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
    if fallback {
        log::warn!("splash: the UI never called splash_done; growing the window anyway");
    }
    let Some(w) = app.get_webview_window("main") else {
        log::warn!("splash: main window missing at grow");
        return;
    };
    let r = target.rect;
    if let Err(e) = w.set_size(PhysicalSize::new(r.w as u32, r.h as u32)) {
        log::warn!("splash: could not size the window: {e}");
    }
    if let Err(e) = w.set_position(PhysicalPosition::new(r.x, r.y)) {
        log::warn!("splash: could not place the window: {e}");
    }
    if let Err(e) = w.set_min_size(Some(LogicalSize::new(MIN.0, MIN.1))) {
        log::warn!("splash: could not set the minimum size: {e}");
    }
    if target.maximized && let Err(e) = w.maximize() {
        log::warn!("splash: could not maximize the window: {e}");
    }
}

#[tauri::command]
pub async fn splash_done(app: AppHandle) {
    if let Err(e) = tauri::async_runtime::spawn_blocking(move || grow(&app, false)).await {
        log::warn!("splash: grow failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centers_the_splash_on_the_target() {
        let b = Rect { x: 320, y: 140, w: 1280, h: 800 };
        assert_eq!(centered(b, 440, 300), Rect { x: 740, y: 390, w: 440, h: 300 });
    }
}
