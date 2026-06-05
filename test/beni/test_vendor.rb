# frozen_string_literal: true

require "test_helper"
require "beni/vendor"

module Beni
  class TestVendor < Minitest::Test
    VENDOR_DIR = "/tmp/beni-vendor-test"

    def test_toolchains_returns_every_known_toolchain_anchored_on_vendor_dir
      toolchains = Vendor.toolchains(vendor_dir: VENDOR_DIR)

      assert_equal %w[mruby wasi-sdk], toolchains.map(&:name)
      assert(toolchains.all? { |t| t.final_dir.start_with?(VENDOR_DIR) })
    end

    def test_toolchains_selects_by_name_in_the_given_order
      toolchains = Vendor.toolchains(vendor_dir: VENDOR_DIR, names: %w[wasi-sdk])

      assert_equal %w[wasi-sdk], toolchains.map(&:name)
    end

    def test_toolchains_rejects_unknown_names
      error = assert_raises(Beni::Error) do
        Vendor.toolchains(vendor_dir: VENDOR_DIR, names: %w[msvc])
      end

      assert_match(/msvc/, error.message)
      assert_match(/mruby/, error.message)
    end

    def test_wasi_sdk_pins_version_and_platform
      toolchain = Vendor.wasi_sdk(vendor_dir: VENDOR_DIR)

      assert_includes toolchain.version_label, Vendor::WASI_SDK_FULL_VERSION
      assert_includes toolchain.tarball_name, Vendor::WASI_SDK_PLATFORM
    end

    def test_mruby_uses_pinned_release_tarball
      toolchain = Vendor.mruby(vendor_dir: VENDOR_DIR)

      assert_equal "#{Vendor::MRUBY_VERSION}.tar.gz", toolchain.tarball_name
      assert_equal "mruby-#{Vendor::MRUBY_VERSION}", toolchain.top_level_dir
    end

    def test_expected_sha256_reads_artifact_env_var
      ENV["BENI_VENDOR_DEMO_SHA256"] = "abc123"

      assert_equal "abc123", Vendor.expected_sha256("DEMO")
      assert_equal "", Vendor.expected_sha256("MISSING")
    ensure
      ENV.delete("BENI_VENDOR_DEMO_SHA256")
    end

    def test_base_url_for_strips_trailing_slash_from_override
      ENV["BENI_VENDOR_BASE_URL"] = "http://127.0.0.1:8080/fixtures/"

      assert_equal "http://127.0.0.1:8080/fixtures", Vendor.base_url_for("https://example.invalid")
    ensure
      ENV.delete("BENI_VENDOR_BASE_URL")
    end
  end
end
