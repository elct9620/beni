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

/// The compile flags the discovered archive was actually built with,
/// less the three `build.rs` derives from the archive itself: the
/// target, the sysroot, and the include root. bindgen and the
/// trampoline compile see everything else the archive saw, so no flag
/// that shapes the generated code is left behind.
///
/// A quoted value has spaces the whitespace split would sever, so a
/// flag carrying one stops the read rather than reaching a compiler
/// in pieces.
fn parse_compile_flags(lib_dir: &std::path::Path) -> Vec<String> {
    let flags: Vec<String> = sidecar_line(lib_dir, "MRUBY_CFLAGS")
        .split_whitespace()
        .filter(|token| {
            !token.starts_with("--target")
                && !token.starts_with("--sysroot")
                && !token.starts_with("-I")
        })
        .map(str::to_owned)
        .collect();
    if let Some(quoted) = flags
        .iter()
        .find(|token| token.contains('"') || token.contains('\''))
    {
        panic!(
            "beni-sys: {} carries a quoted value in `MRUBY_CFLAGS` ({quoted}) — \
             unrecognized flags.mak layout",
            lib_dir.join("libmruby.flags.mak").display()
        );
    }
    flags
}

/// The libraries the archive needs linked, named by the sidecar as
/// `-l` tokens and yielded without the prefix. The archive states its
/// own link set, so a configuration that pulls in a further library is
/// served without the crate being taught about it.
///
/// mruby writes every token on the line through the linker option its
/// toolchain defines, so a token in any other shape names a link set
/// this read would carry only in part; it stops rather than linking
/// against less than the archive names.
fn parse_link_libs(lib_dir: &std::path::Path) -> Vec<String> {
    sidecar_line(lib_dir, "MRUBY_LIBS")
        .split_whitespace()
        .map(|token| {
            token
                .strip_prefix("-l")
                .filter(|name| !name.is_empty() && !name.contains(['"', '\'']))
                .unwrap_or_else(|| {
                    panic!(
                        "beni-sys: {} names `{token}` in `MRUBY_LIBS`, which is not a \
                         `-l<name>` library — unrecognized flags.mak layout",
                        lib_dir.join("libmruby.flags.mak").display()
                    )
                })
                .to_owned()
        })
        .collect()
}

