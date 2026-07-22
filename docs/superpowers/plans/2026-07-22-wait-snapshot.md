<!--
SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
SPDX-License-Identifier: MIT
-->

# wait_for_snapshot Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 用 insta 全屏快照匹配替代 `tui_pty.rs` 里全部 45 个 `wait_for_text`，消除子串假阳性；删除已无用户的 `wait_for_text`。

**Architecture:** tuiprobe 导出 `wait_for_snapshot!` 宏（在调用点展开取 `file!()/module_path!()/line!()`，保证快照落调用方目录）+ `TuiProbe::wait_for_snapshot_at` 方法；方法循环 `drain` → 调 `insta::_macro_support::assert_snapshot`（返回 `Result`，不 panic），复用 insta 的 filter（thread-local `pty_filter` Settings）、snapshot 加载/比较、`.snap.new` 写入、`cargo insta review`。`Ok` = 屏幕已匹配期望 → 返回；`Err` = 还没到 → 继续轮询；超时 → `Error::Timeout`。

**Tech Stack:** Rust (edition 2024), insta 1.48 (`filters` feature), tuiprobe, busx, ratatui/crossterm.

**关键风险（务必知悉）：** `insta::_macro_support` 是 `#[doc(hidden)]`，insta 文档明说"不保证跨 minor 版本稳定"。busx 已 `pin insta = "1"`；每次升 insta minor 版本后必须重跑本计划 Task 7 的验证。

**参考证据（spec）：** `docs/superpowers/specs/2026-07-22-wait-snapshot-design.md`

---

## File Structure

- **Modify** `crates/tuiprobe/Cargo.toml` — `[dependencies]` 加 `insta = { version = "1", features = ["filters"], default-features = false }`。
- **Modify** `crates/tuiprobe/src/harness.rs` — 删 `wait_for_text` / `wait_for_text_timeout`；加 `wait_for_snapshot_at` 方法。
- **Modify** `crates/tuiprobe/src/lib.rs` — `#[macro_export]` 导出 `wait_for_snapshot!` 宏。
- **Modify** `crates/busx/tests/tui_pty.rs` — 把 45 处 `probe.wait_for_text("...")` 换成 `wait_for_snapshot!(&mut probe, "...")`；`pty_filter()` 的 filter 规则保留（assert_snapshot 会读它）。

---

## Task 1: tuiprobe 加 insta 依赖

**Files:**
- Modify: `crates/tuiprobe/Cargo.toml`

- [ ] **Step 1: 加依赖**

`[dependencies]` 段加（保留现有 alacritty_terminal/portable-pty/vte）：

```toml
[dependencies]
alacritty_terminal = "0.26"
portable-pty = "0.8"
vte = "0.15"
insta = { version = "1", features = ["filters"], default-features = false }
```

把现有 `[dev-dependencies] insta = "1"` 删掉（已提升为正式依赖）。

- [ ] **Step 2: 验证编译**

Run: `cargo build -p tuiprobe`
Expected: 成功（insta 被拉入 tuiprobe 的正式依赖）。

- [ ] **Step 3: Commit**

```bash
git add crates/tuiprobe/Cargo.toml
git commit -m "chore(tuiprobe): promote insta to a regular dependency

Needed for wait_for_snapshot, which reuses insta's snapshot
matching (filter + load + compare + .snap.new) instead of
fragile substring waits."
```

---

## Task 2: spike — 验证 `assert_snapshot` 调用能跑通

> 这是整个方案的技术前提：确认 `insta::_macro_support::assert_snapshot` 能从 tuiprobe 调用、签名匹配、返回 Result、并能写/读 `.snap`。先写一个一次性 spike，跑通后再清理。

**Files:**
- Create (临时): `crates/tuiprobe/tests/spike_assert.rs`

- [ ] **Step 1: 写 spike 测试**

