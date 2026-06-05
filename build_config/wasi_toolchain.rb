# frozen_string_literal: true

# WASM-target toolchain template for mruby build configs.
# ========================================================
#
# Registers the +:wasi+ MRuby::Toolchain — wasi-sdk absolute tool paths,
# wasm32-wasi target / sysroot flags, the setjmp/longjmp three-flag set,
# and the GNU archive format. A cross build opts in via
# +conf.toolchain :wasi+.
#
# Loaded by the repo's validation config (+beni.rb+, sibling file) via
# relative path; custom build configs copy what they need.
#
# +Beni::Builder+ exports +BENI_VENDOR_DIR+ (the vendor tree root) when
# it drives mruby's rake; +WASI_SDK_PATH+ overrides the wasi-sdk
# location directly.
#
# Config-time constants live in a dedicated namespace, only defined on
# first load, so `load`-ing this file twice in the same process does not
# warn about constant redefinition. (The MRuby::Toolchain registration
# below is outside the guard but idempotent — a second load merely
# overwrites the registry entry with an identical definition.)
unless defined?(BeniWasiToolchain)
  # Config-time constants for the +:wasi+ toolchain definition below.
  module BeniWasiToolchain
    # +Beni::Builder+ always exports +BENI_VENDOR_DIR+; the bare
    # +vendor/+ fallback only suits running mruby's rake by hand from a
    # project root (under the builder, pwd is the mruby tree itself).
    VENDOR_DIR = (ENV["BENI_VENDOR_DIR"] || File.expand_path("vendor")).freeze
    WASI_SDK   = (ENV["WASI_SDK_PATH"] || File.join(VENDOR_DIR, "wasi-sdk")).freeze
    WASI_SYSROOT = File.join(WASI_SDK, "share", "wasi-sysroot").freeze

    # The three setjmp/longjmp flags. All three must be present at *both*
    # compile and link stages; missing any one trips wasi-libc's
    # `<setjmp.h>` build-time `#error`.
    SJLJ_FLAGS = [
      "-mllvm", "-wasm-enable-sjlj",
      "-mllvm", "-wasm-use-legacy-eh=false"
    ].freeze

    # Cross-compile target. `wasm32-wasi` is the LLVM triple (same ABI
    # as Rust's `wasm32-wasip1` target); the LLVM-triple form is what
    # clang accepts on the command line.
    WASI_TARGET = "wasm32-wasi"

    # Target / sysroot flags applied to every translation unit AND the
    # link step. Frozen so a stray `<<` in a build block raises instead
    # of silently mutating the shared reference.
    TARGET_FLAGS = [
      "--target=#{WASI_TARGET}",
      "--sysroot=#{WASI_SYSROOT}"
    ].freeze
  end
end

# +:wasi+ toolchain — wasi-sdk absolute tool paths, wasm32-wasi target /
# sysroot flags, the setjmp/longjmp three-flag set, and the GNU archive
# format.
MRuby::Toolchain.new(:wasi) do |conf, _params|
  wasi_sdk_bin = File.join(BeniWasiToolchain::WASI_SDK, "bin")

  conf.toolchain :clang

  # ---- Tool commands pinned to wasi-sdk absolute paths -----------------
  conf.cc.command       = File.join(wasi_sdk_bin, "clang")
  conf.cxx.command      = File.join(wasi_sdk_bin, "clang++")
  conf.linker.command   = File.join(wasi_sdk_bin, "clang")
  conf.archiver.command = File.join(wasi_sdk_bin, "llvm-ar")
  # llvm-ar on macOS hosts defaults to Darwin (BSD) archive format,
  # which can fail with "section too large" when the archive contains
  # many wasm objects with long member paths. GNU format uses an
  # extended string table.
  conf.archiver.archive_options = "--format=gnu rs %<outfile>s %<objs>s"

  # ---- Cross-compile target / sysroot ----------------------------------
  conf.cc.flags     << BeniWasiToolchain::TARGET_FLAGS
  conf.cxx.flags    << BeniWasiToolchain::TARGET_FLAGS
  conf.linker.flags << BeniWasiToolchain::TARGET_FLAGS

  # ---- setjmp/longjmp ----------------------------------------------------
  # Apply at compile AND link stages — the three-flag set is non-negotiable.
  conf.cc.flags     << BeniWasiToolchain::SJLJ_FLAGS
  conf.cxx.flags    << BeniWasiToolchain::SJLJ_FLAGS
  conf.linker.flags << BeniWasiToolchain::SJLJ_FLAGS
  conf.linker.libraries << "setjmp" # expands to `-lsetjmp` (wasi-libc libsetjmp.a)
end
