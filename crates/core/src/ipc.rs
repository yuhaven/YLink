//! Line-delimited JSON-RPC 2.0 over stdio.
//!
//! One JSON object per line, UTF-8, no framing headers. Same shape as the
//! Language Server Protocol, chosen because every language can produce it in
//! about ten lines and it is trivially debuggable with `tee`.
//!
//! Deviation from stock JSON-RPC: `error.code` may be a string such as
//! `"E_FIREWALL"`, because plugin errors are meant to be shown to humans and
//! mapped to a `hint` in the UI.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const JSONRPC: &str = "2.0";

/// Methods the core calls on a plugin.
pub mod method {
    pub const INITIALIZE: &str = "initialize";
    pub const DISCOVER_START: &str = "discover.start";
    pub const DISCOVER_STOP: &str = "discover.stop";
    pub const SESSION_START: &str = "session.start";
    pub const SESSION_STOP: &str = "session.stop";
    pub const SETTINGS_APPLY: &str = "settings.apply";
    pub const DIAGNOSE: &str = "diagnose";
    pub const SHUTDOWN: &str = "shutdown";
}

/// Notifications a plugin pushes to the core.
pub mod event {
    pub const DEVICE_FOUND: &str = "device.found";
    pub const DEVICE_LOST: &str = "device.lost";
    pub const SESSION_STATE: &str = "session.state";
    pub const WINDOW_REPORT: &str = "window.report";
    pub const LOG: &str = "log";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl Request {
    pub fn new(id: u64, method: impl Into<String>, params: Value) -> Self {
        Self {
            jsonrpc: JSONRPC.to_string(),
            id,
            method: method.into(),
            params,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<RpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// A failure a human has to act on, not a stack trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    /// Integer JSON-RPC code, or a stable string like `"E_FIREWALL"`.
    pub code: Value,
    pub message: String,
    /// One actionable sentence shown under the error.
    #[serde(default)]
    pub hint: Option<String>,
    /// Optional machine-readable detail (ports, device ids, ...).
    #[serde(default)]
    pub data: Option<Value>,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.hint {
            Some(hint) => write!(f, "{} ({}) — {hint}", self.message, self.code),
            None => write!(f, "{} ({})", self.message, self.code),
        }
    }
}

/// Parse one line from a plugin, classifying it as response or notification.
pub fn classify(line: &str) -> Option<Incoming> {
    let value: Value = serde_json::from_str(line).ok()?;
    if value.get("method").is_some() {
        serde_json::from_value::<Notification>(value)
            .ok()
            .map(Incoming::Notification)
    } else {
        serde_json::from_value::<Response>(value)
            .ok()
            .map(Incoming::Response)
    }
}

#[derive(Debug, Clone)]
pub enum Incoming {
    Response(Response),
    Notification(Notification),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_response_and_notification() {
        let resp = r#"{"jsonrpc":"2.0","id":3,"result":{"ok":true}}"#;
        let note = r#"{"jsonrpc":"2.0","method":"device.found","params":{"id":"abc"}}"#;
        assert!(matches!(classify(resp), Some(Incoming::Response(_))));
        assert!(matches!(classify(note), Some(Incoming::Notification(_))));
    }

    #[test]
    fn string_error_code_survives_roundtrip() {
        let raw = r#"{"jsonrpc":"2.0","id":1,"error":{"code":"E_FIREWALL","message":"blocked","hint":"allow it"}}"#;
        match classify(raw) {
            Some(Incoming::Response(r)) => {
                let err = r.error.unwrap();
                assert_eq!(err.code, serde_json::json!("E_FIREWALL"));
                assert_eq!(err.hint.as_deref(), Some("allow it"));
            }
            _ => panic!("expected response"),
        }
    }
}
