# frozen_string_literal: true

# Vendor toolchain support module
# ===============================
#
# Pure-Ruby helpers backing +tasks/vendor.rake+, ported from kobako as
# part of the temporary Rust-side compile-verification chain. Owns
# pinned versions and declarative +Toolchain+ instances; the rake DSL
# surface in the .rake wrapper iterates +TARBALL_TOOLCHAINS+ to wire
# +file+ / +task+ declarations. Network download lives in
# +BeniVendor::Downloader+, SHA256 verification in
# +BeniVendor::Checksum+, tarball extraction in +BeniVendor::Tarball+,
# and the per-toolchain pipeline composition in +BeniVendor::Toolchain+.
#
# Honors +BENI_VENDOR_BASE_URL+ to point downloads at a local fixture
# during tests, and +BENI_VENDOR_DIR+ to relocate the entire vendor
# tree (also test-only).
#
# Extending: a new tarball-based vendor artifact is added by appending
# one +Toolchain.new(...)+ to +TARBALL_TOOLCHAINS+ and (if its hash
# pinning is enforced via CI env var) adding a +BENI_VENDOR_<KEY>_SHA256+
# entry to the deployment env.

require_relative "beni_vendor/downloader"
require_relative "beni_vendor/checksum"
require_relative "beni_vendor/tarball"
require_relative "beni_vendor/toolchain"

# Vendor toolchain façade.
module BeniVendor
  ROOT       = File.expand_path("../..", __dir__)
  VENDOR_DIR = (ENV["BENI_VENDOR_DIR"] || File.join(ROOT, "vendor")).freeze
  CACHE_DIR  = File.join(VENDOR_DIR, ".cache").freeze

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

  WASI_SDK = Toolchain.new(
    name: "wasi-sdk",
    version_label: "#{WASI_SDK_FULL_VERSION} (#{WASI_SDK_PLATFORM})",
    base_url: "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-#{WASI_SDK_VERSION}",
    tarball_name: "wasi-sdk-#{WASI_SDK_FULL_VERSION}-#{WASI_SDK_PLATFORM}.tar.gz",
    top_level_dir: "wasi-sdk-#{WASI_SDK_FULL_VERSION}-#{WASI_SDK_PLATFORM}",
    final_dir: File.join(VENDOR_DIR, "wasi-sdk"),
    sha_key: :WASI_SDK
  )

  MRUBY = Toolchain.new(
    name: "mruby",
    version_label: MRUBY_VERSION,
    base_url: "https://github.com/mruby/mruby/archive/refs/tags",
    tarball_name: "#{MRUBY_VERSION}.tar.gz",
    top_level_dir: "mruby-#{MRUBY_VERSION}",
    final_dir: File.join(VENDOR_DIR, "mruby"),
    sha_key: :MRUBY
  )

  TARBALL_TOOLCHAINS = [WASI_SDK, MRUBY].freeze

  module_function

  # When BENI_VENDOR_BASE_URL is set, both tarballs are fetched from that
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
  # slug in upper snake case, e.g. +:WASI_SDK+, +:MRUBY+.
  def expected_sha256(key)
    ENV.fetch("BENI_VENDOR_#{key}_SHA256", "")
  end
end
