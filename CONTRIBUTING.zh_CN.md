<!--
SPDX-FileCopyrightText: 2026 Chen Linxian <me@black-desk.cn>

SPDX-License-Identifier: MIT
-->

# 贡献指南

[en](CONTRIBUTING.md) | zh_CN

> [!WARNING]
>
> 本项目处于**早期开发阶段**，代码**完全未经人工审核**，CLI介面也许会频繁变动。

非常欢迎你为 `busx` 提交贡献——无论是安装 `busx`
进行使用、修bug、补测试，还是新增功能。这份文档列出参与协作需要知道的约定。

## 目录

- [拉取代码](#拉取代码)
- [构建与运行](#构建与运行)
- [测试](#测试)
- [代码风格](#代码风格)
- [文档](#文档)
- [提交信息规范](#提交信息规范)
- [拉取请求流程](#拉取请求流程)
- [许可证与 REUSE](#许可证与-reuse)

## 拉取代码

仓库带一个 `.format` 子模块（共享的 `EditorConfig` / `prettier`
/ 格式化配置），clone 时务必带上 `--recurse-submodules`：

```bash
git clone --recurse-submodules https://github.com/black-desk/busx.git
```

如果已经 clone 过，补救一下：

```bash
git submodule update --init --recursive
```

## 构建与运行

`busx` 是 Cargo workspace（根 `Cargo.toml` 是虚拟 manifest），实际代码在
`crates/busx/`，测试夹具在 `crates/testbus/`。所有 `cargo` 命令都可以在
**仓库根目录**直接跑：

```bash
cargo build                       # 调试构建
cargo run -- <args>               # 直接跑，例如 cargo run -- list
cargo run -- --help               # 查看 CLI
cargo run                         # 进入 TUI
```

## 测试

**测试原则：只在公开接口层做端到端测试，不对内部模块写单元测试。**

`busx` 是纯 binary crate（根 `Cargo.toml` 没有 lib
target），对外公开的表面只有两个：CLI 子命令的输入 / 输出，和 TUI 的交互 / 渲染。所有测试都应该从这两层入手——`crates/busx/tests/`
用 `assert_cmd` 起子进程跑真实的 `busx` 二进制；TUI 测试把真实的 `busx`
作为子进程跑在一个伪终端（PTY）里，由 `tuiprobe`
喂键鼠事件、读取渲染输出。内部模块（`dbus/`、`value/`、`ops/`
等里的私有函数）不该有独立的单元测试，它们的行为应该能通过公开表面间接验证到。换句话说：**如果一个测试需要
`pub(crate)` 或者更窄的可见性才能写，那它就是错的测试位置**——把断言挪到 `tests/`
下，通过一条 `busx` 命令行 / 一帧 TUI 输出去验证。

基于这条原则，测试分两层：**集成测试**（`crates/busx/tests/`）和
**TUI 快照测试** （`crates/busx/tests/tui_pty.rs`，用 `tuiprobe`），两者都由
`testbus` 夹具驱动。

### testbus 夹具

`testbus::bus_owned()` 会在后台拉起一个独立的 `dbus-daemon`，注册一个名为
`org.busx.Test` 的测试服务，返回它的地址。集成测试用 `--address` 把 `busx`
指向这个总线，因此**不依赖系统 / 会话总线的状态**，跑出来是确定的。

```rust
let addr = testbus::bus_owned().address.clone();
Command::cargo_bin("busx")?
    .args(["--address", &addr, "list"])
    ...
```

典型用法见 `crates/busx/tests/list.rs`。

### 快照测试（insta）

TUI 渲染逻辑用 [`insta`][insta] 做快照对比，金快照放在
`crates/busx/tests/snapshots/*.snap`。测试在 `tuiprobe` 拉起的真实终端里起真实的
`busx`（连到 `testbus`
私有总线），喂键鼠事件驱动到目标状态，再对终端屏幕做快照——走完整的 `main` → CLI
→ crossterm → ratatui → `render` 管线。用 `wait_for_snapshot!`
轮询直到屏幕匹配某个已有金快照（收敛前的中间帧不落盘），再用
`insta::assert_snapshot!` 断言新帧。

[insta]: https://insta.rs

当你**有意**修改了 TUI 渲染（例如调整布局、文案、颜色），快照测试会失败。这是预期的，请用
`cargo insta` 接受新输出：

```bash
cargo install cargo-insta          # 一次性安装
cargo insta accept                 # 接受所有新快照
cargo insta review                 # 逐个 review，更稳
```

**不接受**的快照改动不要 commit；如果是无意的回归，先回头查 `render.rs` /
`update.rs`，不要直接 accept 掩盖问题。

### 覆盖率

CI 通过 [`black-desk/workflows/rust`][wf-rust] 上报到
[codecov][codecov]。本地不需要专门跑覆盖率，但改动核心路径（`dbus/`、
`value/`、`tui/update.rs`）时请尽量补测试。

[wf-rust]: https://github.com/black-desk/workflows
[codecov]: https://codecov.io/github/black-desk/busx

## 代码风格

风格由 `.format` 子模块统一规定，不靠口口相传：

- **Rust**：4 空格缩进（见 `.format/.editorconfig` 的 `[*.rs]`）。

  ```bash
  cargo fmt --all
  cargo clippy --all-targets --all-features -- -D warnings
  ```

- **Markdown / YAML / JSON /
  TOML**：2 空格、80 列硬换行（`printWidth: 80, proseWrap: "always"`，见
  `.format/.prettierrc`）。改文档后跑一次 prettier（或直接用编辑器的 prettier-on-save）：

  ```bash
  npx prettier --write '**/*.md'
  ```

- **Shell 脚本**：tab 缩进、`#!/usr/bin/env bash`、`set -euo pipefail`。参考
  `scripts/` 下已有的脚本风格。

## 文档

**项目文档只写项目特定的知识。**

`README.md`、`CONTRIBUTING.md`、未来的 `docs/`
都遵守这条标准：通用工具用法（怎么跑 `cargo test`、`git`
怎么用、Rust 语法是什么）一律不写，读者自己查官方文档即可。判断标准很简单——**一段话换个项目还成立，那它就不该出现在这里**。该写的是：这个项目独有的命令、约定、设计决定、CLI
/ TUI 行为、构建 / CI 配置等等。

新增 / 修改文档时按这条标准删减和取舍，不要把通用教程塞进来。

**保持文档同步。** `README.md` 和 `CONTRIBUTING.md`
必须始终反映项目当前的状态。每当改动改变了 CLI / TUI 行为、构建 /
CI 配置或任何已记录的约定，就要在同一个 PR 里更新对应文档，不要让文档与代码脱节。

## 提交信息规范

使用 [Conventional Commits][cc]，提交信息第一行格式为：

```
<type>(<scope>): <subject>
```

- **`type`** 取以下之一：
  - `feat` —— 新功能（CLI 子命令、TUI 交互、新输出格式等）
  - `fix` —— bug 修复
  - `refactor` —— 不改变行为的重构
  - `docs` —— 文档改动
  - `test` —— 仅测试改动
  - `style` —— 格式 / 缩进 / 空白
  - `perf` —— 性能改进
  - `ci` / `chore` —— CI / 构建 / 依赖（`chore(deps)` 用于 dependabot 升级）
- **`scope`**（可选但推荐）：受影响的模块，例如 `tui`、`cli`、`complete`、
  `dbus`、`value`、`ops`、`monitor`、`list`、`cd`、`ci`。
- **`subject`**：祈使句、现在时、小写开头、不加句号。

例如：

```
feat(tui): add / inline filter to list screens
fix(monitor): respect --timeout on idle bus
refactor(value): share sig_str/access_str helpers
docs(readme): update --help blocks for -v and --log
chore(deps): bump clap from 4.6.1 to 4.6.2
```

完整历史可以 `git log --oneline` 看。涉及 GitHub issue /
PR 的提交，在末尾正文里写 `Closes #42` 或 `Refs #42`。

CI 会在每个 PR 上用 [commitlint][cl] 检查提交信息，规则集是
`@commitlint/config-conventional`（见 `.github/workflows/ci.yaml` 的 `generic`
job）。要记住的硬性规则：**正文每行硬换行在 100 列**（`body-max-line-length`）。标题行和 footer 共享同样的 100 列限制，所以正文里任何一行超过 100 列都会让
`generic` 这个 CI job 失败。

[cc]: https://www.conventionalcommits.org/zh-hans/
[cl]: https://commitlint.js.org/

## 拉取请求流程

1. 从 `master` 拉分支：
   ```bash
   git checkout -b feat/my-feature master
   ```
2. 一个 PR 只做一件事。混合了多个无关改动的 PR 评审起来非常痛苦，通常会被要求拆分。
3. 本地过一遍核心检查：
   ```bash
   cargo fmt --all -- --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test
   npx prettier --check '**/*.md'   # 改了文档才需要
   ```
4. **补充测试**。新功能请加集成测试；改了 TUI 渲染请更新快照；修了 bug 请加一条回归测试。不知道怎么写可以参考
   `crates/busx/tests/` 下的现有测试，或者直接在 PR 里问。
5. PR 描述里写清**做了什么 / 为什么 / 怎么测的**。如果有对应 issue，写上
   `Closes #N`。
6. CI 全绿后等 review。`pass` job 是 CI 的总开关，必须绿。

## 许可证与 REUSE

本仓库代码以 **GPL-3.0-or-later** 开源，文档 / 配置 / 脚本以 **MIT** 开源（见
`LICENSE` 与 `LICENSES/`）。仓库遵守 [REUSE 规范][reuse]，
**每一个新增 / 修改的文件都必须带 SPDX 头**：

- Rust / Shell / TOML / YAML 文件用注释头：
  ```rust
  // SPDX-FileCopyrightText: 2026 Chen Linxian <me@black-desk.cn>
  //
  // SPDX-License-Identifier: GPL-3.0-or-later
  ```
  （文档用 `MIT`，脚本按内容定，已有脚本多用 `GPL-3.0-or-later`。）
- Markdown 文件用 HTML 注释头（见本文件顶部）。

年份用**当前年份**；如果是给已有文件加新的实质性改动，可以在已有的
`SPDX-FileCopyrightText` 行下面**追加一行新的**，不要直接改原年份。

```bash
reuse lint
```

CI 里的 `format` job 会跑 `reuse lint`，缺失 / 错误的头会让 PR 失败。

[reuse]: https://reuse.software/spec-3.3/
