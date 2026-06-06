// build.rs — mirror beni-sys's `mruby_linked` cfg.
//
// beni-sys's build script publishes the linked signal through its
// `links = "mruby"` key in every build — `cargo:linked=1` with a real
// archive linked, `cargo:linked=0` in placeholder mode; cargo
// surfaces it to this script as DEP_MRUBY_LINKED. The typed wrapper
// derives its own `mruby_linked` cfg from the signal's value so both
// crates always agree on whether the real FFI surface or the host
// placeholders are in play.

use std::env;

fn main() {
    println!("cargo:rerun-if-env-changed=DEP_MRUBY_LINKED");
    println!("cargo:rustc-check-cfg=cfg(mruby_linked)");
    if env::var("DEP_MRUBY_LINKED").as_deref() == Ok("1") {
        println!("cargo:rustc-cfg=mruby_linked");
    }
}
