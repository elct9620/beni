# frozen_string_literal: true

module BeniCoverage
  # mruby's public embedder API surface, parsed from the vendored
  # headers. Each +Entry+ is one callable symbol (function or
  # function-like macro) the typed wrapper could graduate.
  module Surface
    # Curated embedder headers — mirrors the include set of
    # +crates/beni-sys/src/wrapper.h+ plus the few public headers an
    # embedder reaches (compile/gc/numeric/range). Internal headers
    # (internal.h, opcode.h, khash.h, presym/...) are excluded: their
    # symbols are not API the typed surface graduates.
    HEADERS = %w[
      mruby.h
      mruby/array.h
      mruby/class.h
      mruby/compile.h
      mruby/data.h
      mruby/dump.h
      mruby/error.h
      mruby/gc.h
      mruby/hash.h
      mruby/irep.h
      mruby/numeric.h
      mruby/proc.h
      mruby/range.h
      mruby/string.h
      mruby/value.h
      mruby/variable.h
    ].freeze

    # Uppercase macros mruby documents for embedders. Function-like
    # macros named +mrb_*+ (lowercase) are always embedder API and are
    # admitted separately; every other uppercase macro is internal
    # layout (boxing, RArray flags, opcodes) and stays out of the
    # denominator.
    MACRO_ALLOW = [
      /\AMRB_ARGS_/,
      /\ARSTRING_/,
      /\ARARRAY_/,
      /\ADATA_/,
      /\AMRB_SET_INSTANCE_TT\z/
    ].freeze

    # +mrb_*+ macros that look like embedder API by name but are not:
    # debug and compile-time assertions, and the internal integer hash
    # helper. No embedder calls these, so they stay out of the
    # denominator (see SPEC's coverage measure).
    MACRO_DENY = [
      /\Amrb_assert/,
      /\Amrb_static_assert/,
      /\Amrb_int_hash_func\z/
    ].freeze

    Entry = Data.define(:name, :kind, :header)

    module_function

    def parse(include_root)
      HEADERS.flat_map { |rel| parse_header(include_root, rel) }.uniq(&:name)
    end

    def parse_header(include_root, rel)
      path = File.join(include_root, rel)
      return [] unless File.exist?(path)

      src = File.read(path)
      functions(src, rel) + macros(src, rel)
    end

    def functions(src, header)
      src.scan(/\bMRB_(?:API|INLINE)\b[^;{]*?\b(mrb_[a-z0-9_]+)\s*\(/m)
         .flatten.uniq.map { |name| Entry.new(name:, kind: :function, header:) }
    end

    def macros(src, header)
      src.scan(/^[ \t]*#\s*define\s+([A-Za-z_]\w*)\(/)
         .flatten.select { |name| embedder_macro?(name) }
                 .uniq.map { |name| Entry.new(name:, kind: :macro, header:) }
    end

    def embedder_macro?(name)
      return false if MACRO_DENY.any? { |re| re.match?(name) }

      name.start_with?("mrb_") || MACRO_ALLOW.any? { |re| re.match?(name) }
    end
  end
end
