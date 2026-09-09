# frozen_string_literal: true

require_relative "syntax"

module BeniSurface
  # The attribute state pending for the next item: the capability
  # feature gating it, and whether some other +#[cfg]+ makes it
  # build-specific and so no part of the expectation. Attributes
  # accumulate onto the item that follows, and any line that is neither
  # an attribute nor a doc comment ends the accumulation — which is one
  # rule, so the three scanners read it from here rather than each
  # deciding for themselves what a pending attribute is.
  Gate = Data.define(:feature, :excluded) do
    def self.at(feature)
      new(feature: feature, excluded: false)
    end

    # The gate one attribute line leaves pending. A feature gate names
    # the feature; any other +#[cfg]+ marks the item build-specific;
    # every other attribute leaves the gate as it stands.
    def read(line)
      name = line[Syntax::CFG_FEATURE, :feature]
      return with(feature: name) if name
      return with(excluded: true) if Syntax::CFG_ATTRIBUTE.match?(line)

      self
    end

    # The gate after +line+, given what it resets to when the line ends
    # the accumulation.
    def after(line, reset)
      return read(line) if Syntax::ATTRIBUTE.match?(line)
      return self if Syntax.pending_attributes?(line)

      reset
    end
  end
end
