# frozen_string_literal: true

require "fileutils"
require "rbconfig"

module Beni
  # Drives the vendored mruby tree's build to produce the per-target
  # archives, by running mruby's own +Rakefile+ — the
  # documented build entry point (doc/guides/compile.md). With no
  # +build_config+, mruby falls back to its own
  # +build_config/default.rb+; an explicit config is wired in via the
  # +MRUBY_CONFIG+ env var, which mruby resolves as an absolute path.
  class Builder
    # mruby's anonymous +MRuby::Build.new+ names its target "host"
    # (lib/mruby/build.rb), so the upstream default config produces
    # +build/host/lib/+. A custom build config with different target
    # names supplies its own list via +Beni::Tasks+.
    DEFAULT_TARGETS = %w[host].freeze

    # The sidecar line naming the archive's own path. An archive's file
    # name follows the toolchain that built it, so the sidecar is what
    # says which file to look for.
    ARCHIVE_PATH_KEY = "MRUBY_LIBMRUBY_PATH = "

    # The sidecar mruby writes beside every archive it builds.
    FLAGS_MAK = "libmruby.flags.mak"

    attr_reader :vendor_dir, :build_config, :targets

    def initialize(vendor_dir:, build_config: nil, targets: DEFAULT_TARGETS)
      @vendor_dir = vendor_dir
      @build_config = build_config
      @targets = targets
    end

    def mruby_dir
      File.join(vendor_dir, "mruby")
    end

    # Where one target's archive and its sidecar stage.
    def staged_path(target)
      File.join(mruby_dir, "build", target, "lib")
    end

    def staged_paths
      targets.map { |target| staged_path(target) }
    end

    # Per-target +libmruby.flags.mak+ file-task paths, matching the
    # task names mruby defines in tasks/libmruby.rake (absolute,
    # anchored on the default +build/<target>+ layout).
    def flags_mak_path(target)
      File.join(staged_path(target), FLAGS_MAK)
    end

    # The archive one target's sidecar names, or +nil+ when no sidecar
    # is there to name one.
    def archive_path(target)
      sidecar = flags_mak_path(target)
      return nil unless File.exist?(sidecar)

      File.join(staged_path(target), archive_file_name(sidecar))
    end

    # True when every target has its +libmruby.flags.mak+ sidecar — the
    # channel +beni-sys+ parses for ABI alignment — and the archive that
    # sidecar names, letting callers skip the build without spawning a
    # subprocess. A missing sidecar triggers a rebuild, which is
    # incremental and only emits what it lacks.
    def built?
      targets.all? { |target| exist?(archive_path(target)) }
    end

    # Idempotent build entry point for +rake beni:build+: skip with a
    # note when every artifact is already present, otherwise build and
    # report readiness. A declared config that does not exist aborts
    # before the skip check — stale artifacts must not mask it.
    def ensure_built
      check_build_config!
      if built?
        puts "[beni] archive already present for #{targets.join(" + ")} — skipping"
        return
      end

      build
      puts "[beni] archive ready for #{targets.join(" + ")}"
    end

    # Run mruby's rake and raise unless every target's archive exists
    # afterwards. Alongside the default task, each target's
    # +libmruby.flags.mak+ file task is requested explicitly — mruby's
    # embedder interface recording the exact compile flags, which
    # +beni-sys+'s build script parses to keep bindgen's view of the
    # ABI aligned with the archive. (The file task is defined per
    # target but not part of mruby's default products.) The underlying
    # build is make-style incremental, so re-running on a partially
    # built tree only compiles what is missing.
    def build
      check_build_config!
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

    # The archive's file name, as its sidecar names the path to it.
    # mruby writes that path through a make variable no reader outside
    # make can expand, so the name is what is taken from it.
    def archive_file_name(sidecar)
      line = File.foreach(sidecar).find { |candidate| candidate.start_with?(ARCHIVE_PATH_KEY) }
      raise Error, "[beni] #{sidecar} has no #{ARCHIVE_PATH_KEY.strip} line" unless line

      File.basename(line.delete_prefix(ARCHIVE_PATH_KEY).strip.tr("\\", "/"))
    end

    def exist?(path)
      !path.nil? && File.exist?(path)
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

    def flags_mak_paths
      targets.map { |target| flags_mak_path(target) }
    end

    # The declared config belongs to the consumer; a path that does
    # not exist is a configuration error named before anything spawns.
    def check_build_config!
      return if build_config.nil? || File.exist?(build_config)

      raise Error, "[beni] build config #{build_config} does not exist"
    end

    # Report every missing artifact at once, so a multi-target build
    # failure shows the whole gap instead of one path per run. A target
    # with no sidecar is reported by the sidecar, which is the artifact
    # naming the other one.
    def verify_artifacts!
      missing = targets.flat_map do |target|
        archive = archive_path(target)
        archive.nil? ? [flags_mak_path(target)] : [archive].reject { |path| File.exist?(path) }
      end
      return if missing.empty?

      raise Error, "[beni] build completed but artifacts are missing:\n  #{missing.join("\n  ")}"
    end
  end
end
