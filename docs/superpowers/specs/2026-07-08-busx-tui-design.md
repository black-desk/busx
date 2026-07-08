<!--
SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>

SPDX-License-Identifier: MIT
-->

# busx TUI 设计文档

- 日期：2026-07-08
- 状态：设计稿（待评审）
- 作者：Chen Linxuan
- 关联仓库：`busx`（本仓库）
- 前身：v1 CLI 设计 `2026-07-07-busx-design.md`（本文称「v1 设计」）；本 spec 对应 v1 §15 路线图第 1 项

## 1. 背景与目标

busx v1 已实现 CLI（`list`/`tree`/`introspect`/`call`/`get`/`set`/`monitor`/`completion`），
底层用 zbus。本 spec 设计**交互式 TUI**：在终端里浏览 D-Bus 的 service → object →
interface，做方法调用、属性读写、信号/属性/方法监听，并支持把任意操作一键复制为
`dbus-send` / `busctl` / `qdbus` / `gdbus` 命令。

目标：

1. **`busx` 默认进 TUI**；CLI 子命令保持不变（`busx list` 等）。
2. 复用 v1 已有的 D-Bus 逻辑，不重复造轮子——为此**抽一个共享内部核心模块**，CLI 与
   TUI 都引用它（见 §3）。
3. 全程基于 zbus 原生异步 API（v1 用的是 blocking 薄封装），让 TUI 不被阻塞、监听可取消。
4. UI 组件优先用 ratatui 内置 widget，仅对 ratatui 缺失的能力（对象树、文本输入）引入第三方
   控件。

非目标（沿用 v1 §15，不在本 spec 范围）：`emit`、pcapng `capture`、`--host`/`--machine`
远程与容器总线、`ay` 的 bytestring 视图、内嵌 jq。

## 2. 关键决策汇总

| 维度 | 决策 |
|---|---|
| 默认模式 | 裸 `busx`（无子命令）→ 进 TUI；`busx <subcmd>` → CLI（不变） |
| 核心层 | 抽 `src/dbus/`：所有具体 D-Bus 操作都在此，**async，返回类型化结构体**，不打印/不出 JSON；CLI 与 TUI 都引用 |
| 异步模型 | zbus 原生 async（`async-io` 已在 v1 开启）+ `async-global-executor`（zbus 传递依赖，直引）；UI 线程跑 crossterm 轮询 + channel 抽干 |
| UI 线程并发 | 一次性操作 `spawn` 异步任务，结果经 `flume` 回传；UI 帧循环 `try_recv` |
| 监听取消 | 监听任务 `futures::select!` 把「消息流」与「取消信号」竞争；**离开结果页 drop 取消 Sender → 任务退出**（不依赖执行器 drop 语义） |
| 监听生命周期 | **离开结果页即停**；任意时刻隐式只有一个活跃监听（结果页在栈顶，pop 即停） |
| 组件库 | ratatui 0.30（内置 `List`/`Table`/`Tabs`/`Paragraph`/`Scrollbar`/`Clear`/`Block`…）；第三方仅 `tui-tree-widget`（对象树）、`tui-input`（文本输入）、`arboard`（剪贴板） |
| 属性值刷新 | 进入 interface 页 `GetAll` 取**快照**；`r` 手动刷新；**不做实时订阅** |
| 导航 | drill-down 栈（push / `Esc` 弹）；objects 仅 1 个→直进 interfaces；interfaces（排除标准管家接口）仅 1 个→直进 interface 页 |
| copy-as | dbus-send / busctl / qdbus / gdbus 四种；由「操作描述符 + 类型化值」生成 |
| 测试 | **只端到端**；dbus 核心经 CLI e2e 间接覆盖；TUI 用 ratatui `TestBackend` 快照（按键+屏幕维度，视为有效 e2e）；render 保持为 state 的纯函数以支持快照 |
| crate 结构 | 单 bin 不变（不发布 lib）；逻辑分层到 `src/dbus/`、`src/tui/`；`src/ops/` 变薄 |

## 3. 架构：共享 async 核心层

