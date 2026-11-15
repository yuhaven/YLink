"""Verify the rotate button drawn on the cast window.

Starts a session through the plugin, clicks the overlay button, and checks the
window switches portrait -> landscape -> portrait without restarting anything.
"""

import ctypes
import ctypes.wintypes as wt
import json
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PLUGIN = ROOT / "plugins" / "android" / "bin" / "android-plugin.exe"
SERIAL = "37R0224308003011"

user32 = ctypes.windll.user32


def scrcpy_pids():
    out = subprocess.run(
        ["tasklist", "/FI", "IMAGENAME eq scrcpy.exe", "/FO", "CSV", "/NH"],
        capture_output=True, text=True, encoding="utf-8", errors="ignore",
    ).stdout
    pids = set()
    for line in out.splitlines():
        parts = [p.strip('"') for p in line.split('","')]
        if len(parts) >= 2 and parts[1].isdigit():
            pids.add(int(parts[1]))
    return pids


def find_cast_window():
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
                found.append((hwnd, rect.right - rect.left, rect.bottom - rect.top))
                return False
        return True

    user32.EnumWindows(enum_proc, 0)
    return found[0] if found else None


def ratio():
    win = find_cast_window()
    if not win:
        return None
    _, w, h = win
    return round(w / h, 3)


def click(hwnd):
    rect = wt.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    x = (rect.left + rect.right) // 2
    y = (rect.top + rect.bottom) // 2
    user32.SetCursorPos(x, y)
    time.sleep(0.15)
    user32.mouse_event(0x0002, 0, 0, 0, 0)
    user32.mouse_event(0x0004, 0, 0, 0, 0)


class Plugin:
    def __init__(self):
        self.proc = subprocess.Popen(
            [str(PLUGIN)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL, text=True, encoding="utf-8", bufsize=1,
        )
        self.next_id = 1

    def call(self, method, params=None):
        req = {"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params or {}}
        self.next_id += 1
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

    def close(self):
        try:
            self.call("shutdown")
            self.proc.wait(timeout=5)
        except Exception:
            self.proc.kill()


def main():
    sw = user32.GetSystemMetrics(0)
    sh = user32.GetSystemMetrics(1)
    height = round((sh - 80) * 0.90)
    width = round(height * 9 / 16)
    window = {"x": (sw - width) // 2, "y": (sh - height) // 2,
              "width": width, "height": height,
              "borderless": False, "alwaysOnTop": False}

    plugin = Plugin()
    result = {"before": None, "after_first": None, "after_second": None}
    try:
        plugin.call("initialize", {"api": 0})
        plugin.call("session.start", {
            "deviceId": SERIAL, "window": window,
            "settings": {"orientation": "portrait", "audio": True,
                         "maxSize": 1080, "maxFps": 60, "lockPosition": False},
        })

        deadline = time.time() + 20
        while time.time() < deadline and result["before"] is None:
            time.sleep(0.4)
            result["before"] = ratio()
        print("before:", result["before"])

        deadline = time.time() + 10
        button = None
        while time.time() < deadline and not button:
            button = user32.FindWindowW("CastPaneRotateButton", None)
            if not button:
                time.sleep(0.3)
        if not button:
            raise RuntimeError("rotate button window not found")
        print("button hwnd:", button)

        click(button)
        time.sleep(2.0)
        result["after_first"] = ratio()
        print("after first click:", result["after_first"])

        click(button)
        time.sleep(2.0)
        result["after_second"] = ratio()
        print("after second click:", result["after_second"])
    finally:
        plugin.close()

    ok = (result["before"] and result["before"] < 0.8
          and result["after_first"] and result["after_first"] > 1.2
          and result["after_second"] and result["after_second"] < 0.8)
    print(json.dumps(result))
    print("PASS" if ok else "FAIL")
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
