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

`busx` 是一个用 Rust（基于 [zbus]）实现的 D-Bus 命令行工具，目标是替代
`dbus-send` / `busctl` / `qdbus` 三件套，把它们各自的痛点一次性补齐：

- 入参采用 `busctl` 风格（签名串 + 位置参数），**完整支持嵌套与空容器** ——补齐
  `dbus-send` 的硬伤；
- 默认输出**人类可读文本**；加 `--json` 切到**带类型标签的 JSON**（值
  `{"type":..,"data":..}`，监听为每行一个对象的 NDJSON），对脚本友好、可管道到
  `jq` / python；
- **不会重蹈 sd-bus 的覆辙**：非 string 键的 dict（如 `a{uu}`）正常渲染成
  `[{"key":..,"value":..}]`，绝不崩溃（对比 systemd#32904）；
- 自带**动态 shell 补全**（bash/zsh），实时内省总线；
- **单二进制、零运行时依赖**（纯 Rust，不依赖 libdbus）。

[zbus]: https://crates.io/crates/zbus

## 功能

- `list` —— 列出服务名 + PID + 进程名（人类为表，`--json` 为对象数组）。
- `tree SVC` —— 画单个服务的对象路径树。
- `introspect` —— 列出对象的接口 / 方法 / 信号 / 属性。
- `call SVC OBJ IFACE METHOD SIG ARGS...`
  —— 调用方法（SIG 独立必填、可补全；入参 busctl 风格，支持任意嵌套）。
- `get` / `set` —— 读取（不传属性名走 `GetAll`）/ 写入属性。
- `monitor` —— 监听总线消息，按 match rule 过滤（`--json` 出 NDJSON，含
  `PropertiesChanged` 解码）。
- `completion`
  —— 生成动态 shell 补全脚本（实时内省总线补全服务/路径/接口/方法/签名/属性）。
- **TUI 模式** —— 裸 `busx`（不带子命令）打开全屏交互式浏览器：逐级钻取 service
  → objects → interfaces → interface（methods / properties /
  signals）。调用方法、读写属性、监听信号（Esc 停止），并将任意操作一键复制为
  `dbus-send` / `busctl` / `qdbus` / `gdbus` 命令。支持鼠标。

## 安装

需要 Rust 工具链与一个 D-Bus 环境（仅 Linux）。系统需装有 `dbus-daemon`
（运行/测试时用）。

```bash
# 从 GitHub 安装最新版（二进制落到 ~/.cargo/bin，用 rustup 的话已在 $PATH 里）
cargo install --git https://github.com/black-desk/busx

# 之后升级（一次性：`cargo install cargo-update`）。-g 必加：git 来源默认会被跳过
cargo install-update -g busx       # 或 `cargo install-update -ag` 升级全部
```

## 用法

```bash
# 交互式 TUI（裸 busx 不带子命令）：浏览、调用、监听、copy-as
busx

# 列服务（默认连 session bus，连不上自动回退 system bus）
busx list

# 内省某个对象
busx introspect org.freedesktop.systemd1 /org/freedesktop/systemd1

# 调方法（SIG 是独立必填位；ListUnits 无入参故 SIG 为空串）
busx call org.freedesktop.systemd1 /org/freedesktop/systemd1 \
  org.freedesktop.systemd1.Manager ListUnits ""

# 嵌套入参（dbus-send 做不到的 a{sv} 内含数组；'a{sv}' 即 SIG）：
busx call org.example /obj org.example.Iface Method \
  'a{sv}' 1 'hint' 'a' 's' 2 'a' 'b'

# 读属性（不传属性名 = GetAll）
busx get org.freedesktop.systemd1 /org/freedesktop/systemd1 \
  org.freedesktop.systemd1.Manager

# 监听信号，--json 出 NDJSON 管道到外部 jq 处理
busx --json monitor --signals --interface org.freedesktop.DBus.Properties \
  --member PropertiesChanged | jq 'select(.args[1] != {})'

# 开启补全：把这行加进 ~/.bashrc（zsh 加进 ~/.zshrc）后重开 shell ——
# 它会实时内省总线，补全服务/路径/接口/方法
eval "$(busx completion bash)"     # zsh: eval "$(busx completion zsh)"
```

默认输出人类可读文本；`--json` 切到 type-tagged JSON（`monitor`
为 NDJSON），需要缩进美化或字段变换时管道到外部 `jq` /
python。所有诊断（错误、警告）打到stderr，前缀 `busx:`；退出码 `0` 成功 / `1`
失败。管道到 `less`/`head` 也不会 panic（SIGPIPE 按常规处理）。

## 路线图

1. pcapng `capture`。
2. 值渲染增强：`ay` 字节数组的 bytestring 视图（目前渲染为数字数组）、字符串输出中控制 / 不可打印字符的转义、`Fd` 渲染。

## 许可证

如无特殊说明，该项目的代码以 GNU 通用公共许可协议第三版或任何更新的版本开源，文档、配置文件以及开发维护过程中使用的脚本等以 MIT 许可证开源。

该项目遵守 [REUSE 规范]。你可以使用
[reuse-tool](https://github.com/fsfe/reuse-tool) 生成这个项目的 SPDX 列表：

```bash
reuse spdx
```

[REUSE 规范]: https://reuse.software/spec-3.3/
