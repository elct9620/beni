# frozen_string_literal: true

module Beni
  # Resolution output of the Tasks DSL — immutable, defaults applied.
  # The only input the task-definition phase reads: +vendor_dir+ fully
  # resolved (declaration > +BENI_VENDOR_DIR+ > +vendor/+), +build_config+
  # as an absolute path or +nil+ for mruby's untouched upstream default,
  # +targets+ as the declared set (or +["host"]+), and +toolchains+ as
  # the reference-driven selection.
  class Configuration < Data.define(:vendor_dir, :build_config, :targets, :toolchains)
    # mruby's selected version. Resolution always selects +mruby+ and
    # leads the toolchain set with it, so it is the head's version.
    def mruby_version
      toolchains.first.version
    end
  end
end
