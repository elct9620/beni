# frozen_string_literal: true

require "test_helper"
require "rake"
require "tmpdir"
require "beni/tasks"

module Beni
  class TestBuildConfig < Minitest::Test
    VERSION = "4.0.0"

    def test_generate_copies_the_staged_default_config_verbatim
      with_staged_source do |dir, mruby_dir|
        dest = File.join(dir, "build_config", "mruby.rb")

        assert_equal dest, BuildConfig.generate(dest, mruby_dir: mruby_dir, version: VERSION)
        assert FileUtils.identical?(File.join(mruby_dir, "build_config", "default.rb"), dest),
               "expected the generated config to be a verbatim copy of the staged default config"
      end
    end

    def test_generate_refuses_to_overwrite_an_existing_config
      with_staged_source do |dir, mruby_dir|
        dest = File.join(dir, "mruby.rb")
        File.write(dest, "# hand-tuned")

        error = assert_raises(Beni::Error) { BuildConfig.generate(dest, mruby_dir: mruby_dir, version: VERSION) }

        assert_match(/exists/, error.message)
        assert_equal "# hand-tuned", File.read(dest)
      end
    end

    def test_generate_fails_naming_the_missing_source_when_mruby_is_not_staged
      Dir.mktmpdir("beni-build-config") do |dir|
        mruby_dir = File.join(dir, "vendor", "mruby")
        dest = File.join(dir, "mruby.rb")

        error = assert_raises(Beni::Error) { BuildConfig.generate(dest, mruby_dir: mruby_dir, version: VERSION) }

        assert_match(mruby_dir, error.message)
        refute_path_exists dest
      end
    end

    def test_generate_fails_when_the_staged_source_is_at_another_version
      with_staged_source(staged_version: "3.9.0") do |dir, mruby_dir|
        dest = File.join(dir, "mruby.rb")

        error = assert_raises(Beni::Error) { BuildConfig.generate(dest, mruby_dir: mruby_dir, version: VERSION) }

        assert_match(mruby_dir, error.message)
        assert_match(VERSION, error.message)
        refute_path_exists dest
      end
    end

    private

    # Lay out a fake staged mruby source: the upstream default config
    # plus the +.beni-version+ marker +Vendor::Tarball#prepare+ stamps.
    def with_staged_source(staged_version: VERSION)
      Dir.mktmpdir("beni-build-config") do |dir|
        mruby_dir = File.join(dir, "vendor", "mruby")
        FileUtils.mkdir_p(File.join(mruby_dir, "build_config"))
        File.write(File.join(mruby_dir, "build_config", "default.rb"), "# upstream default config\n")
        File.write(File.join(mruby_dir, Vendor::Tarball::VERSION_MARKER), "#{staged_version}\n")
        yield dir, mruby_dir
      end
    end
  end

  # The rake-task plumbing around the generator (the build_config
  # declaration wiring) — the generator behavior itself is covered above.
  class TestBuildConfigTask < Minitest::Test
    def setup
      @original_application = Rake.application
      Rake.application = Rake::Application.new
    end

    def teardown
      Rake.application = @original_application
    end

    def test_config_task_generates_at_the_declared_build_config_path
      Dir.mktmpdir("beni-tasks-config") do |dir|
        stage_mruby_source(dir)
        dest = File.join(dir, "build_config", "mruby.rb")
        Tasks.new do
          vendor_dir dir
          build_config dest
        end

        capture_io { Rake::Task["beni:config"].invoke }

        assert FileUtils.identical?(File.join(dir, "mruby", "build_config", "default.rb"), dest)
      end
    end

    def test_config_task_fails_without_a_build_config_declaration
      Dir.mktmpdir("beni-tasks-config") do |dir|
        stage_mruby_source(dir)
        Tasks.new { vendor_dir dir }

        error = assert_raises(Beni::Error) { Rake::Task["beni:config"].invoke }

        assert_match(/build_config/, error.message)
      end
    end

    private

    # The marker must carry the DSL's default mruby version — the task
    # resolves it from the same `Vendor::BUILT_IN_PAIRS` entry.
    def stage_mruby_source(vendor_dir)
      mruby_dir = File.join(vendor_dir, "mruby")
      default_version = Vendor::BUILT_IN_PAIRS.fetch("mruby").fetch(:version)
      FileUtils.mkdir_p(File.join(mruby_dir, "build_config"))
      File.write(File.join(mruby_dir, "build_config", "default.rb"), "# upstream default config\n")
      File.write(File.join(mruby_dir, Vendor::Tarball::VERSION_MARKER), "#{default_version}\n")
    end
  end
end
