# frozen_string_literal: true

# mruby static-library build task
# ===============================
#
# Drives mruby's bundled `minirake` against `build_config/beni.rb`,
# producing the per-target `libmruby.a` archives the beni crates link
# against (host native + wasm32-wasip1). This task is the single,
# idempotent entry point:
#
#   $ rake mruby:build      # produces vendor/mruby/build/{host,wasi}/lib/libmruby.a
#   $ rake mruby:clean      # removes both build trees
#
# Depends on `vendor:setup` (tasks/vendor.rake), so the mruby + wasi-sdk
# tarballs are present before minirake fires its first compile.
# Idempotency: the underlying minirake is itself a make-style incremental
# build; on top of that, this task short-circuits when every libmruby.a
# sentinel already exists, so a second `rake mruby:build` invocation is a
# no-op without even invoking minirake.

require_relative "support/beni_mruby"

namespace :mruby do
  desc "Build vendored mruby for host + wasm32-wasip1 (produces #{BeniMruby.libmruby_paths.join(", ")})"
  task build: ["vendor:setup"] do
    if BeniMruby.libmruby_paths.all? { |p| File.exist?(p) }
      puts "[mruby] libmruby.a already present for #{BeniMruby::TARGET_NAMES.join(" + ")} — skipping"
      next
    end

    BeniMruby.invoke_minirake

    BeniMruby.libmruby_paths.each do |path|
      raise "[mruby] build completed but #{path} is missing" unless File.exist?(path)
    end

    puts "[mruby] libmruby.a ready for #{BeniMruby::TARGET_NAMES.join(" + ")}"
  end

  desc "Remove mruby's build trees (keeps vendored mruby source)"
  task :clean do
    BeniMruby::TARGET_NAMES.each do |target|
      build_dir = File.join(BeniMruby::MRUBY_DIR, "build", target)
      FileUtils.rm_rf(build_dir)
      puts "[mruby] removed #{build_dir}"
    end
  end
end
