# frozen_string_literal: true

require "test_helper"
require "tmpdir"
require "digest"
require "beni/vendor/checksum"

module Beni
  module Vendor
    class TestChecksum < Minitest::Test
      def setup
        @dir = Dir.mktmpdir("beni-checksum")
        @path = File.join(@dir, "artifact.tar.gz")
        File.write(@path, "tarball-bytes")
        @digest = Digest::SHA256.file(@path).hexdigest
      end

      def teardown
        FileUtils.remove_entry(@dir)
      end

      def test_returns_digest_when_expected_sha_matches
        actual = Checksum.new(@path, @digest).verify_or_pin

        assert_equal @digest, actual
        assert_equal "#{@digest}\n", File.read("#{@path}.sha256")
      end

      def test_raises_when_expected_sha_mismatches
        error = assert_raises(Beni::Error) do
          Checksum.new(@path, "0" * 64).verify_or_pin
        end

        assert_match(/checksum mismatch/, error.message)
      end

      def test_pins_sidecar_on_first_use_when_no_expected_sha
        Checksum.new(@path, "").verify_or_pin

        assert_equal "#{@digest}\n", File.read("#{@path}.sha256")
      end

      # A nil checksum reaches +Checksum+ whenever a toolchain resolves
      # no built-in or override hash; it must take the same TOFU path as
      # an empty string.
      def test_pins_sidecar_on_first_use_when_expected_sha_is_nil
        Checksum.new(@path, nil).verify_or_pin

        assert_equal "#{@digest}\n", File.read("#{@path}.sha256")
      end

      def test_passes_when_sidecar_matches_on_second_run
        Checksum.new(@path, "").verify_or_pin

        assert_equal @digest, Checksum.new(@path, "").verify_or_pin
      end

      def test_explicit_sha_ignores_and_rewrites_a_stale_pinned_sidecar
        File.write("#{@path}.sha256", "#{"0" * 64}\n")

        Checksum.new(@path, @digest).verify_or_pin

        assert_equal "#{@digest}\n", File.read("#{@path}.sha256")
      end

      def test_raises_on_drift_from_pinned_sidecar
        File.write("#{@path}.sha256", "#{"0" * 64}\n")

        error = assert_raises(Beni::Error) do
          Checksum.new(@path, "").verify_or_pin
        end

        assert_match(/checksum drift/, error.message)
      end
    end
  end
end
