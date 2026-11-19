# CastPane v0.3.0 交付说明：iPad / AirPlay 投屏 + 电脑 ↔ 手机文件互传

- 交付日期：2026-11-16 晚
- 应用 / 插件 / 界面版本：**0.3.0**（界面右上角显示 `v0.3.0`）
- 发布包：`dist/CastPane-v0.3.0-win64.zip`（约 123.7 MB；SHA-256 见同目录 `.sha256`）
- 回滚包：`dist/CastPane-v0.2.0-win64.zip`、`dist/CastPane-v0.1.0-win64.zip`（均保留）
- 目标手机：HUAWEI Mate 60（BRA-AL00）/ HarmonyOS 4.2 / EMUI 14.2 / Android 12（API 31）
- 测试网络：`HUAWEI-10EP0W`；电脑 `192.168.101.6`；手机无线 ADB `192.168.101.5:5555`（`device`）

## 1. 一句话结论

**iPad / AirPlay（已改，待真机画面验证）**：接收引擎 uxplay-windows 2.0.0.1736 已真正安装并通过 SHA-256 校验；补齐了发布 zip 缺失的 GStreamer 1.28.1 `gst-plugin-scanner.exe`；Bonjour 服务已安装并运行。真实 UI 点击「启动接收端」后 **3.61 秒**进入等待态，本机 mDNS 广播 `CastPane._airplay._tcp`（端口 52913），点「停止接收端」**0.85 秒**后进程退出、mDNS 记录消失。**本轮网络中没有 iPad/iPhone（ping + ARP + mDNS 均未发现 Apple 设备），所以没有「iPad 上看见画面」的证据，不写成已看见。** 有 iPad 时按第 3 节的 5 步验收即可补上。

**文件互传（已验证）**：用当前无线 ADB 真机完成 10 MB 随机文件「电脑 → 手机 → 电脑」闭环。手机端 `/sdcard/Download/CastPane/cp-e2e-20261116-202625-5483.bin` 与电脑端 `C:\Users\MLTZ\Downloads\CastPane\cp-e2e-20261116-202625-5483.bin` 的 SHA-256 均为 `953D7572D3C1C7141BA20F2CDB477AE5559789B654E889B1CEFA5CAEA8E6CBD2`；发送耗时 **5.85 秒**（含 Windows 原生选择框操作），接收耗时 **2.18 秒**。传输全程安卓无线投屏窗口仍在（2.69 秒进入 streaming；传输后 `hwnd 12848670` 仍可见）。

**安卓无线投屏回归**：原有 `e2e_wireless.ps1 -Scenario real` 仍然 PASS，点击「开始投屏」到 streaming **2.52 秒**，画面窗口 **3.54 秒**出现；`cargo test --workspace` 全部通过（60 个测试，退出码 0）。没有执行 `adb usb`，没有引入「无线调试 / adb pair / 配对码」。

## 2. 现场检查（写需求卡之前做的）

- 电脑 WLAN：`192.168.101.6`，SSID `HUAWEI-10EP0W`，网关 `192.168.101.1`。
- 全网段 ping + ARP + mDNS（`_airplay._tcp` / `_companion-link._tcp` / `_apple-mobdev2._tcp`）：
  - 在线设备：路由器、Mate 60（`192.168.101.5`）、两台海尔家电、一台 E-Life 设备。
  - **没有 Apple 设备**；当前没有 iPad 接入同一 Wi-Fi。
- 手机无线 ADB 始终为 `192.168.101.5:5555 device`；本轮没有执行 `adb usb`，没有断开该连接。
- 需求卡：[iPad / AirPlay](docs/ipad-airplay-requirement-card.md)、[文件互传](docs/file-transfer-requirement-card.md)。
- 竞品拆解：[AirPlay 接收](docs/ipad-airplay-competitor-teardown.md)、[文件互传](docs/file-transfer-competitor-teardown.md)。
- 线框：[v0.3 线框](docs/assets/v03-wireframe.png)（源 [v03-wireframe.html](docs/design/v03-wireframe.html)）。

