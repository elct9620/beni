# frozen_string_literal: true

require "digest"

module Beni
  module Vendor
    # SHA256 verification for vendored tarballs. One instance per +(path,
    # expected_sha)+ pair; reuse is not supported and not needed by
    # +Beni::Tasks+. Operates in two modes:
    #
    #   * Explicit expected hash (a built-in pair entry or a consumer
    #     override) — must match exactly; mismatch raises.
    #   * Trust-on-first-use (TOFU) — when +expected_sha+ is +nil+ or empty,
    #     the actual hash is pinned to a +.sha256+ sidecar next to the
    #     tarball. Subsequent runs compare against the pinned value and
    #     raise on drift.
    #
    # Public contract is the single +#verify_or_pin+ entry point; the two
    # branches and the digest helper are internal.
    class Checksum
      def initialize(path, expected_sha)
        @path = path
        @expected_sha = expected_sha
      end

      # Verify the tarball against +expected_sha+ (if non-empty) or TOFU-pin
      # against the +.sha256+ sidecar. Returns the computed SHA256 hex digest
      # on success. Raises +Beni::Error+ on mismatch (explicit mode) or drift
      # (TOFU mode); both error messages carry a +[beni]+ prefix for CI log
      # grepping.
      def verify_or_pin
        actual = sha256
        sidecar = "#{@path}.sha256"
        expected? ? verify_against_expected(actual, sidecar) : verify_or_pin_sidecar(actual, sidecar)
        actual
      end

      private

      def expected?
        !@expected_sha.to_s.empty?
      end

      def sha256
        Digest::SHA256.file(@path).hexdigest
      end

      def verify_against_expected(actual, sidecar)
        unless actual == @expected_sha
          raise Error, "[beni] checksum mismatch for #{File.basename(@path)}: " \
                       "expected #{@expected_sha}, got #{actual}"
        end
        File.write(sidecar, "#{actual}\n")
      end

      def verify_or_pin_sidecar(actual, sidecar)
        if File.exist?(sidecar)
          pinned = File.read(sidecar).strip
          return if actual == pinned

          raise Error, "[beni] checksum drift for #{File.basename(@path)}: " \
                       "pinned #{pinned}, got #{actual}"
        end
        File.write(sidecar, "#{actual}\n")
      end
    end
  end
end
