# frozen_string_literal: true

require_relative "syntax"

module BeniSurface
  # Joins the axes of one gate key; neither a feature nor a cfg name
  # holds it.
  AXIS_SEPARATOR = "+"

  # The attribute state pending for the next item: the capability
  # features and archive cfgs gating it, and whether some other +#[cfg]+
  # makes it build-specific and so no part of the expectation. Attributes
  # accumulate onto the item that follows, and any line that is neither
  # an attribute nor a doc comment ends the accumulation — which is one
  # rule, so the three scanners read it from here rather than each
  # deciding for themselves what a pending attribute is.
  Gate = Data.define(:feature, :excluded) do
    def self.at(feature)
      new(feature: feature, excluded: false)
    end

    # The gate key for an item behind both +outer+ and +inner+: every
    # axis either names, so an item an impl and its own fn each gate is
    # expected in a net body gated on both rather than on the inner one.
    def self.combine(outer, inner)
      names = [outer, inner].compact.flat_map { |key| key.split(AXIS_SEPARATOR) }.uniq.sort
      names.join(AXIS_SEPARATOR) unless names.empty?
    end

    # The gate one attribute line leaves pending. A feature gate names
    # the feature and an archive cfg the cfg; any other +#[cfg]+ marks
    # the item build-specific; every other attribute leaves the gate as
    # it stands.
    def read(line)
      name = line[Syntax::CFG_FEATURE, :feature] || line[Syntax::CFG_ARCHIVE, :cfg]
      return with(feature: Gate.combine(feature, name)) if name
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
