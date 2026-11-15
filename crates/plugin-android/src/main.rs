#![cfg_attr(windows, windows_subsystem = "windows")]
//! Android plugin: speaks `docs/plugin-api.md` over stdio and drives scrcpy.
//!
//! Responsibilities are deliberately narrow:
//!   - discover devices via `adb devices -l`
//!   - launch scrcpy with the window rectangle the host computed
//!   - report lifecycle back to the host
//!
//! It never renders video and never touches the screen.

mod adb;
mod overlay;
mod scrcpy;
mod window;

use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const NAME: &str = "Android";

static OUT: OnceLock<Mutex<std::io::Stdout>> = OnceLock::new();
static SESSION: OnceLock<Mutex<Option<Running>>> = OnceLock::new();

struct Running {
    id: String,
    child: Arc<Mutex<Child>>,
    /// Tells the window-pinning thread to stop.
    stop: Arc<AtomicBool>,
    /// Engine process id, used for live window-level settings.
    pid: u32,
}

fn stdout() -> &'static Mutex<std::io::Stdout> {
    OUT.get_or_init(|| Mutex::new(std::io::stdout()))
}

fn session() -> &'static Mutex<Option<Running>> {
    SESSION.get_or_init(|| Mutex::new(None))
}

fn send(obj: Value) {
    let mut out = stdout().lock().unwrap();
    let _ = writeln!(out, "{obj}");
    let _ = out.flush();
}

fn reply(id: Value, result: Value) {
    send(json!({"jsonrpc": "2.0", "id": id, "result": result}));
}

fn reply_error(id: Value, code: &str, message: impl Into<String>, hint: Option<String>) {
    send(json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message.into(), "hint": hint }
    }));
}

fn emit(method: &str, params: Value) {
    send(json!({"jsonrpc": "2.0", "method": method, "params": params}));
}

/// `<plugin dir>/bin/scrcpy/{scrcpy.exe,adb.exe}`, overridable for tests.
fn engine_paths() -> (PathBuf, PathBuf) {
    let dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("scrcpy")))
        .unwrap_or_else(|| PathBuf::from("scrcpy"));

    let scrcpy = std::env::var_os("CASTPANE_SCRCPY")
        .map(PathBuf::from)
        .unwrap_or_else(|| dir.join("scrcpy.exe"));
    let adb = std::env::var_os("CASTPANE_ADB")
        .map(PathBuf::from)
        .unwrap_or_else(|| dir.join("adb.exe"));

    (scrcpy, adb)
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Portrait video becomes a wide window; 90% of the screen height, 16:9.
#[cfg(windows)]
fn landscape_rect() -> Option<(i32, i32, u32, u32)> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    unsafe {
        let screen_w = GetSystemMetrics(SM_CXSCREEN);
        let screen_h = GetSystemMetrics(SM_CYSCREEN);
        if screen_w <= 0 || screen_h <= 0 {
            return None;
        }
        let work_h = screen_h - 80;
        let height = (work_h as f64 * 0.90) as i32;
        let width = (height as f64 * 16.0 / 9.0) as i32;
        Some((
            // Right-aligned, away from the centred console.
            (screen_w - width - 40).max(0),
            (screen_h - height) / 2,
            width as u32,
            height as u32,
        ))
    }
}

fn discover() {
    let (_, adb) = engine_paths();
    if !adb.exists() {
        emit(
            "log",
            json!({"level": "warn", "message": format!("adb 未找到：{}", adb.display())}),
        );
        return;
    }
    match adb::list_devices(&adb) {
        Ok(devices) => {
            for device in devices {
                emit(
                    "device.found",
                    json!({
                        "id": device.serial,
                        "name": device.name,
                        "kind": "android",
                        "detail": device.state,
                    }),
                );
            }
        }
        Err(err) => emit("log", json!({"level": "error", "message": err})),
    }
}

fn start_session(id: Value, params: Value) {
    let (scrcpy, adb) = engine_paths();

    if !scrcpy.exists() {
        reply_error(
            id,
            "E_ENGINE_MISSING",
            format!("找不到 scrcpy：{}", scrcpy.display()),
            Some("运行 scripts/fetch-engines.ps1，或手动把 scrcpy 解压到 plugins/android/bin/scrcpy/".into()),
        );
        return;
    }

    let Some(device_id) = params.get("deviceId").and_then(Value::as_str) else {
        reply_error(id, "E_NO_DEVICE", "session.start 缺少 deviceId", None);
        return;
    };

    // Refuse early with a human reason instead of letting scrcpy fail silently.
    if adb.exists() {
        if let Ok(devices) = adb::list_devices(&adb) {
            match devices.iter().find(|d| d.serial == device_id) {
                None => {
                    reply_error(
                        id,
                        "E_NO_DEVICE",
                        format!("设备 {device_id} 不在列表中"),
                        Some("重新插拔数据线，或点「重新扫描设备」".into()),
                    );
                    return;
                }
                Some(d) if d.state != "device" => {
                    let hint = if d.state == "unauthorized" {
                        "在手机上点「允许 USB 调试」"
                    } else {
                        "解锁手机并保持屏幕点亮"
                    };
                    reply_error(
                        id,
                        "E_PERMISSION",
                        format!("设备状态为 {}", d.state),
                        Some(hint.into()),
                    );
                    return;
                }
                Some(_) => {}
            }
        }
    }

    let settings: scrcpy::Settings = params
        .get("settings")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();
    let window: Option<scrcpy::WindowSpec> = params
        .get("window")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    let cfg = Config {
        device_id: device_id.to_string(),
        settings,
        window,
    };

    match launch(&cfg) {
        Ok(sid) => {
            *config().lock().unwrap() = Some(cfg);
            reply(id, json!({"sessionId": sid}));
            emit(
                "session.state",
                json!({"sessionId": sid, "state": "connecting"}),
            );
        }
        Err(err) => reply_error(
            id,
            "E_ENGINE_FAILED",
            format!("cannot start scrcpy: {err}"),
            None,
        ),
    }
}

