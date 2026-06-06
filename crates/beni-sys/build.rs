// build.rs — beni-sys archive discovery, bindgen run, and static-fn
// trampoline compilation.
//
// Purpose
// -------
// When archive discovery locates a `libmruby.a` for the active cargo
// target, this build script does four things:
//
//   1. Runs bindgen against `src/wrapper.h` to emit the mruby C API
//      FFI surface into `$OUT_DIR/bindings.rs`. The static-fn
//      trampolines bindgen needs to reach `MRB_INLINE` helpers and
//      the `wrapper.h`-defined inline wrappers land in
//      `$OUT_DIR/mruby_static_wrappers.c`.
//   2. Compiles the bindgen-emitted trampoline file against mruby's
//      headers so the trampoline symbols (`mrb_obj_value__extern`,
//      `mrb_rstring_ptr__extern`, etc.) resolve into the rlib's
//      object set. No hand-written C shims remain — the
//      single-translation-unit file produced by bindgen is the
//      entire C surface.
//   3. Emits `cargo:rustc-link-search=native=<lib dir>` plus
//      `cargo:rustc-link-lib=static=mruby` so the resulting rlib drags
//      `libmruby.a` into the consumer's link graph.
//   4. Emits `cargo:rustc-cfg=mruby_linked` so the crate sources
//      include the generated bindings instead of the host
//      placeholders.
//
// Archive discovery
// -----------------
// Discovery is environment-driven, highest precedence first; the
// highest-precedence variable set is the sole source, never falling
// back to a lower one:
//
//   1. `MRUBY_LIB_DIR` — names the directory containing the active
//      target's `libmruby.a` and `libmruby.flags.mak`.
//   2. `BENI_VENDOR_DIR` — names the vendor tree `rake beni:build`
//      populated; the crate reads the `host` build's staged path
//      (`mruby/build/host/lib/`) and serves host cargo targets only.
//      A cross-compiled cargo target never reads the vendor tree and
//      requires `MRUBY_LIB_DIR`.
//   3. With neither variable set, no archive is linked: a host build
//      compiles in placeholder mode (no bindgen, no link directives —
//      `src/lib.rs` supplies stub types so plain `cargo check` works
//      for registry consumers), a cross-compiled build fails.
//
// A set variable whose archive is absent fails the build naming the
// expected path — discovery never silently falls back to placeholder
// mode. wasm32 is the one supported cross target; any other
// cross-compiled cargo target fails naming the target. wasm32 builds
// resolve the wasi-sdk root from `WASI_SDK_PATH`, defaulting to
// `/opt/wasi-sdk` when unset, and fail naming the root in effect when
// it lacks the toolchain.
//
// The discovered lib dir is self-contained: mruby's build copies the
// complete public header tree (source headers, generated headers,
// gem exports) into the sibling `include/` whenever it archives
// `libmruby.a`, so `<lib dir>/../include` is the single include root
// — the same directory the sidecar's own `-I$(MRUBY_PACKAGE_DIR)/
// include` names. The ABI-bearing `-D` defines are NOT hard-coded:
// they are parsed from the `libmruby.flags.mak` sidecar mruby writes
// next to each archive (mruby's official embedder interface,
// recording the exact compile flags; `Beni::Builder` requests it on
// every build), so bindgen and the trampoline compile always see
// what the archive was actually built with.
//
// Linked signal
// -------------
// Every build publishes `cargo:linked=` through the `links = "mruby"`
// key — `1` with a real archive linked, `0` in placeholder mode.
// Direct dependents (the `beni` wrapper crate, downstream gates) read
// it as `DEP_MRUBY_LINKED` and derive their own `mruby_linked` cfg
// from the value.
//
// Idempotency
// -----------
// Cargo only re-runs this script when its source changes or when one
// of the `cargo:rerun-if-env-changed=` / `cargo:rerun-if-changed=`
// entries below changes.

use std::env;
use std::path::{Path, PathBuf};

/// Extract the `-D` defines from the `libmruby.flags.mak` sidecar in
/// `lib_dir` — the flags the discovered archive was actually compiled
/// with. bindgen and the trampoline compile must see the same set or
/// the `mrb_value` layout silently diverges from the archive, so a
/// discovered archive without its sidecar fails loudly instead of
/// guessing. (`MRUBY_CFLAGS = ...` is plain space-separated tokens;
/// only the `-D` ones matter here — include paths and target flags
/// are constructed independently below.)
fn parse_abi_defines(lib_dir: &Path) -> Vec<String> {
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
    cflags
        .split_whitespace()
        .filter(|token| token.starts_with("-D"))
        .map(str::to_owned)
        .collect()
}

/// Non-empty value of the env var named `key`, treating unset and
/// empty as the same "not provided" state.
fn env_path(key: &str) -> Option<String> {
    env::var(key).ok().filter(|s| !s.is_empty())
}

