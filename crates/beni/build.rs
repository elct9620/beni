// build.rs — turns the configured integer width `beni-sys` publishes
// into the `mrb_int64` cfg, which gates the integer conversions whose
// every value fits a 64-bit width.

include!("build/width.rs");

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build/width.rs");
    println!("cargo:rerun-if-env-changed={INTEGER_WIDTH_METADATA}");
    println!("cargo:rustc-check-cfg=cfg(mrb_int64)");

    let metadata = std::env::var(INTEGER_WIDTH_METADATA).ok();
    if configured_mrb_int64(metadata.as_deref()) {
        println!("cargo:rustc-cfg=mrb_int64");
    }
}
