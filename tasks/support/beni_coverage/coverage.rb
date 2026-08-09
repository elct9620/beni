# frozen_string_literal: true

module BeniCoverage
  # How the measure sees one symbol. It is covered — by a recorded Rust
  # item, by an alias partner, or by a graduated item that subsumes it —
  # or it sits outside the measure, declined or gated behind a build
  # flag. Absent from all of those, it is API still owed. A symbol
  # belongs to exactly one of them, which is what lets absence mean one
  # thing.
  class Coverage
    # Manifest sections keyed by C symbol, and what each does to the
    # measure.
    COVERING = %w[typed subsumed].freeze
    OUTSIDE = %w[declined conditional].freeze
    SECTIONS = (COVERING + OUTSIDE).freeze

    # Section entries matching no scanned symbol, symbols more than one
    # section claims, and classifications with nothing behind them — each
    # rejects the manifest.
    attr_reader :unknown, :conflicting, :unexplained

    def initialize(manifest:, sys:, equivalents:, inventory:)
      @sections = SECTIONS.to_h { |name| [name, manifest[name] || {}] }
      @sys = sys
      @equivalents = equivalents
      @unknown = @sections.values.flat_map(&:keys).uniq - inventory
      @conflicting = doubly_claimed
      @unexplained = reasonless
    end

    def in_sys?(name)
      @sys.include?(name)
    end

    def typed?(name)
      @equivalents.key?(name) || COVERING.any? { |section| @sections.fetch(section).key?(name) }
    end

    # Which section takes the symbol out of the measure, so neither
    # numerator nor denominator counts it — nil while it still counts.
    def exclusion(name)
      OUTSIDE.find { |section| @sections.fetch(section).key?(name) }
    end

    # What the report prints for a symbol: the hand-authored reason where
    # one exists, the partner where an alias covers it, and nothing at
    # all where the symbol is still owed.
    def note(name)
      partner = @equivalents[name]
      return "defined as `#{partner}`" if partner

      section = SECTIONS.find { |candidate| @sections.fetch(candidate).key?(name) }
      section ? reason(section, @sections.fetch(section)[name]) : ""
    end

    private

    # A symbol two sections claim, or one an alias covers while a
    # section takes it out of the measure — either way the record says
    # two things about it and only one can be acted on.
    # A symbol taken out of the measure with nothing said about why. The
    # ratio moves on these, so an unreviewable one cannot be allowed to
    # stand; a bare `typed` Note is only an unnamed Rust side, which the
    # report flags without failing.
    def reasonless
      OUTSIDE.flat_map { |section| @sections.fetch(section).select { |_, note| note.to_s.strip.empty? }.keys }.sort
    end

    def doubly_claimed
      named = @sections.values.flat_map(&:keys)
      contradicted = @equivalents.keys.select { |name| exclusion(name) }
      (named.tally.select { |_, count| count > 1 }.keys + contradicted).uniq.sort
    end

    # A typed Note names the Rust side and stands alone; every other
    # section leads with its state, so a row says what it is before it
    # says why. A state with nothing behind it is flagged rather than
    # rendered — an unexplained classification cannot be reviewed.
    def reason(section, note)
      text = note.to_s.strip
      return blank_reason(section) if text.empty?

      text = "`#{text}`" if text.match?(/\A\S+\z/)
      section == "typed" ? text : "#{section}: #{text}"
    end

    # The manifest reads a `~` typed Note as "unspecified"; any other
    # section left blank is a classification nobody can review.
    def blank_reason(section)
      section == "typed" ? "⚠️ unspecified" : "⚠️ #{section} without a reason"
    end
  end
end
