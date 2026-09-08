# frozen_string_literal: true

require_relative "support/beni_docs_bindings"

# The documentation bindings are generated, so the way to change them is
# to regenerate them: a build gate reruns this task and fails on any
# difference, which is what keeps the rendered documentation in step with
# the surface the crate would produce for itself.
namespace :docs do
  desc "Regenerate the documentation bindings from an upstream-default mruby"
  task bindings: "beni:vendor:setup:mruby" do
    path = BeniDocsBindings.generate
    puts "[docs:bindings] wrote #{path.delete_prefix("#{Dir.pwd}/")}"
  end
end
