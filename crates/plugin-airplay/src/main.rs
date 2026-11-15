//! AirPlay plugin: speaks `docs/plugin-api.md` and launches uxplay.
//!
//! AirPlay has no "scan for senders": this plugin *is* the receiver. So
//! `discover.start` reports one pseudo-device (this machine) and
//! `session.start` begins listening; the user then picks the name in the
//! iPhone control centre.
//!
//! Window positioning is not implemented yet: uxplay's Qt window is placed by
//! the app itself. See docs/plugin-api.md §9 for the window takeover design.

mod engine;

use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::process::Child;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const NAME: &str = "AirPlay";
const RECEIVER_DEVICE_ID: &str = "receiver";

static OUT: OnceLock<Mutex<std::io::Stdout>> = OnceLock::new();
static SESSION: OnceLock<Mutex<Option<Running>>> = OnceLock::new();

struct Running {
    id: String,
    child: Arc<Mutex<Child>>,
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

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn discover() {
    emit(
        "device.found",
        json!({
            "id": RECEIVER_DEVICE_ID,
            "name": "AirPlay 接收端",
            "kind": "ios",
            "detail": if engine::find_engine().is_some() { "ready" } else { "engine-missing" },
        }),
    );
}

fn start_session(id: Value, params: Value) {
    let Some(engine_path) = engine::find_engine() else {
        reply_error(
            id,
            "E_ENGINE_MISSING",
            format!("找不到 uxplay：{}", engine::engine_dir().display()),
            Some("运行 scripts/fetch-engines.ps1 -Only airplay，或手动解压 uxplay-windows 到 plugins/airplay/bin/uxplay/".into()),
        );
        return;
    };

    match engine::spawn(&engine_path) {
        Ok(child) => {
            let sid = format!("airplay-{}", now_millis());
            let child = Arc::new(Mutex::new(child));

            *session().lock().unwrap() = Some(Running {
                id: sid.clone(),
                child: Arc::clone(&child),
            });

            reply(id, json!({"sessionId": sid}));
            emit(
                "session.state",
                json!({"sessionId": sid, "state": "connecting"}),
            );
            emit(
                "session.state",
                json!({"sessionId": sid, "state": "streaming"}),
            );

            let receiver = params
                .get("settings")
                .and_then(|s| s.get("receiverName"))
                .and_then(Value::as_str)
                .unwrap_or("CastPane");
            emit(
                "log",
                json!({
                    "level": "info",
                    "message": format!("接收端已启动（{receiver}）：在 iPhone 控制中心点「屏幕镜像」选择它")
                }),
            );

            spawn_waiter(sid, child);
        }
        Err(err) => reply_error(
            id,
            "E_ENGINE_FAILED",
            format!("启动 uxplay 失败：{err}"),
            Some(format!("检查 {}", engine_path.display())),
        ),
    }
}

fn spawn_waiter(session_id: String, child: Arc<Mutex<Child>>) {
    std::thread::spawn(move || loop {
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

        if session().lock().map(|s| s.is_none()).unwrap_or(true) {
            return;
        }
    });
}

fn stop_session(id: Value) {
    let running = session().lock().unwrap().take();
    if let Some(running) = running {
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
    let found = engine::find_engine();
    let checks = vec![json!({
        "name": "uxplay",
        "ok": found.is_some(),
        "detail": found
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| engine::engine_dir().display().to_string()),
        "hint": if found.is_some() { Value::Null } else { json!("运行 scripts/fetch-engines.ps1 -Only airplay") }
    })];
    reply(id, json!({"checks": checks}));
}

fn kill_session() {
    if let Some(running) = session().lock().unwrap().take() {
        if let Ok(mut child) = running.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
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
                    "capabilities": ["discover", "session", "diagnose"]
                }),
            ),
            "discover.start" | "action.rescan" => {
                discover();
                reply(id, json!({"ok": true}));
            }
            "discover.stop" | "settings.apply" => reply(id, json!({"ok": true})),
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
