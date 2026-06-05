// build.rs — mirror beni-sys's `mruby_linked` cfg.
//
// beni-sys's build script publishes `cargo:linked=1` through its
// `links = "mruby"` key when it generated bindings and emitted link
// directives; cargo surfaces that to this script as DEP_MRUBY_LINKED.
// The typed wrapper gates its mruby-calling modules on the same
// `mruby_linked` cfg so both crates always agree on whether the real
// FFI surface or the host placeholders are in play.

use std::env;

fn main() {
    println!("cargo:rerun-if-env-changed=DEP_MRUBY_LINKED");
    println!("cargo:rustc-check-cfg=cfg(mruby_linked)");
    if env::var("DEP_MRUBY_LINKED").is_ok() {
        println!("cargo:rustc-cfg=mruby_linked");
    }
}
