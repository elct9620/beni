# frozen_string_literal: true

require "test_helper"
require "beni"

module Beni
  module DSL
    # The definition-time error paths from SPEC.md's error table — every
    # malformed declaration fails inside the DSL run, before any task
    # exists.
    class TestErrors < Minitest::Test
      def test_an_unknown_toolchain_reference_fails
        error = assert_raises(Error) do
          configure { target(:wasi) { toolchain "llvm" } }
        end

        assert_match(/llvm/, error.message)
        assert_match(/mruby/, error.message)
      end

      def test_an_unknown_toolchain_definition_fails
        error = assert_raises(Error) do
          configure { toolchain("llvm") { version "1" } }
        end

        assert_match(/llvm/, error.message)
      end

      def test_a_definition_naming_mruby_fails
        error = assert_raises(Error) do
          configure { toolchain("mruby") { version "4.0.1" } }
        end

        assert_match(/mruby/, error.message)
        assert_match(/version/, error.message)
      end

      def test_a_definition_missing_sha256_fails
        error = assert_raises(Error) do
          configure { toolchain("wasi-sdk") { version "30.0" } }
        end

        assert_match(/sha256/, error.message)
      end

      def test_a_definition_missing_every_field_fails
        error = assert_raises(Error) do
          configure { toolchain("wasi-sdk") {} } # rubocop:disable Lint/EmptyBlock
        end

        assert_match(/version/, error.message)
      end

      def test_a_block_carrying_toolchain_inside_a_target_block_fails
        error = assert_raises(Error) do
          configure do
            target(:wasi) { toolchain("wasi-sdk") { version "30.0" } }
          end
        end

        assert_match(/block/, error.message)
      end

      def test_a_block_less_toolchain_at_the_top_level_fails
        error = assert_raises(Error) do
          configure { toolchain "wasi-sdk" }
        end

        assert_match(/block/, error.message)
      end

      def test_duplicate_toolchain_definitions_fail
        error = assert_raises(Error) { configure_duplicate_wasi_definitions }

        assert_match(/duplicate/, error.message)
      end

      def test_duplicate_target_declarations_fail
        error = assert_raises(Error) do
          configure do
            target :host
            target "host"
          end
        end

        assert_match(/duplicate/, error.message)
      end

      def test_duplicate_scalar_settings_fail
        %i[version build_config vendor_dir].each do |setting|
          error = assert_raises(Error) do
            configure do
              public_send(setting, "first")
              public_send(setting, "second")
            end
          end

          assert_match(/duplicate/, error.message)
          assert_match(/#{setting}/, error.message)
        end
      end

      def test_a_duplicate_field_inside_a_definition_block_fails
        error = assert_raises(Error) do
          configure do
            toolchain("wasi-sdk") do
              version "30.0"
              version "31.0"
            end
          end
        end

        assert_match(/duplicate/, error.message)
      end

      private

      # Two definitions naming the same toolchain — SPEC's
      # duplicate-definition error row.
      def configure_duplicate_wasi_definitions
        configure do
          2.times do
            toolchain("wasi-sdk") do
              version "30.0"
              sha256 "x"
            end
          end
        end
      end

      # Run +block+ through the DSL exactly as +Beni::Tasks.new+ does.
      def configure(&)
        context = Context.new
        context.instance_exec(&)
        context.configuration
      end
    end
  end
end
