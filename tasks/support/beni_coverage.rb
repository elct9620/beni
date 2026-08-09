# frozen_string_literal: true

require "yaml"
require "fileutils"

require_relative "beni_coverage/surface"
require_relative "beni_coverage/aliases"
require_relative "beni_coverage/frequency"
require_relative "beni_coverage/report"

# mruby C API coverage support module
# ====================================
#
# Pure-Ruby helpers backing the +api:coverage+ rake task. Builds the
# +docs/api_coverage.md+ tracking index by joining four sources:
#
#   inventory — mruby's public embedder surface (+Surface+), scanned
#               from the vendored headers. The denominator; always
#               fresh, never hand-edited.
#   sys       — the raw +beni-sys+ FFI tier, derived automatically:
#               functions from bindgen's generated +bindings.rs+, macros
#               from the +wrapper.h+ static-inline shims (a macro is
#               bound iff a shim body names it). No build staged → the
#               function tier falls back to "all declared".
#   typed     — the safe +beni+ wrapper tier, read from the hand-authored
#               +.api_coverage.yml+ manifest. The Rust shape does not map
#               onto C names mechanically, so this tier is curated by
#               implementers, not inferred.
#   aliases   — symbols the headers make equivalent to a typed one
#               (+Aliases+), covered without being recorded. Derived, so
#               the manifest never spells out what a `#define` already
#               says.
#
# The +priority+ query is a separate concern from the report: it ranks
# the not-yet-typed surface by the call frequency of the mrbgems this
# repo builds (+Frequency+), to point graduation work at the symbols
# embedders lean on hardest.
module BeniCoverage
  ROOT = File.expand_path("../..", __dir__)
  INCLUDE_ROOT = File.join(ROOT, "vendor", "mruby", "include")
  WRAPPER_H = File.join(ROOT, "crates", "beni-sys", "src", "wrapper.h")
  MANIFEST = File.join(ROOT, ".api_coverage.yml")
  OUTPUT = File.join(ROOT, "docs", "api_coverage.md")
  VERSION_H = File.join(INCLUDE_ROOT, "mruby", "version.h")
  ARGS_RS = File.join(ROOT, "crates", "beni", "src", "state", "args.rs")

  # One not-yet-typed symbol worth graduating, with its downstream weight.
  Priority = Data.define(:name, :uses, :header)

  # The coverage the manifest cannot state, because both sides are read
  # rather than authored: the raw FFI tier, and the symbols an alias
  # covers through a recorded partner.
  Derived = Data.define(:sys, :equivalents)

  module_function

  # Scan the inventory, resolve the two coverage tiers, write the report.
  # Returns the report so the rake task can fail on manifest entries
  # that match no scanned symbol.
  def generate
    surface = Surface.parse(INCLUDE_ROOT)
    report = build_report(surface)
    FileUtils.mkdir_p(File.dirname(OUTPUT))
    File.write(OUTPUT, report.to_md)
    report
  end

  # Every embedder symbol the typed tier has not graduated yet, ranked by
  # how often mrbgems call them — the complete worklist for what to bind
  # next. Unused symbols (count 0) stay in, sorted last: no mrbgem reaches
  # them, but the API still exists and graduation is not yet complete.
  def priority
    surface = Surface.parse(INCLUDE_ROOT)
    typed = typed_notes
    rank(surface, typed.merge(equivalents(typed)), Frequency.scan(ROOT, surface.map(&:name)))
  end

  # Which gem sources the ranking counted, so a reader knows how far to
  # trust the order — the +priority+ counterpart of the report's
  # linked-versus-heuristic sys detection.
  def demand_signal
    Frequency.signal(ROOT)
  end

  # The hand-authored typed tier, keyed by C symbol.
  def typed_notes
    load_manifest["typed"] || {}
  end

  # Symbols an alias covers on a recorded partner's behalf — the tier no
  # one writes down.
  def equivalents(typed)
    Aliases.equivalents(Surface.aliases(INCLUDE_ROOT), typed)
  end

  # Manifest notes whose claimed alias relation the headers no longer
  # support — the gate `api:aliases` gives the derived tier.
  def alias_drift
    Aliases.drift(Surface.aliases(INCLUDE_ROOT), typed_notes)
  end

  def alias_claims_count
    Aliases.claims(typed_notes).size
  end

  def rank(surface, typed, uses)
    surface.reject { |e| typed.key?(e.name) }
           .map { |e| Priority.new(name: e.name, uses: uses[e.name], header: e.header) }
           .sort_by { |e| [-e.uses, e.name] }
  end

  def build_report(surface)
    bindings = bindings_files
    manifest = load_manifest
    derived = Derived.new(
      sys: sys_covered(surface, bindings),
      equivalents: equivalents(manifest["typed"] || {})
    )
    Report.new(surface:, manifest:, derived:, version: mruby_version, linked: !bindings.empty?)
  end

  # Names reachable through the raw FFI: bound functions plus shimmed
  # macros.
  def sys_covered(surface, bindings)
    sys_functions(surface, bindings) + sys_macros(surface)
  end

  # Functions bindgen emitted into bindings.rs. With no archive staged
  # the file is absent, so fall back to every declared function (bindgen
  # binds nearly all MRB_API — an approximation the report flags as
  # heuristic).
  def sys_functions(surface, bindings)
    return surface.select { |e| e.kind == :function }.map(&:name) if bindings.empty?

    bindings.flat_map { |f| File.read(f).scan(/\bpub fn\s+(mrb_[a-z0-9_]+)/).flatten }.uniq
  end

  # Macros a wrapper.h static-inline shim binds — detected by the macro's
  # name appearing in the shim source (comments stripped so a mention in
  # prose does not count).
  def sys_macros(surface)
    names = wrapper_identifiers
    surface.select { |e| e.kind == :macro && names.include?(e.name) }.map(&:name)
  end

  def wrapper_identifiers
    src = File.read(WRAPPER_H).gsub(%r{/\*.*?\*/}m, "").gsub(%r{//.*$}, "")
    src.scan(/\b[A-Za-z_]\w*\b/).uniq
  end

  def load_manifest
    return {} unless File.exist?(MANIFEST)

    YAML.load_file(MANIFEST) || {}
  end

  def bindings_files
    Dir.glob(File.join(ROOT, "target", "**", "beni-sys-*", "out", "bindings.rs"))
  end

  def mruby_version
    src = File.read(VERSION_H)
    %w[MAJOR MINOR TEENY].map { |part| src[/MRUBY_RELEASE_#{part}\s+(\d+)/, 1] }.join(".")
  end

  # Specifier chars every +format::+ marker reads, scanned from the +FMT+
  # constants in the args module. The lens gate's numerator is derived from
  # Rust source, so a new marker cannot silently escape the coverage lens.
  def marker_specifiers
    File.readlines(ARGS_RS)
        .reject { |line| line.strip.start_with?("//") }
        .join
        .scan(/const\s+FMT\b[^=]*=\s*c"([^"]*)"/)
        .join.chars.uniq
  end

  # Problems between the marker FMT vocabulary and the +get_args_formats+
  # lens — empty when they agree. Every specifier a marker reads must be
  # recorded covered through a +format::+ surface.
  def formats_drift
    lens = load_manifest["get_args_formats"] || {}
    marker_specifiers.filter_map { |ch| format_lens_problem(ch, lens[ch]) }
  end

  def format_lens_problem(char, entry)
    prefix = "specifier #{char.inspect} is read by a format marker but"
    return "#{prefix} absent from get_args_formats" if entry.nil?
    return "#{prefix} not marked covered" if entry["status"] != "covered"
    return "#{prefix} its via names no format:: surface" unless entry["via"].to_s.include?("format::")

    nil
  end
end
