# frozen_string_literal: true

require "fileutils"
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
      write_crate(src, crate)
      BeniSurface.verify(crate_src: src,
                         net_file: write(dir, "surface_test.rs", net),
                         macro_src: write(dir, "macros.rs", macros))
    end
  end

  def write_crate(src, crate)
    crate.each do |name, body|
      path = File.join(src, name)
      FileUtils.mkdir_p(File.dirname(path))
      File.write(path, body)
    end
  end

  def write(dir, name, body)
    File.join(dir, name).tap { |path| File.write(path, body) }
  end
end
