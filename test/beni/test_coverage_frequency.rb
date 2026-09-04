# frozen_string_literal: true

require "test_helper"
require "tmpdir"

require_relative "../../tasks/support/beni_coverage"

# The Rust half of the demand signal. A scanner that silently matches
# nothing looks exactly like a consumer that calls nothing, so these pin
# that it counts a real use and skips a mention in prose.
class TestCoverageFrequency < Minitest::Test
  def test_counts_a_symbol_reached_through_a_sys_path
    counts = scan_source(<<~RUST, %w[mrb_gc_register mrb_full_gc])
      unsafe { sys::mrb_gc_register(mrb.as_ptr(), v.as_raw()) };
    RUST

    assert_equal 1, counts["mrb_gc_register"]
    assert_equal 0, counts["mrb_full_gc"], "a symbol the source never names counts zero"
  end

  def test_skips_a_symbol_named_only_in_a_comment
    counts = scan_source(<<~RUST, %w[mrb_respond_to])
      //! The dispatch confirms the constant responds via `sys::mrb_respond_to`.
      // sys::mrb_respond_to would go here.
      let unrelated = 1;
    RUST

    assert_equal 0, counts["mrb_respond_to"], "rustdoc prose is a mention, not a use"
  end

  def test_reports_no_consumer_when_none_is_in_reach
    with_consumer(nil) do
      assert_includes BeniCoverage::Frequency.rust_signal(Dir.mktmpdir), "no Rust consumer in reach"
    end
  end

  private

  # Run the Rust scan over one throwaway source file, restricted to +names+.
  def scan_source(source, names)
    Dir.mktmpdir do |dir|
      File.write(File.join(dir, "consumer.rs"), source)
      with_consumer(dir) { BeniCoverage::Frequency.scan_rust(dir, names) }
    end
  end

  def with_consumer(path)
    previous = ENV.fetch("BENI_CONSUMER_PATHS", nil)
    ENV["BENI_CONSUMER_PATHS"] = path
    yield
  ensure
    ENV["BENI_CONSUMER_PATHS"] = previous
  end
end
