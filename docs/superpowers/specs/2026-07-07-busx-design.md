<!--
SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>

SPDX-License-Identifier: MIT
-->

# busx 设计文档

- 日期：2026-07-07
- 状态：v1 已实现（本文档已同步至当前实现）
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
- **没有现成工具能解析 busctl 的位置文本格式**（仅 `awk`/`cut` 能处理扁平单值，嵌套无解）；GLib 也**没有独立的 `gvariant` CLI**。整个生态对「D-Bus 值流式处理」的标准答案只有 **JSON + jq**——故 busx 的机器格式（`--json`）输出 JSON/NDJSON，美化/变换交给用户自选的外部工具（`jq`/python 等），不内嵌 jq（理由见 §11）；默认（无 `--json`）输出人类可读文本。

## 3. 目标与非目标

### v1 目标（本 spec 范围）

1. 列举服务 / 对象树 / 接口内省。
2. 方法调用（`call`），含完整嵌套类型入参。
3. 属性读取（`get`，含 `GetAll` 重载）与写入（`set`）。
4. 监听总线消息（`monitor`），含 `PropertiesChanged` 解码；输出对脚本友好。
5. 动态 shell 补全（bash/zsh，`clap_complete::dynamic`），实时内省总线。
6. 模块化分层（**不发布 lib**）；后续若做 TUI，可在同一 crate 加第二个 `[[bin]]` 共享 `src/` 模块，或届时再抽 lib。

### 非目标（后续阶段）

- TUI 程序。
- `copy as dbus-send / busctl / gdbus`（TUI 内做）。
- `emit`（发信号）、pcapng `capture`。
- `--host` / `--machine` 远程与容器总线。
- `wait`（等待信号后退出，可作 `monitor --limit-messages=1` 的近似替代，暂不单列）。
- 内嵌 jq 子命令（见 §11）。

## 4. 关键决策汇总

| 维度 | 决策 |
|---|---|
| 二进制名 | `busx` |
| 输出格式 | **默认人类可读文本**；全局 `--json` 切到机器格式（值 type-tagged JSON `{"type":..,"data":..}`，`monitor` 为 NDJSON）。美化/变换交给外部 `jq`/python，不内嵌 jq |
| 入参值格式 | **busctl 风格**：签名串 + 位置参数（数组带计数前缀，variant 带内签名）。完整支持嵌套与空容器 |
| 非 string 键 dict | JSON 渲染为 `[{"key":..,"value":..}]`、人类渲染为 `{k: v}`，不崩溃 |
| 错误 | 纯文本打到 stderr，前缀 `busx:`；退出码 `0` 成功 / `1` 失败 |
| SIGPIPE | `main` 入口恢复 `SIG_DFL`：管道到 `less`/`head` 被正常 SIGPIPE 终止（exit 141），不 panic |
| 默认总线 | 未指定时先连 session，连接失败回退 system（`--verbose` 提示） |
| `list` | 人类为 `NAME PID PROCESS` 表（PID 取 `GetConnectionUnixProcessID`，PROCESS 取 `/proc/<pid>/comm`；NAME 截断、总宽 ≤80）；JSON 为 `[{name,pid,process}]` |
| `tree` | 仅单个 service：`tree SERVICE` |
| `call` | `call SVC OBJ IFACE METHOD SIG ARGS...`（SIG 为独立必填位，可补全） |
| `get`/`set` | 短名；`get` 不传属性名走 `GetAll` |
| `monitor` | 默认抓全部消息，`--signals` 收窄；match rule 过滤 |
| 补全 | `clap_complete::dynamic`（`unstable-dynamic`）；补 service/path/interface/method/call签名/get+set属性，总线感知 |
| crate 结构 | 单 bin crate（**不发布 lib**）；逻辑放 `src/` 模块，`src/main.rs` 为入口 |
| 异步模型 | 使用 zbus **blocking API** |

## 5. 架构

单 bin crate（**不发布 lib**），`src/main.rs` 为入口，逻辑按模块分层放在 `src/` 下：

```
busx/
├── src/
│   ├── main.rs           # 入口：clap 解析 → 分发到各模块
│   ├── conn.rs           # 连接管理（默认总线回退逻辑）
│   ├── introspect.rs     # 内省 XML 解析、对象树
│   ├── value/
│   │   ├── encode.rs     # busctl 文本 → zvariant::Value（入参解析）
│   │   ├── decode.rs     # zvariant::Value → type-tagged JSON
│   │   ├── pretty.rs     # zvariant::Value → 人类可读文本
│   │   └── dict.rs       # 非 string 键 dict 的安全渲染
│   ├── ops/
│   │   ├── list.rs       # 带 PID/进程名、NAME 列宽控制
│   │   ├── tree.rs       # 单 service 路径树
│   │   ├── introspect.rs
│   │   ├── call.rs       # 含独立 SIG 位
│   │   ├── property.rs   # get / set
│   │   └── monitor.rs
│   ├── complete.rs       # clap_complete::dynamic 补全
│   ├── error.rs          # 错误类型 + 退出码
│   └── out.rs            # JSON 输出
└── tests/                # 集成测试（assert_cmd 驱动 busx 二进制）
```

