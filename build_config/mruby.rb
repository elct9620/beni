# frozen_string_literal: true

# The repo's validation mruby build config.
# ==========================================
#
# A tuned build beyond the upstream defaults — the consumer posture
# `rake beni:config` seeds (generate the upstream default, then edit):
# a native +host+ build plus wasm32-wasip1 and wasm32-wasip2 cross
# builds, sharing one set of ABI-bearing defines and one mrbgem
# baseline, so the beni crates verify against every target. All add
# the arbitrary-width Integer gem beside the baseline, making this the
# bigint archive with a 32-bit +mrb_int+ — the pairing where values a
# Rust +i64+ holds already outgrow the fixed-width tag — while the
# float32 and no-boxing configs stay without it. mruby-rational needs
# it to build.
#
# The file is `load`ed by mruby's rake when +Beni::Builder+ sets
# +MRUBY_CONFIG+ to its absolute path. The wasi cross builds'
# +conf.toolchain :wasi+ resolves to the wasi toolchain file in the
# mruby tree — the release's own or beni's; +WASI_SDK_PATH+ overrides
# the wasi-sdk location it points at.

# mruby auto-enables its mrbgems lockfile (MRuby::Lockfile's class body
# calls +enable+ on load) and writes it next to MRUBY_CONFIG. Dependency
# pinning is beni's own (future) lock mechanism, not mruby's, so this
# config opts out.
MRuby::Lockfile.disable

require_relative "beni_build_config"

bigint_gems = %w[mruby-bigint mruby-rational].freeze

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

  (BeniBuildConfig::MRBGEM_BASELINE + bigint_gems).each { |gem_name| conf.gem core: gem_name }
  conf.build_mrbc_exec
end

# One cross build per WASI target the crates link for.
{ "wasi" => "wasm32-wasip1", "wasip2" => "wasm32-wasip2" }.each do |name, target|
  MRuby::CrossBuild.new(name) do |conf|
    conf.toolchain :wasi, target: target

    BeniBuildConfig::ABI_DEFINES.each do |define|
      conf.cc.defines  << define
      conf.cxx.defines << define
    end

    (BeniBuildConfig::MRBGEM_BASELINE + bigint_gems).each { |gem_name| conf.gem core: gem_name }
  end
end
