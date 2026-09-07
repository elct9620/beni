// The mruby release the discovered archive states, read from the
// header tree staged beside it.
//
// A build script is outside `cargo test`'s reach, so the parse lives
// here and both `build.rs` and the library's test build include it.
// Names are written in full because the two including scopes import
// different ones.

/// The oldest mruby release the crates build against. Nothing bounds
/// the other end: the FFI surface follows the discovered archive's own
/// headers, so a later release shows what it changed as a compile
/// failure rather than as something a declared ceiling foresaw.
const SUPPORTED_MRUBY_FLOOR: (u32, u32) = (4, 0);

/// The `(major, minor)` release the archive's headers state. The
/// version is the archive's own account of itself, so headers that
/// state none — absent, or declaring neither number — fail loudly
/// rather than letting the build proceed against an unknown release.
fn parse_mruby_release(include_root: &std::path::Path) -> (u32, u32) {
    let version_h = include_root.join("mruby").join("version.h");
    let unreadable = || {
        panic!(
            "beni-sys: {} states no mruby version. The discovered archive's \
             headers must declare MRUBY_RELEASE_MAJOR and \
             MRUBY_RELEASE_MINOR for the build to know what it is compiling \
             against.",
            version_h.display()
        )
    };
    let Ok(header) = std::fs::read_to_string(&version_h) else {
        unreadable()
    };
    let release_number = |key: &str| {
        header.lines().find_map(|line| {
            line.strip_prefix("#define")?
                .trim_start()
                .strip_prefix(key)?
                .split_whitespace()
                .next()?
                .parse::<u32>()
                .ok()
        })
    };
    match (
        release_number("MRUBY_RELEASE_MAJOR"),
        release_number("MRUBY_RELEASE_MINOR"),
    ) {
        (Some(major), Some(minor)) => (major, minor),
        _ => unreadable(),
    }
}

/// Stop a build whose archive predates what the crates build against,
/// naming the release it found. The floor is checked before bindgen
/// runs so an old archive is answered by its version rather than by
/// whichever symbol the wrapper reaches for first.
fn require_supported_mruby(include_root: &std::path::Path) {
    let (major, minor) = parse_mruby_release(include_root);
    let (floor_major, floor_minor) = SUPPORTED_MRUBY_FLOOR;
    if (major, minor) < (floor_major, floor_minor) {
        panic!(
            "beni-sys: the discovered archive is mruby {major}.{minor}, below \
             the supported floor of {floor_major}.{floor_minor}. Build the \
             archive from mruby {floor_major}.{floor_minor} or later — for the \
             vendored chain, set the build config's `version` and re-run \
             `bundle exec rake beni:build`."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_mruby_release, require_supported_mruby};

    /// An include root holding one `mruby/version.h` with the given
    /// body, named after the case so concurrent tests cannot collide.
    fn include_root(case: &str, contents: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("beni-sys-version-{}-{}", std::process::id(), case));
        let mruby = root.join("mruby");
        std::fs::create_dir_all(&mruby).expect("the case directory is creatable");
        std::fs::write(mruby.join("version.h"), contents).expect("the header is writable");
        root
    }

    /// An include root with no `mruby/version.h` in it at all.
    fn headerless_root(case: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("beni-sys-version-{}-{}", std::process::id(), case));
        std::fs::create_dir_all(root.join("mruby")).expect("the case directory is creatable");
        let _ = std::fs::remove_file(root.join("mruby").join("version.h"));
        root
    }

    const VERSION_H: &str = concat!(
        "#ifndef MRUBY_VERSION_H\n",
        "#define MRUBY_VERSION_H\n",
        "#define MRUBY_RELEASE_MAJOR 4\n",
        "#define MRUBY_RELEASE_MINOR 0\n",
        "#define MRUBY_RELEASE_TEENY 0\n",
    );

    #[test]
    fn the_archives_release_is_read_from_its_headers() {
        let root = include_root("read", VERSION_H);
        assert_eq!(parse_mruby_release(&root), (4, 0));
    }

    #[test]
    fn a_release_at_the_floor_builds() {
        require_supported_mruby(&include_root("floor", VERSION_H));
    }

    #[test]
    fn a_release_above_the_floor_builds() {
        let root = include_root(
            "above",
            "#define MRUBY_RELEASE_MAJOR 5\n#define MRUBY_RELEASE_MINOR 2\n",
        );
        require_supported_mruby(&root);
    }

    #[test]
    #[should_panic(expected = "is mruby 3.4, below the supported floor of 4.0")]
    fn a_release_below_the_floor_stops_the_build_naming_itself() {
        let root = include_root(
            "below",
            "#define MRUBY_RELEASE_MAJOR 3\n#define MRUBY_RELEASE_MINOR 4\n",
        );
        require_supported_mruby(&root);
    }

    #[test]
    #[should_panic(expected = "states no mruby version")]
    fn headers_declaring_no_release_stop_the_build() {
        let root = include_root("no-define", "#define MRUBY_VERSION_H\n");
        require_supported_mruby(&root);
    }

    #[test]
    #[should_panic(expected = "states no mruby version")]
    fn a_header_tree_without_the_version_header_stops_the_build() {
        require_supported_mruby(&headerless_root("absent"));
    }
}
