// build.rs — beni-sys archive discovery, bindgen run, and static-fn
// trampoline compilation.
//
// Purpose
// -------
// When archive discovery locates an archive for the active cargo
// target, this build script does four things:
//
//   1. Runs bindgen against `src/wrapper.h` to emit the mruby C API
//      FFI surface into `$OUT_DIR/bindings.rs`. The static-fn
//      trampolines bindgen needs to reach `MRB_INLINE` helpers and
//      the `wrapper.h`-defined inline wrappers land in
//      `$OUT_DIR/mruby_static_wrappers.c`.
//   2. Compiles the bindgen-emitted trampoline file against mruby's
//      headers so the trampoline symbols (`mrb_obj_value__extern`,
//      `mrb_rstring_ptr_func__extern`, etc.) resolve into the rlib's
//      object set. No hand-written C shims remain — the
//      single-translation-unit file produced by bindgen is the
//      entire C surface.
//   3. Emits the link directives that drag the archive and everything
//      it needs beside it into the consumer's link graph — search
//      paths for the discovered lib dir and, on a cross build, the
//      toolchain's own sysroot, plus one `cargo:rustc-link-lib` per
//      library the sidecar names.
//   4. Leaves the bindings at `$OUT_DIR/bindings.rs`, the one path
//      `src/lib.rs` includes them from.
//
// Archive discovery
// -----------------
// Discovery is environment-driven, highest precedence first; the
// highest-precedence variable set is the sole source, never falling
// back to a lower one:
//
//   1. `MRUBY_LIB_DIR` — names the directory containing the active
//      target's archive and `libmruby.flags.mak`.
//   2. `BENI_VENDOR_DIR` — names the vendor tree `rake beni:build`
//      populated; the crate reads the `host` build's staged path
//      (`mruby/build/host/lib/`) and serves host cargo targets only.
//      A cross-compiled cargo target never reads the vendor tree and
//      requires `MRUBY_LIB_DIR`.
//   3. With neither variable set the build fails, naming the variables
//      it consulted.
//
// A documentation build is the one build that runs no discovery: its
// host has nowhere to stage an archive and never links, so
// `src/bindings_docs.rs` is copied to `$OUT_DIR/bindings.rs` and the
// crate compiles against declarations alone.
//
// A set variable whose archive is absent fails the build naming the
// expected path. The supported cross targets are wasm32 and the other
// macOS architecture — the host compiler builds for either macOS
// architecture, so that one needs no toolchain of its own and reaches
// its archive through `MRUBY_LIB_DIR` like every cross target; any other
// cross-compiled cargo target fails naming the target. wasm32 builds
// resolve the wasi-sdk root from `WASI_SDK_PATH`, defaulting to
// `/opt/wasi-sdk` when unset, and fail naming the root in effect when
// it lacks the toolchain, when it is not the root the archive's
// sidecar records as having built it, or when the sidecar records
// none. One root reached by two spellings is one root.
//
// The discovered lib dir is self-contained: mruby's build copies the
// complete public header tree (source headers, generated headers,
// gem exports) into the sibling `include/` whenever it archives
// the archive, so `<lib dir>/../include` is the single include root
// — the same directory the sidecar's own `-I$(MRUBY_PACKAGE_DIR)/
// include` names. Neither the compile flags nor the link set are
// hard-coded: both are parsed from the `libmruby.flags.mak` sidecar
// mruby writes next to each archive (mruby's official embedder
// interface, recording the compiler that built it, the flags that
// compiler was given, and the libraries it needs; `Beni::Builder`
// requests it on every build). A flag holds only for the compiler it
// was written for, so the two consumers differ: the trampoline compile
// uses the compiler the sidecar names and sees every flag it carries,
// less the include path this script substitutes; bindgen, which parses
// with libclang rather than that compiler, sees only the flags
// deciding what the headers declare.
//
// Documentation build
// -------------------
// A documentation host sets `DOCS_RS`, has no network and nowhere to
// stage an archive, and never links. That build copies
// `src/bindings_docs.rs` — written by `rake docs:bindings` from an
// mruby built with mruby's own default config, tracked by nothing and
// carried into the published package by the manifest's `include` — into
// `$OUT_DIR` under the name every other build's bindings take, so
// `src/lib.rs` includes one path and the crate carries no second shape.
//
// Idempotency
// -----------
// Cargo only re-runs this script when its source changes or when one
// of the `cargo:rerun-if-env-changed=` / `cargo:rerun-if-changed=`
// entries below changes.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

