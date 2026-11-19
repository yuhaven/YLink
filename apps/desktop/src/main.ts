import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import "./style.css";

interface JsonSchemaProperty {
  type?: string;
  default?: unknown;
  enum?: string[];
  title?: string;
}

interface JsonSchema {
  properties?: Record<string, JsonSchemaProperty>;
}

interface PanelItem {
  type: "switch" | "select" | "slider" | "button" | "text" | "divider";
  key?: string;
  label?: string;
  text?: string;
  action?: string;
  min?: number;
  max?: number;
  step?: number;
}

interface PluginInfo {
  id: string;
  name: string;
  version: string;
  description: string;
  capabilities: string[];
  schema: JsonSchema;
  panel: PanelItem[];
  running: boolean;
  available: boolean;
}

interface WirelessDevice {
  id: string;
  name: string;
  transport: "wireless" | "usb" | "airplay";
  state: string;
  detail: string;
  endpoint?: string | null;
  lastSeen?: number | null;
}

interface DeviceInfo {
  id: string;
  name: string;
  detail: string;
}

interface FileEntry {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  modified: string;
  kind: "folder" | "image" | "video" | "doc" | "audio" | "archive" | "file";
}

interface DirListing {
  path: string;
  parent?: string | null;
  entries: FileEntry[];
  restricted: boolean;
}

interface TransferView {
  id: string;
  direction: "push" | "pull";
  name: string;
  state: "running" | "done" | "error";
  bytes: number;
  total: number;
  percent: number;
  speed: number;
  localPath?: string | null;
  remotePath?: string | null;
  error?: string | null;
  hint?: string | null;
  filesDone: number;
  filesTotal: number;
}

interface FilesHome {
  downloadDir: string;
  phoneRoot: string;
  phoneUploadDir: string;
}


interface WirelessSnapshot {
  state: "ready" | "usb" | "unauthorized" | "offline" | "not-found" | "error";
  status: string;
  hint?: string | null;
  devices: WirelessDevice[];
  localIp?: string | null;
  phoneIp?: string | null;
  sameNetwork?: boolean | null;
  known: number;
  diagnostics: string[];
}

interface ConnectResult {
  ok: boolean;
  device?: WirelessDevice | null;
  status: string;
  hint?: string | null;
  code?: string | null;
  raw?: string | null;
}

interface PluginMessage {
  method: string;
  params: Record<string, unknown>;
}

interface Check {
  name: string;
  ok: boolean;
  detail: string;
  hint?: string | null;
}

const $ = <T extends HTMLElement>(selector: string): T =>
  document.querySelector(selector) as T;

const deviceList = $("#device-list");
const homeEl = $("#home");
const primary = $<HTMLButtonElement>("#primary-action");
const statusDot = $("#status-dot");
const statusText = $("#status-text");
const networkPill = $("#network-pill");
const networkText = $("#network-text");
const moreToggle = $<HTMLButtonElement>("#more-toggle");
const more = $("#more");
const moreBody = $("#more-body");
const closeMore = $<HTMLButtonElement>("#close-more");
const overlay = $("#overlay");
const progress = $("#progress");
const progressTitle = $("#progress-title");
const progressMessage = $("#progress-message");
const progressSteps = $("#progress-steps");
const progressError = $("#progress-error");
const progressActions = $("#progress-actions");
const toastEl = $("#toast");
const versionEl = $("#version");
const stateProbe = $("#state-probe");
const pageProbe = $("#page-probe");
const transferProbe = $("#transfer-probe");
const filesButton = $<HTMLButtonElement>("#files-button");

const airplayPage = $("#airplay-page");
const airplayBack = $<HTMLButtonElement>("#airplay-back");
const airplayPill = $("#airplay-pill");
const airplayPillText = $("#airplay-pill-text");
const airplayTitle = $("#airplay-title");
const airplaySub = $("#airplay-sub");
const airplayStateEl = $("#airplay-state");
const airplayNameEl = $("#airplay-name");
const airplayStepName = $("#airplay-step-name");
const airplayIpEl = $("#airplay-ip");
const airplayError = $("#airplay-error");
const airplayPrimary = $<HTMLButtonElement>("#airplay-primary");
const airplayDiagnose = $<HTMLButtonElement>("#airplay-diagnose");
const airplayInstall = $<HTMLButtonElement>("#airplay-install");
const airplayChecks = $("#airplay-checks");

const filesPage = $("#files-page");
const filesBack = $<HTMLButtonElement>("#files-back");
const filesRefresh = $<HTMLButtonElement>("#files-refresh");
const filesDeviceEl = $("#files-device");
const filesPathEl = $("#files-path");
const filesSend = $<HTMLButtonElement>("#files-send");
const filesDownload = $<HTMLButtonElement>("#files-download");
const filesDrop = $("#files-drop");
const filesList = $("#files-list");
const filesDropHint = $("#files-drop-hint");
const filesNote = $("#files-note");
const transferCard = $("#transfer-card");
const transferDirection = $("#transfer-direction");
const transferName = $("#transfer-name");
const transferPercent = $("#transfer-percent");
const transferBar = $("#transfer-bar");
const transferBytes = $("#transfer-bytes");
const transferSpeed = $("#transfer-speed");
const transferResult = $("#transfer-result");
const transferActions = $("#transfer-actions");
const transferHistory = $("#transfer-history");

let android: PluginInfo | null = null;
let airplay: PluginInfo | null = null;
let androidValues: Record<string, unknown> = {};
let airplayValues: Record<string, unknown> = {};
let snapshot: WirelessSnapshot | null = null;
let selectedId: string | null = null;
let userPickedDevice = false;
let active: { pluginId: string; deviceId: string; name: string; transport: string } | null = null;
let airplayStarted = false;
let streaming = false;
let scanning = false;
let busy = false;
let restarting = false;
let stopping = false;
let moreOpen = false;
let moreBuilt = false;
let scanSeq = 0;
let toastTimer: number | undefined;
let applyTimer: number | undefined;
let startTimeout: number | undefined;
let holdStatusUntil = 0;

let airplayDevice: DeviceInfo | null = null;
let airplayState: "stopped" | "connecting" | "waiting" | "streaming" | "error" = "stopped";
let airplayHints: Check[] = [];
let airplayNetwork = "";
let airplayErrorCode = "";
let airplayMessage = "";
let airplayHintText = "";

let page: "home" | "airplay" | "files" = "home";
let filesDeviceId: string | null = null;
let filesDeviceName = "";
let filesPath = "/sdcard";
let filesListing: DirListing | null = null;
let filesLoading = false;
let filesError: { message: string; hint?: string | null } | null = null;
let filesSelected = new Set<string>();
let filesHome: FilesHome | null = null;
let transfer: TransferView | null = null;
let transferHistoryItems: TransferView[] = [];
let historyIds = new Set<string>();
let lastTransfer: { kind: "push" | "pull"; paths: string[]; target: string } | null = null;

const logs: string[] = [];

/* ---------- small helpers ---------- */

function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  cls?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (text !== undefined) node.textContent = text;
  return node;
}

const ICONS = {
  wifi: '<path d="M4 10.5a12 12 0 0 1 16 0M7 14a8 8 0 0 1 10 0M10 17.4a3.4 3.4 0 0 1 4 0" stroke="currentColor" stroke-width="2" stroke-linecap="round"/><circle cx="12" cy="20" r="1.3" fill="currentColor"/>',
  phone:
    '<rect x="7" y="2.6" width="10" height="18.8" rx="2.4" stroke="currentColor" stroke-width="1.7"/><path d="M10.6 18.6h2.8" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>',
  usb: '<path d="M12 3v12M12 15l-3-3M12 15l3-3" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/><rect x="9.4" y="2" width="5.2" height="4" rx="1" stroke="currentColor" stroke-width="1.6"/><circle cx="12" cy="19" r="2" stroke="currentColor" stroke-width="1.6"/>',
  cast: '<path d="M4 10.5a12 12 0 0 1 16 0M7.5 14a8 8 0 0 1 9 0M11 17.4a2.8 2.8 0 0 1 2 0" stroke="currentColor" stroke-width="2.1" stroke-linecap="round"/>',
  stop: '<rect x="7" y="7" width="10" height="10" rx="2" fill="currentColor"/>',
  refresh:
    '<path d="M20 11a8 8 0 1 0-1.8 7.1M20 5v6h-6" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>',
  airplay:
    '<rect x="3" y="4.5" width="18" height="12" rx="2" stroke="currentColor" stroke-width="1.6"/><path d="M8 20h8M12 16.5V20" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/><path d="M12 13.2a3.4 3.4 0 0 1 3.4 3.3M12 10.4a6.2 6.2 0 0 1 6.2 6.1" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>',
  folder:
    '<path d="M4 7.5A1.5 1.5 0 0 1 5.5 6H9l2 2h7.5A1.5 1.5 0 0 1 20 9.5v8A1.5 1.5 0 0 1 18.5 19h-13A1.5 1.5 0 0 1 4 17.5z" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"/>',
  image:
    '<rect x="3.5" y="4.5" width="17" height="15" rx="2" stroke="currentColor" stroke-width="1.7"/><circle cx="9" cy="10" r="1.6" stroke="currentColor" stroke-width="1.5"/><path d="m5 17 4.5-4.5 3.5 3.5 2.5-2.5L19 17" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>',
  video:
    '<rect x="3.5" y="5" width="17" height="14" rx="2" stroke="currentColor" stroke-width="1.7"/><path d="m10 9 5 3-5 3z" fill="currentColor"/>',
  doc:
    '<path d="M6 3.5h8l4 4V20a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 6 20z" stroke="currentColor" stroke-width="1.6"/><path d="M14 3.5V8h4M9 12h6M9 15.5h6" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>',
  audio:
    '<path d="M9 18V6l9-2v12" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/><circle cx="7" cy="18" r="2.2" stroke="currentColor" stroke-width="1.7"/><circle cx="16" cy="16" r="2.2" stroke="currentColor" stroke-width="1.7"/>',
  archive:
    '<rect x="4" y="4" width="16" height="16" rx="2" stroke="currentColor" stroke-width="1.6"/><path d="M12 4v6M9.5 7.5h5M9.5 12h5M9.5 15h5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>',
  file:
    '<path d="M6 3.5h8l4 4V20a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 6 20z" stroke="currentColor" stroke-width="1.6"/><path d="M14 3.5V8h4" stroke="currentColor" stroke-width="1.5"/>',
  android:
    '<path d="M7 9h10v7a2 2 0 0 1-2 2H9a2 2 0 0 1-2-2zM9 9 7.5 4.5M15 9l1.5-4.5M9.5 15h5" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/>',
};

