//! Resizing a client freely, past Roblox's minimum size. The client holds its window to a minimum
//! (`WM_GETMINMAXINFO`), and Windows' own sizing loop keeps to it whatever the window's style (see
//! the ignored tests in `window`), so that loop must not start: a low-level mouse hook, in RoALT's
//! process, takes a press on a client's edge (`on_mouse`) and RoALT sizes the window from there
//! (`run_drag`). Nothing goes into the client, and no other window is touched. A small window's
//! title bar goes the same way, since Windows' move loop would throw it up to the minimum too.
//!
//! What's left of Windows' own loops (keyboard moves, or no hook) `on_event` looks after.


use std::sync::Mutex;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_CAPTION_BUTTON_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook};
use windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GA_ROOT, GetAncestor, SW_MAXIMIZE, ShowWindowAsync, MSLLHOOKSTRUCT, SMTO_ABORTIFHUNG, SMTO_BLOCK, SendMessageTimeoutW, SetWindowsHookExW, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_NCHITTEST, WindowFromPoint,
    DispatchMessageW, EVENT_SYSTEM_MOVESIZEEND, EVENT_SYSTEM_MOVESIZESTART, GetCursorPos, GetMessageW, GetWindowRect, GetWindowThreadProcessId, IsZoomed, MSG, OBJID_WINDOW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SetWindowPos,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
};

use super::window;

/// The pids of RoALT's clients: no other window is touched.
static CLIENTS: Mutex<Vec<u32>> = Mutex::new(Vec::new());
/// A drag: its window, where it was, where the pointer took hold, and its style if that was changed
/// for the drag.
type Drag = (isize, RECT, POINT, Option<isize>);
/// The drag under way.
static DRAG: Mutex<Option<Drag>> = Mutex::new(None);

/// How far from an edge a grab counts as that edge's; a window's sizing border is well inside it.
const GRAB: i32 = 16;
/// Differences this small are the drag having worked by itself.
const SETTLED: i32 = 2;
const SMALLEST: i32 = 32;
/// Windows `free_resize` took past their minimum: a move that makes these bigger undoes that.
static SMALL: Mutex<Vec<isize>> = Mutex::new(Vec::new());

pub fn mark_small(hwnd: HWND) {
    let mut s = SMALL.lock().unwrap_or_else(|e| e.into_inner());
    if !s.contains(&(hwnd.0 as isize)) {
        s.push(hwnd.0 as isize);
    }
}

/// Where a window of `from`'s size goes when dragged by the title bar: with the pointer (pure).
fn moved(from: RECT, grab: POINT, to: POINT) -> RECT {
    let (dx, dy) = (to.x - grab.x, to.y - grab.y);
    RECT { left: from.left + dx, top: from.top + dy, right: from.right + dx, bottom: from.bottom + dy }
}

/// An edge drag RoALT is running.
struct EdgeDrag {
    hwnd: isize,
    from: RECT,
    grab: POINT,
    edges: Edges,
    /// By the title bar: same size, new place.
    moving: bool,
    at: POINT,
    released: bool,
}
/// The last press on a small window's title bar, to see a double click.
static LAST_TITLE: Mutex<Option<(isize, std::time::Instant)>> = Mutex::new(None);
const NO_EDGES: Edges = Edges { left: false, right: false, top: false, bottom: false };
/// `WM_NCHITTEST`'s answer for a title bar.
const HT_CAPTION: isize = 2;
static EDGE: Mutex<Option<EdgeDrag>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, PartialEq)]
struct Edges {
    left: bool,
    right: bool,
    top: bool,
    bottom: bool,
}

/// Which edges a `WM_NCHITTEST` answer means; None for anything else (pure).
fn edges_of(hit: isize) -> Option<Edges> {
    // HTLEFT to HTBOTTOMRIGHT.
    if !(10..=17).contains(&hit) {
        return None;
    }
    Some(Edges {
        left: matches!(hit, 10 | 13 | 16),
        right: matches!(hit, 11 | 14 | 17),
        top: matches!(hit, 12..=14),
        bottom: matches!(hit, 15..=17),
    })
}

