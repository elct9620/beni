# frozen_string_literal: true

# The beni repo's own validation build config (host + wasm32-wasip1).
# ====================================================================
#
# NOT the gem's default: +Beni::Tasks+ defaults to no +MRUBY_CONFIG+,
# letting mruby use its untouched upstream +build_config/default.rb+.
# This config exists to validate the toolchain against the repo's
# wasm verification target, mirroring the downstream (kobako) setup.
#
# Drives mruby's build system to produce the `libmruby.a` archives the
# beni crates link against, one per default target:
#
#   * `<vendor_dir>/mruby/build/host/lib/libmruby.a` — native build for
#     the local machine (macOS / Linux), used by host `cargo test`.
#   * `<vendor_dir>/mruby/build/wasi/lib/libmruby.a` — cross-compiled
#     for wasm32-wasip1 against the vendored wasi-sdk, used by
#     `cargo check --target wasm32-wasip1`.
#
# Both targets share the same ABI-bearing `-D` defines (see
# +BeniBuildConfig::ABI_DEFINES+): the typed wrapper in `crates/beni`
# pins MRB_INT32 + MRB_WORDBOX_NO_INLINE_FLOAT semantics, so every
# libmruby.a it links against must be built — and bindgen'd — with the
# same layout. Consumers with similar needs point +Beni::Tasks+ at
# their own config (this file is a copyable starting point — see
# wasi_toolchain.rb, the sibling file it loads by relative path).
#
# This file is `load`ed by mruby's rake when +Beni::Builder+ sets
# `MRUBY_CONFIG` to its absolute path; vendor paths resolve through the
# `BENI_VENDOR_DIR` env var the builder exports alongside it.

# The +:wasi+ MRuby::Toolchain (wasi-sdk tools, wasm32-wasi target /
# sysroot flags, setjmp/longjmp). Sibling template shipped with the gem.
load File.expand_path("wasi_toolchain.rb", __dir__)

# mruby auto-enables its mrbgems lockfile (MRuby::Lockfile's class body
# calls +enable+ on load) and writes it next to MRUBY_CONFIG. Dependency
# pinning is beni's own (future) lock mechanism, not mruby's, so this
# config opts out.
MRuby::Lockfile.disable

# Config-time constants shared across both default targets. Only defined
# on first load, so `load`-ing this file twice in the same process does
# not warn about constant redefinition.
unless defined?(BeniBuildConfig)
  # Config-time constants shared across both default targets.
  module BeniBuildConfig
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

    # Core-gem baseline shared by both targets: mruby-compiler (the
    # wrapper's `mrb_load_nstring` needs it) plus the portable core
    # extension gems. No I/O / network / process gems — those do not
    # exist on wasm32-wasip1, and keeping the two targets' gem sets
    # identical keeps the verified surface identical. Consumers with
    # different needs bring their own build config; this list is only
    # beni's default baseline.
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

  BeniBuildConfig::ABI_DEFINES.each do |define|
    conf.cc.defines  << define
    conf.cxx.defines << define
  end

  BeniBuildConfig::MRBGEM_BASELINE.each { |gem_name| conf.gem core: gem_name }
end
