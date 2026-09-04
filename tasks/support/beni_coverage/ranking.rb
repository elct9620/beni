# frozen_string_literal: true

module BeniCoverage
  # Orders the symbols still owed into a worklist — a separate concern
  # from the coverage report, which answers what is covered rather than
  # what to graduate next.
  #
  # The order comes from the two downstream populations +Frequency+
  # counts, and it takes the Rust consumers first: they are who beni
  # exists for, while an mrbgem runs inside the VM and never reaches for
  # the API an embedder holding values across a host boundary needs. A
  # symbol no consumer reaches keeps its place at the tail, where a zero
  # means "nobody in reach calls it", not "nobody needs it".
  module Ranking
    # One not-yet-typed symbol worth graduating, with its weight from
    # each population. The two counts stay apart: a Rust consumer's call
    # and an mrbgem's are not the same evidence of demand.
    Entry = Data.define(:name, :uses, :rust_uses, :header)

    module_function

    def build(surface, coverage, uses, rust_uses)
      surface.reject { |e| coverage.typed?(e.name) || coverage.exclusion(e.name) }
             .map { |e| entry_for(e, uses, rust_uses) }
             .sort_by { |e| [-e.rust_uses, -e.uses, e.name] }
    end

    def entry_for(symbol, uses, rust_uses)
      Entry.new(
        name: symbol.name, uses: uses[symbol.name],
        rust_uses: rust_uses[symbol.name], header: symbol.header
      )
    end
  end
end
