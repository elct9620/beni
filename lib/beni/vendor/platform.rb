# frozen_string_literal: true

module Beni
  # The build platform half of the vendor façade: how the host's
  # +RUBY_PLATFORM+ names the platform toolchains key their tarballs by.
  module Vendor
    # +RUBY_PLATFORM+ spells the same architecture and operating system
    # differently across hosts; toolchains key their per-platform tarballs
    # by one spelling each.
    ARCH_ALIASES = { "x64" => "x86_64", "amd64" => "x86_64", "aarch64" => "arm64" }.freeze
    OS_ALIASES = { /darwin/ => "macos", /linux/ => "linux", /mingw|mswin/ => "windows" }.freeze

    # The build platform: the architecture and operating system the Rake
    # tasks run on, spelled the way toolchains key their tarballs. Derived
    # from the host rather than matched against a covered set, so a host no
    # toolchain vendors a tarball for still has a name to be named in.
    def self.build_platform(platform = RUBY_PLATFORM)
      "#{platform_arch(platform)}-#{platform_os(platform)}"
    end

    def self.platform_arch(platform)
      arch = platform.split("-").first.to_s
      ARCH_ALIASES.fetch(arch, arch)
    end

    def self.platform_os(platform)
      OS_ALIASES.find { |pattern, _| pattern.match?(platform) }&.last ||
        platform.split("-")[1].to_s
    end

    private_class_method :platform_arch, :platform_os

    # The build platform, resolved once at load.
    BUILD_PLATFORM = build_platform
  end
end
