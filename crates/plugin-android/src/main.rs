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
mod files;
mod overlay;
mod scrcpy;
mod wireless;
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
static WIRELESS: OnceLock<wireless::Wireless> = OnceLock::new();

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

fn wireless() -> &'static wireless::Wireless {
    WIRELESS.get_or_init(wireless::Wireless::default)
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
    let snapshot = wireless().scan(&adb);
    if snapshot.devices.is_empty() {
        emit(
            "log",
            json!({"level": "warn", "message": format!("{} — {}", snapshot.status, snapshot.hint.unwrap_or_default())}),
        );
        return;
    }
    for device in &snapshot.devices {
        emit(
            "device.found",
            json!({
                "id": device.id,
                "name": device.name,
                "kind": "android",
                // The old host only understands these four tokens; the rich
                // state is in the extra fields and in action.wireless_scan.
                "detail": if device.state == "ready" { "device" } else { device.state.as_str() },
                "transport": device.transport,
                "endpoint": device.endpoint,
                "label": device.detail,
            }),
        );
    }
}

fn wireless_scan(id: Value, _params: Value) {
    let (_, adb) = engine_paths();
    let snapshot = wireless().scan(&adb);
    reply(id, serde_json::to_value(snapshot).unwrap_or(Value::Null));
}

fn wireless_connect(id: Value, params: Value) {
    let Some(device_id) = params.get("deviceId").and_then(Value::as_str) else {
        reply_error(id, "E_NO_DEVICE", "缺少 deviceId", None);
        return;
    };
    let (_, adb) = engine_paths();
    let result = wireless().connect(&adb, device_id);
    reply(id, serde_json::to_value(result).unwrap_or(Value::Null));
}

fn wireless_usb_bootstrap(id: Value) {
    let (_, adb) = engine_paths();
    let result = wireless().usb_bootstrap(&adb);
    reply(id, serde_json::to_value(result).unwrap_or(Value::Null));
}

fn wireless_disconnect(id: Value, params: Value) {
    let Some(device_id) = params.get("deviceId").and_then(Value::as_str) else {
        reply_error(id, "E_NO_DEVICE", "缺少 deviceId", None);
        return;
    };
    let (_, adb) = engine_paths();
    let ok = wireless().disconnect(&adb, device_id);
    reply(id, json!({"ok": ok}));
}

/* ---------- file transfer ---------- */

fn ensure_device(adb: &std::path::Path, device_id: &str) -> Result<(), FileFailure> {
    if !adb.exists() {
        return Err(FileFailure {
            code: "E_ADB",
            message: format!("找不到 adb：{}", adb.display()),
            hint: Some("运行 scripts/fetch-engines.ps1".into()),
        });
    }
    let devices = adb::list_devices(adb).map_err(|err| FileFailure {
        code: "E_ADB",
        message: format!("读取设备列表失败：{err}"),
        hint: Some("确认 adb 没有被其他程序占用".into()),
    })?;
    let Some(device) = devices.iter().find(|d| d.serial == device_id) else {
        return Err(FileFailure {
            code: "E_OFFLINE",
            message: format!("手机 {device_id} 已离线"),
            hint: Some("确认手机与电脑在同一 Wi-Fi；回首页点「重新连接」".into()),
        });
    };
    if device.state != "device" {
        return Err(FileFailure {
            code: "E_PERMISSION",
            message: format!("手机状态为 {}，暂时不能读取文件", device.state),
            hint: Some("解锁手机；若提示授权，请在手机上点「允许」".into()),
        });
    }
    Ok(())
}

struct FileFailure {
    code: &'static str,
    message: String,
    hint: Option<String>,
}

impl From<files::FileError> for FileFailure {
    fn from(err: files::FileError) -> Self {
        Self {
            code: err.code,
            message: err.message,
            hint: err.hint,
        }
    }
}

fn files_device(params: &Value) -> Result<(String, std::path::PathBuf), FileFailure> {
    let device_id = params
        .get("deviceId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| FileFailure {
            code: "E_NO_DEVICE",
            message: "缺少 deviceId".into(),
            hint: Some("回首页重新选择已连接手机".into()),
        })?;
    let (_, adb) = engine_paths();
    ensure_device(&adb, &device_id)?;
    Ok((device_id, adb))
}

fn files_list(id: Value, params: Value) {
    let path = params
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or("/sdcard");
    let (device_id, adb) = match files_device(&params) {
        Ok(value) => value,
        Err(err) => {
            reply_error(id, err.code, err.message, err.hint);
            return;
        }
    };
    match files::list(&adb, &device_id, path) {
        Ok(listing) => reply(
            id,
            serde_json::to_value(listing).unwrap_or_else(|_| json!({"entries": []})),
        ),
        Err(err) => reply_error(id, err.code, err.message, err.hint),
    }
}

