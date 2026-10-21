# frozen_string_literal: true

# The repo's no-boxing verification mruby build config.
# =====================================================
#
# A host and a wasm32-wasip1 build with +MRB_NO_BOXING+, so `mrb_value`
# is a tagged union rather than a single word. Both pin +MRB_INT64+,
# which mruby accepts on a 32-bit target only without boxing: this is
# the archive that compiles the crates against a 64-bit `mrb_int`
# beside a 32-bit pointer, a pairing no other build in the chain
# reaches. It carries the debug hooks too, so the debug surface compiles
# and runs over the tagged union. Nothing here is a product posture — it
# exists to be verified against.
#
# The file is `load`ed by mruby's rake when +MRUBY_CONFIG+ points at it.

MRuby::Lockfile.disable

require_relative "beni_build_config"

no_boxing_defines = %w[MRB_NO_BOXING MRB_INT64 MRB_USE_DEBUG_HOOK].freeze

MRuby::Build.new("host") do |conf|
  conf.toolchain :gcc

  no_boxing_defines.each do |define|
    conf.cc.defines  << define
    conf.cxx.defines << define
  end

  BeniBuildConfig::MRBGEM_BASELINE.each { |gem_name| conf.gem core: gem_name }
  conf.build_mrbc_exec
end

MRuby::CrossBuild.new("wasi") do |conf|
  conf.toolchain :wasi

  no_boxing_defines.each do |define|
    conf.cc.defines  << define
    conf.cxx.defines << define
  end

  BeniBuildConfig::MRBGEM_BASELINE.each { |gem_name| conf.gem core: gem_name }
end
