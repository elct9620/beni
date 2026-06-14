# frozen_string_literal: true

require "test_helper"
require "beni/vendor"

module Beni
  class TestVendor < Minitest::Test
    VENDOR_DIR = "/tmp/beni-vendor-test"

    def test_factories_anchor_on_vendor_dir
      toolchains = [Vendor.mruby(vendor_dir: VENDOR_DIR), Vendor.wasi_sdk(vendor_dir: VENDOR_DIR)]

      assert(toolchains.all? { |t| t.final_dir.start_with?(VENDOR_DIR) })
    end

    def test_built_in_pairs_cover_every_known_toolchain
      assert_equal Vendor::TOOLCHAIN_FACTORIES.keys.sort, Vendor::BUILT_IN_PAIRS.keys.sort
    end

    def test_wasi_sdk_built_in_checksums_cover_every_build_platform
      checksums = Vendor::BUILT_IN_PAIRS.fetch("wasi-sdk").fetch(:sha256)

      assert_equal %w[arm64-linux arm64-macos x86_64-linux x86_64-macos], checksums.keys.sort
      assert_includes checksums.keys, Vendor::WASI_SDK_PLATFORM
    end

    def test_wasi_sdk_platform_maps_each_host_triple_to_its_token
      # Both arm64 and aarch64 spellings resolve on each OS.
      assert_equal "arm64-macos", Vendor.wasi_sdk_platform("arm64-darwin23")
      assert_equal "arm64-macos", Vendor.wasi_sdk_platform("aarch64-darwin23")
      assert_equal "x86_64-macos", Vendor.wasi_sdk_platform("x86_64-darwin22")
      assert_equal "arm64-linux", Vendor.wasi_sdk_platform("aarch64-linux-gnu")
      assert_equal "arm64-linux", Vendor.wasi_sdk_platform("arm64-linux")
      assert_equal "x86_64-linux", Vendor.wasi_sdk_platform("x86_64-linux-gnu")
      # Unrecognised triples fall back to the most common host.
      assert_equal "x86_64-linux", Vendor.wasi_sdk_platform("riscv64-linux")
    end

    def test_dependencies_registry_implies_mruby_for_wasi_sdk
      assert_equal %w[mruby], Vendor::DEPENDENCIES.fetch("wasi-sdk")
    end

    def test_mruby_defaults_to_its_built_in_pair
      toolchain = Vendor.mruby(vendor_dir: VENDOR_DIR)
      pair = Vendor::BUILT_IN_PAIRS.fetch("mruby")

      assert_equal "#{pair.fetch(:version)}.tar.gz", toolchain.tarball_name
      assert_equal "mruby-#{pair.fetch(:version)}", toolchain.top_level_dir
      assert_equal pair.fetch(:sha256), toolchain.expected_sha256
    end

    def test_mruby_at_a_non_default_version_falls_to_tofu_pinning
      toolchain = Vendor.mruby(vendor_dir: VENDOR_DIR, version: "4.0.1")

      assert_equal "4.0.1.tar.gz", toolchain.tarball_name
      assert_equal "mruby-4.0.1", toolchain.top_level_dir
      assert_nil toolchain.expected_sha256
    end

    def test_wasi_sdk_defaults_to_the_build_platform_entry_of_its_built_in_pair
      toolchain = Vendor.wasi_sdk(vendor_dir: VENDOR_DIR)
      pair = Vendor::BUILT_IN_PAIRS.fetch("wasi-sdk")

      assert_includes toolchain.tarball_name, pair.fetch(:version)
      assert_includes toolchain.tarball_name, Vendor::WASI_SDK_PLATFORM
      assert_equal pair.fetch(:sha256).fetch(Vendor::WASI_SDK_PLATFORM), toolchain.expected_sha256
    end

    def test_wasi_sdk_override_replaces_version_and_checksum_together
      toolchain = Vendor.wasi_sdk(vendor_dir: VENDOR_DIR, version: "30.0", sha256: "cafe")

      assert_equal "wasi-sdk-30.0-#{Vendor::WASI_SDK_PLATFORM}.tar.gz", toolchain.tarball_name
      assert_includes toolchain.url, "/wasi-sdk-30/"
      assert_equal "cafe", toolchain.expected_sha256
    end

    def test_base_url_for_strips_trailing_slash_from_override
      ENV["BENI_VENDOR_BASE_URL"] = "http://127.0.0.1:8080/fixtures/"

      assert_equal "http://127.0.0.1:8080/fixtures", Vendor.base_url_for("https://example.invalid")
    ensure
      ENV.delete("BENI_VENDOR_BASE_URL")
    end

    def test_stage_wasi_toolchain_file_writes_the_gem_shipped_definition
      Dir.mktmpdir("beni-wasi-toolchain") do |dir|
        staged = Vendor.stage_wasi_toolchain_file(vendor_dir: dir)

        assert_equal File.join(dir, "mruby", "tasks", "toolchains", "wasi.rake"), staged
        assert_includes File.read(staged), "MRuby::Toolchain.new(:wasi)"
      end
    end

    def test_stage_wasi_toolchain_file_replaces_a_stale_copy
      Dir.mktmpdir("beni-wasi-toolchain") do |dir|
        target = File.join(dir, "mruby", "tasks", "toolchains", "wasi.rake")
        FileUtils.mkdir_p(File.dirname(target))
        File.write(target, "# stale\n")

        Vendor.stage_wasi_toolchain_file(vendor_dir: dir)

        refute_equal "# stale\n", File.read(target)
      end
    end
  end
end
