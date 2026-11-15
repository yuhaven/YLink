"""Launch the packaged app, screenshot its window, then close it.

Used to verify that the release build serves the embedded frontend instead of
http://localhost:1420.
"""

import ctypes
import ctypes.wintypes as wt
import subprocess
import sys
import time
from pathlib import Path

from PIL import ImageGrab

ROOT = Path(__file__).resolve().parent.parent
EXE = ROOT / "dist" / "CastPane" / "CastPane.exe"
OUT = ROOT / "build" / "ui.png"

user32 = ctypes.windll.user32


def find_window(pid: int):
    found = []

    @ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def enum_proc(hwnd, _):
        owner = wt.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid and user32.IsWindowVisible(hwnd):
            rect = wt.RECT()
            user32.GetWindowRect(hwnd, ctypes.byref(rect))
            if rect.right - rect.left > 200:
                found.append((hwnd, rect))
                return False
        return True

    user32.EnumWindows(enum_proc, 0)
    return found[0] if found else None


def main() -> int:
    proc = subprocess.Popen([str(EXE)])
    try:
        deadline = time.time() + 15
        window = None
        while time.time() < deadline:
            time.sleep(0.4)
            window = find_window(proc.pid)
            if window:
                break

        if not window:
            print("window not found")
            return 1

        hwnd, rect = window
        user32.SetForegroundWindow(hwnd)
        time.sleep(1.0)

        box = (rect.left, rect.top, rect.right, rect.bottom)
        ImageGrab.grab(bbox=box).save(OUT)
        print(f"saved {OUT} box={box} size={rect.right - rect.left}x{rect.bottom - rect.top}")
        return 0
    finally:
        time.sleep(0.2)
        proc.terminate()
        try:
            proc.wait(timeout=5)
        except Exception:
            proc.kill()


if __name__ == "__main__":
    raise SystemExit(main())