```rust
// crates/tuiprobe/tests/spike_assert.rs
// SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
// SPDX-License-Identifier: MIT

//! 一次性验证：从外部 crate 调 insta::_macro_support::assert_snapshot 能否
//! 跑通（读/写 .snap、返回 Result、apply filter）。跑通后本文件删除。

use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    // env! 在调用 crate 求值；tuiprobe 的 tests 也算调用 crate。
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

#[test]
fn spike_assert_snapshot_matches() {
    let screen = "hello";
    let r = insta::_macro_support::assert_snapshot(
        ("spike_name", screen).into(),
        workspace_root().as_path(),
        "spike_assert_snapshot_matches",
        module_path!(),
        file!(),
        line!(),
        "screen",
    );
    // 首次：无 .snap → Err（insta 写 .snap.new pending）。再跑（接受后）→ Ok。
    eprintln!("spike result: {r:?}");
}
```

- [ ] **Step 2: 跑 spike，生成 pending**

Run: `cargo test -p tuiprobe --test spike_assert -- --nocapture`
Expected: 测试失败（首次无 `.snap`），但在 `crates/tuiprobe/tests/snapshots/spike__spike_name.snap.new`（或 workspace 级 snapshots 目录）写出 pending 文件，`eprintln` 打印 `Err(...)`。stderr 打印 `spike result: Err(...)`。

- [ ] **Step 3: 接受 pending，再跑确认 Ok**

Run: `cargo insta review`（接受 spike 快照），然后 `cargo test -p tuiprobe --test spike_assert -- --nocapture`
Expected: 测试通过，打印 `spike result: Ok(())`。

- [ ] **Step 4: 改 screen 再跑，确认不匹配返回 Err**

把 `let screen = "hello"` 改成 `let screen = "world"`，跑：
Run: `cargo test -p tuiprobe --test spike_assert -- --nocapture`
Expected: 失败，打印 `Err(...)`，写新的 `.snap.new`（diff 显示 hello→world）。验证"不匹配 = Err"成立。然后 `git checkout` 还原 screen，`cargo insta review` 拒绝新 pending。

- [ ] **Step 5: 删 spike，不 commit**

```bash
rm crates/tuiprobe/tests/spike_assert.rs
rm -f crates/tuiprobe/tests/snapshots/spike__*.snap*
```
不提交（spike 仅验证可行性）。

> **若 spike 失败**（签名不匹配 / workspace_root 推导错 / snapshot 落错目录）：回到 spec，与维护者讨论改用 `assert_snapshot!` + `catch_unwind` 的备选方案。**不要在 spike 失败时继续后续 task。**

---

## Task 3: tuiprobe 实现 `wait_for_snapshot_at` + 导出宏

**Files:**
- Modify: `crates/tuiprobe/src/harness.rs`
- Modify: `crates/tuiprobe/src/lib.rs`

- [ ] **Step 1: 写失败测试（用真实 PTY + 一个最小 echo TUI 不现实；改用直接测方法的匹配逻辑）**

> 说明：`wait_for_snapshot_at` 依赖 insta 的 `.snap` 文件 + PTY，难以纯单测。Task 4 在 busx 集成测试里做端到端验证（TDD 的"失败测试"放在那里，因为它才是真实使用场景）。本 task 先实现方法，Task 4 验证。这是对"测试工具本身难以单测"的合理妥协，记于 spec。

- [ ] **Step 2: 在 harness.rs 实现 `wait_for_snapshot_at`**

在 `impl TuiProbe` 内（`wait_for_with_timeout` 附近）加：

