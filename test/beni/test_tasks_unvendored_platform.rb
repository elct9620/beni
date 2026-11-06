# frozen_string_literal: true

require "test_helper"
require "rake"
require "tmpdir"
require "beni/tasks"
require_relative "fresh_rake_application"

module Beni
  # The tasks on a build platform a selected toolchain's built-in pair
  # vendors no tarball for: they load, and only setting that toolchain
  # up is refused, before anything downloads.
  class TestTasksUnvendoredPlatform < Minitest::Test
    include FreshRakeApplication

    UNVENDORED_PLATFORM = "riscv64-linux"

    def test_the_tasks_load_and_clean_on_a_platform_without_the_checksum
      on_unvendored_platform do |dir|
        build_tree = File.join(dir, "mruby", "build", "wasi")
        FileUtils.mkdir_p(build_tree)

        capture_io { Rake::Task["beni:clean"].invoke }

        refute_path_exists build_tree
      end
    end

    def test_setting_up_the_toolchain_names_it_and_the_platform_and_downloads_nothing
      on_unvendored_platform do |dir|
        error = assert_raises(Error) { capture_io { Rake::Task["beni:vendor:setup:wasi_sdk"].invoke } }

        assert_includes error.message, "wasi-sdk"
        assert_includes error.message, UNVENDORED_PLATFORM
        refute_path_exists File.join(dir, ".cache")
      end
    end

    private

    # Loads the tasks for a wasi target with the build platform swapped
    # for one the wasi-sdk pair vendors nothing for.
    def on_unvendored_platform
      dir = Dir.mktmpdir("beni-unvendored")
      with_build_platform(UNVENDORED_PLATFORM) do
        Tasks.new do
          vendor_dir dir
          target(:wasi) { toolchain "wasi-sdk" }
        end
        yield dir
      end
    ensure
      FileUtils.rm_rf(dir) if dir
    end

    def with_build_platform(platform)
      original = Vendor::BUILD_PLATFORM
      swap_build_platform(platform)
      yield
    ensure
      swap_build_platform(original) if original
    end

    def swap_build_platform(platform)
      Vendor.send(:remove_const, :BUILD_PLATFORM)
      Vendor.const_set(:BUILD_PLATFORM, platform)
    end
  end
end
