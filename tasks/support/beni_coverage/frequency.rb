# frozen_string_literal: true

module BeniCoverage
  # Counts how often each embedder API symbol is called, across two
  # populations that do not behave alike and are therefore never summed.
  #
  # C mrbgems approximate demand from inside the VM. Core VM sources are
  # excluded: they implement the API rather than consume it, so their
  # counts would drown the embedder signal. Only the gems a build
  # activated count — the vendored tree ships every bundled gem's sources
  # whether or not the archive contains them, and counting all of them
  # ranks the worklist by an ecosystem beni never builds.
  #
  # Rust consumers reach the C API through +beni::sys+, and they are the
  # population beni exists for. An mrbgem runs inside the VM and never
  # holds a value across a host boundary, so the API a Rust embedder
  # leans on hardest — rooting, interpreter lifecycle — scores zero on
  # the mrbgem side however badly it is needed. A symbol any Rust
  # consumer reaches therefore outranks one only mrbgems call.
  module Frequency
    BUNDLED = "vendor/mruby/mrbgems"
    # The consumer harnesses this repo ships. A Rust consumer outside it
    # — a downstream crate in its own checkout — is named by this
    # environment variable, colon-separated.
    SCENARIOS = "test/scenarios"
    CONSUMERS_ENV = "BENI_CONSUMER_PATHS"
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

    # symbol name => `beni::sys::` use count across the Rust consumers
    # in reach, restricted to +names+ like the mrbgem scan.
    def scan_rust(root, names)
      counts = names.to_h { |name| [name, 0] }
      rust_sources(root).each do |path|
        sys_calls(File.read(path)).each { |name| counts[name] += 1 if counts.key?(name) }
      end
      counts
    end

    # Rust sources belonging to consumers. Build outputs are skipped, and
    # so is the beni crate itself: it implements the binding rather than
    # consuming it, the same reason core VM sources are excluded above.
    def rust_sources(root)
      consumer_roots(root)
        .flat_map { |dir| Dir.glob(File.join(dir, "**", "*.rs")) }
        .reject { |path| path.include?("/target/") }
    end

    def consumer_roots(root)
      [File.join(root, SCENARIOS)] + ENV.fetch(CONSUMERS_ENV, "").split(":").reject(&:empty?)
    end

    # Identifiers reached through a `sys::` path. Line comments are
    # stripped first so a mention in rustdoc does not count as a use.
    def sys_calls(src)
      src.gsub(%r{//.*$}, "").scan(/\bsys::([A-Za-z_]\w*)/).flatten
    end

    # One line saying how far to trust the ranking, mirroring the
    # report's linked-versus-heuristic sys detection: a ranking whose
    # signal is silently weaker is worse than one that says it is weak.
    def signal(root)
      "#{gem_signal(root)}; #{rust_signal(root)}"
    end

    def gem_signal(root)
      gems = active_gems(root)
      return "every bundled gem — no build staged, so gems this build excludes still rank" if gems.empty?

      "#{gems.size} active gems (#{gems.join(", ")})"
    end

    def rust_signal(root)
      count = rust_sources(root).size
      return "no Rust consumer in reach — set #{CONSUMERS_ENV} to rank by what one calls" if count.zero?

      "#{count} Rust consumer sources"
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
