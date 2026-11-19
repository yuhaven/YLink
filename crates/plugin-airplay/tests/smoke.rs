//! Contract and lifecycle tests for the AirPlay plugin.
//!
//! The lifecycle test uses `castpane-mock-engine` as a stand-in receiver, so
//! it needs `cargo test --workspace` (or a prior build of that crate).

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const ENV_ENGINE: &str = "CASTPANE_AIRPLAY_ENGINE";

struct Client {
    child: Child,
    stdin: ChildStdin,
    rx: mpsc::Receiver<String>,
    pending: Vec<Value>,
}

impl Client {
    fn spawn(envs: &[(&str, &str)]) -> Self {
        let exe = std::env::var("CARGO_BIN_EXE_airplay-plugin").expect("plugin binary path");
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

    /// Collect notifications emitted shortly after a request.
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

#[test]
fn answers_initialize_and_diagnose() {
    let mut client = Client::spawn(&[]);

    let init = client.request(1, "initialize", json!({"api": 0}));
    assert_eq!(init["result"]["name"], "AirPlay");
    let caps = init["result"]["capabilities"].as_array().expect("caps");
    assert!(caps.iter().any(|c| c == "session"));

    let diag = client.request(2, "action.diagnose", json!({}));
    let checks = diag["result"]["checks"].as_array().expect("checks");
    let engine = checks
        .iter()
        .find(|c| c["name"] == "引擎")
        .expect("engine check");
    assert!(engine["ok"].is_boolean());
    assert!(checks.iter().any(|c| c["name"] == "Bonjour 发现"));
}

#[test]
fn starts_and_stops_the_receiver_with_a_mock_engine() {
    let mock = mock_engine_path().expect("mock-engine not built; run cargo test --workspace");
    let mock_str = mock.to_str().expect("utf-8 path");

    let mut client = Client::spawn(&[(ENV_ENGINE, mock_str), ("MOCK_ENGINE_SECONDS", "30")]);
    client.request(1, "initialize", json!({"api": 0}));

    let discovered = client.request(2, "discover.start", json!({}));
    assert_eq!(discovered["result"]["ok"], true);

    let started = client.request(3, "session.start", json!({"deviceId": "receiver"}));
    let session_id = started["result"]["sessionId"]
        .as_str()
        .expect("sessionId")
        .to_string();

    let events = client.drain(Duration::from_secs(4));
    let states: Vec<String> = events
        .iter()
        .filter(|e| e["method"] == "session.state")
        .filter_map(|e| e["params"]["state"].as_str().map(str::to_string))
        .collect();
    assert!(
        states.iter().any(|s| s == "waiting"),
        "expected waiting state (no video window in the mock), got {states:?}"
    );
    assert!(
        !states.iter().any(|s| s == "streaming"),
        "mock engine creates no window, so streaming would be a lie: {states:?}"
    );

    let stopped = client.request(4, "session.stop", json!({"sessionId": session_id}));
    assert_eq!(stopped["result"]["ok"], true);
}

fn mock_engine_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_mock-engine") {
        return Some(PathBuf::from(path));
    }
    let exe = std::env::current_exe().ok()?;
    let debug_dir = exe.parent()?.parent()?;
    let candidate = debug_dir.join(if cfg!(windows) {
        "mock-engine.exe"
    } else {
        "mock-engine"
    });
    candidate.exists().then_some(candidate)
}
