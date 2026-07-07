<!--
SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>

SPDX-License-Identifier: MIT
-->

# busx 设计文档

- 日期：2026-07-07
- 状态：草案（待评审）
- 作者：Chen Linxuan
- 关联仓库：`busx`（本仓库）

## 1. 背景与动机

Linux 桌面与系统层长期依赖三款 D-Bus 命令行工具，各有短板：

| 工具 | 入参/出参格式 | 主要短板 |
|---|---|---|
| `dbus-send` | `type:value` | 规范明文**禁止空容器与嵌套容器**（如 array of variant）；`a{sv}` 内嵌容器无法表达；类型覆盖不全 |
| `busctl` | 签名串 + 位置参数 | 支持嵌套，但 sd-bus 的 JSON 转换有 bug（见下）；签名驱动写起来啰嗦；非可移植标准 |
| `qdbus` | 无签名、松散 | 列举够用，但类型松散、属性/返回值展示弱、不利脚本 |

`busx` 的目标是用 Rust（`zbus`）做一个**单二进制、零运行时依赖**的综合替代品，把上面三家的痛点一次性补齐，并以**脚本友好**为核心卖点，为后续 TUI 铺路。

### 关键参考：sd-bus 的 JSON bug（systemd#32904）

`busctl --json` 走的 sd-bus JSON 转换里有一行硬检查「dict entry 的键必须是 string，否则 `-EINVAL`」，**直接 `exit 1` 不输出任何内容**。但 D-Bus 规范只要求键是 **basic 类型**（`y b n q i u x t d s o g`），`a{uv}`、`a{ys}` 等完全合法。真实复现：dbus-broker 的 `org.freedesktop.DBus.Debug.Stats.GetStats` 返回 `a{u a{...}}`（uint32 键），`busctl -j` 调它必崩。

**busx 必须从根上规避此 bug**：非 string 键的 dict 正常渲染（见 §7.2），任何情况下不得因 dict 键类型而崩溃或拒绝输出。

## 2. 调研结论摘要

- **zbus `5.16.0`**（最新，2026-05-29）：纯 Rust、无 C 依赖；`edition 2024`、`rust-version 1.87`（本机 `rustc 1.95.0` 满足）。默认特性含 `async-io` + `blocking-api`，后者直接提供同步 API，适合 CLI。关键能力：`Connection::system/session/address`、`blocking::Proxy` 动态代理、`fdo::{DBus,Properties,Introspectable,ObjectManager}Proxy`、`MessageStream`/`receive_signal` 监听、`zvariant::Value` 动态类型树（**原生支持任意嵌套 variant/struct/dict**）。注意：zbus 5.x 已移除 `xml` 特性，内省 XML 自行解析（用 `roxmltree`）。
- **jaq（v2，纯 Rust）**：可嵌入的 jq 克隆（`jaq-core`/`jaq-interpret`/`jaq-std`），无 C 依赖，保住「单二进制」属性。
- **没有现成工具能解析 busctl 的位置文本格式**（仅 `awk`/`cut` 能处理扁平单值，嵌套无解）；GLib 也**没有独立的 `gvariant` CLI**。整个生态对「D-Bus 值流式处理」的标准答案只有 **JSON + jq**。

## 3. 目标与非目标

### v1 目标（本 spec 范围）

1. 列举服务 / 对象树 / 接口内省。
2. 方法调用（`call`），含完整嵌套类型入参。
3. 属性读取（`get`，含 `GetAll` 重载）与写入（`set`）。
4. 监听总线消息（`monitor`），含 `PropertiesChanged` 解码；输出对脚本友好。
5. 内嵌 jq 子命令（`busx jq`），使 busx 自带 jq、可独立派生脚本。
6. 动态 shell 补全（bash/zsh/fish），实时内省总线。
7. core 实现为 lib，供后续 TUI 复用。

### 非目标（后续阶段）

