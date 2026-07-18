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
#   $ rake api:coverage   — rewrite docs/api_coverage.md. Reads the
#                           generated bindings.rs when an archive is
#                           staged (run after `rake beni:build` for the
#                           exact sys surface), otherwise infers it.
#   $ rake api:priority       — print the top 20 not-yet-typed embedder
#   $ rake "api:priority[50]"   symbols ranked by mrbgems call frequency,
#                             the worklist for what to graduate next. The
#                             optional argument caps the rows; a query
#                             only, writes no file.

require_relative "support/beni_coverage"

namespace :api do
  desc "Regenerate docs/api_coverage.md (mruby C API ↔ Rust binding coverage)"
  task :coverage do
    report = BeniCoverage.generate
    puts "[api:coverage] wrote #{BeniCoverage::OUTPUT}"
    if report.unknown.any?
      abort "[api:coverage] manifest entries match no scanned symbol: #{report.unknown.sort.join(", ")}"
    end
  end

  desc "Rank not-yet-typed mruby C API by mrbgems usage (worklist; top N, default 20)"
  task :priority, [:top] do |_task, args|
    top = Integer(args.top || 20)
    rows = BeniCoverage.priority
    rows.first(top).each { |e| puts "#{e.uses.to_s.rjust(5)}  #{e.name.ljust(34)} #{e.header}" }
    puts "showing #{[top, rows.size].min} of #{rows.size} not-yet-typed symbols"
  end

  desc "Verify every get_args format marker is recorded in the coverage lens"
  task :formats do
    problems = BeniCoverage.formats_drift
    problems.each { |problem| puts "[api:formats] #{problem}" }
    abort "[api:formats] get_args format lens drift detected" unless problems.empty?

    puts "[api:formats] #{BeniCoverage.marker_specifiers.size} marker specifiers all recorded"
  end
end
