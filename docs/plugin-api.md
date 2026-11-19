# 插件协议 v0

> 状态：草案，随 v0.1 冻结。核心只实现本文件描述的内容。

## 1. 三条原则

1. **核心不解码任何一帧视频。** 视频由插件自己的窗口渲染，核心只负责把窗口摆好。
2. **一个新协议 = 一个新目录。** 增加 Miracast、DLNA 不允许改动 `crates/core` 和前端。
3. **UI 是声明式的。** 插件不提供界面代码，只提供 `panel` 描述，由宿主通用渲染。

## 2. 目录结构

```
plugins/<id>/
  plugin.json          # 契约，见第 3 节
  bin/
    <id>-plugin.exe    # 入口，任意语言
    <engine>.exe       # 插件自带的引擎（scrcpy / uxplay ...）
```

`bin/` 不入库（见 `.gitignore`），由 `scripts/fetch-plugins.ps1` 拉取固定版本。

## 3. plugin.json

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `id` | string | ✅ | 全局唯一，小写字母数字，如 `airplay` |
| `name` | string | ✅ | 展示名 |
| `version` | string | ✅ | 插件自身版本 |
| `api` | number | | 契约版本，缺省 `0`；大于核心支持版本时拒绝加载 |
| `description` | string | | 一句话说明 |
| `platforms` | string[] | | `windows` / `macos` / `linux`；空表示全平台 |
| `entry` | string | ✅ | 可执行文件相对路径 |
| `capabilities` | string[] | | `discover` / `session` / `window` / `diagnose` |
| `settings` | object | | 设置项的 JSON Schema，宿主据此校验与存储 |
| `panel` | array | | 声明式控件列表，见第 4 节 |

## 4. panel 控件

| type | 字段 | 渲染 |
|---|---|---|
| `switch` | `key`, `label` | 开关 |
| `select` | `key`, `label` | 下拉，选项取自 `settings` 对应属性的 `enum` |
| `slider` | `key`, `label`, `min`, `max`, `step` | 滑块 |
| `button` | `action`, `label` | 按钮，触发 `action.<name>` 调用 |
| `text` | `text` | 说明文字 |
| `divider` | — | 分隔线 |

宿主遇到不认识的 `type` 时跳过并在诊断面板记一条警告，保证旧宿主能加载新插件。

## 5. 传输层

- stdio，**一行一个 JSON 对象**，UTF-8，无长度头
- 遵循 JSON-RPC 2.0；唯一偏离：`error.code` 允许是字符串（如 `"E_FIREWALL"`）
- 请求带 `id`（自增整数），响应必须回同一个 `id`
- 插件主动推送的消息不带 `id`，只有 `method` 与 `params`

## 6. 生命周期

```
core                          plugin
 │  spawn                       │
 │ ── initialize {api} ───────► │  校验版本，返回能力
 │ ◄──────── result {ok, ...} ──│
 │ ── discover.start ─────────► │
 │ ◄── device.found (通知) ─────│  可多次
 │ ── session.start {deviceId} ►│
 │ ◄── session.state (通知) ────│  connecting → streaming
 │ ◄── window.report {hwnd} ────│  核心据此摆窗口
 │ ── session.stop ────────────►│
 │ ── shutdown ────────────────►│  插件应自行退出
```

- `initialize` 失败 = 插件不可用，核心不会重试
- 进程意外退出时核心最多重启 3 次（指数退避），超过则标记为「不可用」并在 UI 提示
- 核心退出时先发 `shutdown`，2 秒后强杀

## 7. 方法

### core → plugin

| 方法 | params | result |
|---|---|---|
| `initialize` | `{api: number}` | `{name, version, capabilities[]}` |
| `discover.start` | `{}` | `{ok: true}` |
| `discover.stop` | `{}` | `{ok: true}` |
| `session.start` | `{deviceId, settings}` | `{sessionId}` |
| `session.stop` | `{sessionId}` | `{ok: true}` |
| `settings.apply` | `{settings}` | `{ok: true}` |
| `diagnose` | `{}` | `{checks: [{name, ok, detail, hint?}]}` |
| `action.<name>` | 任意对象，原样透传 | 任意 |
| `shutdown` | `{}` | `{ok: true}` |

