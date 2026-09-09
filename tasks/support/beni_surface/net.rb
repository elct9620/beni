# frozen_string_literal: true

require_relative "gate"

module BeniSurface
  # Reads the drift net in +beni-tests+ into one reference body per
  # capability feature, keyed by the feature the body is gated on and
  # +nil+ for the ungated one. An entry a feature carries belongs in
  # that feature's body, so a reference sitting in the wrong body reads
  # as missing rather than passing.
  class Net
    def self.call(path)
      new(File.read(path).each_line.to_a).read.bodies
    end

    attr_reader :bodies

    def initialize(lines)
      @lines = lines
      @bodies = {}
      @gate = Gate.at(nil)
      @consumed = -1
    end

    def read
      @lines.each_index { |index| read_line(index) }
      self
    end

    private

    def read_line(index)
      return if index <= @consumed

      line = @lines[index]
      return collect(index) if !Syntax::ATTRIBUTE.match?(line) && line[Syntax::FN, :name]

      @gate = @gate.after(line, Gate.at(nil))
    end

    # A feature's body is every reference body gated on it, so a net
    # split across several fns is read whole rather than by its last.
    def collect(index)
      stop = closing_brace(index)
      (@bodies[@gate.feature] ||= +"") << @lines[index..stop].join
      @gate = Gate.at(nil)
      @consumed = stop
    end

    # Where one reference body ends: the first indentation-matched
    # closing brace at or after the fn line.
    def closing_brace(start)
      indent = @lines[start][/\A */]
      (start...@lines.size).find { |index| @lines[index].rstrip == "#{indent}}" }
    end
  end
end
