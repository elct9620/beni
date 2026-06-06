# frozen_string_literal: true

require "test_helper"
require "tmpdir"
require "beni/vendor"

module Beni
  module Vendor
    # Pins +Toolchain#fetch+'s download-then-verify ordering: a freshly
    # downloaded tarball is checked before anything can unpack it.
    class TestToolchainFetch < Minitest::Test
      # Stands in for the network at the downloader seam: "downloads"
      # by writing fixed bytes into the cache.
      class FixedBytesDownloader
        def initialize(dest)
          @dest = dest
        end

        def download
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
        assert_path_exists toolchain.tarball_path
      end

      private

      def build_toolchain(expected_sha256:)
        ScriptedToolchain.new(
          name: "demo-kit",
          version_label: "1.0",
          base_url: "https://example.invalid/releases",
          tarball_name: "demo-kit-1.0.tar.gz",
          top_level_dir: "demo-kit-1.0",
          vendor_dir: @vendor_dir,
          expected_sha256: expected_sha256
        )
      end
    end
  end
end
