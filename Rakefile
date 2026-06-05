# frozen_string_literal: true

require "bundler/gem_tasks"
require "minitest/test_task"

Minitest::TestTask.create

require "rubocop/rake_task"

RuboCop::RakeTask.new

Dir.glob(File.join(__dir__, "tasks", "*.rake")).sort.each { |f| load f }

task default: %i[test rubocop]