v1 的 `ops/*` 直接持有 blocking 连接、调用 zbus、并把结果**打印**到 stdout——TUI 无法复用
（它要的是数据，不是已打印文本）。因此抽出一个**共享核心模块 `src/dbus/`**：所有具体 D-Bus
操作集中于此，async，返回**类型化结构体**（不打印、不产出 JSON）。CLI 的 `ops/*` 变成薄壳
（`block_on` 调核心 → 按现有 human/json 形状打印）；TUI 直接 `await` 核心。

```
src/
├── main.rs / cli.rs        # command 改 Option<Command>：None→TUI，Some→CLI 分发
├── dbus/                   # ★ 共享核心：所有具体 D-Bus 操作都在这，async，返回类型化结构体
│   ├── mod.rs              #   再导出 + 共享 trait
│   ├── conn.rs             #   async 连接 + session→system 回退（替换 v1 blocking conn.rs）
│   ├── types.rs            #   ServiceInfo / ObjectNode / BusMessage（内省结构复用 zbus_xml::Node/Interface/Method/Signal/Property/Arg）
│   ├── list.rs             #   list_names
│   ├── tree.rs             #   object_tree（递归 introspect 走路径树，zbus_xml 取子节点）
│   ├── introspect.rs       #   introspect（单对象，zbus_xml::Node::from_reader 解析）
│   ├── call.rs             #   call_method
│   ├── property.rs         #   get_all / get_one / set
│   └── monitor.rs          #   build_match_rule（纯，自 ops/monitor 迁入）+ 客户端过滤 + BecomeMonitor 启动
├── value/                  # 不变（encode/decode/pretty/dict，纯逻辑，核心与 CLI 均复用）
├── out.rs                  # JSON 输出（仅 CLI 用）
├── ops/                    # ★ 变薄：block_on(dbus::xxx()) → human/json 打印（输出形状不变）
├── complete.rs             # 动态补全：改走 dbus::conn（block_on）
└── tui/                    # ★ 新：直接 await dbus::*，跑在 async-global-executor 上
    ├── mod.rs / app.rs     #   App 状态 + 事件循环 + 终端 setup/teardown
    ├── event.rs            #   crossterm 事件轮询
    ├── channel.rs          #   Msg（按键 ‖ DBus 结果/错误/监听消息）+ worker 派发
    ├── elm.rs              #   update(state, Msg)->state 与 render(frame, &state) 纯函数
    ├── copy.rs             #   copy-as 生成（dbus-send/busctl/qdbus/gdbus）
    ├── render.rs           #   共享样式/widget 工具
    └── pages/              #   service / objects / interfaces / interface / detail / result
```

分层原则：**D-Bus 交互（`dbus/`）、值编解码（`value/`）、内省 XML 解析交给 `zbus_xml`，不再自解析，三者各居
其位，互不打印**；`ops/` 与 `tui/` 是两个等价消费者，都只调 `dbus/` + 渲染。

### 连接迁移

- `dbus::conn::connect(user, system, address, verbose)` 改为 **async**，逻辑同 v1（`--address`
  优先；`--system`；否则 session，失败回退 system，`--verbose` 提示）。
- CLI：`let conn = async_global_executor::block_on(dbus::conn::connect(..))?;` 然后每步操作
  `block_on`（CLI 短命、顺序，逐操作 block_on 即可）。
- `complete.rs` 同理改走 async `connect`。
- v1 的 `src/conn.rs` 删除，逻辑并入 `dbus/conn.rs`。
- v1 的 `src/introspect.rs`（roxmltree 手解析）删除；`dbus/introspect`、`dbus/tree` 改用
  `zbus_xml::Node::from_reader`，**移除 `roxmltree` 依赖**。

## 4. 核心层接口与类型

核心用**类型化结构体**取代 v1 里到处传递的 `serde_json::Value`。内省结构**直接复用 `zbus_xml`**
（`Node`/`Interface`/`Method`/`Signal`/`Property`/`Arg`，带 `ArgDirection`、`PropertyAccess`、
`Signature`），不自定义。CLI 的 `--json` 由这些结构体
经 `Serialize` 产出，**输出形状与 v1 保持一致**（现有 CLI e2e 测试守护，见 §13）。

