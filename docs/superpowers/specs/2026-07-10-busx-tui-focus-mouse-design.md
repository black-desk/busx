<!--
SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>

SPDX-License-Identifier: MIT
-->

# busx TUI — 焦点模型重设计 + 鼠标支持

- 日期：2026-07-10
- 状态：设计稿（待评审）
- 关联：总设计 `2026-07-08-busx-tui-design.md`（本文修订其 §8 按键 / Interface 焦点部分）
- 分支：`tui-design`

## 1. 背景与问题

当前 Interface 屏的焦点模型（Phase 3 引入）被用户判定为"离谱"：

- `Tab` 在「当前列 ↔ 按钮栏」之间**切换**——从 methods 列按 Tab 跳到按钮栏，而不是下一个列。
- `Shift+Tab` 在三个列之间**循环**。
- 两者不互逆；`Tab` 的"跳按钮栏"语义出人意料；按钮栏作用于哪个 member 靠一个隐式的 `active_column` 跟踪，在列间 Tab 时会偷偷变、还会过期。

此外整个 TUI **不支持鼠标**。

## 2. 焦点重设计（Interface 屏）

Interface 屏布局不变：**左**（3 个纵向堆叠的列：methods / properties / signals）+ **右**（动作按钮栏，显示当前列所选 member 的动作）。只改焦点/按键语义，和 app 其它地方"Enter 前进、Esc 后退"一致：

- **Tab / Shift+Tab**：循环 3 个**列**（methods→properties→signals→methods，Shift+Tab 反向）。按钮栏**不**在这个环里。
- **↑ / ↓**：当前列里选 member。
- **Enter**：在列上 → 进入该 member 的动作按钮栏；在按钮上 → 触发动作（push Detail/Result，同现状）。
- **Esc**：在按钮栏 → 退回列；在列上 → 弹出 Interface 屏（回 Interfaces，同现状）。

按钮栏始终显示「当前列所选 member」的动作；用 Enter 进入、Esc 退出。**没有 ←/→**。

### 状态模型

- `focus: InterfaceFocus` 始终是三个列之一（`Methods`/`Properties`/`Signals`）——**删掉 `Buttons` 变体**。
- 新增 `in_buttons: bool`：是否聚焦在按钮栏（而非列的 member 列表）。
- **删掉 `active_column`**——`focus` 本身就是当前列；按钮栏永远反映 `focus` 列的所选 member，不会再"过期"。
- 行为：
  - `in_buttons == false`：↑↓ 移 `focus` 列的 `selected`；Tab/Shift+Tab 循环 `focus`；Enter → `in_buttons = true`。
  - `in_buttons == true`：↑↓ 移 `button_selected`；Enter 触发所选按钮；Esc → `in_buttons = false`；Tab/Shift+Tab 退出按钮栏并循环到下一列（`in_buttons = false` 再循环 `focus`）——宽容，避免在按钮栏里 Tab 被卡住。
- 渲染：按钮栏在 `in_buttons` 时高亮（聚焦态）；列在 `!in_buttons && focus==该列` 时高亮。

其它屏的焦点不变（Service/Objects/Interfaces 是单列表，↑↓+Enter+Esc；Detail 已是 Enter 一致：Tab 在字段↔触发间循环、Enter 触发；Result 只滚动）。本次只改 Interface 屏。

## 3. 鼠标支持（全局）

### 启用与事件

- `setup_terminal` 增加 `EnableMouseCapture`；`restore_terminal` 增加 `DisableMouseCapture`。
- `CrosstermSource` 把鼠标事件转成 `Msg::Mouse(crossterm::event::MouseEvent)`（与 Key/Resize 并列）。

### 命中测试

- `State` 增加 `click_targets: Vec<(Rect, ClickTarget)>`——每个可交互目标的矩形 + 身份。
- `ClickTarget` 枚举：`ServiceRow(usize)` / `ObjectsRow(usize)` / `InterfacesRow(usize)` / `MethodRow(usize)` / `PropertyRow(usize)` / `SignalRow(usize)` / `ActionButton(usize)` / `DetailField(usize)` / `DetailTrigger` / `PopupTool(usize)` 等（按屏枚举）。
- **render 记录这些目标**：render 在放置每个可交互 widget 时，把它的 `Rect` + `ClickTarget` 推进一个 **out-param**（`&mut Vec<(Rect, ClickTarget)>`）；render 对 `State` 仍是只读（保持纯函数——可见渲染不变，只多产出一份命中表）。事件循环在 `draw` 后把这份表存进 `State.click_targets`，供 `update` 的鼠标处理读。
  - 这避免了"render 往 State 写"的副作用：render 读 `&State`、写 out-param；写 `State.click_targets` 的是循环（它本来就在 `update` 里改 State）。