### 7.1 Android 无线动作（v0.2）

Android 插件额外实现以下 `action.*`。宿主用 `action(id, name, params)` 透传，不解释参数：

| action | params | result |
|---|---|---|
| `wireless_scan` | `{}` | `WirelessSnapshot`（见 7.2） |
| `wireless_connect` | `{deviceId}` | `ConnectResult`（连接已记住的 `ip:5555`） |
| `wireless_usb_bootstrap` | `{}` | `ConnectResult`（USB 上执行 `tcpip 5555` + `connect`） |
| `wireless_disconnect` | `{deviceId}` | `{ok: boolean}` |

`ConnectResult = {ok, device?, status, hint?, code?, raw?}`。失败时 `code` 取值包括 `E_NO_USB`、`E_PERMISSION`、`E_NO_WIFI`、`E_NETWORK`、`E_TCPIP`、`E_CONNECT`、`E_USB_BOOTSTRAP`。

### 7.2 `WirelessSnapshot`

```json
{
  "state": "ready | usb | unauthorized | offline | not-found | error",
  "status": "已连接 BRA AL00 · 同一 Wi-Fi",
  "hint": "可以拔掉数据线，点「开始投屏」",
  "devices": [
    {
      "id": "192.168.101.5:5555",
      "name": "BRA AL00",
      "transport": "wireless | usb",
      "state": "ready | device | offline | unauthorized | discovered",
      "detail": "已连接 · 同一 Wi-Fi",
      "endpoint": "192.168.101.5:5555",
      "lastSeen": 1794765601
    }
  ],
  "localIp": "192.168.101.6",
  "phoneIp": "192.168.101.5",
  "sameNetwork": true,
  "known": 1,
  "diagnostics": ["电脑地址：192.168.101.6", "adb devices：1 台在线"]
}
```

- `state=usb` 表示检测到 USB 手机，主按钮应调用 `wireless_usb_bootstrap`。
- `state=offline` 表示记得上次 `ip:5555` 但当前不可达；提示用户手机重启/换网后需要重新插线。
- `sameNetwork=false` 时不应尝试 `tcpip`，直接提示连接同一 Wi-Fi。
- 已记住的端点存在 `%LOCALAPPDATA%\CastPane\android-wireless.json`；可用 `CASTPANE_STATE_DIR` 覆盖（测试隔离）。

### plugin → core（通知）

| 方法 | params | 说明 |
|---|---|---|
| `device.found` | `{id, name, kind, detail?, transport?, endpoint?, label?}` | `kind`: `android` / `ios` / `other`；Android 额外带无线状态 |
| `device.lost` | `{id}` | |
| `session.state` | `{sessionId, state, error?, reason?, hint?, ports?}` | `state`: `connecting`/`waiting`/`streaming`/`stopped`/`error`；`reason` 是给用户的一行原因 |
| `window.report` | `{sessionId, handle, width, height}` | `handle` 为原生窗口句柄（Windows 为 HWND 十进制） |
| `transfer.state` | `TransferView` | Android 文件传输进度；见 7.5 |
| `log` | `{level, message}` | 写入日志面板 |

### 7.3 无扫描类协议的约定

AirPlay、Miracast 接收端不存在"扫描发送端"的概念。约定：`discover.start` 上报**本机接收端**作为一个伪设备：

```json
{"id": "receiver", "name": "iPad / iPhone 投屏", "kind": "ios", "detail": "ready", "transport": "airplay"}
```

`detail` 的取值宿主需要理解：

