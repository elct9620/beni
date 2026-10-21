# frozen_string_literal: true

require "test_helper"
require "tmpdir"
require_relative "fixtures"

module Beni
  # Artifact paths and cleanup. The build
  # entry points (subprocess contract, config validation, artifact
  # verification) live in TestBuilderBuild.
  class TestBuilder < Minitest::Test
    include Fixtures

    def setup
      @dir = Dir.mktmpdir("beni-builder")
      @builder = Builder.new(vendor_dir: @dir)
    end

    def teardown
      FileUtils.remove_entry(@dir)
    end

    def test_default_target_is_mruby_upstream_host
      assert_equal [File.join(@dir, "mruby", "build", "host", "lib")], @builder.staged_paths
    end

    def test_targets_are_customizable
      builder = Builder.new(vendor_dir: @dir, targets: %w[host wasi])

      expected = %w[host wasi].map { |t| File.join(@dir, "mruby", "build", t, "lib") }

      assert_equal expected, builder.staged_paths
    end

    def test_the_archive_looked_for_is_the_one_its_sidecar_names
      touch_libmruby(@builder, "host", archive: "libmruby.lib")

      assert_equal File.join(@builder.staged_path("host"), "libmruby.lib"),
                   @builder.archive_path("host")
    end

    def test_no_archive_is_looked_for_without_its_sidecar
      builder = Builder.new(vendor_dir: @dir)
      FileUtils.mkdir_p(builder.staged_path("host"))
      FileUtils.touch(File.join(builder.staged_path("host"), "libmruby.a"))

      assert_nil builder.archive_path("host"),
                 "with no sidecar there is nothing naming an archive to look for"
    end

    def test_clean_removes_target_build_trees_but_keeps_source
      touch_libmruby(@builder, "host")
      source = File.join(@dir, "mruby", "src")
      FileUtils.mkdir_p(source)

      capture_io { @builder.clean }

      refute_path_exists File.join(@dir, "mruby", "build", "host")
      assert_path_exists source
    end
  end
end
