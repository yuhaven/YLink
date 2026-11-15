"""Drive the android plugin once, with the real phone, for a live check.

Starts a session through the plugin (so the exact production path is used),
inspects the scrcpy window geometry, then stops it.

Usage: python scripts/live_cast.py [seconds]
"""

import ctypes
import ctypes.wintypes as wt
import json
import subprocess
import sys
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PLUGIN = ROOT / "plugins" / "android" / "bin" / "android-plugin.exe"
SERIAL = "37R0224308003011"

user32 = ctypes.windll.user32


def screen() -> tuple[int, int]:
    return user32.GetSystemMetrics(0), user32.GetSystemMetrics(1)


def portrait_rect() -> dict:
    """Same rule as the core: 90% of the work area height, 9:16, centred."""
    sw, sh = screen()
    work_h = sh - 80  # taskbar allowance, mirrors apps/desktop/src-tauri/src/lib.rs
    h = round(work_h * 0.90)
    w = round(h * 9 / 16)
    return {
        "x": (sw - w) // 2,
        "y": (work_h - h) // 2,
        "width": w,
        "height": h,
        "borderless": True,
        "alwaysOnTop": True,
    }


class Plugin:
    def __init__(self) -> None:
        self.proc = subprocess.Popen(
            [str(PLUGIN)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            encoding="utf-8",
            bufsize=1,
        )
        self.next_id = 1

    def call(self, method: str, params: dict | None = None) -> dict:
        req = {
            "jsonrpc": "2.0",
            "id": self.next_id,
            "method": method,
            "params": params or {},
        }
        self.next_id += 1
        assert self.proc.stdin and self.proc.stdout
        self.proc.stdin.write(json.dumps(req) + "\n")
        self.proc.stdin.flush()
        while True:
            line = self.proc.stdout.readline()
            if not line:
                raise RuntimeError("plugin closed stdout")
            msg = json.loads(line)
            if msg.get("id") == req["id"]:
                if "error" in msg:
                    raise RuntimeError(f"{method}: {msg['error']}")
                return msg.get("result", {})

    def close(self) -> None:
        try:
            self.call("shutdown")
        except Exception:
            pass
        try:
            self.proc.wait(timeout=5)
        except Exception:
            self.proc.kill()


def find_scrcpy_window():
    """Return (hwnd, x, y, w, h, style, exstyle) of a visible scrcpy window."""
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
                style = user32.GetWindowLongW(hwnd, -16)
                exstyle = user32.GetWindowLongW(hwnd, -20)
                found.append(
                    (
                        hwnd,
                        rect.left,
                        rect.top,
                        rect.right - rect.left,
                        rect.bottom - rect.top,
                        style,
                        exstyle,
                    )
                )
                return False
        return True

    user32.EnumWindows(enum_proc, 0)
    return found[0] if found else None


def scrcpy_pids() -> set[int]:
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


def main() -> int:
    seconds = int(sys.argv[1]) if len(sys.argv) > 1 else 10
    window = portrait_rect()
    print(f"screen={screen()} requested={window}")

    plugin = Plugin()
    try:
        print("initialize:", plugin.call("initialize", {"api": 0}))
        plugin.call("discover.start")
        time.sleep(0.5)

        result = plugin.call(
            "session.start",
            {
                "deviceId": SERIAL,
                "window": window,
                "settings": {"maxSize": 1080, "maxFps": 60, "audio": True},
            },
        )
        print("session:", result)

        deadline = time.time() + 8
        geometry = None
        while time.time() < deadline:
            time.sleep(0.5)
            geometry = find_scrcpy_window()
            if geometry:
                break

        if geometry:
            hwnd, x, y, w, h, style, exstyle = geometry
            topmost = bool(exstyle & 0x00000008)
            caption = bool(style & 0x00C00000)
            print(
                f"window: x={x} y={y} w={w} h={h} ratio={w / h:.3f} "
                f"topmost={topmost} caption={caption}"
            )

            # Try to drag it away; the plugin is supposed to pin it back.
            user32.SetWindowPos(hwnd, 0, 10, 10, w, h, 0x0004 | 0x0010)
            time.sleep(1.2)
            after = find_scrcpy_window()
            if after:
                _, ax, ay, aw, ah, *_ = after
                pinned = abs(ax - x) <= 2 and abs(ay - y) <= 2
                print(f"after nudge: x={ax} y={ay} w={aw} h={ah} pinned={pinned}")
            else:
                print("after nudge: window vanished")
        else:
            print("window: not found")

        shown_at = time.time()
        print(f"staying visible for ~{seconds}s")
        time.sleep(seconds)

        print(f"visible for {time.time() - shown_at:.1f}s")
        print("stop:", plugin.call("session.stop"))
    finally:
        plugin.close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