#[derive(Clone)]
struct Config {
    device_id: String,
    settings: scrcpy::Settings,
    window: Option<scrcpy::WindowSpec>,
}

static CONFIG: OnceLock<Mutex<Option<Config>>> = OnceLock::new();

fn config() -> &'static Mutex<Option<Config>> {
    CONFIG.get_or_init(|| Mutex::new(None))
}

/// Spawn the engine for a config and wire up waiter, overlay and pinning.
fn launch(cfg: &Config) -> Result<String, String> {
    let (scrcpy_path, _adb) = engine_paths();
    let settings = cfg.settings.clone();
    let mut window = cfg.window.clone();

    if settings.orientation == "landscape" {
        if let Some((x, y, width, height)) = landscape_rect() {
            window = Some(scrcpy::WindowSpec {
                x,
                y,
                width,
                height,
                borderless: true,
                always_on_top: false,
            });
        }
    }

    let title = format!("CastPane - {}", cfg.device_id);
    let args = scrcpy::build_args(&cfg.device_id, &title, &settings, window.as_ref());
    let child = scrcpy::spawn(&scrcpy_path, &args).map_err(|e| e.to_string())?;

    let sid = format!("{}-{}", cfg.device_id, now_millis());
    let pid = child.id();
    let child = Arc::new(Mutex::new(child));
    let stop = Arc::new(AtomicBool::new(false));

    *session().lock().unwrap() = Some(Running {
        id: sid.clone(),
        child: Arc::clone(&child),
        stop: Arc::clone(&stop),
        pid,
    });

    spawn_waiter(sid.clone(), child);

    #[cfg(windows)]
    {
        overlay::attach(pid, &cfg.device_id);
        if let Some(w) = window.as_ref() {
            let spec_json = json!({
                "x": w.x,
                "y": w.y,
                "width": w.width,
                "height": w.height,
                "alwaysOnTop": false,
            });
            if let Some(spec) = window::spec_from_json(&spec_json, settings.lock_position) {
                let stop = Arc::clone(&stop);
                std::thread::spawn(move || window::enforce(pid, spec, stop));
            }
        }
    }

    Ok(sid)
}

/// Switch capture orientation and restart the engine: scrcpy cannot rotate a
/// running session when the app on the phone is locked to one orientation.
/// Apply settings to a running session: window-level flags live, anything
/// that changes the capture parameters needs a session restart.
fn apply_settings(id: Value, params: Value) {
    let new_settings: scrcpy::Settings = params
        .get("settings")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let Some(mut cfg) = config().lock().unwrap().clone() else {
        reply(id, json!({"ok": true, "applied": "no-session"}));
        return;
    };

    if cfg.settings == new_settings {
        reply(id, json!({"ok": true, "applied": "noop"}));
        return;
    }

    cfg.settings = new_settings;
    restart_with(cfg);
    reply(id, json!({"ok": true, "applied": "restart"}));
}

