//! Wireless (same Wi-Fi) Android connection for phones without Android 11
//! "Wireless debugging" (for example HarmonyOS 4.x / EMUI).
//!
//! The default path is deliberately USB-assisted:
//!
//! 1. The phone is connected by USB once; `adb tcpip 5555` makes adbd listen
//!    on TCP.
//! 2. `adb connect <phone-ip>:5555` switches the session to Wi-Fi.
//! 3. The endpoint is remembered; later launches reconnect automatically.
//!
//! There is intentionally no pairing-code / QR flow here. HarmonyOS does not
//! expose Android's wireless-debugging pairing screen, so guiding users there
//! would be a dead end.

use crate::adb::{self, Device};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KnownDevice {
    /// Last known `ip:port` endpoint.
    pub id: String,
    pub name: String,
    pub ip: String,
    pub last_seen: u64,
    /// True when the endpoint was created with `adb tcpip` (port 5555).
    #[serde(default)]
    pub tcpip: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KnownStore {
    devices: Vec<KnownDevice>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WirelessDevice {
    pub id: String,
    pub name: String,
    /// `wireless` or `usb`.
    pub transport: String,
    /// `ready`, `device`, `offline`, `unauthorized`, ...
    pub state: String,
    /// Short user-facing line, e.g. `已连接 · 同一 Wi-Fi`.
    pub detail: String,
    pub endpoint: Option<String>,
    pub last_seen: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WirelessSnapshot {
    /// `ready` / `usb` / `unauthorized` / `offline` / `not-found` / `error`.
    pub state: String,
    /// The single status line shown on the home screen.
    pub status: String,
    pub hint: Option<String>,
    pub devices: Vec<WirelessDevice>,
    pub local_ip: Option<String>,
    pub phone_ip: Option<String>,
    pub same_network: Option<bool>,
    pub known: usize,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectResult {
    pub ok: bool,
    pub device: Option<WirelessDevice>,
    pub status: String,
    pub hint: Option<String>,
    pub code: Option<String>,
    pub raw: Option<String>,
}

pub struct Wireless {
    known_path: PathBuf,
    last_auto: Mutex<Option<Instant>>,
}

impl Default for Wireless {
    fn default() -> Self {
        Self::with_state_dir(default_state_dir())
    }
}

impl Wireless {
    pub fn with_state_dir(dir: PathBuf) -> Self {
        Self {
            known_path: dir.join("android-wireless.json"),
            last_auto: Mutex::new(None),
        }
    }

    /// Scan connected devices, try one remembered Wi-Fi endpoint, and return
    /// the one-line status the home screen renders.
    pub fn scan(&self, adb_path: &Path) -> WirelessSnapshot {
        let mut diagnostics = Vec::new();
        let now = now_secs();
        let local_ip = local_ipv4();
        diagnostics.push(format!(
            "电脑地址：{}",
            local_ip.clone().unwrap_or_else(|| "未知".into())
        ));

        let mut store = load_store(&self.known_path);
        let mut devices = match adb::list_devices(adb_path) {
            Ok(devices) => devices,
            Err(err) => {
                diagnostics.push(format!("adb devices 失败：{err}"));
                Vec::new()
            }
        };
        diagnostics.push(format!(
            "adb devices：{} 台在线（{}）",
            devices.len(),
            devices
                .iter()
                .map(|d| format!("{}={}", d.serial, d.state))
                .collect::<Vec<_>>()
                .join(", ")
        ));

        // Auto-reconnect once every 15s. This is the "open the app again on
        // the same Wi-Fi" path: no click and no pairing screen.
        if !has_connected_wireless(&devices) && !store.devices.is_empty() {
            let should_try = {
                let guard = self.last_auto.lock().unwrap();
                guard
                    .map(|last| last.elapsed() >= Duration::from_secs(15))
                    .unwrap_or(true)
            };
            if should_try {
                for endpoint in reconnect_candidates(&store).iter().take(3) {
                    match adb::connect(adb_path, endpoint) {
                        Ok(out) => diagnostics.push(format!(
                            "自动重连 {endpoint}：{}",
                            first_line(&out.combined())
                        )),
                        Err(err) => diagnostics.push(format!("自动重连 {endpoint} 失败：{err}")),
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
                std::thread::sleep(Duration::from_millis(250));
                devices = adb::list_devices(adb_path).unwrap_or(devices);
            }
            *self.last_auto.lock().unwrap() = Some(Instant::now());
        }

        let mut out: Vec<WirelessDevice> = Vec::new();
        let mut seen_ips: HashSet<String> = HashSet::new();

        for device in devices
            .iter()
            .filter(|d| adb::is_wireless_serial(&d.serial))
        {
            let Some((ip, _)) = adb::parse_endpoint(&device.serial) else {
                continue;
            };
            let name = display_name(device);
            if device.state == "device" {
                remember(&mut store, &device.serial, &name, now);
            }
            let detail = match device.state.as_str() {
                "device" => "已连接 · 同一 Wi-Fi",
                "unauthorized" => "未授权 · 请在手机上允许调试",
                "offline" => "已掉线 · 请重新连接",
                other => other,
            };
            seen_ips.insert(ip);
            out.push(WirelessDevice {
                id: device.serial.clone(),
                name,
                transport: "wireless".into(),
                state: if device.state == "device" {
                    "ready".into()
                } else {
                    device.state.clone()
                },
                detail: detail.into(),
                endpoint: Some(device.serial.clone()),
                last_seen: Some(now),
            });
        }

        let mut usb_devices: Vec<Device> = Vec::new();
        for device in devices
            .iter()
            .filter(|d| !adb::is_wireless_serial(&d.serial))
        {
            usb_devices.push(device.clone());
            let name = display_name(device);
            let detail = match device.state.as_str() {
                "device" => "USB 已连接 · 可一键开启无线",
                "unauthorized" => "USB 未授权 · 请在手机上允许 USB 调试",
                "offline" => "USB 已掉线",
                other => other,
            };
            out.push(WirelessDevice {
                id: device.serial.clone(),
                name,
                transport: "usb".into(),
                state: device.state.clone(),
                detail: detail.into(),
                endpoint: None,
                last_seen: Some(now),
            });
        }

        // Keep one remembered endpoint visible when nothing is live, so the
        // user can retry without plugging in first.
        if out.is_empty() && !store.devices.is_empty() {
            let known = &store.devices[0];
            out.push(WirelessDevice {
                id: known.id.clone(),
                name: known.name.clone(),
                transport: "wireless".into(),
                state: "offline".into(),
                detail: "上次连接过 · 当前不在线".to_string(),
                endpoint: Some(known.id.clone()),
                last_seen: Some(known.last_seen),
            });
            seen_ips.insert(known.ip.clone());
        }

        let usb = usb_devices.iter().find(|d| d.state == "device");
        let usb_unauthorized = usb_devices.iter().any(|d| d.state == "unauthorized");
        let usb_ip = usb
            .map(|d| d.serial.as_str())
            .and_then(|serial| adb::wifi_ipv4(adb_path, serial));
        let connected_phone_ip = out
            .iter()
            .find(|d| d.transport == "wireless" && d.state == "ready")
            .and_then(|d| d.endpoint.as_deref())
            .and_then(|e| adb::parse_endpoint(e).map(|(ip, _)| ip));
        let phone_ip = connected_phone_ip
            .or(usb_ip)
            .or_else(|| store.devices.first().map(|k| k.ip.clone()));
        let same_network = match (&local_ip, &phone_ip) {
            (Some(a), Some(b)) => Some(same_subnet(a, b)),
            _ => None,
        };
        if same_network == Some(false) {
            diagnostics.push(format!(
                "网段不同：电脑 {}，手机 {}",
                local_ip.clone().unwrap_or_default(),
                phone_ip.clone().unwrap_or_default()
            ));
        }

        let (state, status, hint) = decide_status(
            &out,
            usb.is_some(),
            usb_unauthorized,
            same_network,
            &local_ip,
            &phone_ip,
        );

        let mut store_changed = false;
        for device in out.iter().filter(|d| d.state == "ready") {
            if let Some(endpoint) = &device.endpoint {
                store_changed |= remember(&mut store, endpoint, &device.name, now);
            }
        }
        if store_changed {
            let _ = save_store(&self.known_path, &store);
        }

        WirelessSnapshot {
            state,
            status,
            hint,
            devices: out,
            local_ip,
            phone_ip,
            same_network,
            known: store.devices.len(),
            diagnostics,
        }
    }

    /// Connect to a remembered/discovered `ip:port` endpoint.
    pub fn connect(&self, adb_path: &Path, device_id: &str) -> ConnectResult {
        let device_id = device_id.trim();
        if adb::parse_endpoint(device_id).is_none() {
            return ConnectResult {
                ok: false,
                device: None,
                status: "连接地址无效".into(),
                hint: Some("请重新检测设备".into()),
                code: Some("E_ENDPOINT".into()),
                raw: None,
            };
        }

        let mut devices = adb::list_devices(adb_path).unwrap_or_default();
        if let Some(device) = devices.iter().find(|d| d.serial == device_id) {
            if device.state == "device" {
                return self.ready_result(device_id, &display_name(device), now_secs());
            }
        }

        let out = match adb::connect(adb_path, device_id) {
            Ok(out) => out,
            Err(err) => {
                return ConnectResult {
                    ok: false,
                    device: None,
                    status: format!("连接失败：{err}"),
                    hint: Some("确认手机与电脑在同一 Wi-Fi".into()),
                    code: Some("E_CONNECT".into()),
                    raw: None,
                }
            }
        };

        let text = out.combined();
        std::thread::sleep(Duration::from_millis(450));
        devices = adb::list_devices(adb_path).unwrap_or(devices);
        if let Some(device) = devices
            .iter()
            .find(|d| d.serial == device_id && d.state == "device")
        {
            return self.ready_result(device_id, &display_name(device), now_secs());
        }

        let lower = text.to_ascii_lowercase();
        if lower.contains("authenticate") || lower.contains("unauthorized") {
            return ConnectResult {
                ok: false,
                device: None,
                status: "无线连接未获授权".into(),
                hint: Some("请在手机上允许 USB 调试，再插线点一次「一键开启无线」".into()),
                code: Some("E_PERMISSION".into()),
                raw: Some(text),
            };
        }

        let hint = if out.timed_out {
            "连接超时：手机可能休眠、Wi-Fi 已断开，或不在同一网段"
        } else {
            "手机重启或切换网络后，tcpip 会失效；插一次数据线，点「一键开启无线」即可恢复"
        };
        ConnectResult {
            ok: false,
            device: None,
            status: "无线连接不可达".into(),
            hint: Some(hint.into()),
            code: Some("E_CONNECT".into()),
            raw: Some(text),
        }
    }

    /// One-time USB-assisted switch to Wi-Fi (`adb tcpip 5555`).
    pub fn usb_bootstrap(&self, adb_path: &Path) -> ConnectResult {
        let devices = adb::list_devices(adb_path).unwrap_or_default();
        let Some(usb) = devices
            .iter()
            .find(|d| !adb::is_wireless_serial(&d.serial) && d.state == "device")
        else {
            let unauthorized = devices
                .iter()
                .any(|d| !adb::is_wireless_serial(&d.serial) && d.state == "unauthorized");
            return ConnectResult {
                ok: false,
                device: None,
                status: if unauthorized {
                    "USB 调试未授权".into()
                } else {
                    "没有检测到 USB 手机".into()
                },
                hint: Some(if unauthorized {
                    "在手机上点「允许 USB 调试」，建议同时打开「仅充电模式下允许 ADB 调试」"
                        .into()
                } else {
                    "用数据线连接手机，并在开发者选项里打开 USB 调试".into()
                }),
                code: Some(if unauthorized {
                    "E_PERMISSION".into()
                } else {
                    "E_NO_USB".into()
                }),
                raw: None,
            };
        };

        let phone_ip = adb::wifi_ipv4(adb_path, &usb.serial);
        let Some(phone_ip) = phone_ip else {
            return ConnectResult {
                ok: false,
                device: None,
                status: "手机没有连接 Wi-Fi".into(),
                hint: Some("请先把手机连接到与电脑相同的 Wi-Fi".into()),
                code: Some("E_NO_WIFI".into()),
                raw: None,
            };
        };
        if let Some(local_ip) = local_ipv4() {
            if !same_subnet(&local_ip, &phone_ip) {
                return ConnectResult {
                    ok: false,
                    device: None,
                    status: "手机与电脑不在同一 Wi-Fi".into(),
                    hint: Some(format!("电脑 {local_ip}，手机 {phone_ip}，请连接同一网络")),
                    code: Some("E_NETWORK".into()),
                    raw: None,
                };
            }
        }

        let endpoint = format!("{phone_ip}:5555");
        if let Some(device) = devices
            .iter()
            .find(|d| d.serial == endpoint && d.state == "device")
        {
            return self.ready_result(&endpoint, &display_name(device), now_secs());
        }

        match adb::tcpip(adb_path, &usb.serial, 5555) {
            Ok(out) if !out.success && !out.contains("restarting") => {
                return ConnectResult {
                    ok: false,
                    device: None,
                    status: "无法开启手机无线端口".into(),
                    hint: Some("手机可能禁止 adb tcpip，或系统版本不支持；可先用 USB 投屏".into()),
                    code: Some("E_TCPIP".into()),
                    raw: Some(out.combined()),
                }
            }
            Err(err) => {
                return ConnectResult {
                    ok: false,
                    device: None,
                    status: "无法开启手机无线端口".into(),
                    hint: Some(err),
                    code: Some("E_TCPIP".into()),
                    raw: None,
                }
            }
            _ => {}
        }

        std::thread::sleep(Duration::from_millis(1200));
        let mut out = adb::connect(adb_path, &endpoint);
        let mut result = self.connect(adb_path, &endpoint);
        // adbd restarts after `tcpip`; on slower phones the first connect can
        // race the restart. Retry twice before declaring failure.
        for _ in 0..2 {
            if result.ok {
                break;
            }
            std::thread::sleep(Duration::from_millis(900));
            out = adb::connect(adb_path, &endpoint);
            result = self.connect(adb_path, &endpoint);
        }
        if result.ok {
            result.status = "已切到无线，可以拔掉数据线".into();
            if let Some(device) = result.device.as_mut() {
                device.detail = "已切到无线 · 可拔线".into();
            }
            return result;
        }
        if let Ok(out) = out {
            result.raw = Some(out.combined());
        }
        result.status = "USB 转无线失败".into();
        result.hint = Some(format!(
            "确认手机仍在 {}，且端口 5555 未被占用；也可重插数据线后再试",
            phone_ip
        ));
        result.code = Some("E_USB_BOOTSTRAP".into());
        result
    }

    pub fn disconnect(&self, adb_path: &Path, device_id: &str) -> bool {
        adb::disconnect(adb_path, device_id)
            .map(|out| out.success || out.contains("disconnected"))
            .unwrap_or(false)
    }

    fn ready_result(&self, endpoint: &str, name: &str, now: u64) -> ConnectResult {
        let mut store = load_store(&self.known_path);
        remember(&mut store, endpoint, name, now);
        let _ = save_store(&self.known_path, &store);
        ConnectResult {
            ok: true,
            device: Some(WirelessDevice {
                id: endpoint.to_string(),
                name: name.to_string(),
                transport: "wireless".into(),
                state: "ready".into(),
                detail: "已连接 · 同一 Wi-Fi".into(),
                endpoint: Some(endpoint.to_string()),
                last_seen: Some(now),
            }),
            status: "已连接，可以开始投屏".into(),
            hint: None,
            code: None,
            raw: None,
        }
    }
}

fn decide_status(
    devices: &[WirelessDevice],
    usb_present: bool,
    usb_unauthorized: bool,
    same_network: Option<bool>,
    local_ip: &Option<String>,
    phone_ip: &Option<String>,
) -> (String, String, Option<String>) {
    let connected: Vec<&WirelessDevice> = devices
        .iter()
        .filter(|d| d.transport == "wireless" && d.state == "ready")
        .collect();
    if let Some(device) = connected.first() {
        let status = if connected.len() == 1 {
            format!("已连接 {} · 同一 Wi-Fi", device.name)
        } else {
            format!("已连接 {} 台手机 · 同一 Wi-Fi", connected.len())
        };
        return (
            "ready".into(),
            status,
            Some("可以拔掉数据线，点「开始投屏」".into()),
        );
    }

    if same_network == Some(false) {
        return (
            "error".into(),
            "手机与电脑不在同一 Wi-Fi".into(),
            Some(format!(
                "电脑 {}，手机 {}。请连接同一个 Wi-Fi 后重试",
                local_ip.clone().unwrap_or_default(),
                phone_ip.clone().unwrap_or_default()
            )),
        );
    }

    if usb_unauthorized {
        return (
            "unauthorized".into(),
            "USB 调试未授权 · 请在手机上点「允许」".into(),
            Some("建议同时打开「仅充电模式下允许 ADB 调试」".into()),
        );
    }

    if usb_present {
        return (
            "usb".into(),
            "手机已用 USB 连接 · 一键开启无线".into(),
            Some("开启后即可拔线；首次只需一次数据线".into()),
        );
    }

    if let Some(device) = devices
        .iter()
        .find(|d| d.transport == "wireless" && d.state == "offline")
    {
        return (
            "offline".into(),
            format!("{} 的无线连接已失效", device.name),
            Some("手机重启或切换 Wi-Fi 后需重新插线；插好后点「一键开启无线」".into()),
        );
    }

    (
        "not-found".into(),
        "请用数据线连接手机一次".into(),
        Some(
            "设置 → 系统和更新 → 开发人员选项 → 打开 USB 调试；再点「检测手机」".into(),
        ),
    )
}

fn reconnect_candidates(store: &KnownStore) -> Vec<String> {
    let mut out: Vec<String> = store
        .devices
        .iter()
        .map(|known| {
            if known.tcpip || known.id.ends_with(":5555") {
                format!("{}:5555", known.ip)
            } else {
                known.id.clone()
            }
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

fn has_connected_wireless(devices: &[Device]) -> bool {
    devices
        .iter()
        .any(|d| adb::is_wireless_serial(&d.serial) && d.state == "device")
}

fn display_name(device: &Device) -> String {
    if device.name.is_empty() || device.name == device.serial {
        device.serial.clone()
    } else {
        device.name.clone()
    }
}

fn first_line(text: &str) -> String {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn remember(store: &mut KnownStore, endpoint: &str, name: &str, now: u64) -> bool {
    let Some((ip, port)) = adb::parse_endpoint(endpoint) else {
        return false;
    };
    let tcpip = port == 5555;
    if let Some(existing) = store.devices.iter_mut().find(|d| d.ip == ip) {
        let changed = existing.id != endpoint
            || existing.name != name
            || existing.tcpip != tcpip
            || now.saturating_sub(existing.last_seen) > 60;
        existing.id = endpoint.to_string();
        existing.name = name.to_string();
        existing.tcpip = tcpip;
        if changed {
            existing.last_seen = now;
        }
        return changed;
    }
    store.devices.push(KnownDevice {
        id: endpoint.to_string(),
        name: name.to_string(),
        ip,
        last_seen: now,
        tcpip,
    });
    store.devices.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
    store.devices.truncate(8);
    true
}

fn load_store(path: &Path) -> KnownStore {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_store(path: &Path, store: &KnownStore) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let raw = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    std::fs::write(path, raw).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

pub fn default_state_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("CASTPANE_STATE_DIR") {
        return PathBuf::from(dir);
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local).join("CastPane");
    }
    std::env::temp_dir().join("CastPane")
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Primary non-loopback IPv4. UDP `connect` only consults the routing table;
/// it does not send a packet or require internet access.
pub fn local_ipv4() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let ip = socket.local_addr().ok()?.ip();
    if ip.is_loopback() || ip.is_unspecified() {
        None
    } else {
        Some(ip.to_string())
    }
}

fn same_subnet(a: &str, b: &str) -> bool {
    let (Ok(a), Ok(b)) = (
        a.parse::<std::net::Ipv4Addr>(),
        b.parse::<std::net::Ipv4Addr>(),
    ) else {
        return false;
    };
    let a = u32::from(a);
    let b = u32::from(b);
    // /24 is the overwhelmingly common home Wi-Fi setup. We deliberately do
    // not guess netmasks from a DHCP lease we cannot read.
    (a & 0xFFFF_FF00) == (b & 0xFFFF_FF00)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str, state: &str, transport: &str) -> WirelessDevice {
        WirelessDevice {
            id: id.into(),
            name: "Phone".into(),
            transport: transport.into(),
            state: state.into(),
            detail: String::new(),
            endpoint: Some(id.into()),
            last_seen: None,
        }
    }

    #[test]
    fn same_subnet_compares_the_first_three_octets() {
        assert!(same_subnet("192.168.1.6", "192.168.1.9"));
        assert!(!same_subnet("192.168.1.6", "192.168.2.9"));
        assert!(!same_subnet("not-an-ip", "192.168.1.9"));
    }

    #[test]
    fn remembers_and_updates_known_devices_by_ip() {
        let mut store = KnownStore::default();
        assert!(remember(&mut store, "192.168.1.5:41000", "Phone", 100));
        assert!(!remember(&mut store, "192.168.1.5:41000", "Phone", 101));
        assert!(remember(&mut store, "192.168.1.5:5555", "Phone", 102));
        assert_eq!(store.devices.len(), 1);
        assert_eq!(store.devices[0].id, "192.168.1.5:5555");
        assert!(store.devices[0].tcpip);
    }

    #[test]
    fn reconnect_uses_port_5555_for_tcpip_entries() {
        let store = KnownStore {
            devices: vec![KnownDevice {
                id: "192.168.1.5:40000".into(),
                name: "Phone".into(),
                ip: "192.168.1.5".into(),
                last_seen: 1,
                tcpip: true,
            }],
        };
        assert_eq!(reconnect_candidates(&store), vec!["192.168.1.5:5555"]);
    }

    #[test]
    fn status_prefers_connected_wireless_over_usb() {
        let devices = vec![
            device("192.168.1.5:5555", "ready", "wireless"),
            device("USB123", "device", "usb"),
        ];
        let (state, status, _) = decide_status(
            &devices,
            true,
            false,
            Some(true),
            &Some("192.168.1.6".into()),
            &Some("192.168.1.5".into()),
        );
        assert_eq!(state, "ready");
        assert!(status.contains("已连接"));
    }

    #[test]
    fn status_explains_network_mismatch() {
        let (state, status, hint) = decide_status(
            &[],
            true,
            false,
            Some(false),
            &Some("192.168.1.6".into()),
            &Some("10.0.0.5".into()),
        );
        assert_eq!(state, "error");
        assert!(status.contains("不在同一 Wi-Fi"));
        assert!(hint.unwrap().contains("192.168.1.6"));
    }

    #[test]
    fn status_guides_usb_first_time_setup() {
        let devices = vec![device("USB123", "device", "usb")];
        let (state, status, hint) = decide_status(
            &devices,
            true,
            false,
            Some(true),
            &Some("192.168.1.6".into()),
            &Some("192.168.1.5".into()),
        );
        assert_eq!(state, "usb");
        assert!(status.contains("一键开启无线"));
        assert!(hint.unwrap().contains("拔线"));
    }

    #[test]
    fn status_explains_expired_tcpip_after_reboot() {
        let devices = vec![device("192.168.1.5:5555", "offline", "wireless")];
        let (state, status, hint) = decide_status(
            &devices,
            false,
            false,
            Some(true),
            &Some("192.168.1.6".into()),
            &Some("192.168.1.5".into()),
        );
        assert_eq!(state, "offline");
        assert!(status.contains("已失效"));
        assert!(hint.unwrap().contains("重新插线"));
    }

    #[test]
    fn status_explains_missing_usb_authorization() {
        let (state, status, hint) = decide_status(
            &[],
            false,
            true,
            Some(true),
            &Some("192.168.1.6".into()),
            &None,
        );
        assert_eq!(state, "unauthorized");
        assert!(status.contains("未授权"));
        assert!(hint.unwrap().contains("仅充电"));
    }

    #[test]
    fn known_store_roundtrips() {
        let dir = std::env::temp_dir().join(format!(
            "castpane-wireless-test-{}-{}",
            std::process::id(),
            now_secs()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("android-wireless.json");
        let mut store = KnownStore::default();
        remember(&mut store, "192.168.1.5:5555", "Phone", 42);
        save_store(&path, &store).unwrap();
        let loaded = load_store(&path);
        assert_eq!(loaded.devices.len(), 1);
        assert_eq!(loaded.devices[0].name, "Phone");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
