# frozen_string_literal: true

require "test_helper"
require "beni"

module Beni
  module DSL
    # Scalar settings and target declarations — defaults, precedence,
    # and path resolution.
    class TestSettings < Minitest::Test
      def test_declared_targets_replace_the_default_set_entirely
        configuration = configure { target :embedded }

        assert_equal %w[embedded], configuration.targets
      end

      def test_target_names_normalize_symbols_and_strings
        configuration = configure do
          target :host
          target "wasi"
        end

        assert_equal %w[host wasi], configuration.targets
      end

      def test_version_selects_mruby_and_a_non_default_version_falls_to_tofu
        configuration = configure { version "4.0.1" }
        mruby = configuration.toolchains.find { |toolchain| toolchain.name == "mruby" }

        assert_equal "4.0.1", mruby.version
        assert_nil mruby.sha256
      end

      def test_vendor_dir_declaration_overrides_env_overrides_default
        with_env("BENI_VENDOR_DIR" => "/env/vendor") do
          declared = configure { vendor_dir "declared" }
          from_env = configure {} # rubocop:disable Lint/EmptyBlock

          assert_equal File.expand_path("declared"), declared.vendor_dir
          assert_equal "/env/vendor", from_env.vendor_dir
        end

        default = configure {} # rubocop:disable Lint/EmptyBlock

        assert_equal File.expand_path("vendor"), default.vendor_dir
      end

      def test_build_config_defaults_to_nil_and_resolves_declared_relative_paths
        default = configure {} # rubocop:disable Lint/EmptyBlock
        declared = configure { build_config "build_config/mruby.rb" }

        assert_nil default.build_config
        assert_equal File.expand_path("build_config/mruby.rb"), declared.build_config
      end

      private

      # Run +block+ through the DSL exactly as +Beni::Tasks.new+ does and
      # return the resolved Configuration.
      def configure(&)
        context = Context.new
        context.instance_exec(&)
        context.configuration
      end

      def with_env(overrides)
        saved = overrides.keys.to_h { |key| [key, ENV.fetch(key, nil)] }
        overrides.each { |key, value| ENV[key] = value }
        yield
      ensure
        saved&.each { |key, value| ENV[key] = value }
      end
    end
  end
end
