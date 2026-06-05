# frozen_string_literal: true

require_relative "lib/beni/version"

Gem::Specification.new do |spec|
  spec.name = "beni"
  spec.version = Beni::VERSION
  spec.authors = ["Aotokitsuruya"]
  spec.email = ["contact@aotoki.me"]

  spec.summary = "mruby dependency manager"
  spec.description = "Reserved: mruby dependency manager that vendors mruby source and builds " \
                     "libmruby.a, under active development. The build chain is being extracted " \
                     "from the kobako project."
  spec.homepage = "https://github.com/elct9620/beni"
  spec.license = "Apache-2.0"
  spec.required_ruby_version = ">= 3.2.0"
  spec.metadata["homepage_uri"] = spec.homepage
  spec.metadata["source_code_uri"] = spec.homepage

  # Require MFA for gem pushes, protecting the gem from supply chain
  # attacks by ensuring no one can publish a new version without
  # multi-factor authentication.
  # See: https://guides.rubygems.org/mfa-requirement-opt-in/
  spec.metadata["rubygems_mfa_required"] = "true"

  # Specify which files should be added to the gem when it is released.
  # The `git ls-files -z` loads the files in the RubyGem that have been added into git.
  gemspec = File.basename(__FILE__)
  spec.files = IO.popen(%w[git ls-files -z], chdir: __dir__, err: IO::NULL) do |ls|
    ls.readlines("\x0", chomp: true).reject do |f|
      (f == gemspec) ||
        f.start_with?(*%w[bin/ Gemfile .gitignore test/ .github/ .rubocop.yml
                          crates/ Cargo.toml Cargo.lock rust-toolchain.toml
                          tasks/])
    end
  end
  spec.bindir = "exe"
  spec.executables = spec.files.grep(%r{\Aexe/}) { |f| File.basename(f) }
  spec.require_paths = ["lib"]

  # Beni's public surface is a Rake task library (Beni::Tasks) and a
  # builder that drives mruby's own rake, so rake is a real runtime
  # dependency, not just a development tool.
  spec.add_dependency "rake", "~> 13.0"

  # For more information and examples about making a new gem, check out our
  # guide at: https://guides.rubygems.org/make-your-own-gem/
end
