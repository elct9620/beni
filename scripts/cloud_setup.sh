#!/usr/bin/env bash
# Cloud session bootstrap
# =======================
#
# A cloud session starts from a fresh VM holding nothing but a clone, so
# everything the task surface expects from a working checkout is rebuilt
# here. A local checkout keeps its own, so the script stops unless it is
# running in the cloud.
set -euo pipefail

[ "${CLAUDE_CODE_REMOTE:-}" = "true" ] || exit 0

cd "$CLAUDE_PROJECT_DIR"

# The default rake task and every hook run through bundler.
bundle check >/dev/null 2>&1 || bundle install
