# CastPane

一个极简的 Windows 投屏与文件互传工具：华为 / 安卓手机同一 Wi-Fi 无线投屏，iPad / iPhone 走 AirPlay，已连接手机还能双向传文件。

- **无线优先**：首次用数据线开通一次，之后同一 Wi-Fi 免插线。
- **iPad 直接可见**：首屏设备区就有「iPad / iPhone 投屏」，不用去「更多」里翻。
- **文件互传**：已连接手机点「文件」，浏览手机目录、下载到电脑、发送到手机。
- **核心不解码视频**：协议与渲染交给插件，核心只做发现、进程守护、窗口摆放、诊断。
- **插件即协议**：加一个新协议 = 在 `plugins/` 下加一个目录，核心与前端零改动。
- **开源免费**：GPL-3.0。

> 状态：v0.3.0。目标机型 HUAWEI Mate 60（BRA-AL00）/ HarmonyOS 4.2.0；iPad 接收端已做完整闭环，等待真机画面验收。

## v0.3.0 新增

### iPad / iPhone 投屏（AirPlay）

同一 Wi-Fi 下，接收端会以 `CastPane` 名称广播；iPad 控制中心「屏幕镜像」里能选到它。

1. 打开 CastPane，首屏设备区点 **iPad / iPhone 投屏**。
2. 点 **启动接收端**。页面显示「接收端已启动 · 等待 iPad」，并给出接收端名称与电脑 IP。
3. iPad 右上角下滑打开控制中心 → **屏幕镜像** → 选择 **CastPane**。
4. 画面出现在独立窗口；CastPane 页面变为「iPad 已连接 · 正在投屏」。
5. 点 **停止接收端** 彻底关闭；只关画面窗口不会停止接收端，iPad 可以重新连接。

首次启动会加载视频组件，约 3 秒进入等待态。若 Windows 弹出防火墙提示，请允许 **专用网络**；否则 iPad 可能看不到接收端。首次使用 AirPlay 还需要 Bonjour 发现服务，页面上的「安装接收组件」会在缺失时出现，按提示允许一次管理员确认即可。

> 本轮现场网络中没有 iPad/iPhone（ping + ARP + mDNS 均未发现 Apple 设备），所以「iPad 上看到画面」标记为**已改，待真机验证**；接收端启动、mDNS 广播、停止清理已经用真实引擎跑通（见交付说明）。

### 电脑 ↔ 手机文件互传