| detail | 含义 | 宿主行为 |
|---|---|---|
| `device` | adb 设备在线 | 允许投屏 |
| `ready` | 接收端可开会话 | 允许投屏 |
| `unauthorized` / `offline` | 设备侧不可用 | 禁用投屏 |
| `engine-missing` | 引擎缺失 | 禁用投屏并提示安装 |
| `engine-incomplete` | 缺 GStreamer scanner/视频插件 | 禁用并提示修复 |
| `bonjour-missing` | Bonjour 发现服务缺失/未运行 | 禁用并提示安装 |

AirPlay 的 `session.state` 额外语义：

| state | 用户可见含义 |
|---|---|
| `connecting` | 正在启动 uxplay / 加载视频组件 |
| `waiting` | 接收端已启动并在 mDNS 广播，等待 iPad 选择本机 |
| `streaming` | 已检测到 uxplay 的可见画面窗口（iPad 已连接） |
| `stopped` | 用户停止，或引擎退出（`error` 字段有原因） |

`waiting` 时 `ports` 可带接收端当前监听的 TCP 端口（uxplay 端口可能因占用而动态选择，不能假定 7000）。

AirPlay 额外动作：

| action | params | result |
|---|---|---|
| `install_bonjour` | `{}` | `{ok, detail}`；运行随包的 `mDNSResponder.exe -install`，需要一次 UAC |

### 7.4 Android 文件互传动作（v0.3）

| action | params | result |
|---|---|---|
| `files_list` | `{deviceId, path}` | `DirListing` |
| `files_pull` | `{deviceId, paths[], localDir?}` | `{transferId}`（开始后台下载） |
| `files_push` | `{deviceId, localPaths[], remoteDir}` | `{transferId}`（开始后台上传） |
| `files_transfer` | `{transferId?}` | `TransferView` / `null` |
| `files_open` | `{path}` | `{ok:true}`（打开电脑文件夹） |
| `files_home` | `{}` | `{downloadDir, phoneRoot, phoneUploadDir}` |

`DirListing`：

```json
{
  "path": "/sdcard/Download",
  "parent": "/sdcard",
  "entries": [
    {"name":"CastPane","path":"/sdcard/Download/CastPane","isDir":true,"size":3440,"modified":"2026-11-16 19:43","kind":"folder"},
    {"name":"demo.mp4","path":"/sdcard/Download/demo.mp4","isDir":false,"size":10485760,"modified":"2026-11-16 19:20","kind":"video"}
  ],
  "restricted": false
}
```

- 只允许 `/sdcard`、`/storage/emulated/0` 及其子目录；拒绝 `..`。
- 列目录时后端自动补尾斜杠（`ls -la /sdcard/`），否则 toybox 会把 `/sdcard` 这个 symlink 本身当成一项返回。
- `kind`: `folder` / `image` / `video` / `doc` / `audio` / `archive` / `file`。
- `/sdcard/Android/data`、`/sdcard/Android/obb` 直接返回 `E_PERMISSION` 与换目录提示。

### 7.5 `TransferView` 与传输进度

```json
{
  "id": "pull-1794829513632",
  "direction": "pull",
  "name": "demo.mp4",
  "state": "running | done | error",
  "bytes": 6619133,
  "total": 10485760,
  "percent": 63.1,
  "speed": 12827410.6,
  "localPath": "C:\\Users\\me\\Downloads\\CastPane\\demo.mp4",
  "remotePath": "/sdcard/Download/CastPane/demo.mp4",
  "error": null,
  "hint": null,
  "startedAt": 1794829513632,
  "finishedAt": null,
  "filesDone": 0,
  "filesTotal": 1
}
```

- `transfer.state` 在传输线程中推送；宿主通过 `poll_events` 转发给 UI。
- 进度来自目标文件大小的轮询，不依赖 adb 的 stdout（adb 只在结束时打印一行）。
- 同名文件自动改名 `name (1).ext`，不覆盖。
- 完成后校验大小；失败保留已传部分，`localPath`/`remotePath` 仍指向它。
- 同一时间只允许一个传输；忙时返回 `E_BUSY`。