include!("build/sidecar.rs");
include!("build/target.rs");
include!("build/version.rs");

/// Non-empty value of the env var named `key`, treating unset and
/// empty as the same "not provided" state.
fn env_path(key: &str) -> Option<String> {
    env::var(key).ok().filter(|s| !s.is_empty())
}

/// Locate the directory holding the active target's archive and its
/// compile-flags sidecar. Every outcome either resolves or panics
/// naming what is missing.
fn discover_lib_dir(is_cross: bool) -> PathBuf {
    if let Some(dir) = env_path("MRUBY_LIB_DIR") {
        let lib_dir = PathBuf::from(dir);
        require_archive(&lib_dir, "MRUBY_LIB_DIR");
        return lib_dir;
    }
    if is_cross {
        panic!(
            "beni-sys: cross-compiled builds require MRUBY_LIB_DIR to name the \
             directory containing the target's archive (the vendor tree is \
             never read for cross targets). Build the archive with \
             `bundle exec rake beni:build`, then set MRUBY_LIB_DIR."
        );
    }
    if let Some(dir) = env_path("BENI_VENDOR_DIR") {
        let lib_dir = PathBuf::from(dir)
            .join("mruby")
            .join("build")
            .join("host")
            .join("lib");
        require_archive(&lib_dir, "BENI_VENDOR_DIR");
        return lib_dir;
    }
    panic!(
        "beni-sys: no archive discovery variable set. Set MRUBY_LIB_DIR to the \
         directory containing this target's archive, or BENI_VENDOR_DIR to \
         the vendor tree `bundle exec rake beni:build` populates."
    );
}

/// Fail loudly when the discovery variable points at a directory with
/// no archive — a set variable is a claim that the archive exists. The
/// name to look for is the archive's own, which its sidecar states.
fn require_archive(lib_dir: &Path, var: &str) {
    let archive = lib_dir.join(parse_archive_file_name(lib_dir));
    if !archive.exists() {
        panic!(
            "beni-sys: {var} is set but {} does not exist. Run \
             `bundle exec rake beni:build` to produce the archive, or point \
             {var} at the correct location.",
            archive.display()
        );
    }
}

/// Resolve the wasi-sdk root for wasm32 builds: `WASI_SDK_PATH` when
/// set, the `/opt/wasi-sdk` convention otherwise. The root must hold
/// the toolchain (`bin/clang`) — a missing toolchain fails naming the
/// root in effect — and must be the one the archive in `lib_dir` was
/// built against, which its sidecar records.
fn resolve_wasi_sdk(lib_dir: &Path) -> String {
    let root = env_path("WASI_SDK_PATH").unwrap_or_else(|| "/opt/wasi-sdk".to_owned());
    if !Path::new(&root).join("bin").join("clang").exists() {
        panic!(
            "beni-sys: the wasi-sdk root in effect ({root}) lacks the wasi-sdk \
             toolchain (no bin/clang). Set WASI_SDK_PATH to the unpacked \
             wasi-sdk root."
        );
    }
    let Some(recorded) = parse_toolchain_root(lib_dir) else {
        panic!(
            "beni-sys: the archive in {} records no toolchain root, so the \
             wasi-sdk root in effect ({root}) cannot be checked against the one \
             that built it. A cross-built archive names its compiler by path; \
             rebuild it through `bundle exec rake beni:build`.",
            lib_dir.display()
        );
    };
    if same_directory(&recorded, Path::new(&root)) {
        return root;
    }
    panic!(
        "beni-sys: the archive in {} was built against the wasi-sdk root {}, \
         but the root in effect is {root}. Point WASI_SDK_PATH at the root \
         that built the archive, or rebuild the archive against this one.",
        lib_dir.display(),
        recorded.display()
    );
}

