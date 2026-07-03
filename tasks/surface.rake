# frozen_string_literal: true

require_relative "support/beni_surface"

# Public-surface drift gate: the placeholder contract holds only while
# the compile-surface test references every inherent pub fn, so the
# reference list is verified mechanically instead of by hand.
namespace :api do
  desc "Verify the compile-surface test references every inherent pub fn"
  task :surface do
    report = BeniSurface.verify
    report.missing.each { |entry| puts "[api:surface] missing reference: #{entry.ref}" }
    report.stale.each { |ref| puts "[api:surface] stale reference: #{ref}" }
    abort "[api:surface] surface drift detected" unless report.ok?

    puts "[api:surface] #{report.total} inherent pub fns all referenced"
  end
end
