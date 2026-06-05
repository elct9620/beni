# frozen_string_literal: true

require_relative "beni/version"

module Beni
  class Error < StandardError; end
end

require_relative "beni/vendor"
require_relative "beni/builder"
