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
      Tasks.new { vendor_dir VENDOR_DIR }

      %w[
        beni:build beni:clean beni:config
        beni:vendor:setup beni:vendor:setup:mruby
        beni:vendor:clean beni:vendor:clobber
      ].each do |name|
        assert Rake::Task.task_defined?(name), "expected task #{name} to be defined"
      end
    end

    def test_wasi_sdk_setup_is_absent_without_a_reference
      Tasks.new { vendor_dir VENDOR_DIR }

      refute Rake::Task.task_defined?("beni:vendor:setup:wasi_sdk"),
             "expected wasi-sdk setup task to be absent without a toolchain reference"
    end

    def test_a_toolchain_reference_defines_its_setup_task_plus_transitive_mruby
      Tasks.new do
        vendor_dir VENDOR_DIR
        target(:wasi) { toolchain "wasi-sdk" }
      end

      assert Rake::Task.task_defined?("beni:vendor:setup:wasi_sdk")
      assert Rake::Task.task_defined?("beni:vendor:setup:mruby")
    end

    def test_an_unknown_toolchain_reference_fails_before_any_task_is_defined
      error = assert_raises(Beni::Error) do
        Tasks.new do
          vendor_dir VENDOR_DIR
          target(:wasi) { toolchain "msvc" }
        end
      end

      assert_match(/msvc/, error.message)
      refute Rake::Task.task_defined?("beni:build"), "no task may exist after a definition-time failure"
    end

    def test_the_retired_assignment_block_form_fails_loudly
      assert_raises(NoMethodError) do
        Tasks.new { |tasks| tasks.vendor_dir = VENDOR_DIR }
      end
    end

    def test_build_depends_on_vendor_setup
      Tasks.new { vendor_dir VENDOR_DIR }

      prerequisites = Rake::Task["beni:build"].prerequisite_tasks.map(&:name)

      assert_includes prerequisites, "beni:vendor:setup"
    end

    def test_tarball_file_tasks_are_anchored_on_vendor_dir
      Tasks.new { vendor_dir VENDOR_DIR }

      mruby_version = Vendor::BUILT_IN_PAIRS.fetch("mruby").fetch(:version)
      mruby_tarball = File.join(VENDOR_DIR, ".cache", "#{mruby_version}.tar.gz")

      assert Rake::Task.task_defined?(mruby_tarball), "expected file task for #{mruby_tarball}"
      assert_includes Rake::Task["beni:vendor:setup:mruby"].prerequisites, mruby_tarball
    end

    def test_a_version_declaration_selects_the_mruby_tarball
      Tasks.new do
        vendor_dir VENDOR_DIR
        version "4.0.1"
      end

      tarball = File.join(VENDOR_DIR, ".cache", "4.0.1.tar.gz")

      assert Rake::Task.task_defined?(tarball), "expected file task for #{tarball}"
    end

    def test_default_build_config_is_nil_so_mruby_uses_its_own_default
      tasks = Tasks.new { vendor_dir VENDOR_DIR }

      assert_nil tasks.configuration.build_config
    end

    def test_build_config_and_targets_are_declarable
      tasks = Tasks.new do
        vendor_dir VENDOR_DIR
        build_config "/custom/config.rb"
        target :embedded
      end

      assert_equal "/custom/config.rb", tasks.configuration.build_config
      assert_equal %w[embedded], tasks.configuration.targets
    end

    # `execute` runs only the setup task's own action — the toolchain
    # prerequisites (downloads) stay out of the unit test.
    def test_vendor_setup_stages_the_wasi_toolchain_file_when_wasi_sdk_is_selected
      Dir.mktmpdir("beni-tasks-wasi") do |dir|
        Tasks.new do
          vendor_dir dir
          target :wasi do
            toolchain "wasi-sdk"
          end
        end

        Rake::Task["beni:vendor:setup"].execute

        assert_path_exists File.join(dir, "mruby", "tasks", "toolchains", "wasi.rake")
      end
    end

    def test_vendor_setup_skips_the_wasi_toolchain_file_without_wasi_sdk
      Dir.mktmpdir("beni-tasks-wasi") do |dir|
        Tasks.new { vendor_dir dir }

        Rake::Task["beni:vendor:setup"].execute

        refute_path_exists File.join(dir, "mruby", "tasks", "toolchains", "wasi.rake")
      end
    end
  end
end
