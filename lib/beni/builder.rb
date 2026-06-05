# frozen_string_literal: true

require "fileutils"
require "rbconfig"

module Beni
  # Drives the vendored mruby tree's build to produce the per-target
  # +libmruby.a+ archives, by running mruby's own +Rakefile+ — the
  # documented build entry point (doc/guides/compile.md). With no
  # +build_config+, mruby falls back to its own
  # +build_config/default.rb+; an explicit config is wired in via the
  # +MRUBY_CONFIG+ env var, which mruby resolves as an absolute path.
  class Builder
    # mruby's anonymous +MRuby::Build.new+ names its target "host"
    # (lib/mruby/build.rb), so the upstream default config produces
    # +build/host/lib/libmruby.a+. A custom build config with
    # different target names supplies its own list via +Beni::Tasks+.
    DEFAULT_TARGETS = %w[host].freeze

    attr_reader :vendor_dir, :build_config, :targets

    def initialize(vendor_dir:, build_config: nil, targets: DEFAULT_TARGETS)
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

    # True when every target's artifacts — +libmruby.a+ plus the
    # +libmruby.flags.mak+ sidecar +beni-sys+ parses for ABI alignment
    # — are already present, letting callers skip the build without
    # spawning a subprocess. An archive without its sidecar (e.g. a
    # tree built before flags.mak joined the contract) triggers a
    # rebuild, which is incremental and only emits the missing file.
    def built?
      artifact_paths.all? { |path| File.exist?(path) }
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

    # Run mruby's rake and raise unless every target's +libmruby.a+
    # exists afterwards. Alongside the default task, each target's
    # +libmruby.flags.mak+ file task is requested explicitly — mruby's
    # embedder interface recording the exact compile flags, which
    # +beni-sys+'s build script parses to keep bindgen's view of the
    # ABI aligned with the archive. (The file task is defined per
    # target but not part of mruby's default products.) The underlying
    # build is make-style incremental, so re-running on a partially
    # built tree only compiles what is missing.
    def build
      cmd = [RbConfig.ruby, "-S", "rake", "default", *flags_mak_paths]
      puts "[beni] cd #{mruby_dir} && #{env.map { |k, v| "#{k}=#{v}" }.join(" ")} #{cmd.join(" ")}"
      run_mruby_rake(env, cmd)
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

    # Every artifact the build must leave behind: the per-target
    # archive and its flags.mak sidecar.
    def artifact_paths
      libmruby_paths + flags_mak_paths
    end

    # Spawn mruby's rake with the parent environment plus the +env+
    # overlay. Extracted as a seam so tests can fake the subprocess
    # while observing the full env + cmd contract.
    def run_mruby_rake(env, cmd)
      system(env, *cmd, chdir: mruby_dir, exception: true)
    end

    # Environment for the mruby build subprocess. +BENI_VENDOR_DIR+
    # lets build configs resolve the vendor tree without knowing where
    # the consuming project lives on disk. +MRUBY_CONFIG+ is only set
    # for an explicit config — absent, mruby falls back to its own
    # +build_config/default.rb+ (lib/mruby/build.rb#mruby_config_path).
    def env
      env = { "BENI_VENDOR_DIR" => vendor_dir }
      env["MRUBY_CONFIG"] = build_config if build_config
      env
    end

    # Per-target +libmruby.flags.mak+ file-task paths, matching the
    # task names mruby defines in tasks/libmruby.rake (absolute,
    # anchored on the default +build/<target>+ layout).
    def flags_mak_paths
      targets.map { |target| File.join(mruby_dir, "build", target, "lib", "libmruby.flags.mak") }
    end

    def verify_artifacts!
      artifact_paths.each do |path|
        raise Error, "[beni] build completed but #{path} is missing" unless File.exist?(path)
      end
    end
  end
end
