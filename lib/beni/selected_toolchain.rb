# frozen_string_literal: true

module Beni
  # One selected toolchain with its resolved +(version, sha256)+ pair.
  # +sha256+ is concrete at resolution time — a definition's declared
  # value or the built-in checksum +Beni::Vendor+ resolves. +nil+ has
  # exactly one reachable meaning: mruby at a non-default version, where
  # verification falls to the TOFU sidecar path.
  class SelectedToolchain < Data.define(:name, :version, :sha256)
  end
end
