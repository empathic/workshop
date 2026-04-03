#!/usr/bin/env bash
# Wrapper around prettier that resolves the svelte plugin from Bazel runfiles.
#
# Prettier 3 uses ESM import() for --plugin resolution, which only walks
# node_modules up from CWD. In the Bazel format-check context CWD is the
# workspace root (no node_modules), so bare specifiers like
# "prettier-plugin-svelte" fail. This wrapper resolves the plugin to an
# absolute file path via runfiles, bypassing ESM resolution entirely.

# --- begin runfiles.bash initialization v3 ---
set -o pipefail; set +e; f=bazel_tools/tools/bash/runfiles/runfiles.bash
source "${RUNFILES_DIR:-/dev/null}/$f" 2>/dev/null || \
  source "$(grep -sm1 "^$f " "${RUNFILES_MANIFEST_FILE:-/dev/null}" | cut -f2- -d' ')" 2>/dev/null || \
  source "$0.runfiles/$f" 2>/dev/null || \
  source "$(grep -sm1 "^$f " "$0.runfiles_manifest" | cut -f2- -d' ')" 2>/dev/null || \
  source "$(grep -sm1 "^$f " "$0.exe.runfiles_manifest" | cut -f2- -d' ')" 2>/dev/null || \
  { echo>&2 "ERROR: runfiles.bash initializer cannot find $f"; exit 1; }; f=; set -e
# --- end runfiles.bash initialization v3 ---

PRETTIER="$(rlocation _main/packages/workshop_ui/prettier_inner_/prettier_inner)"
PLUGIN="$(rlocation _main/packages/workshop_ui/node_modules/prettier-plugin-svelte/plugin.js)"

exec "$PRETTIER" "--plugin=$PLUGIN" "$@"
