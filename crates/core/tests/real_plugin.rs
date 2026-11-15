//! Exercises the host against the *real* android plugin binary.
//!
//! Skipped when the plugin has not been packaged yet
//! (`scripts/package-plugin.ps1 -Plugin android`).

use castpane_core::{PluginEvent, PluginHost};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[test]
fn talks_to_the_packaged_android_plugin() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../plugins/android");
    if !dir.join("bin/android-plugin.exe").exists() {
        eprintln!("skip: run scripts/package-plugin.ps1 -Plugin android first");
        return;
    }

    let mut host = PluginHost::load(&dir).expect("load android manifest");
    assert_eq!(host.manifest().id, "android");
    host.start().expect("plugin handshake");

    host.call("discover.start", serde_json::json!({}))
        .expect("discover.start");

    // With no device attached we still expect the plugin to say *something*
    // (a log line about missing adb) rather than stay silent.
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut saw_event = false;
    while Instant::now() < deadline && !saw_event {
        for event in host.drain_events() {
            if matches!(event, PluginEvent::Notification { .. }) {
                saw_event = true;
            }
        }
        if !saw_event {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    assert!(saw_event, "plugin emitted no notification at all");

    host.stop();
}
