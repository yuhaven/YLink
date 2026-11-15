use castpane_core::{Capability, PanelItem, PluginEvent, PluginHost, Rect, WindowSpec};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tauri::Manager;

/// What the UI needs to render one plugin card. Everything here comes from
/// `plugin.json`; the shell has no per-protocol knowledge.
#[derive(Debug, Serialize)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub capabilities: Vec<Capability>,
    /// JSON Schema for the plugin's settings (defaults, enums, ranges).
    pub schema: Value,
    pub panel: Vec<PanelItem>,
    pub running: bool,
    /// False when `bin/<plugin>.exe` has not been built yet.
    pub available: bool,
}

#[derive(Debug, Serialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub detail: String,
}

/// One asynchronous message from a plugin, forwarded to the UI.
#[derive(Debug, Serialize)]
pub struct PluginMessage {
    pub method: String,
    pub params: Value,
}

struct AppState {
    plugins: Mutex<Vec<PluginHost>>,
}

/// In development the plugins live in the repo; in a release build they ship
/// next to the executable.
fn plugins_dir() -> PathBuf {
    if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../plugins")
    } else {
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join("plugins")))
            .unwrap_or_else(|| PathBuf::from("plugins"))
    }
}

fn load_plugins() -> Vec<PluginHost> {
    let dir = plugins_dir();
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("plugins directory not found: {}", dir.display());
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        match PluginHost::load(&path) {
            Ok(host) => {
                if host.manifest().supports_current_platform() {
                    out.push(host);
                }
            }
            Err(err) => eprintln!("skip plugin {}: {err}", path.display()),
        }
    }
    out.sort_by(|a, b| a.manifest().id.cmp(&b.manifest().id));
    out
}

#[tauri::command]
fn list_plugins(state: tauri::State<'_, AppState>) -> Vec<PluginInfo> {
    let plugins = state.plugins.lock().unwrap();
    plugins
        .iter()
        .map(|host| {
            let m = host.manifest();
            PluginInfo {
                id: m.id.clone(),
                name: m.name.clone(),
                version: m.version.clone(),
                description: m.description.clone(),
                capabilities: m.capabilities.clone(),
                schema: m.settings.clone(),
                panel: m.panel.clone(),
                running: host.is_running(),
                available: m.entry_path().exists(),
            }
        })
        .collect()
}

#[tauri::command]
fn start_plugin(id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
    let mut plugins = state.plugins.lock().unwrap();
    let host = plugins
        .iter_mut()
        .find(|p| p.manifest().id == id)
        .ok_or_else(|| format!("plugin `{id}` not found"))?;
    host.start().map_err(|e| e.to_string())
}

/// Launch the plugin if needed, ask it to scan, and collect `device.found`.
#[tauri::command]
fn discover(id: String, state: tauri::State<'_, AppState>) -> Result<Vec<DeviceInfo>, String> {
    let mut plugins = state.plugins.lock().unwrap();
    let host = plugins
        .iter_mut()
        .find(|p| p.manifest().id == id)
        .ok_or_else(|| format!("plugin `{id}` not found"))?;

    if !host.is_running() {
        host.start().map_err(|e| e.to_string())?;
    }
    // Discard stale events from a previous scan.
    let _ = host.drain_events();

    host.call("discover.start", json!({}))
        .map_err(|e| e.to_string())?;

    // Discovery is asynchronous: give the plugin a moment to emit.
    std::thread::sleep(Duration::from_millis(500));

    let mut devices = Vec::new();
    for event in host.drain_events() {
        if let PluginEvent::Notification { method, params } = event {
            if method == "device.found" {
                devices.push(DeviceInfo {
                    id: params["id"].as_str().unwrap_or_default().to_string(),
                    name: params["name"].as_str().unwrap_or_default().to_string(),
                    detail: params["detail"].as_str().unwrap_or("device").to_string(),
                });
            }
        }
    }
    Ok(devices)
}

/// Start a session, placing the window with the core's portrait geometry.
#[tauri::command]
fn connect(
    id: String,
    device_id: String,
    settings: Option<Value>,
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Value, String> {
    let monitor = app
        .primary_monitor()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "no monitor detected".to_string())?;

    let position = monitor.position();
    let size = monitor.size();
    // Approximation of the work area: leave room for the taskbar.
    let work = Rect::new(
        position.x,
        position.y,
        size.width,
        size.height.saturating_sub(80),
    );
    let spec = WindowSpec::portrait(work);
    let window = json!({
        "x": spec.rect.x,
        "y": spec.rect.y,
        "width": spec.rect.width,
        "height": spec.rect.height,
        "borderless": spec.borderless,
        "alwaysOnTop": spec.always_on_top,
    });

    let mut plugins = state.plugins.lock().unwrap();
    let host = plugins
        .iter_mut()
        .find(|p| p.manifest().id == id)
        .ok_or_else(|| format!("plugin `{id}` not found"))?;

    if !host.is_running() {
        host.start().map_err(|e| e.to_string())?;
    }

    host.call(
        "session.start",
        json!({
            "deviceId": device_id,
            "window": window,
            "settings": settings.unwrap_or_else(|| json!({})),
        }),
    )
    .map_err(|e| e.to_string())
}