function icon(name: keyof typeof ICONS, cls?: string): SVGSVGElement {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("fill", "none");
  svg.setAttribute("aria-hidden", "true");
  if (cls) svg.setAttribute("class", cls);
  svg.innerHTML = ICONS[name];
  return svg;
}

function errText(err: unknown): string {
  if (typeof err === "string") return err;
  if (err && typeof err === "object" && "message" in err) {
    return String((err as { message: unknown }).message);
  }
  try {
    return JSON.stringify(err);
  } catch {
    return String(err);
  }
}

function setStatus(text: string, kind: "" | "ok" | "warn" | "error" | "busy" = "", title?: string) {
  statusText.textContent = text;
  statusText.title = title ?? text;
  statusDot.className = `status-dot ${kind}`.trim();
}

function setProbe(token: string) {
  stateProbe.textContent = `state=${token}`;
}

function toast(message: string) {
  toastEl.textContent = message;
  toastEl.hidden = false;
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => {
    toastEl.hidden = true;
  }, 3200);
}

function logLine(message: string) {
  const line = `· ${message}`;
  logs.push(line);
  if (logs.length > 160) logs.shift();
  const box = document.querySelector<HTMLElement>("#log-box");
  if (box) {
    box.append(h("div", "log-line", line));
    box.scrollTop = box.scrollHeight;
  }
}

function airplayRow(): WirelessDevice | null {
  if (!airplay?.available) return null;
  const detail = airplayDevice?.detail ?? "engine-missing";
  const state =
    detail === "ready"
      ? "ready"
      : detail === "bonjour-missing"
        ? "bonjour-missing"
        : detail === "engine-incomplete"
          ? "engine-incomplete"
          : "engine-missing";
  return {
    id: "airplay:receiver",
    name: "iPad / iPhone 投屏",
    transport: "airplay",
    state,
    detail,
    endpoint: null,
  };
}

function phonePlaceholder(): WirelessDevice {
  return {
    id: "android:scan",
    name: "安卓手机",
    transport: "wireless",
    state: "not-found",
    detail: "首次插线开通，之后同一 Wi-Fi 无线投屏",
    endpoint: null,
  };
}

function visibleDevices(): WirelessDevice[] {
  const devices = snapshot?.devices ?? [];
  const hasReadyWireless = devices.some(
    (d) => d.transport === "wireless" && d.state === "ready",
  );
  const phones = (hasReadyWireless
    ? devices.filter((d) => d.transport === "wireless")
    : devices
  ).slice(0, 2);

  const air = airplayRow();
  if (air) {
    // Keep the AirPlay row visible and never let a second phone push it off
    // the first screen: one phone slot + one AirPlay slot.
    const keep = phones.slice(0, 1);
    if (keep.length === 0) keep.push(phonePlaceholder());
    return [...keep, air];
  }
  if (phones.length === 0) return [phonePlaceholder()];
  return phones;
}

function preferredDevice(): WirelessDevice | null {
  const devices = visibleDevices();
  return (
    devices.find((d) => d.transport === "wireless" && d.state === "ready") ??
    devices.find((d) => d.transport === "usb" && d.state === "device") ??
    devices.find((d) => d.transport === "airplay") ??
    devices[0] ??
    null
  );
}

function selectedDevice(): WirelessDevice | null {
  const devices = visibleDevices();
  return devices.find((d) => d.id === selectedId) ?? preferredDevice();
}

function deviceMeta(device: WirelessDevice): string {
  if (device.transport === "airplay") {
    if (device.state === "ready") return "控制中心 → 屏幕镜像 → CastPane";
    if (device.state === "bonjour-missing") return "缺少 Bonjour 发现服务 · 点开修复";
    if (device.state === "engine-incomplete") return "接收组件不完整 · 点开修复";
    return "接收端引擎缺失 · 点开查看";
  }
  if (device.state === "not-found") {
    return "数据线连接一次，打开 USB 调试后点「检测手机」";
  }
  if (device.transport === "usb") {
    if (device.state === "unauthorized") return "USB 未授权 · 在手机上点「允许」";
    return "USB · 首次开通后即可拔线";
  }
  if (device.state === "ready") {
    return `同一 Wi-Fi · ${device.endpoint ?? ""}`;
  }
  if (device.state === "offline") {
    return `上次 ${device.endpoint ?? ""} · 当前不在线`;
  }
  return device.detail;
}

/* ---------- rendering: home ---------- */

function renderEmptyState() {
  const wrap = h("div", "empty-state");
  const art = h("div", "empty-art");
  art.append(icon("phone"));
  const title = h("div", "empty-title", "同一 Wi-Fi，无线投屏");
  const sub = h(
    "div",
    "empty-sub",
    "第一次用数据线开通，之后同一 Wi-Fi 下不用再插线。",
  );
  const steps = h("ol", "mini-steps");
  const items = [
    "打开手机「设置 → 系统和更新 → 开发人员选项」。",
    "打开「USB 调试」；建议同时打开「仅充电模式下允许 ADB 调试」。",
    "用数据线连接电脑，手机上点「允许」，再点下方「检测手机」。",
  ];
  items.forEach((text, index) => {
    const li = h("li", "mini-step");
    li.append(h("b", undefined, String(index + 1)), h("span", undefined, text));
    steps.append(li);
  });
  wrap.append(art, title, sub, steps);
  deviceList.append(wrap);
}

function renderDevices() {
  const devices = visibleDevices();
  deviceList.replaceChildren();
  deviceList.classList.toggle("is-empty", devices.length === 0);

  if (devices.length === 0) {
    renderEmptyState();
    return;
  }

  if (!selectedId || !devices.some((d) => d.id === selectedId)) {
    selectedId = preferredDevice()?.id ?? null;
  }

  for (const device of devices) {
    const selected = device.id === selectedId;
    const row = h("button", `device${selected ? " selected" : ""}${device.transport === "airplay" ? " airplay" : ""}`);
    row.type = "button";
    row.dataset.deviceId = device.id;
    row.id = device.transport === "airplay" ? "airplay-row" : `device-${device.id.replace(/[^a-zA-Z0-9]/g, "-")}`;
    row.setAttribute("aria-pressed", selected ? "true" : "false");

    const tile = h(
      "div",
      `device-icon${device.transport === "usb" ? " usb" : ""}${
        device.state === "offline" || device.state === "not-found" ? " offline" : ""
      }${device.transport === "airplay" ? " airplay" : ""}`,
    );
    tile.append(
      icon(
        device.transport === "airplay"
          ? "airplay"
          : device.transport === "usb"
            ? "usb"
            : "phone",
      ),
    );

    const info = h("div", "device-info");
    info.append(
      h("div", "device-name", device.name),
      h("div", "device-meta", deviceMeta(device)),
    );

    const badge = h(
      "span",
      `device-badge${
        device.transport === "usb"
          ? " usb"
          : device.state === "offline"
            ? " offline"
            : device.transport === "airplay"
              ? " airplay"
              : ""
      }`,
      device.transport === "airplay"
        ? "AirPlay"
        : device.transport === "usb"
          ? "首次开通"
          : device.state === "offline"
            ? "已失效"
            : device.state === "not-found"
              ? "待连接"
              : "无线",
    );

    let mark: HTMLElement;
    if (selected) {
      mark = h("span", "device-check");
      mark.append(icon(device.transport === "airplay" ? "airplay" : "wifi"));
    } else {
      mark = h("span", "device-radio");
    }

    row.append(tile, info, badge, mark);
    row.addEventListener("click", () => {
      const already = selectedId === device.id;
      userPickedDevice = true;
      selectedId = device.id;
      renderDevices();
      renderPrimary();
      renderStatus();
      renderFilesEntry();
      if (already && device.transport === "airplay") openAirplayPage();
    });
    deviceList.append(row);
  }
}

function primaryAction(): { label: string; icon: keyof typeof ICONS | "spinner"; disabled: boolean } {
  if (busy || scanning) {
    return { label: scanning && !busy ? "正在检测…" : "正在处理…", icon: "spinner", disabled: true };
  }
  const device = selectedDevice();
  if (device?.transport === "airplay") {
    if (airplayState === "connecting") {
      return { label: "正在启动接收端…", icon: "spinner", disabled: true };
    }
    if (airplayState === "waiting" || airplayState === "streaming") {
      return { label: "停止接收端", icon: "stop", disabled: false };
    }
    if (device.state !== "ready") {
      return { label: "修复接收端", icon: "refresh", disabled: false };
    }
    return { label: "启动接收端", icon: "airplay", disabled: false };
  }
  if (streaming) return { label: "停止投屏", icon: "stop", disabled: false };
  if (snapshot?.sameNetwork === false) {
    return { label: "重新检测", icon: "refresh", disabled: false };
  }
  if (!device) return { label: "检测手机", icon: "refresh", disabled: false };
  if (device.state === "not-found") return { label: "检测手机", icon: "refresh", disabled: false };
  if (device.transport === "usb") {
    if (device.state === "unauthorized") {
      return { label: "我已在手机上允许", icon: "refresh", disabled: false };
    }
    return { label: "一键开启无线", icon: "cast", disabled: false };
  }
  if (device.state === "ready") return { label: "开始投屏", icon: "cast", disabled: false };
  if (device.state === "offline") return { label: "重新连接", icon: "refresh", disabled: false };
  return { label: "检测手机", icon: "refresh", disabled: false };
}

