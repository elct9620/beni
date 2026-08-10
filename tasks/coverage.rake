# frozen_string_literal: true

# mruby C API coverage tracking
# =============================
#
# Regenerates +docs/api_coverage.md+, the index of which mruby public
# embedder symbols the Rust layers bind. "API coverage" — the binding
# gap between mruby's C surface and the typed +beni+ wrapper — not test
# line coverage. The .rake wrapper is the rake DSL surface; the parsing
# and rendering live in +tasks/support/beni_coverage.rb+.
#
#   $ rake api:aliases    — hold every alias relation the manifest's
#                           notes claim to what the headers define.
#                           Reads the vendored headers, so it runs
#                           where they are staged: as api:coverage's
#                           prerequisite, not in the default task.
#
#   $ rake api:coverage   — rewrite docs/api_coverage.md. Reads the
#                           generated bindings.rs when an archive is
#                           staged (run after `rake beni:build` for the
#                           exact sys surface), otherwise infers it.
#   $ rake api:priority       — print the top 20 not-yet-typed embedder
#   $ rake "api:priority[50]"   symbols ranked by how often the mrbgems
#                             this repo builds call them, the worklist
#                             for what to graduate next. The optional
#                             argument caps the rows; a query only,
#                             writes no file.

require_relative "support/beni_coverage"

# The gate the derived alias tier needs, held to the vendored headers —
# so it runs where they are staged, ahead of the report that derives
# from them.
namespace :api do
  desc "Verify every recorded #define alias still matches the vendored headers"
  task :aliases do
    abort "[api:aliases] vendored headers absent; run rake beni:vendor:setup first" unless BeniCoverage.headers?

    problems = BeniCoverage.alias_drift
    problems.each { |problem| puts "[api:aliases] #{problem}" }
    abort "[api:aliases] recorded alias relations drifted from the headers" unless problems.empty?

    puts "[api:aliases] #{BeniCoverage.alias_claims_count} recorded alias relations all hold"
  end
end

namespace :api do
  desc "Regenerate docs/api_coverage.md (mruby C API ↔ Rust binding coverage)"
  task coverage: :aliases do
    coverage = BeniCoverage.generate.coverage
    puts "[api:coverage] wrote #{BeniCoverage::OUTPUT}"
    if coverage.conflicting.any?
      abort "[api:coverage] symbols claimed by more than one section: #{coverage.conflicting.join(", ")}"
    end
    if coverage.unexplained.any?
      abort "[api:coverage] taken out of the measure with no reason: #{coverage.unexplained.join(", ")}"
    end
    if coverage.unknown.any?
      abort "[api:coverage] manifest entries match no scanned symbol: #{coverage.unknown.sort.join(", ")}"
    end
  end

  desc "Rank not-yet-typed mruby C API by usage in the mrbgems this repo builds (worklist; top N, default 20)"
  task :priority, [:top] do |_task, args|
    top = Integer(args.top || 20)
    rows = BeniCoverage.priority
    rows.first(top).each { |e| puts "#{e.uses.to_s.rjust(5)}  #{e.name.ljust(34)} #{e.header}" }
    puts "showing #{[top, rows.size].min} of #{rows.size} not-yet-typed symbols"
    puts "demand: #{BeniCoverage.demand_signal}"
  end
end

# The gate the default task runs: the record held to the repo's own
# sources, so it holds anywhere the checkout does — no vendored
# toolchain, no network.
namespace :api do
  desc "Verify every get_args format marker is recorded in the coverage lens"
  task :formats do
    problems = BeniCoverage.formats_drift
    problems.each { |problem| puts "[api:formats] #{problem}" }
    abort "[api:formats] get_args format lens drift detected" unless problems.empty?

    puts "[api:formats] #{BeniCoverage.marker_specifiers.size} marker specifiers all recorded"
  end
end