## 3. iPad / AirPlay：现在能做什么、怎么验收

### 3.1 用户步骤（iPad 侧）

1. 确认 iPad 与电脑连同一个 Wi-Fi（不要用访客网络；AP 隔离会看不到）。
2. 打开 CastPane，首屏设备区点 **iPad / iPhone 投屏** → 点 **启动接收端**。
3. 页面显示「接收端已启动 · 等待 iPad」，并显示接收端名称 `CastPane`、电脑 IP、三步说明。
4. iPad 右上角下滑打开控制中心 → 点 **屏幕镜像** → 在列表中选择 **CastPane**。
5. 独立画面窗口出现；CastPane 页面应变为「iPad 已连接 · 正在投屏」。关窗口只断开本次连接；要点 **停止接收端** 才彻底退出。

首次启动如果 Windows 弹出防火墙提示，请允许 **专用网络**（本轮第一次启动确实弹过，点「允许」后接收端可被局域网访问）。首次使用 AirPlay 还需要 Bonjour 发现服务；缺失时页面会出现 **安装接收组件**，按提示允许一次管理员确认。

### 3.2 本轮已验证（真实 UI + 真实引擎，非 mock）

`powershell -ExecutionPolicy Bypass -File scripts\e2e_v03.ps1 -Scenario airplay` → **PASS**

| 检查 | 结果 |
|---|---|
| 首屏可直接找到 AirPlay 行 | 是；不需要进「更多」 |
| 点「启动接收端」到等待态 | **3.61 秒** |
| mDNS 广播 | `CastPane._airplay._tcp.local.`，端口 `52913`，地址 `192.168.101.6` |
| 引擎 | `uxplay-windows.exe`，uxplay-windows 2.0.0.1736，SHA-256 `9D3A51C1...CCA18` |
| GStreamer scanner | `libexec/gstreamer-1.0/gst-plugin-scanner.exe`，SHA-256 `00F8A2A2...BB2D` |
| Bonjour | `Bonjour Service` 运行中；`--diagnose` 通过 |
| 点「停止接收端」 | **0.85 秒**；进程退出；mDNS 记录消失 |
| 第二次启动（同一进程） | **3.93 秒**到 waiting；**1.01 秒**停止 |
| iPad 画面 / streaming | **未验证**；本轮没有 iPad，不伪造 |
| 截图 | [home-airplay](docs/assets/evidence/home-airplay.png) / [airplay-waiting](docs/assets/evidence/airplay-waiting.png) / [airplay-stopped](docs/assets/evidence/airplay-stopped.png) / [airplay-second-waiting](docs/assets/evidence/airplay-second-waiting.png) |

### 3.3 有 iPad 时的 5 步补充验收

1. 解压 `dist/CastPane-v0.3.0-win64.zip`，运行 `CastPane.exe`，确认右上角 `v0.3.0`。
2. 首屏点 AirPlay 行 → 启动接收端，确认 6 秒内显示「等待 iPad」。
3. iPad 控制中心 → 屏幕镜像，确认列表出现 `CastPane`。
4. 点 `CastPane`，确认独立窗口出现 iPad 画面，CastPane 页面变为「iPad 已连接 · 正在投屏」。
5. 在 CastPane 点「停止接收端」，确认窗口关闭、iPad 控制中心里的 `CastPane` 消失。

若第 3 步看不到 `CastPane`：先检查同一 Wi-Fi / 访客网络，再检查 Windows 防火墙是否允许专用网络，最后点页面「检查接收端」看 Bonjour 与网络检查项；不要先怀疑 iPad。

### 3.4 失败时说人话（已实现）

