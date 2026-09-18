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
# It pins no integer width, so a 64-bit host settles on MRB_INT64 and
# the two configured widths differ from the validation build's in both
# axes at once. Nothing here is a product posture — it exists to be
# verified against.
#
# The file is `load`ed by mruby's rake when +MRUBY_CONFIG+ points at it.

MRuby::Lockfile.disable

require_relative "beni_build_config"

MRuby::Build.new("host") do |conf|
  conf.toolchain :gcc

  conf.cc.defines  << "MRB_USE_FLOAT32"
  conf.cxx.defines << "MRB_USE_FLOAT32"

  BeniBuildConfig::MRBGEM_BASELINE.each { |gem_name| conf.gem core: gem_name }
  conf.build_mrbc_exec
end
