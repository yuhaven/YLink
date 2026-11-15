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

interface PluginInfo {
  id: string;
  name: string;
  version: string;
  description: string;
  capabilities: string[];
  settings: Record<string, unknown>;
  panel: PanelItem[];
  running: boolean;
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

/** Render one declarative control. Unknown types are skipped, not fatal. */
function renderControl(item: PanelItem, settings: Record<string, unknown>): HTMLElement | null {
  switch (item.type) {
    case "text":
      return h("div", "hint", item.text ?? "");
    case "divider":
      return h("div", "divider");
    case "switch": {
      const row = h("div", "row");
      const input = h("input");
      input.type = "checkbox";
      input.checked = Boolean(settings[item.key!] ?? false);
      input.addEventListener("change", () => {
        settings[item.key!] = input.checked;
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
      input.value = String(settings[item.key!] ?? item.min ?? 0);
      input.addEventListener("input", () => {
        settings[item.key!] = Number(input.value);
      });
      row.append(h("label", undefined, item.label ?? item.key ?? ""), input);
      return row;
    }
    case "select": {
      const row = h("div", "row");
      const select = h("select");
      const schema = (settings as any)?.__schema?.[item.key!];
      const options: string[] = schema?.enum ?? [];
      for (const opt of options) {
        const o = h("option", undefined, opt);
        o.value = opt;
        if (settings[item.key!] === opt) o.selected = true;
        select.append(o);
      }
      select.addEventListener("change", () => {
        settings[item.key!] = select.value;
      });
      row.append(h("label", undefined, item.label ?? item.key ?? ""), select);
      return row;
    }
    case "button": {
      const row = h("div", "row");
      const btn = h("button", undefined, item.label ?? item.action ?? "");
      btn.addEventListener("click", () => setStatus(`触发 ${item.action}（未接线）`));
      row.append(btn);
      return row;
    }
    default:
      console.warn("unknown panel item", item);
      return null;
  }
}

/** Poll session events for a while so the user sees state changes. */
function watch(pluginId: string, ticks = 12) {
  let left = ticks;
  const timer = window.setInterval(async () => {
    left -= 1;
    try {
      const messages = await invoke<PluginMessage[]>("poll_events", { id: pluginId });
      for (const msg of messages) {
        if (msg.method === "session.state") {
          const state = msg.params.state as string;
          setStatus(`会话：${state}`);
        }
        if (msg.method === "log") {
          console.info("[plugin]", msg.params.message);
        }
      }
    } catch {
      /* ignore */
    }
    if (left <= 0) window.clearInterval(timer);
  }, 500);
}

function renderDevices(plugin: PluginInfo, container: HTMLElement) {
  const scan = h("button", undefined, "扫描设备");
  const list = h("div", "devices");

  scan.addEventListener("click", async () => {
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
        cast.disabled = device.detail !== "device";
        cast.addEventListener("click", async () => {
          cast.disabled = true;
          try {
            await invoke("connect", { id: plugin.id, deviceId: device.id });
            setStatus(`${device.name} 正在投屏`);
            watch(plugin.id);
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
  });

  container.append(scan, list);
}

function renderCard(plugin: PluginInfo) {
  const card = h("div", "card");
  const head = h("div", "card-head");
  head.append(
    h("span", "card-title", plugin.name),
    h("span", "card-version", `v${plugin.version}`),
  );

  const body = h("div", "card-body");
  for (const item of plugin.panel ?? []) {
    const node = renderControl(item, plugin.settings);
    if (node) body.append(node);
  }

  const devices = h("div", "card-devices");
  renderDevices(plugin, devices);

  card.append(head, h("div", "card-desc", plugin.description), body, devices);
  app.append(card);
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
