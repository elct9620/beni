# frozen_string_literal: true

require "test_helper"
require "rake"
require "tmpdir"
require "beni/tasks"

module Beni
  # The cleanup tasks (beni:clean / beni:vendor:clean / clobber) invoked
  # against a real disposable vendor tree.
  class TestTasksCleanup < Minitest::Test
    def setup
      @original_application = Rake.application
      Rake.application = Rake::Application.new
    end

    def teardown
      Rake.application = @original_application
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

    private

    # Builds a disposable vendor tree (one unpacked toolchain dir plus
    # the tarball cache) and defines the beni:* tasks against it, so
    # the cleanup tasks can be invoked for real.
    def with_vendor_fixture
      dir = Dir.mktmpdir("beni-tasks")
      Tasks.new { vendor_dir dir }
      unpacked = File.join(dir, "mruby")
      cache = File.join(dir, ".cache")
      FileUtils.mkdir_p(unpacked)
      FileUtils.mkdir_p(cache)
      yield dir, unpacked, cache
    ensure
      FileUtils.rm_rf(dir) if dir
    end
  end
end
