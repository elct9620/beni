# frozen_string_literal: true

require_relative "beni/version"

module Beni
  class Error < StandardError; end

  # Directory holding the gem-shipped mruby build configs: the default
  # +beni.rb+ and the reusable +wasi_toolchain.rb+ template. Exported to
  # build-config subprocesses as +BENI_BUILD_CONFIG_DIR+ so user-supplied
  # configs can +load+ the templates without knowing the gem's install
  # path.
  # `__dir__ || "."`: __dir__ is only nil under eval, which never loads
  # this file; the fallback exists to satisfy steep's String? typing.
  BUILD_CONFIG_DIR = File.expand_path("../build_config", __dir__ || ".")
end

require_relative "beni/vendor"
require_relative "beni/builder"
