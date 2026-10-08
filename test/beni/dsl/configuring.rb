# frozen_string_literal: true

module Beni
  module DSL
    # Runs a block through the DSL as +Beni::Tasks.new+ does and answers
    # the resolved Configuration.
    module Configuring
      private

      def configure(&)
        context = Context.new
        context.instance_exec(&)
        context.configuration
      end
    end
  end
end