```rust
// dbus/types.rs
pub struct ServiceInfo { pub name: String, pub pid: Option<u64>, pub process: Option<String> }

pub struct ObjectNode { pub path: String, pub children: Vec<ObjectNode> } // 层级路径树；CLI `tree` 展平排序后保持 v1 输出

// 内省结构直接复用 zbus_xml（不再自定义）：
//   use zbus_xml::{Node, Interface, Method, Signal, Property, Arg, ArgDirection, PropertyAccess};
//   Node::from_reader(reader) -> Node { interfaces, nodes, ... }
//   Method/Signal 含 Arg（带 ArgDirection::In/Out）；Property 含 PropertyAccess::Read/Write/ReadWrite
//   与 Signature（zvariant）。TUI 直接用这些类型渲染；CLI 由它们映射出 v1 的 --json 形状。

pub struct BusMessage { /* type/sender/destination/path/interface/member/serial/reply_serial/
                           error/signature/flags/ts/args —— 字段同 v1 设计 §10 的 NDJSON 对象*/ }
```

核心函数（均 `async`，返回 `error::Result<T>`；`conn` 为 async `zbus::Connection`）：

```rust
// dbus/conn.rs
pub async fn connect(user: bool, system: bool, address: Option<&str>, verbose: bool) -> Result<Connection>;
// dbus/list.rs
pub async fn list_names(conn: &Connection, unique: bool, acquired: bool, activatable: bool) -> Result<Vec<ServiceInfo>>;
// dbus/tree.rs
pub async fn object_tree(conn: &Connection, service: &str) -> Result<ObjectNode>;
// dbus/introspect.rs
pub async fn introspect(conn: &Connection, service: &str, object: &str) -> Result<zbus_xml::Node>;  // 含 interfaces + 子 nodes
// dbus/call.rs
pub async fn call_method(conn, service, object, iface, method, signature: &str, args: &[String]) -> Result<Vec<Value<'static>>>;
// dbus/property.rs
pub async fn get_all(conn, service, object, iface: &str) -> Result<Vec<(String, Value)>>;
pub async fn get_one(conn, service, object, iface, prop) -> Result<Value>;
pub async fn set(conn, service, object, iface, prop, signature: &str, value_tokens: &[String]) -> Result<()>;
// dbus/monitor.rs（纯 + 启动）
pub fn build_match_rule(..) -> MatchRule<'static>;           // 自 ops/monitor 迁入
pub fn matches_method(m: &BusMessage, iface, member, path) -> bool;  // BecomeMonitor 后客户端过滤
pub fn matches_property_change(m: &BusMessage, iface, prop) -> bool;
// BecomeMonitor 启动 + 异步消息流 → BusMessage 的装配在 TUI 监听任务里完成（见 §5/§9）
```

值解析/渲染继续用 `value::encode::parse`（入参）、`value::decode::to_tagged`、
`value::pretty::pretty`，核心与 CLI 都调。

## 5. 并发模型（不卡 UI、监听可取消）

- **连接**：TUI 在 launch 时建**一条** async `Connection`（cheap clone 进各任务），用于一切
  **非 BecomeMonitor** 操作（list/tree/introspect/call/get/set + 信号监听 + 属性监听）。
- **UI 线程**（同步）：循环 = `crossterm::event::poll(短超时)` → 读按键/resize → 把 `flume`
  channel 里 `try_recv` 抽干（应用 `update`）→ 若脏则重绘。UI 线程**从不** `await`、从不阻塞
  在 DBus 上。
- **一次性操作**：`async_global_executor::spawn` 一个任务调 `dbus::*`，把
  `Ok(data)` / `Err(e)` 作为 `Msg::DbusResult` 发回 channel；页面置 loading。
- **监听任务**：持消息流，循环
  `futures::select! { m = stream.next() => tx.send(Msg::Monitor(m)), _ = cancel_rx => break }`。
  监听结果页持有唯一的 `cancel_tx`；**`Esc` 离开该页 → drop `cancel_tx` → select 见断开 → 任务
  退出**（符合 §2「离开即停」）。
- **BecomeMonitor 专用连接**：`BecomeMonitor` 会让连接变成「只收不发」，**不能复用**主连接。
  故**方法监听**（以及任何「全量 monitor」）为该监听单独建一条 `Connection`、对其
  `BecomeMonitor`，离开页时随任务一起 drop。信号监听 / 属性监听用 match-rule 订阅，在主连接上
  进行，非破坏性。
