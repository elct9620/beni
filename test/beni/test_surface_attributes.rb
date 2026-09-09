# frozen_string_literal: true

require "test_helper"
require "tmpdir"

require_relative "../../tasks/support/beni_surface"

# How attributes accumulate onto the item below them. All three
# scanners read that rule from one place, so a fixture carrying the
# doc comment on both sides would move both together and see nothing;
# only the crate side carries one here, so a gate the scan drops shows
# up as an entry the net no longer names.
class TestSurfaceAttributes < Minitest::Test
  def test_carries_a_gate_across_the_doc_comment_below_it
    report = verify(crate: doc_comment_crate, net: doc_comment_net)

    assert_predicate report, :ok?, "the doc comment must not drop either gate"
    assert_equal 2, report.total
  end

  private

  # A gated fn whose doc comment sits between the attribute and the fn.
  def doc_comment_crate
    { "lib.rs" => <<~RUST }
      impl Mrb {
          pub fn open() {}

          #[cfg(feature = "compiler")]
          /// Compiles Ruby source.
          pub fn load_string() {}
      }
    RUST
  end

  # The matching net, whose own gate sits directly on its fn.
  def doc_comment_net
    <<~RUST
      #[test]
      fn ungated() {
          let _ = Mrb::open;
      }

      #[cfg(feature = "compiler")]
      #[test]
      fn compiler() {
          let _ = Mrb::load_string;
      }
    RUST
  end

  def verify(crate:, net:)
    Dir.mktmpdir do |dir|
      src = File.join(dir, "src")
      Dir.mkdir(src)
      crate.each { |name, body| File.write(File.join(src, name), body) }
      net_file = File.join(dir, "surface_test.rs")
      File.write(net_file, net)
      BeniSurface.verify(crate_src: src, net_file: net_file)
    end
  end
end
