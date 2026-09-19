# frozen_string_literal: true

module BeniSurface
  # One macro +beni-macros+ exports, reached through +beni+'s
  # re-exports: an attribute the net applies as +#[beni::name]+, or a
  # derive it names inside +derive(...)+ as +beni::Name+.
  MacroEntry = Data.define(:kind, :name) do
    def ref
      kind == :derive ? "derive(beni::#{name})" : "#[beni::#{name}]"
    end

    alias_method :label, :ref

    def referenced?(body)
      pattern = kind == :derive ? /derive\([^)]*\bbeni::#{name}\b/ : /#\[beni::#{name}\b/
      body.match?(pattern)
    end
  end

  # Reads the proc-macro crate root into the macros it exports: a
  # +#[proc_macro_attribute]+ exports the +pub fn+ it sits on under
  # that fn's name, a +#[proc_macro_derive(Name ...)]+ exports +Name+.
  class Macros
    ATTRIBUTE = /\A#\[proc_macro_attribute\]/
    DERIVE = /\A#\[proc_macro_derive\((?<name>\w+)/

    def self.call(source)
      new.read(source).entries
    end

    attr_reader :entries

    def initialize
      @entries = []
      @pending = false
    end

    def read(source)
      source.each_line(chomp: true) { |line| read_line(line) }
      self
    end

    private

    def read_line(line)
      if (derive = line[DERIVE, :name])
        @entries << MacroEntry.new(kind: :derive, name: derive)
      elsif ATTRIBUTE.match?(line)
        @pending = true
      elsif @pending && (name = line[Syntax::PUB_FN, :name])
        @entries << MacroEntry.new(kind: :attribute, name: name)
        @pending = false
      end
    end
  end
end
