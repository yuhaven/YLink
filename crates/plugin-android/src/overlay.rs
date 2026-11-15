//! Floating controls drawn on top of the cast window.
//!
//! - a rotate button at the top-right corner (switches capture orientation by
//!   asking the plugin to restart the session);
//! - a three-key navigation bar (Back / Home / Recents) at the bottom-right,
//!   which injects `adb shell input keyevent` into the phone.
//!
//! Both are borderless, non-activating topmost popups owned by this plugin and
//! follow the cast window at ~8ms so dragging feels immediate.

#![cfg(windows)]

use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, Ellipse, EndPaint, FillRect,
    Polygon, Rectangle, SelectObject, SetBkMode, SetTextColor, CLEARTYPE_QUALITY, DEFAULT_CHARSET,
    DEFAULT_PITCH, DT_CENTER, DT_SINGLELINE, DT_VCENTER, HGDIOBJ, PAINTSTRUCT, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW,
    GetWindowRect, LoadCursorW, PostQuitMessage, RegisterClassW, SetWindowPos, ShowWindow,
    TranslateMessage, HWND_TOPMOST, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, IDC_HAND, MSG, SWP_NOACTIVATE,
    SWP_NOZORDER, SW_SHOWNOACTIVATE, WM_LBUTTONUP, WM_PAINT, WNDCLASSW,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_VISIBLE,
};

use crate::window::find_window;

const ROTATE_CLASS: &str = "CastPaneRotateButton";
const NAV_CLASS: &str = "CastPaneNavBar";
const BUTTON: i32 = 40;
const NAV_CELL: i32 = 40;
const NAV_WIDTH: i32 = NAV_CELL * 3;

static CLICK_TX: OnceLock<Mutex<Option<Sender<()>>>> = OnceLock::new();
static TARGET_PID: AtomicU32 = AtomicU32::new(0);
static ROTATE_HWND: AtomicIsize = AtomicIsize::new(0);
static NAV_HWND: AtomicIsize = AtomicIsize::new(0);
static SERIAL: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static TRACKER_STARTED: AtomicBool = AtomicBool::new(false);

fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | ((g as u32) << 8) | ((b as u32) << 16)
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn serial() -> Option<String> {
    SERIAL
        .get()
        .and_then(|slot| slot.lock().ok())
        .and_then(|guard| guard.clone())
}

