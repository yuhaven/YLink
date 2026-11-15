//! Find the receiver's top-level window and place it.
//!
//! uxplay-windows is a Qt app with no command line for window geometry, so we
//! locate its HWND by process id and move it with `SetWindowPos`. This is the
//! "window takeover" described in docs/plugin-api.md §9.
//!
//! Everything here is best effort: if the window cannot be found or moved,
//! the receiver still works, just where it wants to be.

#![cfg(windows)]

use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible,
    SetWindowLongPtrW, SetWindowPos, GWL_STYLE, HWND_NOTOPMOST, HWND_TOPMOST, SWP_FRAMECHANGED,
    SWP_NOACTIVATE, SWP_SHOWWINDOW, WS_CAPTION, WS_THICKFRAME,
};

#[derive(Debug, Clone, Copy)]
pub struct Spec {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
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

/// First visible top-level window owned by `pid` that is at least 160px wide.
pub fn find_main_window(pid: u32) -> Option<HWND> {
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

/// Apply the requested geometry and window style.
pub fn adopt(hwnd: HWND, spec: &Spec) {
    unsafe {
        if spec.borderless {
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            let stripped = style & !(WS_CAPTION | WS_THICKFRAME);
            if stripped != style {
                SetWindowLongPtrW(hwnd, GWL_STYLE, stripped as isize);
            }
        }

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
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}

/// Wait for the engine's window to appear, then take it over.
///
/// Returns `Err` with a human reason so the plugin can log something useful
/// instead of failing silently.
pub fn adopt_when_ready(pid: u32, spec: Spec, timeout_ms: u64) -> Result<(), String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    loop {
        if let Some(hwnd) = find_main_window(pid) {
            adopt(hwnd, &spec);
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(format!("{timeout_ms}ms 内没等到引擎窗口（pid {pid}）"));
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
}
