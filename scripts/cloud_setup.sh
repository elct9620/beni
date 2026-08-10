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

# The coverage measure reads its inventory from the vendored mruby headers,
# and the session's egress serves the git protocol but not source archives —
# so the source arrives by clone here, at the version the gem pins.
if [ ! -d vendor/mruby ]; then
  version="$(ruby -Ilib -rbeni/vendor -e 'print Beni::Vendor::BUILT_IN_PAIRS.fetch("mruby").fetch(:version)')"
  GIT_LFS_SKIP_SMUDGE=1 git -c advice.detachedHead=false clone --quiet --depth 1 \
    --branch "$version" https://github.com/mruby/mruby.git vendor/mruby
fi