/// The rectangle a drag asks for: the grabbed edges follow the pointer, never inside out (pure).
fn resized(from: RECT, e: Edges, dx: i32, dy: i32) -> RECT {
    let mut r = from;
    if e.left {
        r.left = (from.left + dx).min(from.right - SMALLEST);
    } else if e.right {
        r.right = (from.right + dx).max(from.left + SMALLEST);
    }
    if e.top {
        r.top = (from.top + dy).min(from.bottom - SMALLEST);
    } else if e.bottom {
        r.bottom = (from.bottom + dy).max(from.top + SMALLEST);
    }
    r
}

/// Decides fast, every mouse event on the PC passes here: the press only costs more, and only over
/// a client, with a short limit for a client that doesn't answer. ponytail: in place for the
/// app's lifetime, returning at once with no clients; install it per client if a mouse ever lags.
unsafe extern "system" fn on_mouse(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
    // SAFETY: for WH_MOUSE_LL `l` points at an MSLLHOOKSTRUCT, for the length of this call.
    let at = unsafe { (*(l.0 as *const MSLLHOOKSTRUCT)).pt };
    if code >= 0 {
        let msg = w.0 as u32;
        let under_way = {
            let mut edge = EDGE.lock().unwrap_or_else(|e| e.into_inner());
            match edge.as_mut() {
                Some(d) => {
                    if follow(d, msg, at) {
                        return LRESULT(1);
                    }
                    true
                }
                None => false,
            }
        };
        // Not holding the lock: a client answers in its own time.
        if !under_way
            && msg == WM_LBUTTONDOWN
            && let Some(d) = edge_under(at)
        {
            let mut edge = EDGE.lock().unwrap_or_else(|e| e.into_inner());
            if edge.is_none() {
                *edge = Some(d);
                std::thread::spawn(run_drag);
                return LRESULT(1);
            }
        }
    }
    // SAFETY: passes the event on, as a hook must.
    unsafe { CallNextHookEx(None, code, w, l) }
}

/// What a mouse event means to a drag under way; true: it's the drag's, nothing else gets it
/// (pure). Once released the worker may still be waiting for the client, and everything passes.
/// A second press proves the release was missed (over an elevated window, or a locked screen).
fn follow(d: &mut EdgeDrag, msg: u32, at: POINT) -> bool {
    if d.released {
        return false;
    }
    match msg {
        WM_LBUTTONDOWN => d.released = true,
        WM_LBUTTONUP => {
            d.at = at;
            d.released = true;
            return true;
        }
        _ => d.at = at,
    }
    false
}

/// Whether `at` is on the window's minimize, maximize or close button, as DWM draws them.
fn on_caption_buttons(hwnd: HWND, at: POINT) -> bool {
    let (mut b, mut w) = (RECT::default(), RECT::default());
    // SAFETY: reads one window's rectangles into locals of the size given.
    unsafe {
        DwmGetWindowAttribute(hwnd, DWMWA_CAPTION_BUTTON_BOUNDS, (&raw mut b).cast(), size_of::<RECT>() as u32).is_ok()
            && GetWindowRect(hwnd, &mut w).is_ok()
            && in_buttons(b, w, at)
    }
}

/// `at` (screen) is inside `buttons` (relative to the window at `window`) (pure).
fn in_buttons(buttons: RECT, window: RECT, at: POINT) -> bool {
    let (x, y) = (at.x - window.left, at.y - window.top);
    (buttons.left..buttons.right).contains(&x) && (buttons.top..buttons.bottom).contains(&y)
}

