# frozen_string_literal: true

require "test_helper"
require "tmpdir"

module Beni
  class TestBuilder < Minitest::Test
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
      @builder = Builder.new(vendor_dir: @dir)
    end

    def teardown
      FileUtils.remove_entry(@dir)
    end

    def test_default_target_is_mruby_upstream_host
      assert_equal [File.join(@dir, "mruby", "build", "host", "lib", "libmruby.a")],
                   @builder.libmruby_paths
    end

    def test_targets_are_customizable
      builder = Builder.new(vendor_dir: @dir, targets: %w[host wasi])

      expected = %w[host wasi].map { |t| File.join(@dir, "mruby", "build", t, "lib", "libmruby.a") }

      assert_equal expected, builder.libmruby_paths
    end

    def test_built_eh_is_false_until_every_target_artifact_exists
      builder = Builder.new(vendor_dir: @dir, targets: %w[host wasi])

      refute_predicate builder, :built?

      touch_libmruby(builder, "host")

      refute_predicate builder, :built?

      touch_libmruby(builder, "wasi")

      assert_predicate builder, :built?
    end

    def test_built_eh_requires_flags_mak_alongside_each_archive
      builder = Builder.new(vendor_dir: @dir)
      path = builder.libmruby_path("host")
      FileUtils.mkdir_p(File.dirname(path))
      FileUtils.touch(path)

      refute_predicate builder, :built?,
                       "an archive without libmruby.flags.mak must trigger a (cheap, incremental) rebuild"
    end

    def test_ensure_built_skips_the_build_when_artifacts_exist
      touch_libmruby(@builder, "host")

      output, = capture_io { @builder.ensure_built }

      assert_includes output, "skipping"
    end

    def test_build_without_build_config_lets_mruby_use_its_own_default
      builder = fake_built_builder

      capture_io { builder.build }

      refute_includes builder.recorded_env, "MRUBY_CONFIG"
    end

    def test_build_passes_an_explicit_build_config_through_mruby_config
      builder = fake_built_builder(build_config: "/path/to/config.rb")

      capture_io { builder.build }

      assert_equal "/path/to/config.rb", builder.recorded_env["MRUBY_CONFIG"]
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

    def test_clean_removes_target_build_trees_but_keeps_source
      touch_libmruby(@builder, "host")
      source = File.join(@dir, "mruby", "src")
      FileUtils.mkdir_p(source)

      capture_io { @builder.clean }

      refute_path_exists File.join(@dir, "mruby", "build", "host")
      assert_path_exists source
    end

    private

    # A fake-seam builder whose artifacts already exist, so `build`
    # records the subprocess contract and passes verification.
    def fake_built_builder(**)
      builder = FakeRakeBuilder.new(vendor_dir: @dir, **)
      builder.targets.each { |target| touch_libmruby(builder, target) }
      builder
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
