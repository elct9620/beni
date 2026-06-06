# frozen_string_literal: true

# The repo's validation mruby build config.
# ==========================================
#
# A tuned build beyond the upstream defaults — the consumer posture
# `rake beni:config` seeds (generate the upstream default, then edit):
# a native +host+ build plus a wasm32-wasip1 cross build, sharing one
# set of ABI-bearing defines and one mrbgem baseline, so the beni
# crates verify against both targets.
#
# The file is `load`ed by mruby's rake when +Beni::Builder+ sets
# +MRUBY_CONFIG+ to its absolute path. The wasi cross build's
# +conf.toolchain :wasi+ resolves to the wasi toolchain file beni
# stages into the mruby tree; +WASI_SDK_PATH+ overrides the wasi-sdk
# location it points at.

# mruby auto-enables its mrbgems lockfile (MRuby::Lockfile's class body
# calls +enable+ on load) and writes it next to MRUBY_CONFIG. Dependency
# pinning is beni's own (future) lock mechanism, not mruby's, so this
# config opts out.
MRuby::Lockfile.disable

# Config-time constants shared across the targets below. Only defined
# on first load, so `load`-ing this file twice in the same process does
# not warn about constant redefinition.
unless defined?(BeniBuildConfig)
  # Config-time constants shared across the targets below.
  module BeniBuildConfig
    # ABI-bearing defines applied to BOTH the host and wasi targets,
    # keeping `mrb_int` width and float boxing identical across them
    # (without MRB_INT32 a 64-bit host defaults to MRB_INT64 while
    # wasm32 stays 32-bit — see mruby's mrbconf.h). The beni crates
    # align themselves automatically: their build script parses the
    # `libmruby.flags.mak` sidecar each build leaves next to the
    # archive, so edits here flow into bindgen without code changes.
    ABI_DEFINES = %w[
      MRB_INT32
      MRB_WORDBOX_NO_INLINE_FLOAT
    ].freeze

    # Core-gem baseline shared by both targets: mruby-compiler (the
    # wrapper's `mrb_load_nstring` needs it) plus the portable core
    # extension gems. No I/O / network / process gems — those do not
    # exist on wasm32-wasip1, and keeping the two targets' gem sets
    # identical keeps the verified surface identical.
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