function renderPrimary() {
  const action = primaryAction();
  primary.replaceChildren();
  if (action.icon === "spinner") {
    primary.append(h("span", "spinner"));
  } else {
    primary.append(icon(action.icon));
  }
  primary.append(h("span", undefined, action.label));
  primary.disabled = action.disabled;
  primary.dataset.action = action.label;
  primary.setAttribute("aria-label", action.label);
}

function renderNetworkPill() {
  networkPill.className = "pill";
  if (snapshot?.sameNetwork === false) {
    networkPill.classList.add("warn");
    networkText.textContent = "网络不同";
    return;
  }
  if (snapshot?.state === "ready") {
    networkPill.classList.add("ok");
    networkText.textContent = "无线 · 同一 Wi-Fi";
    return;
  }
  networkText.textContent = "同一 Wi-Fi";
}

function renderStatus() {
  if (
    airplayState !== "stopped" &&
    (!streaming || selectedDevice()?.transport === "airplay")
  ) {
    if (airplayState === "connecting") {
      setStatus("正在启动 AirPlay 接收端…", "busy");
      setProbe("airplay-connecting");
      return;
    }
    if (airplayState === "waiting") {
      setStatus(
        `接收端已启动 · 在 iPad 控制中心选择 ${airplayValues.receiverName ?? "CastPane"}`,
        "ok",
        airplayHintText || "iPad 控制中心 → 屏幕镜像",
      );
      setProbe("airplay-waiting");
      return;
    }
    if (airplayState === "streaming") {
      setStatus("iPad 已连接 · 正在投屏", "ok", "关闭画面窗口只会断开本次连接，接收端仍在等待");
      setProbe("airplay-streaming");
      return;
    }
    if (airplayState === "error") {
      setStatus(airplayMessage || "AirPlay 接收端没有启动", "error", airplayHintText);
      setProbe("airplay-error");
      return;
    }
  }
  if (selectedDevice()?.transport === "airplay") {
    setStatus(
      "iPad 投屏 · 点「启动接收端」，再去控制中心选屏幕镜像",
      "",
      "接收端名称可在「更多 → AirPlay」修改",
    );
    setProbe("airplay-selected");
    return;
  }
  if (streaming) {
    const suffix = active?.transport === "wireless" ? " · 可拔线" : "";
    setStatus(
      `正在投屏 · ${active?.name ?? "手机"}${suffix}`,
      "ok",
      active?.transport === "wireless" ? "投屏走 Wi-Fi，数据线可以拔掉" : "USB 投屏",
    );
    setProbe("streaming");
    return;
  }
  if (busy) {
    setStatus("正在处理，请稍候…", "busy");
    setProbe("busy");
    return;
  }
  if (scanning && !snapshot) {
    setStatus("正在同一 Wi-Fi 中检测手机…", "busy");
    setProbe("scanning");
    return;
  }
  if (!snapshot) {
    setStatus("正在检测手机…", "busy");
    setProbe("starting");
    return;
  }
  if (Date.now() < holdStatusUntil) {
    setProbe("error");
    return;
  }
  const kind =
    snapshot.state === "ready"
      ? "ok"
      : snapshot.state === "usb" || snapshot.state === "offline"
        ? "warn"
        : snapshot.state === "error" || snapshot.state === "unauthorized"
          ? "error"
          : "";
  setStatus(snapshot.status, kind, snapshot.hint ?? snapshot.status);
  setProbe(snapshot.state);
}

function renderHome() {
  renderNetworkPill();
  renderDevices();
  renderPrimary();
  renderStatus();
  renderFilesEntry();
  if (moreOpen) renderMoreConnection();
}

function renderFilesEntry() {
  const device = selectedDevice();
  const connected =
    device &&
    device.transport !== "airplay" &&
    (device.state === "ready" ||
      (device.transport === "usb" && device.state === "device"));
  filesButton.hidden = page !== "home" || !connected;
  if (connected && device) filesButton.dataset.deviceId = device.id;
}

/* ---------- pages ---------- */

function setPage(next: "home" | "airplay" | "files") {
  page = next;
  homeEl.hidden = next !== "home";
  airplayPage.hidden = next !== "airplay";
  filesPage.hidden = next !== "files";
  airplayPage.setAttribute("aria-hidden", next === "airplay" ? "false" : "true");
  filesPage.setAttribute("aria-hidden", next === "files" ? "false" : "true");
  document.body.classList.toggle("subpage", next !== "home");
  pageProbe.textContent = `page=${next}`;
  if (next === "home") {
    renderHome();
    window.setTimeout(() => primary.focus(), 30);
  }
}

function receiverName(): string {
  const value = airplayValues.receiverName;
  return typeof value === "string" && value.trim() ? value : "CastPane";
}

function renderAirplayChecks() {
  airplayChecks.replaceChildren();
  for (const check of airplayHints) {
    const line = h(
      "div",
      `check-line ${check.ok ? "ok" : "bad"}`,
      `${check.ok ? "✓" : "✗"} ${check.name}：${check.detail}${!check.ok && check.hint ? ` — ${check.hint}` : ""}`,
    );
    airplayChecks.append(line);
  }
}

function airplayStateLabel(): string {
  switch (airplayState) {
    case "connecting":
      return "正在启动接收端";
    case "waiting":
      return "等待 iPad";
    case "streaming":
      return "iPad 已连接";
    case "error":
      return "未启动";
    default:
      return "未启动";
  }
}

function updateAirplayPage() {
  const name = receiverName();
  airplayNameEl.textContent = name;
  airplayStepName.textContent = name;
  airplayIpEl.textContent = airplayNetwork || "同一 Wi-Fi";

  const detail = airplayDevice?.detail ?? "ready";
  const engineMissing =
    detail === "engine-missing" || detail === "engine-incomplete" || detail === "bonjour-missing";

  airplayPill.className = "pill";
  if (airplayState === "connecting") {
    airplayPill.classList.add("warn");
    airplayPillText.textContent = "启动中";
  } else if (airplayState === "waiting" || airplayState === "streaming") {
    airplayPill.classList.add("ok");
    airplayPillText.textContent = airplayState === "streaming" ? "已连接" : "等待 iPad";
  } else if (airplayState === "error") {
    airplayPill.classList.add("warn");
    airplayPillText.textContent = "未启动";
  } else {
    airplayPillText.textContent = engineMissing ? "需要修复" : "未启动";
  }

  const state = h("span", "status-dot");
  if (airplayState === "waiting" || airplayState === "streaming") state.classList.add("ok");
  else if (airplayState === "connecting") state.classList.add("busy");
  else if (airplayState === "error") state.classList.add("error");
  airplayStateEl.className = "waiting-pill";
  if (airplayState === "connecting") airplayStateEl.classList.add("busy");
  if (airplayState === "error") airplayStateEl.classList.add("bad");
  airplayStateEl.replaceChildren(state, document.createTextNode(airplayStateLabel()));

  if (airplayState === "streaming") {
    airplayTitle.textContent = "iPad 正在投屏";
    airplaySub.textContent = "画面在右侧独立窗口；关掉窗口只会断开这次连接。";
  } else if (airplayState === "waiting") {
    airplayTitle.textContent = "等待 iPad 连接";
    airplaySub.textContent = "接收端正在广播，iPad 控制中心应能看到它。";
  } else if (airplayState === "connecting") {
    airplayTitle.textContent = "正在启动接收端";
    airplaySub.textContent = "首次启动会加载视频组件，通常只要几秒。";
  } else if (airplayState === "error" || engineMissing) {
    airplayTitle.textContent = "接收端还没准备好";
    airplaySub.textContent = "按下面的提示修复，再点「启动接收端」。";
  } else {
    airplayTitle.textContent = "在 iPad 上选择 CastPane";
    airplaySub.textContent = "启动接收端后，iPad 控制中心会出现它。";
  }

  airplayPrimary.replaceChildren();
  airplayPrimary.disabled = false;
  let primaryLabel = "启动接收端";
  if (airplayState === "connecting") {
    primaryLabel = "正在启动…";
    airplayPrimary.disabled = true;
  } else if (busy && (airplayState === "waiting" || airplayState === "streaming")) {
    primaryLabel = "正在停止…";
    airplayPrimary.disabled = true;
  } else if (airplayState === "waiting" || airplayState === "streaming") {
    primaryLabel = "停止接收端";
  } else if (engineMissing) {
    primaryLabel = "重新检查";
  }
  airplayPrimary.textContent = primaryLabel;
  airplayPrimary.dataset.action = primaryLabel;

  const showInstall =
    airplayErrorCode.includes("BONJOUR") ||
    airplayHints.some((check) => check.name.includes("Bonjour") && !check.ok);
  airplayInstall.hidden = !showInstall;

  const errorText = airplayState === "error" ? airplayMessage : "";
  const hintText = airplayState === "error" ? airplayHintText : "";
  const failedCheck = airplayHints.find((check) => !check.ok);
  const detailError =
    !errorText && engineMissing
      ? failedCheck
        ? `${failedCheck.name}：${failedCheck.detail}`
        : "接收端组件不完整，暂时不能启动"
      : "";
  const detailHint =
    failedCheck?.hint ?? "点「检查接收端」查看组件与网络；修好后重试。";
  airplayError.hidden = !(errorText || detailError);
  airplayError.textContent = "";
  if (errorText) {
    airplayError.append(h("b", undefined, errorText));
    if (hintText) airplayError.append(h("span", undefined, hintText));
  } else if (detailError) {
    airplayError.append(
      h("b", undefined, detailError),
      h("span", undefined, detailHint),
    );
  }

  renderAirplayChecks();
}

