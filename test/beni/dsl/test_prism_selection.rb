# frozen_string_literal: true

require "test_helper"
require "beni"

module Beni
  module DSL
    # prism is the toolchain its top-level definition selects: no
    # reference reaches it, and no built-in pair stands behind it.
    class TestPrismSelection < Minitest::Test
      def test_a_prism_definition_selects_prism_with_its_own_pair
        configuration = configure do
          toolchain "prism" do
            version "abc123"
            sha256 "cafe"
          end
        end
        prism = configuration.toolchains.find { |toolchain| toolchain.name == "prism" }

        assert_equal %w[mruby prism], configuration.toolchains.map(&:name)
        assert_equal %w[abc123 cafe], [prism.version, prism.sha256]
      end

      def test_a_toolchain_reference_naming_prism_fails
        error = assert_raises(Error) do
          configure { target(:host) { toolchain "prism" } }
        end

        assert_match(/prism/, error.message)
        assert_match(/definition/, error.message)
      end

      def test_a_prism_definition_missing_sha256_fails
        error = assert_raises(Error) do
          configure { toolchain("prism") { version "abc123" } }
        end

        assert_match(/sha256/, error.message)
      end

      private

      def configure(&)
        context = Context.new
        context.instance_exec(&)
        context.configuration
      end
    end
  end
end
