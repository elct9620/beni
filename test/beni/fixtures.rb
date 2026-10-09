# frozen_string_literal: true

require "fileutils"

module Beni
  # Files a test stages in place of what a real build or download leaves.
  module Fixtures
    private

    # Fakes a fully built target: the sidecar naming the archive, and
    # the archive itself beside it.
    def touch_libmruby(builder, target, archive: "libmruby.a")
      dir = builder.staged_path(target)
      FileUtils.mkdir_p(dir)
      File.write(File.join(dir, Builder::FLAGS_MAK),
                 "#{Builder::ARCHIVE_PATH_KEY}$(MRUBY_PACKAGE_DIR)/lib/#{archive}\n")
      FileUtils.touch(File.join(dir, archive))
    end

    # Writes +files+ (relative path => content) under +root/top_level_dir+
    # and packs that directory into the gzipped +tarball+, answering its
    # path.
    def pack_tarball(tarball, root, top_level_dir, files)
      FileUtils.mkdir_p(File.join(root, top_level_dir))
      files.each do |name, content|
        path = File.join(root, top_level_dir, name)
        FileUtils.mkdir_p(File.dirname(path))
        File.write(path, content)
      end
      FileUtils.mkdir_p(File.dirname(tarball))
      system("tar", "-czf", tarball, "-C", root, top_level_dir, exception: true)
      tarball
    end
  end
end
