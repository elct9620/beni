// build.rs — reads the metadata `beni-sys` publishes as `beni` reads it,
// so a test compiles exactly where `beni` carries what it reaches: the
// `mrb_debug_hook` cfg gates the hook tests, and `mrb_int64` the cases a
// 32-bit `mrb_int` alone can reach.

include!("../beni/build/debug_hook.rs");
include!("../beni/build/metadata.rs");
include!("../beni/build/width.rs");

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../beni/build/debug_hook.rs");
    println!("cargo:rerun-if-changed=../beni/build/metadata.rs");
    println!("cargo:rerun-if-changed=../beni/build/width.rs");
    println!("cargo:rerun-if-env-changed={DEBUG_HOOK_METADATA}");
    println!("cargo:rerun-if-env-changed={INTEGER_WIDTH_METADATA}");
    println!("cargo:rerun-if-env-changed={FLOAT_WIDTH_METADATA}");
    println!("cargo:rustc-check-cfg=cfg(mrb_debug_hook)");
    println!("cargo:rustc-check-cfg=cfg(mrb_int64)");
    println!("cargo:rustc-check-cfg=cfg(mrb_float32)");

    let debug_hook = std::env::var(DEBUG_HOOK_METADATA).ok();
    if configured_mrb_debug_hook(debug_hook.as_deref()) {
        println!("cargo:rustc-cfg=mrb_debug_hook");
    }

    let integer = std::env::var(INTEGER_WIDTH_METADATA).ok();
    if configured_mrb_int64(integer.as_deref()) {
        println!("cargo:rustc-cfg=mrb_int64");
    }

    let float = std::env::var(FLOAT_WIDTH_METADATA).ok();
    if configured_mrb_float32(float.as_deref()) {
        println!("cargo:rustc-cfg=mrb_float32");
    }
}
