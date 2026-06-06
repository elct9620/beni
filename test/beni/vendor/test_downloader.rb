# frozen_string_literal: true

require "test_helper"
require "tmpdir"
require "stringio"
require "beni/vendor/downloader"

module Beni
  module Vendor
    class TestDownloader < Minitest::Test
      # Extract-and-override seam: pretends the network responded
      # without opening a connection, scripting one outcome per attempt
      # so the retry policy and the .part/rename atomicity can be
      # tested in isolation. The backoff sleep is dropped to keep the
      # suite fast.
      class ScriptedDownloader < Downloader
        attr_reader :attempts

        def initialize(url, dest, outcomes)
          super(url, dest)
          @outcomes = outcomes
          @attempts = 0
        end

        private

        def fetch(tmp)
          outcome = @outcomes.fetch(@attempts)
          @attempts += 1
          raise outcome if outcome.is_a?(Exception)

          File.write(tmp, outcome)
        end

        def warn_and_sleep(_error, _attempt); end
      end

      def setup
        @dir = Dir.mktmpdir("beni-downloader")
        @dest = File.join(@dir, "cache", "demo-kit-1.0.tar.gz")
      end

      def teardown
        FileUtils.remove_entry(@dir)
      end

      def test_download_writes_dest_and_leaves_no_part_sidecar
        downloader(["tarball-bytes"]).download

        assert_equal "tarball-bytes", File.read(@dest)
        refute_path_exists "#{@dest}.part"
      end

      def test_download_raises_immediately_on_a_permanent_http_error
        scripted = downloader([http_error("404 Not Found")] * 2)

        assert_raises(OpenURI::HTTPError) { scripted.download }

        assert_equal 1, scripted.attempts
        refute_path_exists @dest
      end

      def test_download_never_creates_dest_when_retries_are_exhausted
        scripted = downloader([http_error("502 Bad Gateway")] * (Downloader::MAX_RETRIES + 1))

        assert_raises(OpenURI::HTTPError) { scripted.download }

        assert_equal Downloader::MAX_RETRIES + 1, scripted.attempts
        refute_path_exists @dest
      end

      def test_download_recovers_once_a_transient_error_clears
        scripted = downloader([Net::ReadTimeout.new, http_error("503 Service Unavailable"), "late-bytes"])

        scripted.download

        assert_equal "late-bytes", File.read(@dest)
        assert_equal 3, scripted.attempts
      end

      private

      def downloader(outcomes)
        ScriptedDownloader.new("https://example.invalid/demo-kit-1.0.tar.gz", @dest, outcomes)
      end

      def http_error(status_line)
        OpenURI::HTTPError.new(status_line, StringIO.new)
      end
    end
  end
end
