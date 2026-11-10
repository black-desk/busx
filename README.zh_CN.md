<!--
SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
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
- **服务浏览器**：共用对象／接口树，成员分为属性、方法、信号三个分类。80 列及以上时，导航与详情并排显示；更窄时仅显示当前焦点所在区域。
  `Tab` / `Shift+Tab` 切换焦点，`h/l` 切换成员分类或折叠／展开对象， `j/k`
  上下选择（保留方向键），`gg/G` 跳到首项／末项，`n` 返回导航。对象显示为
  `▸ /App47`，接口缩进显示为
  `@ org.gnome.SessionManager.App`，保留完整接口前缀；超出区域宽度的名称以省略号截断，顶部显示选中项的完整名称。叶子对象默认收起，只有一个子节点的容器路径合并显示；面包屑使用
  `›` 分隔，避免与对象路径中的 `/` 混淆。
- **行内操作**：选择成员即可查看签名、属性权限（`r` / `w` /
  `rw`）和当前值。填写参数后明确激活 `Call` / `Write`
  执行，参数与结果保留在同一页。参数输入框统一对齐，获得焦点时处于普通模式，
  `j/k` 选择字段，`i` 进入插入模式，`Esc`
  返回普通模式并保留焦点。在最后一个字段向下移到按钮行，向上返回最后一个字段，`h/l`
  选择按钮。插入模式下 `Tab` 切换字段，`Enter`
  结束编辑并聚焦操作按钮，不会直接执行。 `Expand result` 全屏查看大结果，`Esc`
  返回编辑区。基本类型的值原样输入，支持空格和空字符串；容器使用 busctl 风格的 token，例如
  `as` 输入 `2 hello world`。容器输入处提示通过 `?`
  查看语法规则和 busctl 来源，编辑时先按 `Esc`。编辑时 `q` 等普通字符正常输入，
  `Ctrl+C` 可在输入框内关闭当前标签。
- **独立标签页**：`t` 新建服务浏览标签，`gt/gT` 切到下一个／上一个标签，
  `{数字}gt`
  跳到指定标签。每页独立保存服务、对象、接口、参数、结果、过滤条件和滚动位置。
  `q` 关闭当前标签，关掉最后一个标签时退出；输入框内可用 `Ctrl+C`
  关闭当前标签。字母快捷键在普通模式下生效，编辑时字符原样输入。顶部 `+`
  也可新建标签，点击标签标题可以切换，右侧的 `×` 关闭对应标签。
- **底部提示**：只用一行显示当前焦点相关的操作和帮助入口；状态消息临时替代操作提示。全局标签快捷键放在帮助页；编辑时先按
  `Esc`，再按 `?` 查看帮助。
- **当前页监听**：`Watch`
  将当前标签切换为单个信号、属性变化或方法调用的监听页。想同时浏览其他内容时按
  `t`，原标签继续收集消息。 `p` 暂停显示但继续收集，`s` 停止监听并保留消息，`x`
  清空保留的事件， `End` 跟随最新事件。`Esc`
  停止监听并恢复该页原来的浏览位置、参数和选中项；关闭标签也会停止它的监听。每项最多保留 1,000 个事件；方法调用监听需要总线允许成为 monitor。
- **copy-as**：`c` 将当前操作预览为 `dbus-send` / `busctl` / `qdbus` / `gdbus` /
  `busx` 命令，选择工具后按 `Enter` 复制；`y`
  复制结果或监听文本。工具不支持的操作会在预览中标明。
- **行内过滤**：服务列表、对象／接口导航和当前成员标签中按
  `/`，即可按子串过滤（大小写不敏感）。输入收窄范围，`↑↓` 选择，`Enter` 打开，
  `Esc` 清除；在输入框外按 `?` 查看快捷键帮助。

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
