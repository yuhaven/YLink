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
# 核心单测（9 个，含一次真实进程的 JSON-RPC 往返）
cargo test -p castpane-core

# 桌面应用
cd apps/desktop
npm install
npm run tauri dev
```

国内网络提示：`crates.io` 建议走镜像（`~/.cargo/config.toml` 里配 `sparse+https://mirrors.tuna.tsinghua.edu.cn/crates.io-index/`），否则首次拉 400+ crate 会很慢。

## 获取引擎

```powershell
powershell -ExecutionPolicy Bypass -File scripts\fetch-engines.ps1 -Only android
```

脚本按**固定版本 + SHA-256** 下载 scrcpy v5.0.1 与 uxplay-windows 2.0.0.1736，解压到 `plugins/*/bin/`（该目录不入库）。

如果 GitHub 直连被阻断（部分网络下 `github.com` release 下载会失败，`raw.githubusercontent.com` 正常），手动下载同名 zip 后解压到对应目录即可：

- scrcpy → `plugins/android/bin/scrcpy/`（需要 `scrcpy.exe`、`adb.exe`、`scrcpy-server`）
- uxplay-windows → `plugins/airplay/bin/uxplay/`

## 设计文档

- [插件协议 v0](docs/plugin-api.md)
- [落地方案 v2](投屏工具-落地方案-v2.md)

## 许可证

GPL-3.0-or-later。内置引擎的许可证：scrcpy 为 Apache-2.0，uxplay 系列为 GPL-3.0。