1. 手机已连接（无线优先）后，首屏状态行右侧出现 **文件**。
2. 点 **文件** 进入文件互传页，默认 `/sdcard`。
3. 点文件夹进入；点文件行勾选，再点 **下载到电脑**，保存到 `%USERPROFILE%\Downloads\CastPane\`。
4. 点 **发送到手机** 选择文件，或把文件/文件夹直接拖进窗口；默认发到当前目录（根目录时发到 `/sdcard/Download/CastPane/`）。
5. 进度、速度、完成路径和失败原因都在页面底部；完成后可 **打开文件夹**。

常用文件夹有快捷入口（互传文件夹 / 下载 / 照片 / 图片 / 文档）。受限目录（如 `/sdcard/Android/data`）会明确提示换目录，不会假装成功。

## 手机怎么用（华为 / 鸿蒙无线投屏）

> 鸿蒙 4.x 的「开发人员选项」里**没有「无线调试」**，这是正常的。不要去找配对码或二维码。

1. 手机「设置 → 系统和更新 → 开发人员选项」。
2. 打开「USB 调试」；建议同时打开「仅充电模式下允许 ADB 调试」。
3. 用数据线连接电脑，手机上点「允许」。
4. 打开 CastPane，首屏会显示这台手机（标记「首次开通」），点 **一键开启无线**。
5. 进度层显示「读取手机 Wi-Fi 地址 → 开启 5555 无线端口 → 连接并启动画面」；成功后状态行显示「正在投屏 · … · 可拔线」。
6. 拔掉数据线，画面继续走 Wi-Fi。下次打开应用会自动重连上次的 `IP:5555`，点一次「开始投屏」即可。

手机重启、切换 Wi-Fi 或路由器重新分配 IP 后，`adb tcpip` 会失效；再插一次数据线，点「一键开启无线」即可恢复。

## 支持的链路

| 链路 | 引擎 | 状态 | 平台 |
|---|---|---|---|
| 安卓同 Wi-Fi（USB 一次开通 `tcpip 5555`） | [scrcpy](https://github.com/Genymobile/scrcpy) v5.0.1 | 无线投屏已实机验证 | Win / mac / Linux |
| 安卓 USB 兜底 | scrcpy v5.0.1 | 已实现 | Win / mac / Linux |
| AirPlay（iPad / iPhone 屏幕镜像） | [uxplay-windows](https://github.com/leapbtw/uxplay-windows) 2.0.0.1736 + GStreamer 1.28.1 scanner | 接收端启动 / mDNS / 停止已验证；iPad 画面待真机 | Windows |
| 电脑 ↔ 手机文件互传 | 复用 adb（scrcpy 自带） | 10 MB 双向真机传输已验证 | Windows |
| Miracast / DLNA | — | 暂不做 | — |

> 本版本**不做** Android 11+ 的「无线调试 / adb pair / 配对码」流程：目标鸿蒙机型没有该入口，而且这条路径会让用户卡在找不到的菜单上。

## 仓库结构

```
crates/core/              # 插件宿主、进程守护、窗口几何（Rust，无 UI）
crates/plugin-android/    # adb 发现、无线投屏、文件互传（files.rs）
crates/plugin-airplay/    # uxplay 接收端、状态机、Bonjour/窗口
apps/desktop/             # Tauri 应用：首屏 + AirPlay 页 + 文件页
plugins/android/          # plugin.json + 引擎包装
plugins/airplay/          # plugin.json + uxplay 引擎
docs/plugin-api.md        # 插件契约（先读这个）
scripts/e2e_wireless.ps1  # 无线投屏 L3 UI E2E
scripts/e2e_v03.ps1       # AirPlay + 文件互传 L3 UI E2E
scripts/mdns_probe.py     # AirPlay mDNS 验收（测试依赖 zeroconf）
```

## 首屏信息架构

首屏只允许三块：**设备列表（含一个主按钮）**、**一行状态**、**一个「更多」入口**。可交互元素 ≤ 5。

- 设备列表最多 2 行：1 台手机 + 1 个 AirPlay 接收端；没有手机时显示一行「安卓手机」占位行。
- 主按钮根据选中项变化：检测手机 / 一键开启无线 / 开始投屏 / 停止投屏 / 重新连接 / 启动接收端 / 停止接收端。
- 已连接手机时，状态行右侧多一个紧凑 **文件** 入口；AirPlay 行不再收在「更多」里。
- 全部画面开关、渲染器、AirPlay 名称设置、日志、诊断都在「更多」里。
- 首次开通没有配对码输入；只有 3 条 USB 步骤和一个进度层。

## 投屏窗口行为

### 安卓

| 行为 | 说明 | 可配置 |
|---|---|---|
| 原生窗口 | 使用 Windows 标准标题栏：可拖动、最小化、最大化、关闭 | — |
| 置顶 | 默认关闭；避免盖住控制台 | — |
| 画面旋转 | 右上角半透明按钮，每次点按画面转 90°，四档循环 | — |
| 三键导航 | 窗口正下方、画面之外的白色工具条 | — |
| 设置实时生效 | 控制台改动立即作用到当前投屏窗口；分辨率/帧率变化会重启画面约 1 秒 | — |
| 位置固定 | 默认关闭；打开后拖动会被拉回 | 面板「固定窗口位置」 |

### AirPlay

- 接收端由 uxplay-windows 在托盘运行；iPad 连接后出现独立画面窗口。
- 窗口为横向 16:10、右侧对齐、原生标题栏；不与安卓窗口共用插件。
- 关掉画面窗口只断开本次连接，接收端继续等待；**停止接收端** 才彻底退出。
- 接收端名称默认 `CastPane`；渲染器/全屏/低延迟在「更多 → iPad / iPhone 投屏」里改，改动会重启接收端（约 1 秒）。

## 验收

```powershell
# AirPlay L3：真实 UI 点击 + 真实 uxplay + mDNS 探测
powershell -ExecutionPolicy Bypass -File scripts\e2e_v03.ps1 -Scenario airplay

# 文件互传 L3：真实无线 ADB，10 MB 双向，校验 SHA-256，且投屏窗口不能掉
powershell -ExecutionPolicy Bypass -File scripts\e2e_v03.ps1 -Scenario files

# 无线投屏 L3：真实 UI 点击
powershell -ExecutionPolicy Bypass -File scripts\e2e_wireless.ps1 -Scenario real

