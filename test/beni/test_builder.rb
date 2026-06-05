# frozen_string_literal: true

require "test_helper"
require "tmpdir"

module Beni
  class TestBuilder < Minitest::Test
    # Extract-and-override seam: pretends mruby's rake ran without
    # spawning a subprocess, so the artifact-verification contract can
    # be tested in isolation.
    class FakeRakeBuilder < Builder
      attr_reader :ran

      private

      def run_mruby_rake(_cmd)
        @ran = true
      end
    end

    def setup
      @dir = Dir.mktmpdir("beni-builder")
      @builder = Builder.new(vendor_dir: @dir, build_config: "/path/to/config.rb")
    end

    def teardown
      FileUtils.remove_entry(@dir)
    end

    def test_libmruby_paths_follow_mruby_build_layout
      expected = %w[host wasi].map { |t| File.join(@dir, "mruby", "build", t, "lib", "libmruby.a") }

      assert_equal expected, @builder.libmruby_paths
    end

    def test_targets_are_customizable
      builder = Builder.new(vendor_dir: @dir, build_config: "config.rb", targets: %w[embedded])

      assert_equal [File.join(@dir, "mruby", "build", "embedded", "lib", "libmruby.a")],
                   builder.libmruby_paths
    end

    def test_built_eh_is_false_until_every_target_artifact_exists
      refute_predicate @builder, :built?

      touch_libmruby("host")

      refute_predicate @builder, :built?

      touch_libmruby("wasi")

      assert_predicate @builder, :built?
    end

    def test_ensure_built_skips_the_build_when_artifacts_exist
      touch_libmruby("host")
      touch_libmruby("wasi")

      output, = capture_io { @builder.ensure_built }

      assert_includes output, "skipping"
    end

    def test_build_raises_when_artifacts_are_missing_after_the_run
      builder = FakeRakeBuilder.new(vendor_dir: @dir, build_config: "/path/to/config.rb")

      error = assert_raises(Beni::Error) do
        capture_io { builder.build }
      end

      assert builder.ran, "expected the rake seam to have been invoked"
      assert_match(/missing/, error.message)
    end

    def test_clean_removes_target_build_trees_but_keeps_source
      touch_libmruby("host")
      source = File.join(@dir, "mruby", "src")
      FileUtils.mkdir_p(source)

      capture_io { @builder.clean }

      refute_path_exists File.join(@dir, "mruby", "build", "host")
      assert_path_exists source
    end

    private

    def touch_libmruby(target)
      path = @builder.libmruby_path(target)
      FileUtils.mkdir_p(File.dirname(path))
      FileUtils.touch(path)
    end
  end
end