/// Locate the directory holding the active target's `libmruby.a` and
/// its compile-flags sidecar. Returns `None` only in placeholder mode
/// (host build, no archive discovery variable set); every other
/// outcome either resolves or panics naming what is missing.
fn discover_lib_dir(is_wasm: bool) -> Option<PathBuf> {
    if let Some(dir) = env_path("MRUBY_LIB_DIR") {
        let lib_dir = PathBuf::from(dir);
        require_archive(&lib_dir, "MRUBY_LIB_DIR");
        return Some(lib_dir);
    }
    if is_wasm {
        panic!(
            "beni-sys: cross-compiled builds require MRUBY_LIB_DIR to name the \
             directory containing the target's libmruby.a (the vendor tree is \
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
        return Some(lib_dir);
    }
    None
}

/// Fail loudly when the discovery variable points at a directory with
/// no archive — a set variable is a claim that the archive exists, so
/// the build never silently degrades to placeholder mode.
fn require_archive(lib_dir: &Path, var: &str) {
    let archive = lib_dir.join("libmruby.a");
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
/// root in effect.
fn resolve_wasi_sdk() -> String {
    let root = env_path("WASI_SDK_PATH").unwrap_or_else(|| "/opt/wasi-sdk".to_owned());
    if !Path::new(&root).join("bin").join("clang").exists() {
        panic!(
            "beni-sys: the wasi-sdk root in effect ({root}) lacks the wasi-sdk \
             toolchain (no bin/clang). Set WASI_SDK_PATH to the unpacked \
             wasi-sdk root."
        );
    }
    root
}

fn main() {
    println!("cargo:rerun-if-env-changed=MRUBY_LIB_DIR");
    println!("cargo:rerun-if-env-changed=BENI_VENDOR_DIR");
    println!("cargo:rerun-if-env-changed=WASI_SDK_PATH");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/wrapper.h");
    println!("cargo:rustc-check-cfg=cfg(mruby_linked)");

    let target = env::var("TARGET").unwrap_or_default();
    let host = env::var("HOST").unwrap_or_default();
    let is_wasm = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default() == "wasm32";
    if target != host && !is_wasm {
        panic!(
            "beni-sys: unsupported cross-compilation target {target}. \
             wasm32 is the one supported cross target."
        );
    }

    let Some(lib_dir) = discover_lib_dir(is_wasm) else {
        // Placeholder mode: host build, no archive discovery variable
        // set (e.g. a registry consumer running `cargo check`).
        // src/lib.rs keeps the stub types compiling; nothing links
        // against mruby. Dependents still get the linked signal.
        println!("cargo:linked=0");
        return;
    };

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

    let wasi_sdk = is_wasm.then(resolve_wasi_sdk);

    // The archive's actual compile defines, from its flags.mak
    // sidecar. Re-run when the sidecar changes — a rebuilt archive
    // with different defines must re-bindgen.
    println!(
        "cargo:rerun-if-changed={}",
        lib_dir.join("libmruby.flags.mak").display()
    );
    let abi_defines = parse_abi_defines(&lib_dir);

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let bindings_rs = out_dir.join("bindings.rs");
    let static_wrappers_c = out_dir.join("mruby_static_wrappers.c");

    run_bindgen(
        &manifest_dir,
        &include_root,
        wasi_sdk.as_deref(),
        &abi_defines,
        &bindings_rs,
        &static_wrappers_c,
    );
    compile_trampolines(
        &include_root,
        wasi_sdk.as_deref(),
        &abi_defines,
        &static_wrappers_c,
    );

    println!("cargo:rustc-link-search=native={}", lib_dir.display());
    println!("cargo:rustc-link-lib=static=mruby");

    // wasi-sdk setjmp library — required because the wasi libmruby.a
    // uses setjmp/longjmp via the new WebAssembly exception handling
    // mechanism (the wasi toolchain file sets
    // `-mllvm -wasm-use-legacy-eh=false`). This produces calls to
    // `__wasm_setjmp`, `__wasm_longjmp`, and `__wasm_setjmp_test`
    // which live in wasi-sdk's `libsetjmp.a` (not in Rust's
    // wasm32-wasip1 self-contained libc). Without this library,
    // rust-lld's `--allow-undefined` flag would turn these into wasm
    // imports that the host cannot satisfy. Host builds use the
    // platform's native setjmp from libc — no extra directive.
    if let Some(wasi_sdk) = wasi_sdk.as_deref() {
        let setjmp_dir = format!("{}/share/wasi-sysroot/lib/wasm32-wasi", wasi_sdk);
        println!("cargo:rustc-link-search=native={}", setjmp_dir);
        println!("cargo:rustc-link-lib=static=setjmp");
    }

    println!("cargo:rustc-cfg=mruby_linked");
    println!("cargo:linked=1");
}

fn run_bindgen(
    manifest_dir: &Path,
    include_root: &Path,
    wasi_sdk: Option<&str>,
    abi_defines: &[String],
    bindings_rs: &Path,
    static_wrappers_c: &Path,
) {
    let wrapper_h = manifest_dir.join("src/wrapper.h");
    let mut builder = bindgen::Builder::default().header(wrapper_h.to_str().unwrap());
    if let Some(wasi_sdk) = wasi_sdk {
        builder = builder
            .clang_arg("--target=wasm32-wasi")
            .clang_arg(format!("--sysroot={}/share/wasi-sysroot", wasi_sdk));
    }
    // `-D<name>[=<value>]` tokens straight from flags.mak.
    for define in abi_defines {
        builder = builder.clang_arg(define);
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
        // declared in `wrapper.h` (`mrb_rstring_ptr`, `mrb_obj_ptr_func`,
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
    wasi_sdk: Option<&str>,
    abi_defines: &[String],
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
    if let Some(wasi_sdk) = wasi_sdk {
        build
            .compiler(format!("{}/bin/clang", wasi_sdk))
            .flag(format!("--sysroot={}/share/wasi-sysroot", wasi_sdk));
    }
    // `-D<name>[=<value>]` tokens straight from flags.mak, passed as
    // raw flags so name=value pairs survive untouched.
    for define in abi_defines {
        build.flag(define);
    }
    build
        .file(static_wrappers_c)
        .include(include_root)
        .compile("beni_mruby_trampolines");
}
