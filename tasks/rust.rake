# frozen_string_literal: true

# Rust workspace verification tasks
# =================================
#
# Repo-local compile-verification chain for the beni crates.
# `crates/beni-sys`'s archive discovery is environment-driven
# (`MRUBY_LIB_DIR` > `BENI_VENDOR_DIR`, no fallback), so each task
# names the repo's vendor artifacts explicitly: host tasks point
# `BENI_VENDOR_DIR` at the vendor tree, the wasm task points
# `MRUBY_LIB_DIR` / `WASI_SDK_PATH` at the wasi staged path and
# toolchain. The artifacts come from the gem's own `beni:build` task
# (dogfooding — see Beni::Tasks in the Rakefile); running a task
# without them fails naming the missing archive.
#
#   $ rake rust:check        — cargo check, host target, linked against
#                              the vendored libmruby.a
#   $ rake rust:test         — cargo test, host target, linked against
#                              the vendored libmruby.a
#   $ rake rust:check:wasm   — cargo check on wasm32-wasip1 (degrades to
#                              host with a warning when the Rust target
#                              is not provisioned)
#   $ rake rust:test:default — cargo test against an mruby built with the
#                              untouched upstream default config — the
#                              gem's clean-build behaviour (64-bit
#                              mrb_int on 64-bit hosts). Catches
#                              width-coincidence bugs that the repo's
#                              MRB_INT32 validation config masks.
#   $ rake rust:verify       — beni:build + the four tasks above; the
#                              single local entry point for "does the
#                              Rust side compile and pass everywhere".

require_relative "support/beni_rust"

namespace :rust do
  desc "cargo check the workspace on the host target"
  task :check do
    abort "cargo not on PATH; install Rust toolchain to run rust:check" unless BeniRust.cargo_available?

    sh(BeniRust.host_env, "cargo", "check", "--workspace")
  end

  desc "cargo test the workspace on the host (wasm32 has no test runner)"
  task :test do
    abort "cargo not on PATH; install Rust toolchain to run rust:test" unless BeniRust.cargo_available?

    sh(BeniRust.host_env, "cargo", "test", "--workspace")
  end

  namespace :check do
    desc "cargo check the workspace on wasm32-wasip1 (host fallback when unprovisioned)"
    task :wasm do
      abort "cargo not on PATH; install Rust toolchain to run rust:check:wasm" unless BeniRust.cargo_available?

      target = BeniRust.wasm_target_or_host
      if target.nil?
        warn "[rust] #{BeniRust::WASM_TARGET} not provisioned; falling back to host check"
        Rake::Task["rust:check"].invoke
        next
      end

      sh(BeniRust.wasm_env, "cargo", "check", "--workspace", "--target", target)
    end
  end

  namespace :test do
    # Catches type/width coincidences the repo's MRB_INT32 validation
    # config masks — see BeniRust.default_abi_test for the mechanics.
    desc "cargo test against an upstream-default mruby build (64-bit mrb_int on 64-bit hosts)"
    task default: "beni:vendor:setup:mruby" do
      abort "cargo not on PATH; install Rust toolchain to run rust:test:default" unless BeniRust.cargo_available?

      BeniRust.default_abi_test
    end
  end

  desc "Full local compile verification: beni:build + host check/test + wasm32 check + default-ABI test"
  task verify: ["beni:build", "rust:check", "rust:test", "rust:check:wasm", "rust:test:default"]
end