```rust
/// 等待当前屏幕与名为 `name` 的 insta 快照一致。复用 insta 的 filter
/// （当前 thread-local `Settings`）、snapshot 加载/比较、`.snap.new` 写入。
///
/// 与 [`wait_for_text`](Self::wait_for_text) 的区别：这里匹配的是整屏
/// （经 filter），不会因子串在中间态提前出现而假阳性。匹配 = `Ok`；
/// 未匹配 = 继续轮询；超时 = [`Error::Timeout`]。
///
/// `assertion_file` / `module_path` / `assertion_line` 由 `wait_for_snapshot!`
/// 宏在调用点填入，保证快照落到调用方（busx 测试）目录。
///
/// **注意**：用了 `insta::_macro_support`（`#[doc(hidden)]`，跨 minor 版本
/// 不稳定）。调用方 crate 须 pin insta 版本并在升级时回归本方法。
pub fn wait_for_snapshot_at(
    &mut self,
    name: &str,
    workspace: &std::path::Path,
    assertion_file: &str,
    module_path: &str,
    assertion_line: u32,
) -> Result<()> {
    let deadline = Instant::now() + self.timeout;
    // workspace 由 wait_for_snapshot! 宏在调用点用 insta::_get_workspace_root!
    // 求值后传入 —— env!("CARGO_MANIFEST_DIR") 必须在调用方 crate 求值，
    // 否则会落到 tuiprobe 自己的 manifest dir，快照写错目录。
    loop {
        self.drain_into_emulator();
        let contents = self.screen.contents();
        let matched = insta::_macro_support::assert_snapshot(
            (name, contents.as_str()).into(),
            workspace,
            "wait_for_snapshot_at",
            module_path,
            assertion_file,
            assertion_line,
            "screen",
        )
        .is_ok();
        if matched {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(Error::Timeout {
                what: format!("snapshot: {name}"),
                screen: self.screen.contents(),
            });
        }
        std::thread::sleep(self.poll_interval);
    }
}
```

- [ ] **Step 3: 在 lib.rs 导出 `wait_for_snapshot!` 宏**

在 `crates/tuiprobe/src/lib.rs` 末尾加：

```rust
/// 等待 `probe` 的屏幕匹配名为 `name` 的 insta 快照。等价于
/// `probe.wait_for_snapshot_at(name, file!(), module_path!(), line!())`，
/// 但宏在调用点展开，让快照落到调用方（测试）目录而非 tuiprobe。
///
/// ```no_run
/// # use tuiprobe::{TuiProbe, wait_for_snapshot};
/// # let mut probe = TuiProbe::new(80, 24).unwrap();
/// wait_for_snapshot!(&mut probe, "my_screen").unwrap();
/// ```
#[macro_export]
macro_rules! wait_for_snapshot {
    ($probe:expr, $name:expr $(,)?) => {
        $probe.wait_for_snapshot_at(
            $name,
            ::insta::_get_workspace_root!().as_path(),
            file!(),
            module_path!(),
            line!(),
        )
    };
}
```

- [ ] **Step 4: 验证 tuiprobe 编译**

Run: `cargo build -p tuiprobe`
Expected: 成功。

- [ ] **Step 5: Commit**

```bash
git add crates/tuiprobe/src/harness.rs crates/tuiprobe/src/lib.rs
git commit -m "feat(tuiprobe): add wait_for_snapshot reusing insta matching

wait_for_snapshot_at drives insta's _macro_support::assert_snapshot
in a poll loop: Ok = screen matches the snapshot (return), Err =
not there yet (keep polling), timeout = Error::Timeout. This reuses
insta's filter/snapshot-load/compare/.snap.new pipeline wholesale,
so no fragile substring match and no hand-rolled filter.

wait_for_snapshot! macro captures file!()/module_path!()/line!() at
the call site so snapshots land in the caller's (busx test) dir."
```

---

## Task 4: 试点迁移 `get_property`（端到端 TDD 验证）

> 用 `get_property`（之前删-sleep 失败的那个）做试点，证明 `wait_for_snapshot!` 在真实 PTY 测试里能消除假阳性。这是整个改动的"失败测试→实现→通过"。

**Files:**
- Modify: `crates/busx/tests/tui_pty.rs`（仅 `get_property` 函数）

- [ ] **Step 1: 把 get_property 的 wait 换成 wait_for_snapshot!**

`get_property` 函数体里，把：

```rust
    probe.wait_for_text("0.5").unwrap();
    insta::assert_snapshot!(probe.screen_contents());
```

改成：

```rust
    wait_for_snapshot!(&mut probe, "get_property_result").unwrap();
    insta::assert_snapshot!(probe.screen_contents());
```

并在文件顶部 `use` 区确认有 `use tuiprobe::{KeyCode, MouseButton, ScrollDirection, TuiProbe};` —— 补上 `wait_for_snapshot` 宏的导入。宏用 `#[macro_export]`，导入方式：

```rust
use tuiprobe::wait_for_snapshot;
```

- [ ] **Step 2: 跑测试，首次生成快照 pending**

Run: `cargo test -p busx --test tui_pty get_property`
Expected: 失败（首次无 `.snap`），写出 `crates/busx/tests/snapshots/tui_pty__get_property_result.snap.new`。

