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
    return {"hwnd": hwnd, "x": x, "y": y, "w": w, "h": h, "ratio": round(w / h, 3)}


def drag(dx: int):
    win = find_window()
    if not win:
        return None
    _, x, y, w, h = win
    start_x = x + w // 2
    start_y = y + 6  # inside the 12px drag strip
    user32.SetCursorPos(start_x, start_y)
    time.sleep(0.15)
    user32.mouse_event(0x0002, 0, 0, 0, 0)  # left down
    for step in range(1, 11):
        user32.SetCursorPos(start_x + dx * step // 10, start_y)
        time.sleep(0.03)
    user32.mouse_event(0x0004, 0, 0, 0, 0)  # left up
    time.sleep(0.5)
    return measure()


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else "measure"
    if cmd == "drag":
        before = measure()
        after = drag(int(sys.argv[2]) if len(sys.argv) > 2 else 150)
        print(json.dumps({"before": before, "after": after,
                          "moved": bool(before and after and before["x"] != after["x"])}))
    else:
        print(json.dumps(measure()))
