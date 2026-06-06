# frozen_string_literal: true

# beni's wasm32-wasip1 cross-compile toolchain — the wasi toolchain
# file, staged into the mruby tree's +tasks/toolchains/+ by
# +beni:vendor:setup+ whenever wasi-sdk is selected. A build config
# activates it with +conf.toolchain :wasi+.
MRuby::Toolchain.new(:wasi) do |conf, _params|
  # WASI_SDK_PATH overrides the wasi-sdk root; the default is the
  # vendor tree this mruby source is staged in (MRUBY_ROOT's parent).
  wasi_sdk = ENV["WASI_SDK_PATH"] || File.join(File.expand_path("..", MRUBY_ROOT), "wasi-sdk")
  bin = File.join(wasi_sdk, "bin")
  target_flags = ["--target=wasm32-wasi", "--sysroot=#{File.join(wasi_sdk, "share", "wasi-sysroot")}"]
  # setjmp/longjmp via the wasm exception-handling mechanism: all
  # three flags must be present at both compile and link stages.
  sjlj_flags = ["-mllvm", "-wasm-enable-sjlj", "-mllvm", "-wasm-use-legacy-eh=false"]

  conf.toolchain :clang
  conf.cc.command       = File.join(bin, "clang")
  conf.cxx.command      = File.join(bin, "clang++")
  conf.linker.command   = File.join(bin, "clang")
  conf.archiver.command = File.join(bin, "llvm-ar")
  # GNU archive format: llvm-ar defaults to the Darwin format on
  # macOS hosts, which can overflow on many long wasm member paths.
  conf.archiver.archive_options = "--format=gnu rs %<outfile>s %<objs>s"

  [conf.cc, conf.cxx, conf.linker].each do |tool|
    tool.flags << target_flags << sjlj_flags
  end
  conf.linker.libraries << "setjmp"
end