/// Whether two paths name the same directory, resolving symlinks and
/// relative segments so one root reached by two spellings still reads
/// as one. A path that does not resolve names no directory, so it can
/// match nothing.
fn same_directory(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

fn main() {
    println!("cargo:rerun-if-env-changed=MRUBY_LIB_DIR");
    println!("cargo:rerun-if-env-changed=BENI_VENDOR_DIR");
    println!("cargo:rerun-if-env-changed=WASI_SDK_PATH");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build/sidecar.rs");
    println!("cargo:rerun-if-changed=build/target.rs");
    println!("cargo:rerun-if-changed=build/version.rs");
    println!("cargo:rerun-if-changed=src/wrapper.h");
    println!("cargo:rerun-if-env-changed=DOCS_RS");
    println!("cargo:rerun-if-changed=src/bindings_docs.rs");

    let target = env::var("TARGET").unwrap_or_default();
    let host = env::var("HOST").unwrap_or_default();
    let is_wasm = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default() == "wasm32";
    require_supported_target(&target, &host, is_wasm);

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    if env::var_os("DOCS_RS").is_some() {
        stage_documentation_bindings(&manifest_dir, &out_dir);
        return;
    }

    let lib_dir = discover_lib_dir(target != host);

    // The complete header tree mruby copies next to the archive on
    // every build — the single include root for bindgen and the
    // trampoline compile.
    let include_root = lib_dir.join("..").join("include");
    if !include_root.exists() {
        panic!(
            "beni-sys: {} is missing — the archive's header tree was not \
             staged alongside it. Re-run `bundle exec rake beni:build` to \
             rebuild the archive together with its headers.",
            include_root.display()
        );
    }
    // Re-run when the header stating the release changes: a re-staged
    // archive from another mruby answers the floor differently.
    println!(
        "cargo:rerun-if-changed={}",
        include_root.join("mruby").join("version.h").display()
    );
    require_supported_mruby(&include_root);

    let wasi_sdk = is_wasm.then(|| resolve_wasi_sdk(&lib_dir));

    // The archive's actual compile defines, from its flags.mak
    // sidecar. Re-run when the sidecar changes — a rebuilt archive
    // with different defines must re-bindgen.
    println!(
        "cargo:rerun-if-changed={}",
        lib_dir.join(FLAGS_MAK).display()
    );
    let compiler = parse_compiler(&lib_dir);
    let compile_flags = parse_compile_flags(&lib_dir);
    let declaration_flags = declaration_flags(&lib_dir, &compile_flags);

    let bindings_rs = out_dir.join("bindings.rs");
    let static_wrappers_c = out_dir.join("mruby_static_wrappers.c");

    run_bindgen(
        &manifest_dir,
        &include_root,
        wasi_sdk.as_deref(),
        &declaration_flags,
        &bindings_rs,
        &static_wrappers_c,
    );
    compile_trampolines(&include_root, &compiler, &compile_flags, &static_wrappers_c);

    // The archive sits where discovery found it; every other library
    // its sidecar names comes from the toolchain that built it, which
    // for a cross build is the wasi-sdk sysroot rather than Rust's own
    // (Rust's wasm32-wasip1 self-contained set carries libc alone).
    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    if let Some(wasi_sdk) = wasi_sdk.as_deref() {
        println!(
            "cargo:rustc-link-search=native={}/share/wasi-sysroot/lib/wasm32-wasip1",
            wasi_sdk
        );
    }
    let archive_file_name = parse_archive_file_name(&lib_dir);
    for lib in parse_link_libs(&lib_dir) {
        // The archive is static by definition; on wasm32 nothing links
        // dynamically, so every library there is static too.
        let kind = if names_the_archive(&lib, &archive_file_name) || is_wasm {
            "static="
        } else {
            ""
        };
        println!("cargo:rustc-link-lib={kind}{lib}");
    }
}

/// Put the documentation bindings where `src/lib.rs`
/// includes bindings from, so a documentation build renders the whole
/// surface with no archive to generate one against.
fn stage_documentation_bindings(manifest_dir: &Path, out_dir: &Path) {
    let source = manifest_dir.join("src").join("bindings_docs.rs");
    if !source.exists() {
        panic!(
            "beni-sys: {} is missing — a documentation build reads it in place \
             of a discovered archive's bindings. Run `bundle exec rake docs:bindings` \
             to write it.",
            source.display()
        );
    }
    fs::copy(&source, out_dir.join("bindings.rs")).expect("stage documentation bindings");
}

fn run_bindgen(
    manifest_dir: &Path,
    include_root: &Path,
    wasi_sdk: Option<&str>,
    declaration_flags: &[String],
    bindings_rs: &Path,
    static_wrappers_c: &Path,
) {
    let wrapper_h = manifest_dir.join("src/wrapper.h");
    let mut builder = bindgen::Builder::default().header(wrapper_h.to_str().unwrap());
    if let Some(wasi_sdk) = wasi_sdk {
        builder = builder
            .clang_arg("--target=wasm32-wasip1")
            .clang_arg(format!("--sysroot={}/share/wasi-sysroot", wasi_sdk));
    }
    // These decide what the archive's headers declare. libclang is not
    // the compiler the sidecar names, so nothing deciding how code is
    // generated reaches it.
    for flag in declaration_flags {
        builder = builder.clang_arg(flag);
    }
    let bindings = builder
        // WORKAROUND rust-bindgen #751: clang's wasm32 frontend defaults
        // to -fvisibility=hidden, so libclang flags every MRB_API
        // function as CXVisibility_Hidden and bindgen drops them. Only
        // the wrap_static_fns wrappers survive without this. Harmless
        // on host targets, so applied unconditionally.
        .clang_arg("-fvisibility=default")
        .clang_arg(format!("-I{}", include_root.display()))
        // WORKAROUND: allowlist_function by name regex misses items
        // under some attribute combinations (related to #751). File-level
        // allowlist matches every declaration in the mruby header tree
        // and is the pattern rb-sys uses.
        .allowlist_file(".*mruby.*\\.h")
        .allowlist_file(".*wrapper\\.h")
        // Blocklist mrb_func_t so its name resolves to our typed alias
        // in lib.rs (with `Value` parameters) instead of bindgen's
        // Option<unsafe extern "C" fn(...)>-wrapped version.
        .blocklist_type("mrb_func_t")
        // WORKAROUND: mrb_gc has mixed `int:2` and `mrb_bool:1`
        // bitfields. clang's actual codegen keeps the int portion in
        // its own 4-byte container; bindgen merges all 7 bits into a
        // single byte, shifting every field after mrb_gc in mrb_state
        // by 4 bytes. opaque_type makes bindgen ask clang for
        // sizeof(mrb_gc) (correct) and emit an opaque blob.
        .opaque_type("mrb_gc")
        .prepend_enum_name(false)
        // Generate trampolines for `static inline` helpers reached
        // through `wrapper.h` — both mruby's own (`mrb_integer_func`,
        // `mrb_obj_value`, `mrb_type`, …) and the macro wrappers
        // declared in `wrapper.h` (`mrb_rstring_ptr_func`, `mrb_obj_ptr_func`,
        // `mrb_gc_arena_save_func`, `mrb_proc_new_func`, …).
        .wrap_static_fns(true)
        .wrap_static_fns_path(static_wrappers_c.with_extension(""))
        .layout_tests(false)
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate()
        .expect("bindgen: failed to generate mruby bindings");
    bindings
        .write_to_file(bindings_rs)
        .expect("bindgen: failed to write bindings.rs");
}

fn compile_trampolines(
    include_root: &Path,
    compiler: &str,
    compile_flags: &[String],
    static_wrappers_c: &Path,
) {
    if !static_wrappers_c.exists() {
        // bindgen always emits this file when `wrap_static_fns` is
        // on; absence means the build is incomplete. Fail loudly so
        // a stale OUT_DIR cannot ship a link graph missing trampoline
        // symbols.
        panic!(
            "beni-sys: bindgen did not emit {}",
            static_wrappers_c.display()
        );
    }
    let mut build = cc::Build::new();
    build.compiler(compiler);
    // Passed as raw flags so name=value pairs survive untouched. This
    // is the compiler they were written for, so every one of them means
    // here what it meant when the archive was built.
    for flag in compile_flags {
        build.flag(flag);
    }
    build
        .file(static_wrappers_c)
        .include(include_root)
        .compile("beni_mruby_trampolines");
}