fn files_pull(id: Value, params: Value) {
    let paths: Vec<String> = params
        .get("paths")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if paths.is_empty() {
        reply_error(
            id,
            "E_NO_SELECTION",
            "先选择一个文件或文件夹",
            Some("点文件行右侧的勾选框".into()),
        );
        return;
    }
    let local_dir = params
        .get("localDir")
        .and_then(Value::as_str)
        .map(str::to_string);
    let (device_id, adb) = match files_device(&params) {
        Ok(value) => value,
        Err(err) => {
            reply_error(id, err.code, err.message, err.hint);
            return;
        }
    };
    match files::start_pull(adb, device_id, paths, local_dir) {
        Ok(transfer_id) => reply(id, json!({"transferId": transfer_id})),
        Err(err) => reply_error(id, err.code, err.message, err.hint),
    }
}

fn files_push(id: Value, params: Value) {
    let local_paths: Vec<String> = params
        .get("localPaths")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    if local_paths.is_empty() {
        reply_error(
            id,
            "E_NO_SELECTION",
            "先选择要发送的文件或文件夹",
            Some("点「发送到手机」，或把文件拖进窗口".into()),
        );
        return;
    }
    let remote_dir = params
        .get("remoteDir")
        .and_then(Value::as_str)
        .unwrap_or("/sdcard/Download/CastPane")
        .to_string();
    let (device_id, adb) = match files_device(&params) {
        Ok(value) => value,
        Err(err) => {
            reply_error(id, err.code, err.message, err.hint);
            return;
        }
    };
    match files::start_push(adb, device_id, local_paths, remote_dir) {
        Ok(transfer_id) => reply(id, json!({"transferId": transfer_id})),
        Err(err) => reply_error(id, err.code, err.message, err.hint),
    }
}

fn files_transfer(id: Value, params: Value) {
    let requested = params.get("transferId").and_then(Value::as_str);
    let view = match requested {
        Some(transfer_id) => files::status(transfer_id),
        None => files::current(),
    };
    reply(id, serde_json::to_value(view).unwrap_or(Value::Null));
}

fn files_open(id: Value, params: Value) {
    let Some(path) = params.get("path").and_then(Value::as_str) else {
        reply_error(id, "E_PATH", "缺少 path", None);
        return;
    };
    match files::open_folder(path) {
        Ok(()) => reply(id, json!({"ok": true})),
        Err(message) => reply_error(id, "E_OPEN", message, Some("打开资源管理器手动查看".into())),
    }
}

