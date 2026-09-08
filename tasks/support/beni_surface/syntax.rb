# frozen_string_literal: true

module BeniSurface
  # The Rust source shapes the surface scanners read. Every construct
  # the gate cares about opens its own line, so matching lines is
  # enough and no parse is needed.
  module Syntax
    INHERENT_IMPL = /\Aimpl(?:<[^>]*>)?\s+(?<type>[A-Z]\w*)(?:<[^>]*>)?\s*(?:where[^{]*)?\{/
    PUB_FN = /\A\s*pub\s+(?:unsafe\s+)?(?:const\s+)?fn\s+(?<name>[a-z_]\w*)/
    FN = /\A\s*fn\s+(?<name>\w+)\s*\(/
    MOD_DECL = /\A\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(?<name>\w+)\s*;/
    INLINE_MOD = /\A\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(?<name>\w+)\s*\{/
    ATTRIBUTE = /\A\s*#\[/
    CFG_ATTRIBUTE = /\A\s*#\[cfg[( ]/
    CFG_FEATURE = /\A\s*#\[cfg\(feature\s*=\s*"(?<feature>[\w-]+)"\)\]/

    module_function

    # The inherent-impl type a column-zero +impl+ line opens; trait
    # impls (+impl X for Y+) stay out of scope, since the compiler type
    # checks trait declarations and default bodies without a reference.
    def impl_type(line)
      return nil if line.include?(" for ")

      line[INHERENT_IMPL, :type]
    end

    # Doc comments and blank lines sit between attributes and the item
    # they gate; any other line consumes the pending attribute state.
    def pending_attributes?(line)
      stripped = line.strip
      stripped.empty? || stripped.start_with?("///", "//")
    end
  end
end
