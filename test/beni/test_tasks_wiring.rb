# frozen_string_literal: true

require "test_helper"
require "rake"
require "beni/tasks"

module Beni
  # Pins the joins between the task definitions and their collaborators:
  # the +beni:build+ action driving the builder, and the resolved
  # toolchain selections reaching the Vendor pipeline.
  class TestTasksWiring < Minitest::Test
    VENDOR_DIR = "/tmp/beni-tasks-test/vendor"

    # Records the build task's collaborator call without spawning
    # mruby's rake — the task-action seam mirroring +FakeRakeBuilder+.
    class RecordingBuilder
      attr_reader :ensure_built_calls

      def initialize
        @ensure_built_calls = 0
      end

      def libmruby_paths
        []
      end

      def ensure_built
        @ensure_built_calls += 1
      end
    end

    class TasksWithRecordingBuilder < Tasks
      def recording_builder
        @recording_builder ||= RecordingBuilder.new
      end

      private

      def builder
        recording_builder
      end
    end

    # Widens visibility so tests can observe the Vendor values the task
    # definitions were wired from.
    class InspectableTasks < Tasks
      public :vendor_toolchains
    end

    def setup
      @original_application = Rake.application
      Rake.application = Rake::Application.new
    end

    def teardown
      Rake.application = @original_application
    end

    def test_build_executes_the_builder
      tasks = TasksWithRecordingBuilder.new { vendor_dir VENDOR_DIR }

      Rake::Task["beni:build"].execute

      assert_equal 1, tasks.recording_builder.ensure_built_calls
    end

    def test_vendor_toolchains_carry_the_resolved_pair_into_the_pipeline
      tasks = overridden_wasi_sdk_tasks

      wasi = tasks.vendor_toolchains.find { |toolchain| toolchain.name == "wasi-sdk" }

      assert_equal "wasi-sdk-34.0-#{Vendor::WASI_SDK_PLATFORM}.tar.gz", wasi.tarball_name
      assert_equal "ab" * 32, wasi.expected_sha256
    end

    private

    # A consumer Rakefile overriding the built-in wasi-sdk pair.
    def overridden_wasi_sdk_tasks
      InspectableTasks.new do
        vendor_dir VENDOR_DIR
        toolchain "wasi-sdk" do
          version "34.0"
          sha256 "ab" * 32
        end
        target(:wasi) { toolchain "wasi-sdk" }
      end
    end
  end
end
