// The compile-flags sidecar `libmruby.flags.mak` — the sole channel
// through which an archive tells the crates how it was built.
//
// A build script is outside `cargo test`'s reach, so the parse lives
// here and both `build.rs` and the library's test build include it.
// Names are written in full because the two including scopes import
// different ones.

/// The file name mruby gives the sidecar it writes beside every
/// archive. SPEC's Terminology defines it; this is the read end.
const FLAGS_MAK: &str = "libmruby.flags.mak";

/// The `libmruby.flags.mak` sidecar beside an archive, read once for
/// every value the build takes from it, with the path each read names
/// when it stops the build.
struct Sidecar {
    path: std::path::PathBuf,
    text: String,
}

impl Sidecar {
    /// Read the sidecar in `lib_dir`. It is the sole channel through
    /// which an archive states how it was built, so an absent one stops
    /// the build rather than leaving the flags to be guessed.
    fn read(lib_dir: &std::path::Path) -> Self {
        let path = lib_dir.join(FLAGS_MAK);
        let text = std::fs::read_to_string(&path).unwrap_or_else(|_| {
            panic!(
                "beni-sys: {} is missing. The discovered archive's compile flags are \
                 unknown, so bindgen cannot be aligned with the archive. Re-run \
                 `bundle exec rake beni:build` (which requests the sidecar), or for \
                 an externally built archive invoke mruby's rake with the sidecar's \
                 file task — `rake <build_dir>/lib/libmruby.flags.mak`.",
                path.display()
            )
        });
        Self { path, text }
    }

    /// The value of one `NAME = ...` line. A sidecar missing the line,
    /// or carrying a layout whose tokens the whitespace split would
    /// mis-read, fails loudly rather than yielding a partial set.
    fn line(&self, key: &str) -> String {
        let prefix = format!("{key} = ");
        let value = self
            .text
            .lines()
            .find_map(|line| line.strip_prefix(prefix.as_str()))
            .unwrap_or_else(|| {
                panic!(
                    "beni-sys: {} has no `{prefix}` line — unrecognized flags.mak layout",
                    self.path.display()
                )
            });
        // A make continuation line means the value carries on past what
        // this read sees, so the read stops rather than acting on a
        // fragment of it.
        if value.trim_end().ends_with('\\') {
            panic!(
                "beni-sys: {} carries a continuation line in `{key}` — unrecognized \
                 flags.mak layout",
                self.path.display()
            );
        }
        value.to_owned()
    }
}

/// The compile flags the discovered archive was actually built with,
/// less its include path, which names the header tree through a make
/// variable no reader outside make can expand; `build.rs` substitutes
/// the tree staged beside the archive. Everything else the archive saw
/// reaches the trampoline compile, which uses the compiler these flags
/// were written for.
///
/// A quoted value has spaces the whitespace split would sever, so a
/// flag carrying one stops the read rather than reaching a compiler
/// in pieces.
fn parse_compile_flags(sidecar: &Sidecar) -> Vec<String> {
    let flags: Vec<String> = sidecar
        .line("MRUBY_CFLAGS")
        .split_whitespace()
        .filter(|token| !names_header_tree(token))
        .map(str::to_owned)
        .collect();
    if let Some(quoted) = flags
        .iter()
        .find(|token| token.contains('"') || token.contains('\''))
    {
        panic!(
            "beni-sys: {} carries a quoted value in `MRUBY_CFLAGS` ({quoted}) — \
             unrecognized flags.mak layout",
            sidecar.path.display()
        );
    }
    flags
}

/// Whether a token names the header tree. `build.rs` substitutes the
/// tree staged beside the archive, so the flag is dropped in either
/// toolchain's spelling of it.
fn names_header_tree(token: &str) -> bool {
    token.starts_with("-I") || token.starts_with("/I")
}

/// The MSVC spelling of each flag deciding what the headers declare,
/// paired with the spelling libclang reads it under.
const DECLARATION_FLAG_SPELLINGS: [(&str, &str); 3] =
    [("/D", "-D"), ("/U", "-U"), ("/std:", "-std=")];