**分层原则**：D-Bus 交互、值编解码、JSON 输出各居其位；`main.rs` 只做参数解析与分发。后续若做 TUI，可在同 crate 加第二个 `[[bin]]` 共享这些模块，或届时再抽 lib——不为推测性的复用提前付出 lib 边界成本。

## 6. CLI 参考

全局选项（适用于多数子命令）：

```
busx [--user | --system | --address=ADDR]   # 默认: session→system 回退
     [--verbose]
     [--json]                                # 默认人类可读；--json 切机器格式
     <COMMAND>
```

**默认输出为人类可读文本**；加全局 `--json` 后切换为机器格式（值 type-tagged JSON，`monitor` 为 NDJSON）。美化或字段变换交给外部 `jq`/python 等（busx 不内嵌 jq，见 §11）。方法调用/属性操作的超时沿用底层库默认（libdbus/zbus 约 25s）。

子命令：

| 命令 | 形式 | 说明 |
|---|---|---|
| `list` | `busx list [--unique\|--acquired\|--activatable]` | 列出服务名 + PID + 进程名（人类为表，JSON 为 `[{name,pid,process}]`） |
| `tree` | `busx tree SERVICE` | 单个 service 的对象路径树（`Introspect` 递归） |
| `introspect` | `busx introspect SVC OBJ [IFACE]` | 接口/方法/信号/属性（人类分组 / 结构化 JSON） |
| `call` | `busx call SVC OBJ IFACE METHOD SIG ARGS...` | 方法调用；SIG 为独立必填位（可补全），入参 busctl 风格 |
| `get` | `busx get SVC OBJ [IFACE] [PROP...]` | 见 §8 重载规则 |
| `set` | `busx set SVC OBJ IFACE PROP SIG ARGS...` | 写属性 |
| `monitor` | `busx monitor [SERVICE...] [FILTER...]` | 见 §10 |
| `completion` | `busx completion <SHELL>` | 生成动态补全脚本（见 §12） |

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

### 7.2 出参

**两种模式**：默认人类可读文本；`--json` 出 type-tagged JSON。D-Bus 值的渲染规则如下（两模式共用类型映射，只是排版不同）。

**JSON（`--json`）**——每个值渲染为 `{"type":"<签名>","data":<原生 JSON>}`：

- 简单类型：`s/o/g → string`、`y/n/q/i/u/x/t → number`、`d → number`、`b → bool`。
- 数组：`{"type":"a<i>","data":[...]}`。
- struct：`{"type":"(...)", "data":[...]}`（元素按序）。
- variant：`{"type":"v","data":{<内层 type-tagged 对象>}}`。
- **string 键 dict**：`{"type":"a{sv}","data":{"Volume":{...},"Muted":{...}}}`（JSON object）。
- **非 string 键 dict**：`{"type":"a{uv}","data":[{"key":{"type":"u","data":0},"value":{...}}]}`（**键保留为原生类型**，无损、jq 友好、绝不崩溃）。
- `ay`（字节数组）：整数数组（无损）。bytestring 字符串视图列为后续（§15）。

方法返回多个值时，输出为**值对象数组**（每个返回值一个对象），无歧义。`list`/`tree`/`introspect` 等结构化命令按各自语义输出 JSON 数组/对象，**不套** type-tag 包装。

**人类（默认）**——值的 pretty 写法（`src/value/pretty.rs`）：数字裸写、字符串加引号、`true/false`、数组 `[a, b]`、dict `{k: v}`（**含非 string 键，如 `{1:10}`**）、variant `<d 0.5>`（带内类型）。`call`/`get` 每个值一行 `<type>  <pretty>`；`list` 为 `NAME PID PROCESS` 表（NAME 截断、总宽 ≤80）；`tree` 为路径树；`introspect` 按接口分组；`monitor` 为 dbus-send 式分块（见 §10）。

JSON 始终紧凑；缩进美化或变换用外部 `jq`/python。

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

