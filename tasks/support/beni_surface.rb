# frozen_string_literal: true

# Public-surface drift gate support
# =================================
#
# Backs the +api:surface+ rake task: scans the +beni+ crate for the
# inherent +pub fn+ surface and verifies the
# +full_api_surface_compiles_in_both_modes+ test references every
# entry, keeping the placeholder contract honest — each public item
# exists whether or not mruby is linked, and only a reference breaks
# compilation when an item is cfg-gated out of one mode.
#
# Scope: inherent +pub fn+s declared in column-zero +impl+ blocks.
# Trait items stay out — the compiler type-checks trait declarations
# and default bodies in both modes without a reference. A fn carrying
# a +#[cfg]+ attribute is a deliberate single-mode item and is
# excluded from the expectation.
module BeniSurface
  ROOT = File.expand_path("../..", __dir__)
  CRATE_SRC = File.join(ROOT, "crates", "beni", "src")
  SURFACE_TEST_FILE = File.join(CRATE_SRC, "lib.rs")
  SURFACE_TEST = "full_api_surface_compiles_in_both_modes"

  INHERENT_IMPL = /\Aimpl(?:<[^>]*>)?\s+(?<type>[A-Z]\w*)(?:<[^>]*>)?\s*(?:where[^{]*)?\{/
  PUB_FN = /\A\s*pub\s+(?:unsafe\s+)?(?:const\s+)?fn\s+(?<name>[a-z_]\w*)/
  ATTRIBUTE = /\A\s*#\[/
  CFG_ATTRIBUTE = /\A\s*#\[cfg[( ]/

  # One inherent public method path, e.g. +Value::cv_get+.
  Entry = Data.define(:type, :name) do
    def ref
      "#{type}::#{name}"
    end
  end

  # Comparison outcome between the scanned surface and the test body.
  Report = Data.define(:missing, :stale, :total) do
    def ok?
      missing.empty? && stale.empty?
    end
  end

  module_function

  def verify
    entries = surface
    body = test_body
    # A generic type instantiates between the segments
    # (+DataType::<u8>::new+), so the matcher tolerates one turbofish.
    missing = entries.reject { |e| body.match?(/\b#{e.type}::(?:<[^>]*>::)?#{e.name}\b/) }
    Report.new(missing: missing, stale: stale_refs(entries, body), total: entries.size)
  end

  # Every inherent pub fn across the crate sources, deduplicated and
  # ordered for stable diagnostics.
  def surface
    Dir.glob(File.join(CRATE_SRC, "**", "*.rs"))
       .flat_map { |file| scan(File.read(file)) }
       .uniq
       .sort_by(&:ref)
  end

  def scan(source)
    entries = []
    type = nil
    gated = false
    source.each_line(chomp: true) do |line|
      next type = impl_type(line) if type.nil?

      type, gated = scan_impl_line(entries, type, gated, line)
    end
    entries
  end

  # The inherent-impl type a column-zero +impl+ line opens; trait
  # impls (+impl X for Y+) stay out of scope.
  def impl_type(line)
    line.include?(" for ") ? nil : line[INHERENT_IMPL, :type]
  end

  # Advance the in-impl scanner by one line, collecting an entry when
  # an ungated pub fn appears and leaving the block at its
  # column-zero closing brace.
  def scan_impl_line(entries, type, gated, line)
    return [nil, false] if line == "}"
    return [type, gated || CFG_ATTRIBUTE.match?(line)] if ATTRIBUTE.match?(line)

    name = line[PUB_FN, :name]
    entries << Entry.new(type: type, name: name) if name && !gated
    [type, keeps_pending_attributes?(line) ? gated : false]
  end

  # Doc comments and blank lines sit between attributes and the fn
  # they gate; any other line consumes the pending attribute state.
  def keeps_pending_attributes?(line)
    stripped = line.strip
    stripped.empty? || stripped.start_with?("///", "//")
  end

  # The reference body of the surface test, bounded by the fn line
  # and its indentation-matched closing brace.
  def test_body
    lines = File.read(SURFACE_TEST_FILE).each_line.to_a
    start = lines.index { |l| l.include?("fn #{SURFACE_TEST}") }
    raise "#{SURFACE_TEST} not found in #{SURFACE_TEST_FILE}" unless start

    stop = (start...lines.size).find { |i| lines[i].rstrip == "    }" }
    lines[start..stop].join
  end

  # References naming a scanned type but no scanned fn — a rename or
  # removal the test has not caught up with.
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
