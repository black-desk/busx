<!--
SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>

SPDX-License-Identifier: MIT
-->

# busx

[![checks][badge-shields-io-checks]][actions]
[![codecov][badge-shields-io-codecov]][codecov]
[![commit activity][badge-shields-io-commit-activity]][commits]
[![contributors][badge-shields-io-contributors]][contributors]
[![release date][badge-shields-io-release-date]][releases]
![commits since release][badge-shields-io-commits-since-release]

[badge-shields-io-checks]:
  https://img.shields.io/github/check-runs/black-desk/busx/master
[actions]: https://github.com/black-desk/busx/actions
[badge-shields-io-commit-activity]:
  https://img.shields.io/github/commit-activity/w/black-desk/busx/master
[commits]: https://github.com/black-desk/busx/commits/master
[badge-shields-io-contributors]:
  https://img.shields.io/github/contributors/black-desk/busx
[contributors]: https://github.com/black-desk/busx/graphs/contributors
[badge-shields-io-release-date]:
  https://img.shields.io/github/release-date/black-desk/busx
[releases]: https://github.com/black-desk/busx/releases
[badge-shields-io-commits-since-release]:
  https://img.shields.io/github/commits-since/black-desk/busx/latest
[badge-shields-io-codecov]:
  https://codecov.io/github/black-desk/busx/graph/badge.svg?token=9DDJPR1SVE
[codecov]: https://codecov.io/github/black-desk/busx

[en](README.md) | zh_CN

> [!WARNING]
>
> 本项目处于**早期开发阶段**，代码**完全未经人工审核**，请勿用于生产环境。

`busx` 是一个用 Rust（基于 [zbus]）实现的 D-Bus
TUI/命令行工具，类似于 dbus-send 以及 d-feet/d-spy。

[zbus]: https://crates.io/crates/zbus

## 功能

- 不带子命令直接运行
  `busx`，进入交互式浏览器（服务 → 对象 → 接口 → 方法 / 属性 / 信号）；
- 带子命令（`list` / `call` / `get` / …）则按纯命令行方式工作，方便写进脚本。

```bash
busx --help
```

```text
D-Bus CLI (dbus-send/busctl/qdbus replacement)

Usage: busx [OPTIONS] [COMMAND]

Commands:
  list        List service names on the bus
  tree        Show the object path tree of a service
  introspect  Show interfaces/methods/signals/properties of an object
  call        Call a method
  get         Get properties (no property names => GetAll)
  set         Set a property
  monitor     Monitor bus messages
  help        Print this message or the help of the given subcommand(s)

Options:
      --user                      Connect to the session bus (the default falls back to the system bus on failure)
      --system                    Connect to the system bus
      --address <ADDRESS>         Connect to the bus at ADDRESS (e.g. unix:path=...)
  -v...                           Increase log verbosity (-v / -vv / -vvv)
      --log <PATH>                TUI log file (default: $XDG_CACHE_HOME/busx/busx.log)
      --show-standard-interfaces  Show standard D-Bus interfaces in the TUI (hidden by default)
      --json                      Emit type-tagged JSON (default: human text)
  -h, --help                      Print help
  -V, --version                   Print version
```

几个常用例子：

```bash
# 交互式 TUI（不带子命令）
busx

# 列出总线上的服务（默认会话总线，失败回退系统总线）
busx list

# 内省一个对象
busx introspect org.freedesktop.systemd1 /org/freedesktop/systemd1

# 调用方法（SIG 是独立的必填参数；ListUnits 无参，故 SIG 为 ""）
busx call org.freedesktop.systemd1 /org/freedesktop/systemd1 \
  org.freedesktop.systemd1.Manager ListUnits ""

# 读属性（不给属性名 = GetAll）
busx get org.freedesktop.systemd1 /org/freedesktop/systemd1 \
  org.freedesktop.systemd1.Manager

# 监听信号；--json 输出 NDJSON，方便管道给 jq
busx --json monitor --signals --interface org.freedesktop.DBus.Properties \
  --member PropertiesChanged | jq 'select(.args[1] != {})'

# 启用动态补全：加进 ~/.bashrc（zsh 用 ~/.zshrc）后重启 shell
eval "$(busx completion bash)"
```

默认输出是人类友好的纯文本；`--json` 切换到**带类型标签的 JSON**（`monitor`
为 NDJSON）——每个值都是
`{"type":..,"data":..}`，完整保留 D-Bus 类型信息，管道给外部 `jq` /
python 即可。所有诊断（错误、警告）打到 stderr 并带 `busx:` 前缀；退出码 `0`
成功 / `1` 失败；管道到 `less`/`head` 不会 panic（SIGPIPE 按常规处理）。

### 命令行模式：与 busctl 的不同

- **输出**：默认人类友好纯文本；`--json`
  给出**带类型标签的 JSON**，完整保留 D-Bus 类型信息，方便脚本 / `jq` 处理。
- **默认总线**：默认连会话总线。
- **纯 Rust 单二进制**：不依赖 libdbus，也不依赖 systemd。

### TUI 模式：与 d-feet 的不同

- **跑在终端里**：不需要图形环境，SSH、TTY、容器里都能用；d-feet /
  d-spy 是 GTK 图形程序，必须有桌面。
- **copy-as**：任意一次操作（调用、读 / 写属性、监听信号）都能按 `dbus-send` /
  `busctl` / `qdbus` / `gdbus` 四种格式复制成命令

## 路线图

计划中的重构与已知问题记录在 [TODO.md](TODO.md)。

## 安装

```bash
# 从 GitHub 安装最新版（二进制落到 ~/.cargo/bin，用 rustup 的话已在 $PATH 里）
cargo install --git https://github.com/black-desk/busx

# 之后升级（一次性：`cargo install cargo-update`）。-g 必加：git 来源默认会被跳过
cargo install-update -g busx       # 或 `cargo install-update -ag` 升级全部
```

## 许可证

如无特殊说明，该项目的代码以 GNU 通用公共许可协议第三版或任何更新的版本开源，文档、配置文件以及开发维护过程中使用的脚本等以 MIT 许可证开源。

该项目遵守 [REUSE 规范]。你可以使用
[reuse-tool](https://github.com/fsfe/reuse-tool) 生成这个项目的 SPDX 列表：

```bash
reuse spdx
```

[REUSE 规范]: https://reuse.software/spec-3.3/
