# frozen_string_literal: true

require_relative "beni/version"

module Beni
  class Error < StandardError; end
end

require_relative "beni/vendor"
require_relative "beni/builder"
require_relative "beni/build_config"
require_relative "beni/target"
require_relative "beni/toolchain_definition"
require_relative "beni/selected_toolchain"
require_relative "beni/configuration"
require_relative "beni/dsl"