/// Stop the current engine and start a new one with the given config.
fn restart_with(cfg: Config) {
    if let Some(running) = session().lock().unwrap().take() {
        running.stop.store(true, Ordering::Relaxed);
        if let Ok(mut child) = running.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    match launch(&cfg) {
        Ok(sid) => {
            *config().lock().unwrap() = Some(cfg);
            emit(
                "session.state",
                json!({"sessionId": sid, "state": "connecting"}),
            );
        }
        Err(err) => emit(
            "log",
            json!({"level": "error", "message": format!("restart failed: {err}")}),
        ),
    }
}

fn toggle_orientation() {
    let Some(mut cfg) = config().lock().unwrap().clone() else {
        return;
    };
    cfg.settings.orientation = if cfg.settings.orientation == "landscape" {
        "portrait".to_string()
    } else {
        "landscape".to_string()
    };
    restart_with(cfg);
}

/// Watch the engine process and report its death exactly once.
///
/// The engine may reject an argument and exit within a second, so "streaming"
/// is only announced after a short grace period with the process still alive.
fn spawn_waiter(session_id: String, child: Arc<Mutex<Child>>) {
    std::thread::spawn(move || {
        let started = Instant::now();
        let mut announced = false;

        loop {
            std::thread::sleep(Duration::from_millis(300));

            let exited = {
                let mut guard = match child.lock() {
                    Ok(guard) => guard,
                    Err(_) => return,
                };
                match guard.try_wait() {
                    Ok(status) => status,
                    Err(_) => return,
                }
            };

            if let Some(status) = exited {
                let error = if status.success() {
                    Value::Null
                } else {
                    json!({"code": "E_ENGINE_EXIT", "message": status.to_string()})
                };
                emit(
                    "session.state",
                    json!({"sessionId": session_id, "state": "stopped", "error": error}),
                );
                let mut slot = session().lock().unwrap();
                if slot.as_ref().map(|r| r.id == session_id).unwrap_or(false) {
                    *slot = None;
                }
                return;
            }

            if !announced && started.elapsed() >= Duration::from_millis(900) {
                announced = true;
                emit(
                    "session.state",
                    json!({"sessionId": session_id, "state": "streaming"}),
                );
            }

            if session().lock().map(|s| s.is_none()).unwrap_or(true) {
                return;
            }
        }
    });
}

fn stop_session(id: Value) {
    let running = session().lock().unwrap().take();
    if let Some(running) = running {
        running.stop.store(true, Ordering::Relaxed);
        if let Ok(mut child) = running.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
        emit(
            "session.state",
            json!({"sessionId": running.id, "state": "stopped"}),
        );
    }
    reply(id, json!({"ok": true}));
}

fn diagnose(id: Value) {
    let (scrcpy, adb) = engine_paths();
    let mut checks = Vec::new();

    checks.push(json!({
        "name": "scrcpy",
        "ok": scrcpy.exists(),
        "detail": scrcpy.display().to_string(),
        "hint": if scrcpy.exists() { Value::Null } else { json!("运行 scripts/fetch-engines.ps1") }
    }));
    checks.push(json!({
        "name": "adb",
        "ok": adb.exists(),
        "detail": adb.display().to_string(),
        "hint": if adb.exists() { Value::Null } else { json!("adb 随 scrcpy 一起发布") }
    }));

    if adb.exists() {
        let devices = adb::list_devices(&adb).unwrap_or_default();
        checks.push(json!({
            "name": "devices",
            "ok": !devices.is_empty(),
            "detail": format!("{} 台设备", devices.len()),
            "hint": if devices.is_empty() { json!("插好数据线并允许 USB 调试") } else { Value::Null }
        }));

        // Audio forwarding needs Android 13 (API 33) in scrcpy. Saying so beats
        // letting the user wonder why the phone still plays the sound.
        if let Some(device) = devices.first() {
            match adb::sdk_level(&adb, &device.serial) {
                Some(sdk) => checks.push(json!({
                    "name": "audio",
                    "ok": sdk >= 33,
                    "detail": format!("Android API {sdk}"),
                    "hint": if sdk >= 33 {
                        Value::Null
                    } else {
                        json!("音频转发需要 Android 13(API 33) 及以上，当前系统只能在本机播放")
                    }
                })),
                None => checks.push(json!({
                    "name": "audio",
                    "ok": false,
                    "detail": "无法读取系统版本",
                    "hint": "adb shell getprop 被拒绝"
                })),
            }
        }
    }

    reply(id, json!({"checks": checks}));
}

fn kill_session() {
    if let Some(running) = session().lock().unwrap().take() {
        running.stop.store(true, Ordering::Relaxed);
        if let Ok(mut child) = running.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn main() {
    #[cfg(windows)]
    {
        let (click_tx, click_rx) = std::sync::mpsc::channel::<()>();
        overlay::init(click_tx);
        std::thread::spawn(move || {
            while click_rx.recv().is_ok() {
                toggle_orientation();
            }
        });
    }

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(request) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        if id.is_null() {
            continue; // host notifications are not answered
        }
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

        match method {
            "initialize" => reply(
                id,
                json!({
                    "name": NAME,
                    "version": env!("CARGO_PKG_VERSION"),
                    "capabilities": ["discover", "session", "diagnose"]
                }),
            ),
            "discover.start" | "action.rescan" => {
                discover();
                reply(id, json!({"ok": true}));
            }
            "discover.stop" => reply(id, json!({"ok": true})),
            "settings.apply" => apply_settings(id, params),
            "session.start" => start_session(id, params),
            "session.stop" => stop_session(id),
            "diagnose" | "action.diagnose" => diagnose(id),
            "shutdown" => {
                kill_session();
                reply(id, json!({"ok": true}));
                std::process::exit(0);
            }
            other => reply_error(
                id,
                "E_UNSUPPORTED",
                format!("方法未实现：{other}"),
                Some("插件协议 v0 只支持 docs/plugin-api.md 列出的方法".into()),
            ),
        }
    }

    kill_session();
}
