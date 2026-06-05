// build.rs — beni-sys link wiring, bindgen run, and static-fn
// trampoline compilation.
//
// Purpose
// -------
// When a vendored `libmruby.a` is staged for the active target, this
// build script does four things:
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
//   3. Emits `cargo:rustc-link-search=native=$MRUBY_LIB_DIR` plus
//      `cargo:rustc-link-lib=static=mruby` so the resulting rlib drags
//      `libmruby.a` into the consumer's link graph.
//   4. Emits `cargo:rustc-cfg=mruby_linked` (and `cargo:linked=1` for
//      downstream build scripts via the `links = "mruby"` key) so the
//      crate sources include the generated bindings instead of the
//      host placeholders.
//
// Target selection mirrors `build_config/mruby.rb`: the host target
// links `vendor/mruby/build/host/lib/libmruby.a` (native build) and
// wasm32 links `vendor/mruby/build/wasi/lib/libmruby.a` (wasi-sdk
// cross build). The ABI-bearing `-D` defines are NOT hard-coded:
// they are parsed from the `libmruby.flags.mak` sidecar mruby writes
// next to each archive (mruby's official embedder interface,
// recording the exact compile flags; `Beni::Builder` requests it on
// every build), so bindgen and the trampoline compile always see
// what the archive was actually built with — whatever the build
// config, including mruby's untouched upstream default, where the
// flag list is empty and mrbconf.h's header-level platform defaults
// apply identically to both sides.
//
// When no `libmruby.a` is staged, host targets fall back to a
// placeholder build (no bindgen, no link directives — `src/lib.rs`
// supplies stub types so plain `cargo check` works for registry
// consumers), while wasm32 targets panic: an explicit cross-target
// build without the staged toolchain is always a mistake.
//
// Contract with the Rake driver
// -----------------------------
// `rake beni:build` (the beni gem's Beni::Tasks, dogfooded by this
// repo's Rakefile) produces both archives in the default vendor
// layout, which this script auto-detects. Two env vars override the
// probing:
//
//   * `MRUBY_LIB_DIR` — absolute path to the directory containing
//     `libmruby.a` for the active target. Drives the link-search +
//     link-lib directives, and the build-dir include resolution for
//     mruby's generated headers (`mruby/presym/id.h`).
//   * `WASI_SDK_PATH` — absolute path to the unpacked wasi-sdk root
//     (wasm32 only). Drives bindgen's clang invocation and the setjmp
//     library link directive.
//
// Idempotency
// -----------
// Cargo only re-runs this script when its source changes or when one
// of the `cargo:rerun-if-env-changed=` / `cargo:rerun-if-changed=`
// entries below changes.

use std::env;
use std::path::{Path, PathBuf};

