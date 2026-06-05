# frozen_string_literal: true

# mruby static-library build support module
# =========================================
#
# Pure-Ruby helpers backing +tasks/mruby.rake+. Owns the vendored
# +minirake+ invocation that produces the per-target +libmruby.a+
# archives. The .rake wrapper is the rake DSL surface that glues
# +BeniMruby.invoke_minirake+ to the +rake mruby:build+ task.

require "fileutils"
require "rbconfig"

# Build helpers for the vendored mruby tree. See sibling
# +tasks/mruby.rake+ for the rake DSL.
module BeniMruby
  ROOT          = File.expand_path("../..", __dir__)
  VENDOR_DIR    = (ENV["BENI_VENDOR_DIR"] || File.join(ROOT, "vendor")).freeze
  MRUBY_DIR     = File.join(VENDOR_DIR, "mruby").freeze
  BUILD_CONFIG  = File.join(ROOT, "build_config", "beni.rb").freeze

  # mruby places artefacts under `build/<target-name>/lib/libmruby.a`,
  # where `<target-name>` matches the `MRuby::Build.new(<name>)` /
  # `MRuby::CrossBuild.new(<name>)` argument in `build_config/beni.rb`.
  # Both targets build from the single config in one minirake run.
  TARGET_NAMES = %w[host wasi].freeze

  def self.libmruby_path(target)
    File.join(MRUBY_DIR, "build", target, "lib", "libmruby.a")
  end

  def self.libmruby_paths
    TARGET_NAMES.map { |t| libmruby_path(t) }
  end

  # Run mruby's minirake with our build config wired in via
  # MRUBY_CONFIG. mruby reads that env var (absolute path or basename
  # of a file under build_config/) to choose its top-level builds.
  def self.invoke_minirake(*args)
    env = { "MRUBY_CONFIG" => BUILD_CONFIG }
    cmd = [RbConfig.ruby, minirake, *args]
    puts "[mruby] cd #{MRUBY_DIR} && MRUBY_CONFIG=#{BUILD_CONFIG} #{cmd.join(" ")}"
    system(env, *cmd, chdir: MRUBY_DIR, exception: true)
  end

  # mruby ships a vendored copy of +minirake+ at the top of its tree.
  # Internal helper for +invoke_minirake+; not part of the public surface.
  def self.minirake
    File.join(MRUBY_DIR, "minirake")
  end

  private_class_method :minirake
end