fn files_home(id: Value) {
    reply(
        id,
        json!({
            "downloadDir": files::default_download_dir().display().to_string(),
            "phoneRoot": "/sdcard",
            "phoneUploadDir": "/sdcard/Download/CastPane",
        }),
    );
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
        let mut devices = adb::list_devices(&adb).unwrap_or_default();
        let wireless_serial = adb::is_wireless_serial(device_id);
        if wireless_serial
            && !devices
                .iter()
                .any(|d| d.serial == device_id && d.state == "device")
        {
            // The phone may have woken up or changed its port since the last
            // scan. Reconnect once instead of failing with "not in list".
            let result = wireless().connect(&adb, device_id);
            if result.ok {
                devices = adb::list_devices(&adb).unwrap_or(devices);
            } else {
                reply_error(
                    id,
                    result.code.as_deref().unwrap_or("E_NO_DEVICE"),
                    result.status,
                    result.hint,
                );
                return;
            }
        }
        match devices.iter().find(|d| d.serial == device_id) {
            None => {
                let hint = if wireless_serial {
                    "确认手机与电脑在同一 Wi-Fi，且手机仍在线；必要时插线点「一键开启无线」"
                } else {
                    "插好数据线，或点「更多 → 重新检测」"
                };
                reply_error(
                    id,
                    "E_NO_DEVICE",
                    format!("设备 {device_id} 不在列表中"),
                    Some(hint.into()),
                );
                return;
            }
            Some(d) if d.state != "device" => {
                let hint = if d.state == "unauthorized" {
                    if wireless_serial {
                        "在手机上点「允许 USB 调试」，再插线点一次「一键开启无线」"
                    } else {
                        "在手机上点「允许 USB 调试」"
                    }
                } else {
                    "解锁手机并保持屏幕点亮，确认 Wi-Fi 未断开"
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
    let (scrcpy_path, adb_path) = engine_paths();
    let settings = cfg.settings.clone();
    let mut window = cfg.window.clone();

    if true {
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

    spawn_waiter(
        sid.clone(),
        child,
        cfg.device_id.clone(),
        adb_path.clone(),
        adb::is_wireless_serial(&cfg.device_id),
    );

    set_brightness(&adb_path, &cfg.device_id, settings.brightness);

    #[cfg(windows)]
    {
        overlay::attach(pid, &cfg.device_id);
        std::thread::spawn(move || window::place_right(pid));
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


/// Set the phone's screen brightness (0 leaves it alone). `adb shell` holds
/// WRITE_SETTINGS, so this works without touching the phone's UI.
fn set_brightness(adb: &std::path::Path, serial: &str, percent: u32) {
    if percent == 0 {
        return;
    }
    let raw = (percent.min(100) * 255 / 100).to_string();
    for (key, value) in [("screen_brightness_mode", "0"), ("screen_brightness", raw.as_str())] {
        let mut command = std::process::Command::new(adb);
        command.args(["-s", serial, "shell", "settings", "put", "system", key, value]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let _ = command.output();
    }
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

    // Brightness is applied directly on the phone: no restart needed.
    let mut probe = new_settings.clone();
    probe.brightness = cfg.settings.brightness;
    if probe == cfg.settings {
        cfg.settings.brightness = new_settings.brightness;
        let (_, adb_path) = engine_paths();
        set_brightness(&adb_path, &cfg.device_id, new_settings.brightness);
        *config().lock().unwrap() = Some(cfg);
        reply(id, json!({"ok": true, "applied": "live"}));
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
    let current = cfg.settings.orientation.parse::<u32>().unwrap_or(0);
    cfg.settings.orientation = ((current + 90) % 360).to_string();
    restart_with(cfg);
}

/// Watch the engine process and report its death exactly once.
///
/// The engine may reject an argument and exit within a second, so "streaming"
/// is only announced after a short grace period with the process still alive.
fn spawn_waiter(
    session_id: String,
    child: Arc<Mutex<Child>>,
    device_id: String,
    adb_path: PathBuf,
    wireless: bool,
) {
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
                // A stop/restart initiated by us takes the slot before killing
                // the process; only an unexpected exit can be a device drop.
                let intentional = session()
                    .lock()
                    .map(|slot| slot.is_none())
                    .unwrap_or(true);
                let device_present = if wireless && !intentional {
                    adb::list_devices(&adb_path)
                        .map(|devices| {
                            devices
                                .iter()
                                .any(|d| d.serial == device_id && d.state == "device")
                        })
                        .unwrap_or(true)
                } else {
                    true
                };

                let (error, reason, hint) = if intentional {
                    (Value::Null, "投屏已停止", Value::Null)
                } else if !device_present {
                    (
                        json!({"code": "E_DEVICE_LOST", "message": "手机已离线"}),
                        "设备已掉线 · 请检查 Wi-Fi 后重连",
                        json!("确认手机与电脑仍在同一 Wi-Fi；手机重启后需插线重新开启"),
                    )
                } else if status.success() {
                    (Value::Null, "投屏已结束", Value::Null)
                } else {
                    (
                        json!({"code": "E_ENGINE_EXIT", "message": status.to_string()}),
                        "画面进程已退出 · 可能是手机锁屏或编码失败",
                        json!("解锁手机后重试；若反复失败，打开「更多 → 诊断」"),
                    )
                };

                emit(
                    "session.state",
                    json!({
                        "sessionId": session_id,
                        "state": "stopped",
                        "error": error,
                        "reason": reason,
                        "hint": hint,
                    }),
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
            "hint": if devices.is_empty() {
                json!("插好数据线，并在开发者选项里打开 USB 调试")
            } else { Value::Null }
        }));

        let snapshot = wireless().scan(&adb);
        checks.push(json!({
            "name": "wireless",
            "ok": matches!(snapshot.state.as_str(), "ready" | "usb" | "offline"),
            "detail": snapshot.status,
            "hint": snapshot.hint,
        }));
        checks.push(json!({
            "name": "network",
            "ok": snapshot.same_network != Some(false),
            "detail": format!(
                "电脑 {} · 手机 {} · {}",
                snapshot.local_ip.clone().unwrap_or_else(|| "未知".into()),
                snapshot.phone_ip.clone().unwrap_or_else(|| "未知".into()),
                match snapshot.same_network {
                    Some(true) => "同一网段",
                    Some(false) => "不同网段",
                    None => "未检测到手机网络",
                }
            ),
            "hint": if snapshot.same_network == Some(false) {
                json!("让手机与电脑连接同一个 Wi-Fi")
            } else { Value::Null }
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
    files::init_emitter(emit);
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
            "action.wireless_scan" => wireless_scan(id, params),
            "action.wireless_connect" => wireless_connect(id, params),
            "action.wireless_usb_bootstrap" => wireless_usb_bootstrap(id),
            "action.wireless_disconnect" => wireless_disconnect(id, params),
            "action.files_list" => files_list(id, params),
            "action.files_pull" => files_pull(id, params),
            "action.files_push" => files_push(id, params),
            "action.files_transfer" => files_transfer(id, params),
            "action.files_open" => files_open(id, params),
            "action.files_home" => files_home(id),
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
