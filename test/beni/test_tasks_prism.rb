# frozen_string_literal: true

require "test_helper"
require "rake"
require "tmpdir"
require "digest"
require "beni/tasks"
require_relative "fixtures"
require_relative "fresh_rake_application"

module Beni
  # A prism definition staged through the real task chain: fixture
  # tarballs sit in the tarball cache, so setup unpacks without a
  # download and the staged tree is what the assertions read.
  class TestTasksPrism < Minitest::Test
    include Fixtures
    include FreshRakeApplication

    COMMIT = "c0e37816e97e23e92524a4070e1b99a4025bc63f"

    def setup
      super
      @dir = Dir.mktmpdir("beni-prism")
      @vendor_dir = File.join(@dir, "vendor")
      @prism_sha256 = cache_fixture("prism-#{COMMIT}", "#{COMMIT}.tar.gz", "templates/template.rb")
    end

    def teardown
      FileUtils.remove_entry(@dir)
      super
    end

    def test_vendor_setup_stages_the_prism_tree_inside_the_mruby_source
      cache_fixture("mruby-9.9.0", "9.9.0.tar.gz", "Rakefile")

      setup_with(mruby_version: "9.9.0", task: "beni:vendor:setup")

      assert_path_exists File.join(prism_tree, "templates", "template.rb")
    end

    def test_a_re_extracted_mruby_source_regains_the_prism_tree
      cache_fixture("mruby-9.9.0", "9.9.0.tar.gz", "Rakefile")
      cache_fixture("mruby-9.9.1", "9.9.1.tar.gz", "Rakefile")
      setup_with(mruby_version: "9.9.0", task: "beni:vendor:setup")

      setup_with(mruby_version: "9.9.1", task: "beni:vendor:setup")

      assert_equal "9.9.1", File.read(File.join(@vendor_dir, "mruby", Vendor::Tarball::VERSION_MARKER)).strip
      assert_path_exists File.join(prism_tree, "templates", "template.rb")
    end

    def test_setting_up_prism_alone_stages_the_mruby_source_first
      cache_fixture("mruby-9.9.0", "9.9.0.tar.gz", "Rakefile")

      setup_with(mruby_version: "9.9.0", task: "beni:vendor:setup:prism")

      assert_path_exists File.join(@vendor_dir, "mruby", "Rakefile")
      assert_path_exists File.join(prism_tree, "templates", "template.rb")
    end

    private

    def prism_tree
      File.join(@vendor_dir, "mruby", "mrbgems", "mruby-compiler", "lib", "prism")
    end

    def setup_with(mruby_version:, task:)
      Rake.application = Rake::Application.new
      define_tasks(mruby_version)
      capture_io { Rake::Task[task].invoke }
    end

    def define_tasks(mruby_version)
      vendor = @vendor_dir
      prism_sha256 = @prism_sha256
      Tasks.new do
        vendor_dir vendor
        version mruby_version
        toolchain("prism") do
          version COMMIT
          sha256 prism_sha256
        end
      end
    end

    # Write a tarball holding +top_level_dir/file+ into the tarball cache
    # under +tarball_name+ and return its SHA256.
    def cache_fixture(top_level_dir, tarball_name, file)
      tarball = pack_tarball(File.join(@vendor_dir, ".cache", tarball_name), File.join(@dir, "src"),
                             top_level_dir, file => top_level_dir)
      Digest::SHA256.file(tarball).hexdigest
    end
  end
end
