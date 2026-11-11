# frozen_string_literal: true

# beni's wasm32 WASI cross-compile toolchain — the wasi toolchain file,
# staged into the mruby tree's +tasks/toolchains/+ by
# +beni:vendor:setup+ when the tree holds none of its own. A build
# config activates it with +conf.toolchain :wasi+, or names another
# WASI target with +conf.toolchain :wasi, target: "wasm32-wasip2"+ —
# the parameter mruby's own wasi toolchain file takes.
MRuby::Toolchain.new(:wasi) do |conf, params|
  # +beni:build+ points WASI_SDK_PATH at the vendored wasi-sdk.
  wasi_sdk = ENV["WASI_SDK_PATH"] || "/opt/wasi-sdk"
  bin = File.join(wasi_sdk, "bin")
  target = params[:target] || "wasm32-wasip1"
  target_flags = ["--target=#{target}", "--sysroot=#{File.join(wasi_sdk, "share", "wasi-sysroot")}"]
  # setjmp/longjmp via the wasm exception-handling mechanism: these
  # flags go to every compile and the link, and the link adds the
  # setjmp library.
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