/// `adb shell input keyevent <code>` with no console window.
fn send_key(code: u32) {
    let Some(serial) = serial() else { return };
    let Some(exe) = std::env::current_exe().ok() else {
        return;
    };
    let adb = exe
        .parent()
        .map(|dir| dir.join("scrcpy").join("adb.exe"))
        .unwrap_or_else(|| "adb".into());

    let mut command = Command::new(adb);
    command.args([
        "-s",
        &serial,
        "shell",
        "input",
        "keyevent",
        &code.to_string(),
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let _ = command.spawn();
}

unsafe extern "system" fn rotate_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            let brush = CreateSolidBrush(rgb(28, 31, 38));
            FillRect(hdc, &ps.rcPaint, brush);
            DeleteObject(brush as HGDIOBJ);

            let font = CreateFontW(
                20,
                0,
                0,
                0,
                600,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                0,
                0,
                CLEARTYPE_QUALITY as u32,
                DEFAULT_PITCH as u32,
                wide("Segoe MDL2 Assets").as_ptr(),
            );
            let old = SelectObject(hdc, font as HGDIOBJ);
            SetBkMode(hdc, TRANSPARENT as i32);
            SetTextColor(hdc, rgb(232, 236, 244));
            let mut rect: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut rect);
            let glyph = wide("\u{E7AD}");
            DrawTextW(
                hdc,
                glyph.as_ptr(),
                -1,
                &mut rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            SelectObject(hdc, old);
            DeleteObject(font as HGDIOBJ);
            EndPaint(hwnd, &ps);
            0
        }
        WM_LBUTTONUP => {
            if let Some(slot) = CLICK_TX.get() {
                if let Ok(guard) = slot.lock() {
                    if let Some(tx) = guard.as_ref() {
                        let _ = tx.send(());
                    }
                }
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn nav_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            let brush = CreateSolidBrush(rgb(28, 31, 38));
            FillRect(hdc, &ps.rcPaint, brush);

            let white = CreateSolidBrush(rgb(232, 236, 244));
            let old = SelectObject(hdc, white as HGDIOBJ);

            // Back: triangle.
            let mut back = [
                windows_sys::Win32::Foundation::POINT { x: 26, y: 12 },
                windows_sys::Win32::Foundation::POINT { x: 26, y: 28 },
                windows_sys::Win32::Foundation::POINT { x: 14, y: 20 },
            ];
            Polygon(hdc, back.as_mut_ptr(), 3);

            // Home: circle.
            Ellipse(hdc, NAV_CELL + 11, 11, NAV_CELL + 29, 29);

            // Recents: square.
            Rectangle(hdc, NAV_CELL * 2 + 12, 12, NAV_CELL * 2 + 28, 28);

            SelectObject(hdc, old);
            DeleteObject(white as HGDIOBJ);
            EndPaint(hwnd, &ps);
            0
        }
        WM_LBUTTONUP => {
            let x = (lparam & 0xffff) as i16 as i32;
            let cell = (x / NAV_CELL).clamp(0, 2);
            // KEYCODE_BACK = 4, KEYCODE_HOME = 3, KEYCODE_APP_SWITCH = 187
            let code = [4u32, 3, 187][cell as usize];
            std::thread::spawn(move || send_key(code));
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Keep both bars pinned to the cast window. 8ms keeps up with dragging.
fn track() {
    loop {
        std::thread::sleep(Duration::from_millis(8));
        let pid = TARGET_PID.load(Ordering::Relaxed);
        if pid == 0 {
            continue;
        }
        let Some(target) = find_window(pid) else { continue };
        unsafe {
            let mut rect: RECT = std::mem::zeroed();
            if GetWindowRect(target, &mut rect) == 0 {
                continue;
            }
            let rotate = ROTATE_HWND.load(Ordering::Relaxed) as HWND;
            if !rotate.is_null() {
                SetWindowPos(
                    rotate,
                    HWND_TOPMOST,
                    rect.right - BUTTON - 8,
                    rect.top + 44,
                    BUTTON,
                    BUTTON,
                    SWP_NOACTIVATE,
                );
            }
            let nav = NAV_HWND.load(Ordering::Relaxed) as HWND;
            if !nav.is_null() {
                SetWindowPos(
                    nav,
                    HWND_TOPMOST,
                    rect.right - NAV_WIDTH - 8,
                    rect.bottom - NAV_CELL - 12,
                    NAV_WIDTH,
                    NAV_CELL,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
            }
        }
    }
}

fn create_window(class: &str, proc: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT, width: i32, height: i32) -> HWND {
    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class_name = wide(class);
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.style = CS_HREDRAW | CS_VREDRAW;
        wc.lpfnWndProc = Some(proc);
        wc.hInstance = hinstance;
        wc.hCursor = LoadCursorW(std::ptr::null_mut(), IDC_HAND);
        wc.lpszClassName = class_name.as_ptr();
        RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name.as_ptr(),
            wide(class).as_ptr(),
            WS_POPUP | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            width,
            height,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );
        if !hwnd.is_null() {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
        hwnd
    }
}

/// Register the rotate-click channel once.
pub fn init(tx: Sender<()>) {
    let slot = CLICK_TX.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = slot.lock() {
        *guard = Some(tx);
    }
}

/// Point the controls at an engine process (creating them on first use).
pub fn attach(pid: u32, device_serial: &str) {
    TARGET_PID.store(pid, Ordering::Relaxed);
    let slot = SERIAL.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = slot.lock() {
        *guard = Some(device_serial.to_string());
    }

    if ROTATE_HWND.load(Ordering::Relaxed) == 0 {
        std::thread::spawn(move || unsafe {
            let rotate = create_window(ROTATE_CLASS, rotate_proc, BUTTON, BUTTON);
            ROTATE_HWND.store(rotate as isize, Ordering::Relaxed);
            let nav = create_window(NAV_CLASS, nav_proc, NAV_WIDTH, NAV_CELL);
            NAV_HWND.store(nav as isize, Ordering::Relaxed);

            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if !rotate.is_null() {
                DestroyWindow(rotate);
            }
            if !nav.is_null() {
                DestroyWindow(nav);
            }
            PostQuitMessage(0);
        });
    }

    if !TRACKER_STARTED.swap(true, Ordering::Relaxed) {
        std::thread::spawn(track);
    }
}

/// Kept for API compatibility with the rest of the plugin.
pub fn _unused(_stop: Arc<AtomicBool>) {}