- `update` 收到 `Msg::Mouse(Down(Left), col, row, ..)`：在 `state.click_targets` 里找包含 `(col,row)` 的目标 → 执行等价动作：
  - 列表行（Service/Objects/Interfaces/Method/Property/Signal 行）→ 选中它（设 `selected`；若是 Interface 的某列行，同时把 `focus` 切到该列、`in_buttons=false`）。
  - `ActionButton(i)` → 等价于"选中第 i 个按钮 + Enter"（触发动作）。
  - `DetailField(i)` → 聚焦该字段（`field_selected = i`, `focus = Field`）。
  - `DetailTrigger` → 等价于触发（同 Enter on trigger）。
  - `PopupTool(i)` → 选中该工具（同 ↑↓），**不**自动复制（复制要明确点/Enter；或双击复制——见下）。
- 滚轮（`ScrollDown`/`ScrollUp`）：滚动当前可滚动区（Result 的 `scroll`；长列表若有滚动也滚）。
- 右键/拖拽：本期不做（YAGNI）。

### 交互

- 列表行（Service/Objects/Interfaces/Method/Property/Signal）：左键单击 = 选中（设 `selected`；Interface 列行同时把 `focus` 切到该列、`in_buttons=false`）。下钻/激活仍用 Enter（键盘）——鼠标只负责选中，避免误触下钻。
- `ActionButton(i)` / `DetailTrigger`：左键单击 = 激活（等价 Enter）。
- `DetailField(i)`：左键单击 = 聚焦该字段。
- `PopupTool(i)`：左键单击 = 选中预览（等价 ↑↓）；复制用 Enter。
- 滚轮（`ScrollDown`/`ScrollUp`）：滚动当前可滚动区（Result 的 `scroll`）。
- 右键/拖拽/悬停/双击：本期不做。

## 4. 测试

- **焦点模型**：沿用 TestBackend + insta，按键驱动 `update`（Tab 循环列、Enter 进按钮、Esc 退、↑↓ 选）。快照 + 行为断言（`focus`/`in_buttons`/`selected`/`button_selected`）。
- **鼠标**：`Msg::Mouse` 是普通数据，可构造。测法（不脆）：
  1. `term.draw(|f| render(f, &state, &mut targets))` → render 产出 `targets`；存进 `state.click_targets`。
  2. 从 `click_targets` 里找目标（如 `MethodRow(1)`），读它的 `Rect`。
  3. 喂 `Msg::Mouse(Down(Left), col=rect.x+1, row=rect.y+1)` → `update` 命中 → 选中/激活。
  4. 断言状态变化（`selected==1` / 触发了 Effect / 屏幕栈变了）。
  - 坐标从 `click_targets` 读、不硬编码 → 改布局不会让测试乱挂。
- 覆盖：点选 Service/Objects/Interfaces 行、点 Interface 列行（含切列）、点 ActionButton 激活、点 DetailField/Trigger、点 PopupTool、滚轮滚动 Result。
- 鼠标的真终端行为（crossterm 鼠标捕获本身）不在自动测试内——人工验收。

## 5. 范围与非目标

- **做**：Interface 屏焦点重设计（Tab 循环列、Enter 进按钮、Esc 退、删 active_column）；全局鼠标（点选/点按钮/滚轮，含双击激活）；焦点 + 鼠标的 TestBackend 测试。
- **不做**：右键菜单、拖拽、鼠标悬停高亮、双击、触摸板手势；其它屏的焦点改造（它们已 OK）。
- 不改 Interface 屏布局（3 列 + 右按钮栏）。

## 6. 实现注记

- `InterfaceFocus` 删 `Buttons` 变体；`InterfaceScreen` 删 `active_column`、加 `in_buttons: bool`。受影响的 Phase 3–5 测试字面量同步更新。
- `Msg::Mouse` 加入；`CrosstermSource::next` 把 `Event::Mouse` 转成它（`non_mouse` 改名/扩展）。
- render 签名：`render(frame, &State, &mut Vec<(Rect, ClickTarget)>)`；`render_to_string` 测试助手与 `run_loop` 的 draw 闭包相应更新。
- `Effect` 无需新增（鼠标激活复用现有 Enter 路径产生的 Effect）。
