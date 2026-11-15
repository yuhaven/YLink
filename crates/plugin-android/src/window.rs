//! Optional position pinning for the scrcpy window.
//!
//! The window keeps its native Windows frame: title bar, minimise, maximise and
//! close all behave the way users expect, and dragging works out of the box.
//! An earlier version stripped the frame and tried to re-implement dragging;
//! that removed the standard controls and broke on cross-process subclassing,
//! so it is gone.
//!
//! The only thing left here is the optional "lock position" feature.

#![cfg(windows)]

use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowRect, GetWindowThreadProcessId, IsWindow, IsWindowVisible, SetWindowPos,
    HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOOWNERZORDER, SWP_NOSIZE,
};

#[derive(Debug, Clone, Copy)]
pub struct Spec {
    pub x: i32,
    pub y: i32,
    pub topmost: bool,
    pub lock_position: bool,
}

/// Read the `window` object sent with `session.start`.
///
/// Size is ignored on purpose: the engine owns it, so the video is never
/// stretched.
pub fn spec_from_json(value: &Value, lock_position: bool) -> Option<Spec> {
    Some(Spec {
        x: value.get("x")?.as_i64()? as i32,
        y: value.get("y")?.as_i64()? as i32,
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
        Some((
            rect.left,
            rect.top,
            (rect.right - rect.left).max(1) as u32,
            (rect.bottom - rect.top).max(1) as u32,
        ))
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

/// Apply the always-on-top flag to a running window without moving it.
pub fn set_topmost(pid: u32, topmost: bool) {
    let Some(hwnd) = find_window(pid) else { return };
    unsafe {
        SetWindowPos(
            hwnd,
            if topmost {
                HWND_TOPMOST
            } else {
                std::ptr::null_mut()
            },
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// Keep the window where the host asked for it. Only started when the user
/// enables "固定窗口位置"; the window itself is a normal framed window.
pub fn enforce(pid: u32, spec: Spec, stop: Arc<AtomicBool>) {
    if !spec.lock_position {
        return;
    }

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

    let Some(hwnd) = hwnd else { return };

    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(400));
        if unsafe { IsWindow(hwnd) } == 0 {
            return;
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
        assert_eq!(spec.x, 936);
        assert!(spec.lock_position && spec.topmost);
    }

    #[test]
    fn missing_coordinates_are_rejected() {
        assert!(spec_from_json(&serde_json::json!({"x": 1}), true).is_none());
    }
}
