# frozen_string_literal: true

# Rust workspace support module
# =============================
#
# Pure-Ruby helpers backing +tasks/rust.rake+. Owns cargo availability
# and wasm32-wasip1 target detection. The .rake wrapper is the rake DSL
# surface that glues these helpers to +rake rust:check+ /
# +rake rust:test+ / +rake rust:check:wasm+.

require "fileutils"
require "open3"
require "rbconfig"

require "beni/builder"
require "beni/vendor"

# Helpers for the Cargo workspace at the repo root. See sibling
# +tasks/rust.rake+ for the rake DSL.
module BeniRust
  ROOT = File.expand_path("../..", __dir__)
  VENDOR_DIR = File.join(ROOT, "vendor")
  MRUBY_SOURCE_DIR = File.join(VENDOR_DIR, "mruby")
  WASM_TARGET = "wasm32-wasip1"

  # Whether cargo can be spawned. Asking cargo itself needs no lookup
  # tool, which +which+ is on POSIX hosts and is not on Windows.
  def self.cargo_available?
    !system("cargo", "--version", %i[out err] => File::NULL).nil?
  end

  # Stop +task+ before it spawns a cargo that is not there, naming the
  # task the caller asked for.
  def self.require_cargo!(task)
    return if cargo_available?

    abort "cargo not on PATH; install Rust toolchain to run #{task}"
  end

  # Stop a wasm leg whose target standard library is not installed.
  def self.require_wasm_target!
    return if wasm_target_installed?

    abort "#{WASM_TARGET} is not installed; run `rustup target add #{WASM_TARGET}` " \
          "(rust-toolchain.toml declares it, so a rustup-managed toolchain has it)"
  end

  # Archive discovery env for host-target cargo runs: the vendor tree
  # `beni:build` populates, named explicitly because beni-sys's
  # discovery is environment-driven with no fallback. A shell's
  # +MRUBY_LIB_DIR+ outranks the vendor tree there, so it is cleared.
  def self.host_env
    { "BENI_VENDOR_DIR" => VENDOR_DIR, "MRUBY_LIB_DIR" => nil }
  end

  # Archive discovery env for wasm32 cargo runs: cross targets never
  # read the vendor tree, so the wasi staged path and the unpacked
  # wasi-sdk root are named directly. +lib_dir+ defaults to the wasi
  # archive `beni:build` stages.
  def self.wasm_env(lib_dir = File.join(MRUBY_SOURCE_DIR, "build", "wasi", "lib"))
    {
      "MRUBY_LIB_DIR" => lib_dir,
      "WASI_SDK_PATH" => File.join(VENDOR_DIR, "wasi-sdk")
    }
  end

  # What every cargo test leg hands its test binaries: a skip for each
  # test BENI_TEST_SKIP names, whitespace-separated. A lane running a
  # release the crates do not support yet lists its known failures
  # there, so the chain still fails on any other.
  def self.test_harness_args
    names = ENV.fetch("BENI_TEST_SKIP", "").split
    return [] if names.empty?

    ["--", *names.flat_map { |name| ["--skip", name] }]
  end

  # What a documentation host sets: DOCS_RS on and no archive discovery
  # variable, so beni-sys stages the generated documentation bindings.
  # Its own target dir, since flipping DOCS_RS would otherwise
  # invalidate the main verification cache.
  DOCUMENTATION_ENV = { "DOCS_RS" => "1", "BENI_VENDOR_DIR" => nil, "MRUBY_LIB_DIR" => nil }.freeze
  DOCUMENTATION_TARGET_DIR = File.join(ROOT, "target", "docs-rs")

  # Whether the generated documentation bindings still declare
  # everything the crate calls. rustdoc type checks signatures and not
  # function bodies, so the render below cannot answer this: the
  # bindings surface grows by a `sys::` call inside a body, which is
  # exactly what a render passes over. Only a compile asks.
  def self.documentation_build_check
    run!(DOCUMENTATION_ENV, "cargo", "check", "-p", "beni-sys", "-p", "beni",
         "--target-dir", DOCUMENTATION_TARGET_DIR, chdir: ROOT)
  end

  # Render the docs the way a documentation host does, holding rustdoc's
  # own diagnostics — broken intra-doc links, malformed examples — to
  # the same bar the rest of the build meets.
  def self.documentation_build_doc
    run!(DOCUMENTATION_ENV.merge("RUSTDOCFLAGS" => "-D warnings"),
         "cargo", "doc", "-p", "beni-sys", "-p", "beni", "--no-deps",
         "--target-dir", DOCUMENTATION_TARGET_DIR, chdir: ROOT)
  end

  # Echo-then-run with the env overlay, raising on failure — the same
  # subprocess shape `rake sh` provides, available outside the DSL.
  def self.run!(env, *cmd, chdir:)
    echo("rust", env, cmd, chdir: chdir)
    system(env, *cmd, chdir: chdir, exception: true)
  end

  # Print a subprocess as it will run — its directory, env overlay, and
  # command — under +tag+.
  def self.echo(tag, env, cmd, chdir:)
    puts "[#{tag}] cd #{chdir} && #{env.map { |k, v| "#{k}=#{v}" }.join(" ")} #{cmd.join(" ")}"
  end

  # Whether the wasm target's standard library is installed. The sysroot
  # prints for any triple rustc knows, installed or not, so what settles
  # it is the target's own lib directory holding the compiled std.
  def self.wasm_target_installed?
    libdir, status = Open3.capture2("rustc", "--print", "target-libdir", "--target", WASM_TARGET)
    status.success? && !Dir.glob(File.join(libdir.strip, "*.rlib")).empty?
  rescue StandardError
    false
  end
end

require_relative "beni_rust/abi_legs"
