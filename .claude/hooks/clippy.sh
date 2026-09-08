#!/bin/sh
# Stop-hook clippy gate. There is one shape of this crate to lint and it
# needs a staged libmruby.a, so a missing archive fails here and names
# the command that produces one. A gate that skipped instead would read
# as a warning nobody has to act on, which is how an unlinted branch
# reaches a commit.
set -eu

root="${CLAUDE_PROJECT_DIR:?}"

require() {
  [ -e "$1" ] && return 0
  printf 'clippy hook: %s is missing. Run bundle exec rake beni:build to stage the archives and the wasi toolchain.\n' "$1" >&2
  exit 1
}

require "$root/vendor/mruby/build/host/lib/libmruby.a"
require "$root/vendor/mruby/build/wasi/lib/libmruby.a"
require "$root/vendor/wasi-sdk/bin/clang"

BENI_VENDOR_DIR="$root/vendor" \
  cargo clippy --manifest-path "$root/Cargo.toml" --workspace --all-targets -q -- -D warnings >&2

MRUBY_LIB_DIR="$root/vendor/mruby/build/wasi/lib" WASI_SDK_PATH="$root/vendor/wasi-sdk" \
  cargo clippy --target wasm32-wasip1 --manifest-path "$root/Cargo.toml" --workspace -q -- -D warnings >&2