/// The client edge at `at`, if any.
fn edge_under(at: POINT) -> Option<EdgeDrag> {
    if CLIENTS.lock().unwrap_or_else(|e| e.into_inner()).is_empty() {
        return None;
    }
    // SAFETY: read-only window calls; the message has a short time limit.
    unsafe {
        let hwnd = GetAncestor(WindowFromPoint(at), GA_ROOT);
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if !CLIENTS.lock().unwrap_or_else(|e| e.into_inner()).contains(&pid) {
            return None;
        }
        let spot = ((at.y as u16 as u32) << 16 | at.x as u16 as u32) as isize;
        let mut hit = 0usize;
        if SendMessageTimeoutW(hwnd, WM_NCHITTEST, WPARAM(0), LPARAM(spot), SMTO_ABORTIFHUNG | SMTO_BLOCK, 40, Some(&mut hit)).0 == 0 {
            return None;
        }
        let h = hwnd.0 as isize;
        // Roblox answers caption for part of its minimize button: a press there is the button's.
        let moving = hit as isize == HT_CAPTION && !on_caption_buttons(hwnd, at) && SMALL.lock().unwrap_or_else(|e| e.into_inner()).contains(&h) && !IsZoomed(hwnd).as_bool();
        let edges = if moving { NO_EDGES } else { edges_of(hit as isize)? };
        let mut from = RECT::default();
        GetWindowRect(hwnd, &mut from).ok()?;
        // The press is taken, so Windows can't see a double click: that one is done here.
        let mut released = false;
        if moving {
            let mut last = LAST_TITLE.lock().unwrap_or_else(|e| e.into_inner());
            let twice = last.is_some_and(|(w, t)| w == h && t.elapsed().as_millis() <= u128::from(GetDoubleClickTime()));
            *last = (!twice).then(|| (h, std::time::Instant::now()));
            if twice {
                let _ = ShowWindowAsync(hwnd, SW_MAXIMIZE);
                released = true;
            }
        }
        Some(EdgeDrag { hwnd: h, from, grab: at, edges, moving, at, released })
    }
}

