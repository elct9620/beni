#!/bin/sh
# Stop-hook clippy gate. beni-sys's archive discovery is env-driven
# with no fallback, so the staged vendor artifacts are named
# explicitly: BENI_VENDOR_DIR lints the linked host code paths,
# MRUBY_LIB_DIR + WASI_SDK_PATH the wasm32 ones. Without staged
# artifacts the gate degrades — host lints placeholder mode, wasm32
# is skipped — and reports the degradation via the hook's
# systemMessage instead of failing, so a fresh clone is never
# blocked but a weakened lint never passes silently.
set -eu

root="${CLAUDE_PROJECT_DIR:?}"
degraded=""

if [ -f "$root/vendor/mruby/build/host/lib/libmruby.a" ]; then
  export BENI_VENDOR_DIR="$root/vendor"
else
  degraded="host archive not staged (placeholder lint only)"
fi
cargo clippy --manifest-path "$root/Cargo.toml" --workspace --all-targets -q -- -D warnings >&2

if rustc --target wasm32-wasip1 --print sysroot >/dev/null 2>&1; then
  if [ -f "$root/vendor/mruby/build/wasi/lib/libmruby.a" ] \
    && [ -x "$root/vendor/wasi-sdk/bin/clang" ]; then
    MRUBY_LIB_DIR="$root/vendor/mruby/build/wasi/lib" WASI_SDK_PATH="$root/vendor/wasi-sdk" \
      cargo clippy --target wasm32-wasip1 --manifest-path "$root/Cargo.toml" --workspace -q -- -D warnings >&2
  else
    degraded="${degraded:+$degraded; }wasm32 artifacts not staged (wasm32 lint skipped)"
  fi
fi

if [ -n "$degraded" ]; then
  printf '{"systemMessage":"clippy hook degraded: %s — run \\u0060bundle exec rake beni:build\\u0060 for full coverage"}\n' "$degraded"
fi
