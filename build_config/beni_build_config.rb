# frozen_string_literal: true

# Config-time constants the repo's mruby build configs share.
# ===========================================================
#
# Each config below +build_config/+ decides its own targets and ABI;
# what they share is the mrbgem baseline, and the ABI defines the
# validation build pins. Kept in one file so a config verifying a
# different ABI differs from the others in the ABI alone.

# Only defined on first load, so `load`-ing a config twice in the same
# process does not warn about constant redefinition.
unless defined?(BeniBuildConfig)
  # Config-time constants shared across the repo's build configs.
  module BeniBuildConfig
    # ABI-bearing defines the validation build applies to BOTH its host
    # and wasi targets, keeping `mrb_int` width and float boxing
    # identical across them (without MRB_INT32 a 64-bit host defaults to
    # MRB_INT64 while wasm32 stays 32-bit — see mruby's mrbconf.h). The
    # beni crates align themselves automatically: their build script
    # parses the `libmruby.flags.mak` sidecar each build leaves next to
    # the archive, so edits here flow into bindgen without code changes.
    ABI_DEFINES = %w[
      MRB_INT32
      MRB_WORDBOX_NO_INLINE_FLOAT
    ].freeze

    # Core-gem baseline shared by every config: mruby-compiler (the
    # wrapper's `mrb_load_nstring` needs it) plus the portable core
    # extension gems. No I/O / network / process gems — those do not
    # exist on wasm32-wasip1, and keeping every target's gem set
    # identical keeps the verified surface identical.
    MRBGEM_BASELINE = %w[
      mruby-compiler
      mruby-array-ext
      mruby-enum-ext
      mruby-hash-ext
      mruby-numeric-ext
      mruby-object-ext
      mruby-proc-ext
      mruby-range-ext
      mruby-string-ext
      mruby-sprintf
      mruby-symbol-ext
      mruby-error
      mruby-metaprog
    ].freeze
  end
end
