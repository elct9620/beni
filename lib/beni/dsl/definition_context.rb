# frozen_string_literal: true

module Beni
  module DSL
    # Vocabulary inside a top-level +toolchain <name> do … end+ block —
    # the +(version, sha256)+ pair and nothing else. Both fields are
    # required exactly once; the check runs when the block returns.
    class DefinitionContext
      # Run +block+ on a fresh context and return the collected
      # ToolchainDefinition.
      def self.collect(name, &)
        context = new(name)
        context.instance_exec(&)
        context.to_definition
      end

      def initialize(name)
        @name = name
        @version = nil
        @sha256 = nil
      end

      def version(value)
        raise Error, "duplicate `version` in toolchain #{@name.inspect} definition" if @version

        @version = value
      end

      def sha256(value)
        raise Error, "duplicate `sha256` in toolchain #{@name.inspect} definition" if @sha256

        @sha256 = value
      end

      # The collected definition; raises when the block left +version+
      # or +sha256+ undeclared.
      def to_definition
        version = @version
        sha256 = @sha256
        if version.nil? || sha256.nil?
          missing = [("version" if version.nil?), ("sha256" if sha256.nil?)].compact
          raise Error, "toolchain #{@name.inspect} definition missing #{missing.join(" and ")}"
        end

        ToolchainDefinition.new(name: @name, version: version, sha256: sha256)
      end
    end
  end
end