function openAirplayPage() {
  setPage("airplay");
  updateAirplayPage();
  void runAirplayDiagnose();
}

async function refreshAirplayDevice() {
  if (!airplay) return;
  try {
    const devices = await invoke<DeviceInfo[]>("discover", { id: "airplay" });
    airplayDevice = devices[0] ?? null;
  } catch (err) {
    logLine(`AirPlay 检测失败：${errText(err)}`);
  }
  if (page === "home") renderHome();
}

async function startAirplay() {
  if (!airplay) {
    toast("AirPlay 插件未安装");
    return;
  }
  if (airplayState === "waiting" || airplayState === "streaming") {
    await stopAirplay();
    return;
  }
  if (airplayState === "connecting") return;

  busy = true;
  airplayState = "connecting";
  airplayErrorCode = "";
  airplayMessage = "";
  airplayHintText = "";
  setStatus("正在启动 AirPlay 接收端…", "busy");
  renderPrimary();
  updateAirplayPage();
  try {
    await invoke("connect", {
      id: "airplay",
      deviceId: "receiver",
      settings: airplayValues,
    });
    airplayStarted = true;
    setPage("airplay");
    updateAirplayPage();
    void runAirplayDiagnose();
  } catch (err) {
    busy = false;
    airplayStarted = false;
    airplayState = "error";
    airplayErrorCode = "E_START";
    airplayMessage = `AirPlay 启动失败：${errText(err)}`;
    airplayHintText = "点「检查接收端」看引擎/Bonjour/网络检查项；修好后重试。";
    setStatus(airplayMessage, "error", airplayHintText);
    updateAirplayPage();
    renderPrimary();
    logLine(`${airplayMessage} — ${airplayHintText}`);
  }
}

async function stopAirplay() {
  if (airplayState === "stopped") return;
  busy = true;
  updateAirplayPage();
  try {
    await invoke("stop_cast", { id: "airplay" });
  } catch (err) {
    logLine(`停止 AirPlay 失败：${errText(err)}`);
  }
  airplayState = "stopped";
  airplayStarted = false;
  busy = false;
  airplayMessage = "";
  airplayHintText = "";
  setStatus("已停止 AirPlay 接收端", "");
  updateAirplayPage();
  renderPrimary();
  renderStatus();
}

async function runAirplayDiagnose() {
  airplayChecks.replaceChildren(h("div", "check-line", "正在检查接收端…"));
  try {
    const result = await invoke<{ checks?: Check[] }>("action", {
      id: "airplay",
      name: "diagnose",
      params: {},
    });
    airplayHints = result.checks ?? [];
    const network = airplayHints.find((c) => c.name === "网络");
    if (network?.detail) {
      airplayNetwork = network.detail
        .replace(/^电脑\s*/, "")
        .replace(/\s*·\s*Wi-Fi\s*/, " · ");
    }
    const bonjour = airplayHints.find((c) => c.name.includes("Bonjour"));
    const engine = airplayHints.find((c) => c.name === "引擎");
    const video = airplayHints.find((c) => c.name === "视频组件");
    if (bonjour && !bonjour.ok) airplayErrorCode = "E_BONJOUR_MISSING";
    else if ((engine && !engine.ok) || (video && !video.ok)) {
      airplayErrorCode = "E_ENGINE_MISSING";
    } else if (!airplayErrorCode.startsWith("E_START")) {
      airplayErrorCode = "";
    }
    if (page === "airplay") updateAirplayPage();
  } catch (err) {
    airplayChecks.replaceChildren(
      h("div", "check-line bad", `检查失败：${errText(err)}`),
    );
  }
}

async function installBonjour() {
  if (!airplay) return;
  airplayInstall.disabled = true;
  airplayInstall.textContent = "正在安装…";
  try {
    const result = await invoke<{ detail?: string }>("action", {
      id: "airplay",
      name: "install_bonjour",
      params: {},
    });
    toast(result.detail || "Bonjour 组件已安装");
    logLine(`Bonjour：${result.detail ?? "已安装"}`);
    await refreshAirplayDevice();
    await runAirplayDiagnose();
  } catch (err) {
    toast(`安装失败：${errText(err)}`);
    airplayError.hidden = false;
    airplayError.textContent = `安装失败：${errText(err)}`;
  } finally {
    airplayInstall.disabled = false;
    airplayInstall.textContent = "安装接收组件";
  }
}

/* ---------- files page ---------- */

function formatBytes(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let size = value;
  let index = 0;
  while (size >= 1024 && index < units.length - 1) {
    size /= 1024;
    index += 1;
  }
  return `${size >= 100 || index === 0 ? Math.round(size) : size.toFixed(1)} ${units[index]}`;
}

function formatSpeed(value: number): string {
  if (!Number.isFinite(value) || value <= 0) return "";
  return `${formatBytes(value)}/s`;
}

function fileIcon(kind: FileEntry["kind"]): string {
  switch (kind) {
    case "folder":
      return "folder";
    case "image":
      return "image";
    case "video":
      return "video";
    case "doc":
      return "doc";
    case "audio":
      return "audio";
    case "archive":
      return "archive";
    default:
      return "file";
  }
}

function fileKindLabel(entry: FileEntry): string {
  if (entry.isDir) return "文件夹";
  switch (entry.kind) {
    case "image":
      return "照片";
    case "video":
      return "视频";
    case "doc":
      return "文档";
    case "audio":
      return "音频";
    case "archive":
      return "压缩包";
    default:
      return "文件";
  }
}

function phonePathSegments(path: string): Array<{ label: string; path: string }> {
  let rest = path;
  if (rest.startsWith("/storage/emulated/0")) {
    rest = rest.slice("/storage/emulated/0".length);
  } else if (rest.startsWith("/sdcard")) {
    rest = rest.slice("/sdcard".length);
  }
  const segments: Array<{ label: string; path: string }> = [
    { label: "手机", path: "/sdcard" },
  ];
  const parts = rest.split("/").filter(Boolean);
  let current = "/sdcard";
  for (const part of parts) {
    current = `${current}/${part}`;
    segments.push({ label: part, path: current });
  }
  return segments;
}

function renderBreadcrumb() {
  filesPathEl.replaceChildren();
  const segments = phonePathSegments(filesPath);
  segments.forEach((segment, index) => {
    if (index > 0) {
      const sep = h("span", "crumb-sep", "/");
      filesPathEl.append(sep);
    }
    const button = h(
      "button",
      `crumb${index === segments.length - 1 ? " current" : ""}`,
      segment.label,
    );
    button.type = "button";
    button.dataset.path = segment.path;
    button.addEventListener("click", () => void loadFiles(segment.path));
    filesPathEl.append(button);
  });
}

function renderTransferCard() {
  if (!transfer) {
    transferCard.hidden = true;
    transferProbe.textContent = "transfer=idle";
    return;
  }
  transferCard.hidden = false;
  transferDirection.textContent = transfer.direction === "pull" ? "接收" : "发送";
  transferDirection.className = `transfer-tag ${transfer.direction}`;
  transferName.textContent = transfer.name;
  const percent = transfer.state === "done" ? 100 : Math.max(0, Math.min(100, transfer.percent || 0));
  transferPercent.textContent = transfer.state === "running" && transfer.total === 0
    ? "传输中"
    : `${Math.round(percent)}%`;
  transferBar.style.width = `${percent}%`;
  transferBar.classList.toggle("indeterminate", transfer.state === "running" && transfer.total === 0);
  transferBytes.textContent =
    transfer.total > 0
      ? `${formatBytes(transfer.bytes)} / ${formatBytes(transfer.total)}`
      : `${formatBytes(transfer.bytes)}`;
  transferSpeed.textContent =
    transfer.state === "running" ? formatSpeed(transfer.speed) : "";

  transferResult.hidden = true;
  transferResult.textContent = "";
  transferActions.replaceChildren();

  if (transfer.state === "done") {
    transferResult.hidden = false;
    if (transfer.direction === "pull") {
      transferResult.textContent = `已保存到 ${transfer.localPath ?? "下载目录"}`;
      if (transfer.localPath) {
        const open = h("button", "ghost-button", "打开文件夹");
        open.type = "button";
        open.addEventListener("click", () => {
          void invoke("action", {
            id: "android",
            name: "files_open",
            params: { path: transfer?.localPath },
          }).catch((err) => toast(`打开失败：${errText(err)}`));
        });
        transferActions.append(open);
      }
    } else {
      transferResult.textContent = `已发送到 ${transfer.remotePath ?? "手机目录"}`;
    }
    const close = h("button", "ghost-button", "关闭");
    close.type = "button";
    close.addEventListener("click", () => {
      transfer = null;
      renderTransferCard();
    });
    transferActions.append(close);
  } else if (transfer.state === "error") {
    transferResult.hidden = false;
    transferResult.textContent = `${transfer.error ?? "传输失败"}${transfer.hint ? ` · ${transfer.hint}` : ""}`;
    if (lastTransfer) {
      const retry = h("button", "ghost-button", "重试");
      retry.type = "button";
      retry.addEventListener("click", () => retryLastTransfer());
      transferActions.append(retry);
    }
    const close = h("button", "ghost-button", "关闭");
    close.type = "button";
    close.addEventListener("click", () => {
      transfer = null;
      renderTransferCard();
    });
    transferActions.append(close);
  } else {
    transferResult.hidden = false;
    transferResult.textContent =
      transfer.direction === "pull" ? "正在从手机接收…" : "正在发送到手机…";
  }

  transferProbe.textContent = `transfer=${transfer.id}:${transfer.state}:${Math.round(percent)}`;
}

