# frozen_string_literal: true

require "tmpdir"

require_relative "../../tasks/support/beni_surface"

# Runs the surface gate against fixture sources written to a scratch
# tree: the crate's modules, the net file, and the proc-macro crate
# root, which exports nothing unless a case gives it macros.
module SurfaceHarness
  private

  def verify(crate:, net:, macros: "")
    Dir.mktmpdir do |dir|
      src = File.join(dir, "src")
      Dir.mkdir(src)
      crate.each { |name, body| File.write(File.join(src, name), body) }
      BeniSurface.verify(crate_src: src,
                         net_file: write(dir, "surface_test.rs", net),
                         macro_src: write(dir, "macros.rs", macros))
    end
  end

  def write(dir, name, body)
    File.join(dir, name).tap { |path| File.write(path, body) }
  end
end
