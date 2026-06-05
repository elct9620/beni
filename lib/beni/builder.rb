# frozen_string_literal: true

require "fileutils"
require "rbconfig"

module Beni
  # Drives the vendored mruby tree's build to produce the per-target
  # +libmruby.a+ archives, by running mruby's own +Rakefile+ — the
  # documented build entry point (doc/guides/compile.md) — with the
  # build config wired in via the +MRUBY_CONFIG+ env var, which mruby
  # resolves as an absolute path.
  class Builder
    # Build-target names matching the +MRuby::Build.new(<name>)+ /
    # +MRuby::CrossBuild.new(<name>)+ arguments in the gem-shipped
    # default config; mruby places artefacts under
    # +build/<target-name>/lib/libmruby.a+. A custom build config with
    # different target names supplies its own list via +Beni::Tasks+.
    DEFAULT_TARGETS = %w[host wasi].freeze

    attr_reader :vendor_dir, :build_config, :targets

    def initialize(vendor_dir:, build_config:, targets: DEFAULT_TARGETS)
      @vendor_dir = vendor_dir
      @build_config = build_config
      @targets = targets
    end

    def mruby_dir
      File.join(vendor_dir, "mruby")
    end

    def libmruby_path(target)
      File.join(mruby_dir, "build", target, "lib", "libmruby.a")
    end

    def libmruby_paths
      targets.map { |target| libmruby_path(target) }
    end

    # True when every target's +libmruby.a+ is already present, letting
    # callers skip the build without spawning a subprocess.
    def built?
      libmruby_paths.all? { |path| File.exist?(path) }
    end

    # Idempotent build entry point for +rake beni:build+: skip with a
    # note when every artifact is already present, otherwise build and
    # report readiness.
    def ensure_built
      if built?
        puts "[beni] libmruby.a already present for #{targets.join(" + ")} — skipping"
        return
      end

      build
      puts "[beni] libmruby.a ready for #{targets.join(" + ")}"
    end

    # Run mruby's rake against +build_config+ and raise unless every
    # target's +libmruby.a+ exists afterwards. The underlying build is
    # make-style incremental, so re-running on a partially built tree
    # only compiles what is missing.
    def build
      cmd = [RbConfig.ruby, "-S", "rake"]
      puts "[beni] cd #{mruby_dir} && MRUBY_CONFIG=#{build_config} #{cmd.join(" ")}"
      run_mruby_rake(cmd)
      verify_artifacts!
    end

    # Remove each target's build tree (keeps the vendored mruby source).
    def clean
      targets.each do |target|
        dir = File.join(mruby_dir, "build", target)
        FileUtils.rm_rf(dir)
        puts "[beni] removed #{dir}"
      end
    end

    private

    # Spawn mruby's rake with the parent environment plus the +env+
    # overlay. Extracted as a seam so tests can fake the subprocess.
    def run_mruby_rake(cmd)
      system(env, *cmd, chdir: mruby_dir, exception: true)
    end

    # Environment for the mruby build subprocess. +BENI_VENDOR_DIR+ and
    # +BENI_BUILD_CONFIG_DIR+ let build configs resolve the vendor tree
    # and the gem-shipped config templates without knowing where the gem
    # or the consuming project lives on disk.
    def env
      {
        "MRUBY_CONFIG" => build_config,
        "BENI_VENDOR_DIR" => vendor_dir,
        "BENI_BUILD_CONFIG_DIR" => BUILD_CONFIG_DIR
      }
    end

    def verify_artifacts!
      libmruby_paths.each do |path|
        raise Error, "[beni] build completed but #{path} is missing" unless File.exist?(path)
      end
    end
  end
end
