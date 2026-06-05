# frozen_string_literal: true

# mruby build configuration for beni's compile-verification chain.
# ================================================================
#
# Drives mruby's build system to produce the `libmruby.a` archives the
# beni crates link against, one per verification target:
#
#   * `vendor/mruby/build/host/lib/libmruby.a` — native build for the
#     local machine (macOS / Linux), used by host `cargo test`.
#   * `vendor/mruby/build/wasi/lib/libmruby.a` — cross-compiled for
#     wasm32-wasip1 against the vendored wasi-sdk, used by
#     `cargo check --target wasm32-wasip1`.
#
# Both targets share the same ABI-bearing `-D` defines (see
# +BeniBuildConfig::ABI_DEFINES+): the typed wrapper in `crates/beni`
# pins MRB_INT32 + MRB_WORDBOX_NO_INLINE_FLOAT semantics, so every
# libmruby.a it links against must be built — and bindgen'd — with the
# same layout. Widening beni to other mruby configurations is future
# work; this file is the temporary verification baseline.
#
# This file is `load`ed by mruby's minirake when the wrapping rake task
# (tasks/mruby.rake) sets `MRUBY_CONFIG=$PWD/build_config/beni.rb`.

# Resolve vendor toolchain paths relative to this file. mruby's build
# system `instance_eval`s this file in the context of MRuby::RakeFile
# (which has no `__dir__`-equivalent helper), so we anchor on `__FILE__`
# explicitly. Config-time constants live in a dedicated namespace. The
# whole module is only defined on first load, so `load`-ing this file
# twice in the same process does not warn about constant redefinition.
unless defined?(BeniBuildConfig)
  # Config-time constants shared across both verification targets.
  module BeniBuildConfig
    CONFIG_DIR   = File.expand_path(__dir__)
    PROJECT_ROOT = File.expand_path("..", CONFIG_DIR)
    VENDOR_DIR   = (ENV["BENI_VENDOR_DIR"] || File.join(PROJECT_ROOT, "vendor")).freeze
    WASI_SDK     = (ENV["WASI_SDK_PATH"] || File.join(VENDOR_DIR, "wasi-sdk")).freeze
    WASI_SYSROOT = File.join(WASI_SDK, "share", "wasi-sysroot").freeze

    # ABI-bearing defines applied to BOTH the host and wasi targets.
    # `crates/beni-sys`'s build.rs mirrors these into bindgen and the
    # trampoline compile, and the typed wrapper assumes the resulting
    # layout (i32 integers, heap-boxed floats under word boxing).
    # Changing this list means changing build.rs and the wrapper in the
    # same commit.
    ABI_DEFINES = %w[
      MRB_INT32
      MRB_WORDBOX_NO_INLINE_FLOAT
    ].freeze

    # The three setjmp/longjmp flags (wasi target only). All three must
    # be present at *both* compile and link stages; missing any one trips
    # wasi-libc's `<setjmp.h>` build-time `#error`.
    SJLJ_FLAGS = [
      "-mllvm", "-wasm-enable-sjlj",
      "-mllvm", "-wasm-use-legacy-eh=false"
    ].freeze

    # Cross-compile target. `wasm32-wasi` is the LLVM triple (same ABI
    # as Rust's `wasm32-wasip1` target); the LLVM-triple form is what
    # clang accepts on the command line.
    WASI_TARGET = "wasm32-wasi"

    # Target / sysroot flags applied to every translation unit AND the
    # link step. Frozen so a stray `<<` in the build block raises instead
    # of silently mutating the shared reference.
    TARGET_FLAGS = [
      "--target=#{WASI_TARGET}",
      "--sysroot=#{WASI_SYSROOT}"
    ].freeze

    # Core-gem baseline shared by both targets: mruby-compiler (the
    # wrapper's `mrb_load_nstring` needs it) plus the portable core
    # extension gems. No I/O / network / process gems — those do not
    # exist on wasm32-wasip1, and keeping the two targets' gem sets
    # identical keeps the verified surface identical. Consumers with
    # different needs bring their own build config; this list is only
    # beni's verification baseline.
    MRBGEM_BASELINE = %w[
      mruby-compiler
      mruby-array-ext
      mruby-enum-ext
      mruby-hash-ext
      mruby-numeric-ext
      mruby-object-ext
      mruby-proc-ext
      mruby-range-ext
      mruby-string-ext
      mruby-sprintf
      mruby-symbol-ext
      mruby-error
      mruby-metaprog
    ].freeze
  end
end

# +:wasi+ toolchain — wasi-sdk absolute tool paths, wasm32-wasi target /
# sysroot flags, the setjmp/longjmp three-flag set, and the GNU archive
# format. The cross build opts in via +conf.toolchain :wasi+; the host
# build stays on +:gcc+ so each target picks the toolchain that matches
# it.
MRuby::Toolchain.new(:wasi) do |conf, _params|
  wasi_sdk_bin = File.join(BeniBuildConfig::WASI_SDK, "bin")

  conf.toolchain :clang

  # ---- Tool commands pinned to wasi-sdk absolute paths -----------------
  conf.cc.command       = File.join(wasi_sdk_bin, "clang")
  conf.cxx.command      = File.join(wasi_sdk_bin, "clang++")
  conf.linker.command   = File.join(wasi_sdk_bin, "clang")
  conf.archiver.command = File.join(wasi_sdk_bin, "llvm-ar")
  # llvm-ar on macOS hosts defaults to Darwin (BSD) archive format,
  # which can fail with "section too large" when the archive contains
  # many wasm objects with long member paths. GNU format uses an
  # extended string table.
  conf.archiver.archive_options = "--format=gnu rs %<outfile>s %<objs>s"

  # ---- Cross-compile target / sysroot ----------------------------------
  conf.cc.flags     << BeniBuildConfig::TARGET_FLAGS
  conf.cxx.flags    << BeniBuildConfig::TARGET_FLAGS
  conf.linker.flags << BeniBuildConfig::TARGET_FLAGS

  # ---- setjmp/longjmp ----------------------------------------------------
  # Apply at compile AND link stages — the three-flag set is non-negotiable.
  conf.cc.flags     << BeniBuildConfig::SJLJ_FLAGS
  conf.cxx.flags    << BeniBuildConfig::SJLJ_FLAGS
  conf.linker.flags << BeniBuildConfig::SJLJ_FLAGS
  conf.linker.libraries << "setjmp" # expands to `-lsetjmp` (wasi-libc libsetjmp.a)

  # ---- ABI defines ---------------------------------------------------------
  BeniBuildConfig::ABI_DEFINES.each do |define|
    conf.cc.defines  << define
    conf.cxx.defines << define
  end
end

# Native host build — full libmruby.a plus the host mrbc the cross build
# borrows. +:gcc+ forces a bare +gcc+ so +Toolchain.guess+ cannot pick
# +:clang+ on macOS and resolve through PATH into wasi-sdk's clang
# (on macOS `gcc` is Apple clang anyway).
MRuby::Build.new("host") do |conf|
  conf.toolchain :gcc

  BeniBuildConfig::ABI_DEFINES.each do |define|
    conf.cc.defines  << define
    conf.cxx.defines << define
  end

  BeniBuildConfig::MRBGEM_BASELINE.each { |gem_name| conf.gem core: gem_name }
  conf.build_mrbc_exec
end

MRuby::CrossBuild.new("wasi") do |conf|
  conf.toolchain :wasi

  BeniBuildConfig::MRBGEM_BASELINE.each { |gem_name| conf.gem core: gem_name }
end
