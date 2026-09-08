# frozen_string_literal: true

require_relative "beni_surface/syntax"
require_relative "beni_surface/scan"
require_relative "beni_surface/modules"
require_relative "beni_surface/net"

# Public-surface drift gate support
# =================================
#
# Backs the +api:surface+ rake task: scans the +beni+ crate for the
# inherent +pub fn+ surface and verifies the drift net in +beni-tests+
# names every entry. The net lives in consumer position, so a
# reference there also carries the root re-exports its path runs
# through; this scan is what keeps the net and the surface from
# drifting apart as items are added or removed.
#
# Scope: inherent +pub fn+s declared in column-zero +impl+ blocks.
# Trait items stay out — the compiler type-checks trait declarations
# and default bodies without a reference.
#
# A capability feature is an axis of the expectation rather than an
# exemption from it: an item a feature carries is expected in the net
# body gated on that feature, so enabling the feature has a net of its
# own and disabling it leaves the ungated net whole. A fn carrying some
# other +#[cfg]+ is build-specific and stays out of the expectation.
module BeniSurface
  ROOT = File.expand_path("../..", __dir__)
  CRATE_SRC = File.join(ROOT, "crates", "beni", "src")
  SURFACE_TEST_FILE = File.join(ROOT, "crates", "beni-tests", "src", "surface_test.rs")

  # Comparison outcome between the scanned surface and the net bodies.
  Report = Data.define(:missing, :stale, :total) do
    def ok?
      missing.empty? && stale.empty?
    end
  end

  module_function

  def verify(crate_src: CRATE_SRC, net_file: SURFACE_TEST_FILE)
    entries = surface(crate_src)
    bodies = Net.call(net_file)
    missing = entries.reject { |entry| referenced?(bodies[entry.feature], entry) }
    Report.new(missing: missing, stale: stale_refs(entries, bodies.values.join), total: entries.size)
  end

  # Whether one net body names the entry. A generic type instantiates
  # between the segments (+DataType::<u8>::new+), so the matcher
  # tolerates one turbofish.
  def referenced?(body, entry)
    return false unless body

    body.match?(/\b#{entry.type}::(?:<[^>]*>::)?#{entry.name}\b/)
  end

  # Every inherent pub fn across the crate sources, deduplicated and
  # ordered for stable diagnostics.
  def surface(crate_src = CRATE_SRC)
    features = Modules.call(crate_src)
    Dir.glob(File.join(crate_src, "**", "*.rs"))
       .flat_map { |file| Scan.call(File.read(file), features[file]) }
       .uniq
       .sort_by { |entry| [entry.feature.to_s, entry.ref] }
  end

  # References naming a scanned type but no scanned fn — a rename or
  # removal the net has not caught up with.
  def stale_refs(entries, body)
    known_types = entries.map(&:type).uniq
    known_refs = entries.map(&:ref)
    body.scan(/\b([A-Z]\w*)::([a-z_]\w*)\b/)
        .uniq
        .map { |type, name| "#{type}::#{name}" }
        .select { |ref| known_types.include?(ref.split("::").first) }
        .reject { |ref| known_refs.include?(ref) }
        .sort
  end
end
