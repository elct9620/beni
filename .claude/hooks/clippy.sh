#!/bin/sh
# Stop-hook clippy gate: rust:clippy lints every build of every
# verification config, and fails naming the task that stages an
# archive not built yet. A gate that skipped instead would read as a
# warning nobody has to act on, which is how an unlinted branch reaches
# a commit.
set -eu

cd "${CLAUDE_PROJECT_DIR:?}"
bundle exec rake rust:clippy >&2
