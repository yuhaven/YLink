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
    case "text": {
      const p = h("div", "hint", item.text ?? "");
      return p;
    }
    case "divider":
      return h("div", "divider");
    case "switch": {
      const row = h("div", "row");
      const label = h("label", undefined, item.label ?? item.key ?? "");
      const input = h("input");
      input.type = "checkbox";
      input.checked = Boolean(settings[item.key!] ?? false);
      input.addEventListener("change", () => {
        settings[item.key!] = input.checked;
      });
      row.append(label, input);
      return row;
    }
    case "slider": {
      const row = h("div", "row");
      const label = h("label", undefined, item.label ?? item.key ?? "");
      const input = h("input");
      input.type = "range";
      input.min = String(item.min ?? 0);
      input.max = String(item.max ?? 100);
      input.step = String(item.step ?? 1);
      input.value = String(settings[item.key!] ?? item.min ?? 0);
      input.addEventListener("input", () => {
        settings[item.key!] = Number(input.value);
      });
      row.append(label, input);
      return row;
    }
    case "select": {
      const row = h("div", "row");
      const label = h("label", undefined, item.label ?? item.key ?? "");
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
      row.append(label, select);
      return row;
    }
    case "button": {
      const row = h("div", "row");
      const btn = h("button", undefined, item.label ?? item.action ?? "");
      btn.addEventListener("click", () => {
        setStatus(`触发 ${item.action}（未接线）`);
      });
      row.append(btn);
      return row;
    }
    default:
      console.warn("unknown panel item", item);
      return null;
  }
}

function renderCard(plugin: PluginInfo) {
  const card = h("div", "card");
  const head = h("div", "card-head");
  head.append(
    h("span", "card-title", plugin.name),
    h("span", "card-version", `v${plugin.version}`),
  );

  const desc = h("div", "card-desc", plugin.description);

  const body = h("div", "card-body");
  for (const item of plugin.panel ?? []) {
    const node = renderControl(item, plugin.settings);
    if (node) body.append(node);
  }

  const actions = h("div", "actions");
  const start = h("button", "primary", plugin.running ? "运行中" : "启动");
  start.disabled = plugin.running;
  start.addEventListener("click", async () => {
    start.disabled = true;
    setStatus(`正在启动 ${plugin.name}…`);
    try {
      await invoke("start_plugin", { id: plugin.id });
      start.textContent = "运行中";
      setStatus(`${plugin.name} 已启动`);
    } catch (err) {
      start.disabled = false;
      setStatus(`${plugin.name} 启动失败`);
      console.error(err);
    }
  });
  actions.append(start);

  card.append(head, desc, body, actions);
  app.append(card);
}

async function load() {
  try {
    const plugins = await invoke<PluginInfo[]>("list_plugins");
    app.replaceChildren();
    if (plugins.length === 0) {
      const p = h("p", "empty", "没有找到插件。检查 plugins/ 目录。");
      app.append(p);
      return;
    }
    for (const p of plugins) renderCard(p);
    setStatus(`${plugins.length} 个插件`);
  } catch (err) {
    // Running in a plain browser (vite) without the Tauri backend.
    app.replaceChildren();
    const p = h("p", "empty", "未连接到 Tauri 后端（浏览器预览模式）");
    app.append(p);
    setStatus("预览模式");
    console.warn(err);
  }
}

load();
