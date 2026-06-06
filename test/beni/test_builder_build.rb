# frozen_string_literal: true

require "test_helper"
require "tmpdir"

module Beni
  # The build entry points: subprocess contract, config validation,
  # and artifact verification. Path/cleanup behavior lives in
  # TestBuilder.
  class TestBuilderBuild < Minitest::Test
    # Extract-and-override seam: pretends mruby's rake ran without
    # spawning a subprocess, recording the env + cmd contract so the
    # subprocess wiring and artifact verification can be tested in
    # isolation.
    class FakeRakeBuilder < Builder
      attr_reader :ran, :recorded_env, :recorded_cmd

      private

      def run_mruby_rake(env, cmd)
        @ran = true
        @recorded_env = env
        @recorded_cmd = cmd
      end
    end

    def setup
      @dir = Dir.mktmpdir("beni-builder")
    end

    def teardown
      FileUtils.remove_entry(@dir)
    end

    def test_build_without_build_config_lets_mruby_use_its_own_default
      builder = fake_built_builder

      capture_io { builder.build }

      refute_includes builder.recorded_env, "MRUBY_CONFIG"
    end

    def test_build_passes_an_explicit_build_config_through_mruby_config
      config = touch_build_config
      builder = fake_built_builder(build_config: config)

      capture_io { builder.build }

      assert_equal config, builder.recorded_env["MRUBY_CONFIG"]
    end

    def test_build_aborts_naming_a_missing_build_config
      missing = File.join(@dir, "missing_config.rb")
      builder = FakeRakeBuilder.new(vendor_dir: @dir, build_config: missing)

      error = assert_raises(Beni::Error) do
        capture_io { builder.build }
      end

      assert_includes error.message, missing
      refute builder.ran, "mruby's rake must not spawn for a missing config"
    end

    def test_ensure_built_aborts_on_missing_config_even_when_artifacts_exist
      missing = File.join(@dir, "missing_config.rb")
      builder = FakeRakeBuilder.new(vendor_dir: @dir, build_config: missing)
      builder.targets.each { |target| touch_libmruby(builder, target) }

      error = assert_raises(Beni::Error) do
        capture_io { builder.ensure_built }
      end

      assert_includes error.message, missing,
                      "stale artifacts must not mask a missing config"
    end

    def test_build_exports_the_vendor_dir_and_nothing_gem_specific
      builder = fake_built_builder

      capture_io { builder.build }

      assert_equal @dir, builder.recorded_env["BENI_VENDOR_DIR"]
      refute_includes builder.recorded_env, "BENI_BUILD_CONFIG_DIR"
    end

    def test_build_requests_flags_mak_alongside_the_default_task
      builder = fake_built_builder

      capture_io { builder.build }

      flags_mak = File.join(@dir, "mruby", "build", "host", "lib", "libmruby.flags.mak")

      assert_includes builder.recorded_cmd, "default"
      assert_includes builder.recorded_cmd, flags_mak
    end

    def test_build_raises_when_artifacts_are_missing_after_the_run
      builder = FakeRakeBuilder.new(vendor_dir: @dir)

      error = assert_raises(Beni::Error) do
        capture_io { builder.build }
      end

      assert builder.ran, "expected the rake seam to have been invoked"
      assert_match(/missing/, error.message)
    end

    def test_verification_reports_every_missing_artifact
      builder = FakeRakeBuilder.new(vendor_dir: @dir, targets: %w[host wasi])

      error = assert_raises(Beni::Error) do
        capture_io { builder.build }
      end

      assert_includes error.message, builder.libmruby_path("host")
      assert_includes error.message, builder.libmruby_path("wasi")
    end

    private

    # A fake-seam builder whose artifacts already exist, so `build`
    # records the subprocess contract and passes verification.
    def fake_built_builder(**)
      builder = FakeRakeBuilder.new(vendor_dir: @dir, **)
      builder.targets.each { |target| touch_libmruby(builder, target) }
      builder
    end

    # A real (empty) config file in the tmpdir — the build aborts on
    # a config path that does not exist, so pass-through tests need
    # one on disk.
    def touch_build_config
      path = File.join(@dir, "config.rb")
      FileUtils.touch(path)
      path
    end

    # Fakes a fully built target: the archive plus the flags.mak
    # sidecar the build always requests alongside it.
    def touch_libmruby(builder, target)
      path = builder.libmruby_path(target)
      FileUtils.mkdir_p(File.dirname(path))
      FileUtils.touch(path)
      FileUtils.touch(File.join(File.dirname(path), "libmruby.flags.mak"))
    end
  end
end
