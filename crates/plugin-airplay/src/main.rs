#![cfg_attr(windows, windows_subsystem = "windows")]
//! AirPlay plugin: speaks `docs/plugin-api.md`, owns the uxplay receiver, and
//! reports the states the UI needs: engine missing, waiting for a device,
//! streaming, stopped.
//!
//! AirPlay has no "scan for senders": this plugin *is* the receiver. So
//! `discover.start` reports one pseudo-device (this machine) and
//! `session.start` begins listening; the user then picks the name in the iPad
//! control centre.
//!
//! The receiver's video window is a separate top-level window owned by
//! uxplay-windows. It appears only when a client connects; its presence is the
//! observable signal for `streaming`. When it disappears we go back to
//! `waiting` instead of pretending the session is still on screen.

mod engine;
mod window;

use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::process::Child;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const NAME: &str = "AirPlay";
const RECEIVER_DEVICE_ID: &str = "receiver";
const DEFAULT_RECEIVER_NAME: &str = "CastPane";

static OUT: OnceLock<Mutex<std::io::Stdout>> = OnceLock::new();
static SESSION: OnceLock<Mutex<Option<Running>>> = OnceLock::new();
static SETTINGS: OnceLock<Mutex<Value>> = OnceLock::new();

struct Running {
    id: String,
    pid: u32,
    child: Arc<Mutex<Child>>,
}

fn stdout() -> &'static Mutex<std::io::Stdout> {
    OUT.get_or_init(|| Mutex::new(std::io::stdout()))
}

fn session() -> &'static Mutex<Option<Running>> {
    SESSION.get_or_init(|| Mutex::new(None))
}

fn settings() -> &'static Mutex<Value> {
    SETTINGS.get_or_init(|| Mutex::new(json!({})))
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

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn receiver_name(settings: &Value) -> String {
    settings
        .get("receiverName")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(DEFAULT_RECEIVER_NAME)
        .to_string()
}

