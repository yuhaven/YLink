"""Measure and drag the scrcpy window. Called by e2e_cast.ps1."""

import ctypes
import ctypes.wintypes as wt
import json
import subprocess
import sys
import time

user32 = ctypes.windll.user32


def scrcpy_pids():
    out = subprocess.run(
        ["tasklist", "/FI", "IMAGENAME eq scrcpy.exe", "/FO", "CSV", "/NH"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="ignore",
    ).stdout
    pids = set()
    for line in out.splitlines():
        parts = [p.strip('"') for p in line.split('","')]
        if len(parts) >= 2 and parts[1].isdigit():
            pids.add(int(parts[1]))
    return pids


def find_window():
    pids = scrcpy_pids()
    if not pids:
        return None
    found = []

    @ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def enum_proc(hwnd, _):
        pid = wt.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        if pid.value in pids and user32.IsWindowVisible(hwnd):
            rect = wt.RECT()
            user32.GetWindowRect(hwnd, ctypes.byref(rect))
            if rect.right - rect.left > 100:
                found.append((hwnd, rect.left, rect.top,
                              rect.right - rect.left, rect.bottom - rect.top))
                return False
        return True

    user32.EnumWindows(enum_proc, 0)
    return found[0] if found else None


def measure():
    win = find_window()
    if not win:
        return None
    hwnd, x, y, w, h = win
    style = user32.GetWindowLongW(hwnd, -16)
    exstyle = user32.GetWindowLongW(hwnd, -20)
    return {
        "hwnd": hwnd,
        "x": x,
        "y": y,
        "w": w,
        "h": h,
        "ratio": round(w / h, 3),
        "caption": bool(style & 0x00C00000),
        "minimize_box": bool(style & 0x00020000),
        "maximize_box": bool(style & 0x00010000),
        "topmost": bool(exstyle & 0x00000008),
    }


def drag(dx: int):
    win = find_window()
    if not win:
        return None
    _, x, y, w, h = win
    start_x = x + w // 2
    # Middle of the native title bar. y+6 lands on the resize border, which
    # starts a resize instead of a move.
    start_y = y + 30
    user32.SetCursorPos(start_x, start_y)
    time.sleep(0.15)
    user32.mouse_event(0x0002, 0, 0, 0, 0)  # left down
    for step in range(1, 11):
        user32.SetCursorPos(start_x + dx * step // 10, start_y)
        time.sleep(0.03)
    user32.mouse_event(0x0004, 0, 0, 0, 0)  # left up
    time.sleep(0.5)
    return measure()


def controls():
    """Exercise minimise / restore / maximise on the cast window."""
    win = find_window()
    if not win:
        return {"ok": False}
    hwnd, x, y, w, h = win
    out = {}

    user32.ShowWindow(hwnd, 6)  # SW_MINIMIZE
    time.sleep(0.6)
    out["minimized"] = bool(user32.IsIconic(hwnd))

    user32.ShowWindow(hwnd, 9)  # SW_RESTORE
    time.sleep(0.6)
    out["restored"] = not bool(user32.IsIconic(hwnd))

    user32.ShowWindow(hwnd, 3)  # SW_MAXIMIZE
    time.sleep(0.7)
    rect = wt.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    width = rect.right - rect.left
    out["maximized_width"] = width
    out["maximized"] = width >= 1900

    user32.ShowWindow(hwnd, 9)
    time.sleep(0.5)
    out["ok"] = out["minimized"] and out["restored"] and out["maximized"]
    return out


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else "measure"
    if cmd == "drag":
        before = measure()
        after = drag(int(sys.argv[2]) if len(sys.argv) > 2 else 150)
        print(json.dumps({"before": before, "after": after,
                          "moved": bool(before and after and before["x"] != after["x"])}))
    elif cmd == "controls":
        print(json.dumps(controls()))
    else:
        print(json.dumps(measure()))
