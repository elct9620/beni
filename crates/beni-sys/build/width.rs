// The configured integer width the bindings a build uses declare,
// published to the crates that depend on this one directly.
//
// A build script is outside `cargo test`'s reach, so the parse lives
// here and both `build.rs` and the library's test build include it.
// Names are written in full because the two including scopes import
// different ones.

/// The `links` metadata key carrying the configured integer width —
/// `true` for 64 bits, `false` for 32 — which a direct dependent's build
/// reads as `DEP_MRUBY_DEFINES_MRB_INT64`.
const INTEGER_WIDTH_METADATA: &str = "defines_mrb_int64";

/// Whether the bindings declare a 64-bit `mrb_int`, read from the
/// `MRB_INT_BIT` constant mruby settles the width into. Bindings that
/// declare no width, or one other than 32 or 64, fail loudly rather than
/// letting a dependent build pick its conversions against a guess.
fn declares_mrb_int64(bindings_rs: &std::path::Path) -> bool {
    let undeclared = || {
        panic!(
            "beni-sys: {} declares no integer width. The bindings must carry \
             `MRB_INT_BIT` as 32 or 64 for the crates above to know which \
             integer conversions the archive supports.",
            bindings_rs.display()
        )
    };
    let Ok(bindings) = std::fs::read_to_string(bindings_rs) else {
        undeclared()
    };
    let bits = bindings.lines().find_map(|line| {
        line.trim_start()
            .strip_prefix("pub const MRB_INT_BIT:")?
            .split_once('=')?
            .1
            .trim()
            .strip_suffix(';')?
            .parse::<u32>()
            .ok()
    });
    match bits {
        Some(64) => true,
        Some(32) => false,
        _ => undeclared(),
    }
}

/// The build-script directive publishing the configured integer width
/// the bindings at `bindings_rs` declare as the integer-width metadata.
fn integer_width_directive(bindings_rs: &std::path::Path) -> String {
    format!(
        "cargo:{INTEGER_WIDTH_METADATA}={}",
        declares_mrb_int64(bindings_rs)
    )
}

#[cfg(test)]
mod tests {
    use super::integer_width_directive;

    /// A bindings file with the given body, named after the case so
    /// concurrent tests cannot collide.
    fn bindings(case: &str, contents: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("beni-sys-width-{}-{}", std::process::id(), case));
        std::fs::create_dir_all(&dir).expect("the case directory is creatable");
        let path = dir.join("bindings.rs");
        std::fs::write(&path, contents).expect("the bindings are writable");
        path
    }

    #[test]
    fn a_64_bit_width_is_read_from_the_bindings() {
        let path = bindings(
            "64",
            "pub const MRB_INT_BIT: u32 = 64;\npub type mrb_int = i64;\n",
        );
        assert_eq!(
            integer_width_directive(&path),
            "cargo:defines_mrb_int64=true"
        );
    }

    #[test]
    fn a_32_bit_width_is_read_from_the_bindings() {
        let path = bindings(
            "32",
            "pub const MRB_INT_BIT: u32 = 32;\npub type mrb_int = i32;\n",
        );
        assert_eq!(
            integer_width_directive(&path),
            "cargo:defines_mrb_int64=false"
        );
    }

    #[test]
    #[should_panic(expected = "declares no integer width")]
    fn bindings_declaring_no_width_stop_the_build() {
        integer_width_directive(&bindings("none", "pub type mrb_int = i64;\n"));
    }

    #[test]
    #[should_panic(expected = "declares no integer width")]
    fn a_width_other_than_32_or_64_stops_the_build() {
        integer_width_directive(&bindings("16", "pub const MRB_INT_BIT: u32 = 16;\n"));
    }

    #[test]
    #[should_panic(expected = "declares no integer width")]
    fn absent_bindings_stop_the_build() {
        let path = bindings("absent", "");
        std::fs::remove_file(&path).expect("the bindings are removable");
        integer_width_directive(&path);
    }
}