/// Follows the pointer until the button comes up, a frame at a time; a slow client only gets
/// fewer frames. Each frame is a whole `free_resize`: Roblox can put its style back after any
/// resize, so changing the style once for the drag isn't enough.
fn run_drag() {
    let Some((h, from, grab, edges, moving)) = EDGE.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|d| (d.hwnd, d.from, d.grab, d.edges, d.moving)) else { return };
    let hwnd = HWND(h as *mut _);
    // The press never reached it, so it didn't come to the front by itself.
    let _ = super::afk::to_front(hwnd);
    let mut last = from;
    loop {
        let (at, done) = match EDGE.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            Some(d) => (d.at, d.released),
            None => return,
        };
        let r = if moving { moved(from, grab, at) } else { resized(from, edges, at.x - grab.x, at.y - grab.y) };
        if (r.left, r.top, r.right, r.bottom) != (last.left, last.top, last.right, last.bottom) {
            last = r;
            if moving {
                // SAFETY: a window-manager call on a top-level window. Windows only applies the
                // minimum when the size changes.
                let _ = unsafe { SetWindowPos(hwnd, None, r.left, r.top, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE) };
            } else {
                window::free_resize(hwnd, (r.left, r.top, r.right - r.left, r.bottom - r.top), false);
            }
        }
        if done {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
    remember_size(hwnd);
    *EDGE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

pub fn set_clients(pids: Vec<u32>) {
    *CLIENTS.lock().unwrap_or_else(|e| e.into_inner()) = pids;
}

/// The rectangle a drag ends in: the edges it took hold of follow the pointer, the rest stay.
/// None for a drag by the title bar, which is a move (pure).
fn dragged(from: RECT, grab: POINT, to: POINT) -> Option<RECT> {
    let near = |a: i32, b: i32| (a - b).abs() <= GRAB;
    let (left, right) = (near(grab.x, from.left), near(grab.x, from.right));
    let (top, bottom) = (near(grab.y, from.top), near(grab.y, from.bottom));
    if !(left || right || top || bottom) {
        return None;
    }
    Some(resized(from, Edges { left, right, top, bottom }, to.x - grab.x, to.y - grab.y))
}

unsafe extern "system" fn on_event(_: HWINEVENTHOOK, event: u32, hwnd: HWND, object: i32, _: i32, _: u32, _: u32) {
    if object != OBJID_WINDOW.0 {
        return;
    }
    let mut pid = 0;
    // SAFETY: reads which process owns a window into a local.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if !CLIENTS.lock().unwrap_or_else(|e| e.into_inner()).contains(&pid) {
        return;
    }
    let (mut at, mut rect) = (POINT::default(), RECT::default());
    // SAFETY: read the pointer and one window's rectangle into locals.
    if unsafe { GetCursorPos(&mut at) }.is_err() || unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
        return;
    }
    // The lock isn't held over the window calls, which wait for the client.
    if event == EVENT_SYSTEM_MOVESIZESTART {
        let small = SMALL.lock().unwrap_or_else(|e| e.into_inner()).contains(&(hwnd.0 as isize));
        let style = (small && dragged(rect, at, at).is_none()).then(|| window::style_of(hwnd));
        if let Some(s) = style {
            window::set_style(hwnd, window::drag_style(s as u32) as isize);
        }
        *DRAG.lock().unwrap_or_else(|e| e.into_inner()) = Some((hwnd.0 as isize, rect, at, style));
        return;
    }
    let taken = DRAG.lock().unwrap_or_else(|e| e.into_inner()).take().filter(|d| d.0 == hwnd.0 as isize);
    if let Some((h, from, grab, style)) = taken {
        if let Some(s) = style {
            window::set_style(hwnd, s);
            // SAFETY: only makes the frame follow the style again.
            let _ = unsafe { SetWindowPos(hwnd, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED) };
        }
        let wide = |r: RECT| r.right - r.left;
        let tall = |r: RECT| r.bottom - r.top;
        let target = match dragged(from, grab, at) {
            Some(r) => Some(r).filter(|r| (wide(*r) - wide(rect)).abs() + (tall(*r) - tall(rect)).abs() > SETTLED),
            // A move. Grown means thrown back to the minimum, unless it was snapped full-screen.
            None => {
                // SAFETY: reads one window's state.
                let zoomed = unsafe { IsZoomed(hwnd) }.as_bool();
                let small = SMALL.lock().unwrap_or_else(|e| e.into_inner()).contains(&h);
                (small && !zoomed && (wide(rect) > wide(from) + SETTLED || tall(rect) > tall(from) + SETTLED)).then(|| moved(from, grab, at))
            }
        };
        if let Some(r) = target {
            // On its own thread: this one watches the whole desktop, and a busy client answers late.
            std::thread::spawn(move || {
                let hwnd = HWND(h as *mut _);
                window::free_resize(hwnd, (r.left, r.top, r.right - r.left, r.bottom - r.top), false);
                remember_size(hwnd);
            });
        } else {
            remember_size(hwnd);
        }
    }
}

/// The pid of the window, if it is one of RoALT's clients'.
fn client_of(hwnd: HWND) -> Option<u32> {
    let mut pid = 0;
    // SAFETY: reads which process owns a window into a local.
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    CLIENTS.lock().unwrap_or_else(|e| e.into_inner()).contains(&pid).then_some(pid)
}

/// A drag of a client's window just ended.
fn remember_size(hwnd: HWND) {
    if let Some(pid) = client_of(hwnd) {
        window::user_sized(pid, hwnd);
    }
}

/// This thread's messages, for as long as the app runs.
fn pump() {
    let mut msg = MSG::default();
    // SAFETY: an ordinary message loop on this thread.
    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        // SAFETY: as above.
        unsafe { DispatchMessageW(&msg) };
    }
}

