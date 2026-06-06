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
  # or with declarations (a custom config cross-building to wasm32-wasip1):
  #
  #   Beni::Tasks.new do
  #     build_config "build_config/mruby.rb"
  #
  #     target :host
  #     target :wasi do
  #       toolchain "wasi-sdk"
  #     end
  #   end
  #
  # The block is the declarative DSL from SPEC.md, run on +DSL::Context+:
  # scalar settings (+version+ / +build_config+ / +vendor_dir+), target
  # declarations carrying toolchain references, and top-level toolchain
  # definitions overriding a built-in pair. Every malformed declaration
  # raises here — no task defined, nothing downloaded.
  #
  # Defined tasks:
  #
  #   rake beni:build           — fetch toolchains + build libmruby.a per target
  #   rake beni:clean           — remove mruby build trees (keeps source)
  #   rake beni:config          — generate the upstream default build config
  #   rake beni:vendor:setup    — download & unpack the selected toolchains
  #   rake beni:vendor:clean    — remove unpacked toolchains (keeps tarball cache)
  #   rake beni:vendor:clobber  — remove the vendor tree entirely
  class Tasks < Rake::TaskLib
    # The resolved declarations — exposed so consumers and tests can
    # inspect what the task definitions were wired from.
    attr_reader :configuration

    def initialize(&block)
      super()
      context = DSL::Context.new
      context.instance_exec(&block) if block
      @configuration = context.configuration
      define
    end

    private

    def builder
      @builder ||= Builder.new(
        vendor_dir: configuration.vendor_dir,
        build_config: configuration.build_config,
        targets: configuration.targets
      )
    end

    # The selected toolchains as Vendor pipeline values, each carrying
    # its resolved version and checksum.
    def vendor_toolchains
      @vendor_toolchains ||= configuration.toolchains.map do |selected|
        Vendor.public_send(
          Vendor::TOOLCHAIN_FACTORIES.fetch(selected.name),
          vendor_dir: configuration.vendor_dir, version: selected.version, sha256: selected.sha256
        )
      end
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
      desc "Fetch and unpack the selected vendor toolchains (#{vendor_toolchains.map(&:name).join(" + ")})"
      task setup: vendor_toolchains.map { |toolchain| "setup:#{toolchain.task_name}" }
    end

    def define_vendor_clean_tasks
      desc "Remove unpacked vendor toolchains (keeps cached tarballs)"
      task :clean do
        vendor_toolchains.each { |toolchain| FileUtils.rm_rf(toolchain.final_dir) }
      end

      desc "Remove #{configuration.vendor_dir} entirely (unpacked trees and cached tarballs)"
      task :clobber do
        FileUtils.rm_rf(configuration.vendor_dir)
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
      desc "Build vendored mruby for #{configuration.targets.join(" + ")} " \
           "(produces #{builder.libmruby_paths.join(", ")})"
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
      desc "Generate mruby's upstream default build config at the `build_config` declaration's path"
      task :config do
        dest = configuration.build_config
        raise Error, "beni:config requires a `build_config` declaration naming the file to generate" unless dest

        BuildConfig.generate(dest, mruby_dir: builder.mruby_dir, version: mruby_version)
        puts "[beni] generated #{dest} — edit it to define further targets"
      end
    end

    # mruby's selected version — always present, `mruby` is selected in
    # every resolution.
    def mruby_version
      configuration.toolchains.to_h { |toolchain| [toolchain.name, toolchain.version] }.fetch("mruby")
    end
  end
end