function renderTransferHistory() {
  const recent = transferHistoryItems.slice(0, 4);
  transferHistory.hidden = recent.length === 0;
  transferHistory.replaceChildren();
  if (recent.length === 0) return;
  transferHistory.append(h("div", "history-title", "最近传输"));
  for (const item of recent) {
    const row = h("div", `history-row ${item.state}`);
    row.append(
      h("span", "history-name", item.name),
      h(
        "span",
        "history-meta",
        item.state === "done"
          ? `${item.direction === "pull" ? "已接收" : "已发送"} · ${formatBytes(item.total || item.bytes)}`
          : `失败 · ${item.error ?? ""}`,
      ),
    );
    transferHistory.append(row);
  }
}

function renderFilesPage() {
  renderBreadcrumb();
  filesDeviceEl.textContent = filesDeviceName || "已连接手机";
  filesNote.textContent = filesHome
    ? `下载保存到 ${filesHome.downloadDir}；发送到 ${filesPath}。`
    : "手机路径 /sdcard；电脑下载目录 Downloads\\CastPane。";
  filesSend.disabled = filesLoading || transfer?.state === "running";
  filesDownload.disabled =
    filesLoading || filesSelected.size === 0 || transfer?.state === "running";
  filesDropHint.hidden = true;

  if (filesLoading) {
    filesList.replaceChildren(
      h(
        "div",
        "files-loading",
        "正在读取手机目录…",
      ),
    );
    const spinner = h("span", "spinner");
    filesList.firstElementChild?.prepend(spinner);
    return;
  }

  if (filesError) {
    const box = h("div", "files-error");
    box.id = "files-error";
    box.append(
      h("b", undefined, filesError.message),
      h("span", undefined, filesError.hint ?? "点右上角刷新重试。"),
    );
    const retry = h("button", "ghost-button", "重试");
    retry.type = "button";
    retry.addEventListener("click", () => void loadFiles(filesPath));
    box.append(retry);
    filesList.replaceChildren(box);
    return;
  }

  const listing = filesListing;
  if (!listing) {
    filesList.replaceChildren(h("div", "files-loading", "还没有读取目录。"));
    return;
  }

  filesList.replaceChildren();
  if (listing.parent) {
    const up = h("button", "file-row up");
    up.type = "button";
    up.dataset.path = listing.parent;
    up.append(
      h("span", "file-icon kind-folder"),
      (() => {
        const info = h("span", "file-info");
        info.append(h("span", "file-name", ".."), h("span", "file-meta", "返回上一级"));
        return info;
      })(),
      h("span", "file-size", ""),
    );
    up.addEventListener("click", () => void loadFiles(listing.parent!));
    filesList.append(up);
  }

  if (listing.entries.length === 0) {
    const empty = h("div", "files-empty");
    empty.append(h("b", undefined, "这个文件夹是空的"), h("span", undefined, "可以点「发送到手机」放点东西进来。"));
    filesList.append(empty);
  }

  for (const entry of listing.entries) {
    const row = h("button", `file-row${entry.isDir ? " dir" : ""}`);
    row.type = "button";
    row.dataset.path = entry.path;
    row.dataset.dir = entry.isDir ? "1" : "0";
    row.setAttribute("title", entry.path);

    const iconBox = h("span", `file-icon kind-${entry.kind}`);
    iconBox.append(icon(fileIcon(entry.kind) as keyof typeof ICONS));

    const info = h("span", "file-info");
    info.append(
      h("span", "file-name", entry.name),
      h("span", "file-meta", `${fileKindLabel(entry)} · ${entry.modified || "时间未知"}`),
    );

    const size = h(
      "span",
      "file-size",
      entry.isDir ? "文件夹" : formatBytes(entry.size),
    );

    const check = h("span", `file-check${filesSelected.has(entry.path) ? " on" : ""}`);
    row.append(iconBox, info, size, check);
    row.addEventListener("click", () => {
      if (entry.isDir) {
        void loadFiles(entry.path);
        return;
      }
      if (filesSelected.has(entry.path)) filesSelected.delete(entry.path);
      else filesSelected.add(entry.path);
      renderFilesPage();
    });
    filesList.append(row);
  }

  if (transfer?.state === "running") {
    filesSend.disabled = true;
    filesDownload.disabled = true;
  }
}

async function loadFiles(path: string) {
  if (!filesDeviceId) {
    filesError = { message: "还没有连接手机", hint: "回首页连接手机后再进文件互传。" };
    renderFilesPage();
    return;
  }
  filesLoading = true;
  filesError = null;
  filesSelected.clear();
  filesPath = path;
  renderFilesPage();
  try {
    const listing = await invoke<DirListing>("action", {
      id: "android",
      name: "files_list",
      params: { deviceId: filesDeviceId, path },
    });
    filesPath = listing.path;
    filesListing = listing;
    filesError = null;
  } catch (err) {
    filesListing = null;
    filesError = {
      message: `读取失败：${errText(err)}`,
      hint: "确认手机仍在线；权限目录请换到 Download、DCIM、Pictures。",
    };
  } finally {
    filesLoading = false;
    renderFilesPage();
  }
}

async function openFilesPage() {
  const device = selectedDevice();
  if (
    !device ||
    device.transport === "airplay" ||
    !(device.state === "ready" || (device.transport === "usb" && device.state === "device"))
  ) {
    toast("先连接手机，再打开文件互传");
    return;
  }
  if (filesDeviceId !== device.id) {
    filesDeviceId = device.id;
    filesDeviceName = device.name;
    filesPath = "/sdcard";
    filesListing = null;
  }
  filesSelected.clear();
  filesError = null;
  setPage("files");
  renderFilesPage();
  if (!filesHome) {
    try {
      filesHome = await invoke<FilesHome>("action", {
        id: "android",
        name: "files_home",
        params: {},
      });
    } catch (err) {
      logLine(`读取文件路径失败：${errText(err)}`);
    }
  }
  await loadFiles(filesPath || "/sdcard");
}

async function beginTransfer(
  kind: "push" | "pull",
  paths: string[],
  target: string,
) {
  if (paths.length === 0) return;
  lastTransfer = { kind, paths, target };
  transfer = {
    id: "pending",
    direction: kind,
    name: paths[0].split(/[\\/]/).pop() ?? "文件",
    state: "running",
    bytes: 0,
    total: 0,
    percent: 0,
    speed: 0,
    remotePath: kind === "push" ? target : null,
    localPath: kind === "pull" ? (filesHome?.downloadDir ?? null) : null,
    filesDone: 0,
    filesTotal: paths.length,
  };
  renderTransferCard();
  try {
    const result = await invoke<{ transferId?: string }>("action", {
      id: "android",
      name: kind === "push" ? "files_push" : "files_pull",
      params:
        kind === "push"
          ? { deviceId: filesDeviceId, localPaths: paths, remoteDir: target }
          : { deviceId: filesDeviceId, paths, localDir: filesHome?.downloadDir },
    });
    if (result?.transferId) {
      const view = await invoke<TransferView | null>("action", {
        id: "android",
        name: "files_transfer",
        params: { transferId: result.transferId },
      });
      if (view) {
        transfer = view;
        renderTransferCard();
      }
    }
  } catch (err) {
    transfer = {
      id: "local-error",
      direction: kind,
      name: paths[0].split(/[\\/]/).pop() ?? "文件",
      state: "error",
      bytes: 0,
      total: 0,
      percent: 0,
      speed: 0,
      error: errText(err),
      hint: "确认手机仍在线；大文件传输失败可以重试。",
      filesDone: 0,
      filesTotal: paths.length,
    };
    renderTransferCard();
  }
}

function retryLastTransfer() {
  if (!lastTransfer) return;
  void beginTransfer(lastTransfer.kind, lastTransfer.paths, lastTransfer.target);
}

async function sendToPhone() {
  if (transfer?.state === "running") {
    toast("等当前传输完成");
    return;
  }
  try {
    const picked = await invoke<string[]>("pick_files");
    if (!picked || picked.length === 0) return;
    const target =
      filesListing?.restricted || filesPath === "/sdcard" || !filesPath
        ? "/sdcard/Download/CastPane"
        : filesPath;
    await beginTransfer("push", picked, target);
  } catch (err) {
    toast(`选择文件失败：${errText(err)}`);
  }
}

async function downloadSelected() {
  if (filesSelected.size === 0) {
    toast("先勾选一个文件");
    return;
  }
  if (transfer?.state === "running") {
    toast("等当前传输完成");
    return;
  }
  await beginTransfer("pull", [...filesSelected], filesPath);
}

async function pushDropped(paths: string[]) {
  if (!filesDeviceId || paths.length === 0) return;
  const target =
    filesListing?.restricted || filesPath === "/sdcard" || !filesPath
      ? "/sdcard/Download/CastPane"
      : filesPath;
  await beginTransfer("push", paths, target);
}



/* ---------- actions ---------- */

