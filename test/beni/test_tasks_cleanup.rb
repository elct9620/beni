# frozen_string_literal: true

require "test_helper"
require "rake"
require "tmpdir"
require "beni/tasks"
require_relative "fresh_rake_application"

module Beni
  # The cleanup tasks (beni:clean / beni:vendor:clean / clobber) invoked
  # against a real disposable vendor tree.
  class TestTasksCleanup < Minitest::Test
    include FreshRakeApplication

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

    def test_vendor_clean_removes_a_toolchain_no_longer_selected
      with_vendor_fixture do |dir, _unpacked, _cache|
        staged_before = File.join(dir, "wasi-sdk")
        FileUtils.mkdir_p(staged_before)

        Rake::Task["beni:vendor:clean"].invoke

        refute_path_exists staged_before
      end
    end

    def test_clean_removes_undeclared_build_trees_and_keeps_cloned_gems
      with_vendor_fixture do |dir, _unpacked, _cache|
        build = File.join(dir, "mruby", "build")
        undeclared = File.join(build, "wasi")
        cloned = File.join(build, "repos", "host")
        FileUtils.mkdir_p([undeclared, cloned])

        capture_io { Rake::Task["beni:clean"].invoke }

        refute_path_exists undeclared
        assert_path_exists cloned
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
