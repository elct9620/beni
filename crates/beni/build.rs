// build.rs — turns the configured widths `beni-sys` publishes into the
// `mrb_int64` and `mrb_float32` cfgs, which gate the conversions whose
// every value fits the width the archive was built with.

include!("build/width.rs");

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build/width.rs");
    println!("cargo:rerun-if-env-changed={INTEGER_WIDTH_METADATA}");
    println!("cargo:rerun-if-env-changed={FLOAT_WIDTH_METADATA}");
    println!("cargo:rustc-check-cfg=cfg(mrb_int64)");
    println!("cargo:rustc-check-cfg=cfg(mrb_float32)");

    let integer = std::env::var(INTEGER_WIDTH_METADATA).ok();
    if configured_mrb_int64(integer.as_deref()) {
        println!("cargo:rustc-cfg=mrb_int64");
    }

    let float = std::env::var(FLOAT_WIDTH_METADATA).ok();
    if configured_mrb_float32(float.as_deref()) {
        println!("cargo:rustc-cfg=mrb_float32");
    }
}