async function scan(options: { silent?: boolean } = {}) {
  if (!android || scanning || busy || streaming) return;
  if (!options.silent) holdStatusUntil = 0;
  scanning = true;
  const seq = ++scanSeq;
  if (!options.silent || !snapshot) {
    setStatus("正在检测手机…", "busy");
  }
  renderPrimary();
  try {
    const result = await invoke<WirelessSnapshot>("action", {
      id: "android",
      name: "wireless_scan",
      params: {},
    });
    if (seq !== scanSeq) return;
    snapshot = result;
    for (const line of result.diagnostics ?? []) logLine(line);
    const devices = visibleDevices();
    if (!userPickedDevice || !selectedId || !devices.some((d) => d.id === selectedId)) {
      selectedId = preferredDevice()?.id ?? null;
    }
    renderHome();
  } catch (err) {
    if (seq !== scanSeq) return;
    setStatus(`检测失败：${errText(err)}`, "error");
    toast(`检测失败：${errText(err)}`);
    logLine(`检测失败：${errText(err)}`);
  } finally {
    if (seq === scanSeq) {
      scanning = false;
      renderPrimary();
      renderStatus();
    }
  }
}

async function startCast(device: WirelessDevice): Promise<boolean> {
  holdStatusUntil = 0;
  busy = true;
  setProbe("connecting");
  setStatus("正在启动画面…", "busy");
  renderPrimary();
  try {
    await invoke("connect", {
      id: "android",
      deviceId: device.id,
      settings: androidValues,
    });
    active = {
      pluginId: "android",
      deviceId: device.id,
      name: device.name,
      transport: device.transport,
    };
    setStatus("正在连接手机画面…", "busy");
    renderPrimary();
    window.clearTimeout(startTimeout);
    startTimeout = window.setTimeout(() => {
      if (!streaming && active?.deviceId === device.id) {
        busy = false;
        setStatus("画面启动超时 · 打开「更多 → 诊断」", "error");
        renderPrimary();
      }
    }, 15000);
    return true;
  } catch (err) {
    busy = false;
    active = null;
    setStatus(`投屏失败：${errText(err)}`, "error");
    toast(`投屏失败：${errText(err)}`);
    logLine(`投屏失败：${errText(err)}`);
    renderPrimary();
    return false;
  }
}

async function stopCast() {
  if (!active) return;
  stopping = true;
  busy = true;
  setStatus("正在停止投屏…", "busy");
  renderPrimary();
  try {
    await invoke("stop_cast", { id: active.pluginId });
  } catch (err) {
    logLine(`停止投屏失败：${errText(err)}`);
  }
  active = null;
  streaming = false;
  busy = false;
  setStatus("已停止投屏", "");
  renderPrimary();
  window.setTimeout(() => {
    stopping = false;
    void scan({ silent: true });
  }, 600);
}

async function reconnectAndCast(device: WirelessDevice) {
  holdStatusUntil = 0;
  busy = true;
  setStatus("正在重新连接…", "busy");
  renderPrimary();
  try {
    const result = await invoke<ConnectResult>("action", {
      id: "android",
      name: "wireless_connect",
      params: { deviceId: device.id },
    });
    if (!result.ok) {
      busy = false;
      setStatus(result.status, "error", result.hint ?? result.status);
      toast(result.hint ?? result.status);
      logLine(`${result.status}：${result.hint ?? ""}`);
      renderPrimary();
      void scan({ silent: true });
      return;
    }
    const connected = result.device ?? { ...device, state: "ready" };
    selectedId = connected.id;
    snapshot = snapshot
      ? {
          ...snapshot,
          state: "ready",
          status: `已连接 ${connected.name} · 同一 Wi-Fi`,
          devices: [
            connected,
            ...snapshot.devices.filter((d) => d.id !== connected.id),
          ],
        }
      : snapshot;
    renderHome();
    await startCast(connected);
  } catch (err) {
    busy = false;
    setStatus(`重新连接失败：${errText(err)}`, "error");
    toast(`重新连接失败：${errText(err)}`);
    renderPrimary();
  }
}

function setProgressStep(step: number, state: "idle" | "active" | "done" | "failed") {
  const li = progressSteps.querySelector<HTMLElement>(`li[data-step="${step}"]`);
  if (!li) return;
  li.classList.remove("active", "done", "failed");
  if (state !== "idle") li.classList.add(state);
}

function resetProgress() {
  progressTitle.textContent = "正在开启无线";
  progressMessage.textContent = "用数据线开通一次，之后同一 Wi-Fi 无需插线。";
  progressError.hidden = true;
  progressError.textContent = "";
  progressActions.replaceChildren();
  for (let i = 1; i <= 3; i += 1) setProgressStep(i, "idle");
}

function showProgress() {
  resetProgress();
  progress.hidden = false;
  setProgressStep(1, "active");
}

function hideProgress() {
  progress.hidden = true;
}

function failProgress(message: string) {
  setProbe("error");
  progressTitle.textContent = "没能开启无线";
  progressError.textContent = message;
  progressError.hidden = false;
  progressActions.replaceChildren();
  const retry = h("button", "ghost-button", "重试");
  retry.type = "button";
  retry.addEventListener("click", () => {
    progress.hidden = true;
    void bootstrapAndCast();
  });
  const usb = snapshot?.devices.find(
    (d) => d.transport === "usb" && d.state === "device",
  );
  if (usb) {
    const fallback = h("button", "ghost-button", "直接用 USB 投屏");
    fallback.type = "button";
    fallback.addEventListener("click", () => {
      hideProgress();
      busy = false;
      void startCast(usb);
    });
    progressActions.append(fallback);
  }
  const close = h("button", "ghost-button", "关闭");
  close.type = "button";
  close.addEventListener("click", () => {
    hideProgress();
    busy = false;
    renderPrimary();
  });
  progressActions.append(retry, close);
  busy = false;
  renderPrimary();
}

async function bootstrapAndCast() {
  holdStatusUntil = 0;
  const device = selectedDevice();
  if (!device || device.transport !== "usb") {
    void scan();
    return;
  }
  busy = true;
  setProbe("bootstrapping");
  setStatus("正在开启无线…", "busy");
  renderPrimary();
  showProgress();
  setProgressStep(1, "active");
  const timers = [
    window.setTimeout(() => {
      setProgressStep(1, "done");
      setProgressStep(2, "active");
    }, 650),
    window.setTimeout(() => {
      setProgressStep(2, "done");
      setProgressStep(3, "active");
    }, 2200),
  ];
  try {
    const result = await invoke<ConnectResult>("action", {
      id: "android",
      name: "wireless_usb_bootstrap",
      params: {},
    });
    timers.forEach((timer) => window.clearTimeout(timer));
    if (!result.ok || !result.device) {
      setProgressStep(2, "failed");
      failProgress(result.hint ?? result.status);
      return;
    }
    for (let i = 1; i <= 3; i += 1) setProgressStep(i, "done");
    progressTitle.textContent = "无线已开启";
    progressMessage.textContent = "可以拔掉数据线，正在启动画面…";
    selectedId = result.device.id;
    snapshot = snapshot
      ? {
          ...snapshot,
          state: "ready",
          status: `已连接 ${result.device.name} · 同一 Wi-Fi`,
          devices: [
            result.device,
            ...snapshot.devices.filter((d) => d.id !== result.device!.id),
          ],
        }
      : snapshot;
    logLine(`USB 转无线成功：${result.device.endpoint ?? result.device.id}`);
    renderHome();
    const started = await startCast(result.device);
    if (started) {
      window.setTimeout(hideProgress, 500);
    } else {
      hideProgress();
    }
  } catch (err) {
    timers.forEach((timer) => window.clearTimeout(timer));
    setProgressStep(2, "failed");
    failProgress(`开启无线失败：${errText(err)}`);
  }
}

async function disconnectWireless() {
  const device = selectedDevice();
  if (!device) return;
  if (streaming || active) await stopCast();
  busy = true;
  setStatus("正在断开无线…", "busy");
  renderPrimary();
  try {
    await invoke("action", {
      id: "android",
      name: "wireless_disconnect",
      params: { deviceId: device.endpoint ?? device.id },
    });
    toast("已断开无线；再次连接时若不可达，插一次数据线即可恢复");
  } catch (err) {
    toast(`断开失败：${errText(err)}`);
  }
  busy = false;
  await scan({ silent: true });
}

async function runDiagnose() {
  const box = document.querySelector<HTMLElement>("#checks-box");
  if (!box || !android) return;
  box.replaceChildren(h("div", "check-line", "正在自检…"));
  try {
    const result = await invoke<{ checks?: Check[] }>("action", {
      id: "android",
      name: "diagnose",
      params: {},
    });
    box.replaceChildren();
    for (const check of result.checks ?? []) {
      const line = h(
        "div",
        `check-line ${check.ok ? "ok" : "bad"}`,
        `${check.ok ? "✓" : "✗"} ${check.name}：${check.detail}${
          !check.ok && check.hint ? ` — ${check.hint}` : ""
        }`,
      );
      box.append(line);
    }
    if ((result.checks ?? []).length === 0) {
      box.append(h("div", "check-line", "插件没有返回检查项"));
    }
  } catch (err) {
    box.replaceChildren(h("div", "check-line bad", `自检失败：${errText(err)}`));
  }
}

/* ---------- polling ---------- */

function startPolling() {
  window.setInterval(async () => {
    const ids: string[] = [];
    if (android) ids.push("android");
    if (airplay) ids.push("airplay");
    for (const id of ids) {
      try {
        const messages = await invoke<PluginMessage[]>("poll_events", { id });
        for (const message of messages) handleMessage(id, message);
      } catch {
        /* plugin not running yet */
      }
    }
  }, 900);
}

