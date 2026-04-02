#!/usr/bin/env bash
set -euo pipefail

DATA_DIR="${1:-local/state/dev}"

bazel build //packages/workshop:work_embedded
exec bazel-bin/packages/workshop/work_embedded --data-dir "$DATA_DIR" server
