# frozen_string_literal: true

require "yaml"
require "fileutils"

require_relative "beni_coverage/surface"
require_relative "beni_coverage/report"

# mruby C API coverage support module
# ====================================
#
# Pure-Ruby helpers backing the +api:coverage+ rake task. Builds the
# +docs/api_coverage.md+ tracking index by joining three sources:
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
module BeniCoverage
  ROOT = File.expand_path("../..", __dir__)
  INCLUDE_ROOT = File.join(ROOT, "vendor", "mruby", "include")
  WRAPPER_H = File.join(ROOT, "crates", "beni-sys", "src", "wrapper.h")
  MANIFEST = File.join(ROOT, ".api_coverage.yml")
  OUTPUT = File.join(ROOT, "docs", "api_coverage.md")
  VERSION_H = File.join(INCLUDE_ROOT, "mruby", "version.h")

  module_function

  # Scan the inventory, resolve the two coverage tiers, write the report.
  # Returns the output path so the rake task can echo it.
  def generate
    surface = Surface.parse(INCLUDE_ROOT)
    FileUtils.mkdir_p(File.dirname(OUTPUT))
    File.write(OUTPUT, build_report(surface).to_md)
    OUTPUT
  end

  def build_report(surface)
    bindings = bindings_files
    Report.new(
      surface:, sys: sys_covered(surface, bindings), manifest: load_manifest,
      version: mruby_version, linked: !bindings.empty?
    )
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
end
