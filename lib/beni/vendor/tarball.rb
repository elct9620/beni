# frozen_string_literal: true

require "fileutils"
require "pathname"

module Beni
  module Vendor
    # Unpacks a vendored tarball into +final_dir+, idempotent on a
    # +.beni-version+ marker stamped inside the tree. One instance per
    # +(tarball, top_level_dir, final_dir, version)+ configuration; reuse is
    # not supported and not needed by +Beni::Tasks+.
    #
    # A version mismatch (toolchain bump) forces a clean re-extract, so the
    # unpacked tree never lags the pinned version. Public contract is the
    # single +#prepare+ entry point; the staging-directory step is internal.
    class Tarball
      # Marker stamped inside +final_dir+ after a successful unpack; a matching
      # value short-circuits +#prepare+, a mismatch forces re-extract.
      VERSION_MARKER = ".beni-version"

      def initialize(tarball:, top_level_dir:, final_dir:, version:)
        @tarball = tarball
        @top_level_dir = top_level_dir
        @final_dir = final_dir
        @version = version
      end

      # Extract the tarball into a staging sibling of +final_dir+, then
      # atomically move the +top_level_dir+ subtree into place and stamp the
      # version marker. A no-op when the stamped version already matches.
      # Raises if the tarball does not contain the expected +top_level_dir+
      # root. The staging tree is removed whether or not extraction succeeds.
      def prepare
        return if installed_version == @version

        staging = "#{@final_dir}.staging"
        begin
          extract_to_staging(staging)
          promote(staging)
        ensure
          FileUtils.rm_rf(staging)
        end
      end

      private

      # Version recorded by the last successful unpack, or +nil+ when the tree
      # is absent or predates version stamping (forcing a re-extract).
      def installed_version
        marker = File.join(@final_dir, VERSION_MARKER)
        File.read(marker).strip if File.exist?(marker)
      end

      def extract_to_staging(staging)
        FileUtils.rm_rf(staging)
        FileUtils.mkdir_p(staging)
        # An absolute Windows path opens with a drive letter, which GNU
        # tar reads as the host half of a +host:path+ remote spec and
        # tries to connect to. Naming the tarball from the directory it
        # unpacks into leaves no colon for any tar to find.
        from_staging = Pathname.new(@tarball).relative_path_from(Pathname.new(staging))
        run_tar(from_staging.to_s, staging)
      end

      # Spawn tar with the staging directory as its working directory.
      # Extracted as a seam so tests can observe the argument shape
      # without unpacking anything.
      def run_tar(tarball, staging)
        system("tar", "-xzf", tarball, chdir: staging, exception: true)
      end

      # Move the expected +top_level_dir+ subtree out of +staging+ into
      # +final_dir+ and stamp the version marker.
      def promote(staging)
        src = File.join(staging, @top_level_dir)
        raise Error, "[beni] expected #{src} after extracting #{@tarball}, missing" unless File.directory?(src)

        FileUtils.rm_rf(@final_dir)
        FileUtils.mkdir_p(File.dirname(@final_dir))
        FileUtils.mv(src, @final_dir)
        File.write(File.join(@final_dir, VERSION_MARKER), "#{@version}\n")
      end
    end
  end
end