- TUI 程序。
- `copy as dbus-send / busctl / gdbus`（TUI 内做）。
- `emit`（发信号）、pcapng `capture`。
- `--host` / `--machine` 远程与容器总线。
- `wait`（等待信号后退出，可作 `monitor --limit-messages=1` 的近似替代，暂不单列）。

## 4. 关键决策汇总

| 维度 | 决策 |
|---|---|
| 二进制名 | `busx` |
| 入参值格式 | **busctl 风格**：签名串 + 位置参数（数组带计数前缀，variant 带内签名）。完整支持嵌套与空容器 |
| 出参值格式 | **type-tagged JSON**：`{"type":"<sig>","data":<native>}`；TTY 下 dbus-send 式 pretty；`--format=busctl` 出 busctl 文本（供 round-trip / copy-as） |
| 非 string 键 dict | 渲染为 `[{"key":..,"value":..}]`，不崩溃 |
| 错误 | 纯文本打到 stderr + 非零退出（`--json` 不改变错误形态） |
| 默认总线 | 未指定时先连 session，连接失败回退 system（`--verbose` 提示） |
| `get`/`set` | 短名；`get` 不传属性名则走 `GetAll` |
| `monitor` | 默认抓全部消息，`--signals` 收窄；match rule 过滤 |
| jq | 内嵌 `jaq`，作 `busx jq` 薄 passthrough 子命令 |
| crate 结构 | 单 crate `busx`：`[lib]`（可复用核心）+ `[[bin]]`（薄 CLI） |
| 异步模型 | 使用 zbus **blocking API** |

## 5. 架构

单 crate，lib + bin 两个 target：

```
busx/
├── src/
│   ├── lib.rs            # 核心 lib：对外公共 API
│   ├── conn.rs           # 连接管理（默认总线回退逻辑）
│   ├── introspect.rs     # 内省 XML 解析、对象树
│   ├── value/
│   │   ├── encode.rs     # busctl 文本 → zvariant::Value（入参解析）
│   │   ├── decode.rs     # zvariant::Value → type-tagged JSON / pretty / busctl 文本
│   │   └── dict.rs       # 非 string 键 dict 的安全渲染
│   ├── ops/
│   │   ├── list.rs
│   │   ├── tree.rs
│   │   ├── introspect_cmd.rs
│   │   ├── call.rs
│   │   ├── property.rs   # get / set
│   │   └── monitor.rs
│   ├── jq.rs             # jaq 桥接
│   ├── complete.rs       # 动态补全候选生成
│   ├── error.rs          # 错误类型 + 退出码
│   └── fmt.rs            # 输出格式调度（auto/json/pretty/busctl）
├── src/bin/
│   └── busx.rs           # 薄 CLI：clap 解析 → 调 lib
└── tests/                # 集成测试
```

**lib 边界原则**：所有 D-Bus 交互、值编解码、格式化、jaq 桥接都在 lib；bin 只做参数解析与 `println`。后续 `busx-tui` crate 依赖本 lib 即可复用全部能力，无需现在拆 workspace。

## 6. CLI 参考

全局选项（适用于多数子命令）：

```
busx [--user | --system | --address=ADDR]   # 默认: session→system 回退
     [--format=auto|json|busctl|pretty]     # 值输出格式; 默认 auto
     [-j]                                   # --format=json 的简写
     [--timeout=DUR]                        # call/属性操作超时，默认 25s
     [--verbose]
     <COMMAND>
```

`--format` 取值：`auto`（TTY→`pretty`，管道→`json`）、`json`（type-tagged JSON，缩进随 TTY：管道紧凑、TTY 缩进）、`busctl`（busctl 文本）、`pretty`（dbus-send 式人类可读）。`-j` 等价于 `--format=json`。v1 不再单独提供 `--json=pretty|short` 之类的子开关；若后续确需强制缩进风格再加。

子命令：