/// Extract the `-D` defines from the `libmruby.flags.mak` sidecar in
/// `lib_dir` — the flags the staged archive was actually compiled
/// with. bindgen and the trampoline compile must see the same set or
/// the `mrb_value` layout silently diverges from the archive, so a
/// staged archive without its sidecar fails loudly instead of
/// guessing. (`MRUBY_CFLAGS = ...` is plain space-separated tokens;
/// only the `-D` ones matter here — include paths and target flags
/// are constructed independently below.)
fn parse_abi_defines(lib_dir: &Path) -> Vec<String> {
    let flags_mak = lib_dir.join("libmruby.flags.mak");
    let content = std::fs::read_to_string(&flags_mak).unwrap_or_else(|_| {
        panic!(
            "beni-sys: {} is missing. The staged libmruby.a's compile flags are \
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

fn main() {
    println!("cargo:rerun-if-env-changed=MRUBY_LIB_DIR");
    println!("cargo:rerun-if-env-changed=WASI_SDK_PATH");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/wrapper.h");
    println!("cargo:rustc-check-cfg=cfg(mruby_linked)");

    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let is_wasm = target_arch == "wasm32";
    // Matches the MRuby::Build / MRuby::CrossBuild names in
    // build_config/mruby.rb, which shape the vendor build tree layout.
    let mruby_build_name = if is_wasm { "wasi" } else { "host" };

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let vendor_dir = manifest_dir.join("..").join("..").join("vendor");
    let mruby_include = vendor_dir.join("mruby").join("include");

    let wasi_sdk = env::var("WASI_SDK_PATH")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let p = vendor_dir.join("wasi-sdk");
            p.exists().then(|| p.to_string_lossy().into_owned())
        });
    let mruby_lib_dir = env::var("MRUBY_LIB_DIR")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            let p = vendor_dir
                .join("mruby")
                .join("build")
                .join(mruby_build_name)
                .join("lib");
            p.exists().then(|| p.to_string_lossy().into_owned())
        });
    let mruby_build_include = mruby_lib_dir.as_ref().and_then(|lib_dir| {
        let p = PathBuf::from(lib_dir).join("..").join("include");
        p.exists().then_some(p)
    });

    let staged =
        mruby_include.exists() && mruby_build_include.is_some() && (!is_wasm || wasi_sdk.is_some());
    if !staged {
        if is_wasm {
            // An explicit wasm32 build without the staged toolchain can
            // never link; fail loudly with the recovery command.
            panic!(
                "beni-sys: vendor toolchain not staged for wasm32 build. \
                 Run `bundle exec rake beni:build` first."
            );
        }
        // Host placeholder mode: no libmruby.a available (e.g. a
        // registry consumer running `cargo check`). src/lib.rs keeps
        // the stub types compiling; nothing links against mruby.
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let bindings_rs = out_dir.join("bindings.rs");
    let static_wrappers_c = out_dir.join("mruby_static_wrappers.c");
    let wasi_sdk = if is_wasm { wasi_sdk.as_deref() } else { None };
    let mruby_build_include = mruby_build_include.as_deref().unwrap();

    // The archive's actual compile defines, from its flags.mak
    // sidecar. Re-run when the sidecar changes — a rebuilt archive
    // with different defines must re-bindgen.
    let mruby_lib_path = PathBuf::from(mruby_lib_dir.as_ref().unwrap());
    println!(
        "cargo:rerun-if-changed={}",
        mruby_lib_path.join("libmruby.flags.mak").display()
    );
    let abi_defines = parse_abi_defines(&mruby_lib_path);

    run_bindgen(
        &manifest_dir,
        &mruby_include,
        mruby_build_include,
        wasi_sdk,
        &abi_defines,
        &bindings_rs,
        &static_wrappers_c,
    );
    compile_trampolines(
        &mruby_include,
        mruby_build_include,
        wasi_sdk,
        &abi_defines,
        &static_wrappers_c,
    );

    if let Some(lib_dir) = mruby_lib_dir.as_ref() {
        println!("cargo:rustc-link-search=native={}", lib_dir);
        println!("cargo:rustc-link-lib=static=mruby");
    }

    // wasi-sdk setjmp library — required because the wasi libmruby.a
    // uses setjmp/longjmp via the new WebAssembly exception handling
    // mechanism (`build_config/mruby.rb` sets
    // `-mllvm -wasm-use-legacy-eh=false`). This produces calls to
    // `__wasm_setjmp`, `__wasm_longjmp`, and `__wasm_setjmp_test`
    // which live in wasi-sdk's `libsetjmp.a` (not in Rust's
    // wasm32-wasip1 self-contained libc). Without this library,
    // rust-lld's `--allow-undefined` flag would turn these into wasm
    // imports that the host cannot satisfy. Host builds use the
    // platform's native setjmp from libc — no extra directive.
    if let Some(wasi_sdk) = wasi_sdk {
        let setjmp_dir = format!("{}/share/wasi-sysroot/lib/wasm32-wasi", wasi_sdk);
        println!("cargo:rustc-link-search=native={}", setjmp_dir);
        println!("cargo:rustc-link-lib=static=setjmp");
    }

    println!("cargo:rustc-cfg=mruby_linked");
    // Downstream build scripts (the `beni` wrapper crate) read
    // DEP_MRUBY_LINKED to gate their own `mruby_linked` cfg.
    println!("cargo:linked=1");
}

fn run_bindgen(
    manifest_dir: &Path,
    mruby_include: &Path,
    mruby_build_include: &Path,
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
        .clang_arg(format!("-I{}", mruby_include.display()))
        .clang_arg(format!("-I{}", mruby_build_include.display()))
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
    mruby_include: &Path,
    mruby_build_include: &Path,
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
        .include(mruby_include)
        .include(mruby_build_include)
        .compile("beni_mruby_trampolines");
}
