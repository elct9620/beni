# frozen_string_literal: true

require "test_helper"
require "tmpdir"

require_relative "../../tasks/support/beni_rust"

# The verification chain's own mruby build trees sit outside the staged
# source, so re-extracting it for another release leaves them in place.
# Each must start over rather than reuse that release's objects.
class TestRustBuildDirs < Minitest::Test
  def setup
    @dir = Dir.mktmpdir("beni-rust-build")
    @source = File.join(@dir, "mruby")
    @build = File.join(@dir, "build")
    stage_source("4.0.0")
  end

  def teardown
    FileUtils.remove_entry(@dir)
  end

  def test_a_tree_built_from_another_release_starts_over
    BeniRust.converge_build_dir(@build, @source)
    File.write(File.join(@build, "object.o"), "4.0.0")
    stage_source("4.1.0-rc2")

    BeniRust.converge_build_dir(@build, @source)

    refute_path_exists File.join(@build, "object.o")
  end

  def test_a_tree_built_from_the_staged_release_is_kept
    BeniRust.converge_build_dir(@build, @source)
    File.write(File.join(@build, "object.o"), "4.0.0")

    BeniRust.converge_build_dir(@build, @source)

    assert_path_exists File.join(@build, "object.o")
  end

  private

  def stage_source(version)
    FileUtils.mkdir_p(@source)
    File.write(File.join(@source, Beni::Vendor::Tarball::VERSION_MARKER), "#{version}\n")
  end
end
