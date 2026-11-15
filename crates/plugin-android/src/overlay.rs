//! A small "rotate" button drawn on top of the cast window.
//!
//! Clicking it asks the plugin to switch the capture orientation. scrcpy has
//! no runtime rotation for a locked app, so the plugin restarts the session -
//! the button is the trigger, not the mechanism.

#![cfg(windows)]

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect,
    SelectObject, SetBkMode, SetTextColor, CLEARTYPE_QUALITY, DEFAULT_CHARSET, DEFAULT_PITCH,
    DT_CENTER, DT_SINGLELINE, DT_VCENTER, HGDIOBJ, PAINTSTRUCT, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW,
    GetWindowRect, LoadCursorW, MoveWindow, PostQuitMessage, RegisterClassW, SetTimer,
    SetWindowPos, ShowWindow, TranslateMessage, HWND_TOPMOST, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, IDC_HAND,
    MSG, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_SHOWNOACTIVATE, WM_DESTROY, WM_LBUTTONUP,
    WM_PAINT, WM_TIMER, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
    WS_VISIBLE,
};

use crate::window::find_window;

const BUTTON_SIZE: i32 = 40;
const CLASS_NAME: &str = "CastPaneRotateButton";

static CLICK_TX: OnceLock<Mutex<Option<Sender<()>>>> = OnceLock::new();
static TARGET_PID: AtomicU32 = AtomicU32::new(0);
static OVERLAY_HWND: AtomicIsize = AtomicIsize::new(0);
static STARTED: AtomicBool = AtomicBool::new(false);

fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | ((g as u32) << 8) | ((b as u32) << 16)
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe extern "system" fn wndproc(
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
                24,
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
                wide("Segoe UI Symbol").as_ptr(),
            );
            let old_font = SelectObject(hdc, font as HGDIOBJ);
            SetBkMode(hdc, TRANSPARENT as i32);
            SetTextColor(hdc, rgb(232, 236, 244));

            let mut rect: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut rect);
            let glyph = wide("\u{27F3}");
            DrawTextW(
                hdc,
                glyph.as_ptr(),
                -1,
                &mut rect,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );

            SelectObject(hdc, old_font);
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
        WM_TIMER => {
            follow_target(hwnd);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Keep the button at the top-right corner of the cast window, above it.
fn follow_target(hwnd: HWND) {
    let pid = TARGET_PID.load(Ordering::Relaxed);
    if pid == 0 {
        return;
    }
    let Some(target) = find_window(pid) else { return };
    unsafe {
        let mut rect: RECT = std::mem::zeroed();
        if GetWindowRect(target, &mut rect) == 0 {
            return;
        }
        // Below the title bar, so it does not cover minimise/maximise/close.
        let x = rect.right - BUTTON_SIZE - 8;
        let y = rect.top + 44;
        MoveWindow(hwnd, x, y, BUTTON_SIZE, BUTTON_SIZE, 1);
        SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// Register the click channel once, before any session starts.
pub fn init(tx: Sender<()>) {
    if let Some(slot) = CLICK_TX.get() {
        if let Ok(mut guard) = slot.lock() {
            *guard = Some(tx);
        }
        return;
    }
    let slot = CLICK_TX.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = slot.lock() {
        *guard = Some(tx);
    }
}

/// Point the button at an engine process, creating the window on first use.
pub fn attach(pid: u32) {
    TARGET_PID.store(pid, Ordering::Relaxed);
    if OVERLAY_HWND.load(Ordering::Relaxed) != 0 {
        return; // already created; it follows the new pid
    }

    std::thread::spawn(move || unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class_name = wide(CLASS_NAME);

        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.style = CS_HREDRAW | CS_VREDRAW;
        wc.lpfnWndProc = Some(wndproc);
        wc.hInstance = hinstance;
        wc.hCursor = LoadCursorW(std::ptr::null_mut(), IDC_HAND);
        wc.lpszClassName = class_name.as_ptr();
        RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name.as_ptr(),
            wide("rotate").as_ptr(),
            WS_POPUP | WS_VISIBLE,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            BUTTON_SIZE,
            BUTTON_SIZE,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return;
        }
        OVERLAY_HWND.store(hwnd as isize, Ordering::Relaxed);
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetTimer(hwnd, 1, 200, None);
        follow_target(hwnd);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        DestroyWindow(hwnd);
        OVERLAY_HWND.store(0, Ordering::Relaxed);
    });

    STARTED.store(true, Ordering::Relaxed);
    let _ = Duration::from_millis(0); // keep imports tidy
}
