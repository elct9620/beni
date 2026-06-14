# frozen_string_literal: true

require_relative "vendor/downloader"
require_relative "vendor/checksum"
require_relative "vendor/tarball"
require_relative "vendor/toolchain"

module Beni
  # Vendor toolchain façade. Owns the release-vendored toolchain pins and
  # the factory methods that build declarative +Toolchain+ values anchored
  # on a caller-supplied +vendor_dir+; +Beni::Tasks+ calls the factories
  # with each selected toolchain's resolved +(version, sha256)+ to wire
  # +file+ / +task+ declarations. Network download lives in
  # +Vendor::Downloader+, SHA256 verification in +Vendor::Checksum+,
  # tarball extraction in +Vendor::Tarball+, and the per-toolchain
  # pipeline composition in +Vendor::Toolchain+.
  #
  # Honors +BENI_VENDOR_BASE_URL+ to point downloads at a local fixture
  # during tests.
  module Vendor
    # ---- Built-in pairs ----------------------------------------------------
    # The version and checksum pair this release vendors per toolchain.
    # wasi-sdk ships one tarball per build platform, so its checksum is
    # keyed by +WASI_SDK_PLATFORM+ (values from the GitHub release asset
    # digests); mruby's source archive is host-agnostic with a single
    # checksum.
    #
    # wasi-sdk: must be >= 26 for native wasm32-wasip1 setjmp/longjmp
    # support. 33's +libc.a+ supplies +__wasi_init_tp+, which Rust's
    # wasm32-wasip1 +crt1-command.o+ references from 1.96 onward. Keep in
    # lockstep with the channel in +rust-toolchain.toml+ — in both this
    # repo and kobako.
    BUILT_IN_PAIRS = {
      "mruby" => {
        version: "4.0.0",
        sha256: "e2ea271dbed14e9f2b33df773ae447b747dbc242ce2675022c0a57efea85a7b4"
      },
      "wasi-sdk" => {
        version: "33.0",
        sha256: {
          "arm64-linux" => "4f98ee738c7abb45c81a94d1461fc53cc569d1cd01498951c8184d841a027844",
          "arm64-macos" => "85c997a2665ead91673b5bb88b7d0df3fc8900df3bfa244f720d478187bbdc78",
          "x86_64-linux" => "0ba8b5bfaeb2adf3f29bab5841d76cf5318ab8e1642ea195f88baba1abd47bce",
          "x86_64-macos" => "18f3f201ba9734e6a4455b0b6410690395a55e9ffa9f6f5066f66083a94b93b3"
        }
      }
    }.freeze

    # Transitive toolchain dependencies, folded into the selected set at
    # task-definition time: referencing wasi-sdk implies mruby.
    DEPENDENCIES = { "wasi-sdk" => %w[mruby] }.freeze

    # Map a host triple to the platform token wasi-sdk keys its per-platform
    # tarballs by (wasi-sdk only; mruby's tarball is host-agnostic).
    # +x86_64-linux+ is both the most common host and the safest fallback
    # for unrecognised triples, so it is the +else+ branch.
    def self.wasi_sdk_platform(platform = RUBY_PLATFORM)
      case platform
      when /arm64-darwin|aarch64-darwin/ then "arm64-macos"
      when /x86_64-darwin/               then "x86_64-macos"
      when /aarch64-linux|arm64-linux/   then "arm64-linux"
      else "x86_64-linux"
      end
    end

    # The build platform's wasi-sdk token, resolved once at load.
    WASI_SDK_PLATFORM = wasi_sdk_platform

    # Known toolchain names mapped to their factory methods — the name
    # domain the DSL validates against and +Beni::Tasks+ dispatches on.
    TOOLCHAIN_FACTORIES = {
      "mruby" => :mruby,
      "wasi-sdk" => :wasi_sdk
    }.freeze

    module_function

    def wasi_sdk(vendor_dir:, version: nil, sha256: nil)
      version ||= BUILT_IN_PAIRS.fetch("wasi-sdk").fetch(:version)
      # The release tag carries the major version only; tarballs carry the
      # full version plus the platform.
      Toolchain.new(
        name: "wasi-sdk",
        version_label: "#{version} (#{WASI_SDK_PLATFORM})",
        base_url: "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-#{version.split(".").first}",
        tarball_name: "wasi-sdk-#{version}-#{WASI_SDK_PLATFORM}.tar.gz",
        top_level_dir: "wasi-sdk-#{version}-#{WASI_SDK_PLATFORM}",
        vendor_dir: vendor_dir,
        expected_sha256: sha256 || built_in_sha256("wasi-sdk", version)
      )
    end

    def mruby(vendor_dir:, version: nil, sha256: nil)
      version ||= BUILT_IN_PAIRS.fetch("mruby").fetch(:version)
      Toolchain.new(
        name: "mruby",
        version_label: version,
        base_url: "https://github.com/mruby/mruby/archive/refs/tags",
        tarball_name: "#{version}.tar.gz",
        top_level_dir: "mruby-#{version}",
        vendor_dir: vendor_dir,
        expected_sha256: sha256 || built_in_sha256("mruby", version)
      )
    end

    # When BENI_VENDOR_BASE_URL is set, all tarballs are fetched from that
    # base URL (test fixture). The base URL must serve files named exactly
    # +tarball_name+ for each toolchain.
    def base_url_for(default)
      override = ENV.fetch("BENI_VENDOR_BASE_URL", nil)
      return default if override.nil? || override.empty?

      override.chomp("/")
    end

    # The built-in checksum for +name+ at +version+: the build platform's
    # entry when the toolchain's checksums are platform-keyed, +nil+ for
    # any version other than the vendored one — mruby's TOFU pinning path.
    def built_in_sha256(name, version)
      pair = BUILT_IN_PAIRS.fetch(name)
      return nil unless version == pair.fetch(:version)

      checksum = pair.fetch(:sha256)
      checksum.is_a?(Hash) ? checksum.fetch(WASI_SDK_PLATFORM) : checksum
    end

    # Write the gem-shipped wasi toolchain file into the staged mruby
    # source, where mruby's +conf.toolchain :wasi+ resolves it from.
    # Idempotent and always overwriting, so a re-extracted tree or an
    # older beni's copy converges on this release's definition. Returns
    # the staged path.
    def stage_wasi_toolchain_file(vendor_dir:)
      # `__dir__ || "."`: __dir__ is only nil under eval, which never
      # loads this file; the fallback satisfies steep's String? typing.
      source = File.expand_path("vendor/toolchains/wasi.rake", __dir__ || ".")
      target = File.join(vendor_dir, "mruby", "tasks", "toolchains", "wasi.rake")
      FileUtils.mkdir_p(File.dirname(target))
      FileUtils.cp(source, target)
      target
    end
  end
end
