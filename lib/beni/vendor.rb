# frozen_string_literal: true

require_relative "vendor/downloader"
require_relative "vendor/checksum"
require_relative "vendor/tarball"
require_relative "vendor/toolchain"

module Beni
  # Vendor toolchain façade. Owns the pinned toolchain versions and the
  # factory methods that build declarative +Toolchain+ values anchored on
  # a caller-supplied +vendor_dir+; +Beni::Tasks+ iterates +toolchains+
  # to wire +file+ / +task+ declarations. Network download lives in
  # +Vendor::Downloader+, SHA256 verification in +Vendor::Checksum+,
  # tarball extraction in +Vendor::Tarball+, and the per-toolchain
  # pipeline composition in +Vendor::Toolchain+.
  #
  # Honors +BENI_VENDOR_BASE_URL+ to point downloads at a local fixture
  # during tests.
  #
  # Extending: a new tarball-based vendor artifact is added with one
  # factory method appended to +toolchains+ and (if its hash pinning is
  # enforced via CI env var) a +BENI_VENDOR_<KEY>_SHA256+ entry in the
  # deployment env.
  module Vendor
    # ---- Pinned versions ---------------------------------------------------
    # wasi-sdk: must be >= 26 for native wasm32-wasip1 setjmp/longjmp support.
    # 33's +libc.a+ supplies +__wasi_init_tp+, which Rust's wasm32-wasip1
    # +crt1-command.o+ references from 1.96 onward (wasi-sdk 26's libc lacks it,
    # breaking the command-bin link). Keep in lockstep with the channel in
    # +rust-toolchain.toml+ — in both this repo and kobako.
    WASI_SDK_VERSION      = "33"
    WASI_SDK_MINOR        = "0"
    WASI_SDK_FULL_VERSION = "#{WASI_SDK_VERSION}.#{WASI_SDK_MINOR}".freeze

    # mruby: pinned release tarball.
    MRUBY_VERSION = "4.0.0"

    # ---- Platform detection (wasi-sdk only; mruby tarball is host-agnostic).
    # +x86_64-linux+ is both the most common host triple and the safest
    # fallback for unrecognised ones, so we collapse both cases into the
    # +else+ branch rather than carrying an explicit +when+ that would
    # duplicate the default.
    WASI_SDK_PLATFORM =
      case RUBY_PLATFORM
      when /arm64-darwin|aarch64-darwin/ then "arm64-macos"
      when /x86_64-darwin/               then "x86_64-macos"
      when /aarch64-linux|arm64-linux/   then "arm64-linux"
      else "x86_64-linux"
      end

    module_function

    # All tarball-based vendor toolchains, anchored on +vendor_dir+.
    def toolchains(vendor_dir:)
      [wasi_sdk(vendor_dir: vendor_dir), mruby(vendor_dir: vendor_dir)]
    end

    def wasi_sdk(vendor_dir:)
      Toolchain.new(
        name: "wasi-sdk",
        version_label: "#{WASI_SDK_FULL_VERSION} (#{WASI_SDK_PLATFORM})",
        base_url: "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-#{WASI_SDK_VERSION}",
        tarball_name: "wasi-sdk-#{WASI_SDK_FULL_VERSION}-#{WASI_SDK_PLATFORM}.tar.gz",
        top_level_dir: "wasi-sdk-#{WASI_SDK_FULL_VERSION}-#{WASI_SDK_PLATFORM}",
        vendor_dir: vendor_dir
      )
    end

    def mruby(vendor_dir:)
      Toolchain.new(
        name: "mruby",
        version_label: MRUBY_VERSION,
        base_url: "https://github.com/mruby/mruby/archive/refs/tags",
        tarball_name: "#{MRUBY_VERSION}.tar.gz",
        top_level_dir: "mruby-#{MRUBY_VERSION}",
        vendor_dir: vendor_dir
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

    # Expected SHA256 for a vendored tarball, sourced from
    # +BENI_VENDOR_<KEY>_SHA256+ env vars (empty string falls back to TOFU
    # sidecar pinning in +Checksum#verify_or_pin+). +key+ is the artifact
    # slug in upper snake case, e.g. +"WASI_SDK"+, +"MRUBY"+.
    def expected_sha256(key)
      ENV.fetch("BENI_VENDOR_#{key}_SHA256", "")
    end
  end
end