| 命令 | 形式 | 说明 |
|---|---|---|
| `list` | `busx list [--unique\|--acquired\|--activatable]` | 列出服务名（`ListNames`/`ListActivatableNames`） |
| `tree` | `busx tree [SERVICE...]` | 对象路径树（`Introspect` 递归 / `GetManagedObjects`） |
| `introspect` | `busx introspect SVC OBJ [IFACE] [--xml]` | 接口/方法/信号/属性；`--xml` 出内省原生 XML |
| `call` | `busx call SVC OBJ IFACE METHOD [SIG ARGS...]` | 方法调用，入参为 busctl 风格 |
| `get` | `busx get SVC OBJ [IFACE] [PROP...]` | 见 §8 重载规则 |
| `set` | `busx set SVC OBJ IFACE PROP SIG ARGS...` | 写属性 |
| `monitor` | `busx monitor [SERVICE...] [FILTER...]` | 见 §10 |
| `jq` | `busx jq <filter> [FILE...]` | 内嵌 jaq，薄 passthrough（见 §11） |
| `completion` | `busx completion <SHELL>` | 生成动态补全脚本 |

注：`--json` 等全局选项既可放子命令前，也通过环境支持（`-j` = `--json=pretty`(TTY) / `short`(管道)）。

## 7. 值格式规范

### 7.1 入参（busctl 风格）

紧随方法名给出一个**签名串**，其后按位置给出各参数值，规则与 busctl 一致：

- 简单类型：直接给值字符串。布尔接受 `true/yes/on/1` 与 `false/no/off/0`。
- 数组（`a*`）：先给一个**元素个数**，再依次给元素。
- variant（`v`）：先给**内层签名**，再给内容。
- struct / dict-entry：内容按布局直接平铺。

示例：

```
s  jawoll                                  # 单个字符串 "jawoll"
as 3 hello world foobar                    # 三元素字符串数组
a{sv} 3 One s Eins Two u 2 Yes b true      # dict(string→variant)，三组
v s hello                                  # variant 参数：内层签名 s + 值
```

**与 dbus-send 的关键差异**：busx **完整支持嵌套与空容器**（`av`、`a{sv}` 内含数组/struct、空 `as 0` 等均合法）。补全会在选定方法后据内省签名逐 token 引导入参（见 §12）。

### 7.2 出参（type-tagged JSON）

每个值渲染为 `{"type":"<签名>","data":<原生 JSON>}`：

- 简单类型：`s/o/g → string`、`y/n/q/i/u/x/t → number`、`d → number`、`b → bool`。
- 数组：`{"type":"a<i>","data":[...]}`。
- struct：`{"type":"(...)", "data":[...]}`（元素按序）。
- variant：`{"type":"v","data":{<内层 type-tagged 对象>}}`。
- **string 键 dict**：`{"type":"a{sv}","data":{"Volume":{...},"Muted":{...}}}`（JSON object）。
- **非 string 键 dict**：`{"type":"a{uv}","data":[{"key":{"type":"u","data":0},"value":{...}}]}`（**键保留为原生类型**，无损、jq 友好、绝不崩溃）。
- `ay`（字节数组）：默认渲染为整数数组（无损）；`--bytestrings` 可选渲染为 UTF-8 字符串（对齐 gdbus/dbus-send 的 bytestring 习惯，列为后续增强）。

方法返回多个值时，type-tagged JSON 输出为**值对象数组**（每个返回值一个对象），保证无歧义。

TTY 下（`--format=auto`）默认输出 dbus-send 式 pretty（人类可读）；管道下默认 type-tagged JSON（机器可读）。`--format=busctl` 显式切到 busctl 文本（便于把值 round-trip 喂回 `busx call`/`busctl`，及 TUI 的 copy-as-busctl）。

## 8. `get` / `set` 重载规则

`get` 采用渐进式：

```
busx get SVC OBJ                  → Properties.GetAll("")        # 所有接口的全部属性
busx get SVC OBJ IFACE            → Properties.GetAll(IFACE)     # 该接口全部属性
busx get SVC OBJ IFACE PROP...    → Properties.Get(IFACE,PROP)   # 逐个精确取
```

