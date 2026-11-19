"""Screenshot one top-level window.

Usage:
    python scripts/window_shot.py <pid|process-name> <out.png>

Used by the wireless E2E script so evidence is captured from the real window,
not a browser mock.
"""

import ctypes
import ctypes.wintypes as wt
import subprocess
import sys
import time
from pathlib import Path

from PIL import ImageGrab

user32 = ctypes.windll.user32


def pids_for(target: str) -> set[int]:
    if target.isdigit():
        return {int(target)}
    name = target if target.lower().endswith(".exe") else target + ".exe"
    out = subprocess.run(
        ["tasklist", "/FI", f"IMAGENAME eq {name}", "/FO", "CSV", "/NH"],
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


def find_window(pids: set[int]):
    found = []

    @ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def enum_proc(hwnd, _):
        pid = wt.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
        if pid.value in pids and user32.IsWindowVisible(hwnd):
            rect = wt.RECT()
            user32.GetWindowRect(hwnd, ctypes.byref(rect))
            if rect.right - rect.left > 100:
                found.append((hwnd, rect))
                return False
        return True

    user32.EnumWindows(enum_proc, 0)
    return found[0] if found else None


def main() -> int:
    if len(sys.argv) < 3:
        print("usage: window_shot.py <pid|process-name> <out.png>")
        return 2
    pids = pids_for(sys.argv[1])
    if not pids:
        print("no process")
        return 1
    deadline = time.time() + 10
    window = None
    while time.time() < deadline:
        window = find_window(pids)
        if window:
            break
        time.sleep(0.3)
    if not window:
        print("no window")
        return 1
    hwnd, rect = window
    user32.SetForegroundWindow(hwnd)
    time.sleep(0.6)
    out = Path(sys.argv[2])
    out.parent.mkdir(parents=True, exist_ok=True)
    ImageGrab.grab(bbox=(rect.left, rect.top, rect.right, rect.bottom)).save(out)
    print(f"saved {out} {rect.right - rect.left}x{rect.bottom - rect.top}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
