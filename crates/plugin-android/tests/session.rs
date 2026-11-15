//! End-to-end session test for the android plugin, using mock adb/scrcpy so it
//! runs without a phone or the real engine.
//!
//! Covers the path that matters most and was previously untested: discover ->
//! session.start -> window flags handed to scrcpy -> streaming -> stop.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

struct Client {
    child: Child,
    stdin: ChildStdin,
    rx: mpsc::Receiver<String>,
    pending: Vec<Value>,
}

impl Client {
    fn spawn(envs: &[(&str, String)]) -> Self {
        let exe = std::env::var("CARGO_BIN_EXE_android-plugin").expect("plugin binary path");
        let mut command = Command::new(exe);
        for (key, value) in envs {
            command.env(key, value);
        }
        let mut child = command
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

        Self {
            child,
            stdin,
            rx,
            pending: Vec::new(),
        }
    }

    fn request(&mut self, id: u64, method: &str, params: Value) -> Value {
        let payload = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
        writeln!(self.stdin, "{payload}").expect("write");
        self.stdin.flush().expect("flush");

        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = self.rx.recv_timeout(left).expect("response timeout");
            let value: Value = serde_json::from_str(&line).expect("json");
            if value.get("id").and_then(Value::as_u64) == Some(id) {
                return value;
            }
            self.pending.push(value);
        }
    }

    fn drain(&mut self, window: Duration) -> Vec<Value> {
        let deadline = Instant::now() + window;
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            match self.rx.recv_timeout(left) {
                Ok(line) => {
                    if let Ok(value) = serde_json::from_str::<Value>(&line) {
                        self.pending.push(value);
                    }
                }
                Err(_) => break,
            }
        }
        std::mem::take(&mut self.pending)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn bin(name: &str) -> String {
    let var = format!("CARGO_BIN_EXE_{name}");
    std::env::var(var).unwrap_or_else(|_| panic!("{name} binary not built"))
}

#[test]
fn casts_to_a_mock_device_with_portrait_geometry() {
    let args_file = std::env::temp_dir().join(format!(
        "castpane-scrcpy-args-{}.txt",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&args_file);

    let mut client = Client::spawn(&[
        ("CASTPANE_ADB", bin("mock-adb")),
        ("CASTPANE_SCRCPY", bin("mock-scrcpy")),
        ("MOCK_SCRCPY_ARGS_FILE", args_file.display().to_string()),
        ("MOCK_ENGINE_SECONDS", "30".to_string()),
    ]);

    client.request(1, "initialize", json!({}));

    client.request(2, "discover.start", json!({}));
    let events = client.drain(Duration::from_secs(2));
    let device = events
        .iter()
        .find(|event| event["method"] == "device.found")
        .expect("device.found");
    assert_eq!(device["params"]["id"], "MOCK123456");
    assert_eq!(device["params"]["detail"], "device");

    let started = client.request(
        3,
        "session.start",
        json!({
            "deviceId": "MOCK123456",
            "window": {
                "x": 700, "y": 52, "width": 527, "height": 936,
                "borderless": true, "alwaysOnTop": true
            }
        }),
    );
    assert!(
        started["result"]["sessionId"].is_string(),
        "unexpected response: {started}"
    );

    let events = client.drain(Duration::from_secs(3));
    let states: Vec<&str> = events
        .iter()
        .filter(|event| event["method"] == "session.state")
        .filter_map(|event| event["params"]["state"].as_str())
        .collect();
    assert!(
        states.contains(&"streaming"),
        "expected streaming, got {states:?}"
    );

    // scrcpy must receive the portrait geometry computed by the host.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !args_file.exists() {
        std::thread::sleep(Duration::from_millis(50));
    }
    let args = std::fs::read_to_string(&args_file).expect("scrcpy arguments were not recorded");
    assert!(args.contains("-s MOCK123456"), "missing serial: {args}");
    assert!(
        !args.contains("--window-borderless"),
        "cast window must keep its native title bar: {args}"
    );
    assert!(
        !args.contains("--always-on-top"),
        "cast must not cover the console by default: {args}"
    );

    let stopped = client.request(4, "session.stop", json!({}));
    assert_eq!(stopped["result"]["ok"], true);

    let _ = std::fs::remove_file(&args_file);
}

#[test]
fn refuses_an_unauthorized_device_with_a_hint() {
    let mut client = Client::spawn(&[
        ("CASTPANE_ADB", bin("mock-adb")),
        ("CASTPANE_SCRCPY", bin("mock-scrcpy")),
        ("MOCK_ADB_STATE", "unauthorized".to_string()),
    ]);

    client.request(1, "initialize", json!({}));
    let response = client.request(2, "session.start", json!({"deviceId": "MOCK123456"}));
    assert_eq!(response["error"]["code"], "E_PERMISSION");
    assert!(response["error"]["hint"].is_string());
}
