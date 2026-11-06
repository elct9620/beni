# frozen_string_literal: true

require "test_helper"
require "tmpdir"
require "beni/vendor"
require_relative "demo_kit"

module Beni
  module Vendor
    # Pins +Toolchain#fetch+'s download-then-verify ordering: a freshly
    # downloaded tarball is checked before anything can unpack it.
    class TestToolchainFetch < Minitest::Test
      # Stands in for the network at the downloader seam: "downloads"
      # by writing fixed bytes into the cache.
      class FixedBytesDownloader
        # The destinations downloaded into, across instances.
        def self.downloads
          @downloads ||= []
        end

        def initialize(dest)
          @dest = dest
        end

        def download
          self.class.downloads << @dest
          FileUtils.mkdir_p(File.dirname(@dest))
          File.write(@dest, "tarball-bytes")
        end
      end

      class ScriptedToolchain < Toolchain
        private

        def downloader
          FixedBytesDownloader.new(tarball_path)
        end
      end

      def setup
        @dir = Dir.mktmpdir("beni-toolchain-fetch")
        @vendor_dir = File.join(@dir, "vendor")
        FixedBytesDownloader.downloads.clear
      end

      def teardown
        FileUtils.remove_entry(@dir)
      end

      def test_fetch_downloads_and_pins_the_tarball
        toolchain = build_toolchain(expected_sha256: nil)

        capture_io { toolchain.fetch }

        assert_equal "tarball-bytes", File.read(toolchain.tarball_path)
        assert_path_exists "#{toolchain.tarball_path}.sha256"
      end

      def test_fetch_aborts_when_the_download_fails_verification
        toolchain = build_toolchain(expected_sha256: "0" * 64)

        error = assert_raises(Beni::Error) { capture_io { toolchain.fetch } }

        assert_match(/checksum mismatch/, error.message)
        refute_path_exists toolchain.final_dir
        refute_path_exists toolchain.tarball_path
      end

      def test_install_drops_a_cached_tarball_that_fails_verification
        toolchain = build_toolchain(expected_sha256: "0" * 64)
        FileUtils.mkdir_p(File.dirname(toolchain.tarball_path))
        File.write(toolchain.tarball_path, "stale-bytes")

        assert_raises(Beni::Error) { capture_io { toolchain.install } }

        refute_path_exists toolchain.final_dir
        refute_path_exists toolchain.tarball_path
      end

      def test_fetch_refuses_a_missing_platform_checksum_before_downloading
        missing = UnvendoredChecksum.new(name: "demo-kit", version: "1.0", platform: "x86_64-windows")
        toolchain = build_toolchain(expected_sha256: missing)

        error = assert_raises(Beni::Error) { capture_io { toolchain.fetch } }

        assert_includes error.message, "demo-kit"
        assert_includes error.message, "x86_64-windows"
        assert_empty FixedBytesDownloader.downloads
      end

      private

      def build_toolchain(expected_sha256:)
        ScriptedToolchain.new(**DEMO_KIT, vendor_dir: @vendor_dir, expected_sha256:)
      end
    end
  end
end
