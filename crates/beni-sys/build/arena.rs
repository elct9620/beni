// The GC arena configuration the bindings a build uses declare.
//
// A build script is outside `cargo test`'s reach, so the check lives
// here and both `build.rs` and the library's test build include it.

/// Stop the build when the bindings at `bindings_rs` declare a
/// fixed-size GC arena, which the crates above do not support. mruby
/// declares the arena's pre-allocated overflow error on `mrb_state` in
/// that configuration alone.
fn refuse_fixed_arena(bindings_rs: &std::path::Path) {
    let bindings = std::fs::read_to_string(bindings_rs).unwrap_or_default();
    if bindings
        .lines()
        .any(|line| line.trim_start().starts_with("pub arena_err:"))
    {
        panic!(
            "beni-sys: {} declares a fixed-size GC arena (MRB_GC_FIXED_ARENA). \
             An archive configured that way is outside what the crates above \
             support.",
            bindings_rs.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{bindings, refuse_fixed_arena};

    #[test]
    fn a_growable_arena_builds() {
        refuse_fixed_arena(&bindings(
            "growable",
            "pub struct mrb_state {\n    pub nomem_err: *mut RObject,\n    \
             pub stack_err: *mut RObject,\n}\n",
        ));
    }

    #[test]
    #[should_panic(expected = "declares a fixed-size GC arena")]
    fn a_fixed_arena_stops_the_build() {
        refuse_fixed_arena(&bindings(
            "fixed",
            "pub struct mrb_state {\n    pub nomem_err: *mut RObject,\n    \
             pub stack_err: *mut RObject,\n    pub arena_err: *mut RObject,\n}\n",
        ));
    }
}
