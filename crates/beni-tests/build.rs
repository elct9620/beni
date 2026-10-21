// build.rs — turns the debug-hook metadata `beni-sys` publishes into the
// `mrb_debug_hook` cfg, so the hook tests compile exactly where `beni`
// carries the hook surface.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=DEP_MRUBY_DEFINES_MRB_DEBUG_HOOK");
    println!("cargo:rustc-check-cfg=cfg(mrb_debug_hook)");
    if std::env::var("DEP_MRUBY_DEFINES_MRB_DEBUG_HOOK").as_deref() == Ok("true") {
        println!("cargo:rustc-cfg=mrb_debug_hook");
    }
}
