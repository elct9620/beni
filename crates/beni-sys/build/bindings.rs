// The bindings a build uses, read once for the build script's
// configuration checks, and the constants they declare.
//
// A build script is outside `cargo test`'s reach, so the read lives
// here and both `build.rs` and the library's test build include it.

/// The value of the `u32` constant `name` that `bindings` declares, read
/// from the `pub const name: u32 = value;` line bindgen writes for it.
pub(crate) fn declared_u32(bindings: &str, name: &str) -> Option<u32> {
    bindings.lines().find_map(|line| {
        line.trim_start()
            .strip_prefix("pub const ")?
            .strip_prefix(name)?
            .strip_prefix(':')?
            .split_once('=')?
            .1
            .trim()
            .strip_suffix(';')?
            .parse::<u32>()
            .ok()
    })
}

/// The bindings a build generated, read once for every check, with the
/// path each check names when it stops the build.
pub(crate) struct Bindings {
    pub(crate) path: std::path::PathBuf,
    pub(crate) text: String,
}

impl Bindings {
    /// Read the bindings at `path`. A build whose bindings cannot be read
    /// stops here, before any check guesses at what they declare.
    pub(crate) fn read(path: &std::path::Path) -> Self {
        let text = std::fs::read_to_string(path).unwrap_or_else(|err| {
            panic!(
                "beni-sys: cannot read the bindings at {}: {err}",
                path.display()
            )
        });
        Self {
            path: path.to_owned(),
            text,
        }
    }
}

/// Bindings declaring `contents`, named after `case` in the messages a
/// check stops the build with.
#[cfg(test)]
pub(crate) fn bindings(case: &str, contents: &str) -> Bindings {
    Bindings {
        path: std::path::PathBuf::from(case),
        text: contents.to_owned(),
    }
}

#[cfg(test)]
mod read_tests {
    use super::Bindings;

    #[test]
    fn readable_bindings_are_read_whole() {
        let path = std::env::temp_dir().join(format!(
            "beni-sys-bindings-{}-readable.rs",
            std::process::id()
        ));
        std::fs::write(&path, "pub const MRB_INT_BIT: u32 = 64;\n").expect("writable");
        assert_eq!(
            Bindings::read(&path).text,
            "pub const MRB_INT_BIT: u32 = 64;\n"
        );
    }

    #[test]
    #[should_panic(expected = "cannot read the bindings")]
    fn absent_bindings_stop_the_build() {
        let path = std::env::temp_dir().join(format!(
            "beni-sys-bindings-{}-absent.rs",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        Bindings::read(&path);
    }
}
