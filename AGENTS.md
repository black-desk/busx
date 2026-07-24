<!--
SPDX-FileCopyrightText: 2026 Chen Linxian <me@black-desk.cn>

SPDX-License-Identifier: MIT
-->

# AGENTS

This file is the entry point for AI coding agents working in `busx`.

## Read the contribution guide first

**Before making any change, you MUST read [CONTRIBUTING.md](CONTRIBUTING.md).**
It is the single source of truth for this project's conventions — clone
(submodules), build & run, the testing philosophy, code style, commit messages,
the pull-request workflow, and the REUSE / SPDX-header license rules. This file
only adds what is specific to AI-assisted work.

## Language

This document is in English only as a documentation convention. It does not
dictate how you talk to the user. Communicate with the user in whatever language
they use; do not switch to English just because this file is in English.

## AI attribution — `Assisted-by`

Any change written with the help of an AI tool MUST record that fact in the
commit message with an `Assisted-by` trailer. This follows the Linux kernel's
guidance for [coding assistants][kernel-ca] (see also the "Using Assisted-by"
section of [submitting-patches][kernel-sp]).

The required format:

```
Assisted-by: AGENT:MODEL
```

- `AGENT` — the AI tool or framework, e.g. `Codex`, `Claude`.
- `MODEL` — the specific model version used, e.g. `gpt-5`, `claude-3-opus`.

Optional specialized analysis tools may follow, as in the kernel spec; basic
tools (git, cargo, editors) are never listed:

```
Assisted-by: Codex:gpt-5
Assisted-by: Claude:claude-3-opus sparse
```

Place the trailer at the end of the commit body:

```
feat(tui): clip long result lines

Long result rows now scroll horizontally instead of wrapping the layout.

Assisted-by: Codex:gpt-5
```

The human author stays fully responsible for reviewing the output and meeting
every rule in CONTRIBUTING.md.

[kernel-ca]:
  https://www.kernel.org/doc/html/latest/process/coding-assistants.html
[kernel-sp]:
  https://www.kernel.org/doc/html/latest/process/submitting-patches.html