- 所有诊断（错误、警告，如总线回退提示）以**纯文本**打到 stderr，统一前缀 `busx:`（不强制 `error:` 子串，留余地给 `busx: warning:` 之类）。诊断**始终纯文本**（`--json` 只影响 stdout；stderr 不变），看退出码即可。
- D-Bus error reply 原样保留错误名与消息，如：
  `busx: org.freedesktop.DBus.Error.ServiceUnknown: Name has no owner`。

### SIGPIPE

Rust 默认把 SIGPIPE 设为 `SIG_IGN`，管道下游（`less`/`head`）提前关闭会让 stdout 写返回 `EPIPE`，`println!` 因此 panic。busx 在 `main()` 入口把 SIGPIPE 恢复为 `SIG_DFL`，故管道场景按 Unix 惯例被 SIGPIPE 终止（exit 141），不 panic。

### 退出码

- `0`：成功。
- `1`：失败（任何错误：D-Bus error reply、无此服务、内省失败、参数错误等）。

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

默认人类可读（dbus-send 式分块，每条消息一段）；`--json` 时为 **NDJSON**（每条消息一行 JSON 对象），便于管道到外部 `jq`/python 过滤（如 `busx --json monitor | jq 'select(.member=="Bar")'`）。下面的字段说明针对 NDJSON 形态。

NDJSON 每条消息对象示例：

```json
{"type":"signal","sender":":1.5","destination":":1.100","path":"/org/foo",
 "interface":"org.freedesktop.DBus.Properties","member":"PropertiesChanged",
 "serial":47,"reply_serial":null,"error":null,
 "signature":"sa{sv}as","flags":[],
 "ts":1720000000.123456,
 "args":[
   {"type":"s","data":"org.foo.Iface"},
   {"type":"a{sv}","data":{"Volume":{"type":"d","data":0.5}}},
   {"type":"as","data":["Muted"]}
 ]}
```

字段说明：`type`（消息类型）、`sender`/`destination`、`path`/`interface`/`member`、`serial`（本次消息序号）、`reply_serial`（仅 method_return/error，关联的 call 序号）、`error`（仅 error 类型，错误名）、`signature`（body 签名串）、`flags`（消息标志，如 `no_reply_expected`/`no_auto_start`，无则 `[]`）、`ts`（收到时刻，epoch 秒浮点）、`args`（按位置的 type-tagged 值数组）。

**完备性**：此对象覆盖了 D-Bus 消息所有**语义相关**字段（header + 完整解码的 body）。刻意省略的只有无信息量的协议版本号；`h`（文件描述符）类型按本进程收到的 fd 编号渲染（不可移植，仅作占位）。若需逐字节原始报文，用后续的 pcapng `capture`（§15）。

`PropertiesChanged`（`sa{sv}as`）天然落在 `args` 里：`args[0]`=接口名，`args[1]`=变更属性 dict（`a{sv}` 已展开），`args[2]`=失效属性名列表。脚本可直接据 `member=="PropertiesChanged"` 过滤后取 `args[1]` 处理。

## 11. 为什么不内嵌 jq

最初设想内嵌 `jaq` 作 `busx jq` 子命令，让 busx 自带 jq、在无 jq 环境也能派生脚本。**已核实（2026-07）放弃**，理由：

- `jaq` crate **全版本 bin-only**（crates.io `has_lib=false`，docs.rs 明示「not a library」）；可复用的 `jaq-core` 只是解释器，**不含 CLI flag 解析**。
- 故要内嵌只能二选一：自建一套 jq flag 解析，或 vendor jaq 的 CLI 层。两者都是 busx 独自维护、跟随 jq/jaq 演进的长期成本，与「薄工具」定位不符。

替代：busx 输出标准 JSON/NDJSON，美化与变换交给用户既有的外部 `jq`/python 等。信号过滤的主诉求已由 `monitor` 的 D-Bus match rule（§10）覆盖，不依赖 jq。若未来 `jaq` 发布可复用的库入口，再重新评估（见 §15）。

## 12. 动态补全（v1 硬需求）

基于 `clap_complete::dynamic`（启用 `unstable-dynamic`，`CompleteEnv` + `ArgValueCompleter`）：clap 负责子命令/flag/全局 flag 的补全与解析；位置参数挂 `ArgValueCompleter` 闭包实时内省总线给候选（**总线感知**——闭包从 `std::env::args_os()` 读 `--user/--system/--address` 选对总线）。`busx completion <shell>` 输出对应 shell 的注册脚本。

补全位置：

```
busx call <TAB>              → well-known 服务名
busx call svc <TAB>          → object path
busx call svc /o <TAB>       → interface
... iface <TAB>              → method
... method <TAB>             → 该方法入参签名（call 的 SIG 位）
busx get/set ... iface <TAB> → 属性名
```

每次补全**实时内省、不缓存**。补全失败（如未连总线）静默退化为不补，不影响主命令。