- [ ] **Step 3: 审查 pending（实现者职责，见 Task 7 标准）**

读 `.snap.new`，确认它是 Result 屏（面包屑含 `> Result`，框内 `0.5`），**不是** Interface 视图。若内容是 Interface 视图 → bug（操作没到结果屏），停下排查，不进 review。

- [ ] **Step 4: 接受快照，确认通过**

Run: `cargo insta review`（接受 `get_property_result`），再 `cargo test -p busx --test tui_pty get_property`
Expected: 通过。

- [ ] **Step 5: 关键验证 — 删该测试路径上的 sleep，确认仍稳**

本 task 暂不改 harness sleep（Task 6 统一删）。但手动确认：`wait_for_snapshot!` 在结果屏（非中间态）匹配，证明假阳性已消除（之前 `wait_for_text("0.5")` 的假阳性根因是 `0.5` 在 Interface 视图就存在；现在匹配整屏 Result，不会假阳性）。

- [ ] **Step 6: Commit**

```bash
git add crates/busx/tests/tui_pty.rs crates/busx/tests/snapshots/tui_pty__get_property_result.snap
git commit -m "test(busx): migrate get_property to wait_for_snapshot

Pilot for the full migration: replaces the fragile
wait_for_text(\"0.5\") (whose target existed on-screen before the
operation, causing false positives) with a full-screen snapshot
match against the Result screen."
```

---

## Task 5: 迁移剩余 44 处 `wait_for_text`

**Files:**
- Modify: `crates/busx/tests/tui_pty.rs`

> 对每个 `wait_for_text("X")`，替换为 `wait_for_snapshot!(&mut probe, "<unique_name>")`，name 选稳定且唯一的（等同一屏的多个 wait 可同名，但跨尺寸不同屏要不同 name）。模式与 Task 4 完全一致。

- [ ] **Step 1: 列出全部 45 处 + 规划 name**

Run: `grep -n "wait_for_text" crates/busx/tests/tui_pty.rs`
为每处分配快照 name（如 `service_list`、`interface_volume`、`objects_list`、`drill_path`、`filter_scroll`、`result_call_fd`、`result_set_ok`、`listen_armed` 等）。等同一屏（同尺寸）的复用同名。

- [ ] **Step 2: 分批替换（建议按测试函数，每批 3–5 个函数）**

每批替换后：

Run: `cargo test -p busx --test tui_pty <相关测试>`
Expected: 首次失败生成 `.snap.new` → 审查 → `cargo insta review` 接受 → 再跑通过。

替换模式（每处 `wait_for_text` → `wait_for_snapshot!`）：

```rust
// 前
probe.wait_for_text("org.busx.ScrollA").unwrap();
// 后
wait_for_snapshot!(&mut probe, "service_list").unwrap();
```

- [ ] **Step 3: 每批 Commit**

每批一个 commit，message 形如 `test(busx): migrate <范围> waits to wait_for_snapshot`。

- [ ] **Step 4: 全量跑，确认无残留 wait_for_text**

Run: `grep -n "wait_for_text" crates/busx/tests/tui_pty.rs`
Expected: 无输出（全部迁移完）。

Run: `cargo test -p busx --test tui_pty`
Expected: 全部通过。

---

## Task 6: 删除 `wait_for_text` / `wait_for_text_timeout`

**Files:**
- Modify: `crates/tuiprobe/src/harness.rs`

> 顺序：Task 5 确认 busx 测试已无任何 `wait_for_text` 调用、`cargo build` 通过后，才能删方法（否则编译失败）。

- [ ] **Step 1: 删两个方法**

从 `crates/tuiprobe/src/harness.rs` 的 `impl TuiProbe` 删除：
- `pub fn wait_for_text(&mut self, text: &str) -> Result<()>` 整个方法（含 doc）。
- `pub fn wait_for_text_timeout(&mut self, text: &str, timeout: Duration) -> Result<()>` 整个方法。

保留 `wait_for` / `wait_for_with_timeout`（通用原语，`wait_for_snapshot_at` 不依赖它们但保留无害；若 clippy 报未使用再评估）。

- [ ] **Step 2: 验证编译 + 全量测试**

Run: `cargo build -p tuiprobe && cargo test -p busx --test tui_pty`
Expected: 成功 + 全部通过。

