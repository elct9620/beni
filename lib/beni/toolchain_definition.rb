# frozen_string_literal: true

module Beni
  # A top-level +toolchain <name>+ block (SPEC.md Terminology: toolchain
  # definition) — carries the +version+ and +sha256+ pair that replaces
  # the named toolchain's built-in pair.
  class ToolchainDefinition < Data.define(:name, :version, :sha256)
  end
end
