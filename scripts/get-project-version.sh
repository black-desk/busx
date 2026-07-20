#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Chen Linxuan <me@black-desk.cn>
#
# SPDX-License-Identifier: GPL-3.0-or-later

# Print the busx package version (vX.Y.Z) for the release actions.
# Runs at the workspace root, where a bare `cargo pkgid` fails (virtual
# manifest), so pin the package explicitly.
set -euo pipefail
echo "v$(cargo pkgid -p busx | awk -F'[#@]' '{print $NF}')"
