// Whether the archive carries mruby's debug hooks, taken from the
// metadata `beni-sys` publishes for it.
//
// A build script is outside `cargo test`'s reach, so the read lives
// here and both `build.rs` and the library's test build include it.

/// The variable the debug-hook metadata reaches this build through.
const DEBUG_HOOK_METADATA: &str = "DEP_MRUBY_DEFINES_MRB_DEBUG_HOOK";

/// Whether the archive was built with `MRB_USE_DEBUG_HOOK`, from the
/// metadata's value. A build without the metadata, or with a value other
/// than `true` or `false`, fails loudly: the hook surface reads fields
/// only such an archive's state carries, so its presence is never guessed.
fn configured_mrb_debug_hook(metadata: Option<&str>) -> bool {
    match metadata {
        Some("true") => true,
        Some("false") => false,
        _ => unpublished_metadata(
            DEBUG_HOOK_METADATA,
            "`true` or `false` for whether the archive carries the debug hooks",
            metadata,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::configured_mrb_debug_hook;

    #[test]
    fn an_archive_with_the_debug_hooks_is_read_from_the_metadata() {
        assert!(configured_mrb_debug_hook(Some("true")));
    }

    #[test]
    fn an_archive_without_the_debug_hooks_is_read_from_the_metadata() {
        assert!(!configured_mrb_debug_hook(Some("false")));
    }

    #[test]
    #[should_panic(expected = "DEP_MRUBY_DEFINES_MRB_DEBUG_HOOK is unset")]
    fn a_build_without_the_debug_hook_metadata_stops() {
        configured_mrb_debug_hook(None);
    }
}
