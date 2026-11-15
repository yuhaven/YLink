//! Take over the scrcpy window: strip decorations, pin the position.
//!
//! scrcpy's own `--window-borderless` left `WS_CAPTION` in place on Windows,
//! so the plugin removes the frame itself and then keeps the window where the
//! host asked it to be. Pinning matters because a frameless topmost window is
//! easy to nudge by accident and hard to put back.

#![cfg(windows)]

use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsWindow,
    IsWindowVisible, SetWindowLongPtrW, SetWindowPos, GWL_STYLE, HWND_TOPMOST, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, WS_CAPTION,
    WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
};

#[derive(Debug, Clone, Copy)]
pub struct Spec {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub topmost: bool,
    pub lock_position: bool,
}

/// Read the `window` object sent with `session.start`.
pub fn spec_from_json(value: &Value, lock_position: bool) -> Option<Spec> {
    Some(Spec {
        x: value.get("x")?.as_i64()? as i32,
        y: value.get("y")?.as_i64()? as i32,
        width: value.get("width")?.as_u64()? as u32,
        height: value.get("height")?.as_u64()? as u32,
        topmost: value
            .get("alwaysOnTop")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        lock_position,
    })
}

struct Search {
    pid: u32,
    found: HWND,
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let search = &mut *(lparam as *mut Search);
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid != search.pid || IsWindowVisible(hwnd) == 0 {
        return 1;
    }
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    if GetWindowRect(hwnd, &mut rect) != 0 && (rect.right - rect.left) > 160 {
        search.found = hwnd;
        return 0;
    }
    1
}

pub fn find_window(pid: u32) -> Option<HWND> {
    let mut search = Search {
        pid,
        found: std::ptr::null_mut(),
    };
    unsafe {
        EnumWindows(Some(enum_proc), &mut search as *mut Search as LPARAM);
    }
    if search.found.is_null() {
        None
    } else {
        Some(search.found)
    }
}

/// Remove the frame and make the window un-draggable through normal means:
/// no caption, no resize border, no system menu (so Alt+Space has no "Move").
///
/// Returns true when something was actually changed.
fn strip_frame(hwnd: HWND) -> bool {
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        let stripped = style
            & !(WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX);
        if stripped == style {
            return false;
        }
        SetWindowLongPtrW(hwnd, GWL_STYLE, stripped as isize);
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        );
        true
    }
}

fn rect_of(hwnd: HWND) -> Option<(i32, i32, u32, u32)> {
    unsafe {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return None;
        }
        let width = (rect.right - rect.left).max(1) as u32;
        let height = (rect.bottom - rect.top).max(1) as u32;
        Some((rect.left, rect.top, width, height))
    }
}

fn place(hwnd: HWND, x: i32, y: i32, width: u32, height: u32, topmost: bool) {
    unsafe {
        SetWindowPos(
            hwnd,
            if topmost {
                HWND_TOPMOST
            } else {
                std::ptr::null_mut()
            },
            x,
            y,
            width as i32,
            height as i32,
            SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        );
    }
}

/// Wait for the engine window, strip its frame, then hold it in place.
///
/// The locked size is whatever the engine chose (scrcpy locks the aspect ratio
/// to the video); only the position is ours. Forcing our own width would
/// stretch the picture.
pub fn enforce(pid: u32, spec: Spec, stop: Arc<AtomicBool>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut hwnd = None;
    while Instant::now() < deadline {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        if let Some(found) = find_window(pid) {
            hwnd = Some(found);
            break;
        }
        std::thread::sleep(Duration::from_millis(150));
    }

    let Some(mut hwnd) = hwnd else { return };
    strip_frame(hwnd);

    // Position is ours; size belongs to the engine (it locks the aspect ratio
    // to the video, and forcing our own size would stretch the picture).
    if let Some((_, _, width, height)) = rect_of(hwnd) {
        place(hwnd, spec.x, spec.y, width, height, spec.topmost);
    }

    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(400));

        if unsafe { IsWindow(hwnd) } == 0 {
            match find_window(pid) {
                Some(found) => {
                    hwnd = found;
                    strip_frame(hwnd);
                }
                None => return,
            }
            continue;
        }

        // SDL may re-apply its own style shortly after the window appears.
        strip_frame(hwnd);

        if !spec.lock_position {
            continue;
        }
        if let Some((x, y, width, height)) = rect_of(hwnd) {
            if (x - spec.x).abs() > 2 || (y - spec.y).abs() > 2 {
                place(hwnd, spec.x, spec.y, width, height, spec.topmost);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_window_spec() {
        let raw = serde_json::json!({
            "x": 936, "y": 68, "width": 688, "height": 1224,
            "borderless": true, "alwaysOnTop": true
        });
        let spec = spec_from_json(&raw, true).expect("spec");
        assert_eq!(spec.width, 688);
        assert!(spec.lock_position && spec.topmost);
    }

    #[test]
    fn missing_coordinates_are_rejected() {
        assert!(spec_from_json(&serde_json::json!({"x": 1}), true).is_none());
    }
}
