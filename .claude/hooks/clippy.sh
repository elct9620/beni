#!/bin/sh
# Stop-hook clippy gate. The crates take a different shape under each
# configured integer width, float width, and boxing, so the host is
# linted against the repo's 32-bit validation archive, mruby's
# upstream-default 64-bit one, the 32-bit float one, and the no-boxing
# one, and wasm32 against its own and the no-boxing one. Each needs its
# staged libmruby.a, so a missing archive fails here and names the
# command that produces it. A gate that skipped instead would read as a
# warning nobody has to act on, which is how an unlinted branch reaches
# a commit.
set -eu

root="${CLAUDE_PROJECT_DIR:?}"

require() {
  [ -e "$1" ] && return 0
  printf 'clippy hook: %s is missing. Run bundle exec rake %s to stage it.\n' "$1" "$2" >&2
  exit 1
}

require "$root/vendor/mruby/build/host/lib/libmruby.a" beni:build
require "$root/vendor/mruby/build/wasi/lib/libmruby.a" beni:build
require "$root/vendor/wasi-sdk/bin/clang" beni:build
require "$root/tmp/mruby-default-build/host/lib/libmruby.a" rust:test:default
require "$root/tmp/mruby-float32-build/host/lib/libmruby.a" rust:test:float32
require "$root/tmp/mruby-no-boxing-build/host/lib/libmruby.a" rust:test:no_boxing
require "$root/tmp/mruby-no-boxing-build/wasi/lib/libmruby.a" rust:test:no_boxing

BENI_VENDOR_DIR="$root/vendor" \
  cargo clippy --manifest-path "$root/Cargo.toml" --workspace --all-targets -q -- -D warnings >&2

MRUBY_LIB_DIR="$root/tmp/mruby-default-build/host/lib" \
  cargo clippy --manifest-path "$root/Cargo.toml" --target-dir "$root/target/default-abi" --workspace --all-targets -q -- -D warnings >&2

MRUBY_LIB_DIR="$root/tmp/mruby-float32-build/host/lib" \
  cargo clippy --manifest-path "$root/Cargo.toml" --target-dir "$root/target/float32" --workspace --all-targets -q -- -D warnings >&2

MRUBY_LIB_DIR="$root/tmp/mruby-no-boxing-build/host/lib" \
  cargo clippy --manifest-path "$root/Cargo.toml" --target-dir "$root/target/no-boxing" --workspace --all-targets -q -- -D warnings >&2

MRUBY_LIB_DIR="$root/tmp/mruby-no-boxing-build/wasi/lib" WASI_SDK_PATH="$root/vendor/wasi-sdk" \
  cargo clippy --target wasm32-wasip1 --manifest-path "$root/Cargo.toml" --target-dir "$root/target/no-boxing" --workspace -q -- -D warnings >&2

MRUBY_LIB_DIR="$root/vendor/mruby/build/wasi/lib" WASI_SDK_PATH="$root/vendor/wasi-sdk" \
  cargo clippy --target wasm32-wasip1 --manifest-path "$root/Cargo.toml" --workspace -q -- -D warnings >&2
