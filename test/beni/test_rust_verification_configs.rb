# frozen_string_literal: true

require "test_helper"
require "rake"

require_relative "../../tasks/support/beni_rust"

# rust:clippy reads what other tasks staged and, for a config not staged
# yet, names the task to run. That name has to be a task that exists.
class TestRustVerificationConfigs < Minitest::Test
  TASKS_DIR = File.expand_path("../../tasks", __dir__)

  def test_each_scratch_config_names_a_task_that_stages_it
    with_repo_tasks do |app|
      BeniRust::VerificationConfig::ALL.reject(&:vendored?).each do |config|
        assert app.lookup(config.staging_task), "#{config.name} names #{config.staging_task}"
      end
    end
  end

  def test_the_vendored_config_is_staged_by_beni_build
    assert_equal "beni:build", BeniRust::VerificationConfig::MRUBY.staging_task
  end

  private

  def with_repo_tasks
    saved = Rake.application
    Rake.application = Rake::Application.new
    load File.join(TASKS_DIR, "rust.rake")
    yield Rake.application
  ensure
    Rake.application = saved
  end
end
