import { invoke } from "@tauri-apps/api/core";
import "./style.css";

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

interface JsonSchema {
  type?: string;
  properties?: Record<
    string,
    { type?: string; default?: unknown; enum?: string[]; title?: string }
  >;
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

interface DeviceInfo {
  id: string;
  name: string;
  detail: string;
}

interface PluginMessage {
  method: string;
  params: Record<string, unknown>;
}

const app = document.querySelector<HTMLElement>("#plugins")!;
const statusEl = document.querySelector<HTMLElement>("#status")!;

function setStatus(text: string) {
  statusEl.textContent = text;
}

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

/** Manifest defaults, overlaid with whatever the user chose last time. */
function loadValues(id: string, schema: JsonSchema): Record<string, unknown> {
  const values: Record<string, unknown> = {};
  for (const [key, prop] of Object.entries(schema.properties ?? {})) {
    if (prop.default !== undefined) values[key] = prop.default;
  }
  try {
    const raw = localStorage.getItem(`castpane.settings.${id}`);
    if (raw) Object.assign(values, JSON.parse(raw));
  } catch {
    /* corrupt storage is not fatal */
  }
  return values;
}

function saveValues(id: string, values: Record<string, unknown>) {
  try {
    localStorage.setItem(`castpane.settings.${id}`, JSON.stringify(values));
  } catch {
    /* storage unavailable: settings last for this session only */
  }
}

/** Render one declarative control. Unknown types are skipped, not fatal. */
function renderControl(
  item: PanelItem,
  values: Record<string, unknown>,
  schema: JsonSchema,
  onAction: (action: string) => void,
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
      input.addEventListener("input", () => {
        values[item.key!] = Number(input.value);
      });
      row.append(h("label", undefined, item.label ?? item.key ?? ""), input);
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
      });
      row.append(h("label", undefined, item.label ?? item.key ?? ""), select);
      return row;
    }
    case "button": {
      const row = h("div", "row");
      const btn = h("button", undefined, item.label ?? item.action ?? "");
      btn.addEventListener("click", () => onAction(item.action ?? ""));
      row.append(btn);
      return row;
    }
    default:
      console.warn("unknown panel item", item);
      return null;
  }
}

/** Poll a plugin forever: session state to the status bar, the rest to the log. */
function startPolling(pluginId: string, report: (line: string) => void) {
  window.setInterval(async () => {
    try {
      const messages = await invoke<PluginMessage[]>("poll_events", { id: pluginId });
      for (const msg of messages) {
        if (msg.method === "session.state") {
          setStatus(`会话：${msg.params.state as string}`);
        } else if (msg.method === "log") {
          report(`· ${msg.params.message as string}`);
        } else if (msg.method === "device.found") {
          report(`+ 发现 ${msg.params.name as string}`);
        }
      }
    } catch {
      /* plugin not running yet */
    }
  }, 1000);
}

function renderDevices(
  plugin: PluginInfo,
  container: HTMLElement,
  values: Record<string, unknown>,
): () => void {
  const scan = h("button", undefined, "扫描设备");
  const list = h("div", "devices");

  async function run() {
    scan.disabled = true;
    scan.textContent = "扫描中…";
    try {
      const devices = await invoke<DeviceInfo[]>("discover", { id: plugin.id });
      list.replaceChildren();
      if (devices.length === 0) {
        list.append(h("div", "hint", "没有发现设备。插好数据线并开启 USB 调试后再试。"));
      }
      for (const device of devices) {
        const row = h("div", "device");
        const info = h("div");
        info.append(
          h("div", "device-name", device.name),
          h("div", "device-meta", `${device.id} · ${device.detail}`),
        );
        const cast = h("button", "primary", "投屏");
        // "ready" is airplay's receiver; "device" is an adb device.
        const blocked = ["unauthorized", "offline", "engine-missing"].includes(device.detail);
        cast.disabled = blocked;
        if (device.detail === "engine-missing") cast.title = "缺少接收端引擎";
        cast.addEventListener("click", async () => {
          cast.disabled = true;
          try {
            await invoke("connect", {
              id: plugin.id,
              deviceId: device.id,
              settings: values,
            });
            setStatus(`${device.name} 正在投屏`);
          } catch (err) {
            cast.disabled = false;
            setStatus(`投屏失败：${err}`);
          }
        });
        row.append(info, cast);
        list.append(row);
      }
    } catch (err) {
      setStatus(`扫描失败：${err}`);
    } finally {
      scan.disabled = false;
      scan.textContent = "扫描设备";
    }
  }

  scan.addEventListener("click", () => void run());
  if (!plugin.available) {
    scan.disabled = true;
    scan.title = `插件未安装：缺少 ${plugin.id} 的可执行文件`;
  }
  container.append(scan, list);
  return () => void run();
}

function renderCard(plugin: PluginInfo) {
  const card = h("div", "card");
  const head = h("div", "card-head");
  head.append(
    h("span", "card-title", plugin.name),
    h("span", "card-version", `v${plugin.version}`),
  );
  if (!plugin.available) head.append(h("span", "badge", "未安装"));

  const log = h("div", "log");
  const report = (line: string) => {
    log.append(h("div", "log-line", line));
    log.scrollTop = log.scrollHeight;
  };

  const body = h("div", "card-body");
  const values = loadValues(plugin.id, plugin.schema);

  const onAction = async (name: string) => {
    if (!name) return;
    try {
      const result = await invoke<{
        checks?: Array<{ name: string; ok: boolean; detail: string; hint?: string }>;
      }>("action", { id: plugin.id, name });
      const checks = result?.checks ?? [];
      if (checks.length === 0) {
        report(`✓ ${name}`);
      }
      for (const check of checks) {
        report(
          `${check.ok ? "✓" : "✗"} ${check.name}：${check.detail}` +
            (check.ok ? "" : ` — ${check.hint ?? "检查失败"}`),
        );
      }
    } catch (err) {
      report(`✗ ${name} 失败：${err}`);
    }
  };

  for (const item of plugin.panel ?? []) {
    const node = renderControl(item, values, plugin.schema, (action) =>
      void onAction(action),
    );
    if (node) body.append(node);
  }
  // Delegated listeners: every control persists without wiring each one.
  body.addEventListener("change", () => saveValues(plugin.id, values));
  body.addEventListener("input", () => saveValues(plugin.id, values));

  const devices = h("div", "card-devices");
  const scan = renderDevices(plugin, devices, values);

  card.append(head, h("div", "card-desc", plugin.description), body, devices, log);
  app.append(card);

  startPolling(plugin.id, report);
  if (plugin.available) window.setTimeout(scan, 400);
}

async function load() {
  try {
    const plugins = await invoke<PluginInfo[]>("list_plugins");
    app.replaceChildren();
    if (plugins.length === 0) {
      app.append(h("p", "empty", "没有找到插件。检查 plugins/ 目录。"));
      return;
    }
    for (const plugin of plugins) renderCard(plugin);
    setStatus(`${plugins.length} 个插件`);
  } catch (err) {
    // Running in a plain browser (vite) without the Tauri backend.
    app.replaceChildren();
    app.append(h("p", "empty", "未连接到 Tauri 后端（浏览器预览模式）"));
    setStatus("预览模式");
    console.warn(err);
  }
}

load();
