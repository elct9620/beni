# frozen_string_literal: true

require "rake"

module Beni
  # Gives each test a Rake application of its own, so the tasks one test
  # defines never reach another, and puts the process's back afterwards.
  module FreshRakeApplication
    def setup
      super
      @original_application = Rake.application
      Rake.application = Rake::Application.new
    end

    def teardown
      Rake.application = @original_application
      super
    end
  end
end
