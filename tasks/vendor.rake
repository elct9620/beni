# frozen_string_literal: true

# Vendor toolchain rake task
# ==========================
#
# Fetches and unpacks the build-time toolchain into `vendor/`. The
# tarball-based artifacts (mruby, wasi-sdk) are declared as
# +BeniVendor::Toolchain+ values in +tasks/support/beni_vendor.rb+;
# this file iterates over +BeniVendor::TARBALL_TOOLCHAINS+ to wire one
# +file+ task and one +setup:<name>+ task per artifact. Adding a new
# tarball artifact is a single +Toolchain.new(...)+ entry — no rake DSL
# surgery required.
#
# Idempotency: tarball downloads are +file+ tasks keyed on the cached
# tarball path; unpacking short-circuits unless the version stamped in
# the unpacked tree differs from the pinned version, in which case a
# bump forces a clean re-extract.
#
# Honors +BENI_VENDOR_BASE_URL+ to point downloads at a local fixture
# during tests, and +BENI_VENDOR_DIR+ to relocate the entire vendor
# tree (also test-only).

require_relative "support/beni_vendor"

namespace :vendor do
  BeniVendor::TARBALL_TOOLCHAINS.each do |toolchain|
    file toolchain.tarball_path do
      toolchain.fetch
    end
  end

  namespace :setup do
    BeniVendor::TARBALL_TOOLCHAINS.each do |toolchain|
      desc "Download and unpack #{toolchain.name} #{toolchain.version_label} into #{toolchain.final_dir}"
      task toolchain.task_name => toolchain.tarball_path do
        toolchain.install
      end
    end
  end

  desc "Fetch and unpack all build-time vendor toolchains (mruby + wasi-sdk)"
  task setup: BeniVendor::TARBALL_TOOLCHAINS.map { |t| "setup:#{t.task_name}" }

  desc "Remove unpacked vendor toolchains (keeps cached tarballs)"
  task :clean do
    BeniVendor::TARBALL_TOOLCHAINS.each { |t| FileUtils.rm_rf(t.final_dir) }
  end

  desc "Remove vendor/ entirely (unpacked trees and cached tarballs)"
  task :clobber do
    FileUtils.rm_rf(BeniVendor::VENDOR_DIR)
  end
end