`GetAll("")` 是 D-Bus 规范允许的「全接口」语义。`set` 必须显式给值：

```
busx set SVC OBJ IFACE PROP SIG ARGS...
```

## 9. 默认总线 / 错误处理 / 退出码

### 默认总线

未指定 `--user/--system/--address` 时：先尝试连 session bus；连接失败（`DBUS_SESSION_BUS_ADDRESS` 未设、socket 不存在、连接被拒等）则**静默回退**到 system bus。`--verbose` 时在 stderr 打一行回退提示。root 等无 session 的场景自动落到 system。

### 错误处理

- 所有错误以**纯文本**打到 stderr，前缀 `busx: error:`。
- JSON 输出模式（`-j`/`--format=json`）**不改变**错误形态（jq 只读 stdin，stderr 不进 jq；纯文本错误 + 看退出码即可）。
- D-Bus error reply 原样保留错误名与消息，如：
  `busx: error: org.freedesktop.DBus.Error.ServiceUnknown: Name has no owner`。

### 退出码

- `0`：成功。
- `1`：运行时/D-Bus 错误（error reply、无此服务、内省失败等）。
- `2`：用法/参数解析错误。

## 10. monitor

```
busx monitor [SERVICE...] [FILTER...]
```

- `SERVICE...`（位置）：well-known 或 unique 名，过滤到「与这些 peer 相关」的消息（同 busctl）。
- 过滤（人话 flag，内部组合成 match rule）：
  - `--interface=IFACE`、`--member=MEMBER`、`--path=PATH`、`--sender=NAME`
  - `--match=MATCH`：直接给 D-Bus 原生 match rule，如 `--match "type='signal',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged',path='/org/foo'"`
- 消息类型：
  - **默认抓全部**（method_call / method_return / error / signal），等同 busctl/dbus-monitor 的总线嗅探。
  - `--signals` 收窄到只看信号。
- 流控：`--limit-messages=N`（收 N 条后退出）、`--timeout=DUR`（超时退出），便于脚本化「等到某事件」。

### 输出

- **管道（非 TTY）**：NDJSON，每条消息一行 JSON 对象。便于 `busx monitor | busx jq 'select(...)'`。
- **TTY**：dbus-send 式分块，人类可读。

NDJSON 每条消息对象示例：

```json
{"type":"signal","sender":":1.5","destination":":1.100","path":"/org/foo",
 "interface":"org.freedesktop.DBus.Properties","member":"PropertiesChanged",
 "serial":47,"reply_serial":null,"error":null,
 "ts":1720000000.123456,
 "args":[
   {"type":"s","data":"org.foo.Iface"},
   {"type":"a{sv}","data":{"Volume":{"type":"d","data":0.5}}},
   {"type":"as","data":["Muted"]}
 ]}
```

字段说明：`type`（消息类型）、`sender`/`destination`、`path`/`interface`/`member`、`serial`（本次消息序号）、`reply_serial`（仅 method_return/error，关联的 call 序号）、`error`（仅 error 类型，错误名）、`ts`（收到时刻，epoch 秒浮点）、`args`（按位置的 type-tagged 值数组）。

`PropertiesChanged`（`sa{sv}as`）天然落在 `args` 里：`args[0]`=接口名，`args[1]`=变更属性 dict（`a{sv}` 已展开），`args[2]`=失效属性名列表。脚本可直接据 `member=="PropertiesChanged"` 过滤后取 `args[1]` 处理。

## 11. busx jq（内嵌 jaq）

