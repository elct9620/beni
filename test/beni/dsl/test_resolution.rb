# frozen_string_literal: true

require "test_helper"
require "beni"

module Beni
  module DSL
    # Reference-driven toolchain selection and version pairing — the
    # Configuration the task-definition phase consumes.
    class TestResolution < Minitest::Test
      def test_zero_declarations_resolve_to_the_host_target_and_mruby_built_in_pair
        configuration = configure {} # rubocop:disable Lint/EmptyBlock
        pair = Vendor::BUILT_IN_PAIRS.fetch("mruby")
        mruby = configuration.toolchains.fetch(0)

        assert_equal %w[host], configuration.targets
        assert_equal %w[mruby], configuration.toolchains.map(&:name)
        assert_equal pair.fetch(:version), mruby.version
        assert_equal pair.fetch(:sha256), mruby.sha256
      end

      def test_a_toolchain_reference_selects_the_toolchain_with_transitive_mruby
        configuration = configure do
          target(:wasi) { toolchain "wasi-sdk" }
        end

        assert_equal %w[mruby wasi-sdk], configuration.toolchains.map(&:name).sort
      end

      def test_wasi_sdk_selection_defaults_to_the_built_in_pair_of_the_build_platform
        configuration = configure do
          target(:wasi) { toolchain "wasi-sdk" }
        end
        pair = Vendor::BUILT_IN_PAIRS.fetch("wasi-sdk")
        selected = configuration.toolchains.find { |toolchain| toolchain.name == "wasi-sdk" }

        assert_equal pair.fetch(:version), selected.version
        assert_equal pair.fetch(:sha256).fetch(Vendor::WASI_SDK_PLATFORM), selected.sha256
      end

      def test_a_definition_replaces_the_built_in_pair_for_a_referenced_toolchain
        selected = configuration_with_wasi_override.toolchains.find { |toolchain| toolchain.name == "wasi-sdk" }

        assert_equal "30.0", selected.version
        assert_equal "cafe", selected.sha256
      end

      def test_a_definition_nothing_references_is_inert
        configuration = configure do
          toolchain "wasi-sdk" do
            version "30.0"
            sha256 "cafe"
          end
        end

        assert_equal %w[mruby], configuration.toolchains.map(&:name)
      end

      def test_referencing_mruby_is_legal_redundancy
        configuration = configure do
          target(:host) { toolchain "mruby" }
        end

        assert_equal %w[mruby], configuration.toolchains.map(&:name)
      end

      def test_a_repeated_reference_is_idempotent
        configuration = configure do
          target :wasi do
            toolchain "wasi-sdk"
            toolchain "wasi-sdk"
          end
        end

        assert_equal(1, configuration.toolchains.map(&:name).count { |name| name == "wasi-sdk" })
      end

      private

      # A wasi target referencing wasi-sdk plus a definition overriding
      # its built-in pair — the override path.
      def configuration_with_wasi_override
        configure do
          target(:wasi) { toolchain "wasi-sdk" }

          toolchain "wasi-sdk" do
            version "30.0"
            sha256 "cafe"
          end
        end
      end

      # Run +block+ through the DSL exactly as +Beni::Tasks.new+ does and
      # return the resolved Configuration.
      def configure(&)
        context = Context.new
        context.instance_exec(&)
        context.configuration
      end
    end
  end
end
