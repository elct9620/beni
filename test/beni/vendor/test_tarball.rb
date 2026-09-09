# frozen_string_literal: true

require "test_helper"
require "tmpdir"
require "beni/vendor/tarball"

module Beni
  module Vendor
    class TestTarball < Minitest::Test
      # Extract-and-override seam: unpacks for real while recording the
      # arguments tar was handed, so the argument shape can be asserted
      # on a host that cannot produce a Windows path.
      class RecordingTarball < Tarball
        attr_reader :named, :working_dir

        private

        def run_tar(tarball, staging)
          @named = tarball
          @working_dir = staging
          super
        end
      end

      def setup
        @dir = Dir.mktmpdir("beni-tarball")
        @final_dir = File.join(@dir, "toolchain")
        @tarball = make_tarball("toolchain-1.0", "README" => "hello")
      end

      def teardown
        FileUtils.remove_entry(@dir)
      end

      def test_prepare_unpacks_top_level_dir_and_stamps_version
        tarball_for("1.0").prepare

        assert_equal "hello", File.read(File.join(@final_dir, "README"))
        assert_equal "1.0\n", File.read(File.join(@final_dir, Tarball::VERSION_MARKER))
      end

      def test_prepare_is_a_noop_when_version_matches
        tarball_for("1.0").prepare
        canary = File.join(@final_dir, "local-change")
        File.write(canary, "kept")

        tarball_for("1.0").prepare

        assert_path_exists canary
      end

      def test_prepare_reextracts_cleanly_on_version_bump
        tarball_for("1.0").prepare
        stale = File.join(@final_dir, "stale-file")
        File.write(stale, "old")

        tarball_for("2.0").prepare

        refute_path_exists stale
        assert_equal "2.0\n", File.read(File.join(@final_dir, Tarball::VERSION_MARKER))
      end

      def test_prepare_on_a_corrupt_tarball_leaves_no_tree_behind
        corrupt = File.join(@dir, "corrupt.tar.gz")
        File.write(corrupt, "not a gzip stream")
        tarball = Tarball.new(
          tarball: corrupt, top_level_dir: "toolchain-1.0",
          final_dir: @final_dir, version: "1.0"
        )

        assert_raises(StandardError) { capture_subprocess_io { tarball.prepare } }

        refute_path_exists @final_dir
        refute_path_exists "#{@final_dir}.staging"
      end

      def test_the_tarball_is_named_from_the_directory_it_unpacks_into
        # A path opening with a drive letter is what GNU tar reads as the
        # host half of a `host:path` remote spec, and that is what every
        # absolute path looks like on Windows. No host here can produce
        # one, so what is pinned is the shape that never carries it.
        tarball = recording_tarball
        tarball.prepare

        refute File.absolute_path?(tarball.named),
               "an absolute path opens with a drive letter on Windows"
        assert_equal "#{@final_dir}.staging", tarball.working_dir
        assert_equal File.expand_path(@tarball),
                     File.expand_path(tarball.named, tarball.working_dir),
                     "the name still resolves to the tarball from where tar runs"
      end

      def test_prepare_raises_when_top_level_dir_is_missing
        error = assert_raises(Beni::Error) do
          Tarball.new(
            tarball: @tarball, top_level_dir: "wrong-root",
            final_dir: @final_dir, version: "1.0"
          ).prepare
        end

        assert_match(/wrong-root/, error.message)
      end

      private

      def recording_tarball
        RecordingTarball.new(
          tarball: @tarball, top_level_dir: "toolchain-1.0",
          final_dir: @final_dir, version: "1.0"
        )
      end

      def tarball_for(version)
        Tarball.new(
          tarball: @tarball, top_level_dir: "toolchain-1.0",
          final_dir: @final_dir, version: version
        )
      end

      def make_tarball(top_level_dir, files)
        src = File.join(@dir, "src", top_level_dir)
        FileUtils.mkdir_p(src)
        files.each { |name, content| File.write(File.join(src, name), content) }
        path = File.join(@dir, "#{top_level_dir}.tar.gz")
        system("tar", "-czf", path, "-C", File.join(@dir, "src"), top_level_dir, exception: true)
        path
      end
    end
  end
end
