# frozen_string_literal: true

require "fileutils"

module Beni
  # Generates the consumer's editable mruby build config: a verbatim
  # copy of the staged mruby source's own +build_config/default.rb+ —
  # the configured version's upstream default. Exposed as
  # +rake beni:config+ by +Beni::Tasks+; the generated file belongs to
  # the consuming project and seeds further customization.
  module BuildConfig
    module_function

    # Copy the staged source's default config to +dest+, creating
    # missing parent directories. Requires +version+'s mruby source
    # staged under +mruby_dir+ and refuses to clobber an existing
    # (likely hand-tuned) config. Returns +dest+.
    def generate(dest, mruby_dir:, version:)
      raise Error, "[beni] #{dest} already exists — delete it first to regenerate" if File.exist?(dest)

      source = staged_default_config(mruby_dir, version)
      FileUtils.mkdir_p(File.dirname(dest))
      FileUtils.cp(source, dest)
      dest
    end

    # The staged source's upstream default config path, verified to be
    # +version+'s: the +.beni-version+ marker +Vendor::Tarball#prepare+
    # stamps must match, so a stale tree never seeds a config for the
    # wrong release.
    def staged_default_config(mruby_dir, version)
      source = File.join(mruby_dir, "build_config", "default.rb")
      return source if staged_version(mruby_dir) == version && File.exist?(source)

      raise Error,
            "[beni] mruby #{version}'s source is not staged at #{mruby_dir} — " \
            "run `rake beni:vendor:setup` first"
    end

    def staged_version(mruby_dir)
      marker = File.join(mruby_dir, Vendor::Tarball::VERSION_MARKER)
      File.read(marker).strip if File.exist?(marker)
    end
  end
end
