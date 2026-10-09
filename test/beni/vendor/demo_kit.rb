# frozen_string_literal: true

module Beni
  module Vendor
    # The toolchain the vendor tests stage: every field but where it
    # unpacks and the checksum it is held to.
    DEMO_KIT = {
      name: "demo-kit",
      version_label: "1.0",
      base_url: "https://example.invalid/releases",
      tarball_name: "demo-kit-1.0.tar.gz",
      top_level_dir: "demo-kit-1.0"
    }.freeze
  end
end
