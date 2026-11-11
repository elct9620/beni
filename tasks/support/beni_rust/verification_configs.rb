# frozen_string_literal: true

# The verification configs: each mruby build config the crates are
# verified against, the builds it stages, and the WASI target each of
# those compiles for. +beni:build+ stages build_config/mruby.rb into
# the vendor tree; every other config builds into a scratch tree of its
# own, so switching between them never rebuilds another's objects.
module BeniRust
  # One mruby build config the crates are verified against. +builds+
  # maps each mruby build it stages to the cargo target that links it,
  # +nil+ for the host. +config_file+ is +nil+ for mruby's own
  # +build_config/default.rb+.
  VerificationConfig = Data.define(:name, :config_file, :builds) do
    # Whether +beni:build+ stages this config into the vendor tree,
    # where host cargo runs discover it.
    def vendored?
      name == "mruby"
    end

    # The task that stages this config's archives.
    def staging_task
      vendored? ? "beni:build" : "rust:test:#{name}"
    end

    def build_dir
      return File.join(MRUBY_SOURCE_DIR, "build") if vendored?

      File.join(ROOT, "tmp", "mruby-#{name.tr("_", "-")}-build")
    end

    def lib_dir(build)
      File.join(build_dir, build, "lib")
    end

    def flags_mak_paths
      builds.keys.map { |build| File.join(lib_dir(build), Beni::Builder::FLAGS_MAK) }
    end

    # A config off the vendor tree keeps a cargo target dir of its own,
    # since switching MRUBY_LIB_DIR would invalidate the main one.
    def target_dir_args
      vendored? ? [] : ["--target-dir", File.join(ROOT, "target", name.tr("_", "-"))]
    end
  end

  class VerificationConfig
    # build_config/mruby.rb: MRB_INT32, word boxing, and the debug
    # hooks, on the host and every WASI target the crates link for.
    MRUBY = new(name: "mruby", config_file: File.join(ROOT, "build_config", "mruby.rb"),
                builds: { "host" => nil, **BeniBuildConfig::WASI_BUILDS })

    # mruby's untouched upstream default: a 64-bit +mrb_int+ on 64-bit
    # hosts, which build_config/mruby.rb never compiles.
    DEFAULT = new(name: "default", config_file: nil, builds: { "host" => nil })

    # The only config where +mrb_float+ is +float+, and the only one
    # with a 32-bit +mrb_int+ and no bigint.
    FLOAT32 = new(name: "float32", config_file: File.join(ROOT, "build_config", "float32.rb"),
                  builds: { "host" => nil })

    # The only config where +mrb_value+ is a tagged union; its wasi build
    # names no target, so it compiles for the wasi toolchain's default,
    # and is the only one pairing a 64-bit +mrb_int+ with a 32-bit
    # pointer.
    NO_BOXING = new(name: "no_boxing", config_file: File.join(ROOT, "build_config", "no_boxing.rb"),
                    builds: { "host" => nil, "wasi" => "wasm32-wasip1" })

    ALL = [MRUBY, DEFAULT, FLOAT32, NO_BOXING].freeze
  end

  # Run cargo +subcommand+ with +args+ against the archive +config+'s
  # +build+ staged, for the cargo target that build links.
  def self.cargo(config, build, subcommand, *)
    target = config.builds.fetch(build)
    require_wasm_target!(target) if target
    run!(cargo_env(config, build), "cargo", subcommand, *(["--target", target] if target),
         *config.target_dir_args, *, chdir: ROOT)
  end

  # Archive discovery env for one build: a WASI build names its staged
  # path and wasi-sdk root, the vendored host build is discovered
  # through the vendor tree, and any other host build is named directly.
  def self.cargo_env(config, build)
    return wasm_env(config.lib_dir(build)) if config.builds.fetch(build)
    return host_env if config.vendored?

    { "MRUBY_LIB_DIR" => config.lib_dir(build) }
  end

  # Build +config+ into its scratch tree under mruby's own rake, and
  # answer it. The vendored config is +beni:build+'s to stage.
  def self.stage(config)
    return config if config.vendored?

    converge_build_dir(config.build_dir)
    env = { "MRUBY_BUILD_DIR" => config.build_dir, "MRUBY_CONFIG" => config.config_file,
            "WASI_SDK_PATH" => Beni::Vendor.wasi_sdk_root(vendor_dir: VENDOR_DIR) }
    run!(env, RbConfig.ruby, "-S", "rake", "default", *config.flags_mak_paths, chdir: MRUBY_SOURCE_DIR)
    config
  end

  # Stop when +config+ has a build not staged yet, naming the task that
  # stages it. A gate that skipped instead would read as a pass.
  def self.require_staged!(config)
    missing = config.flags_mak_paths.reject { |path| File.exist?(path) }
    return if missing.empty?

    abort "#{missing.join(", ")} missing; run `bundle exec rake #{config.staging_task}` to stage it"
  end

  # Stage +config+, run the wrapper tests against its host archive, and
  # link the test binaries against each of its WASI archives.
  def self.test_against(config)
    stage(config)
    cargo(config, "host", "test", "-p", "beni", "-p", "beni-tests", *test_harness_args)
    config.builds.compact.each_key { |build| cargo(config, build, "test", "--workspace", "--no-run") }
  end

  # Lint the workspace against every build of every verification
  # config, each of which compiles a different shape of the crates.
  def self.clippy
    VerificationConfig::ALL.each do |config|
      require_staged!(config)
      config.builds.each do |build, target|
        cargo(config, build, "clippy", "--workspace", *("--all-targets" unless target), "--", "-D", "warnings")
      end
    end
  end

  # Start +build_dir+ over unless it was built from the mruby release
  # staged at +source_dir+. The tree lives outside the staged source, so
  # a re-extraction leaves it in place. mruby's make-style build would
  # then link the other release's objects.
  def self.converge_build_dir(build_dir, source_dir = MRUBY_SOURCE_DIR)
    marker = Beni::Vendor::Tarball::VERSION_MARKER
    release = File.read(File.join(source_dir, marker))
    stamp = File.join(build_dir, marker)
    return if File.exist?(stamp) && File.read(stamp) == release

    FileUtils.rm_rf(build_dir)
    FileUtils.mkdir_p(build_dir)
    File.write(stamp, release)
  end
end