# 没有真机时：mock adb + mock scrcpy 的 UI 点击
powershell -ExecutionPolicy Bypass -File scripts\e2e_wireless.ps1 -Scenario mock-usb
powershell -ExecutionPolicy Bypass -File scripts\e2e_wireless.ps1 -Scenario mock-empty
powershell -ExecutionPolicy Bypass -File scripts\e2e_wireless.ps1 -Scenario mock-network
powershell -ExecutionPolicy Bypass -File scripts\e2e_wireless.ps1 -Scenario mock-reconnect

# 传统窗口行为 E2E（需要真机）
powershell -ExecutionPolicy Bypass -File scripts\e2e_cast.ps1 -Orientation landscape
powershell -ExecutionPolicy Bypass -File scripts\e2e_cast.ps1 -Orientation portrait
```

`e2e_v03.ps1 -Scenario files` 的前置条件：`adb devices` 能看到手机（USB 或 `IP:5555` 的 `device` 状态）；否则脚本直接报 `PRECONDITION FAILED`（退出码 2）。`-Scenario airplay` 不需要 iPad，但会验证真实接收端广播；有 iPad 在场时才能补「画面出现」证据。

`scripts/gate.ps1` 尚未实现；本版本没有声称门禁全绿。

## 运行行为

- 正常启动显示主窗口；`CastPane.exe --silent` 启动时不显示窗口，只留托盘图标
- 关闭窗口 = 隐藏到托盘，进程继续在后台；真正退出走托盘菜单「退出」
- 左键点托盘图标恢复窗口
- 重复启动不会开第二个窗口，只会把已有窗口叫到前台
- `CastPane-cli.exe --diagnose`：**无窗口自检**，终端里直接跑，输出可重定向到文件，退出码 0 表示全部通过

`--diagnose` 示例（v0.3.0）：

```
CastPane diagnose — 2 plugin(s) from ...\plugins

[airplay] AirPlay v0.3.0
  ✓ 引擎: ...\plugins\airplay\bin\uxplay\uxplay-windows.exe
  ✓ 视频组件: ...\libexec\gstreamer-1.0\gst-plugin-scanner.exe
  ✓ 解码与渲染: H.264 / D3D11 / playback 组件齐全
  ✓ Bonjour 发现: 服务运行中，iPad 可以发现在线接收端
  ✓ 网络: 电脑 192.168.101.6 · Wi-Fi HUAWEI-10EP0W
  · 接收端: 当前未启动；点「启动接收端」后在这里看监听端口
  ✓ Windows 防火墙: 若 iPad 看不到接收端，请在防火墙弹窗中允许「专用网络」

[android] Android v0.3.0
  ✓ scrcpy: ...\plugins\android\bin\scrcpy\scrcpy.exe
  ✓ adb: ...\plugins\android\bin\scrcpy\adb.exe
  ✓ devices: 1 台设备
  ✓ wireless: 已连接 BRA AL00 · 同一 Wi-Fi
  ✓ network: 电脑 192.168.101.6 · 手机 192.168.101.5 · 同一网段
  ✗ audio: Android API 31  → 音频转发需要 Android 13(API 33) 及以上
```

## 开发

需要 Rust（MSVC 工具链）、Node 20+、VS Build Tools（C++ 桌面开发）。

```powershell
# 全部测试（核心 / 无线 / AirPlay / 文件解析）
cargo test --workspace

# 无窗口自检
target\debug\castpane.exe --diagnose
```

插件支持用环境变量覆盖引擎路径，方便没有设备时联调：

| 变量 | 作用 |
|---|---|
| `CASTPANE_ADB` | 替换 adb 可执行文件 |
| `CASTPANE_SCRCPY` | 替换 scrcpy 可执行文件 |
| `CASTPANE_AIRPLAY_ENGINE` | 替换 uxplay 可执行文件（测试时跳过组件检查） |
| `CASTPANE_AIRPLAY_ENGINE_DIR` | 替换 uxplay 目录（用于测「引擎缺失」） |
| `CASTPANE_STATE_DIR` | 替换已记住手机的存储目录（测试隔离） |

测试就是靠这些变量把模拟引擎塞给插件的。L3 mock 场景还会用 `MOCK_ADB_STATE_FILE` / `MOCK_USB_PHONE_IP` 模拟「USB 手机 + tcpip」。

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
powershell -ExecutionPolicy Bypass -File scripts\fetch-engines.ps1 -Only airplay
```

脚本按**固定版本 + SHA-256** 下载：

- scrcpy v5.0.1 → `plugins/android/bin/scrcpy/`
- uxplay-windows 2.0.0.1736 → `plugins/airplay/bin/uxplay/`
- GStreamer 1.28.1 `gst-plugin-scanner.exe` → `plugins/airplay/bin/uxplay/libexec/gstreamer-1.0/`

