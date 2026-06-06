# frozen_string_literal: true

module Beni
  module DSL
    # Vocabulary inside a +target <name> do … end+ block — block-less
    # toolchain references only. References are set-semantic: repeats
    # collapse, and referencing +mruby+ is legal redundancy.
    class TargetContext
      # Run +block+ on a fresh context and return the collected
      # reference names.
      def self.collect(&)
        context = new
        context.instance_exec(&)
        context.references
      end

      attr_reader :references

      def initialize
        @references = []
      end

      def toolchain(name, &block)
        if block
          raise Error,
                "`toolchain #{name.inspect}` inside a target block must not carry a block — " \
                "definitions live at the top level"
        end
        DSL.assert_known_toolchain!(name)

        @references << name unless @references.include?(name)
      end
    end
  end
end
