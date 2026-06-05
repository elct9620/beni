# frozen_string_literal: true

require "test_helper"
require "rake"
require "tmpdir"
require "beni/tasks"

module Beni
  class TestTasks < Minitest::Test
    VENDOR_DIR = "/tmp/beni-tasks-test/vendor"

    def setup
      @original_application = Rake.application
      Rake.application = Rake::Application.new
    end

    def teardown
      Rake.application = @original_application
    end

    def test_defines_the_beni_task_suite
      Tasks.new { |tasks| tasks.vendor_dir = VENDOR_DIR }

      %w[
        beni:build beni:clean
        beni:vendor:setup beni:vendor:setup:wasi_sdk beni:vendor:setup:mruby
        beni:vendor:clean beni:vendor:clobber
      ].each do |name|
        assert Rake::Task.task_defined?(name), "expected task #{name} to be defined"
      end
    end

    def test_build_depends_on_vendor_setup
      Tasks.new { |tasks| tasks.vendor_dir = VENDOR_DIR }

      prerequisites = Rake::Task["beni:build"].prerequisite_tasks.map(&:name)

      assert_includes prerequisites, "beni:vendor:setup"
    end

    def test_tarball_file_tasks_are_anchored_on_vendor_dir
      Tasks.new { |tasks| tasks.vendor_dir = VENDOR_DIR }

      mruby_tarball = File.join(VENDOR_DIR, ".cache", "#{Vendor::MRUBY_VERSION}.tar.gz")

      assert Rake::Task.task_defined?(mruby_tarball), "expected file task for #{mruby_tarball}"
      assert_includes Rake::Task["beni:vendor:setup:mruby"].prerequisites, mruby_tarball
    end

    def test_default_build_config_is_shipped_with_the_gem
      tasks = Tasks.new

      assert_equal Tasks::DEFAULT_BUILD_CONFIG, tasks.build_config
      assert_path_exists Tasks::DEFAULT_BUILD_CONFIG
    end

    def test_vendor_clean_removes_unpacked_trees_but_keeps_the_tarball_cache
      with_vendor_fixture do |dir, unpacked, cache|
        Rake::Task["beni:vendor:clean"].invoke

        refute_path_exists unpacked
        assert_path_exists cache
        assert_path_exists dir
      end
    end

    def test_vendor_clobber_removes_the_vendor_tree_entirely
      with_vendor_fixture do |dir, _unpacked, _cache|
        Rake::Task["beni:vendor:clobber"].invoke

        refute_path_exists dir
      end
    end

    def test_clean_removes_mruby_build_trees
      with_vendor_fixture do |dir, _unpacked, _cache|
        build_tree = File.join(dir, "mruby", "build", "host")
        FileUtils.mkdir_p(build_tree)

        capture_io { Rake::Task["beni:clean"].invoke }

        refute_path_exists build_tree
      end
    end

    def test_build_config_and_targets_are_customizable
      tasks = Tasks.new do |config|
        config.vendor_dir = VENDOR_DIR
        config.build_config = "/custom/config.rb"
        config.targets = %w[embedded]
      end

      assert_equal "/custom/config.rb", tasks.build_config
      assert_equal %w[embedded], tasks.targets
    end

    private

    # Builds a disposable vendor tree (one unpacked toolchain dir plus
    # the tarball cache) and defines the beni:* tasks against it, so
    # the cleanup tasks can be invoked for real.
    def with_vendor_fixture
      dir = Dir.mktmpdir("beni-tasks")
      Tasks.new { |tasks| tasks.vendor_dir = dir }
      unpacked = File.join(dir, "wasi-sdk")
      cache = File.join(dir, ".cache")
      FileUtils.mkdir_p(unpacked)
      FileUtils.mkdir_p(cache)
      yield dir, unpacked, cache
    ensure
      FileUtils.rm_rf(dir) if dir
    end
  end
end
