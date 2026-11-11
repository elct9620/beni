# frozen_string_literal: true

require_relative "vendor/platform"
require_relative "vendor/downloader"
require_relative "vendor/checksum"
require_relative "vendor/unvendored_checksum"
require_relative "vendor/tarball"
require_relative "vendor/toolchain"

module Beni
  # Vendor toolchain façade. Owns each toolchain's built-in pair and
  # the factory methods that build declarative +Toolchain+ values anchored
  # on a caller-supplied +vendor_dir+; +Beni::Tasks+ calls the factories
  # with each selected toolchain's resolved +(version, sha256)+ to wire
  # +file+ / +task+ declarations. Network download lives in
  # +Vendor::Downloader+, SHA256 verification in +Vendor::Checksum+,
  # tarball extraction in +Vendor::Tarball+, and the per-toolchain
  # pipeline composition in +Vendor::Toolchain+.
  #
  # Honors +BENI_VENDOR_BASE_URL+ to move every download to a mirror.
  module Vendor
    # ---- Built-in pairs ----------------------------------------------------
    # The version and checksum pair this release vendors per toolchain.
    # wasi-sdk ships one tarball per build platform, so its checksum is
    # keyed by +BUILD_PLATFORM+ (values from the GitHub release asset
    # digests); mruby's source archive is host-agnostic with a single
    # checksum.
    #
    # wasi-sdk: must be >= 26 for native WASI setjmp/longjmp support.
    # 33's +libc.a+ supplies +__wasi_init_tp+, which Rust's
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

    # Toolchains only a top-level definition selects. Each completes the
    # mruby source rather than serving a target, and has no built-in pair.
    SELECTED_BY_DEFINITION = %w[prism].freeze

    # Known toolchain names mapped to their factory methods — the name
    # domain the DSL validates against and +Beni::Tasks+ dispatches on.
    TOOLCHAIN_FACTORIES = {
      "mruby" => :mruby,
      "wasi-sdk" => :wasi_sdk,
      "prism" => :prism
    }.freeze

    # What every wasi toolchain file beni has shipped says about itself,
    # and an mruby release's own does not.
    WASI_TOOLCHAIN_MARK = "beni:vendor:setup"

    # Where a toolchain staging somewhere other than its own name under
    # the vendor tree unpacks; every other one unpacks at its name.
    STAGED_DIRS = {
      "prism" => File.join("mruby", "mrbgems", "mruby-compiler", "lib", "prism")
    }.freeze

    module_function

    def wasi_sdk(vendor_dir:, version: nil, sha256: nil)
      version ||= built_in_version("wasi-sdk")
      # The release tag carries the major version only; tarballs carry the
      # full version plus the platform.
      Toolchain.new(
        name: "wasi-sdk",
        version_label: "#{version} (#{BUILD_PLATFORM})",
        base_url: "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-#{version.split(".").first}",
        tarball_name: "wasi-sdk-#{version}-#{BUILD_PLATFORM}.tar.gz",
        top_level_dir: "wasi-sdk-#{version}-#{BUILD_PLATFORM}",
        vendor_dir: vendor_dir,
        expected_sha256: sha256 || built_in_sha256("wasi-sdk", version)
      )
    end

    def mruby(vendor_dir:, version: nil, sha256: nil)
      version ||= built_in_version("mruby")
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

    # The Prism source mruby's compiler gem carries as a git submodule,
    # which a release's source tarball leaves out. +version+ is a
    # +ruby/prism+ commit. It stages where the compiler gem reads the
    # submodule from, so mruby's build does not fetch it.
    def prism(vendor_dir:, version:, sha256:)
      Toolchain.new(
        name: "prism",
        version_label: version,
        base_url: "https://github.com/ruby/prism/archive",
        tarball_name: "#{version}.tar.gz",
        top_level_dir: "prism-#{version}",
        vendor_dir: vendor_dir,
        staged_dir: staged_dir("prism"),
        expected_sha256: sha256
      )
    end

    # When BENI_VENDOR_BASE_URL is set, all tarballs are fetched from that
    # mirror, which serves each toolchain's tarball under its exact
    # +tarball_name+.
    def base_url_for(default)
      override = ENV.fetch("BENI_VENDOR_BASE_URL", nil)
      return default if override.nil? || override.empty?

      override.chomp("/")
    end

    # The version +name+'s built-in pair vendors.
    def built_in_version(name)
      BUILT_IN_PAIRS.fetch(name).fetch(:version)
    end
    private_class_method :built_in_version

    # Where +name+ unpacks, relative to the vendor tree.
    def staged_dir(name)
      STAGED_DIRS.fetch(name, name)
    end

    # The built-in checksum for +name+ at +version+: the build platform's
    # entry when the toolchain's checksums are platform-keyed, +nil+ for
    # any version other than the vendored one — mruby's TOFU pinning path.
    # A platform-keyed pair carrying no entry for +platform+ vendors no
    # tarball there: the lookup answers an +UnvendoredChecksum+, which
    # setup refuses, rather than letting another platform's checksum
    # stand in.
    def built_in_sha256(name, version, platform = BUILD_PLATFORM)
      pair = BUILT_IN_PAIRS.fetch(name)
      return nil unless version == built_in_version(name)

      checksum = pair.fetch(:sha256)
      return checksum unless checksum.is_a?(Hash)

      checksum.fetch(platform) { UnvendoredChecksum.new(name:, version:, platform:) }
    end

    # Put a wasi toolchain file in the staged mruby source, where
    # mruby's +conf.toolchain :wasi+ resolves it from. One the tree
    # already holds that beni did not write — an mruby release's own —
    # is kept; otherwise the gem-shipped file is written, so a
    # re-extracted tree or an older beni's copy converges on this
    # release's definition. Returns the staged path.
    def stage_wasi_toolchain_file(vendor_dir:)
      target = File.join(vendor_dir, "mruby", "tasks", "toolchains", "wasi.rake")
      return target if File.exist?(target) && !File.read(target).include?(WASI_TOOLCHAIN_MARK)

      # `__dir__ || "."`: __dir__ is only nil under eval, which never
      # loads this file; the fallback satisfies steep's String? typing.
      source = File.expand_path("vendor/toolchains/wasi.rake", __dir__ || ".")
      FileUtils.mkdir_p(File.dirname(target))
      FileUtils.cp(source, target)
      target
    end
  end
end
