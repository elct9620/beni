# frozen_string_literal: true

require "test_helper"
require "tmpdir"

require_relative "../../tasks/support/beni_surface"

# The drift net's feature axis. An item a capability feature carries
# used to leave the expectation the moment it was gated, so the gate
# stayed green while the net stopped naming it. These pin that a gated
# item is expected in its own feature's net body and nowhere else.
class TestSurfaceGate < Minitest::Test
  def test_expects_a_feature_gated_fn_in_that_feature_body
    report = verify(crate: gated_crate, net: complete_net)

    assert_predicate report, :ok?
    assert_equal 3, report.total
  end

  def test_reports_a_feature_gated_fn_the_net_never_names
    report = verify(crate: gated_crate, net: ungated_net_only)

    assert_equal %w[Ccontext::load_nstring Mrb::load_string], report.missing.map(&:ref).sort
  end

  def test_reports_a_feature_gated_fn_named_in_the_ungated_body
    net = <<~RUST
      #[test]
      fn ungated() {
          let _ = Mrb::open;
          let _ = Mrb::load_string;
          let _ = Ccontext::load_nstring;
      }
    RUST

    report = verify(crate: gated_crate, net: net)

    refute_predicate report, :ok?, "a gated item named in the ungated body is in the wrong net"
  end

  def test_names_the_feature_in_a_missing_entry_diagnostic
    report = verify(crate: gated_crate, net: ungated_net_only)

    assert_includes report.missing.map(&:label), "Mrb::load_string (feature compiler)"
  end

  def test_leaves_a_non_feature_cfg_out_of_the_expectation
    crate = { "lib.rs" => <<~RUST }
      impl Mrb {
          pub fn open() {}

          #[cfg(test)]
          pub fn probe() {}
      }
    RUST

    report = verify(crate: crate, net: "#[test]\nfn ungated() {\n    let _ = Mrb::open;\n}\n")

    assert_predicate report, :ok?
    assert_equal 1, report.total, "a build-specific fn is neither expected nor counted"
  end

  def test_refuses_a_gated_inline_module
    crate = { "lib.rs" => <<~RUST }
      #[cfg(feature = "compiler")]
      mod ccontext {
          impl Ccontext {
              pub fn load_nstring() {}
          }
      }
    RUST

    error = assert_raises(RuntimeError) { verify(crate: crate, net: "") }

    assert_match(/gated inline module `ccontext`/, error.message)
  end

  private

  # A crate whose compiler surface is gated two ways: a whole module
  # declared behind the feature, and one fn gated where its siblings
  # are not.
  def gated_crate
    { "lib.rs" => gated_root, "ccontext.rs" => gated_module, "state.rs" => mixed_module }
  end

  # A crate root declaring one module behind the feature.
  def gated_root
    <<~RUST
      pub mod state;

      #[cfg(feature = "compiler")]
      pub mod ccontext;
    RUST
  end

  # A module whose items carry no gate of their own — the declaration
  # in the root is what puts them behind the feature.
  def gated_module
    <<~RUST
      impl Ccontext {
          pub fn load_nstring() {}
      }
    RUST
  end

  # A module the feature does not carry, holding one fn that it does.
  def mixed_module
    <<~RUST
      impl Mrb {
          pub fn open() {}

          #[cfg(feature = "compiler")]
          pub fn load_string() {}
      }
    RUST
  end

  def complete_net
    <<~RUST
      #[test]
      fn ungated() {
          let _ = Mrb::open;
      }

      #[cfg(feature = "compiler")]
      #[test]
      fn compiler() {
          let _ = Mrb::load_string;
          let _ = Ccontext::load_nstring;
      }
    RUST
  end

  def ungated_net_only
    <<~RUST
      #[test]
      fn ungated() {
          let _ = Mrb::open;
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