fn low_latency(settings: &Value) -> bool {
    settings
        .get("lowLatency")
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

fn renderer(settings: &Value) -> String {
    settings
        .get("renderer")
        .and_then(Value::as_str)
        .unwrap_or("auto")
        .to_string()
}

fn fullscreen(settings: &Value) -> bool {
    settings
        .get("fullscreen")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn discover_detail() -> &'static str {
    match engine::check_ready() {
        Ok(_) => "ready",
        Err(err) => match err.code {
            "E_BONJOUR_MISSING" | "E_BONJOUR_STOPPED" => "bonjour-missing",
            "E_SCANNER_MISSING" | "E_PLUGIN_MISSING" => "engine-incomplete",
            _ => "engine-missing",
        },
    }
}

fn discover() {
    emit(
        "device.found",
        json!({
            "id": RECEIVER_DEVICE_ID,
            "name": "iPad / iPhone 投屏",
            "kind": "ios",
            "detail": discover_detail(),
            "transport": "airplay",
            "label": "控制中心 → 屏幕镜像",
        }),
    );
}

struct Started {
    id: String,
    pid: u32,
    receiver: String,
}

/// Validate the engine and spawn it. Returns the session id and pid.
fn begin(values: &Value) -> Result<Started, engine::EngineError> {
    let engine_path = engine::check_ready()?;

    // The override is a test hook: the mock engine owns its lifecycle, so a
    // real uxplay running on the developer machine must not fail the test.
    let using_override = std::env::var_os(engine::ENV_OVERRIDE).is_some();
    if !using_override && engine::running_receivers() > 0 {
        return Err(engine::EngineError {
            code: "E_ALREADY_RUNNING",
            message: "已有另一个 AirPlay 接收端在运行".to_string(),
            hint: Some("先在它的托盘图标里退出，再回到 CastPane 点「启动接收端」".to_string()),
        });
    }

    let receiver = receiver_name(values);
    let args_path = engine::write_arguments(&receiver, low_latency(values)).map_err(|err| {
        engine::EngineError {
            code: "E_ARGUMENTS",
            message: format!("无法写入接收端配置：{err}"),
            hint: Some(format!("检查 {} 是否可写", engine::arguments_path().display())),
        }
    })?;
    engine::apply_gui_settings(&renderer(values), fullscreen(values));

    let child = engine::spawn(&engine_path).map_err(|err| engine::EngineError {
        code: "E_ENGINE_FAILED",
        message: format!("启动 uxplay 失败：{err}"),
        hint: Some(format!("检查 {}", engine_path.display())),
    })?;

    let pid = child.id();
    let sid = format!("airplay-{}", now_millis());
    *session().lock().unwrap() = Some(Running {
        id: sid.clone(),
        pid,
        child: Arc::new(Mutex::new(child)),
    });
    *settings().lock().unwrap() = values.clone();

    emit(
        "log",
        json!({
            "level": "info",
            "message": format!(
                "接收端 {} 已启动（pid {}）；配置：{}",
                receiver,
                pid,
                args_path.display()
            )
        }),
    );

    Ok(Started {
        id: sid,
        pid,
        receiver,
    })
}

fn start_session(id: Value, params: Value) {
    let settings = params
        .get("settings")
        .cloned()
        .unwrap_or_else(|| json!({}));

    match begin(&settings) {
        Ok(started) => {
            reply(id, json!({"sessionId": started.id}));
            emit(
                "session.state",
                json!({"sessionId": started.id, "state": "connecting"}),
            );
            monitor(started.id, started.pid, started.receiver);
        }
        Err(err) => reply_error(id, err.code, err.message, err.hint),
    }
}

/// Watch the receiver: process death, the video window appearing/disappearing,
/// and the transition from "started" to "waiting for an iPad".
fn monitor(session_id: String, pid: u32, receiver: String) {
    std::thread::spawn(move || {
        let started = Instant::now();
        let mut streaming = false;
        let mut waiting_announced = false;
        let mut last_window = None;

        loop {
            std::thread::sleep(Duration::from_millis(400));

            if session()
                .lock()
                .map(|slot| slot.as_ref().map(|r| r.id != session_id).unwrap_or(true))
                .unwrap_or(true)
            {
                return; // stopped or replaced
            }

            let child = {
                let slot = session().lock().unwrap();
                slot.as_ref()
                    .filter(|r| r.id == session_id)
                    .map(|r| Arc::clone(&r.child))
            };
            let Some(child) = child else { return };

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
                    json!({
                        "sessionId": session_id,
                        "state": "stopped",
                        "error": error,
                        "reason": "AirPlay 接收端已退出",
                        "hint": "点「启动接收端」重试；反复退出请打开诊断看引擎检查项",
                    }),
                );
                let mut slot = session().lock().unwrap();
                if slot.as_ref().map(|r| r.id == session_id).unwrap_or(false) {
                    *slot = None;
                }
                return;
            }

            let hwnd = window::find_video_window(pid);

            match hwnd {
                Some(hwnd) => {
                    let changed = Some(hwnd) != last_window;
                    last_window = Some(hwnd);
                    if !streaming || changed {
                        if let Some(spec) = window::receiver_rect() {
                            window::adopt(hwnd, &spec);
                        }
                        if !streaming {
                            emit(
                                "session.state",
                                json!({
                                    "sessionId": session_id,
                                    "state": "streaming",
                                    "reason": "iPad 已连接",
                                }),
                            );
                            emit(
                                "log",
                                json!({"level": "info", "message": "iPad 已连接，画面窗口已就绪"}),
                            );
                            streaming = true;
                            waiting_announced = false;
                        }
                    }
                }
                None => {
                    last_window = None;
                    if streaming {
                        streaming = false;
                        waiting_announced = true;
                        emit(
                            "session.state",
                            json!({
                                "sessionId": session_id,
                                "state": "waiting",
                                "reason": "iPad 已断开，接收端继续等待",
                                "hint": "在 iPad 控制中心重新选择接收端即可",
                            }),
                        );
                        emit(
                            "log",
                            json!({"level": "info", "message": "iPad 已断开，接收端仍在等待"}),
                        );
                    } else if !waiting_announced && started.elapsed() >= Duration::from_millis(2200)
                    {
                        waiting_announced = true;
                        let ports = engine::listening_ports(pid);
                        let hint = if ports.is_empty() {
                            "还没检测到监听端口：检查端口占用或 Windows 防火墙是否放行"
                        } else {
                            "iPad 控制中心 → 屏幕镜像 → 选择接收端"
                        };
                        emit(
                            "session.state",
                            json!({
                                "sessionId": session_id,
                                "state": "waiting",
                                "reason": format!("接收端 {receiver} 已启动，等待 iPad"),
                                "hint": hint,
                                "ports": ports,
                            }),
                        );
                        emit(
                            "log",
                            json!({
                                "level": "info",
                                "message": format!(
                                    "接收端 {receiver} 已启动，等待 iPad 在控制中心选择它"
                                )
                            }),
                        );
                    }
                }
            }
        }
    });
}