- 新增直接依赖：`async-global-executor`（zbus 传递依赖，直引不增编译）、`flume`（同步/异步
  双向，UI `try_recv` + worker `send`）、`futures`（`select!` / `StreamExt`）。

## 6. TUI 状态 / 渲染 / 更新（Elm 式，为快照测试服务）

TUI 内部按三层切，使 `render` 成为 `state` 的纯函数（这是 `TestBackend` 快照的前提，也是「为
可测试性而设计」的落点）：

- `state`：纯数据——连接句柄、channel、屏幕栈、各屏数据、焦点、loading/错误标志、popup。
- `update(state, Msg) -> state`：纯函数。`Msg = Key(KeyEvent) | Resize | DbusResult(..) |
  Monitor(BusMessage) | Error(..)`。
- `render(frame, &state)`：纯函数，只读 state 画图（ratatui + 第三方控件）。

事件循环只做：取 Msg → `update` → `render`。DBus 副作用只在 worker 任务里发生，结果以 Msg 回流。

## 7. 导航模型与自动跳过

drill-down 栈，每屏是一个 `Screen` 变体：

```
Service → Objects(tree) → Interfaces → Interface(三列) → Detail → Result
```

- **自动跳过**：进入下一级时，若该级**恰好 1 个**可选条目则跳过该屏直接再下一级——
  - 选 service → 若该 service 恰好 1 个 object → 直进 Interfaces（仍带该 object）；
  - 选 object → 若该 object 恰好 1 个非标准接口 → 直进 Interface 页。
  - 恰好 0 个 → 不跳过，显示空页（空态提示）。
- **Interfaces 排除**标准管家接口：`org.freedesktop.DBus.Introspectable`、
  `.Properties`、`.Peer`（以及 `.Debug.Stats` 等明显属调试的）。
- **返回**：`Esc` 弹当前屏；在 Service 屏 `Esc`（或 `q`）退出；`Ctrl+C` 总是退出。

## 8. 各页面与按键

全局按键：`j/k ↑↓` 选项移动 · `Tab/Shift+Tab` 列间/按钮间切焦点 · `Enter` 激活 · `Esc` 返回
（顶层=退出）· `q` 退出 · `r` 刷新（上下文相关：刷服务列表 / 刷属性值 / 重跑上次调用）· `?`
帮助叠层 · `c` 复制（上下文相关：复制结果文本）。

**底部按键提示栏**：每个屏幕最底一行固定显示当前上下文最相关的按键（完整列表用 `?` 打开帮助
叠层），随屏切换内容，例如：

- Service / Interfaces：`↑↓ select · Enter open · Esc back · q quit · ?`
- Objects（树）：`↑↓/→← navigate · Enter open · Esc back`
- Interface：`Tab switch · Enter activate · r refresh · Esc back`
- Detail：`Tab move · Enter trigger · Esc back`
- Result：`↑↓ scroll · c copy · Esc back/stop`
- copy-as 弹层：`↑↓ choose · Enter copy · Esc cancel`

该栏属 `render` 的一部分，每屏始终绘制（含于 TestBackend 快照）。页面专属动作按钮（如 Result
的 `[Copy]`/`[copy as ▾]`）位于该栏之上。

- **Service**：`List` 列出 `ServiceInfo`（name + pid + process）；`Enter` → Objects。
- **Objects**：`tui-tree-widget` 渲染 `ObjectNode` 路径树；`Enter` → Interfaces。
- **Interfaces**：`List` 列出非标准接口名；`Enter` → Interface。
- **Interface**：左列纵向三块 `List`（methods / properties / signals；properties 直接展示
  `GetAll` 快照值）；选中项在右列出动作按钮：
  - method：`[调用]` `[监听]`
  - property：`[读取]` `[设置]` `[监听]`
  - signal：`[监听]`
  - `Tab` 在「三列」与「按钮栏」间切焦点。
- **Detail**：按动作类型出表单 + `[触发]` + `[copy as ▾]`（见 §9）。
- **Result**：`Paragraph`+滚动偏移（调用返回 pretty，可切 JSON 视图；监听则流式追加，每条一段
  dbus-send 式块）；footer `[Copy]` `[copy as ▾]`。`Copy` 经 `arboard` 写系统剪贴板。

