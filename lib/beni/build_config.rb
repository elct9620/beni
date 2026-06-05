# frozen_string_literal: true

require "fileutils"

module Beni
  # Generates a customizable mruby build config from the gem-shipped
  # self-contained template (host + wasm32-wasip1 starting point).
  # Exposed as +rake beni:config+ by +Beni::Tasks+; the generated file
  # belongs to the consuming project and is theirs to edit.
  #
  # The template lives under lib/ so every runtime asset travels with
  # the code path that loads it — the packaged gem cannot drift from
  # the checkout.
  module BuildConfig
    # `__dir__ || "."`: __dir__ is only nil under eval, which never
    # loads this file; the fallback exists to satisfy steep's String?
    # typing.
    TEMPLATE = File.expand_path("templates/build_config.rb", __dir__ || ".")

    module_function

    # Copy the template to +dest+, refusing to clobber an existing
    # (likely hand-tuned) config. Returns +dest+.
    def generate(dest)
      raise Error, "#{dest} already exists — delete it first to regenerate" if File.exist?(dest)

      FileUtils.mkdir_p(File.dirname(dest))
      FileUtils.cp(TEMPLATE, dest)
      dest
    end
  end
end
