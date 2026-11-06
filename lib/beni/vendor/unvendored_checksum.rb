# frozen_string_literal: true

module Beni
  module Vendor
    # The checksum a platform-keyed built-in pair lacks for the build
    # platform. It stands in for the checksum until setup, which refuses
    # it before anything downloads, so the tasks still load there.
    class UnvendoredChecksum < Data.define(:name, :version, :platform)
      def message
        "[beni] #{name} #{version} vendors no tarball for build platform #{platform}; " \
          "declare the toolchain with a version and sha256 for this platform"
      end
    end
  end
end
