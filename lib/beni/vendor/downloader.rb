# frozen_string_literal: true

require "fileutils"
require "open-uri"
require "net/http" # eager — TRANSIENT_ERRORS names Net::* at class-eval

module Beni
  module Vendor
    # Tarball download with exponential-backoff retry on transient network
    # failures. One instance per +(url, dest)+ pair; reuse is not supported
    # and not needed by +Beni::Tasks+.
    #
    # Public contract is the single +#download+ entry point; +TRANSIENT_ERRORS+
    # and +MAX_RETRIES+ are exposed as tunable knobs but the retry mechanics
    # themselves are internal.
    class Downloader
      # Retry attempts wait +1 << attempt+ seconds (2 + 4 + 8 = 14s total)
      # — enough to ride out a GitHub archive 502 / TCP read timeout.
      MAX_RETRIES = 3

      # Transient network errors retried by the internal +with_retry+ wrapper.
      # +OpenURI::HTTPError+ is narrowed to 5xx; 4xx (URL typo, deleted repo)
      # bypasses the retry path.
      TRANSIENT_ERRORS = [
        OpenURI::HTTPError, Net::ReadTimeout, Net::OpenTimeout,
        Errno::ECONNRESET, SocketError
      ].freeze

      def initialize(url, dest)
        @url = url
        @dest = dest
      end

      # Fetch +url+ into +dest+ atomically via a +.part+ sidecar, retrying
      # transient failures with exponential backoff. Permanent failures
      # (4xx, DNS resolution failure on non-network condition) surface
      # immediately. Raises whatever the underlying +URI#open+ raises after
      # the retry budget is exhausted.
      def download
        FileUtils.mkdir_p(File.dirname(@dest))
        tmp = "#{@dest}.part"
        with_retry { fetch(tmp) }
        File.rename(tmp, @dest)
      end

      private

      # Stream the URL body into +tmp+ — the seam between the retry
      # policy and the network; tests override this to script outcomes
      # per attempt.
      def fetch(tmp)
        URI.parse(@url).open("rb") { |io| File.open(tmp, "wb") { |f| IO.copy_stream(io, f) } }
      end

      def with_retry
        attempts = 0
        begin
          yield
        rescue *TRANSIENT_ERRORS => e
          raise if permanent?(e) || (attempts += 1) > MAX_RETRIES

          warn_and_sleep(e, attempts)
          retry
        end
      end

      def permanent?(error)
        error.is_a?(OpenURI::HTTPError) && !error.message.match?(/\A5\d\d\b/)
      end

      def warn_and_sleep(error, attempt)
        warn "[beni] retry #{attempt}/#{MAX_RETRIES} after #{error.class}: " \
             "#{error.message.lines.first&.strip}"
        sleep(1 << attempt)
      end
    end
  end
end
