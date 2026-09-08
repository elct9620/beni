# frozen_string_literal: true

require_relative "support/beni_docs_bindings"

# The documentation bindings are generated where they are read, so
# nothing tracks them and nothing can go stale. A local build writes its
# own before the documentation leg reads it; a release writes the copy
# the published package carries, which is the one case where the host
# that writes has to be the host that will read.
namespace :docs do
  desc "Generate the documentation bindings from an upstream-default mruby"
  task bindings: "beni:vendor:setup:mruby" do
    path = BeniDocsBindings.generate
    puts "[docs:bindings] wrote #{path.delete_prefix("#{Dir.pwd}/")}"
  end

  namespace :bindings do
    desc "Generate the documentation bindings a published package carries"
    task release: "beni:vendor:setup:mruby" do
      BeniDocsBindings.require_documentation_host!
      Rake::Task["docs:bindings"].invoke
    end
  end
end
