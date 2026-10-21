# frozen_string_literal: true

require "test_helper"
require_relative "surface_harness"

# The drift net's axes past one gate each: an item two axes gate is
# expected where both gate the net, and a net body only some builds
# compile answers for no axis.
class TestSurfaceGateAxes < Minitest::Test
  include SurfaceHarness

  def test_a_build_specific_net_body_names_nothing_for_the_expectation
    crate = { "lib.rs" => "impl Mrb {\n    pub fn open() {}\n}\n" }
    net = "#[cfg(not(mrb_int64))]\n#[test]\nfn narrow() {\n    let _ = Mrb::open;\n}\n"

    assert_equal ["Mrb::open"], verify(crate: crate, net: net).missing.map(&:ref)
  end

  def test_expects_an_item_two_axes_gate_in_a_body_gated_on_both
    crate = { "lib.rs" => <<~RUST }
      #[cfg(feature = "compiler")]
      impl Mrb {
          #[cfg(mrb_debug_hook)]
          pub fn hooked() {}
      }
    RUST
    one_axis = "#[cfg(feature = \"compiler\")]\n#[test]\nfn compiled() {\n    let _ = Mrb::hooked;\n}\n"
    both = "#[cfg(feature = \"compiler\")]\n#[cfg(mrb_debug_hook)]\n#[test]\nfn both() {\n    let _ = Mrb::hooked;\n}\n"

    assert_equal ["Mrb::hooked (feature compiler, cfg mrb_debug_hook)"],
                 verify(crate: crate, net: one_axis).missing.map(&:label)
    assert_predicate verify(crate: crate, net: both), :ok?
  end

  def test_expects_an_item_nested_gated_modules_carry_behind_both
    crate = { "lib.rs" => "#[cfg(feature = \"compiler\")]\npub mod outer;\n",
              "outer.rs" => "#[cfg(mrb_debug_hook)]\npub mod inner;\n",
              "outer/inner.rs" => "impl TraceArg {\n    pub fn lineno() {}\n}\n" }

    assert_equal ["TraceArg::lineno (feature compiler, cfg mrb_debug_hook)"],
                 verify(crate: crate, net: "").missing.map(&:label)
  end
end
