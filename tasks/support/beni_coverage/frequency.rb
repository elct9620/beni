# frozen_string_literal: true

module BeniCoverage
  # Counts how often each embedder API symbol is called across mruby's
  # bundled mrbgems C sources. mrbgems are the embedder-shaped consumers
  # of the C API, so their call sites approximate downstream demand —
  # the signal that orders graduation priority in the coverage report.
  # Core VM sources are excluded: they implement the API rather than
  # consume it, so their counts would drown the embedder signal.
  module Frequency
    GLOB = "vendor/mruby/mrbgems/**/*.c"

    module_function

    # symbol name => call-site count, restricted to +names+ (the scanned
    # embedder surface) so the frequency shares the coverage denominator.
    # Every name gets a key — unused symbols read as 0, not absent.
    def scan(root, names)
      counts = names.to_h { |name| [name, 0] }
      Dir.glob(File.join(root, GLOB)).each do |path|
        calls(File.read(path)).each { |name| counts[name] += 1 if counts.key?(name) }
      end
      counts
    end

    # Identifiers used in call position. Comments are stripped first so a
    # mention in prose does not count as a use.
    def calls(src)
      strip_comments(src).scan(/\b([A-Za-z_]\w*)\s*\(/).flatten
    end

    def strip_comments(src)
      src.gsub(%r{/\*.*?\*/}m, "").gsub(%r{//.*$}, "")
    end
  end
end