/// One compile flag as binding generation reads it, or `None` when the
/// flag decides how code is generated rather than what the headers
/// declare. A flag already in clang's spelling passes through; the
/// archive's toolchain spelling of the same flag crosses as clang's.
fn declaration_flag(token: &str) -> Option<String> {
    for (msvc, clang) in DECLARATION_FLAG_SPELLINGS {
        if let Some(value) = token.strip_prefix(msvc) {
            return Some(format!("{clang}{value}"));
        }
        if token.starts_with(clang) {
            return Some(token.to_owned());
        }
    }
    None
}

/// Whether a token is a flag spelling with nothing attached, in either
/// toolchain's form — a flag whose value is the next token, which the
/// whitespace split has already severed from it.
fn is_bare_spelling(token: &str) -> bool {
    DECLARATION_FLAG_SPELLINGS
        .iter()
        .any(|(msvc, clang)| token == *msvc || token == *clang)
}

/// The language standard binding generation parses under when the
/// sidecar names none. An archive whose compiler was given no standard
/// was built under that compiler's default, and libclang's own default
/// is not it: below C11 mruby declares its never-returning functions
/// through a compiler attribute, which is the form the archive carries
/// and the one bindgen reads as a diverging function.
const UNNAMED_STANDARD: &str = "-std=gnu99";

/// The flags deciding what the archive's headers declare — macro
/// definitions and removals, and the language standard — in the
/// spelling libclang reads. Binding generation parses with a toolchain
/// that is never the one the sidecar names, so it is held to these
/// alone, and the archive's toolchain spelling of each is handed over
/// as clang's.
///
/// A definition or removal carrying its value in the next token would
/// reach the parse without it, so it stops the read instead.
fn declaration_flags(sidecar: &Sidecar, compile_flags: &[String]) -> Vec<String> {
    if let Some(bare) = compile_flags.iter().find(|token| is_bare_spelling(token)) {
        panic!(
            "beni-sys: {} names a bare `{bare}` in `MRUBY_CFLAGS`, whose value is a \
             separate token — unrecognized flags.mak layout",
            sidecar.path.display()
        );
    }
    let mut flags: Vec<String> = compile_flags
        .iter()
        .filter_map(|token| declaration_flag(token))
        .collect();
    if !flags.iter().any(|flag| flag.starts_with("-std=")) {
        flags.push(UNNAMED_STANDARD.to_owned());
    }
    flags
}

/// The file name the archive's sidecar gives it. mruby writes the path
/// through a make variable no reader outside make can expand, so the
/// name is what is taken from it — the archive itself sits in the
/// directory discovery resolved.
fn parse_archive_file_name(sidecar: &Sidecar) -> String {
    let path = sidecar.line("MRUBY_LIBMRUBY_PATH");
    let name = path.trim().rsplit(['/', '\\']).next().unwrap_or_default();
    if name.is_empty() || name.contains('$') {
        panic!(
            "beni-sys: {} names `{path}` in `MRUBY_LIBMRUBY_PATH`, which carries no \
             file name the archive can be looked for under — unrecognized flags.mak \
             layout",
            sidecar.path.display()
        );
    }
    name.to_owned()
}

/// Whether `name` is the library the archive's own file answers to,
/// under either convention a static library is named by: a GNU-style
/// `lib<name>.a`, or MSVC's `<name>.lib`.
fn names_the_archive(name: &str, archive_file_name: &str) -> bool {
    archive_file_name == format!("lib{name}.a") || archive_file_name == format!("{name}.lib")
}

/// The library name one `MRUBY_LIBS` token carries, in either form
/// mruby's toolchains name a library by: a GNU-style `-l<name>` option,
/// or MSVC's `<name>.lib` file. `None` for a token in neither form.
fn link_lib_name(token: &str) -> Option<&str> {
    let name = token
        .strip_prefix("-l")
        .or_else(|| token.strip_suffix(".lib"))?;
    (!name.is_empty() && !name.contains(['"', '\''])).then_some(name)
}

