# frozen_string_literal: true

module BeniCoverage
  # Coverage the manifest does not state because the headers already do:
  # a macro defined as another symbol names the same capability, so
  # covering either covers both. Reads the relations +Surface+ derives
  # against the relations the manifest's notes claim, and reports where
  # the two disagree.
  module Aliases
    # How a Note spells out an alias relation, from either side of it.
    CLAIMED = /`(\w+)\([^)]*\)` is a `#define` alias of `(\w+)`/

    module_function

    # Symbols the manifest leaves silent that an alias covers anyway,
    # mapped to the partner covering them. The relation is symmetric: a
    # recorded macro covers the symbol it is defined as, and a recorded
    # symbol covers the macro defined as it.
    def equivalents(derived, typed)
      derived.each_with_object({}) do |(name, target), covered|
        covered[name] = target if typed.key?(target) && !typed.key?(name)
        covered[target] = name if typed.key?(name) && !typed.key?(target)
      end
    end

    # Notes claiming a `#define` alias the headers no longer support —
    # empty when every claim still holds. A stale claim reads as
    # coverage nothing derives.
    def drift(derived, typed)
      claims(typed).filter_map do |name, target|
        next if derived[name] == target

        "#{name} is recorded as a `#define` alias of #{target}, which the headers do not define"
      end
    end

    def claims(typed)
      typed.each_value.flat_map { |note| note.to_s.scan(CLAIMED) }
    end
  end
end
