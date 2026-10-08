# frozen_string_literal: true

require "bundler/gem_tasks"
require "minitest/test_task"

# Unit tests only — the consumer-scenario harnesses under
# test/scenarios/ vendor a full mruby tree whose own *_test.rb files
# the default glob would sweep in.
Minitest::TestTask.create do |t|
  t.test_globs = ["test/test_*.rb", "test/beni/**/test_*.rb"]
end

require "rubocop/rake_task"

RuboCop::RakeTask.new

require "steep/rake_task"

Steep::RakeTask.new

# Dogfooding: the repo builds its own vendored mruby through the gem's
# task library, exactly like a consumer with a custom build config
# would. build_config/mruby.rb is the repo's validation harness — host
# + wasm32-wasip1 with the ABI defines the beni crates' verification
# mirrors. The gem's default stays mruby's untouched upstream
# build_config/default.rb.
require "beni/tasks"

# BENI_MRUBY_VERSION selects the mruby release the chain builds; unset,
# it builds the gem's default. A release whose compiler gem parses with
# Prism also needs the ruby/prism commit its submodule records, keyed
# here by that release.
MRUBY_VERSION = ENV.fetch("BENI_MRUBY_VERSION", nil)
PRISM_SOURCES = {
  "4.1.0-rc2" => {
    commit: "c0e37816e97e23e92524a4070e1b99a4025bc63f",
    sha256: "1d90fcd65f78c361d8fb1f9a835611e3f48c1816471feed6764204f037f73383"
  }
}.freeze

Beni::Tasks.new do
  build_config "build_config/mruby.rb"
  if MRUBY_VERSION
    version MRUBY_VERSION
    if (prism = PRISM_SOURCES[MRUBY_VERSION])
      toolchain "prism" do
        version prism.fetch(:commit)
        sha256 prism.fetch(:sha256)
      end
    end
  end

  target :host
  target :wasi do
    toolchain "wasi-sdk"
  end
end

Dir.glob(File.join(__dir__, "tasks", "*.rake")).each { |f| load f }

task default: %i[test rubocop steep api:surface api:formats]