> 注：clap_derive 4.6 尚不支持 `#[arg(add = ...)]`，故补全用一份手搭的 `Cli` 镜像 `command()` 挂 completer，与真正 `Cli` 有漂移风险；等 clap 支持后可直接用 `Cli::command()`。

## 13. 依赖

均为纯 Rust、无 C 依赖：

| crate | 版本约束 | 用途 |
|---|---|---|
| `zbus` | `5` | D-Bus 通信（默认含 `async-io`+`blocking-api`，用 blocking） |
| `zvariant` | 随 zbus 5.x | `Value` 动态类型树 |
| `clap` | `4` | CLI（derive） |
| `clap_complete` | `4`（`unstable-dynamic`） | 动态补全（`CompleteEnv`/`ArgValueCompleter`） |
| `serde` | `1` | 序列化 |
| `serde_json` | `1` | type-tagged JSON / NDJSON |
| `roxmltree` | `0.20` | 内省 XML 解析 |
| `enumflags2` | `0.7` | 消息 flags 类型 |
| `libc` | `0.2` | 恢复 SIGPIPE 默认处置 |
| `thiserror` | `2` | 错误类型 |
| `anyhow` | `1` | bin 层错误聚合 |

开发依赖（`[dev-dependencies]`）：`assert_cmd`、`predicates`、`nix`（fixture 清理 daemon）。集成测试另需系统装有 `dbus-daemon` 二进制。

实现阶段以 `cargo add` 时解析到的最新兼容版本为准。

## 14. 测试策略

**只做端到端的集成测试。**

- **统一准备一次**：所有集成测试共用一条独立消息总线——测试入口用一次性 fixture（`OnceLock`/`Once`）spawn 一个 `dbus-daemon --session --print-address`，进程内用 zbus 注册一个小测试服务（暴露含嵌套类型 / 属性 / 信号 / 非 string 键 dict 的接口），地址供全部用例复用；不为每个用例重启 daemon。
- 因用例在共享总线上并行执行，**用例间用独立 object path / well-known 名隔离**（如每个 `#[test]` 取唯一后缀）避免互相干扰。
- 用 [`assert_cmd`](https://crates.io/crates/assert_cmd) 以子进程方式驱动 `busx` 二进制，通过 `--address=<该总线地址>` 指向测试总线，断言 stdout（人类文本 / type-tagged JSON / NDJSON）与退出码。
- 覆盖：`list`/`tree`/`introspect`/`call`（含嵌套入参 + 独立 SIG）/`get`/`set`/`monitor`（触发 `PropertiesChanged` 与非 string 键 dict，断言不崩溃）/`completion`（service/path/interface/method/SIG/property + 子命令位 + 全局 flag 透传）。每个命令的人类格式与 `--json` 格式各有用例；`list` 含 NAME 截断/80 列宽回归。
- 默认总线 session→system 回退：构造无 session 环境的用例断言回退。
- CI：取消 `.github/workflows/ci.yaml` 里 `rust` job 的注释，复用模板的 [`black-desk/workflows/rust@master`](https://github.com/black-desk/workflows)（fmt/clippy/test/build 一站式）。ubuntu runner 自带 `dbus-daemon`，集成测试可直接跑。

## 15. 路线图（后续阶段，不在 v1）

> 此路线图属对外信息：**实现阶段写入 `README`**（作为项目对外发布说明），本节仅作设计备案；二者需保持同步。

1. **TUI**：基于本 crate 的 `src/` 模块（加第二个 `[[bin]]` 或届时抽 lib），提供交互式浏览/call/monitor；支持 `copy as dbus-send / busctl / gdbus`。
2. `emit`（发信号）、pcapng `capture`。
3. `--host` / `--machine` 远程与容器总线。
4. `ay` 的 bytestring 字符串视图等值渲染增强。
5. 若 `jaq` 将来发布可复用的「flag 解析 + 运行」库入口，重新评估内嵌 `busx jq` 子命令。

## 16. 参考资料

- dbus-send(1)：https://dbus.freedesktop.org/doc/dbus-send.1.html
- busctl(1)：https://www.freedesktop.org/software/systemd/man/busctl.html
- D-Bus 规范（类型系统 / match rules）：https://dbus.freedesktop.org/doc/dbus-specification.html
- sd-bus 非 string 键 dict JSON bug：https://github.com/systemd/systemd/issues/32904
- zbus：https://crates.io/crates/zbus
- jaq（已核实 bin-only，不作库）：https://github.com/01mf02/jaq
- Lennart 介绍 sd-bus：https://0pointer.net/blog/the-new-sd-bus-api-of-systemd.html