| 情况 | 页面/诊断给什么 | 下一步 |
|---|---|---|
| 引擎缺失 | 「AirPlay 接收引擎没有安装」+ 路径 | 运行 `scripts/fetch-engines.ps1 -Only airplay` |
| 缺 GStreamer scanner / 视频插件 | 「接收引擎缺少视频组件」 | 重新跑 fetch 脚本补齐；脚本会校验两个 SHA-256 |
| Bonjour 缺失/未运行 | 「首次使用需要 Bonjour 发现服务」+ **安装接收组件** | 允许一次 UAC；或手动装 Apple Bonjour |
| 端口/防火墙 | 等待态 `hint` 会提示「没检测到监听端口：检查端口占用或防火墙」；诊断显示监听端口 | 允许专用网络；关掉占用端口的程序 |
| 接收端没启动 | 进程退出 → 「AirPlay 接收端已退出」+ 诊断入口 | 点启动重试；看诊断的引擎/视频/Bonjour 项 |
| iPad 没选本机 | 文案始终是「等待 iPad」，不写「正在投屏」 | 在控制中心选 `CastPane` |
| 已有另一个 uxplay 在跑 | 「已有另一个 AirPlay 接收端在运行」 | 先在它的托盘退出 |
| 不是同一 Wi-Fi | 页面显示电脑 IP/SSID，并提示同一 Wi-Fi / 访客网络/AP 隔离 | 换到同一家庭 Wi-Fi |

## 4. 文件互传：已验证的双向闭环

### 4.1 入口与操作

