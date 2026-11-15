//! CastPane core.
//!
//! The core deliberately knows nothing about video codecs or casting
//! protocols. It only does four things:
//!
//! 1. load plugin manifests from disk
//! 2. spawn / supervise plugin processes
//! 3. speak line-delimited JSON-RPC with them
//! 4. decide where the plugin's window goes
//!
//! Protocol work (AirPlay, ADB, ...) lives inside the plugin processes.

pub mod host;
pub mod ipc;
pub mod manifest;
pub mod window;

pub use host::{PluginEvent, PluginHost};
pub use manifest::{PanelItem, PluginManifest};
pub use window::{Rect, WindowSpec};

/// Version of the plugin contract implemented by this crate.
///
/// A plugin whose manifest declares a newer major version is rejected.
pub const PLUGIN_API_VERSION: u32 = 0;
