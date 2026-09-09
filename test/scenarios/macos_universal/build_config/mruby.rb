# frozen_string_literal: true

# One target per macOS architecture.
#
# A consumer shipping a universal binary builds its crate once per
# architecture and merges the two with lipo; each of those builds links
# the archive built here for its own architecture. Copy this into a
# consumer project and edit the gems.
#
# The cross target is an +MRuby::CrossBuild+ so it borrows the mrbc the
# +host+ build produced: a compiler built for the other architecture
# cannot run on this one.

require "rbconfig"

# mruby writes its mrbgems lockfile next to this file; dependency
# pinning is beni's to own, not mruby's.
MRuby::Lockfile.disable

# The architecture this machine is not; the host build covers the other.
CROSS_ARCH = RbConfig::CONFIG["host_cpu"] == "arm64" ? "x86_64" : "arm64"

MRuby::Build.new("host") do |conf|
  conf.toolchain :clang

  conf.gem core: "mruby-compiler"
  conf.build_mrbc_exec
end

MRuby::CrossBuild.new("cross") do |conf|
  conf.toolchain :clang

  conf.cc.flags << "-arch #{CROSS_ARCH}"
  conf.linker.flags << "-arch #{CROSS_ARCH}"

  conf.gem core: "mruby-compiler"
end
