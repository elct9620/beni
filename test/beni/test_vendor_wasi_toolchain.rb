# frozen_string_literal: true

require "test_helper"
require "tmpdir"
require "beni/vendor"

module Beni
  # Staging the wasi toolchain file: beni's own where the mruby tree
  # holds none it did not write, the release's own kept.
  class TestVendorWasiToolchain < Minitest::Test
    def test_stage_wasi_toolchain_file_writes_the_gem_shipped_definition
      Dir.mktmpdir("beni-wasi-toolchain") do |dir|
        staged = Vendor.stage_wasi_toolchain_file(vendor_dir: dir)

        assert_equal File.join(dir, "mruby", "tasks", "toolchains", "wasi.rake"), staged
        assert_includes File.read(staged), "MRuby::Toolchain.new(:wasi)"
      end
    end

    def test_stage_wasi_toolchain_file_keeps_one_the_mruby_release_ships
      Dir.mktmpdir("beni-wasi-toolchain") do |dir|
        target = File.join(dir, "mruby", "tasks", "toolchains", "wasi.rake")
        FileUtils.mkdir_p(File.dirname(target))
        File.write(target, "MRuby::Toolchain.new(:wasi) { |conf, params| }\n")

        Vendor.stage_wasi_toolchain_file(vendor_dir: dir)

        assert_equal "MRuby::Toolchain.new(:wasi) { |conf, params| }\n", File.read(target)
      end
    end

    def test_stage_wasi_toolchain_file_replaces_a_stale_copy
      Dir.mktmpdir("beni-wasi-toolchain") do |dir|
        target = File.join(dir, "mruby", "tasks", "toolchains", "wasi.rake")
        FileUtils.mkdir_p(File.dirname(target))
        File.write(target, "# stale, staged by +beni:vendor:setup+\n")

        Vendor.stage_wasi_toolchain_file(vendor_dir: dir)

        assert_includes File.read(target), "params[:target]"
      end
    end
  end
end