- 已连接手机时，首屏状态行右侧出现 **文件**（不新增首屏第二排按钮；首屏可交互元素仍 ≤ 5）。
- 文件页：面包屑、常用文件夹快捷入口（互传文件夹 / 下载 / 照片 / 图片 / 文档）、发送到手机、下载到电脑、文件列表、传输卡片、最近传输。
- 点文件行勾选 → **下载到电脑**；点 **发送到手机** 打开 Windows 原生多选文件框，或把文件/文件夹拖进窗口。
- 下载默认 `%USERPROFILE%\Downloads\CastPane\`；同名文件自动 `name (1).ext`，不覆盖。
- 发送默认当前手机目录；在 `/sdcard` 根目录时发到 `/sdcard/Download/CastPane/`。

### 4.2 最终真机证据

`powershell -ExecutionPolicy Bypass -File scripts\e2e_v03.ps1 -Scenario files` → **PASS**

| 项 | 值 |
|---|---|
| 测试文件 | `cp-e2e-20261116-202625-5483.bin`（10,485,760 字节，随机内容） |
| 电脑 → 手机 | 5.95 秒（点击发送到传输完成，含原生选择框）；手机路径 `/sdcard/Download/CastPane/cp-e2e-20261116-202625-5483.bin` |
| 手机 → 电脑 | 2.18 秒；电脑路径 `C:\Users\MLTZ\Downloads\CastPane\cp-e2e-20261116-202625-5483.bin` |
| SHA-256（两端一致） | `953D7572D3C1C7141BA20F2CDB477AE5559789B654E889B1CEFA5CAEA8E6CBD2` |
| 大小 | 10,485,760 字节（两端一致） |
| 安卓投屏回归 | 点击开始投屏 2.79 秒 streaming；传输后 scrcpy 窗口 `hwnd 12848670` 仍在，`cast_still_alive=true` |
| 第二次打开文件页 | 成功（`second_open=true`），仍能看到刚传的文件 |
| 受限目录异常 | 进入 `/sdcard/Android/data` 显示错误卡片与换目录提示（`permission_error=true`） |
| 截图 | [files-list](docs/assets/evidence/files-list.png) / [files-sent](docs/assets/evidence/files-sent.png) / [files-download-root](docs/assets/evidence/files-download-root.png) / [files-received](docs/assets/evidence/files-received.png) / [files-permission](docs/assets/evidence/files-permission.png) / [home-streaming](docs/assets/evidence/home-streaming.png) |

补充：直接走插件 JSON-RPC（不经 UI）的 10 MB 传输测得 push 1.6 秒、pull 1.1 秒，说明 UI 的 5.95 秒主要花在原生文件选择框与轮询刷新，不是传输慢。交付以 UI 路径的 5.95/2.18 秒为准。

### 4.3 进度与失败

- 进度来自目标文件大小的轮询（adb 只在结束时打印一行），大文件会持续变化；传输卡片显示文件名、方向、百分比、已传/总字节、速度，完成后显示落盘路径并可「打开文件夹」。
- 同一时间只允许一个传输；忙时按钮禁用/返回「已有文件正在传输」。
- 失败原因：离线、权限（`/sdcard/Android/data`）、空间不足、文件不存在、大小校验不一致、30 分钟超时；部分完成会保留文件并给路径。
- 传输使用独立 adb 进程，不碰 scrcpy 会话；L3 已断言传输后投屏窗口仍存在。

## 5. 三层测试结果

### 5.1 L1/L2：`cargo test --workspace`

- 退出码 **0**；共 **60 个测试**通过，0 失败。
  - core：13（settings、窗口几何、IPC、manifest、host roundtrip、real plugin）
  - plugin-airplay：9（引擎路径/名称、窗口、mock 生命周期、诊断）
  - plugin-android：38（adb 解析、无线状态机、scrcpy 参数、文件列表解析、路径限制、重名文件、6 个集成 + 1 个 smoke）
- 新增覆盖：`files::parse_ls`（toybox `ls -la`、中文名、空格、symlink、隐藏文件）、路径规范化/`..` 拒绝、父目录、重名文件、文件类型分类、AirPlay 组件/参数、AirPlay 状态机（mock 引擎只到 waiting，不伪造 streaming）。

### 5.2 L3：真实 UI 点击

| 命令 | 结果 | 关键数值 |
|---|---|---|
| `scripts\e2e_v03.ps1 -Scenario airplay` | PASS | 3.61 s 到 waiting；mDNS `CastPane:52913`；0.85 s 停止；mDNS 消失 |
| `scripts\e2e_v03.ps1 -Scenario files` | PASS | 10 MB push 5.95 s / pull 2.18 s；两端 SHA-256 一致；投屏窗口存活 |
| `scripts\e2e_wireless.ps1 -Scenario real` | PASS | 2.52 s 到 streaming；3.54 s 画面窗口；原无线路径未回归 |
| `CastPane-cli.exe --diagnose` | 1 个已知问题（音频 API 31） | AirPlay 引擎/视频/Bonjour/网络/接收端全通过 |

测试隔离：每个 E2E 开始前杀掉 castpane / 插件 / scrcpy / uxplay，删除 `settings.json` 与 WebView2 缓存；文件测试使用独立临时文件并在结束后保留最终证据对、清理旧测试文件。

### 5.3 未验证 / 不能宣称

- **iPad 真机画面与「已连接」状态未验证**：本轮无 iPad。接收端广播、端口、进程、停止清理已验证；`streaming` 分支的代码以「可见画面窗口出现」为判据，但没有客户端触发过。交付说明只写「已改，待真机验证」。
- 防火墙首次放行是在手工/测试中点的「允许」；没有做「无管理员权限自动放行」，也没有静默改防火墙规则。
- Bonjour 安装需要一次 UAC；本轮机器已安装并运行，未在干净机器上从零点过「安装接收组件」按钮。
- `scripts/gate.ps1` 仍不存在；本版本没有声称门禁全绿。
- 未做亮度、音频、插件化大改等 backlog；未做红果短剧；未改 DSH 权限/凭据文件。

## 6. 发布包与回滚

- 构建路径：`npm run tauri -- build --no-bundle`（由 `scripts/package.ps1` 调用），随后 `cargo build --release -p castpane-plugin-android` / `-p castpane-plugin-airplay`。
- 发布包：`dist/CastPane-v0.3.0-win64.zip`；SHA-256：`见同目录 CastPane-v0.3.0-win64.zip.sha256`。
- 包内校验：
  - 不含 `settings.json`；不含 `mock*` / `e2e*` / `.pdb` / `.log`（zip 内匹配数 0）。
  - 含 `CastPane.exe`、`CastPane-cli.exe`、两个插件、scrcpy、uxplay、`gst-plugin-scanner.exe`。
  - 默认配置：`lockPosition=false`、`stayAwake=true`（来自 `plugins/android/plugin.json`）。
  - 界面显示 `v0.3.0`（[home-airplay](docs/assets/evidence/home-airplay.png)）。
- 回滚：解压 `dist/CastPane-v0.2.0-win64.zip` 覆盖当前目录即可回到 0.2.0；`dist/CastPane-v0.1.0-win64.zip` 保留更早回滚。设置文件不在 zip 内，不会把 0.3 的状态带回去。
- 注意：AirPlay 首次安装的 Bonjour 是系统服务，回滚应用不会自动卸载它；这是 Windows 上 uxplay 的依赖，不属于应用配置。

## 7. 能做 / 不能做 / 已知问题

### 能做

- 同一 Wi-Fi 华为 / 安卓无线投屏；USB 一次 `tcpip 5555` 后免插线；原有二次重连、掉线提示、诊断保留。
- iPad / iPhone AirPlay 接收端：首屏可见、启动/等待/停止、mDNS 广播、独立画面窗口、原生标题栏、Bonjour 安装入口、失败人话。
- 已连接手机（无线/USB）文件浏览、下载到电脑、发送文件/文件夹到手机、进度/速度/完成路径/失败原因、常用目录、拖拽发送。
- 投屏会话与文件传输互不影响（L3 已断言）。

### 不能做 / 本轮没做

- 没有 iPad 真机画面证据；不能写「iPad 上已经看见」。
- 不做 AirPlay DRM 视频、AirPlay 2 多房间、Apple TV 遥控。
- 不做 Miracast / DLNA。
- 不做 Android 11+「无线调试 / adb pair / 配对码」；目标鸿蒙没有该入口。
- 文件互传不做断点续传；不绕过 `/sdcard/Android/data` 权限；不做云盘/公网。
- 不自动安装 Bonjour / 自动改防火墙（需要用户点一次 UAC/允许）。
- 不做亮度、音频、插件化大改等 backlog；`gate.ps1` 未实现。

### 已知问题

- AirPlay 首次启动会加载约 255 MB 引擎，等待态约 3.4–3.6 秒；首次防火墙提示会再慢一点。
- uxplay 发布 zip 缺 GStreamer scanner，已由 fetch 脚本补齐；如果手动解压引擎而不跑脚本，可能出现「能广播但没画面」，诊断会标红「视频组件」。
- AirPlay 接收端名称默认 `CastPane`；当前没有在界面上做文本输入（面板只有 switch/select/slider/button），要改名需改 `%APPDATA%\leapbtw\uxplay-windows\arguments.txt` 或后续加输入控件。
- AirPlay 端口由 uxplay 动态选择（本轮 52913），不要假定 7000；以 mDNS/诊断显示的端口为准。
- 文件列表 `ls -la` 对极端文件名（换行、前后空格）不保证完整；`/sdcard` 必须用尾斜杠，否则 toybox 会把 symlink 本身列出来（已修复）。
- 传输进度在 10 MB 这种小文件上可能只看到 0→100（UI 900 ms 轮询 + 传输很快）；大文件会持续更新。
- Windows 中文控制台用 `Get-Content` 读 UTF-8 插件 JSON 会显示乱码，这是 PowerShell 5.1 的读取编码问题，不是包内文件损坏；Rust/应用读取正常。

## 8. 需要用户拍板的点（不阻塞本轮）

1. **AirPlay 引擎体积**：便携包从 17 MB 涨到约 124 MB（解压后引擎约 255 MB）。如果更在意体积，可做「首次启动再下载引擎」；但首次使用会多一次下载。
2. **Bonjour 系统服务**：首次安装会在 Windows 注册 `Bonjour Service`（uxplay 自带 mDNSResponder）。如果不想动系统服务，可改成随包启动用户态 mDNS（工作量大、兼容性风险高）。
3. **防火墙策略**：当前是首次弹窗让用户允许。是否要做「一键添加专用网络规则」（需要 UAC）？
4. **文件下载位置**：当前固定 `Downloads\CastPane` + 打开文件夹。是否要每次弹保存框，或允许自定义？
5. **接收端名称**：默认 `CastPane`，界面暂不能改名。是否要加一个文本输入（会新增控件）？
6. **iPad 真机验收时间**：需要 iPad 在同一 Wi-Fi；本轮没有设备，验收清单已写在第 3.3 节。

## 9. DoD 对照（诚实版）

- [x] 需求卡已写，成功标准是可观察量（用户点哪里、几秒内看到什么）。
- [x] 有歧义的原话已给出选项并自选（用户不在场，已授权自行选定；拍板点见第 8 节）。
- [x] 涉及界面：竞品拆解表 + 线框图已写（截图含 Windows 投影设置、手机连接、Android 无线显示、0.2 更多抽屉）。
- [x] 平台能力已确认（uxplay/Bonjour/GStreamer scanner/adb push-pull/GetOpenFileNameW）。
- [x] 首屏可交互元素 ≤ 5（实际 5：更多 / 手机行 / AirPlay 行 / 主按钮 / 文件）。
- [x] 每个控件都有「变更 → 应用到当前会话 → 验收」路径：AirPlay 设置会重启接收端（applied=restart）；文件页按钮均有 E2E 点击。
- [x] 能实时生效的设置没有被做成「下次启动才生效」（Android 亮屏/亮度/窗口；AirPlay 名称/渲染器重启接收端并提示）。
- [x] 必须重启的设置会提示「正在应用」，重启后会话正常（AirPlay page 收到 connecting/waiting）。
- [x] L3 E2E 覆盖真实点击路径：airplay / files / wireless real 全 PASS。
- [x] 测试前已隔离：旧进程 / settings.json / WebView2。
- [x] 发布只走 `npm run tauri -- build --no-bundle`，界面版本号 `v0.3.0`。
- [x] 发布包默认配置已校验（lockPosition=false / stayAwake=true），测试产物未进入 zip。
- [x] 文档与实现同批更新（README / plugin-api / 需求卡 / 竞品拆解 / 交付说明 / 复盘）。
- [x] 已按首次使用 / 二次使用 / 异常路径自测：首次启动 AirPlay、同一进程二次启动/停止（3.93/1.01 秒）、文件页二次打开、/sdcard/Android/data 受限目录错误卡片。引擎缺失/端口失败路径由单元测试与诊断覆盖，本轮没有单独做 L3 点击。
- [x] 交付说明写明「能做什么 / 不能做什么 / 已知问题」。
- [x] 有回滚到上一版路径（0.2.0 / 0.1.0 zip）。
- [ ] **iPad 真机画面**：待验证（本轮无 iPad）。
- [ ] `scripts/gate.ps1`：未实现，未声称门禁全绿。

## 10. 证据索引

- 首屏：`docs/assets/evidence/home-airplay.png`（v0.3.0、手机行、AirPlay 行、主按钮、文件入口）
- AirPlay 等待 / 停止 / 第二次等待：`docs/assets/evidence/airplay-waiting.png`、`airplay-stopped.png`、`airplay-second-waiting.png`
- 文件列表 / 发送 / 下载目录 / 接收完成：`docs/assets/evidence/files-list.png`、`files-sent.png`、`files-download-root.png`、`files-received.png`
- 受限目录错误卡片：`docs/assets/evidence/files-permission.png`
- 投屏与文件共存：`docs/assets/evidence/home-streaming.png`
- 机器可读结果：`docs/assets/evidence/v03-result-airplay.json`、`docs/assets/evidence/v03-result-files.json`、`docs/assets/evidence/v03-verification-results.json`
- 无线回归：`build/e2e-wireless/`（`e2e_wireless.ps1 -Scenario real` 输出）
- 全量测试日志：`%TEMP%\cargo-test-v03.log`（本机；包内不包含）