fn stop_session(id: Value) {
    let running = session().lock().unwrap().take();
    if let Some(running) = running {
        stop_running(&running);
        emit(
            "session.state",
            json!({"sessionId": running.id, "state": "stopped", "reason": "已停止接收端"}),
        );
    }
    reply(id, json!({"ok": true}));
}

fn stop_running(running: &Running) {
    engine::kill_tree(running.pid);
    if let Ok(mut child) = running.child.lock() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn apply_settings(id: Value, params: Value) {
    let new_settings = params
        .get("settings")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let Some(running) = session().lock().unwrap().take() else {
        reply(id, json!({"ok": true, "applied": "no-session"}));
        return;
    };

    if *settings().lock().unwrap() == new_settings {
        *session().lock().unwrap() = Some(running);
        reply(id, json!({"ok": true, "applied": "noop"}));
        return;
    }

    stop_running(&running);
    match begin(&new_settings) {
        Ok(started) => {
            reply(id, json!({"ok": true, "applied": "restart"}));
            emit(
                "session.state",
                json!({"sessionId": started.id, "state": "connecting"}),
            );
            monitor(started.id, started.pid, started.receiver);
        }
        Err(err) => reply_error(id, err.code, err.message, err.hint),
    }
}

fn install_bonjour(id: Value) {
    let Some(engine_path) = engine::find_engine() else {
        reply_error(
            id,
            "E_ENGINE_MISSING",
            "找不到 AirPlay 接收引擎，先补齐引擎再安装 Bonjour",
            Some("运行 scripts/fetch-engines.ps1 -Only airplay".into()),
        );
        return;
    };
    match engine::install_bonjour(&engine_path) {
        Ok(detail) => {
            emit(
                "log",
                json!({"level": "info", "message": format!("Bonjour：{detail}")}),
            );
            reply(id, json!({"ok": true, "detail": detail}));
        }
        Err(message) => reply_error(id, "E_BONJOUR_INSTALL", message, Some("也可以手动安装 Apple Bonjour，或直接点「重新检查」".into())),
    }
}

fn check(
    name: &str,
    ok: bool,
    detail: impl Into<String>,
    hint: Option<String>,
) -> Value {
    json!({
        "name": name,
        "ok": ok,
        "detail": detail.into(),
        "hint": hint,
    })
}

fn diagnose(id: Value) {
    let mut checks = Vec::new();

    let engine_found = engine::find_engine();
    checks.push(check(
        "引擎",
        engine_found.is_some(),
        engine_found
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| engine::engine_dir().display().to_string()),
        if engine_found.is_some() {
            None
        } else {
            Some("运行 scripts/fetch-engines.ps1 -Only airplay".to_string())
        },
    ));

    let scanner = engine::scanner_path();
    checks.push(check(
        "视频组件",
        scanner.exists(),
        scanner.display().to_string(),
        if scanner.exists() {
            None
        } else {
            Some("重新运行 scripts/fetch-engines.ps1 -Only airplay 补齐 GStreamer scanner".to_string())
        },
    ));

    let missing: Vec<String> = engine::video_plugins()
        .into_iter()
        .filter(|(path, _)| !path.exists())
        .map(|(path, label)| format!("{label}（{}）", path.display()))
        .collect();
    checks.push(check(
        "解码与渲染",
        missing.is_empty(),
        if missing.is_empty() {
            "H.264 / D3D11 / playback 组件齐全".to_string()
        } else {
            format!("缺少：{}", missing.join("、"))
        },
        if missing.is_empty() {
            None
        } else {
            Some("重新安装 AirPlay 引擎".to_string())
        },
    ));

    let bonjour = engine::bonjour_status();
    checks.push(check(
        "Bonjour 发现",
        bonjour == engine::Bonjour::Running,
        match bonjour {
            engine::Bonjour::Running => "服务运行中，iPad 可以发现在线接收端".to_string(),
            engine::Bonjour::Stopped => "服务已安装但未运行".to_string(),
            engine::Bonjour::Missing => "未安装 Bonjour 服务".to_string(),
        },
        match bonjour {
            engine::Bonjour::Running => None,
            engine::Bonjour::Stopped => Some("启动 Bonjour Service，或点「安装接收组件」".to_string()),
            engine::Bonjour::Missing => Some("点页面上的「安装接收组件」（需要一次管理员确认）".to_string()),
        },
    ));

    let ip = engine::local_ipv4().unwrap_or_else(|| "未知".into());
    let ssid = engine::wifi_ssid();
    checks.push(check(
        "网络",
        engine::local_ipv4().is_some(),
        match &ssid {
            Some(ssid) => format!("电脑 {ip} · Wi-Fi {ssid}"),
            None => format!("电脑 {ip} · 有线或未连接 Wi-Fi"),
        },
        if engine::local_ipv4().is_some() {
            Some("让 iPad 连接同一个 Wi-Fi；不同网段/访客网络/AP 隔离都会看不到接收端".to_string())
        } else {
            Some("电脑没有可用网络，先连接 Wi-Fi 或网线".to_string())
        },
    ));

    let running = {
        let slot = session().lock().unwrap();
        slot.as_ref().map(|r| (r.pid, r.id.clone()))
    };
    match running {
        Some((pid, _)) => {
            let ports = engine::listening_ports(pid);
            checks.push(check(
                "接收端",
                !ports.is_empty(),
                format!(
                    "pid {pid} · {}",
                    if ports.is_empty() {
                        "没有监听端口".to_string()
                    } else {
                        format!("监听端口 {:?}", ports)
                    }
                ),
                if ports.is_empty() {
                    Some("端口被占用或防火墙阻止；关掉占用 7000/7100 的程序后重试".to_string())
                } else {
                    None
                },
            ));
        }
        None => checks.push(check(
            "接收端",
            true,
            "当前未启动；点「启动接收端」后在这里看监听端口",
            None,
        )),
    }

    checks.push(check(
        "Windows 防火墙",
        true,
        "若 iPad 看不到接收端，请在 Windows 防火墙弹窗中允许「专用网络」",
        Some("公共网络/访客网络通常会被隔离，改用家庭 Wi-Fi".to_string()),
    ));

    reply(id, json!({"checks": checks}));
}

fn kill_session() {
    if let Some(running) = session().lock().unwrap().take() {
        stop_running(&running);
    }
}

fn main() {
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
            continue;
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
                    "capabilities": ["discover", "session", "window", "diagnose"]
                }),
            ),
            "discover.start" | "action.rescan" => {
                discover();
                reply(id, json!({"ok": true}));
            }
            "discover.stop" => reply(id, json!({"ok": true})),
            "session.start" => start_session(id, params),
            "session.stop" => stop_session(id),
            "settings.apply" => apply_settings(id, params),
            "diagnose" | "action.diagnose" => diagnose(id),
            "action.install_bonjour" => install_bonjour(id),
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
