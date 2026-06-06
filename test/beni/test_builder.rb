# frozen_string_literal: true

require "test_helper"
require "tmpdir"

module Beni
  # Artifact paths, built-state detection, and cleanup. The build
  # entry points (subprocess contract, config validation, artifact
  # verification) live in TestBuilderBuild.
  class TestBuilder < Minitest::Test
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

    def test_clean_removes_target_build_trees_but_keeps_source
      touch_libmruby(@builder, "host")
      source = File.join(@dir, "mruby", "src")
      FileUtils.mkdir_p(source)

      capture_io { @builder.clean }

      refute_path_exists File.join(@dir, "mruby", "build", "host")
      assert_path_exists source
    end

    private

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
