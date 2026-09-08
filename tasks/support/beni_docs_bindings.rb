# frozen_string_literal: true

# Documentation bindings support
# ==============================
#
# Backs +rake docs:bindings+: rewrites
# +crates/beni-sys/src/bindings_docs.rs+, the declarations a
# documentation build reads where no archive can be staged. They come
# from an mruby built with its own upstream default config, so the
# rendered surface carries what a consumer gets before editing anything.
#
# Bindings are shaped by the host as well as the config — the platform
# decides +va_list+'s form and which constants its headers define — so
# they are generated on the platform the documentation host builds on
# and nowhere else. Another host would write a different file, which the
# freshness gate would read as drift.
#
# The file is whatever the crate's own build script wrote for that
# archive, taken from the +out_dir+ cargo reports for it. Nothing here
# knows how a binding is generated, so the checked-in file cannot state
# a surface the crate would not produce for itself.

require "json"
require "open3"
require "rbconfig"

require_relative "beni_rust"

# Regeneration of the documentation bindings. See sibling
# +tasks/docs.rake+ for the rake DSL.
module BeniDocsBindings
  ROOT = File.expand_path("../..", __dir__)
  CRATE = "beni-sys"
  TARGET = File.join(ROOT, "crates", CRATE, "src", "bindings_docs.rs")
  BUILD_DIR = File.join(ROOT, "tmp", "docs-bindings-target")
  # docs.rs builds every crate on this target and cross-compiles the
  # rest, so this is the platform the checked-in bindings describe.
  DOCUMENTATION_HOST = "x86_64-unknown-linux-gnu"
  HEADER = <<~RUST
    // Documentation bindings, written by `rake docs:bindings` — never
    // edited. Read by a documentation build alone, where no archive can
    // be staged; every other build generates its own from the archive it
    // discovered. See SPEC.md's Terminology.
  RUST

  module_function

  # Rewrite the checked-in bindings and return the path written.
  def generate
    require_documentation_host!
    out_dir = build_out_dir(upstream_default_lib_dir)
    File.write(TARGET, HEADER + File.read(File.join(out_dir, "bindings.rs")))
    TARGET
  end

  # Refuse to write bindings this host would shape differently from the
  # one that reads them.
  def require_documentation_host!
    return if rustc_host == DOCUMENTATION_HOST

    abort "docs:bindings runs on #{DOCUMENTATION_HOST}, the target the documentation host " \
          "builds on; this is #{rustc_host}. CI regenerates the file on every verify run and " \
          "attaches it when it differs from the committed one."
  end

  # The triple rustc reports for this machine.
  def rustc_host
    out, status = Open3.capture2("rustc", "-vV")
    raise "rustc -vV failed" unless status.success?

    out[/^host: (.+)$/, 1]
  end

  # The mruby built with no MRUBY_CONFIG, so mruby's own
  # build_config/default.rb decides the ABI. Shares the build tree the
  # default-ABI test leg uses.
  def upstream_default_lib_dir
    lib_dir = File.join(BeniRust::DEFAULT_ABI_BUILD_DIR, "host", "lib")
    BeniRust.run!({ "MRUBY_BUILD_DIR" => BeniRust::DEFAULT_ABI_BUILD_DIR },
                  RbConfig.ruby, "-S", "rake", "default",
                  File.join(lib_dir, "libmruby.flags.mak"),
                  chdir: File.join(ROOT, "vendor", "mruby"))
    lib_dir
  end

  # Build the crate against +lib_dir+ and return the OUT_DIR cargo
  # reports for its build script — the documented +out_dir+ field of the
  # +build-script-executed+ JSON message.
  def build_out_dir(lib_dir)
    stdout = capture!({ "MRUBY_LIB_DIR" => lib_dir },
                      "cargo", "build", "-p", CRATE,
                      "--target-dir", BUILD_DIR, "--message-format", "json")
    dirs = stdout.each_line.filter_map { |line| out_dir_of(JSON.parse(line)) }.uniq
    raise "cargo reported #{dirs.size} build script out_dir for #{CRATE}" unless dirs.size == 1

    dirs.first
  end

  # The out_dir a cargo JSON message carries for this crate's build
  # script, or nil for every other message.
  def out_dir_of(message)
    return nil unless message["reason"] == "build-script-executed"
    return nil unless message["package_id"].to_s.include?(CRATE)

    message["out_dir"]
  end

  # Echo-then-run capturing stdout, raising on failure. cargo's progress
  # goes to stderr, so it still reaches the terminal while the JSON
  # stream is read here.
  def capture!(env, *cmd)
    puts "[docs] cd #{ROOT} && #{env.map { |k, v| "#{k}=#{v}" }.join(" ")} #{cmd.join(" ")}"
    stdout, status = Open3.capture2(env, *cmd, chdir: ROOT)
    raise "#{cmd.first} failed with #{status.exitstatus}" unless status.success?

    stdout
  end
end
