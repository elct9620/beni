# frozen_string_literal: true

require "test_helper"
require "rake"

# A leg that builds mruby from the staged source needs the whole vendor
# tree beni:vendor:setup stages, the prism tree and the wasi toolchain
# file included, not the mruby source alone.
class TestMrubyBuildingLegs < Minitest::Test
  TASKS_DIR = File.expand_path("../../tasks", __dir__)
  LEGS = %w[rust:test:default rust:test:float32 docs:bindings docs:bindings:release].freeze

  def test_each_mruby_building_leg_runs_after_the_whole_vendor_setup
    with_repo_tasks do |app|
      LEGS.each do |leg|
        assert_includes app[leg].prerequisites, "beni:vendor:setup", leg
      end
    end
  end

  private

  def with_repo_tasks
    saved = Rake.application
    Rake.application = Rake::Application.new
    %w[rust.rake docs.rake].each { |file| load File.join(TASKS_DIR, file) }
    yield Rake.application
  ensure
    Rake.application = saved
  end
end
