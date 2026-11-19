//! Find the receiver's video window and place it.
//!
//! uxplay-windows is a tray-only Qt app until an AirPlay client connects; the
//! GStreamer video window is a separate top-level window owned by the same
//! process. We locate it by process id and move it with `SetWindowPos`.
//!
//! Deliberately no `SetWindowLongPtrW`: changing a window's style in another
//! process is not supported (and the skill forbids cross-process subclassing).
//! The native title bar is kept, matching the Android cast window.

#![cfg(windows)]

use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsWindow,
    IsWindowVisible, SetWindowPos, SetWindowTextW, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE,
    SWP_SHOWWINDOW,
};

#[derive(Debug, Clone, Copy)]
pub struct Spec {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    /// Kept for protocol compatibility; the native frame is never stripped.
    pub borderless: bool,
    pub always_on_top: bool,
}

/// Parse the `window` object the host sends with `session.start`.
pub fn spec_from_json(value: &serde_json::Value) -> Option<Spec> {
    Some(Spec {
        x: value.get("x")?.as_i64()? as i32,
        y: value.get("y")?.as_i64()? as i32,
        width: value.get("width")?.as_u64()? as u32,
        height: value.get("height")?.as_u64()? as u32,
        borderless: value
            .get("borderless")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        always_on_top: value
            .get("alwaysOnTop")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
    })
}

/// A landscape window for an iPad/iPhone mirror: 90% of the work height and
/// 16:10 wide, anchored to the right edge so it never covers the console.
///
/// The host only knows "portrait phone" geometry, so the receiver picks its own
/// aspect. GStreamer letterboxes the actual stream; the user can still resize.
pub fn receiver_rect() -> Option<Spec> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    unsafe {
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        if screen_w <= 0 || screen_h <= 0 {
            return None;
        }
        let work_h = screen_h - 80;
        let height = (work_h as f64 * 0.90) as i32;
        let width = (height as f64 * 1.6) as i32;
        Some(Spec {
            x: (screen_w - width - 40).max(0),
            y: (screen_h - height) / 2,
            width: width as u32,
            height: height as u32,
            borderless: false,
            always_on_top: false,
        })
    }
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
        return 1; // keep enumerating
    }

    // The tray app's settings window is 320px wide and titled
    // "uxplay-windows"; never mistake it for the video window.
    let mut title = [0u16; 128];
    let len = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32);
    let title = String::from_utf16_lossy(&title[..len.max(0) as usize]);
    if title.eq_ignore_ascii_case("uxplay-windows") {
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
        return 0; // stop
    }
    1
}

/// First visible top-level video window owned by `pid`.
///
/// The tray/observer windows are hidden, so visibility + width is enough to
/// skip them. The Qt main window is intentionally hidden by uxplay-windows.
pub fn find_video_window(pid: u32) -> Option<HWND> {
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

/// Backwards-compatible name used by the old tests.
pub fn find_main_window(pid: u32) -> Option<HWND> {
    find_video_window(pid)
}

pub fn is_alive(hwnd: HWND) -> bool {
    unsafe { IsWindow(hwnd) != 0 }
}

/// Apply the requested geometry and a stable title.
pub fn adopt(hwnd: HWND, spec: &Spec) {
    let title: Vec<u16> = "CastPane - AirPlay\0".encode_utf16().collect();
    unsafe {
        SetWindowTextW(hwnd, title.as_ptr());
        SetWindowPos(
            hwnd,
            if spec.always_on_top {
                HWND_TOPMOST
            } else {
                HWND_NOTOPMOST
            },
            spec.x,
            spec.y,
            spec.width as i32,
            spec.height as i32,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}

/// Wait for the engine's video window to appear, then take it over.
pub fn adopt_when_ready(pid: u32, spec: Spec, timeout_ms: u64) -> Result<(), String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    loop {
        if let Some(hwnd) = find_video_window(pid) {
            adopt(hwnd, &spec);
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!("{timeout_ms}ms 内没等到画面窗口（pid {pid}）"));
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_window_spec() {
        let raw = serde_json::json!({
            "x": 700, "y": 52, "width": 527, "height": 936,
            "borderless": true, "alwaysOnTop": true
        });
        let spec = spec_from_json(&raw).expect("spec");
        assert_eq!(spec.width, 527);
        assert!(spec.borderless && spec.always_on_top);
    }

    #[test]
    fn rejects_incomplete_spec() {
        assert!(spec_from_json(&serde_json::json!({"x": 1})).is_none());
    }

    #[test]
    fn no_window_for_a_dead_pid() {
        // PID 0 is the idle process: it owns no visible window.
        assert!(find_main_window(0).is_none());
    }

    #[test]
    fn receiver_rect_is_landscape_and_on_screen() {
        if let Some(spec) = receiver_rect() {
            assert!(spec.width > spec.height, "AirPlay window must be landscape");
            assert_eq!(spec.borderless, false);
            assert_eq!(spec.always_on_top, false);
        }
    }
}
