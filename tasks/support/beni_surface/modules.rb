# frozen_string_literal: true

require_relative "gate"

module BeniSurface
  # Maps each crate source to the capability feature its whole module
  # sits behind. A gated module declaration carries its gate to the file
  # it names and to every file nested under it, so the items inside need
  # no gate of their own and the scan still knows what carries them.
  module Modules
    module_function

    def call(root)
      Dir.glob(File.join(root, "**", "*.rs")).each_with_object({}) do |file, map|
        gated(File.read(file)).each do |name, feature|
          files(file, name).each { |path| map[path] = feature }
        end
      end
    end

    # The gated module declarations one source makes, as name and
    # feature pairs.
    def gated(source)
      gate = Gate.at(nil)
      source.each_line(chomp: true).with_object([]) do |line, pairs|
        unless Syntax::ATTRIBUTE.match?(line)
          refuse_inline(line, gate.feature)
          name = line[Syntax::MOD_DECL, :name]
          pairs << [name, gate.feature] if name && gate.feature
        end
        gate = gate.after(line, Gate.at(nil))
      end
    end

    # A module written inline has no file to carry its gate to, so its
    # items would read as ungated and pass a net that never names them.
    # The map refuses the source rather than measuring it wrong.
    def refuse_inline(line, feature)
      name = feature && line[Syntax::INLINE_MOD, :name]
      return unless name

      raise "gated inline module `#{name}` has no file to carry feature `#{feature}`; declare it as `mod #{name};`"
    end

    # The sources a module declaration names: the file itself and
    # everything nested beneath it.
    def files(parent, name)
      base = child_dir(parent)
      [File.join(base, "#{name}.rs"), *Dir.glob(File.join(base, name, "**", "*.rs"))]
        .select { |path| File.file?(path) }
    end

    # Where a source's own child modules live: beside it for a crate or
    # module root, in a directory named after it otherwise.
    def child_dir(parent)
      dir = File.dirname(parent)
      base = File.basename(parent, ".rs")
      %w[lib mod main].include?(base) ? dir : File.join(dir, base)
    end
  end
end
