# frozen_string_literal: true

require "bundler/gem_tasks"
require "minitest/test_task"

Minitest::TestTask.create

require "rubocop/rake_task"

RuboCop::RakeTask.new

require "steep/rake_task"

Steep::RakeTask.new

Dir.glob(File.join(__dir__, "tasks", "*.rake")).each { |f| load f }

task default: %i[test rubocop steep]
