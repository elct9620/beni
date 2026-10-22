# frozen_string_literal: true

# The repo's 32-bit-float verification mruby build config.
# ========================================================
#
# One host target built with +MRB_USE_FLOAT32+, so `mrb_float` is
# `float` rather than `double`. The beni crates offer a different set of
# float conversions under each configured float width; this is the
# archive that compiles and runs the 32-bit set, which the validation
# config's double-width build never reaches.
#
# It also pins +MRB_INT32+ without the arbitrary-width Integer gem, the
# one integer posture no other verification build covers: the crates
# offer a narrower set of integer conversions where a 32-bit +mrb_int+
# has no bigint to fall back on. Nothing here is a product posture — it
# exists to be verified against.
#
# The file is `load`ed by mruby's rake when +MRUBY_CONFIG+ points at it.

MRuby::Lockfile.disable

require_relative "beni_build_config"

MRuby::Build.new("host") do |conf|
  conf.toolchain :gcc

  %w[MRB_USE_FLOAT32 MRB_INT32].each do |define|
    conf.cc.defines  << define
    conf.cxx.defines << define
  end

  BeniBuildConfig::MRBGEM_BASELINE.each { |gem_name| conf.gem core: gem_name }
  conf.build_mrbc_exec
end
