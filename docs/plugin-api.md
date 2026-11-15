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
| `action.<name>` | `{}` | 任意 |
| `shutdown` | `{}` | `{ok: true}` |

### plugin → core（通知）

| 方法 | params | 说明 |
|---|---|---|
| `device.found` | `{id, name, kind, detail?}` | `kind`: `android` / `ios` / `other` |
| `device.lost` | `{id}` | |
| `session.state` | `{sessionId, state, error?}` | `state`: `connecting`/`streaming`/`stopped`/`error` |
| `window.report` | `{sessionId, handle, width, height}` | `handle` 为原生窗口句柄（Windows 为 HWND 十进制） |
| `log` | `{level, message}` | 写入日志面板 |

### 7.1 无扫描类协议的约定

AirPlay、Miracast 接收端不存在"扫描发送端"的概念。约定：`discover.start` 上报**本机接收端**作为一个伪设备：

```json
{"id": "receiver", "name": "AirPlay 接收端", "kind": "ios", "detail": "ready"}
```

`detail` 的取值宿主需要理解：

| detail | 含义 | 宿主行为 |
|---|---|---|
| `device` | adb 设备在线 | 允许投屏 |
| `ready` | 接收端可开会话 | 允许投屏 |
| `unauthorized` / `offline` | 设备侧不可用 | 禁用投屏 |
| `engine-missing` | 引擎缺失 | 禁用投屏并提示安装 |

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

## 9. 窗口接管

插件只管渲染，不管位置：

1. 插件创建窗口后发 `window.report`
2. 核心查该设备的窗口配置（9:16 / 置顶 / 无边框）
3. 核心用 `SetWindowPos` / `SetWindowLong` 应用
4. 用户拖动窗口后核心记住几何，下次同设备恢复

`window.report` 之后插件**不应**再自行改变窗口尺寸；需要全屏等行为时通过 `settings.apply` 由核心下发。

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
