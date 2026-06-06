# frozen_string_literal: true

module Beni
  # Declarative configuration vocabulary for +Beni::Tasks+. Three
  # contexts each expose exactly the declarations legal at their
  # position — top level (+Context+), inside a target block
  # (+TargetContext+), and inside a toolchain definition block
  # (+DefinitionContext+) — so a malformed declaration fails in the
  # method itself, at definition time.
  module DSL
    module_function

    # A toolchain name outside the Vendor registry fails at the
    # declaration that names it, never mid-build.
    def assert_known_toolchain!(name)
      return if Vendor::TOOLCHAIN_FACTORIES.key?(name)

      raise Error, "unknown toolchain #{name.inspect} (known: #{Vendor::TOOLCHAIN_FACTORIES.keys.join(", ")})"
    end
  end
end

require_relative "dsl/context"
require_relative "dsl/target_context"
require_relative "dsl/definition_context"
