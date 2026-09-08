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
#   $ rake rust:check:nodefault
#                            — cargo check the beni crate alone with
#                              every capability feature off, which is
#                              the shape a consumer embedding mruby
#                              without a compiler gets.
#   $ rake rust:check:wasm   — cargo check on wasm32-wasip1
#   $ rake rust:check:docs   — what a documentation host builds: no
#                              archive, the checked-in documentation
#                              bindings standing in for one. Compiles
#                              first, then renders — a render passes
#                              over function bodies, which is where the
#                              bindings surface grows. Runs anywhere,
#                              since it reads the committed file rather
#                              than writing it, so a host that cannot
#                              regenerate the bindings still learns here
#                              that its change has outgrown them.
#   $ rake rust:link:wasm    — link wasm32-wasip1 test binaries against the
#                              staged archive. `cargo check` never reaches
#                              the linker, so this is what exercises the
#                              link set the sidecar names.
#   $ rake rust:test:default — cargo test against an mruby built with the
#                              untouched upstream default config — the
#                              gem's clean-build behaviour (64-bit
#                              mrb_int on 64-bit hosts). Catches
#                              width-coincidence bugs that the repo's
#                              MRB_INT32 validation config masks.
#   $ rake rust:verify       — beni:build + the tasks above; the
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
    desc "cargo check the workspace on wasm32-wasip1"
    task :wasm do
      abort "cargo not on PATH; install Rust toolchain to run rust:check:wasm" unless BeniRust.cargo_available?
      abort BeniRust::MISSING_WASM_TARGET unless BeniRust.wasm_target_installed?

      sh(BeniRust.wasm_env, "cargo", "check", "--workspace", "--target", BeniRust::WASM_TARGET)
    end

    # Cargo unifies features across the members it builds together, so
    # a workspace-wide --no-default-features still hands `beni` the
    # defaults a sibling asked for. Naming the one package is what makes
    # the shape a consumer gets with default features off the shape this
    # leg actually compiles.
    desc "cargo check the beni crate with every capability feature off"
    task :nodefault do
      abort "cargo not on PATH; install Rust toolchain to run rust:check:nodefault" unless BeniRust.cargo_available?

      sh(BeniRust.host_env, "cargo", "check", "-p", "beni", "--no-default-features")
    end

    # The documentation bindings can only be written on the platform the
    # documentation host builds on, but whether they still describe a
    # surface the crate compiles against is the same question
    # everywhere — so this leg runs on every host and needs no archive.
    # It compiles before it renders: rustdoc type checks signatures and
    # not bodies, and a body is where a new `sys::` call appears.
    desc "Build as a documentation host would: no archive, checked-in bindings"
    task :docs do
      abort "cargo not on PATH; install Rust toolchain to run rust:check:docs" unless BeniRust.cargo_available?

      BeniRust.documentation_build_check
      BeniRust.documentation_build_doc
    end
  end

  namespace :link do
    # `cargo check` never reaches the linker, so the link directives the
    # archive's sidecar drives — which libraries, found through which
    # search paths — are only exercised by producing a real artifact.
    # wasm32 has no test runner, so the binaries are built and not run.
    desc "link wasm32-wasip1 test binaries against the staged archive"
    task :wasm do
      abort "cargo not on PATH; install Rust toolchain to run rust:link:wasm" unless BeniRust.cargo_available?
      abort BeniRust::MISSING_WASM_TARGET unless BeniRust.wasm_target_installed?

      sh(BeniRust.wasm_env, "cargo", "test", "--workspace", "--target", BeniRust::WASM_TARGET, "--no-run")
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

  desc "Full local compile verification: build + host, feature-off, wasm32, documentation and default-ABI legs"
  task verify: ["beni:build", "rust:check", "rust:test", "rust:check:nodefault", "rust:check:wasm",
                "rust:link:wasm", "rust:check:docs", "rust:test:default"]
end
