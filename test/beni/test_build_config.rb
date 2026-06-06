# frozen_string_literal: true

require "test_helper"
require "rake"
require "tmpdir"
require "beni/tasks"

module Beni
  class TestBuildConfig < Minitest::Test
    def test_generate_copies_the_template_to_the_destination
      Dir.mktmpdir("beni-build-config") do |dir|
        dest = File.join(dir, "build_config", "mruby.rb")

        assert_equal dest, BuildConfig.generate(dest)
        assert FileUtils.identical?(BuildConfig::TEMPLATE, dest),
               "expected the generated config to be a verbatim template copy"
      end
    end

    def test_generate_refuses_to_overwrite_an_existing_config
      Dir.mktmpdir("beni-build-config") do |dir|
        dest = File.join(dir, "mruby.rb")
        File.write(dest, "# hand-tuned")

        error = assert_raises(Beni::Error) { BuildConfig.generate(dest) }

        assert_match(/exists/, error.message)
        assert_equal "# hand-tuned", File.read(dest)
      end
    end

    # Dogfooding doubles as the template's acceptance test: the repo's
    # own validation config must stay regenerable via `rake beni:config`.
    def test_the_repos_validation_config_is_the_unmodified_template_output
      repo_config = File.expand_path("../../build_config/mruby.rb", __dir__)

      assert FileUtils.identical?(BuildConfig::TEMPLATE, repo_config),
             "build_config/mruby.rb drifted from templates/build_config.rb — " \
             "regenerate it with rake beni:config or fold the change into the template"
    end
  end

  # The rake-task plumbing around the generator (argument handling and
  # the Beni::Tasks wiring) — the generator behavior itself is covered
  # above.
  class TestBuildConfigTask < Minitest::Test
    def setup
      @original_application = Rake.application
      Rake.application = Rake::Application.new
    end

    def teardown
      Rake.application = @original_application
    end

    def test_config_task_generates_a_build_config_at_the_given_path
      Dir.mktmpdir("beni-tasks-config") do |dir|
        Tasks.new { vendor_dir dir }
        dest = File.join(dir, "mruby.rb")

        capture_io { Rake::Task["beni:config"].invoke(dest) }

        assert FileUtils.identical?(BuildConfig::TEMPLATE, dest)
      end
    end
  end
end
