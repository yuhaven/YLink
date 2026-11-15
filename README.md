# CastPane

一个极简的 Windows 投屏接收端。iPhone、iPad、安卓手机，一个托盘图标全收。

- **核心不解码视频**：协议与渲染交给插件，核心只做发现、进程守护、窗口摆放、诊断。
- **插件即协议**：加一个新协议 = 在 `plugins/` 下加一个目录，核心与前端零改动。
- **开源免费**：GPL-3.0。

> 状态：v0.1 开发中。目标 14 天出可用版本，见 [落地方案](投屏工具-落地方案-v2.md)。

## 支持的链路

| 链路 | 引擎 | 状态 | 平台 |
|---|---|---|---|
| 安卓 USB / 无线 | [scrcpy](https://github.com/Genymobile/scrcpy) v5.0.1 | 已实现，待引擎到位联调 | Win / mac / Linux |
| AirPlay（iPhone / iPad） | [uxplay-windows](https://github.com/leapbtw/uxplay-windows) | 已实现（接收端 + 窗口接管，待真机验证） | Windows |
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

## 运行行为

- 正常启动显示主窗口；`CastPane.exe --silent` 启动时不显示窗口，只留托盘图标
- 关闭窗口 = 隐藏到托盘，进程继续在后台；真正退出走托盘菜单「退出」
- 左键点托盘图标恢复窗口
- 重复启动不会开第二个窗口，只会把已有窗口叫到前台
- `CastPane.exe --diagnose`：**无窗口自检**，逐个插件跑诊断并打印结果，退出码 0 表示全部通过

`--diagnose` 示例：

```
CastPane diagnose — 2 plugin(s) from ...\plugins

[airplay] AirPlay v0.1.0
  ✗ uxplay: ...\plugins\airplay\bin\uxplay  → 运行 scripts/fetch-engines.ps1 -Only airplay

[android] Android v0.1.0
  ✗ scrcpy: ...\scrcpy.exe  → 运行 scripts/fetch-engines.ps1
  ✗ adb: ...\adb.exe  → adb 随 scrcpy 一起发布

3 个问题
```

## 开发

需要 Rust（MSVC 工具链）、Node 20+、VS Build Tools（C++ 桌面开发）。

```powershell
# 核心单测（9 个，含一次真实进程的 JSON-RPC 往返）
cargo test -p castpane-core

# 全部测试（28 个，含用模拟引擎跑通的投屏会话）
cargo test --workspace

# 无窗口自检
target\debug\castpane.exe --diagnose
```

插件支持用环境变量覆盖引擎路径，方便没有设备时联调：

| 变量 | 作用 |
|---|---|
| `CASTPANE_ADB` | 替换 adb 可执行文件 |
| `CASTPANE_SCRCPY` | 替换 scrcpy 可执行文件 |
| `CASTPANE_AIRPLAY_ENGINE` | 替换 uxplay 可执行文件 |

测试就是靠这三个变量把模拟引擎塞给插件的。

桌面应用：

```powershell
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

### 国内网络的两个坑

1. **DNS 污染**：`github.com` 可能被解析到 `20.6.7.17` 这类黑洞 IP，表现为连接超时。脚本用 `--resolve` 固定到可用的边缘 IP（如 `20.27.177.113`）。
2. **证书吊销检查失败**：`curl: (35) CRYPT_E_REVOCATION_OFFLINE`，CRL 服务器不可达导致 TLS 直接失败。脚本加 `--ssl-no-revoke` 跳过。

还有一个陷阱：`release-assets.githubusercontent.com` 的短时速度可以是 `github.com` 的十倍，但持续大文件下载会被限速到几 KB/s。100 MB 以上的 uxplay 建议手动下载：

- scrcpy → `plugins/android/bin/scrcpy/`（需要 `scrcpy.exe`、`adb.exe`、`scrcpy-server`）
- uxplay-windows → `plugins/airplay/bin/uxplay/`

## 打包插件

插件是可执行文件，需要构建后放到 `plugins/<id>/bin/`：

```powershell
powershell -ExecutionPolicy Bypass -File scripts\package-plugin.ps1 -Plugin android -Release
```

宿主启动时会读取 `plugins/*/plugin.json`，按 manifest 渲染界面；插件进程通过 stdio 上的 JSON-RPC 通信。协议见 [docs/plugin-api.md](docs/plugin-api.md)。

## 便携版

```powershell
powershell -ExecutionPolicy Bypass -File scripts\package.ps1
```

产出 `dist/CastPane/`（免安装目录）和 `dist/CastPane-v0.1.0-win64.zip`。发布版从 **exe 同目录** 的 `plugins/` 加载插件，所以整个文件夹可以随意搬动。

设置也存在 exe 同目录的 `settings.json`，整个文件夹搬到另一台机器后设置仍然有效。

目录结构：

```
CastPane/
  CastPane.exe
  plugins/android/{plugin.json, bin/}
  plugins/airplay/{plugin.json, bin/}
  LICENSE
  README.md
```

## 设计文档

- [插件协议 v0](docs/plugin-api.md)
- [落地方案 v2](投屏工具-落地方案-v2.md)

## 许可证

GPL-3.0-or-later。内置引擎的许可证：scrcpy 为 Apache-2.0，uxplay 系列为 GPL-3.0。
