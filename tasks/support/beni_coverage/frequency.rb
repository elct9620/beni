# frozen_string_literal: true

module BeniCoverage
  # Counts how often each embedder API symbol is called across the
  # mrbgems this repo builds. mrbgems are the embedder-shaped consumers
  # of the C API, so their call sites approximate downstream demand —
  # the signal that orders graduation priority in the coverage report.
  # Core VM sources are excluded: they implement the API rather than
  # consume it, so their counts would drown the embedder signal.
  #
  # Only the gems a build activated count. The vendored tree ships every
  # bundled gem's sources whether or not the archive contains them, and
  # counting all of them ranks the worklist by an ecosystem beni never
  # builds — POSIX io and socket gems reaching for API a Rust embedder
  # has no use for.
  module Frequency
    BUNDLED = "vendor/mruby/mrbgems"
    # mruby rewrites this only when the gem set changes, so it is the
    # authoritative record of what a target built — mruby's own
    # +gem_init.c+ takes it as a prerequisite for the same reason.
    ACTIVE_GEMS = "vendor/mruby/build/*/mrbgems/active_gems.txt"

    module_function

    # symbol name => call-site count, restricted to +names+ (the scanned
    # embedder surface) so the frequency shares the coverage denominator.
    # Every name gets a key — unused symbols read as 0, not absent.
    def scan(root, names)
      counts = names.to_h { |name| [name, 0] }
      sources(root).each do |path|
        calls(File.read(path)).each { |name| counts[name] += 1 if counts.key?(name) }
      end
      counts
    end

    # The C sources whose calls count as demand: the active gems' where a
    # build has declared them, every bundled gem otherwise.
    def sources(root)
      gems = active_gems(root)
      dirs = gems.empty? ? [BUNDLED] : gems.map { |gem| File.join(BUNDLED, gem) }
      dirs.flat_map { |dir| Dir.glob(File.join(root, dir, "**", "*.c")) }
    end

    # Gem names the staged builds activated, unioned across targets:
    # what this repo builds is every target, and their gem sets differ.
    # Empty when no build is staged. A name with no bundled directory —
    # a path or git gem a consumer added — contributes no sources.
    def active_gems(root)
      Dir.glob(File.join(root, ACTIVE_GEMS))
         .flat_map { |file| File.readlines(file, chomp: true) }
         .reject(&:empty?).uniq.sort
    end

    # One line saying how far to trust the ranking, mirroring the
    # report's linked-versus-heuristic sys detection: a ranking whose
    # signal is silently weaker is worse than one that says it is weak.
    def signal(root)
      gems = active_gems(root)
      return "every bundled gem — no build staged, so gems this build excludes still rank" if gems.empty?

      "#{gems.size} active gems (#{gems.join(", ")})"
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
