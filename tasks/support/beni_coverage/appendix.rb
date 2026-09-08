# frozen_string_literal: true

module BeniCoverage
  # The report sections that sit beside the ratio rather than inside it:
  # the +mrb_get_args+ specifier lens, the symbols admitted from a
  # library-internal header, the Rust-native extensions, and the manifest
  # entries no scanned symbol matches. Each renders from a manifest
  # section that is not keyed by the scanned inventory, which is what
  # keeps them out of +Report+'s per-header tables.
  module Appendix
    private

    def formats_block
      return nil if @formats.empty?

      rows = @formats.map { |spec, entry| formats_row(spec, entry) }
      <<~MD.chomp
        ## get_args format specifiers

        `mrb_get_args`' format string is a specifier vocabulary — one symbol,
        many capabilities — measured as its own lens. Every specifier is
        covered (✅); the Via column names the surface that covers each one.

        | Specifier | Covered | Via |
        |-----------|:-------:|-----|
        #{rows.join("\n")}
      MD
    end

    # A literal `|` specifier is escaped so it does not close the table cell.
    def formats_row(spec, entry)
      cell = spec == "|" ? "\\|" : spec
      "| `#{cell}` | ✅ | #{entry["via"]} |"
    end

    def admitted_block
      return nil if @admitted.empty?

      rows = @admitted.sort.map { |name, note| "| `#{name}` | #{note.to_s.strip} |" }
      <<~MD.chomp
        ## Admitted internal symbols

        Declared in a header mruby marks internal to the library, so outside the
        embedder API the ratio measures. Each names the typed item it was
        admitted for, which is the only thing that admits one.

        | Symbol | Admitted for |
        |--------|--------------|
        #{rows.join("\n")}
      MD
    end

    def extensions_block
      return nil if @extensions.empty?

      rows = @extensions.sort.map { |item, desc| "| `#{item}` | #{desc.to_s.strip} |" }
      <<~MD.chomp
        ## Rust extensions

        Rust-native surface with no 1:1 mruby C API — not part of the ratio.

        | Item | Description |
        |------|-------------|
        #{rows.join("\n")}
      MD
    end

    def unknown_block
      return nil if @coverage.unknown.empty?

      rows = @coverage.unknown.sort.map { |name| "- `#{name}`" }
      <<~MD.chomp
        ## Unknown manifest entries

        Listed in `.api_coverage.yml` but absent from the scanned headers
        (renamed/removed upstream, or a typo):

        #{rows.join("\n")}
      MD
    end
  end
end
