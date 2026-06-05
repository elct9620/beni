# frozen_string_literal: true

module Beni
  module Vendor
    # Declarative value object describing a tarball-style vendored
    # toolchain. Captures the +(remote, cache, unpacked)+ triple anchored
    # on +vendor_dir+, and exposes the three pipeline stages (+#fetch+,
    # +#verify+, +#install+) that +Beni::Tasks+ wires into +file+ /
    # +task+ declarations.
    #
    # Adding a new tarball-based vendor artifact is a single factory
    # method in +Beni::Vendor+; the rake DSL loop in +Beni::Tasks+
    # picks it up automatically.
    #
    # Fields:
    #
    #   * +name+          — display name; also the basename of the unpacked
    #                       tree under +vendor_dir+ and the base for the
    #                       +setup:<name>+ task identifier.
    #   * +version_label+ — version string; printed in the download log and
    #                       stamped into +final_dir+ as the idempotency key
    #                       that detects a bump and forces a re-extract.
    #   * +base_url+      — remote URL prefix; resolved through
    #                       +Beni::Vendor.base_url_for+ so test fixtures
    #                       can override via +BENI_VENDOR_BASE_URL+.
    #   * +tarball_name+  — filename joined to both +base_url+ (download)
    #                       and the +.cache+ directory (cache location).
    #   * +top_level_dir+ — the single top-level directory produced when
    #                       the tarball is extracted; passed through to
    #                       +Tarball#prepare+ under the same name.
    #   * +vendor_dir+    — root of the vendor tree; anchors +final_dir+
    #                       and +tarball_path+.
    class Toolchain < Data.define(:name, :version_label, :base_url, :tarball_name, :top_level_dir, :vendor_dir)
      # Symbol used to identify the +setup:<task_name>+ rake task. Dashes
      # in +name+ are not valid in rake task identifiers, so we map them
      # to underscores at this single seam.
      def task_name
        name.tr("-", "_").to_sym
      end

      # Upper-snake-case artifact slug used by +Beni::Vendor.expected_sha256+
      # to look up the +BENI_VENDOR_<KEY>_SHA256+ environment variable,
      # e.g. +"WASI_SDK"+, +"MRUBY"+.
      def sha_key
        name.tr("-", "_").upcase
      end

      # Resolved download URL. Honours the +BENI_VENDOR_BASE_URL+ test
      # fixture override at call time (not at construction time), so a
      # test can flip the env var after the Toolchain is built.
      def url
        "#{Vendor.base_url_for(base_url)}/#{tarball_name}"
      end

      # Destination under +vendor_dir+ where the unpacked tree is moved.
      def final_dir
        File.join(vendor_dir, name)
      end

      # Local cache path for the downloaded tarball. Lives under
      # +vendor_dir/.cache+ (the cache moves with the vendor tree).
      def tarball_path
        File.join(vendor_dir, ".cache", tarball_name)
      end

      # Download the tarball into +tarball_path+ and verify its SHA256.
      # Intended as the body of the +file tarball_path+ rake task; the
      # task's mtime-based caching avoids re-downloading on a cache hit.
      def fetch
        puts "[beni] downloading #{name} #{version_label} from #{url}"
        Downloader.new(url, tarball_path).download
        verify
      end

      # Recompute the cached tarball's SHA256 and check it against the
      # expected hash (or pin via TOFU sidecar). Idempotent — safe to
      # call from both +file+ and +setup+ task bodies when the latter
      # depends on the former.
      def verify
        Checksum.new(tarball_path, Vendor.expected_sha256(sha_key)).verify_or_pin
      end

      # Verify the cached tarball, then unpack it into +final_dir+ via
      # +Tarball#prepare+. A no-op when the version stamped under +final_dir+
      # already matches +version_label+.
      def install
        verify
        Tarball.new(
          tarball: tarball_path,
          top_level_dir: top_level_dir,
          final_dir: final_dir,
          version: version_label
        ).prepare
        puts "[beni] #{name} ready at #{final_dir}"
      end
    end
  end
end
