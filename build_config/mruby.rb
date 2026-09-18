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

require_relative "beni_build_config"

# Native host build — the full archive plus the host mrbc the cross build
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
