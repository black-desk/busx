<!--
SPDX-FileCopyrightText: 2026 Chen Linxian <me@black-desk.cn>

SPDX-License-Identifier: MIT
-->

# 贡献指南

[en](CONTRIBUTING.md) | zh_CN

> [!WARNING]
>
> 早期开发阶段：代码**从未经过人工审核**，CLI 介面也许会频繁变动。

为 `busx` 贡献的约定。

## 目录

- [拉取代码](#拉取代码)
- [构建与运行](#构建与运行)
- [测试](#测试)
- [代码风格](#代码风格)
- [文档](#文档)
- [提交信息规范](#提交信息规范)
- [AI 辅助的变更](#ai-辅助的变更)
- [许可证与 REUSE](#许可证与-reuse)

## 拉取代码

`.format` 子模块提供共享的 `EditorConfig` / `prettier` 配置，因此 clone 时带上
`--recurse-submodules`：

```bash
git clone --recurse-submodules https://github.com/black-desk/busx.git
```

## 构建与运行

Cargo workspace（根 `Cargo.toml` 是虚拟 manifest），代码在
`crates/busx/`，测试夹具在 `crates/testbus/`。所有 `cargo`
命令都在**仓库根目录**运行：

```bash
cargo build
cargo run -- <args>               # 例如 cargo run -- list
cargo run -- --help               # 查看 CLI
cargo run                         # 进入 TUI
```

## 测试

**测试原则：只在公开接口层做端到端测试，不对内部模块写单元测试。**

`busx` 是纯二进制 crate（根 `Cargo.toml` 没有 lib
target），只暴露两个公开面：CLI 子命令的输入 / 输出，以及 TUI 的交互 / 渲染。每个测试都从其中之一进入——
`crates/busx/tests/` 用 `assert_cmd` 起真正的 `busx`
二进制；TUI 测试在 PTY 里把真正的 `busx` 当子进程跑，`tuiprobe`
负责喂键鼠事件并读取渲染输出。内部模块（`dbus/`、`value/`、`ops/`、……）不写独立单元测试，其行为通过公开面间接验证。
**如果一个测试需要 `pub(crate)` 或更窄可见性才能写，那它放错了地方**——挪进
`tests/`，用 CLI / TUI 输出来验证。

测试分两层：**集成测试**（`crates/busx/tests/`）和 **TUI 快照测试**
（`crates/busx/tests/tui_pty.rs`，经 `tuiprobe`），两者都由 `testbus` 夹具驱动。

### testbus 夹具

`testbus::bus_owned()` 在后台起一个独立的 `dbus-daemon`，注册名为
`org.busx.Test` 的测试服务并返回其地址。集成测试通过 `--address` 把 `busx`
指向它，因此**不依赖系统 / 会话 bus 状态**，结果是确定性的：

```rust
let addr = testbus::bus_owned().address.clone();
Command::cargo_bin("busx")?
    .args(["--address", &addr, "list"])
    ...
```

典型用法见 `crates/busx/tests/list.rs`。

### 快照测试（insta）

TUI 渲染逻辑用 [`insta`][insta] 做快照测试，期望快照在
`crates/busx/tests/snapshots/*.snap`。每个测试用 `tuiprobe`
起一个真实终端，跑真正的 `busx`（连到 `testbus`
私有 bus），驱动到目标状态后截屏——走完整条 `main` → CLI → crossterm → ratatui →
`render` 管线。`wait_for_snapshot!`
轮询直到屏幕匹配已有的期望快照（收敛前的中间帧永不持久化），随后
`insta::assert_snapshot!` 断言新帧。

[insta]: https://insta.rs

当你**有意**改动 TUI 渲染（布局、文案、配色），快照测试会失败——这是预期的。接受新输出：

```bash
cargo install cargo-insta          # 一次性安装
cargo insta accept                 # 接受所有新快照
cargo insta review                 # 逐个审核，更安全
```

绝不提交你并非有意的快照变更；若是无意回归，先去查 `render.rs` /
`update.rs`，别盲目 accept。

CI 通过 [`black-desk/workflows/rust`][wf-rust] 上报测试覆盖率到
[codecov][codecov]。请补充和你的改动相关的测试用例。

[wf-rust]: https://github.com/black-desk/workflows
[codecov]: https://codecov.io/github/black-desk/busx

## 代码风格

风格由 `.format` 子模块固定：

- **Rust**：4 空格缩进（见 `.format/.editorconfig` 的 `[*.rs]`）。

  ```bash
  cargo fmt --all
  cargo clippy --all-targets --all-features -- -D warnings
  ```

- **Markdown / YAML / JSON /
  TOML**：2 空格缩进，80 列硬换行（`printWidth: 80, proseWrap: "always"`，见
  `.format/.prettierrc`）。改完文档跑一次 prettier（或用编辑器的 prettier-on-save）：

  ```bash
  npx prettier --write '**/*.md'
  ```

- **Shell 脚本**：tab 缩进，`#!/usr/bin/env bash`，`set -euo pipefail`。跟
  `scripts/` 下现有脚本的风格走。

## 文档

**项目文档应只包含项目特有的知识。**

`README.md`、`CONTRIBUTING.md` 及未来的 `docs/` 不写通用工具用法（怎么跑
`cargo test`、git、Rust 语法）——读者可以自己去查官方文档。标准：**一句话换个项目还成立，那它就不属于这里。**
该写的是：本项目自己的命令、约定、设计决策、CLI / TUI 行为、构建 / CI 配置。

改文档时按这个标准修剪——别塞通用教程。

**保持文档同步。** `README.md` 和 `CONTRIBUTING.md`
必须始终反映当前状态。当改动改变了 CLI / TUI 行为、构建 /
CI 配置或任何已记录的约定，就在同一个 PR 里更新对应文档。

## 提交信息规范

遵循 [Conventional Commits][cc]，提交信息第一行格式为：

```
<type>(<scope>): <subject>
```

CI 用 [commitlint][cl] 检查提交信息（`@commitlint/config-conventional`，即
`.github/workflows/ci.yaml` 的 `generic`
job）。硬性规则：**正文每行硬换行在 100 列**（`body-max-line-length`）；标题行和 footer 共享该限制，所以正文任意一行超过 100 列都会让
`generic` 失败。

[cc]: https://www.conventionalcommits.org/zh-hans/
[cl]: https://commitlint.js.org/

## AI 辅助的变更

任何借助 AI 工具做出的变更都必须署名。

### 署名 —— `Assisted-by`

按 Linux 内核对 [coding assistants][kernel-ca] 的指引（另见
[submitting-patches][kernel-sp] 的 "Using Assisted-by" 一节），给提交信息加一个
`Assisted-by` trailer。

格式：

```
Assisted-by: AGENT:MODEL
```

- `AGENT` —— AI 工具或框架，例如 `Codex`、`Claude`。
- `MODEL` —— 模型版本，例如 `gpt-5`、`claude-3-opus`。

可选的专用分析工具可跟在后面（git / cargo / 编辑器这类基础工具一律不列）：

```
Assisted-by: Codex:gpt-5
Assisted-by: Claude:claude-3-opus sparse
```

若你无法可靠地确定自己的 `AGENT` /
`MODEL`，不要猜——问用户，并原样记录他们告诉你的内容。

[kernel-ca]:
  https://www.kernel.org/doc/html/latest/process/coding-assistants.html
[kernel-sp]:
  https://www.kernel.org/doc/html/latest/process/submitting-patches.html

## 许可证与 REUSE

代码是 **GPL-3.0-or-later**；文档 / 配置 / 脚本是 **MIT**（见 `LICENSE` 与
`LICENSES/`）。仓库遵守
[REUSE 规范][reuse]，**每个新增 / 修改的文件都必须带 SPDX 头**：

- Rust / Shell / TOML / YAML 文件用注释头：
  ```rust
  // SPDX-FileCopyrightText: 2026 Chen Linxian <me@black-desk.cn>
  //
  // SPDX-License-Identifier: GPL-3.0-or-later
  ```
  （文档用 `MIT`；脚本视内容而定——现有脚本大多用 `GPL-3.0-or-later`。）
- Markdown 文件用 HTML 注释头（见本文件开头）。

用**当前年份**。对已有文件做新的实质性改动时，在现有 `SPDX-FileCopyrightText`
下面**追加一行**——不要覆盖原年份。

```bash
reuse lint
```

`format` CI job 跑 `reuse lint`；缺失 / 错误的头会让 PR 失败。

[reuse]: https://reuse.software/spec-3.3/
