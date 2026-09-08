# frozen_string_literal: true

require_relative "support/beni_surface"

# Public-surface drift gate: the net in beni-tests only catches a
# removal it names, so the reference list is checked against the
# crate's own surface mechanically instead of by hand.
namespace :api do
  desc "Verify the compile-surface test references every inherent pub fn"
  task :surface do
    report = BeniSurface.verify
    report.missing.each { |entry| puts "[api:surface] missing reference: #{entry.label}" }
    report.stale.each { |ref| puts "[api:surface] stale reference: #{ref}" }
    abort "[api:surface] surface drift detected" unless report.ok?

    puts "[api:surface] #{report.total} inherent pub fns all referenced"
  end
end
