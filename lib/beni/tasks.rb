# frozen_string_literal: true

require "fileutils"
require "rake/tasklib"

require_relative "../beni"

module Beni
  # Rake task library exposing the +beni:*+ namespace. Add to a Rakefile:
  #
  #   require "beni/tasks"
  #
  #   Beni::Tasks.new
  #
  # or with overrides (a custom config cross-building to wasm32-wasip1):
  #
  #   Beni::Tasks.new do |tasks|
  #     tasks.build_config = File.expand_path("build_config/custom.rb")
  #     tasks.targets = %w[host wasi]
  #     tasks.toolchains = %w[mruby wasi-sdk]
  #   end
  #
  # Defined tasks:
  #
  #   rake beni:build           — fetch toolchains + build libmruby.a per target
  #   rake beni:clean           — remove mruby build trees (keeps source)
  #   rake beni:config[path]    — generate a customizable build config
  #   rake beni:vendor:setup    — download & unpack the configured toolchains
  #   rake beni:vendor:clean    — remove unpacked toolchains (keeps tarball cache)
  #   rake beni:vendor:clobber  — remove the vendor tree entirely
  #
  # Settings (set them in the block; they are read once when the tasks
  # are defined, so later mutation has no effect):
  #
  #   * +vendor_dir+   — where toolchains unpack and mruby builds; defaults
  #     to +vendor/+ under the Rakefile's working directory, or the
  #     +BENI_VENDOR_DIR+ env var when set (test-fixture relocation).
  #   * +build_config+ — mruby build config path; defaults to +nil+, which
  #     lets mruby use its own +build_config/default.rb+ — the upstream
  #     defaults, untouched.
  #   * +targets+      — build-target names to verify after the build,
  #     matching the +MRuby::Build.new(<name>)+ names in the config
  #     (the upstream default config names its single target +host+).
  #   * +toolchains+   — vendor toolchain names to download, from
  #     +Vendor::TOOLCHAIN_FACTORIES+; defaults to mruby alone. Add
  #     +wasi-sdk+ when the build config cross-compiles to wasm.
  class Tasks < Rake::TaskLib
    attr_accessor :vendor_dir, :build_config, :targets, :toolchains

    def initialize
      super
      @vendor_dir = ENV["BENI_VENDOR_DIR"] || File.expand_path("vendor")
      @build_config = nil
      @targets = Builder::DEFAULT_TARGETS
      @toolchains = %w[mruby]
      yield self if block_given?
      define
    end

    private

    def builder
      @builder ||= Builder.new(vendor_dir: vendor_dir, build_config: build_config, targets: targets)
    end

    def vendor_toolchains
      @vendor_toolchains ||= Vendor.toolchains(vendor_dir: vendor_dir, names: toolchains)
    end

    def define
      namespace :beni do
        define_vendor_namespace
        define_build_task
        define_clean_task
        define_config_task
      end
    end

    def define_vendor_namespace
      namespace :vendor do
        vendor_toolchains.each { |toolchain| define_toolchain_tasks(toolchain) }
        define_vendor_setup_task
        define_vendor_clean_tasks
      end
    end

    def define_vendor_setup_task
      desc "Fetch and unpack the build-time vendor toolchains (#{vendor_toolchains.map(&:name).join(" + ")})"
      task setup: vendor_toolchains.map { |toolchain| "setup:#{toolchain.task_name}" }
    end

    def define_vendor_clean_tasks
      desc "Remove unpacked vendor toolchains (keeps cached tarballs)"
      task :clean do
        vendor_toolchains.each { |toolchain| FileUtils.rm_rf(toolchain.final_dir) }
      end

      desc "Remove #{vendor_dir} entirely (unpacked trees and cached tarballs)"
      task :clobber do
        FileUtils.rm_rf(vendor_dir)
      end
    end

    def define_toolchain_tasks(toolchain)
      file toolchain.tarball_path do
        toolchain.fetch
      end

      namespace :setup do
        desc "Download and unpack #{toolchain.name} #{toolchain.version_label} into #{toolchain.final_dir}"
        task toolchain.task_name => toolchain.tarball_path do
          toolchain.install
        end
      end
    end

    def define_build_task
      desc "Build vendored mruby for #{targets.join(" + ")} (produces #{builder.libmruby_paths.join(", ")})"
      task build: "vendor:setup" do
        builder.ensure_built
      end
    end

    def define_clean_task
      desc "Remove mruby's build trees (keeps vendored mruby source)"
      task :clean do
        builder.clean
      end
    end

    def define_config_task
      desc "Generate a customizable mruby build config (default: build_config/mruby.rb)"
      task :config, [:path] do |_task, args|
        path = args[:path] || File.expand_path("build_config/mruby.rb")
        BuildConfig.generate(path)
        puts "[beni] generated #{path} — point Beni::Tasks#build_config at it"
      end
    end
  end
end
