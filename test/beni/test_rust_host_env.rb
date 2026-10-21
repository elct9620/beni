# frozen_string_literal: true

require "test_helper"

require_relative "../../tasks/support/beni_rust"

# The host legs discover the vendor tree's archive, so a shell that
# exports an archive path of its own must not redirect them.
class TestRustHostEnv < Minitest::Test
  def test_the_host_legs_read_the_vendor_tree
    assert_equal BeniRust::VENDOR_DIR, BeniRust.host_env.fetch("BENI_VENDOR_DIR")
  end

  def test_the_host_legs_clear_a_shell_archive_path
    env = BeniRust.host_env

    assert env.key?("MRUBY_LIB_DIR"), "an unset key would leave the shell's value in place"
    assert_nil env["MRUBY_LIB_DIR"]
  end
end