## 9. 监听语义与 Detail 表单

各动作 → Detail → Result：

| 动作 | Detail 内容 | 触发后 |
|---|---|---|
| method 调用 | 每个 in-arg 一个 `tui-input`（标 name+签名），预填 busctl 风格；`[调用]` | 一次性 → Result（返回值 pretty/+JSON） |
| method 监听 | 显示 match-rule 预览；`[监听]` | **专用连接 BecomeMonitor + 客户端过滤**到该 (iface,member,path) 的 method_call/return/error → Result 流式 |
| property 读取 | 无输入（签名已知）；`[读取]` | 一次性 `Get` → Result（值 pretty/+JSON） |
| property 设置 | 签名已知（取自 introspect），单值 `tui-input`；`[设置]` | 一次性 `Set` → Result（成功/错误） |
| property 监听 | match-rule 预览；`[监听]` | 主连接 match-rule 订阅 `PropertiesChanged`，过滤到该属性 → Result 流式 |
| signal 监听 | match-rule 预览；`[监听]` | 主连接订阅该 signal → Result 流式 |

- method 监听需要 `BecomeMonitor`（特权）；若被总线拒绝 → 错误 toast（无法降级满足，因为看
  method_call/return/error 只有 BecomeMonitor 一条路）。
- 所有监听：Result 页 `Esc` 离开即停（§5）。

## 10. copy-as 生成

`tui/copy.rs` 由「操作描述符 + 类型化值」生成命令串，Detail 与 Result 页共用（Result 页的
copy-as 反映产生该结果的操作）：

| 操作 | dbus-send | busctl | qdbus | gdbus |
|---|---|---|---|---|
| method call | `--print-reply` + `type:value`（嵌套 best-effort + 提示其固有限制） | `call S O IFACE METHOD SIG args` | `qdbus S /O IFACE.METHOD args` | `gdbus call -e -d S -o /O -m IFACE.METHOD args` |
| property get | `... Properties.Get string:IFACE string:PROP` | `get-property S O IFACE PROP` | `qdbus ... getProperty ...` | `gdbus call ... Properties.Get` |
| property set | `... Properties.Set ...` | `set-property ...` | `qdbus ... setProperty ...` | `gdbus call ... Properties.Set` |
| monitor | `dbus-monitor "<rule>"` | `busctl monitor "<rule>"` | —（qdbus 无 monitor） | `gdbus monitor` |

- 某工具无法表达某操作时（如 qdbus 无 monitor、dbus-send 无法表达嵌套），该条目在该操作下
  **禁用/置灰并提示原因**。
- 入参取当前 Detail 表单里的值（未填用占位）；Result 页取产生该结果的实际值。
- copy-as 弹层选中某工具时**预览**其生成的命令串（一并供 TestBackend 快照覆盖，见 §13）。

## 11. 错误处理

- 核心 `error::Result` 的错误经 channel 回 UI 作 `Msg::Error`：以**非致命 toast / 状态行**展示，
  绝不崩 TUI。
- launch 连接失败：错误屏（显示 `busx:` 风格消息）+ `[重试]`/`[退出]`。
- 调用/设置失败：在 Result 页用红字显示 D-Bus 错误名 + 消息；copy-as 仍可用。
- worker 任务在 executor 上运行，单个任务的 panic 被执行器隔离、不波及 UI 线程；DBus 错误以
  `Result` 经 channel 回流。

## 12. 依赖

均为纯 Rust、无 C 依赖（除 `arboard` 可能用到系统剪贴板后端，按其默认特性）。小版本以
`cargo add` 时最新兼容版为准（同 v1 §13）。

| crate | 最新（2026-07，crates.io） | 用途 |
|---|---|---|
| `zbus` | 5.17（已在用） | async D-Bus（`async-io` 已开） |
| `zbus_xml` | 5.2 | 内省 XML 解析（取代 v1 的 `roxmltree`，移除该依赖） |
| `ratatui` | 0.30 | TUI 框架（0.30 起 widget 在拆出的 `ratatui-widgets`，依赖 `ratatui` 自动带入） |
| `crossterm` | 0.29 | 终端后端 + 事件 |
| `tui-tree-widget` | 0.24 | 对象树（ratatui 内置无 Tree） |
| `tui-input` | 0.15 | 文本输入（ratatui 内置无输入控件） |
| `arboard` | 3.6 | 系统剪贴板 |
| `async-global-executor` | 2.x | 跑异步任务（zbus 传递依赖，直引） |
| `flume` | 0.12 | UI↔worker channel（同步/异步双向） |
| `futures` | 0.3 | `select!` / `StreamExt` |