/// The toolchain root the archive was built against, derived from the
/// compiler the sidecar names. A cross build's sidecar names it as
/// `<root>/bin/<compiler>`; a host build names a compiler off `PATH`
/// with no root to derive, and yields `None`.
fn parse_toolchain_root(lib_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let cc = sidecar_line(lib_dir, "MRUBY_CC");
    let bin = std::path::Path::new(cc.trim()).parent()?;
    if bin.file_name()? != "bin" {
        return None;
    }
    Some(bin.parent()?.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::{parse_compile_flags, parse_link_libs, parse_toolchain_root};

    /// A directory holding one sidecar with the given `MRUBY_CFLAGS`
    /// body, named after the case so concurrent tests cannot collide.
    fn sidecar_dir(case: &str, contents: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("beni-sys-sidecar-{}-{}", std::process::id(), case));
        std::fs::create_dir_all(&dir).expect("the case directory is creatable");
        std::fs::write(dir.join("libmruby.flags.mak"), contents).expect("the sidecar is writable");
        dir
    }

    /// A directory with no sidecar in it at all.
    fn empty_dir(case: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("beni-sys-sidecar-{}-{}", std::process::id(), case));
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
    fn the_archives_compile_flags_are_read_from_its_cflags() {
        let dir = sidecar_dir("host", HOST_SIDECAR);
        assert_eq!(
            parse_compile_flags(&dir),
            vec![
                "-std=gnu99".to_owned(),
                "-g".to_owned(),
                "-O3".to_owned(),
                "-Wall".to_owned(),
                "-DMRB_INT32".to_owned(),
                "-DMRB_WORDBOX_NO_INLINE_FLOAT".to_owned(),
            ],
            "every flag is carried but the include root build.rs derives"
        );
    }

    #[test]
    fn a_cross_builds_codegen_flags_are_carried_and_its_own_target_is_not() {
        // `-mllvm -wasm-use-legacy-eh=false` decides how setjmp
        // compiles, so it reaches the trampoline compile; the target
        // and sysroot are the archive's own paths, which build.rs
        // derives from the discovered lib dir instead.
        let dir = sidecar_dir(
            "wasi",
            "MRUBY_CFLAGS = --target=wasm32-wasip1 --sysroot=/opt/wasi-sdk/share/wasi-sysroot \
             -mllvm -wasm-use-legacy-eh=false -DMRB_INT32\n",
        );
        assert_eq!(
            parse_compile_flags(&dir),
            vec![
                "-mllvm".to_owned(),
                "-wasm-use-legacy-eh=false".to_owned(),
                "-DMRB_INT32".to_owned(),
            ],
            "a flag and the value it governs stay together and in order"
        );
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
        let dir = sidecar_dir("extra-libs", "MRUBY_LIBS = -lmruby -lm -lsetjmp -lz\n");
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
    #[should_panic(expected = "is not a `-l<name>` library")]
    fn a_quoted_library_fails_rather_than_naming_one_with_its_quotes() {
        let dir = sidecar_dir("libs-quoted", "MRUBY_LIBS = -l\"mruby\" -l\"m\"\n");
        parse_link_libs(&dir);
    }

    #[test]
    #[should_panic(expected = "is not a `-l<name>` library")]
    fn a_library_named_without_the_l_option_fails_rather_than_linking_none() {
        let dir = sidecar_dir("libs-msvc", "MRUBY_LIBS = libmruby.lib\n");
        parse_link_libs(&dir);
    }

    #[test]
    #[should_panic(expected = "continuation line in `MRUBY_LIBS`")]
    fn a_continuation_in_the_link_set_fails_rather_than_dropping_a_library() {
        let dir = sidecar_dir("libs-continuation", "MRUBY_LIBS = -lmruby \\\n  -lm\n");
        parse_link_libs(&dir);
    }

    #[test]
    fn a_cross_builds_toolchain_root_is_derived_from_its_compiler() {
        let dir = sidecar_dir(
            "cc-cross",
            "MRUBY_CC = /opt/wasi-sdk/bin/clang\nMRUBY_CFLAGS = -DMRB_INT32\n",
        );
        assert_eq!(
            parse_toolchain_root(&dir),
            Some(std::path::PathBuf::from("/opt/wasi-sdk"))
        );
    }

    #[test]
    fn a_host_build_names_a_compiler_with_no_root_to_derive() {
        let dir = sidecar_dir("cc-host", "MRUBY_CC = gcc\nMRUBY_CFLAGS = -DMRB_INT32\n");
        assert_eq!(parse_toolchain_root(&dir), None);
    }

    #[test]
    fn a_compiler_outside_a_bin_directory_yields_no_root() {
        // Only the `<root>/bin/<compiler>` shape names a root; anything
        // else is read as naming none rather than as naming its parent.
        let dir = sidecar_dir(
            "cc-loose",
            "MRUBY_CC = /usr/local/clang\nMRUBY_CFLAGS = -DMRB_INT32\n",
        );
        assert_eq!(parse_toolchain_root(&dir), None);
    }

    #[test]
    #[should_panic(expected = "is missing")]
    fn a_missing_sidecar_fails_naming_the_path() {
        parse_compile_flags(&empty_dir("absent"));
    }

    #[test]
    #[should_panic(expected = "has no `MRUBY_CFLAGS = ` line")]
    fn a_sidecar_without_a_cflags_line_fails() {
        let dir = sidecar_dir("no-cflags", "MRUBY_CC = gcc\nMRUBY_LIBS = -lmruby\n");
        parse_compile_flags(&dir);
    }

    #[test]
    #[should_panic(expected = "continuation line in `MRUBY_CFLAGS`")]
    fn a_continuation_line_fails_rather_than_dropping_the_rest() {
        let dir = sidecar_dir(
            "continuation",
            "MRUBY_CFLAGS = -DMRB_INT32 \\\n  -DMRB_UTF8\n",
        );
        parse_compile_flags(&dir);
    }

    #[test]
    #[should_panic(expected = "carries a quoted value")]
    fn a_quoted_value_fails_rather_than_reaching_a_compiler_in_pieces() {
        let dir = sidecar_dir("quoted-define", "MRUBY_CFLAGS = -DMRB_NAME=\"a b\"\n");
        parse_compile_flags(&dir);
    }
}
