// Whether the bindings a build uses carry mruby's debug hooks, published
// to the crates that depend on this one directly.
//
// A build script is outside `cargo test`'s reach, so the read lives
// here and both `build.rs` and the library's test build include it.

/// The `links` metadata key carrying whether the archive was built with
/// `MRB_USE_DEBUG_HOOK`, which a direct dependent's build reads as
/// `DEP_MRUBY_DEFINES_MRB_DEBUG_HOOK`.
const DEBUG_HOOK_METADATA: &str = "defines_mrb_debug_hook";

/// Whether `bindings` declare the code fetch hook, the field mruby adds to
/// its state only under `MRB_USE_DEBUG_HOOK`. The define carries no value
/// for the bindings to declare, so the field is what records it.
fn declares_debug_hook(bindings: &Bindings) -> bool {
    bindings
        .text
        .lines()
        .any(|line| line.trim_start().starts_with("pub code_fetch_hook:"))
}

/// The build-script directive publishing whether `bindings` carry the
/// debug hooks as the debug-hook metadata.
fn debug_hook_directive(bindings: &Bindings) -> String {
    format!(
        "cargo:{DEBUG_HOOK_METADATA}={}",
        declares_debug_hook(bindings)
    )
}

#[cfg(test)]
mod tests {
    use super::{bindings, debug_hook_directive};

    #[test]
    fn a_state_with_the_code_fetch_hook_carries_the_debug_hooks() {
        let path = bindings(
            "hooked",
            "pub struct mrb_state {\n    pub code_fetch_hook: ::std::option::Option<\n",
        );
        assert_eq!(
            debug_hook_directive(&path),
            "cargo:defines_mrb_debug_hook=true"
        );
    }

    #[test]
    fn a_state_without_the_code_fetch_hook_carries_none() {
        let path = bindings(
            "plain",
            "pub struct mrb_state {\n    pub ud: *mut c_void,\n",
        );
        assert_eq!(
            debug_hook_directive(&path),
            "cargo:defines_mrb_debug_hook=false"
        );
    }
}
