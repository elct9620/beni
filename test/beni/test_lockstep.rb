# frozen_string_literal: true

require "test_helper"

module Beni
  # Release lockstep: the three packages ship one version, and the Rust
  # channel moves together with the wasi-sdk pin. These asserts turn a
  # partial bump into a test failure instead of a broken downstream
  # build.
  class TestLockstep < Minitest::Test
    ROOT = File.expand_path("../..", __dir__)

    def test_gem_workspace_and_dependency_versions_match
      workspace = File.read(File.join(ROOT, "Cargo.toml"))[/^version = "([^"]+)"/, 1]
      dependency = File.read(File.join(ROOT, "crates", "beni", "Cargo.toml"))[
        /beni-sys = \{[^}]*version = "([^"]+)"/, 1
      ]

      assert_equal Beni::VERSION, workspace,
                   "workspace Cargo.toml version drifted from lib/beni/version.rb"
      assert_equal Beni::VERSION, dependency,
                   "crates/beni's beni-sys dependency version drifted from lib/beni/version.rb"
    end

    def test_rust_channel_and_wasi_sdk_pin_move_together
      channel = File.read(File.join(ROOT, "rust-toolchain.toml"))[/^channel = "([^"]+)"/, 1]
      wasi_sdk = Beni::Vendor::BUILT_IN_PAIRS.fetch("wasi-sdk").fetch(:version)

      # wasm32-wasip1's crt1-command.o references __wasi_init_tp from Rust
      # 1.96 onward, and wasi-sdk 33's libc.a is the first to supply it —
      # a channel at or past 1.96 requires the pin at or past 33. Bump the
      # pair together, here and in kobako.
      return unless Gem::Version.new(channel) >= Gem::Version.new("1.96")

      assert_operator Gem::Version.new(wasi_sdk), :>=, Gem::Version.new("33"),
                      "rust-toolchain #{channel} needs wasi-sdk >= 33 (__wasi_init_tp); pinned #{wasi_sdk}"
    end
  end
end