function handleAirplayState(params: Record<string, unknown>) {
  const state = String(params.state ?? "");
  const reason = params.reason ? String(params.reason) : "";
  const hint = params.hint ? String(params.hint) : "";
  const error = params.error as { code?: string; message?: string } | null | undefined;

  if (state === "connecting") {
    airplayState = "connecting";
    busy = true;
    airplayMessage = reason || "正在启动接收端…";
    airplayHintText = hint;
  } else if (state === "waiting") {
    airplayState = "waiting";
    busy = false;
    airplayStarted = true;
    airplayMessage = reason || "接收端已启动，等待 iPad";
    airplayHintText = hint || "iPad 控制中心 → 屏幕镜像";
  } else if (state === "streaming") {
    airplayState = "streaming";
    busy = false;
    airplayStarted = true;
    airplayMessage = "iPad 已连接 · 正在投屏";
    airplayHintText = "关掉画面窗口只会断开这次连接";
    toast("iPad 已连接，正在投屏");
  } else if (state === "stopped") {
    airplayState = error ? "error" : "stopped";
    busy = false;
    airplayStarted = false;
    airplayMessage = reason || (error ? "AirPlay 接收端异常退出" : "已停止接收端");
    airplayHintText = hint;
    if (error?.code) airplayErrorCode = error.code;
  }

  if (page === "airplay") updateAirplayPage();
  renderStatus();
  renderPrimary();
}

function handleTransfer(view: TransferView) {
  if (!view) return;
  const wasRunning = transfer?.state === "running";
  transfer = view;
  if ((view.state === "done" || view.state === "error") && !historyIds.has(view.id)) {
    historyIds.add(view.id);
    transferHistoryItems.unshift(view);
    transferHistoryItems = transferHistoryItems.slice(0, 6);
  }
  renderTransferCard();
  renderTransferHistory();
  if (view.state === "error" && wasRunning) {
    logLine(`文件传输失败：${view.error ?? ""} ${view.hint ?? ""}`.trim());
  }
}

function handleMessage(pluginId: string, message: PluginMessage) {
  if (message.method === "session.state") {
    if (pluginId === "airplay") {
      handleAirplayState(message.params);
      return;
    }
    const state = String(message.params.state ?? "");
    if (state === "connecting") {
      if (active?.pluginId === "android") {
        busy = true;
        setStatus("正在启动画面…", "busy");
        renderPrimary();
      }
    } else if (state === "streaming") {
      busy = false;
      streaming = true;
      restarting = false;
      window.clearTimeout(startTimeout);
      hideProgress();
      renderStatus();
      renderPrimary();
      renderHome();
      if (active?.pluginId === "android") {
        toast(
          active.transport === "wireless"
            ? "已开始无线投屏，数据线可以拔掉"
            : "已开始 USB 投屏",
        );
      }
    } else if (state === "stopped") {
      if (restarting) return;
      const reason = String(message.params.reason ?? "投屏已结束");
      const hint = message.params.hint ? String(message.params.hint) : "";
      const hadError = Boolean(message.params.error) || reason.includes("掉线");
      busy = false;
      streaming = false;
      window.clearTimeout(startTimeout);
      hideProgress();
      if (active?.pluginId !== "android") {
        if (hadError) logLine(`${reason}${hint ? ` — ${hint}` : ""}`);
        renderPrimary();
        return;
      }
      if (stopping) {
        setStatus("已停止投屏", "");
        setProbe("stopped");
        active = null;
        window.setTimeout(() => {
          stopping = false;
          void scan({ silent: true });
        }, 500);
      } else {
        active = null;
        setStatus(reason, hadError ? "error" : "", hint || reason);
        setProbe(hadError ? "error" : "stopped");
        if (hadError) {
          holdStatusUntil = Date.now() + 8000;
          toast(hint || reason);
        }
        logLine(`${reason}${hint ? ` — ${hint}` : ""}`);
        window.setTimeout(() => void scan({ silent: true }), 600);
      }
      renderPrimary();
    }
  } else if (message.method === "transfer.state") {
    if (pluginId === "android") {
      handleTransfer(message.params as unknown as TransferView);
    }
  } else if (message.method === "log") {
    logLine(String(message.params.message ?? ""));
  } else if (message.method === "device.found") {
    logLine(`发现 ${String(message.params.name ?? "设备")}`);
  }
}

/* ---------- settings / panel ---------- */

async function loadValues(id: string, schema: JsonSchema): Promise<Record<string, unknown>> {
  const values: Record<string, unknown> = {};
  for (const [key, prop] of Object.entries(schema.properties ?? {})) {
    if (prop.default !== undefined) values[key] = prop.default;
  }
  try {
    const stored = await invoke<Record<string, unknown> | null>("load_settings", { id });
    if (stored && typeof stored === "object") Object.assign(values, stored);
  } catch {
    /* preview mode */
  }
  return values;
}

function saveValues(id: string, values: Record<string, unknown>) {
  void invoke("save_settings", { id, values }).catch(() => {
    /* read-only folder: settings last for this session only */
  });
}

function scheduleApply(pluginId: string, values: Record<string, unknown>) {
  const shouldApply =
    (pluginId === "android" && active?.pluginId === "android" && !stopping) ||
    (pluginId === "airplay" && airplayStarted);
  if (!shouldApply) return;
  window.clearTimeout(applyTimer);
  applyTimer = window.setTimeout(async () => {
    restarting = true;
    try {
      const result = await invoke<{ applied?: string }>("apply_settings", {
        id: pluginId,
        settings: values,
      });
      const note = document.querySelector<HTMLElement>("#settings-note");
      if (result?.applied === "restart") {
        if (note) note.textContent = "正在重启画面以应用设置…";
        toast("正在应用设置，画面会短暂重启");
      } else {
        if (note) note.textContent = "设置已应用到当前画面";
        toast("设置已应用");
      }
    } catch (err) {
      restarting = false;
      toast(`应用设置失败：${errText(err)}`);
    }
  }, 300);
}

function renderControl(
  item: PanelItem,
  values: Record<string, unknown>,
  schema: JsonSchema,
  onAction: (action: string) => void,
  onChange: (key: string) => void,
): HTMLElement | null {
  switch (item.type) {
    case "text":
      return h("div", "hint", item.text ?? "");
    case "divider":
      return h("div", "divider");
    case "switch": {
      const row = h("div", "row");
      const input = h("input");
      input.type = "checkbox";
      input.checked = Boolean(values[item.key!] ?? false);
      input.addEventListener("change", () => {
        values[item.key!] = input.checked;
        onChange(item.key!);
      });
      row.append(h("label", undefined, item.label ?? item.key ?? ""), input);
      return row;
    }
    case "slider": {
      const row = h("div", "row");
      const input = h("input");
      input.type = "range";
      input.min = String(item.min ?? 0);
      input.max = String(item.max ?? 100);
      input.step = String(item.step ?? 1);
      input.value = String(values[item.key!] ?? item.min ?? 0);
      const value = h("span", "hint", String(input.value));
      input.addEventListener("input", () => {
        values[item.key!] = Number(input.value);
        value.textContent = input.value;
      });
      input.addEventListener("change", () => {
        values[item.key!] = Number(input.value);
        onChange(item.key!);
      });
      const label = h("label");
      label.append(h("span", undefined, item.label ?? item.key ?? ""), value);
      row.append(label, input);
      return row;
    }
    case "select": {
      const row = h("div", "row");
      const select = h("select");
      const options = schema.properties?.[item.key!]?.enum ?? [];
      for (const opt of options) {
        const o = h("option", undefined, opt);
        o.value = opt;
        if (values[item.key!] === opt) o.selected = true;
        select.append(o);
      }
      select.addEventListener("change", () => {
        values[item.key!] = select.value;
        onChange(item.key!);
      });
      row.append(h("label", undefined, item.label ?? item.key ?? ""), select);
      return row;
    }
    case "button": {
      const row = h("div", "row");
      const button = h("button", "ghost-button", item.label ?? item.action ?? "");
      button.type = "button";
      button.addEventListener("click", () => onAction(item.action ?? ""));
      row.append(button);
      return row;
    }
    default:
      return null;
  }
}

function renderPanel(
  plugin: PluginInfo,
  values: Record<string, unknown>,
  container: HTMLElement,
  onAction: (action: string) => void,
) {
  const onChange = () => {
    saveValues(plugin.id, values);
    scheduleApply(plugin.id, values);
  };
  for (const item of plugin.panel ?? []) {
    const node = renderControl(item, values, plugin.schema, onAction, onChange);
    if (node) container.append(node);
  }
}

/* ---------- more drawer ---------- */

function section(title: string): HTMLElement {
  const box = h("section", "section");
  box.append(h("h3", "section-title", title));
  return box;
}

