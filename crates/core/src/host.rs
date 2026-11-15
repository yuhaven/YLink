//! Spawn, talk to, and reap one plugin process.
//!
//! Deliberately blocking and thread-based: plugin calls are rare (once per
//! device or session), and a blocking API keeps the host easy to reason about.

use crate::ipc::{self, Request, Response};
use crate::manifest::{ManifestError, PluginManifest};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long a single request may take before the plugin is considered hung.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Everything that arrives asynchronously from a plugin.
#[derive(Debug, Clone)]
pub enum PluginEvent {
    /// `device.found`, `session.state`, ...
    Notification { method: String, params: Value },
    /// The process died. `code` is the exit status if known.
    Exited { code: Option<i32> },
    /// A line that was not valid JSON-RPC. Kept for the diagnostics panel.
    Malformed(String),
}

struct Shared {
    stdin: Mutex<ChildStdin>,
    pending: Mutex<HashMap<u64, Sender<Response>>>,
    next_id: AtomicU64,
    events: Sender<PluginEvent>,
}

/// Handle to one plugin process.
pub struct PluginHost {
    manifest: PluginManifest,
    shared: Option<Arc<Shared>>,
    child: Option<Child>,
    events: Receiver<PluginEvent>,
}

impl PluginHost {
    pub fn load(dir: impl Into<PathBuf>) -> Result<Self, ManifestError> {
        let manifest = PluginManifest::load(dir)?;
        // Replaced with a live channel in `start`.
        let (_placeholder_tx, events) = channel();
        Ok(Self {
            manifest,
            shared: None,
            child: None,
            events,
        })
    }

    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    pub fn is_running(&self) -> bool {
        self.child.is_some()
    }

    /// Spawn the plugin and complete the `initialize` handshake.
    pub fn start(&mut self) -> Result<(), HostError> {
        if self.child.is_some() {
            return Ok(());
        }
        if !self.manifest.supports_current_platform() {
            return Err(HostError::UnsupportedPlatform(self.manifest.id.clone()));
        }
        let entry = self.manifest.entry_path();
        if !entry.exists() {
            return Err(HostError::MissingEntry(entry));
        }

        let mut child = Command::new(&entry)
            .args(&self.manifest.args)
            .current_dir(&self.manifest.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| HostError::Spawn(entry.clone(), e.to_string()))?;

        let stdin = child.stdin.take().expect("stdin piped");
        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");

        let (events_tx, events_rx) = channel();
        self.events = events_rx;

        let shared = Arc::new(Shared {
            stdin: Mutex::new(stdin),
            pending: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            events: events_tx.clone(),
        });

        // stdout -> JSON-RPC demux
        {
            let shared = Arc::clone(&shared);
            std::thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines() {
                    let Ok(line) = line else { break };
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    match ipc::classify(line) {
                        Some(ipc::Incoming::Response(resp)) => {
                            let waiter = shared.pending.lock().unwrap().remove(&resp.id);
                            if let Some(tx) = waiter {
                                let _ = tx.send(resp);
                            }
                        }
                        Some(ipc::Incoming::Notification(n)) => {
                            let _ = shared.events.send(PluginEvent::Notification {
                                method: n.method,
                                params: n.params,
                            });
                        }
                        None => {
                            let _ = shared.events.send(PluginEvent::Malformed(line.to_string()));
                        }
                    }
                }
            });
        }

        // stderr -> line log
        {
            let events = events_tx;
            std::thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines().map_while(Result::ok) {
                    let _ = events.send(PluginEvent::Notification {
                        method: ipc::event::LOG.to_string(),
                        params: Value::String(line),
                    });
                }
            });
        }

        self.shared = Some(shared);
        self.child = Some(child);

        // Handshake. A plugin that cannot initialize is not usable.
        let result = self.call(
            ipc::method::INITIALIZE,
            serde_json::json!({ "api": crate::PLUGIN_API_VERSION }),
        );
        if let Err(err) = result {
            self.stop();
            return Err(err);
        }
        Ok(())
    }

    /// Blocking request/response.
    pub fn call(&self, method: &str, params: Value) -> Result<Value, HostError> {
        let shared = self.shared.as_ref().ok_or(HostError::NotRunning)?;
        let id = shared.next_id.fetch_add(1, Ordering::SeqCst);

        let (tx, rx) = channel();
        shared.pending.lock().unwrap().insert(id, tx);

        let request = Request::new(id, method, params);
        let mut line = serde_json::to_string(&request)
            .map_err(|e| HostError::Protocol(format!("serialize request: {e}")))?;
        line.push('\n');

        {
            let mut stdin = shared.stdin.lock().unwrap();
            stdin
                .write_all(line.as_bytes())
                .and_then(|_| stdin.flush())
                .map_err(|e| HostError::Protocol(format!("write to plugin: {e}")))?;
        }

        match rx.recv_timeout(CALL_TIMEOUT) {
            Ok(resp) => match resp.error {
                Some(err) => Err(HostError::Rpc {
                    method: method.to_string(),
                    error: err,
                }),
                None => Ok(resp.result.unwrap_or(Value::Null)),
            },
            Err(RecvTimeoutError::Timeout) => {
                shared.pending.lock().unwrap().remove(&id);
                Err(HostError::Timeout(method.to_string()))
            }
            Err(RecvTimeoutError::Disconnected) => Err(HostError::NotRunning),
        }
    }

    /// Fire-and-forget notification to the plugin.
    pub fn notify(&self, method: &str, params: Value) -> Result<(), HostError> {
        let shared = self.shared.as_ref().ok_or(HostError::NotRunning)?;
        let request = Request::new(0, method, params);
        let mut line = serde_json::to_string(&request)
            .map_err(|e| HostError::Protocol(format!("serialize: {e}")))?;
        line.push('\n');
        let mut stdin = shared.stdin.lock().unwrap();
        stdin
            .write_all(line.as_bytes())
            .and_then(|_| stdin.flush())
            .map_err(|e| HostError::Protocol(format!("write: {e}")))
    }

    /// Non-blocking poll for pending events.
    pub fn try_next_event(&self) -> Option<PluginEvent> {
        self.events.try_recv().ok()
    }

    /// Drop all buffered events, e.g. after a reconnect.
    pub fn drain_events(&self) -> Vec<PluginEvent> {
        self.events.try_iter().collect()
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.shared = None;
        let _ = self.events.try_iter().count();
    }
}

impl Drop for PluginHost {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("plugin is not running")]
    NotRunning,
    #[error("plugin {0} does not support this platform")]
    UnsupportedPlatform(String),
    #[error("plugin executable not found: {0}")]
    MissingEntry(PathBuf),
    #[error("cannot spawn {0}: {1}")]
    Spawn(PathBuf, String),
    #[error("{method} failed: {error}")]
    Rpc {
        method: String,
        error: ipc::RpcError,
    },
    #[error("{0} timed out")]
    Timeout(String),
    #[error("protocol error: {0}")]
    Protocol(String),
}
