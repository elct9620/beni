# frozen_string_literal: true

require "test_helper"
require "tmpdir"
require "beni/vendor"

module Beni
  module Vendor
    class TestToolchain < Minitest::Test
      def setup
        @dir = Dir.mktmpdir("beni-toolchain")
        @vendor_dir = File.join(@dir, "vendor")
        @toolchain = build_toolchain(expected_sha256: nil)
      end

      def teardown
        FileUtils.remove_entry(@dir)
      end

      def test_task_name_maps_dashes_to_underscores
        assert_equal :demo_kit, @toolchain.task_name
      end

      def test_url_joins_base_url_and_tarball_name
        assert_equal "https://example.invalid/releases/demo-kit-1.0.tar.gz", @toolchain.url
      end

      def test_url_honors_base_url_override
        with_env("BENI_VENDOR_BASE_URL" => "http://127.0.0.1:8080/fixtures/") do
          assert_equal "http://127.0.0.1:8080/fixtures/demo-kit-1.0.tar.gz", @toolchain.url
        end
      end

      def test_final_dir_and_tarball_path_are_anchored_on_vendor_dir
        assert_equal File.join(@vendor_dir, "demo-kit"), @toolchain.final_dir
        assert_equal File.join(@vendor_dir, ".cache", "demo-kit-1.0.tar.gz"), @toolchain.tarball_path
      end

      def test_install_verifies_and_unpacks_cached_tarball
        put_fixture_tarball_in_cache

        capture_io { @toolchain.install }

        assert_equal "hello", File.read(File.join(@toolchain.final_dir, "README"))
        assert_path_exists "#{@toolchain.tarball_path}.sha256"
      end

      def test_install_accepts_a_tarball_matching_expected_sha256
        put_fixture_tarball_in_cache
        toolchain = build_toolchain(expected_sha256: Digest::SHA256.file(@toolchain.tarball_path).hexdigest)

        capture_io { toolchain.install }

        assert_equal "hello", File.read(File.join(toolchain.final_dir, "README"))
      end

      def test_install_aborts_on_expected_sha256_mismatch_without_unpacking
        put_fixture_tarball_in_cache
        toolchain = build_toolchain(expected_sha256: "0" * 64)

        error = assert_raises(Beni::Error) { toolchain.install }

        assert_match(/checksum mismatch/, error.message)
        refute_path_exists toolchain.final_dir
      end

      private

      def build_toolchain(expected_sha256:)
        Toolchain.new(
          name: "demo-kit",
          version_label: "1.0",
          base_url: "https://example.invalid/releases",
          tarball_name: "demo-kit-1.0.tar.gz",
          top_level_dir: "demo-kit-1.0",
          vendor_dir: @vendor_dir,
          expected_sha256: expected_sha256
        )
      end

      def with_env(overrides)
        saved = overrides.keys.to_h { |key| [key, ENV.fetch(key, nil)] }
        overrides.each { |key, value| ENV[key] = value }
        yield
      ensure
        saved.each { |key, value| ENV[key] = value }
      end

      def put_fixture_tarball_in_cache
        src = File.join(@dir, "src", "demo-kit-1.0")
        FileUtils.mkdir_p(src)
        File.write(File.join(src, "README"), "hello")
        FileUtils.mkdir_p(File.dirname(@toolchain.tarball_path))
        system("tar", "-czf", @toolchain.tarball_path, "-C", File.join(@dir, "src"), "demo-kit-1.0",
               exception: true)
      end
    end
  end
end