## 8. 错误约定
```json
{
  "code": "E_FIREWALL",
  "message": "UDP 7000 被防火墙阻止",
  "hint": "点击「自动放行」添加规则",
  "data": { "ports": ["7000/udp", "7100/tcp"] }
}
```

- `message` 说明**发生了什么**，`hint` 说明**用户该做什么**
- 没有 `hint` 的错误视为程序缺陷，会在诊断面板标红
- 保留码：`E_NO_DEVICE`、`E_PERMISSION`、`E_FIREWALL`、`E_PORT_BUSY`、`E_ENGINE_MISSING`、`E_UNSUPPORTED`
- v0.3 新增：`E_SCANNER_MISSING`、`E_PLUGIN_MISSING`、`E_BONJOUR_MISSING`、`E_BONJOUR_STOPPED`、`E_ALREADY_RUNNING`、`E_BUSY`、`E_PATH`、`E_NOT_FOUND`、`E_NO_SPACE`、`E_VERIFY`、`E_OFFLINE`、`E_TIMEOUT`

## 9. 窗口接管

插件只管渲染，不管位置：

1. 插件创建窗口后发 `window.report`
2. 核心查该设备的窗口配置（9:16 / 置顶 / 无边框）
3. 核心用 `SetWindowPos` / `SetWindowLong` 应用
4. 用户拖动窗口后核心记住几何，下次同设备恢复

`window.report` 之后插件**不应**再自行改变窗口尺寸；需要全屏等行为时通过 `settings.apply` 由核心下发。

### 实现现状（v0.1）

| 插件 | 做法 |
|---|---|
| android | scrcpy 原生支持 `--window-x/y/width/height --window-borderless`，**不需要接管** |
| airplay | uxplay 无窗口参数，插件按子进程 PID 枚举**可见画面窗口**后 `SetWindowTextW` 改名 + `SetWindowPos` 摆位（`crates/plugin-airplay/src/window.rs`）。不改跨进程窗口样式，保留原生标题栏 |

AirPlay 的可见窗口只在客户端连接后才出现，因此插件以「窗口出现」作为 `streaming` 的判据，窗口消失回到 `waiting`。接收端自身是托盘程序，主窗口隐藏，不会被误当成画面窗口。

## 10. 安全模型

插件是本地可执行文件，**安装即信任**。核心不联网校验签名、不做沙箱。理由是个人工具场景下沙箱收益极低、成本极高。

- 插件只能通过 stdio 与核心通信，不提供网络端口
- 核心以子进程方式启动插件，退出时用 Job Object 一并回收
- 插件崩溃不影响核心

## 11. 最小可运行插件（Python，30 行）

```python
import json, sys

def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

for line in sys.stdin:
    req = json.loads(line)
    method, rid = req.get("method"), req.get("id")
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": rid,
              "result": {"name": "Demo", "version": "0.1.0", "capabilities": ["discover"]}})
    elif method == "discover.start":
        send({"jsonrpc": "2.0", "id": rid, "result": {"ok": True}})
        send({"jsonrpc": "2.0", "method": "device.found",
              "params": {"id": "demo-1", "name": "假设备", "kind": "other"}})
    elif method == "shutdown":
        send({"jsonrpc": "2.0", "id": rid, "result": {"ok": True}})
        break
    elif rid is not None:
        send({"jsonrpc": "2.0", "id": rid,
              "error": {"code": "E_UNSUPPORTED", "message": method, "hint": "v0 未实现"}})
```

用这个就能在没有真实引擎的情况下联调核心与 UI。

## 12. 版本策略

- `api` 为整数主版本，只增不改语义
- 新增可选字段算兼容变更，不升 `api`
- 删除字段、改语义 → `api + 1`，核心同时支持相邻两个版本
