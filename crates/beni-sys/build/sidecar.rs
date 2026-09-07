// The compile-flags sidecar `libmruby.flags.mak` — the sole channel
// through which an archive tells the crates how it was built.
//
// A build script is outside `cargo test`'s reach, so the parse lives
// here and both `build.rs` and the library's test build include it.
// Names are written in full because the two including scopes import
// different ones.

/// The value of one `NAME = ...` line in the `libmruby.flags.mak`
/// sidecar in `lib_dir`. The sidecar is the sole channel through which
/// an archive states how it was built, so a sidecar that is absent, is
/// missing the line, or carries a layout whose tokens the whitespace
/// split would mis-read fails loudly rather than yielding a partial
/// set.
fn sidecar_line(lib_dir: &std::path::Path, key: &str) -> String {
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
    let prefix = format!("{key} = ");
    let value = content
        .lines()
        .find_map(|line| line.strip_prefix(prefix.as_str()))
        .unwrap_or_else(|| {
            panic!(
                "beni-sys: {} has no `{prefix}` line — unrecognized flags.mak layout",
                flags_mak.display()
            )
        });
    // A make continuation line means the value carries on past what
    // this read sees, so the read stops rather than acting on a
    // fragment of it.
    if value.trim_end().ends_with('\\') {
        panic!(
            "beni-sys: {} carries a continuation line in `{key}` — unrecognized \
             flags.mak layout",
            flags_mak.display()
        );
    }
    value.to_owned()
}

/// Extract the `-D` defines from the sidecar's compile flags — the
/// flags the discovered archive was actually compiled with. bindgen and
/// the trampoline compile must see the same set or the `mrb_value`
/// layout silently diverges from the archive. (`MRUBY_CFLAGS = ...` is
/// plain space-separated tokens; only the `-D` ones matter here —
/// `build.rs` constructs include paths and target flags independently.)
fn parse_abi_defines(lib_dir: &std::path::Path) -> Vec<String> {
    let cflags = sidecar_line(lib_dir, "MRUBY_CFLAGS");
    // A quoted `-D` value has spaces the whitespace split would sever,
    // so it stops the read rather than corrupting the define.
    if cflags
        .split_whitespace()
        .any(|token| token.starts_with("-D") && (token.contains('"') || token.contains('\'')))
    {
        panic!(
            "beni-sys: {} carries a quoted `-D` value in `MRUBY_CFLAGS` — \
             unrecognized flags.mak layout",
            lib_dir.join("libmruby.flags.mak").display()
        );
    }
    cflags
        .split_whitespace()
        .filter(|token| token.starts_with("-D"))
        .map(str::to_owned)
        .collect()
}

/// The libraries the archive needs linked, named by the sidecar as
/// `-l` tokens and yielded without the prefix. The archive states its
/// own link set, so a configuration that pulls in a further library is
/// served without the crate being taught about it.
fn parse_link_libs(lib_dir: &std::path::Path) -> Vec<String> {
    sidecar_line(lib_dir, "MRUBY_LIBS")
        .split_whitespace()
        .filter_map(|token| token.strip_prefix("-l"))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{parse_abi_defines, parse_link_libs};

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
    fn the_archives_link_set_is_read_from_its_libs() {
        let dir = sidecar_dir("host-libs", HOST_SIDECAR);
        assert_eq!(
            parse_link_libs(&dir),
            vec!["mruby".to_owned(), "m".to_owned()],
            "every -l token is carried, without its prefix"
        );
    }

    #[test]
    fn a_link_set_carries_whatever_the_configuration_added() {
        // A cross build's sidecar names the toolchain library its
        // exception mechanism needs; a gem's configuration could name
        // any other. Neither is known to the crate.
        let dir = sidecar_dir(
            "extra-libs",
            "MRUBY_LIBS = -lmruby -lm -lsetjmp -lz\n",
        );
        assert_eq!(
            parse_link_libs(&dir),
            vec![
                "mruby".to_owned(),
                "m".to_owned(),
                "setjmp".to_owned(),
                "z".to_owned(),
            ]
        );
    }

    #[test]
    #[should_panic(expected = "has no `MRUBY_LIBS = ` line")]
    fn a_sidecar_without_a_libs_line_fails() {
        let dir = sidecar_dir("no-libs", "MRUBY_CFLAGS = -DMRB_INT32\n");
        parse_link_libs(&dir);
    }

    #[test]
    #[should_panic(expected = "continuation line in `MRUBY_LIBS`")]
    fn a_continuation_in_the_link_set_fails_rather_than_dropping_a_library() {
        let dir = sidecar_dir("libs-continuation", "MRUBY_LIBS = -lmruby \\\n  -lm\n");
        parse_link_libs(&dir);
    }

    #[test]
    #[should_panic(expected = "is missing")]
    fn a_missing_sidecar_fails_naming_the_path() {
        parse_abi_defines(&empty_dir("absent"));
    }

    #[test]
    #[should_panic(expected = "has no `MRUBY_CFLAGS = ` line")]
    fn a_sidecar_without_a_cflags_line_fails() {
        let dir = sidecar_dir("no-cflags", "MRUBY_CC = gcc\nMRUBY_LIBS = -lmruby\n");
        parse_abi_defines(&dir);
    }

    #[test]
    #[should_panic(expected = "continuation line in `MRUBY_CFLAGS`")]
    fn a_continuation_line_fails_rather_than_dropping_the_rest() {
        let dir = sidecar_dir("continuation", "MRUBY_CFLAGS = -DMRB_INT32 \\\n  -DMRB_UTF8\n");
        parse_abi_defines(&dir);
    }

    #[test]
    #[should_panic(expected = "quoted `-D` value")]
    fn a_quoted_define_fails_rather_than_severing_its_value() {
        let dir = sidecar_dir("quoted-define", "MRUBY_CFLAGS = -DMRB_NAME=\"a b\"\n");
        parse_abi_defines(&dir);
    }
}
