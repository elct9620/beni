# frozen_string_literal: true

# Rust workspace support module
# =============================
#
# Pure-Ruby helpers backing +tasks/rust.rake+. Owns cargo availability
# and wasm32-wasip1 target detection. The .rake wrapper is the rake DSL
# surface that glues these helpers to +rake rust:check+ /
# +rake rust:test+ / +rake rust:check:wasm+.

require "open3"

# Helpers for the Cargo workspace at the repo root. See sibling
# +tasks/rust.rake+ for the rake DSL.
module BeniRust
  ROOT = File.expand_path("../..", __dir__)
  WASM_TARGET = "wasm32-wasip1"

  def self.cargo_available?
    system("which cargo > /dev/null 2>&1")
  end

  # Returns WASM_TARGET if the toolchain has it provisioned, otherwise nil
  # so the caller falls back to the host target. Keeps the task useful in
  # CI lanes that haven't yet installed the cross target.
  def self.wasm_target_or_host
    out, status = Open3.capture2("rustc", "--print", "target-list")
    return nil unless status.success?
    return nil unless out.include?(WASM_TARGET)

    # Probe whether the target's sysroot is actually present; if absent,
    # cargo check would fail. Degrade gracefully to host instead.
    _probe, probe_status = Open3.capture2(
      "rustc", "--target", WASM_TARGET, "--print", "sysroot"
    )
    probe_status.success? ? WASM_TARGET : nil
  rescue StandardError
    nil
  end
end
