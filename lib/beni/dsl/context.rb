# frozen_string_literal: true

module Beni
  module DSL
    # Top-level vocabulary of the +Beni::Tasks.new+ block. Collects the
    # scalar settings, target declarations, and toolchain definitions —
    # validating each declaration eagerly — and resolves them into the
    # +Configuration+ the task-definition phase consumes.
    class Context
      def initialize
        @settings = {}
        @targets = {}
        @definitions = {}
      end

      def version(value)
        declare_scalar(:version, value)
      end

      def build_config(path)
        declare_scalar(:build_config, path)
      end

      def vendor_dir(path)
        declare_scalar(:vendor_dir, path)
      end

      # A +target <name>+ declaration; the optional block holds the
      # target's toolchain references.
      def target(name, &block)
        key = name.to_s
        raise Error, "duplicate `target` declaration #{key.inspect}" if @targets.key?(key)

        # @type var references: Array[String]
        references = block ? TargetContext.collect(&block) : []
        @targets[key] = Target.new(name: key, references: references)
      end

      # A top-level +toolchain <name>+ — always a definition, so the
      # block is part of the grammar and +mruby+ is never definable.
      def toolchain(name, &block)
        key = name.to_s
        raise Error, "top-level `toolchain #{key.inspect}` must carry a definition block" unless block
        if key == "mruby"
          raise Error, "a toolchain definition never names \"mruby\" — select it with the `version` setting"
        end

        DSL.assert_known_toolchain!(key)
        raise Error, "duplicate toolchain definition #{key.inspect}" if @definitions.key?(key)

        @definitions[key] = DefinitionContext.collect(key, &block)
      end

      # Resolve the collected declarations (SPEC.md Behaviors: selection
      # is reference-driven; defaults fall to the built-in pairs).
      def configuration
        Configuration.new(
          vendor_dir: resolved_vendor_dir,
          build_config: resolved_build_config,
          targets: resolved_targets,
          toolchains: selected_names.map { |name| selected_toolchain(name) }
        )
      end

      private

      def declare_scalar(key, value)
        raise Error, "duplicate `#{key}` declaration" if @settings.key?(key)

        @settings[key] = value
      end

      def resolved_vendor_dir
        File.expand_path(@settings[:vendor_dir] || ENV.fetch("BENI_VENDOR_DIR", nil) || "vendor")
      end

      def resolved_build_config
        path = @settings[:build_config]
        path && File.expand_path(path)
      end

      def resolved_targets
        return Builder::DEFAULT_TARGETS.dup if @targets.empty?

        @targets.keys
      end

      # References plus their transitive dependencies; +mruby+ is always
      # selected and leads the set so it stages first.
      def selected_names
        references = @targets.values.flat_map(&:references)
        dependencies = references.flat_map { |name| Vendor::DEPENDENCIES.fetch(name, []) }
        (%w[mruby] + references + dependencies).uniq
      end

      def selected_toolchain(name)
        definition = @definitions[name]
        return SelectedToolchain.new(name: name, version: definition.version, sha256: definition.sha256) if definition

        version = name == "mruby" ? mruby_version : Vendor::BUILT_IN_PAIRS.fetch(name).fetch(:version)
        SelectedToolchain.new(name: name, version: version, sha256: Vendor.built_in_sha256(name, version))
      end

      def mruby_version
        @settings[:version] || Vendor::BUILT_IN_PAIRS.fetch("mruby").fetch(:version)
      end
    end
  end
end
