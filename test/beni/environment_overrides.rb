# frozen_string_literal: true

module Beni
  # Sets environment variables for the length of a block and puts back
  # what each held before, an unset variable included.
  module EnvironmentOverrides
    private

    def with_env(overrides)
      saved = overrides.keys.to_h { |key| [key, ENV.fetch(key, nil)] }
      overrides.each { |key, value| ENV[key] = value }
      yield
    ensure
      saved&.each { |key, value| ENV[key] = value }
    end
  end
end