> uxplay 发布 zip 缺少 GStreamer plugin scanner，只有 19 个插件 DLL。没有 scanner 时接收端能广播但无法解码 H.264；脚本会从 MSYS2 拉取固定版本 scanner，并校验包与文件两个 SHA-256。

### 国内网络的两个坑

1. **DNS 污染**：`github.com` 可能被解析到 `20.6.7.17` 这类黑洞 IP，表现为连接超时。脚本用 `--resolve` 固定到可用的边缘 IP（如 `20.27.177.113`）。
2. **证书吊销检查失败**：`curl: (35) CRYPT_E_REVOCATION_OFFLINE`，CRL 服务器不可达导致 TLS 直接失败。脚本加 `--ssl-no-revoke` 跳过。

uxplay 包约 113 MB（解压 255 MB）；下载失败时脚本会换直链/重试，也可手动下载后解压到 `plugins/airplay/bin/uxplay/`，再运行 `-Only airplay` 补 scanner。

## 打包插件

```powershell
powershell -ExecutionPolicy Bypass -File scripts\package-plugin.ps1 -Plugin android -Release
powershell -ExecutionPolicy Bypass -File scripts\package-plugin.ps1 -Plugin airplay -Release
```

## 便携版

```powershell
powershell -ExecutionPolicy Bypass -File scripts\package.ps1
```

产出 `dist/CastPane/`（免安装目录）和 `dist/CastPane-v0.3.0-win64.zip`（约 129 MB，含 AirPlay 引擎）。发布版从 **exe 同目录** 的 `plugins/` 加载插件，所以整个文件夹可以随意搬动。

设置存在 exe 同目录的 `settings.json`；已记住的无线端点存在 `%LOCALAPPDATA%\CastPane\android-wireless.json`；AirPlay 的接收端参数写在 `%APPDATA%\leapbtw\uxplay-windows\arguments.txt`（同时保留 `.castpane.bak`）。发布包不会带 `settings.json`，也不会带测试产物。

回滚：保留上一版 `dist/CastPane-v0.2.0-win64.zip` 和 `dist/CastPane-v0.1.0-win64.zip`，解压即可回到旧版。每个 zip 旁有 `.sha256`。

## 已知限制

- 首次开通必须用一次数据线；应用不能替用户打开 USB 调试。
- 手机重启、切 Wi-Fi、换 IP 后需要再插一次线重新 `tcpip`。
- 华为 / 鸿蒙没有 Android 11 的「无线调试 / adb pair」，本版本不做配对码。
- Android 12（API 31）不支持 scrcpy 音频转发（需要 API 33+），画面可用。
- 不同网段、公共 Wi-Fi、AP 隔离不在本版本自动打洞范围内。
- AirPlay 本轮没有 iPad 真机：接收端启动/mDNS/停止已验证，**iPad 画面与「已连接」状态待真机验证**。
- AirPlay 首次使用需要 Bonjour 服务与一次 Windows 防火墙放行；没有管理员权限时只能按提示手动安装/放行。
- AirPlay 不做 DRM 视频、AirPlay 2 多房间、Apple TV 遥控。
- 文件互传不做断点续传；传输中断会保留已传部分并提示路径。
- `/sdcard/Android/data`、`/sdcard/Android/obb` 等受限目录只提示，不绕过系统权限。
- 文件名含换行等极端字符的目录项不保证完整显示。

## 设计文档

- [v0.3.0 交付说明（iPad 投屏 + 文件互传，含验收步骤与证据）](iPad投屏与文件互传-交付说明.md)
- [iPad / AirPlay 需求卡](docs/ipad-airplay-requirement-card.md)
- [文件互传需求卡](docs/file-transfer-requirement-card.md)
- [AirPlay 竞品拆解](docs/ipad-airplay-competitor-teardown.md)
- [文件互传竞品拆解](docs/file-transfer-competitor-teardown.md)
- [v0.3 线框图](docs/assets/v03-wireframe.png)
- [无线投屏 0.2.0 交付说明（历史）](无线投屏-交付说明.md)
- [插件协议 v0](docs/plugin-api.md)
- [落地方案 v2](投屏工具-落地方案-v2.md)

## 许可证

GPL-3.0-or-later。内置引擎的许可证：scrcpy 为 Apache-2.0，uxplay 系列为 GPL-3.0，GStreamer 为 LGPL-2.1。
