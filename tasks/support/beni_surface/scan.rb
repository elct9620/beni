# frozen_string_literal: true

require_relative "gate"

module BeniSurface
  # One inherent public method path, e.g. +Value::cv_get+, together
  # with the capability feature carrying it — +nil+ when every build
  # has it.
  Entry = Data.define(:type, :name, :feature) do
    def ref
      "#{type}::#{name}"
    end

    # How a diagnostic names the entry: the path, and the capability
    # feature it sits behind when one does.
    def label
      feature ? "#{ref} (feature #{feature})" : ref
    end
  end

  # Reads one crate source into the inherent +pub fn+ entries it
  # declares. Attributes accumulate onto the item that follows: those
  # before a column-zero +impl+ gate every fn in it, those before a fn
  # gate that one alone, and a source whose whole module a feature
  # carries starts every entry behind that feature.
  class Scan
    def self.call(source, feature)
      new(feature).read(source).entries
    end

    attr_reader :entries

    def initialize(feature)
      @feature = feature
      @entries = []
      @type = nil
      @block = Gate.at(feature)
      @gate = @block
    end

    def read(source)
      source.each_line(chomp: true) { |line| read_line(line) }
      self
    end

    private

    def read_line(line)
      return read_outside_impl(line) if @type.nil?

      read_inside_impl(line)
    end

    def read_outside_impl(line)
      type = Syntax.impl_type(line)
      return open_impl(type) if type

      @gate = @gate.after(line, Gate.at(@feature))
    end

    def open_impl(type)
      @type = type
      @block = @gate
    end

    def read_inside_impl(line)
      return close_impl if line == "}"

      collect(line[Syntax::PUB_FN, :name]) unless Syntax::ATTRIBUTE.match?(line)
      @gate = @gate.after(line, @block)
    end

    def collect(name)
      return unless name && !@gate.excluded

      @entries << Entry.new(type: @type, name: name, feature: @gate.feature)
    end

    def close_impl
      @type = nil
      @block = Gate.at(@feature)
      @gate = @block
    end
  end
end
