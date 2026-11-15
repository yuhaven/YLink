//! Contract smoke test: the plugin answers `initialize` and `diagnose`
//! exactly as `docs/plugin-api.md` requires, before any engine exists.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

fn spawn_plugin() -> (
    std::process::Child,
    std::process::ChildStdin,
    mpsc::Receiver<String>,
) {
    let exe = std::env::var("CARGO_BIN_EXE_android-plugin").expect("plugin binary path");
    let mut child = Command::new(exe)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn plugin");
    let stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    (child, stdin, rx)
}

fn send(stdin: &mut std::process::ChildStdin, payload: &str) {
    writeln!(stdin, "{payload}").expect("write request");
    stdin.flush().expect("flush");
}

fn next(rx: &mpsc::Receiver<String>) -> serde_json::Value {
    let line = rx.recv_timeout(Duration::from_secs(5)).expect("no response");
    serde_json::from_str(&line).expect("valid json")
}

#[test]
fn answers_initialize_and_diagnose() {
    let (mut child, mut stdin, rx) = spawn_plugin();

    send(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"api":0}}"#,
    );
    let init = next(&rx);
    assert_eq!(init["id"], 1);
    assert_eq!(init["result"]["name"], "Android");
    let caps = init["result"]["capabilities"]
        .as_array()
        .expect("capabilities array");
    assert!(caps.iter().any(|c| c == "discover"));
    assert!(caps.iter().any(|c| c == "session"));

    send(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":2,"method":"diagnose","params":{}}"#,
    );
    let diag = next(&rx);
    assert_eq!(diag["id"], 2);
    let checks = diag["result"]["checks"].as_array().expect("checks array");
    assert!(checks.len() >= 2, "expected engine checks");
    // Without a vendored engine this is false, but it must be a real check.
    assert_eq!(checks[0]["name"], "scrcpy");
    assert!(checks[0]["ok"].is_boolean());

    send(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":3,"method":"session.start","params":{}}"#,
    );
    let bad = next(&rx);
    // Missing engine or missing deviceId, depending on what is installed.
    let code = bad["error"]["code"].as_str().unwrap_or_default();
    assert!(code.starts_with("E_"), "expected an error code, got {bad}");

    let _ = child.kill();
    let _ = child.wait();
}
