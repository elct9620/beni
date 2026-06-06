# frozen_string_literal: true

module Beni
  # A +target <name>+ declaration (SPEC.md Terminology: target
  # declaration) — names one build target to verify and holds the
  # toolchain references its block declared.
  class Target < Data.define(:name, :references)
  end
end