- [ ] **Step 3: Commit**

```bash
git add crates/tuiprobe/src/harness.rs
git commit -m "refactor(tuiprobe): remove wait_for_text/wait_for_text_timeout

No users remain after the busx migration to wait_for_snapshot.
YAGNI — drop them rather than keep a fragile substring API around."
```

---

## Task 7: 删 harness sleep + 全量回归（flaky 改善验证）

**Files:**
- Modify: `crates/tuiprobe/src/harness.rs`

> 前置结论（spec/调试记录）：`wait_for_snapshot` 全屏匹配不会在中间态假阳性，所以关键点的 `sleep(20ms)` 同步屏障不再必要。本 task 删除并验证 flaky 改善。

- [ ] **Step 1: 删三处 sleep（保留 drain）**

`send_key_with_mods` / `mouse_click` / `mouse_scroll` 里，删 `std::thread::sleep(Duration::from_millis(20));` 一行，保留其后的 `self.drain_into_emulator();`。

- [ ] **Step 2: 连跑 5 次，确认稳定（不再 flaky）**

Run:
```bash
for i in 1 2 3 4 5; do cargo test -p busx --test tui_pty 2>&1 | grep "test result"; done
```
Expected: 5 次全部 `23 passed; 0 failed`（或当前迁移后的总数），无间歇失败。若出现失败 → 回到 systematic-debugging：是某 wait_for_snapshot 的目标屏选错（假阳性复发），还是真 flaky。

- [ ] **Step 3: 全量覆盖率回归**

Run: `cd crates/busx && cargo llvm-cov --all-features --workspace 2>&1 | tail -5`
Expected: 成功（exit 0），覆盖率不低于现状（Lines ≥ 78%）。

- [ ] **Step 4: Commit**

```bash
git add crates/tuiprobe/src/harness.rs
git commit -m "perf(tuiprobe): drop the 20ms per-input sleep

wait_for_snapshot matches the full screen, so it no longer false-
positives on an intermediate state — the sleep that acted as an
input-sync band-aid is no longer needed. Removes ~20ms per key
press and reduces parallel-load-induced flakiness."
```

---

## Task 8: 快照逐个验证（实现者交付，见 spec "快照验证"）

**Files:** `crates/busx/tests/snapshots/*.snap`（全部新生成的 wait 快照）

- [ ] **Step 1: 逐个读 wait 快照，核对语义**

对每个 `wait_for_snapshot!` 对应的 `.snap`，确认它记录的是"该 wait 调用时点 busx 理应处于的屏"：
- 结果类（`*_result`）：面包屑含 `> Result`，含返回值。
- 列表类（`service_list` 等）：含预期的服务行。
- armed 类（`listen_*`）：含 `Call`/`Listen` 已激活标志。

若任一快照是中间态 / 报错屏 / 空屏 → bug，定位（操作未到？wait 时机？busx 渲染？）并修，不让错误快照进入提交。

- [ ] **Step 2: 确认跨尺寸的同名快照各自正确**

64×12 / 80×20 / 80×24 三种尺寸下的同名快照（若存在）布局各自正确。

- [ ] **Step 3: 全量 `cargo insta review` 走一遍**

确保无遗留 `.snap.new`。

---

## Self-Review（计划作者自查）

1. **Spec 覆盖**：spec 的"全部迁移"→ Task 5；"删 wait_for_text"→ Task 6；"快照验证"→ Task 8；"复用 insta"→ Task 2/3；"filter 共享"→ 复用 insta 无需自定义（pty_filter 保留）。覆盖完整。
2. **Placeholder 扫描**：Task 5 的 name 规划是"列出 + 规划"（有 grep 命令），非空 placeholder。insta API 调用基于 1.48 实读签名（runtime.rs:846、macros.rs:373-386），非臆造。
3. **类型一致**：`wait_for_snapshot_at(name, file, module, line)` 签名在 Task 3 定义、Task 4/5 使用，一致。宏 `wait_for_snapshot!($probe, $name)` 跨 task 一致。
4. **已知风险**：`_macro_support` doc(hidden)——Task 2 spike 先验证，失败则回退备选（catch_unwind + 公开宏），已在 Task 2 Step 5 标注。
