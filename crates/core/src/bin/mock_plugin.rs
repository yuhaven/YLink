//! A fake plugin used by `tests/host_roundtrip.rs` and for UI development
//! before the real engines are vendored.
//!
//! It speaks exactly the protocol in `docs/plugin-api.md` and nothing else.

use std::io::{BufRead, Write};

fn reply(out: &mut impl Write, id: serde_json::Value, result: serde_json::Value) {
    let msg = serde_json::json!({"jsonrpc": "2.0", "id": id, "result": result});
    writeln!(out, "{msg}").unwrap();
    out.flush().unwrap();
}

fn emit(out: &mut impl Write, method: &str, params: serde_json::Value) {
    let msg = serde_json::json!({"jsonrpc": "2.0", "method": method, "params": params});
    writeln!(out, "{msg}").unwrap();
    out.flush().unwrap();
}

fn main() {
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(req) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");

        match method {
            "initialize" => reply(
                &mut out,
                id,
                serde_json::json!({
                    "name": "Mock",
                    "version": "0.0.1",
                    "capabilities": ["discover", "session"]
                }),
            ),
            "discover.start" => {
                emit(
                    &mut out,
                    "device.found",
                    serde_json::json!({"id": "mock-1", "name": "Mock Phone", "kind": "android"}),
                );
                reply(&mut out, id, serde_json::json!({"ok": true}));
            }
            "diagnose" => reply(
                &mut out,
                id,
                serde_json::json!({"checks": [
                    {"name": "mock", "ok": true, "detail": "always fine"}
                ]}),
            ),
            "shutdown" => {
                reply(&mut out, id, serde_json::json!({"ok": true}));
                std::process::exit(0);
            }
            other => {
                let err = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": {
                        "code": "E_UNSUPPORTED",
                        "message": format!("method `{other}` not implemented by mock"),
                        "hint": "这是测试插件"
                    }
                });
                writeln!(out, "{err}").unwrap();
                out.flush().unwrap();
            }
        }
    }
}
