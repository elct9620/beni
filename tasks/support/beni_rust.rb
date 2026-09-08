# frozen_string_literal: true

# Rust workspace support module
# =============================
#
# Pure-Ruby helpers backing +tasks/rust.rake+. Owns cargo availability
# and wasm32-wasip1 target detection. The .rake wrapper is the rake DSL
# surface that glues these helpers to +rake rust:check+ /
# +rake rust:test+ / +rake rust:check:wasm+.

require "open3"
require "rbconfig"

# Helpers for the Cargo workspace at the repo root. See sibling
# +tasks/rust.rake+ for the rake DSL.
module BeniRust
  ROOT = File.expand_path("../..", __dir__)
  WASM_TARGET = "wasm32-wasip1"
  MISSING_WASM_TARGET = "#{WASM_TARGET} is not installed; run `rustup target add #{WASM_TARGET}` " \
                        "(rust-toolchain.toml declares it, so a rustup-managed toolchain has it)".freeze

  # Scratch build dir for the default-ABI leg. Lives under tmp/
  # (gitignored) and is incremental across runs.
  DEFAULT_ABI_BUILD_DIR = File.join(ROOT, "tmp", "mruby-default-build")

  def self.cargo_available?
    system("which cargo > /dev/null 2>&1")
  end

  # Archive discovery env for host-target cargo runs: the vendor tree
  # `beni:build` populates, named explicitly because beni-sys's
  # discovery is environment-driven with no fallback.
  def self.host_env
    { "BENI_VENDOR_DIR" => File.join(ROOT, "vendor") }
  end

  # Archive discovery env for wasm32 cargo runs: cross targets never
  # read the vendor tree, so the wasi staged path and the unpacked
  # wasi-sdk root are named directly.
  def self.wasm_env
    {
      "MRUBY_LIB_DIR" => File.join(ROOT, "vendor", "mruby", "build", "wasi", "lib"),
      "WASI_SDK_PATH" => File.join(ROOT, "vendor", "wasi-sdk")
    }
  end

  # The default-ABI leg: build the vendored mruby with NO MRUBY_CONFIG
  # (mruby falls back to its own build_config/default.rb — what a
  # clean consumer build gets), then run the wrapper tests against
  # that archive. On 64-bit hosts this exercises the
  # mrb_int = int64_t layout the repo's MRB_INT32 validation config
  # never sees, so type/width coincidences cannot hide. The cargo
  # target dir is split off so the MRUBY_LIB_DIR switch does not
  # invalidate the main verification cache.
  def self.default_abi_test
    lib_dir = File.join(DEFAULT_ABI_BUILD_DIR, "host", "lib")
    flags_mak = File.join(lib_dir, "libmruby.flags.mak")

    run!({ "MRUBY_BUILD_DIR" => DEFAULT_ABI_BUILD_DIR },
         RbConfig.ruby, "-S", "rake", "default", flags_mak,
         chdir: File.join(ROOT, "vendor", "mruby"))
    run!({ "MRUBY_LIB_DIR" => lib_dir },
         "cargo", "test", "-p", "beni", "-p", "beni-tests",
         "--target-dir", File.join(ROOT, "target", "default-abi"),
         chdir: ROOT)
  end

  # What a documentation host sets: DOCS_RS on and no archive discovery
  # variable, so beni-sys stages the checked-in documentation bindings.
  # Its own target dir, since flipping DOCS_RS would otherwise
  # invalidate the main verification cache.
  DOCUMENTATION_ENV = { "DOCS_RS" => "1", "BENI_VENDOR_DIR" => nil, "MRUBY_LIB_DIR" => nil }.freeze
  DOCUMENTATION_TARGET_DIR = File.join(ROOT, "target", "docs-rs")

  # Whether the checked-in documentation bindings still declare
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
    puts "[rust] cd #{chdir} && #{env.map { |k, v| "#{k}=#{v}" }.join(" ")} #{cmd.join(" ")}"
    system(env, *cmd, chdir: chdir, exception: true)
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
