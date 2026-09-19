# frozen_string_literal: true

require "test_helper"
require_relative "surface_harness"

# The macros +beni-macros+ exports are surface too: the net applies
# each one — an attribute as +#[beni::name]+, a derive inside
# +derive(...)+ — so dropping a re-export breaks the net's compilation.
class TestSurfaceMacros < Minitest::Test
  include SurfaceHarness

  def test_expects_each_exported_macro_applied_in_the_net
    net = <<~RUST
      #[test]
      fn ungated() {
          let _ = Mrb::open;
          #[beni::wrap(class = "Wrapped")]
          struct Wrapped;
          #[derive(beni::TypedData)]
          #[beni(class = "Derived")]
          struct Derived;
      }
    RUST

    report = verify(crate: ungated_crate, net: net, macros: macro_crate)

    assert_predicate report, :ok?
    assert_equal 3, report.total
  end

  def test_reports_an_exported_macro_the_net_never_applies
    report = verify(crate: ungated_crate, net: ungated_net_only, macros: macro_crate)

    assert_equal ["#[beni::wrap]", "derive(beni::TypedData)"], report.missing.map(&:ref).sort
  end

  private

  def ungated_crate
    { "lib.rs" => "impl Mrb {\n    pub fn open() {}\n}\n" }
  end

  def ungated_net_only
    "#[test]\nfn ungated() {\n    let _ = Mrb::open;\n}\n"
  end

  # A proc-macro crate root exporting one attribute and one derive.
  def macro_crate
    <<~RUST
      /// Doc.
      #[proc_macro_attribute]
      pub fn wrap(attrs: TokenStream, item: TokenStream) -> TokenStream {
          item
      }

      #[proc_macro_derive(TypedData, attributes(beni))]
      pub fn derive_typed_data(input: TokenStream) -> TokenStream {
          input
      }
    RUST
  end
end
