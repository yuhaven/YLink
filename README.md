# CastPane

一个极简的 Windows 投屏接收端。iPhone、iPad、安卓手机，一个托盘图标全收。

- **核心不解码视频**：协议与渲染交给插件，核心只做发现、进程守护、窗口摆放、诊断。
- **插件即协议**：加一个新协议 = 在 `plugins/` 下加一个目录，核心与前端零改动。
- **开源免费**：GPL-3.0。

> 状态：v0.1 开发中。目标 14 天出可用版本，见 [落地方案](投屏工具-落地方案-v2.md)。

## 支持的链路

| 链路 | 引擎 | 状态 | 平台 |
|---|---|---|---|
| 安卓 USB / 无线 | [scrcpy](https://github.com/Genymobile/scrcpy) | 计划中 | Win / mac / Linux |
| AirPlay（iPhone / iPad） | [uxplay-windows](https://github.com/leapbtw/uxplay-windows) | 计划中 | Windows |
| Miracast | — | 暂不做 | — |
| DLNA | — | 暂不做 | — |

## 仓库结构

```
crates/core/          # 插件宿主、进程守护、窗口几何（Rust，无 UI）
apps/desktop/         # Tauri 应用：托盘 + 设备列表 + 设置
plugins/android/      # plugin.json + 引擎包装
plugins/airplay/      # plugin.json + 引擎包装
docs/plugin-api.md    # 插件契约（先读这个）
```

## 开发

需要 Rust（MSVC 工具链）、Node 20+、VS Build Tools（C++ 桌面开发）。

```powershell
# 核心单测
cargo test -p castpane-core

# 桌面应用（阶段二接入）
cd apps/desktop
npm install
npm run tauri dev
```

## 设计文档

- [插件协议 v0](docs/plugin-api.md)
- [落地方案 v2](投屏工具-落地方案-v2.md)

## 许可证

GPL-3.0-or-later。内置引擎的许可证：scrcpy 为 Apache-2.0，uxplay 系列为 GPL-3.0。