> ratatui↔crossterm、ratatui↔tui-tree-widget/tui-input 的小版本兼容在 `cargo add` 时由 cargo
> 解析确认。

## 13. 测试策略

**只做端到端**，不写单元测试；架构为可测性服务（核心与渲染分离、render 为纯函数）。

- **dbus 核心**：不单独测。CLI `ops/*` 迁移后仍调用同一核心，**现有 CLI e2e（对测试总线
  驱动 `busx` 二进制）即等价于核心的 e2e**——重构后保持全绿，核心即有覆盖。
- **CLI e2e**：沿用 v1 harness（一次性 spawn `dbus-daemon --session`，注册含嵌套类型/属性/
  信号/非 string 键 dict 的测试服务；`assert_cmd` 驱动二进制，断言 stdout + 退出码）。**重构
  守护契约**：所有现有 CLI 测试必须继续通过（human / `--json` / NDJSON 输出形状不变）。
- **TUI**：`ratatui::backend::TestBackend` 快照测试——喂合成按键序列驱动 `update`，`draw` 进
  内存 Buffer，字符化后与黄金快照比对（`insta`）。覆盖关键流程：service→objects→interfaces→
  interface 浏览、方法调用 detail/result、监听 result 流式、copy-as 菜单。监听/调用结果用
  **canned `Msg`** 注入（不连真总线），保证快照确定。
  - 语义上「按键 + 屏幕显示」已是 TUI 的有效端到端维度（不驱动真二进制/pty，但覆盖了
    状态机 + 渲染）。
- **copy-as**：纯函数，无 CLI 出口；按上述取向，优先用 TestBackend 快照覆盖（把生成的命令串
    随 copy-as 弹层预览的命令串一起快照），不单列单测。

## 14. 分阶段计划（每阶段独立可提交、可测）

0. **抽 `dbus/` 核心 + 迁 CLI**：建 `src/dbus/`（types + async conn + 各操作）；`ops/*` 改
   `block_on` 调核心 + 打印；`complete.rs`/`conn.rs` 收口；**无 TUI**，现有 CLI e2e 全绿。内省解析改用 `zbus_xml`（删 `src/introspect.rs`、移除 `roxmltree`），CLI `introspect` --json 经映射保持 v1 形状。
1. **TUI 骨架 + Service 页**：终端 setup/teardown、事件循环、channel、Elm 三层；`command` 改
   `Option`（裸 `busx` 进 TUI）；Service 列表（驱动 `list_names`）。
2. **浏览三连**：Objects（tui-tree-widget）+ Interfaces + Interface（三列 + 属性 `GetAll` 快照
   + `r` 刷新）。
3. **调用 / 读写**：方法调用 detail+result、属性 get/set detail+result（一次性，loading 态）。
4. **监听**：信号监听 / 属性监听（主连接 match-rule）/ 方法监听（专用连接 BecomeMonitor）→
   Result 流式 + **`Esc` 离开即停**。
5. **copy-as + 剪贴板**：四工具命令生成 + `arboard` 复制。
6. **打磨**：帮助叠层（`?`）、错误 toast/状态行、JSON 视图切换、边界与空态、TestBackend 快照
   补齐。

## 15. 非目标 / 后续

- 不做 `emit`、pcapng `capture`、`--host`/`--machine` 远程与容器总线、`ay` bytestring 视图、
  内嵌 jq（同 v1 §15）。
- 实时属性订阅（PropertiesChanged live-update）列为后续增强（当前仅快照 + 手动刷新）。
- 多监听并发（当前「离开即停」，任意时刻一个活跃监听）列为后续：若需要，引入监听列表/标签
  切换，扩展状态模型。
- `gdbus` 之外若有更多 copy-as 目标需求，再扩。