/// The libraries the archive needs linked, yielded as bare names. The
/// archive states its own link set, so a configuration that pulls in a
/// further library is served without the crate being taught about it.
///
/// mruby writes every token on the line through the linker option its
/// toolchain defines, so a token in neither of those forms names a link
/// set this read would carry only in part; it stops rather than linking
/// against less than the archive names.
fn parse_link_libs(sidecar: &Sidecar) -> Vec<String> {
    sidecar
        .line("MRUBY_LIBS")
        .split_whitespace()
        .map(|token| {
            link_lib_name(token)
                .unwrap_or_else(|| {
                    panic!(
                        "beni-sys: {} names `{token}` in `MRUBY_LIBS`, which is neither a \
                         `-l<name>` option nor a `<name>.lib` file — unrecognized \
                         flags.mak layout",
                        sidecar.path.display()
                    )
                })
                .to_owned()
        })
        .collect()
}

/// The compiler that built the discovered archive. A cross build's
/// sidecar names it by path, a host build by a name off `PATH`; either
/// way it is the compiler the sidecar's flags were written for, and the
/// one the trampoline compile uses.
fn parse_compiler(sidecar: &Sidecar) -> String {
    sidecar.line("MRUBY_CC").trim().to_owned()
}

/// The toolchain root the archive was built against, derived from the
/// compiler the sidecar names. A cross build's sidecar names it as
/// `<root>/bin/<compiler>`; a host build names a compiler off `PATH`
/// with no root to derive, and yields `None`.
fn parse_toolchain_root(sidecar: &Sidecar) -> Option<std::path::PathBuf> {
    let cc = parse_compiler(sidecar);
    let bin = std::path::Path::new(&cc).parent()?;
    if bin.file_name()? != "bin" {
        return None;
    }
    Some(bin.parent()?.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::{
        declaration_flags, names_the_archive, parse_archive_file_name, parse_compile_flags,
        parse_compiler, parse_link_libs, parse_toolchain_root, Sidecar,
    };

    /// A sidecar with the given body, in a directory named after the
    /// case so concurrent tests cannot collide.
    fn sidecar(case: &str, contents: &str) -> Sidecar {
        let dir =
            std::env::temp_dir().join(format!("beni-sys-sidecar-{}-{}", std::process::id(), case));
        std::fs::create_dir_all(&dir).expect("the case directory is creatable");
        std::fs::write(dir.join("libmruby.flags.mak"), contents).expect("the sidecar is writable");
        Sidecar::read(&dir)
    }

    /// A directory with no sidecar in it at all.
    fn empty_dir(case: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("beni-sys-sidecar-{}-{}", std::process::id(), case));
        std::fs::create_dir_all(&dir).expect("the case directory is creatable");
        let _ = std::fs::remove_file(dir.join("libmruby.flags.mak"));
        dir
    }

    /// What mruby's visualcpp toolchain writes: the archive named as a
    /// file, and every flag in MSVC's spelling.
    const MSVC_SIDECAR: &str = concat!(
        "# GNU make is required to use this file.\n",
        "MRUBY_CC = cl.exe\n",
        "MRUBY_CFLAGS = /nologo /W3 /MD /O2 /D_CRT_SECURE_NO_WARNINGS /we4013",
        " /DMRB_STACK_EXTEND_DOUBLING /DMRB_INT32 /I\"$(MRUBY_PACKAGE_DIR)/include\"\n",
        "MRUBY_LIBS = libmruby.lib\n",
        "MRUBY_LIBMRUBY_PATH = $(MRUBY_PACKAGE_DIR)\\lib\\libmruby.lib\n",
    );

    const HOST_SIDECAR: &str = concat!(
        "# GNU make is required to use this file.\n",
        "MRUBY_CC = gcc\n",
        "MRUBY_CFLAGS = -std=gnu99 -g -O3 -Wall -DMRB_INT32",
        " -DMRB_WORDBOX_NO_INLINE_FLOAT -I\"$(MRUBY_PACKAGE_DIR)/include\"\n",
        "MRUBY_LIBS = -lmruby -lm\n",
        "MRUBY_LIBMRUBY_PATH = $(MRUBY_PACKAGE_DIR)/lib/libmruby.a\n",
    );

    #[test]
    fn the_archives_compile_flags_are_read_from_its_cflags() {
        let sidecar = sidecar("host", HOST_SIDECAR);
        assert_eq!(
            parse_compile_flags(&sidecar),
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
    fn a_cross_builds_flags_reach_the_trampoline_compile_whole() {
        // The target and sysroot belong to the compiler the sidecar
        // names, and `-mllvm -wasm-use-legacy-eh=false` decides how
        // setjmp compiles; the trampoline compile uses that compiler,
        // so all of them reach it.
        let sidecar = sidecar(
            "wasi",
            "MRUBY_CFLAGS = --target=wasm32-wasip1 --sysroot=/opt/wasi-sdk/share/wasi-sysroot \
             -mllvm -wasm-use-legacy-eh=false -DMRB_INT32 -I\"$(MRUBY_PACKAGE_DIR)/include\"\n",
        );
        assert_eq!(
            parse_compile_flags(&sidecar),
            vec![
                "--target=wasm32-wasip1".to_owned(),
                "--sysroot=/opt/wasi-sdk/share/wasi-sysroot".to_owned(),
                "-mllvm".to_owned(),
                "-wasm-use-legacy-eh=false".to_owned(),
                "-DMRB_INT32".to_owned(),
            ],
            "a flag and the value it governs stay together and in order"
        );
    }

    #[test]
    fn an_msvc_archives_compile_flags_are_read_from_its_cflags() {
        let sidecar = sidecar("msvc", MSVC_SIDECAR);
        assert_eq!(
            parse_compile_flags(&sidecar),
            vec![
                "/nologo".to_owned(),
                "/W3".to_owned(),
                "/MD".to_owned(),
                "/O2".to_owned(),
                "/D_CRT_SECURE_NO_WARNINGS".to_owned(),
                "/we4013".to_owned(),
                "/DMRB_STACK_EXTEND_DOUBLING".to_owned(),
                "/DMRB_INT32".to_owned(),
            ],
            "MSVC names the header tree with /I, and that one flag is the only one dropped"
        );
    }

    #[test]
    fn an_msvc_archives_declaration_flags_reach_binding_generation_as_clangs() {
        // libclang never reads MSVC's spelling, so each flag deciding
        // what the headers declare crosses in the spelling it does read.
        let sidecar = sidecar(
            "msvc-declares",
            "MRUBY_CFLAGS = /nologo /std:c11 /O2 /DMRB_INT32 /UMRB_USE_FLOAT32\n",
        );
        let flags = parse_compile_flags(&sidecar);
        assert_eq!(
            declaration_flags(&sidecar, &flags),
            vec![
                "-std=c11".to_owned(),
                "-DMRB_INT32".to_owned(),
                "-UMRB_USE_FLOAT32".to_owned(),
            ]
        );
    }

    #[test]
    fn a_sidecar_naming_no_standard_is_parsed_under_one_that_keeps_the_archives_form() {
        // mruby's MSVC toolchain gives its compiler no standard, so the
        // archive is built under that compiler's default. libclang's own
        // default is a newer one, under which mruby declares its
        // never-returning functions in a form the bindings do not carry.
        let sidecar = sidecar("msvc-no-standard", MSVC_SIDECAR);
        let flags = parse_compile_flags(&sidecar);
        assert_eq!(
            declaration_flags(&sidecar, &flags),
            vec![
                "-D_CRT_SECURE_NO_WARNINGS".to_owned(),
                "-DMRB_STACK_EXTEND_DOUBLING".to_owned(),
                "-DMRB_INT32".to_owned(),
                "-std=gnu99".to_owned(),
            ]
        );
    }

    #[test]
    fn a_sidecar_naming_a_standard_is_parsed_under_that_one_alone() {
        let sidecar = sidecar("named-standard", HOST_SIDECAR);
        let flags = parse_compile_flags(&sidecar);
        let standards: Vec<String> = declaration_flags(&sidecar, &flags)
            .into_iter()
            .filter(|flag| flag.starts_with("-std="))
            .collect();
        assert_eq!(
            standards,
            vec!["-std=gnu99".to_owned()],
            "the archive's own, and no second one"
        );
    }

    #[test]
    #[should_panic(expected = "names a bare `/D` in `MRUBY_CFLAGS`")]
    fn a_bare_msvc_define_fails_rather_than_reaching_the_parse_without_its_value() {
        let sidecar = sidecar("msvc-bare-define", "MRUBY_CFLAGS = /D MRB_INT32\n");
        let flags = parse_compile_flags(&sidecar);
        declaration_flags(&sidecar, &flags);
    }

    #[test]
    fn the_archives_file_name_is_read_from_the_path_its_sidecar_names_it_by() {
        assert_eq!(
            parse_archive_file_name(&sidecar("archive-gnu", HOST_SIDECAR)),
            "libmruby.a"
        );
        assert_eq!(
            parse_archive_file_name(&sidecar("archive-msvc", MSVC_SIDECAR)),
            "libmruby.lib",
            "MSVC separates the path with backslashes and names the file .lib"
        );
    }

    #[test]
    #[should_panic(expected = "carries no file name")]
    fn a_path_whose_file_name_is_a_make_variable_fails_rather_than_looking_for_it() {
        let sidecar = sidecar("archive-variable", "MRUBY_LIBMRUBY_PATH = $(MRUBY_LIB)\n");
        parse_archive_file_name(&sidecar);
    }

    #[test]
    fn the_archive_answers_to_the_name_its_own_file_is_built_from() {
        assert!(names_the_archive("mruby", "libmruby.a"));
        assert!(names_the_archive("libmruby", "libmruby.lib"));
        // Everything else the link set names is a library beside it.
        assert!(!names_the_archive("m", "libmruby.a"));
        assert!(!names_the_archive("kernel32", "libmruby.lib"));
        // One toolchain's naming never answers for the other's file.
        assert!(!names_the_archive("mruby", "libmruby.lib"));
    }

    #[test]
    fn the_archives_compiler_is_read_from_its_cc() {
        let sidecar = sidecar("wasi-cc", "MRUBY_CC = /opt/wasi-sdk/bin/clang\n");
        assert_eq!(
            parse_compiler(&sidecar),
            "/opt/wasi-sdk/bin/clang".to_owned()
        );
    }

    #[test]
    fn only_the_flags_deciding_what_the_headers_declare_reach_binding_generation() {
        // `-mllvm -wasm-use-legacy-eh=false` decides how code is
        // generated and `-Wall` what is warned about; neither changes a
        // declaration, and libclang need not know either.
        let sidecar = sidecar(
            "wasi-declares",
            "MRUBY_CFLAGS = -std=gnu99 -g -O3 -Wall -mllvm -wasm-use-legacy-eh=false \
             -DMRB_INT32 -UMRB_USE_FLOAT32\n",
        );
        let flags = parse_compile_flags(&sidecar);
        assert_eq!(
            declaration_flags(&sidecar, &flags),
            vec![
                "-std=gnu99".to_owned(),
                "-DMRB_INT32".to_owned(),
                "-UMRB_USE_FLOAT32".to_owned(),
            ],
            "the language standard and the macro state cross; nothing else does"
        );
    }

    #[test]
    #[should_panic(expected = "names a bare `-D` in `MRUBY_CFLAGS`")]
    fn a_bare_define_fails_rather_than_reaching_the_parse_without_its_value() {
        let sidecar = sidecar("bare-define", "MRUBY_CFLAGS = -D MRB_INT32\n");
        let flags = parse_compile_flags(&sidecar);
        declaration_flags(&sidecar, &flags);
    }

    #[test]
    fn the_archives_link_set_is_read_from_its_libs() {
        let sidecar = sidecar("host-libs", HOST_SIDECAR);
        assert_eq!(
            parse_link_libs(&sidecar),
            vec!["mruby".to_owned(), "m".to_owned()],
            "every -l token is carried, without its prefix"
        );
    }

    #[test]
    fn a_link_set_carries_whatever_the_configuration_added() {
        // A cross build's sidecar names the toolchain library its
        // exception mechanism needs; a gem's configuration could name
        // any other. Neither is known to the crate.
        let sidecar = sidecar("extra-libs", "MRUBY_LIBS = -lmruby -lm -lsetjmp -lz\n");
        assert_eq!(
            parse_link_libs(&sidecar),
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
        let sidecar = sidecar("no-libs", "MRUBY_CFLAGS = -DMRB_INT32\n");
        parse_link_libs(&sidecar);
    }

    #[test]
    #[should_panic(expected = "neither a `-l<name>` option nor a `<name>.lib` file")]
    fn a_quoted_library_fails_rather_than_naming_one_with_its_quotes() {
        let sidecar = sidecar("libs-quoted", "MRUBY_LIBS = -l\"mruby\" -l\"m\"\n");
        parse_link_libs(&sidecar);
    }

    #[test]
    fn an_msvc_link_set_names_each_library_by_its_file() {
        let sidecar = sidecar("libs-msvc", "MRUBY_LIBS = libmruby.lib kernel32.lib\n");
        assert_eq!(
            parse_link_libs(&sidecar),
            vec!["libmruby".to_owned(), "kernel32".to_owned()],
            "MSVC names a library by its file, and the name is what is carried out"
        );
    }

    #[test]
    #[should_panic(expected = "neither a `-l<name>` option nor a `<name>.lib` file")]
    fn a_library_named_in_neither_form_fails_rather_than_linking_none() {
        let sidecar = sidecar("libs-bare", "MRUBY_LIBS = mruby\n");
        parse_link_libs(&sidecar);
    }

    #[test]
    #[should_panic(expected = "continuation line in `MRUBY_LIBS`")]
    fn a_continuation_in_the_link_set_fails_rather_than_dropping_a_library() {
        let sidecar = sidecar("libs-continuation", "MRUBY_LIBS = -lmruby \\\n  -lm\n");
        parse_link_libs(&sidecar);
    }

    #[test]
    fn a_cross_builds_toolchain_root_is_derived_from_its_compiler() {
        let sidecar = sidecar(
            "cc-cross",
            "MRUBY_CC = /opt/wasi-sdk/bin/clang\nMRUBY_CFLAGS = -DMRB_INT32\n",
        );
        assert_eq!(
            parse_toolchain_root(&sidecar),
            Some(std::path::PathBuf::from("/opt/wasi-sdk"))
        );
    }

    #[test]
    fn a_host_build_names_a_compiler_with_no_root_to_derive() {
        let sidecar = sidecar("cc-host", "MRUBY_CC = gcc\nMRUBY_CFLAGS = -DMRB_INT32\n");
        assert_eq!(parse_toolchain_root(&sidecar), None);
    }

    #[test]
    fn a_compiler_outside_a_bin_directory_yields_no_root() {
        // Only the `<root>/bin/<compiler>` shape names a root; anything
        // else is read as naming none rather than as naming its parent.
        let sidecar = sidecar(
            "cc-loose",
            "MRUBY_CC = /usr/local/clang\nMRUBY_CFLAGS = -DMRB_INT32\n",
        );
        assert_eq!(parse_toolchain_root(&sidecar), None);
    }

    #[test]
    #[should_panic(expected = "is missing")]
    fn a_missing_sidecar_fails_naming_the_path() {
        Sidecar::read(&empty_dir("absent"));
    }

    #[test]
    #[should_panic(expected = "has no `MRUBY_CFLAGS = ` line")]
    fn a_sidecar_without_a_cflags_line_fails() {
        let sidecar = sidecar("no-cflags", "MRUBY_CC = gcc\nMRUBY_LIBS = -lmruby\n");
        parse_compile_flags(&sidecar);
    }

    #[test]
    #[should_panic(expected = "continuation line in `MRUBY_CFLAGS`")]
    fn a_continuation_line_fails_rather_than_dropping_the_rest() {
        let sidecar = sidecar(
            "continuation",
            "MRUBY_CFLAGS = -DMRB_INT32 \\\n  -DMRB_UTF8\n",
        );
        parse_compile_flags(&sidecar);
    }

    #[test]
    #[should_panic(expected = "carries a quoted value")]
    fn a_quoted_value_fails_rather_than_reaching_a_compiler_in_pieces() {
        let sidecar = sidecar("quoted-define", "MRUBY_CFLAGS = -DMRB_NAME=\"a b\"\n");
        parse_compile_flags(&sidecar);
    }
}
