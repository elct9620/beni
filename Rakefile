# frozen_string_literal: true

require "bundler/gem_tasks"
require "minitest/test_task"

Minitest::TestTask.create

require "rubocop/rake_task"

RuboCop::RakeTask.new

require "steep/rake_task"

Steep::RakeTask.new

# Dogfooding: the repo builds its own vendored mruby through the gem's
# task library, exactly like a consumer with a custom build config
# would. build_config/mruby.rb is the unmodified `rake beni:config`
# template output (test_build_config.rb pins the identity) and serves
# as the repo's validation harness — host + wasm32-wasip1 with the ABI
# defines the beni crates' verification mirrors. The gem's default
# stays mruby's untouched upstream build_config/default.rb.
require "beni/tasks"

Beni::Tasks.new do |tasks|
  tasks.build_config = File.expand_path("build_config/mruby.rb", __dir__)
  tasks.targets = %w[host wasi]
  tasks.toolchains = %w[mruby wasi-sdk]
end

Dir.glob(File.join(__dir__, "tasks", "*.rake")).each { |f| load f }

task default: %i[test rubocop steep]
