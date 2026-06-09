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

require_relative "support/beni_coverage"

namespace :api do
  desc "Regenerate docs/api_coverage.md (mruby C API ↔ Rust binding coverage)"
  task :coverage do
    path = BeniCoverage.generate
    puts "[api:coverage] wrote #{path}"
  end
end
