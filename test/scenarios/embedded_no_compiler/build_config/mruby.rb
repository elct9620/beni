# frozen_string_literal: true

# A build that leaves mruby's compiler gem out.
#
# `conf.toolchain` alone selects the host toolchain and no gembox, so
# the archive carries mruby's core and nothing that compiles Ruby at
# run time. A consumer whose Ruby surface is defined and dispatched
# from Rust wants exactly this, and turns beni's default features off
# to match. Copy this into a consumer project and add the gems that
# consumer needs.
#
# mruby builds its own mrblib with an internal compiler build either
# way, so leaving the gem out removes it from the archive alone.

MRuby::Lockfile.disable

MRuby::Build.new("host") do |conf|
  conf.toolchain
end
