# frozen_string_literal: true

# The per-ABI legs: each builds the vendored mruby under one
# verification config into its own scratch tree, then runs or links
# the crates against what that build staged.
module BeniRust
  # Scratch build dir for the default-ABI leg. Lives under tmp/
  # (gitignored) and is incremental across runs against one mruby
  # release.
  DEFAULT_ABI_BUILD_DIR = File.join(ROOT, "tmp", "mruby-default-build")

  # Scratch build dir for the 32-bit-float leg, kept apart from the
  # other ABIs' trees for the same reason they are kept apart from
  # each other.
  FLOAT32_BUILD_DIR = File.join(ROOT, "tmp", "mruby-float32-build")

  # Scratch build dir for the no-boxing leg, apart from the other ABIs'
  # trees for the same reason.
  NO_BOXING_BUILD_DIR = File.join(ROOT, "tmp", "mruby-no-boxing-build")

  # The default-ABI leg: build the vendored mruby with NO MRUBY_CONFIG
  # (mruby falls back to its own build_config/default.rb — what a
  # clean consumer build gets), then run the wrapper tests against
  # that archive. On 64-bit hosts this exercises the
  # mrb_int = int64_t layout the repo's MRB_INT32 validation config
  # never sees, so type/width coincidences cannot hide. The cargo
  # target dir is split off so the MRUBY_LIB_DIR switch does not
  # invalidate the main verification cache.
  def self.default_abi_test
    abi_test(upstream_default_lib_dir, "default-abi")
  end

  # The 32-bit-float leg: build the vendored mruby under the float32
  # verification config, then run the wrapper tests against that
  # archive. It is the only build that compiles the crates' 32-bit
  # float conversions, which no other ABI in the chain reaches. Its own
  # cargo target dir, since the MRUBY_LIB_DIR switch would otherwise
  # invalidate the main verification cache.
  def self.float32_test
    abi_test(float32_lib_dir, "float32")
  end

  # The no-boxing leg: build the vendored mruby under the no-boxing
  # verification config, run the wrapper tests against its host archive,
  # and link the wasm32 test binaries against its wasi one. It is the
  # only build where `mrb_value` is a tagged union, and its wasi archive
  # the only one pairing a 64-bit `mrb_int` with a 32-bit pointer.
  def self.no_boxing_test
    host_lib_dir, wasi_lib_dir = no_boxing_lib_dirs
    abi_test(host_lib_dir, "no-boxing")
    run!(wasm_env(wasi_lib_dir),
         "cargo", "test", "--workspace", "--target", WASM_TARGET, "--no-run",
         "--target-dir", File.join(ROOT, "target", "no-boxing"),
         chdir: ROOT)
  end

  # Run the wrapper tests against the archive staged in +lib_dir+, in
  # the cargo target dir +target+ names.
  def self.abi_test(lib_dir, target)
    run!({ "MRUBY_LIB_DIR" => lib_dir },
         "cargo", "test", "-p", "beni", "-p", "beni-tests",
         "--target-dir", File.join(ROOT, "target", target), *test_harness_args,
         chdir: ROOT)
  end

  # Build the vendored mruby under build_config/float32.rb and answer
  # the staged path.
  def self.float32_lib_dir
    staged_abi_lib_dirs(FLOAT32_BUILD_DIR,
                        { "MRUBY_CONFIG" => File.join(ROOT, "build_config", "float32.rb") }).first
  end

  # Build the vendored mruby under build_config/no_boxing.rb and answer
  # the host and the wasi staged paths.
  def self.no_boxing_lib_dirs
    staged_abi_lib_dirs(NO_BOXING_BUILD_DIR,
                        { "MRUBY_CONFIG" => File.join(ROOT, "build_config", "no_boxing.rb") },
                        targets: %w[host wasi])
  end

  # Build the vendored mruby with no MRUBY_CONFIG, so mruby's own
  # build_config/default.rb decides the ABI, and answer the staged path.
  # The documentation bindings are generated from the same build: both
  # want the surface a consumer gets before editing anything.
  def self.upstream_default_lib_dir
    staged_abi_lib_dirs(DEFAULT_ABI_BUILD_DIR).first
  end

  # Build the vendored mruby into +build_dir+ under +env+, and answer
  # the staged path of each of +targets+, in order.
  def self.staged_abi_lib_dirs(build_dir, env = {}, targets: %w[host])
    converge_build_dir(build_dir)
    lib_dirs = targets.map { |target| File.join(build_dir, target, "lib") }
    run!({ "MRUBY_BUILD_DIR" => build_dir, **env },
         RbConfig.ruby, "-S", "rake", "default",
         *lib_dirs.map { |lib_dir| File.join(lib_dir, Beni::Builder::FLAGS_MAK) },
         chdir: MRUBY_SOURCE_DIR)
    lib_dirs
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
