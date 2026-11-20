//! Frame-free live previews, where Windows can't capture a client without drawing its yellow frame
//! around it (`capture`, before Windows 11): DWM draws each client's window over its card, straight
//! onto RoALT's window (`DwmRegisterThumbnail`, as Alt+Tab does). Nothing is captured or read
//! back. The page places them (`place`) as its cards move, and leaves out every one a dialog, a menu
//! or the edge of the view would cut: DWM draws a thumbnail over everything in RoALT's window.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use serde::Deserialize;
use ts_rs::TS;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Dwm::{
    DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION, DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_VISIBLE, DwmRegisterThumbnail,
    DwmUnregisterThumbnail, DwmUpdateThumbnailProperties,
};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, GetWindowRect, IsIconic, IsWindow, IsWindowVisible};

/// A client's picture box on the page, in CSS pixels; `dim` while something of the card's own sits
/// over the picture (its keys, a status pill), which then shows through.
#[derive(Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ThumbPlace {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub dim: bool,
}

/// How much of a dim thumbnail shows (of 255): the card's keys under it stay readable.
const DIM: u8 = 70;

/// The thumbnails drawn now, by instance id: the client's window and DWM's handle.
static SHOWN: LazyLock<Mutex<HashMap<String, (isize, isize)>>> = LazyLock::new(Mutex::default);

/// Draws the clients in `places` on `dest` (RoALT's window, `scale` physical pixels per CSS pixel);
/// every other thumbnail goes. `window_of`: a client's main window when it has one that isn't
/// minimized (looked up only when the one drawn is gone, hidden or minimized).
pub fn place(dest: HWND, scale: f64, places: &[ThumbPlace], window_of: impl Fn(&str) -> Option<HWND>) {
    let mut shown = SHOWN.lock().unwrap_or_else(|e| e.into_inner());
    // The client area's corner in the window: DWM places thumbnails from the window's own corner.
    let offset = client_offset(dest);
    let mut kept = Vec::new();
    for p in places.iter().filter(|p| sane(p)) {
        let cached = shown.get(&p.id).map(|&(h, _)| HWND(h as _)).filter(|&h| usable(h));
        let Some(source) = cached.or_else(|| window_of(&p.id)) else { continue };
        let thumb = match shown.get(&p.id) {
            Some(&(h, t)) if h == source.0 as isize => t,
            old => {
                if let Some(&(_, t)) = old {
                    // SAFETY: a handle DWM gave out and this map still holds.
                    let _ = unsafe { DwmUnregisterThumbnail(t) };
                }
                // SAFETY: two window handles; DWM refuses ones that are gone.
                match unsafe { DwmRegisterThumbnail(dest, source) } {
                    Ok(t) => t,
                    Err(e) => {
                        shown.remove(&p.id);
                        log::debug!("thumbnail of instance {}: not drawn: {e}", p.id);
                        continue;
                    }
                }
            }
        };
        shown.insert(p.id.clone(), (source.0 as isize, thumb));
        kept.push(p.id.as_str());
        let mut source_rect = RECT::default();
        // SAFETY: reads the client size of a window that exists (checked above).
        if unsafe { GetClientRect(source, &mut source_rect) }.is_err() {
            continue;
        }
        let rect = fit(p, scale, offset, (source_rect.right, source_rect.bottom));
        let props = DWM_THUMBNAIL_PROPERTIES {
            dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_OPACITY | DWM_TNP_VISIBLE | DWM_TNP_SOURCECLIENTAREAONLY,
            rcDestination: rect,
            opacity: if p.dim { DIM } else { 255 },
            fVisible: true.into(),
            fSourceClientAreaOnly: true.into(),
            ..Default::default()
        };
        // SAFETY: a live thumbnail handle and properties that outlive the call.
        if let Err(e) = unsafe { DwmUpdateThumbnailProperties(thumb, &props) } {
            log::debug!("thumbnail of instance {}: not placed: {e}", p.id);
        }
    }
    shown.retain(|id, &mut (_, t)| {
        let keep = kept.contains(&id.as_str());
        if !keep {
            // SAFETY: a handle DWM gave out, dropped from the map right after.
            let _ = unsafe { DwmUnregisterThumbnail(t) };
        }
        keep
    });
}

/// Finite and within any screen: whatever the page sends.
fn sane(p: &ThumbPlace) -> bool {
    [p.x, p.y, p.width, p.height].iter().all(|v| v.is_finite() && v.abs() < 100_000.0) && p.width >= 1.0 && p.height >= 1.0
}

/// A window still there, shown and not minimized.
fn usable(h: HWND) -> bool {
    // SAFETY: only reads a window's state; a stale handle answers false.
    unsafe { IsWindow(Some(h)).as_bool() && IsWindowVisible(h).as_bool() && !IsIconic(h).as_bool() }
}

/// Where `dest`'s client area starts inside the window, in physical pixels.
fn client_offset(dest: HWND) -> (i32, i32) {
    let (mut window, mut client) = (RECT::default(), POINT::default());
    // SAFETY: reads RoALT's own window's position.
    let ok = unsafe { GetWindowRect(dest, &mut window).is_ok() && ClientToScreen(dest, &mut client).as_bool() };
    if ok { (client.x - window.left, client.y - window.top) } else { (0, 0) }
}

/// The source, `size` physical pixels, fitted into the picture box with its shape kept, centered
/// (pure).
fn fit(p: &ThumbPlace, scale: f64, (ox, oy): (i32, i32), size: (i32, i32)) -> RECT {
    let (bx, by, bw, bh) = (p.x * scale, p.y * scale, p.width * scale, p.height * scale);
    let (sw, sh) = (f64::from(size.0.max(1)), f64::from(size.1.max(1)));
    let k = (bw / sw).min(bh / sh);
    let (w, h) = (sw * k, sh * k);
    let (left, top) = (bx + (bw - w) / 2.0, by + (bh - h) / 2.0);
    RECT { left: left.round() as i32 + ox, top: top.round() as i32 + oy, right: (left + w).round() as i32 + ox, bottom: (top + h).round() as i32 + oy }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f64, y: f64, width: f64, height: f64) -> ThumbPlace {
        ThumbPlace { id: "a".into(), x, y, width, height, dim: false }
    }

    #[test]
    fn fits_the_window_into_its_box() {
        let r = |r: RECT| (r.left, r.top, r.right, r.bottom);
        // A 16:9 client in a 16:9 box at 150 %: fills it.
        assert_eq!(r(fit(&at(10.0, 20.0, 320.0, 180.0), 1.5, (0, 0), (1920, 1080))), (15, 30, 495, 300));
        // A square client in a wide box: pillarboxed, centered.
        assert_eq!(r(fit(&at(0.0, 0.0, 320.0, 180.0), 1.0, (0, 0), (500, 500))), (70, 0, 250, 180));
        // The client area's corner inside the window moves it along.
        assert_eq!(r(fit(&at(0.0, 0.0, 160.0, 90.0), 1.0, (8, 8), (1600, 900))), (8, 8, 168, 98));
        assert_eq!(r(fit(&at(0.0, 0.0, 100.0, 100.0), 1.0, (0, 0), (0, 0))).2, 100, "a client with no size yet");
    }

    #[test]
    fn only_sane_boxes() {
        assert!(sane(&at(0.0, 0.0, 10.0, 10.0)));
        assert!(!sane(&at(f64::NAN, 0.0, 10.0, 10.0)) && !sane(&at(0.0, 0.0, 0.5, 10.0)) && !sane(&at(1e9, 0.0, 10.0, 10.0)));
    }
}