/// Invoke a declarative panel action (`action.<name>`) on a plugin.
#[tauri::command]
fn action(
    id: String,
    name: String,
    state: tauri::State<'_, AppState>,
) -> Result<Value, String> {
    let mut plugins = state.plugins.lock().unwrap();
    let host = plugins
        .iter_mut()
        .find(|p| p.manifest().id == id)
        .ok_or_else(|| format!("plugin `{id}` not found"))?;

    if !host.is_running() {
        host.start().map_err(|e| e.to_string())?;
    }

    host.call(&format!("action.{name}"), json!({}))
        .map_err(|e| e.to_string())
}

/// Drain pending plugin notifications (session state, logs, ...).
#[tauri::command]
fn poll_events(id: String, state: tauri::State<'_, AppState>) -> Vec<PluginMessage> {
    let mut plugins = state.plugins.lock().unwrap();
    let Some(host) = plugins.iter_mut().find(|p| p.manifest().id == id) else {
        return Vec::new();
    };
    host.drain_events()
        .into_iter()
        .filter_map(|event| match event {
            PluginEvent::Notification { method, params } => Some(PluginMessage { method, params }),
            _ => None,
        })
        .collect()
}

/// Bring the main window back from the tray.
fn show_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Tray icon with two entries: show the window, or really quit.
fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("CastPane")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// Headless self-check used by `CastPane.exe --diagnose` and by CI.
///
/// Starts every available plugin, asks it to diagnose itself, prints the
/// result and returns a process exit code (0 = everything passed).
/// Deliberately does not touch the GUI, so it is safe to run in a terminal
/// while the app (or a game) is running.
pub fn headless_diagnose() -> i32 {
    let plugins = load_plugins();
    println!(
        "CastPane diagnose — {} plugin(s) from {}",
        plugins.len(),
        plugins_dir().display()
    );
    if plugins.is_empty() {
        eprintln!("no plugins found");
        return 2;
    }

    let mut failures = 0usize;

    for mut host in plugins {
        let id = host.manifest().id.clone();
        let name = host.manifest().name.clone();
        let version = host.manifest().version.clone();
        let entry = host.manifest().entry_path();

        println!("\n[{id}] {name} v{version}");
        if !entry.exists() {
            println!("  ! 未安装：{}", entry.display());
            println!("    → powershell -File scripts/package-plugin.ps1 -Plugin {id}");
            failures += 1;
            continue;
        }

        if let Err(err) = host.start() {
            println!("  ✗ 启动失败：{err}");
            failures += 1;
            continue;
        }

        match host.call("diagnose", json!({})) {
            Ok(value) => {
                let checks = value
                    .get("checks")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if checks.is_empty() {
                    println!("  · 插件没有返回检查项");
                }
                for check in checks {
                    let ok = check.get("ok").and_then(Value::as_bool).unwrap_or(false);
                    let check_name = check.get("name").and_then(Value::as_str).unwrap_or("?");
                    let detail = check.get("detail").and_then(Value::as_str).unwrap_or("");
                    let hint = check.get("hint").and_then(Value::as_str);
                    println!(
                        "  {} {check_name}: {detail}{}",
                        if ok { "✓" } else { "✗" },
                        hint.map(|h| format!("  → {h}")).unwrap_or_default()
                    );
                    if !ok {
                        failures += 1;
                    }
                }
            }
            Err(err) => {
                println!("  ✗ diagnose 失败：{err}");
                failures += 1;
            }
        }

        host.stop();
    }

    println!("\n{failures} 个问题");
    if failures == 0 {
        0
    } else {
        1
    }
}

pub fn run() {
    let plugins = load_plugins();
    eprintln!(
        "loaded {} plugin(s) from {}",
        plugins.len(),
        plugins_dir().display()
    );

    tauri::Builder::default()
        // Must be registered first: a second launch should focus this window.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            show_window(app);
        }))
        .manage(AppState {
            plugins: Mutex::new(plugins),
        })
        .invoke_handler(tauri::generate_handler![
            list_plugins,
            start_plugin,
            discover,
            connect,
            action,
            poll_events
        ])
        .setup(|app| {
            build_tray(app)?;
            // `--silent` starts in the tray without showing a window: used by
            // background starts, autostart, and automated smoke checks.
            let silent = std::env::args().any(|arg| arg == "--silent" || arg == "--hidden");
            if !silent {
                show_window(app.handle());
            }
            Ok(())
        })
        // Closing the window hides it; quitting goes through the tray.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running CastPane");
}
