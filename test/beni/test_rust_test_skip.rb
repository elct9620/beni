# frozen_string_literal: true

require "test_helper"

require_relative "../../tasks/support/beni_rust"

# BENI_TEST_SKIP names the tests a chain leaves out by name, so a lane
# can list the failures it already knows and still fail on a new one.
class TestRustTestSkip < Minitest::Test
  def test_each_named_test_becomes_a_skip_for_the_test_binaries
    with_skip("first_known  second_known") do
      assert_equal %w[-- --skip first_known --skip second_known], BeniRust.test_harness_args
    end
  end

  def test_no_named_test_passes_nothing_to_the_test_binaries
    with_skip(nil) { assert_empty BeniRust.test_harness_args }
    with_skip(" ") { assert_empty BeniRust.test_harness_args }
  end

  private

  def with_skip(value)
    saved = ENV.fetch("BENI_TEST_SKIP", nil)
    ENV["BENI_TEST_SKIP"] = value
    yield
  ensure
    ENV["BENI_TEST_SKIP"] = saved
  end
end
