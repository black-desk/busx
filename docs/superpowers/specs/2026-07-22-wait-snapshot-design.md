<!--
SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
SPDX-License-Identifier: MIT
-->

# `wait_for_snapshot` — 关键同步点的全屏快照等待

## 背景

`tui_pty` 测试用 `wait_for_text(子串)` 做关键步骤的同步。当目标子串在
**操作之前就已在屏幕上**时，`wait_for_text` 会假阳性通过——典型是
`get_property` 的 `wait_for_text("0.5")`，而 `0.5` 是 properties 栏
`volume` 的值，从 drill 完成起就一直渲染着。于是 wait 立即匹配，掩盖了
"操作根本没把状态推进到结果屏"。harness 里的 `sleep(20ms)` 长期作为
输入间的同步屏障掩盖了这个缺陷。

根因已通过 systematic-debugging 实证：
- `app.rs` 事件循环是"一事件 → update → 下一帧 draw"，无单帧批量处理。
- 三测试对照唯一相关变量是"wait 目标是否操作前就在屏"：
  `call`("/dev/null")、`set`("ok") 操作前不在屏 → 过；
  `get`("0.5") 操作前在屏 → 挂。

## 目标

**所有** `wait_for_text` 同步点都用全屏快照匹配替代，彻底消除假阳性、
统一同步机制（不再两种 wait 并存）。复用 insta 的 snapshot 文件 +
filter + review 流程。由 tuiprobe 提供 `wait_for_snapshot`。用户已知
并接受快照量较大（~20–30 个独立快照），由实现者逐个验证快照正确性。
**删除** `wait_for_text` / `wait_for_text_timeout`——全换后它们当前
无用户（tuiprobe 未发布，busx 是唯一使用者且已全部迁移）。YAGNI，不
保留给假想的未来用户。删除是迁移的**最后一步**：所有调用迁完、
`tui_pty.rs` 编译通过后才能删，否则测试编译失败。

## 非目标

- 不删除所有 `sleep`：仅关键点因 `wait_for_snapshot` 不再依赖它；
  非关键点的 sleep 去留后续单独评估。
- 不改 busx 的业务逻辑。

## 架构决策

**`wait_for_snapshot` 作为 `tuiprobe::TuiProbe` 的方法；`insta` 从
`dev-dependency` 提为正式 `dependency`。**

理由：
- tuiprobe 的定位是 TUI **测试框架**（description："PTY-based
  integration testing for TUI applications"；category：
  `development-tools::testing`）。snapshot 测试库（insta）是同类核心
  依赖，对目标用户（写测试的人）无额外负担，合理。
- 放进 tuiprobe 能直接复用 insta 的 thread-local `Settings`（filter）
  与 snapshot 加载 / 写 pending 的 API，实现比"在 busx 测试侧手写
  insta 交互"更干净，也避免 filter 规则两处定义漂移。

## API

```rust
impl TuiProbe {
    /// 等待当前屏幕（经当前 thread-local insta `Settings` 的 filter）
    /// 与名为 `name` 的已记录快照一致。
    ///
    /// - `.snap` 不存在时：把当前屏写成 `.snap.new`（pending）并视为
    ///   等待成功——与 `assert_snapshot!` 首次行为一致，走
    ///   `cargo insta review` 接受。
    /// - 超时：把当前屏写成 `.snap.new`，让 review 并排显示差异，而非
    ///   裸超时 + 一张屏。
    pub fn wait_for_snapshot(&mut self, name: &str) -> Result<()> { ... }
}
```

用法（busx 测试）：

```rust
// 替换脆弱的 probe.wait_for_text("0.5").unwrap();
probe.wait_for_snapshot("get_property_result").unwrap();
```

## 机制

1. **加载期望**：读取 `tests/snapshots/<module>__<name>.snap` 的内容
   （insta 存的是 filter 后的内容）。
2. **比较**：复用 tuiprobe 已有的 `wait_for` 循环——每轮 `drain` →
   取当前屏 → 套用当前 thread-local insta `Settings` 的 filter（即测试
   里 `pty_filter().bind_to_scope()` 设的那套）→ 与期望比较。因 wait
   和 `assert_snapshot!` 读同一 thread-local Settings，filter 规则
   天然一致，不会漂移。
3. **首次**（`.snap` 不存在）：写当前屏为 `.snap.new`（pending），视为
   等待成功。
4. **超时**：写当前屏为 `.snap.new`，让 review 显示 diff。

filter 与现有 `pty_filter()`（PID / PROC / SOCKET+GUID）完全同一套，
无需在第二处维护。

## 迁移范围（全部 wait_for_text）

busx `tui_pty.rs` 里的 **45 个 `wait_for_text` 调用**全部迁移到
`wait_for_snapshot`。去重后 14 个不同目标字符串，但跨终端尺寸
（64×12 / 80×20 / 80×24）与上下文，独立快照约 **20–30 个**——等同一
屏幕状态的多个 wait 共用同一个快照（同名）。

迁移要点：
- 每个 wait 点选一个稳定、唯一的快照名（如 `service_list`、
  `interface_volume`、`get_result`）。
- 多个 wait 等同一屏（同尺寸）→ 复用同名快照，不重复生成。
- 跨尺寸的同逻辑状态 → 各自一个快照（布局不同）。

## 快照验证（核心交付）

~20–30 个 wait 快照生成后，**由实现者逐个验证**（用户不盲审）：每个
快照必须对应"该 wait 调用时点 busx 理应处于的屏幕状态"。检查：
- 内容符合测试语义（如 get 的结果快照是 Result 屏含返回值，不是
  Interface 屏；listen 快照是已 armed 状态等）。
- 不是中间态 / 报错屏 / 空屏。
- 跨尺寸的同名快照各自正确。

若某快照记录了错误态 → 视为 bug，定位（操作未到达？wait 时机？busx
渲染问题？）并修，不让错误快照进入 `cargo insta review`。全部验证通过
后，再交用户 review 接受。

## 验证

1. 实现后**删除 harness 关键点的 `sleep(20ms)`**，跑 `cargo test
   --test tui_pty`，确认迁移后的测试在无 sleep 下仍稳定通过——证明
   `wait_for_snapshot` 既消除假阳性、又不再依赖 sleep。
2. 连跑多次 + 全量 `cargo llvm-cov --all-features --workspace` 确认
   tui_pty flaky 改善。
3. `cargo insta review` 审查首批 wait 快照（含上述人工审查）。

## 开放实现细节（writing-plans 阶段定）

- insta 是否暴露"对任意字符串应用当前 Settings filter"的公开 API；
  若无，从 `pty_filter()` 抽出共享的 filter-apply，供 wait 与 assert
  复用。
- 写 `.snap.new` 的具体方式（insta 公开 API vs 手动按 insta 文件格式
  写 frontmatter+content，命名 `<module>__<name>.snap.new`）。