function renderMoreConnection() {
  const host = document.querySelector<HTMLElement>("#more-connection");
  if (!host) return;
  host.replaceChildren();
  const device = selectedDevice();
  const card = h("div", "conn-card");
  if (device) {
    card.append(h("div", "conn-name", device.name));
    const meta = h("div", "conn-meta");
    meta.textContent =
      device.transport === "usb"
        ? `USB · ${device.detail}`
        : `${device.detail} · ${device.endpoint ?? ""}`;
    card.append(meta);
  } else {
    card.append(h("div", "conn-name", "尚未连接"), h("div", "conn-meta", "首次使用需要一次数据线开通。"));
  }
  host.append(card);

  const row = h("div", "button-row");
  const rescan = h("button", "ghost-button", "重新检测");
  rescan.type = "button";
  rescan.addEventListener("click", () => void scan());
  row.append(rescan);

  if (
    device &&
    device.transport !== "airplay" &&
    (device.state === "ready" || (device.transport === "usb" && device.state === "device"))
  ) {
    const files = h("button", "ghost-button", "文件互传");
    files.type = "button";
    files.addEventListener("click", () => {
      closeMoreDrawer();
      void openFilesPage();
    });
    row.append(files);
  }

  if (device?.state === "offline") {
    const reconnect = h("button", "ghost-button", "重新连接");
    reconnect.type = "button";
    reconnect.addEventListener("click", () => void reconnectAndCast(device));
    row.append(reconnect);
  }

  if (streaming) {
    const stop = h("button", "ghost-button", "停止投屏");
    stop.type = "button";
    stop.addEventListener("click", () => void stopCast());
    row.append(stop);
  }

  if (device?.transport === "wireless" && device.state === "ready" && !streaming) {
    const disconnect = h("button", "danger-button", "断开无线");
    disconnect.type = "button";
    disconnect.addEventListener("click", () => void disconnectWireless());
    row.append(disconnect);
  }

  const usb = snapshot?.devices.find((d) => d.transport === "usb" && d.state === "device");
  if (usb && !streaming) {
    const usbCast = h("button", "ghost-button", "用 USB 直接投屏");
    usbCast.type = "button";
    usbCast.addEventListener("click", () => void startCast(usb));
    row.append(usbCast);
  }
  host.append(row);

  const guide = h("div", "guide-card");
  guide.innerHTML =
    "<b>首次开通（只需一次）</b><ol>" +
    "<li>手机：设置 → 系统和更新 → 开发人员选项 → 打开「USB 调试」。</li>" +
    "<li>建议同时打开「仅充电模式下允许 ADB 调试」。</li>" +
    "<li>用数据线连接电脑，手机上点「允许」，回到首屏点「一键开启无线」。</li>" +
    "<li>看到「已切到无线」后拔线；手机重启后需要再插一次。</li>" +
    "</ol>";
  host.append(guide);

  const note = h("div", "hint");
  note.id = "settings-note";
  note.textContent =
    snapshot?.sameNetwork === false
      ? "手机与电脑不在同一 Wi-Fi，无线连接会失败。"
      : "无线连接走同一 Wi-Fi；USB 仅用于首次开通和兜底。";
  host.append(note);
}

function ensureMoreBuilt() {
  if (moreBuilt || !android) return;
  moreBuilt = true;
  moreBody.replaceChildren();

  const connection = section("连接");
  const connHost = h("div");
  connHost.id = "more-connection";
  connection.append(connHost);
  moreBody.append(connection);

  const settings = section("画面与窗口");
  const settingsHost = h("div");
  renderPanel(android, androidValues, settingsHost, (action) => {
    if (action === "diagnose") void runDiagnose();
    else if (action) {
      void invoke("action", { id: "android", name: action, params: {} }).catch((err) =>
        toast(`操作失败：${errText(err)}`),
      );
    }
  });
  settings.append(settingsHost);
  moreBody.append(settings);

  const other = section("iPad / iPhone 投屏");
  const airplayBox = h("div", "airplay-card");
  airplayBox.append(
    h("div", "airplay-card-title", airplay ? "AirPlay 接收端" : "AirPlay 插件未安装"),
    h(
      "div",
      "hint",
      airplay
        ? "iPad 控制中心 → 屏幕镜像 → 选择接收端。接收端名称与画面设置在这里调整；启动/停止在首屏或投屏页。"
        : "没有找到 AirPlay 插件，检查 plugins/airplay。",
    ),
  );
  if (airplay?.available) {
    const airplayActions = h("div", "button-row");
    const open = h("button", "ghost-button", "打开 iPad 投屏页");
    open.type = "button";
    open.addEventListener("click", () => {
      closeMoreDrawer();
      openAirplayPage();
    });
    airplayActions.append(open);
    airplayBox.append(airplayActions);

    const airplayPanel = h("div");
    renderPanel(airplay, airplayValues, airplayPanel, (action) => {
      if (action === "install_bonjour") {
        void installBonjour();
        return;
      }
      closeMoreDrawer();
      openAirplayPage();
      void runAirplayDiagnose();
    });
    airplayBox.append(airplayPanel);
  }
  other.append(airplayBox);
  moreBody.append(other);

  const diagnostics = section("诊断与日志");
  const diagActions = h("div", "button-row");
  const run = h("button", "ghost-button", "运行自检");
  run.type = "button";
  run.addEventListener("click", () => void runDiagnose());
  const clear = h("button", "ghost-button", "清空日志");
  clear.type = "button";
  clear.addEventListener("click", () => {
    logs.length = 0;
    const box = document.querySelector<HTMLElement>("#log-box");
    if (box) box.replaceChildren();
  });
  diagActions.append(run, clear);
  const checksBox = h("div", "checks");
  checksBox.id = "checks-box";
  const logBox = h("div", "log");
  logBox.id = "log-box";
  for (const line of logs) logBox.append(h("div", "log-line", line));
  diagnostics.append(diagActions, checksBox, logBox);
  moreBody.append(diagnostics);
}

function openMore() {
  if (!android) return;
  ensureMoreBuilt();
  renderMoreConnection();
  moreOpen = true;
  more.classList.add("open");
  more.setAttribute("aria-hidden", "false");
  overlay.hidden = false;
  requestAnimationFrame(() => overlay.classList.add("open"));
  closeMore.focus();
}

function closeMoreDrawer() {
  moreOpen = false;
  more.classList.remove("open");
  more.setAttribute("aria-hidden", "true");
  overlay.classList.remove("open");
  window.setTimeout(() => {
    overlay.hidden = true;
  }, 220);
}

/* ---------- boot ---------- */

async function load() {
  try {
    const plugins = await invoke<PluginInfo[]>("list_plugins");
    android = plugins.find((p) => p.id === "android") ?? null;
    airplay = plugins.find((p) => p.id === "airplay") ?? null;
    let version = "";
    try {
      version = await invoke<string>("app_version");
    } catch {
      /* preview */
    }
    versionEl.textContent = version ? `v${version}` : "";

    if (!android) {
      deviceList.replaceChildren(h("div", "empty-state", "没有找到 Android 插件。"));
      primary.disabled = true;
      setStatus("Android 插件未安装", "error");
      return;
    }

    androidValues = await loadValues(android.id, android.schema);
    if (airplay) airplayValues = await loadValues(airplay.id, airplay.schema);

    if (!android.available) {
      setStatus("Android 插件缺少可执行文件", "error");
      primary.disabled = true;
      return;
    }

    // Discover the AirPlay pseudo-device before the first paint so the row
    // shows the real engine state instead of a placeholder that flips.
    if (airplay?.available) await refreshAirplayDevice();

    renderHome();
    startPolling();
    window.setTimeout(() => void scan(), 120);
    window.setInterval(() => {
      if (page === "home" && !streaming && !busy && !scanning && !moreOpen) {
        void scan({ silent: true });
      }
    }, 8000);
    window.addEventListener("focus", () => {
      if (page === "home" && !streaming && !busy && !scanning) void scan({ silent: true });
    });
  } catch (err) {
    deviceList.replaceChildren(
      h("div", "empty-state", "未连接到 Tauri 后端（浏览器预览模式）"),
    );
    setStatus("预览模式", "");
    console.warn(err);
  }
}

moreToggle.addEventListener("click", () => {
  if (moreOpen) closeMoreDrawer();
  else openMore();
});
closeMore.addEventListener("click", closeMoreDrawer);
overlay.addEventListener("click", closeMoreDrawer);

primary.addEventListener("click", () => {
  if (busy || scanning) return;
  const device = selectedDevice();
  if (device?.transport === "airplay") {
    if (airplayState === "waiting" || airplayState === "streaming") {
      void stopAirplay();
    } else if (airplayState === "connecting") {
      return;
    } else if (device.state !== "ready") {
      openAirplayPage();
      void runAirplayDiagnose();
    } else {
      void startAirplay();
    }
    return;
  }
  if (streaming) {
    void stopCast();
    return;
  }
  if (snapshot?.sameNetwork === false) {
    void scan();
    return;
  }
  if (!device) {
    void scan();
    return;
  }
  if (device.state === "not-found") {
    void scan();
    return;
  }
  if (device.transport === "usb") {
    if (device.state === "unauthorized") void scan();
    else void bootstrapAndCast();
    return;
  }
  if (device.state === "ready") void startCast(device);
  else if (device.state === "offline") void reconnectAndCast(device);
  else void scan();
});

airplayBack.addEventListener("click", () => setPage("home"));
airplayDiagnose.addEventListener("click", () => void runAirplayDiagnose());
airplayInstall.addEventListener("click", () => void installBonjour());
airplayPrimary.addEventListener("click", () => {
  if (airplayState === "waiting" || airplayState === "streaming") {
    void stopAirplay();
  } else if (airplayState === "connecting") {
    return;
  } else if ((airplayDevice?.detail ?? "ready") !== "ready") {
    void runAirplayDiagnose();
  } else {
    void startAirplay();
  }
});
filesBack.addEventListener("click", () => setPage("home"));
filesRefresh.addEventListener("click", () => void loadFiles(filesPath));
document.querySelectorAll<HTMLButtonElement>(".quick-chip").forEach((button) => {
  button.addEventListener("click", () => {
    const target = button.dataset.path;
    if (target) void loadFiles(target);
  });
});
filesSend.addEventListener("click", () => void sendToPhone());
filesDownload.addEventListener("click", () => void downloadSelected());
filesButton.addEventListener("click", () => void openFilesPage());

document.addEventListener("keydown", (event) => {
  if (event.key !== "Escape") return;
  if (!progress.hidden) return;
  if (moreOpen) {
    closeMoreDrawer();
    return;
  }
  if (page !== "home") setPage("home");
});

try {
  void getCurrentWebview().onDragDropEvent((event) => {
    const payload = event.payload;
    if (payload.type === "over" || payload.type === "enter") {
      filesDrop.classList.add("drag");
      return;
    }
    filesDrop.classList.remove("drag");
    if (payload.type === "drop" && page === "files") {
      void pushDropped(payload.paths);
    }
  });
} catch (err) {
  logLine(`拖拽不可用：${errText(err)}`);
}

load();
