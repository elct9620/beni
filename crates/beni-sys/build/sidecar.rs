// The compile-flags sidecar `libmruby.flags.mak` — the sole channel
// through which an archive tells the crates how it was built.
//
// A build script is outside `cargo test`'s reach, so the parse lives
// here and both `build.rs` and the library's test build include it.
// Names are written in full because the two including scopes import
// different ones.

/// Extract the `-D` defines from the `libmruby.flags.mak` sidecar in
/// `lib_dir` — the flags the discovered archive was actually compiled
/// with. bindgen and the trampoline compile must see the same set or
/// the `mrb_value` layout silently diverges from the archive, so a
/// discovered archive without its sidecar fails loudly instead of
/// guessing. (`MRUBY_CFLAGS = ...` is plain space-separated tokens;
/// only the `-D` ones matter here — `build.rs` constructs include
/// paths and target flags independently.)
fn parse_abi_defines(lib_dir: &std::path::Path) -> Vec<String> {
    let flags_mak = lib_dir.join("libmruby.flags.mak");
    let content = std::fs::read_to_string(&flags_mak).unwrap_or_else(|_| {
        panic!(
            "beni-sys: {} is missing. The discovered libmruby.a's compile flags are \
             unknown, so bindgen cannot be aligned with the archive. Re-run \
             `bundle exec rake beni:build` (which requests the sidecar), or for \
             an externally built archive invoke mruby's rake with the sidecar's \
             file task — `rake <build_dir>/lib/libmruby.flags.mak`.",
            flags_mak.display()
        )
    });
    let cflags = content
        .lines()
        .find_map(|line| line.strip_prefix("MRUBY_CFLAGS = "))
        .unwrap_or_else(|| {
            panic!(
                "beni-sys: {} has no `MRUBY_CFLAGS = ` line — unrecognized \
                 flags.mak layout",
                flags_mak.display()
            )
        });
    // The sidecar is the sole ABI channel: a layout the token scan
    // would silently mis-read — a make continuation line, or a quoted
    // `-D` value whose spaces the whitespace split severs — fails
    // loudly instead of dropping or corrupting flags.
    let quoted_define = cflags
        .split_whitespace()
        .any(|token| token.starts_with("-D") && (token.contains('"') || token.contains('\'')));
    if cflags.trim_end().ends_with('\\') || quoted_define {
        panic!(
            "beni-sys: {} carries a continuation line or quoted `-D` value in \
             `MRUBY_CFLAGS` — unrecognized flags.mak layout",
            flags_mak.display()
        );
    }
    cflags
        .split_whitespace()
        .filter(|token| token.starts_with("-D"))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::parse_abi_defines;

    /// A directory holding one sidecar with the given `MRUBY_CFLAGS`
    /// body, named after the case so concurrent tests cannot collide.
    fn sidecar_dir(case: &str, contents: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "beni-sys-sidecar-{}-{}",
            std::process::id(),
            case
        ));
        std::fs::create_dir_all(&dir).expect("the case directory is creatable");
        std::fs::write(dir.join("libmruby.flags.mak"), contents)
            .expect("the sidecar is writable");
        dir
    }

    /// A directory with no sidecar in it at all.
    fn empty_dir(case: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "beni-sys-sidecar-{}-{}",
            std::process::id(),
            case
        ));
        std::fs::create_dir_all(&dir).expect("the case directory is creatable");
        let _ = std::fs::remove_file(dir.join("libmruby.flags.mak"));
        dir
    }

    const HOST_SIDECAR: &str = concat!(
        "# GNU make is required to use this file.\n",
        "MRUBY_CC = gcc\n",
        "MRUBY_CFLAGS = -std=gnu99 -g -O3 -Wall -DMRB_INT32",
        " -DMRB_WORDBOX_NO_INLINE_FLOAT -I\"$(MRUBY_PACKAGE_DIR)/include\"\n",
        "MRUBY_LIBS = -lmruby -lm\n",
    );

    #[test]
    fn the_archives_defines_are_read_from_its_cflags() {
        let dir = sidecar_dir("host", HOST_SIDECAR);
        assert_eq!(
            parse_abi_defines(&dir),
            vec![
                "-DMRB_INT32".to_owned(),
                "-DMRB_WORDBOX_NO_INLINE_FLOAT".to_owned(),
            ],
            "every -D token is carried and nothing else is"
        );
    }

    #[test]
    fn a_quoted_include_path_is_not_mistaken_for_a_quoted_define() {
        // `-I"$(MRUBY_PACKAGE_DIR)/include"` carries quotes but is not
        // a define, so it must not trip the layout guard.
        let dir = sidecar_dir("quoted-include", HOST_SIDECAR);
        assert_eq!(parse_abi_defines(&dir).len(), 2);
    }

    /// A cross build's sidecar carries toolchain and codegen flags
    /// that decide ABI as surely as a define does. The parse reaches
    /// only the `-D` tokens, so this pins which flags reach the crate
    /// and which do not.
    #[test]
    fn flags_other_than_defines_are_dropped() {
        let dir = sidecar_dir(
            "wasi",
            "MRUBY_CFLAGS = --target=wasm32-wasip1 --sysroot=/opt/wasi-sdk/share/wasi-sysroot \
             -mllvm -wasm-use-legacy-eh=false -DMRB_INT32\n",
        );
        assert_eq!(parse_abi_defines(&dir), vec!["-DMRB_INT32".to_owned()]);
    }

    #[test]
    #[should_panic(expected = "is missing")]
    fn a_missing_sidecar_fails_naming_the_path() {
        parse_abi_defines(&empty_dir("absent"));
    }

    #[test]
    #[should_panic(expected = "no `MRUBY_CFLAGS = ` line")]
    fn a_sidecar_without_a_cflags_line_fails() {
        let dir = sidecar_dir("no-cflags", "MRUBY_CC = gcc\nMRUBY_LIBS = -lmruby\n");
        parse_abi_defines(&dir);
    }

    #[test]
    #[should_panic(expected = "continuation line or quoted")]
    fn a_continuation_line_fails_rather_than_dropping_the_rest() {
        let dir = sidecar_dir("continuation", "MRUBY_CFLAGS = -DMRB_INT32 \\\n  -DMRB_UTF8\n");
        parse_abi_defines(&dir);
    }

    #[test]
    #[should_panic(expected = "continuation line or quoted")]
    fn a_quoted_define_fails_rather_than_severing_its_value() {
        let dir = sidecar_dir("quoted-define", "MRUBY_CFLAGS = -DMRB_NAME=\"a b\"\n");
        parse_abi_defines(&dir);
    }
}