- 形式：`busx jq <filter> [FILE...]`，行为对齐 `jq`：无 FILE 则读 stdin，应用 filter，输出结果。
- 实现：薄 passthrough 到 `jaq`（`jaq-core`/`jaq-interpret`/`jaq-std`）。支持 jq 常用 flag（`-r`/`-c`/`-n`/`-s`/`-e`/`--arg`/`--slurpfile` 等）。
- **不做** type-tag 解包等特殊处理：用户直接对 busx 输出的 JSON 写 jq 表达式（如 `busx monitor | busx jq 'select(.member=="PropertiesChanged") | .args[1]'`）。
- 目的：让 busx **自带 jq**，在任何未装 jq 的环境也能基于 busx 派生脚本；除此之外不引入额外耦合。

## 12. 动态补全（v1 硬需求）

`busx completion <shell>` 生成 bash/zsh/fish 脚本，脚本在补全时回调 busx 实时取候选（`clap_complete` 静态部分 + 隐藏 `__complete` 子命令做动态部分）：

```
busx call <TAB>        → 实时 ListNames 列 well-known 服务名
busx call svc <TAB>    → tree/introspect 列 object path
busx call svc /o <TAB> → 列 interface
... iface <TAB>        → 列 method
... method <TAB>       → 据内省入参签名，逐 token 引导（键/类型/值）
```

内省有缓存以避免每次按键都打总线。补全失败（如未连总线）静默退化为不补，不影响主命令。

## 13. 依赖

均为纯 Rust、无 C 依赖：

| crate | 版本约束 | 用途 |
|---|---|---|
| `zbus` | `5` | D-Bus 通信（默认含 `async-io`+`blocking-api`，用 blocking） |
| `zvariant` | 随 zbus 5.x | `Value` 动态类型树 |
| `clap` | `4` | CLI（derive） |
| `clap_complete` | `4` | 静态补全脚本生成 |
| `serde` | `1` | 序列化 |
| `serde_json` | `1` | type-tagged JSON |
| `jaq`（`jaq-core`/`jaq-interpret`/`jaq-std`） | `2` | 内嵌 jq |
| `roxmltree` | `0.20` | 内省 XML 解析 |
| `thiserror` | `2` | 错误类型 |
| `anyhow` | `1` | bin 层错误聚合 |

实现阶段以 `cargo add` 时解析到的最新兼容版本为准。

## 14. 测试策略

- **单测（lib）**：
  - 值编解码往返：覆盖嵌套 variant、`a{sv}`、struct、空容器、**非 string 键 dict（`a{uv}`/`a{ys}`）**，断言无崩溃且无损。
  - 内省 XML 解析：典型 introspection XML → 结构化接口/成员树。
  - 入参文本解析：busctl 风格各类型 token → `Value`。
- **集成测试**：
  - 真实 session bus + 进程内小 zbus 测试服务，覆盖 `call`（含嵌套入参）、`get`（`GetAll`/`Get`）、`monitor`（含触发 `PropertiesChanged`）。
  - 默认总线回退逻辑（构造无 session 环境，断言回退 system）。
- **补全**：冒烟测试 `__complete` 对固定总线的候选输出。

## 15. 路线图（后续阶段，不在 v1）

1. **TUI**：基于本 lib，提供交互式浏览/call/monitor；支持 `copy as dbus-send / busctl / gdbus`。
2. `emit`（发信号）、pcapng `capture`。
3. `--host` / `--machine` 远程与容器总线。
4. `--bytestrings` 等 value 渲染增强。

## 16. 参考资料

- dbus-send(1)：https://dbus.freedesktop.org/doc/dbus-send.1.html
- busctl(1)：https://www.freedesktop.org/software/systemd/man/busctl.html
- D-Bus 规范（类型系统 / match rules）：https://dbus.freedesktop.org/doc/dbus-specification.html
- sd-bus 非 string 键 dict JSON bug：https://github.com/systemd/systemd/issues/32904
- zbus：https://crates.io/crates/zbus
- jaq：https://crates.io/crates/jaq-core （仓库 https://github.com/01mf02/jaq）
- Lennart 介绍 sd-bus：https://0pointer.net/blog/the-new-sd-bus-api-of-systemd.html