/// Watches for window drags, for as long as the app runs. A thread each: `on_event` can wait for a
/// client, and every mouse event on the PC would wait with it.
pub fn start() {
    std::thread::spawn(|| {
        // SAFETY: out of context, so nothing is loaded into another process; the callback stays
        // valid for the program's life, and this thread pumps the messages it arrives through.
        let hook = unsafe { SetWinEventHook(EVENT_SYSTEM_MOVESIZESTART, EVENT_SYSTEM_MOVESIZEEND, None, Some(on_event), 0, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS) };
        if hook.is_invalid() {
            log::warn!("could not watch window drags: a small client grows to Roblox's minimum size while moved");
        }
        pump();
    });
    std::thread::spawn(|| {
        // SAFETY: a low-level hook runs on this thread, which pumps; its procedure lives forever.
        let mouse = unsafe {
            let module = GetModuleHandleW(None).ok();
            SetWindowsHookExW(WH_MOUSE_LL, Some(on_mouse), module.map(Into::into), 0)
        };
        if let Err(e) = mouse {
            log::warn!("no free resizing of clients: {e}");
        }
        pump();
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::WindowsAndMessaging::WM_MOUSEMOVE;

    const R: RECT = RECT { left: 100, top: 100, right: 900, bottom: 700 };

    #[test]
    fn only_the_drags_own_release_is_taken() {
        let p = |x, y| POINT { x, y };
        let mut d = EdgeDrag { hwnd: 0, from: R, grab: p(0, 0), edges: NO_EDGES, moving: false, at: p(0, 0), released: false };
        assert!(!follow(&mut d, WM_MOUSEMOVE, p(5, 5)));
        assert_eq!((d.at.x, d.released), (5, false));
        assert!(follow(&mut d, WM_LBUTTONUP, p(9, 9)), "its release");
        assert_eq!((d.at.x, d.released), (9, true));
        // The worker is stuck on a slow client: other clicks on the PC stay whole.
        assert!(!follow(&mut d, WM_LBUTTONDOWN, p(50, 50)));
        assert!(!follow(&mut d, WM_LBUTTONUP, p(50, 50)));
        assert_eq!(d.at.x, 9, "the window stays where it was let go");
        // The release was missed: the next press ends the drag, and goes to where it was aimed.
        d.released = false;
        assert!(!follow(&mut d, WM_LBUTTONDOWN, p(70, 70)));
        assert!(d.released);
        assert!(!follow(&mut d, WM_LBUTTONUP, p(70, 70)));
    }
    fn p(x: i32, y: i32) -> POINT {
        POINT { x, y }
    }

    #[test]
    fn a_press_on_the_caption_buttons_is_theirs() {
        // Measured on a client: buttons 663..809 x 0..30 of a window at (650, 143).
        let (b, w) = (RECT { left: 663, top: 0, right: 809, bottom: 30 }, RECT { left: 650, top: 143, right: 1466, bottom: 781 });
        assert!(in_buttons(b, w, p(650 + 687, 153)), "where Roblox says caption");
        assert!(!in_buttons(b, w, p(650 + 400, 153)), "the rest of the title bar");
        assert!(!in_buttons(b, w, p(650 + 687, 143 + 30)));
    }

    #[test]
    fn hit_test_answers_are_edges() {
        let e = |l, r, t, b| Some(Edges { left: l, right: r, top: t, bottom: b });
        assert_eq!(edges_of(10), e(true, false, false, false));
        assert_eq!(edges_of(11), e(false, true, false, false));
        assert_eq!(edges_of(13), e(true, false, true, false));
        assert_eq!(edges_of(17), e(false, true, false, true));
        assert_eq!(edges_of(1), None, "the client area: a click for Roblox");
        assert_eq!(edges_of(2), None, "the title bar");
    }

    /// The real client, started signed out on this desktop: what its window does with each way of
    /// resizing. By hand: `hidden-launch` with this test's name, --ignored --exact --nocapture.
    #[test]
    #[ignore = "starts Roblox: run on a desktop of its own, with no client running"]
    fn real_client_window() {
        let exe = std::env::var("ROALT_TEST_CLIENT").unwrap();
        let mut child = std::process::Command::new(exe).spawn().unwrap();
        let pid = child.id();
        let wait = |ms| std::thread::sleep(std::time::Duration::from_millis(ms));
        let mut found = None;
        for _ in 0..120 {
            wait(500);
            if let Some((h, _)) = super::super::process::main_window(pid) {
                found = Some(h);
                break;
            }
        }
        let Some(hwnd) = found else {
            let _ = child.kill();
            panic!("no window");
        };
        wait(6000);
        let rect = || {
            let mut r = RECT::default();
            // SAFETY: reads one window's rectangle into a local.
            unsafe { GetWindowRect(hwnd, &mut r) }.unwrap();
            r
        };
        let size = |r: RECT| (r.right - r.left, r.bottom - r.top);
        println!("style {:#x}, size {:?}", window::style_of(hwnd), size(rect()));
        // The plain way: Windows holds it to the minimum.
        // SAFETY: a window-manager call on a top-level window.
        let _ = unsafe { SetWindowPos(hwnd, None, 50, 50, 300, 250, SWP_NOZORDER | SWP_NOACTIVATE) };
        wait(500);
        println!("plain SetWindowPos 300x250: {:?}", size(rect()));
        window::free_resize(hwnd, (50, 50, 300, 250), false);
        wait(500);
        println!("free_resize 300x250: {:?}, style {:#x}", size(rect()), window::style_of(hwnd));
        window::free_resize(hwnd, (50, 50, 1000, 800), false);
        wait(500);
        println!("free_resize 1000x800: {:?}", size(rect()));

        set_clients(vec![pid]);
        let send = |msg: u32, x: i32, y: i32| {
            let m = MSLLHOOKSTRUCT { pt: p(x, y), ..Default::default() };
            // SAFETY: as the system calls it, with a struct that outlives the call.
            unsafe { on_mouse(0, WPARAM(msg as usize), LPARAM(&m as *const _ as isize)) }.0
        };
        let start = rect();
        let (cx, cy) = (start.right - 3, start.bottom - 3);
        println!("press on the corner taken: {}", send(WM_LBUTTONDOWN, cx, cy));
        for i in 1..=40 {
            send(WM_MOUSEMOVE, cx - i * 18, cy - i * 14);
            wait(30);
            if i % 8 == 0 {
                println!("  drag step {i}: {:?}", size(rect()));
            }
        }
        send(WM_LBUTTONUP, cx - 720, cy - 560);
        wait(800);
        let small = rect();
        println!("after the drag: {:?}, style {:#x}", size(small), window::style_of(hwnd));

        // By the title bar.
        let (tx, ty) = (small.left + (small.right - small.left) / 2, small.top + 15);
        println!("press on the title bar taken: {}", send(WM_LBUTTONDOWN, tx, ty));
        for i in 1..=20 {
            send(WM_MOUSEMOVE, tx + i * 10, ty + i * 5);
            wait(30);
            if i % 5 == 0 {
                println!("  move step {i}: {:?}", size(rect()));
            }
        }
        send(WM_LBUTTONUP, tx + 200, ty + 100);
        wait(800);
        let end = rect();
        println!("after the move: {:?} at {},{} (from {},{})", size(end), end.left, end.top, small.left, small.top);
        set_clients(Vec::new());
        let _ = std::process::Command::new("taskkill").args(["/PID", &pid.to_string(), "/T", "/F"]).output();
        let _ = child.wait();
    }

    /// The whole path but the OS delivering the events: a window that insists on 800 x 600 goes
    /// to any size on a drag of its corner, live, and a press inside it isn't taken. Needs a
    /// desktop of its own: `hidden-launch`, then this test by name with --ignored --exact.
    #[test]
    #[ignore = "opens a window: run on a desktop of its own"]
    fn dragging_an_edge_goes_past_the_minimum() {
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DefWindowProcW, DestroyWindow, MINMAXINFO, PeekMessageW, PM_REMOVE, RegisterClassW, TranslateMessage, WINDOW_EX_STYLE, WM_GETMINMAXINFO, WNDCLASSW, WS_OVERLAPPEDWINDOW, WS_VISIBLE};
        use windows::core::w;
        unsafe extern "system" fn insists(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
            if msg == WM_GETMINMAXINFO {
                // SAFETY: for this message `l` points at the MINMAXINFO to fill in.
                unsafe { (*(l.0 as *mut MINMAXINFO)).ptMinTrackSize = POINT { x: 800, y: 600 } };
                return LRESULT(0);
            }
            // SAFETY: the default for everything else.
            unsafe { DefWindowProcW(hwnd, msg, w, l) }
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop2 = stop.clone();
        let owner = std::thread::spawn(move || unsafe {
            let class = WNDCLASSW { lpfnWndProc: Some(insists), lpszClassName: w!("roalt-test-edge"), ..Default::default() };
            RegisterClassW(&class);
            let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), w!("roalt-test-edge"), w!("t"), WS_OVERLAPPEDWINDOW | WS_VISIBLE, 100, 100, 1000, 800, None, None, None, None).unwrap();
            tx.send(hwnd.0 as isize).unwrap();
            let mut msg = MSG::default();
            while !stop2.load(std::sync::atomic::Ordering::Relaxed) {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            DestroyWindow(hwnd).unwrap();
        });
        let hwnd = HWND(rx.recv().unwrap() as *mut _);
        set_clients(vec![std::process::id()]);
        let send = |msg: u32, x: i32, y: i32| {
            let m = MSLLHOOKSTRUCT { pt: p(x, y), ..Default::default() };
            // SAFETY: as the system calls it, with a struct that outlives the call.
            unsafe { on_mouse(0, WPARAM(msg as usize), LPARAM(&m as *const _ as isize)) }.0
        };
        let rect = || {
            let mut r = RECT::default();
            // SAFETY: reads one window's rectangle into a local.
            unsafe { GetWindowRect(hwnd, &mut r) }.unwrap();
            r
        };
        let style = window::style_of(hwnd);
        let wait = |ms| std::thread::sleep(std::time::Duration::from_millis(ms));
        wait(300);
        assert_eq!(send(WM_LBUTTONDOWN, 600, 500), 0, "inside: Roblox's click");
        let start = rect();
        let (cx, cy) = (start.right - 2, start.bottom - 2);
        assert_eq!(send(WM_LBUTTONDOWN, cx, cy), 1, "the corner is ours");
        let (mut w_max, mut h_max) = (0, 0);
        for i in 1..=40 {
            send(WM_MOUSEMOVE, cx - i * 20, cy - i * 15);
            wait(12);
            let r = rect();
            if i > 10 {
                (w_max, h_max) = (r.right - r.left, r.bottom - r.top);
            }
        }
        let end = send(WM_LBUTTONUP, cx - 800, cy - 600);
        assert_eq!(end, 1);
        wait(200);
        let r = rect();
        println!("from {}x{} live {w_max}x{h_max} end {}x{}", start.right - start.left, start.bottom - start.top, r.right - r.left, r.bottom - r.top);
        assert_eq!((r.right - r.left, r.bottom - r.top), (start.right - start.left - 800, start.bottom - start.top - 600));
        assert!(w_max < 800, "live, not on release: {w_max}");
        assert_eq!(window::style_of(hwnd), style, "title bar and edges are back");
        assert!(EDGE.lock().unwrap().is_none());
        set_clients(Vec::new());
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        owner.join().unwrap();
    }

    #[test]
    fn a_move_keeps_the_size_and_follows_the_pointer() {
        let r = moved(R, p(500, 110), p(650, 90));
        assert_eq!((r.left, r.top, r.right, r.bottom), (250, 80, 1050, 680));
    }

    #[test]
    fn the_grabbed_edges_follow_the_pointer() {
        let size = |r: RECT| (r.left, r.top, r.right, r.bottom);
        assert_eq!(size(dragged(R, p(898, 400), p(300, 450)).unwrap()), (100, 100, 302, 700), "right edge");
        assert_eq!(size(dragged(R, p(101, 400), p(600, 400)).unwrap()), (599, 100, 900, 700), "left edge");
        assert_eq!(size(dragged(R, p(500, 698), p(500, 200)).unwrap()), (100, 100, 900, 202), "bottom edge");
        assert_eq!(size(dragged(R, p(899, 699), p(300, 300)).unwrap()), (100, 100, 301, 301), "corner");
        assert!(dragged(R, p(500, 400), p(100, 100)).is_none(), "the middle: not a resize");
        assert_eq!(size(dragged(R, p(898, 400), p(-500, 400)).unwrap()), (100, 100, 132, 700), "never inside out");
    }
}
