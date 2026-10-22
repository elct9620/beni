// Whether the bindings a build uses come from a bigint archive, published
// to the crates that depend on this one directly.
//
// A build script is outside `cargo test`'s reach, so the read lives
// here and both `build.rs` and the library's test build include it.

/// The `links` metadata key carrying whether the archive was built with
/// `MRB_USE_BIGINT`, which a direct dependent's build reads as
/// `DEP_MRUBY_DEFINES_MRB_BIGINT`.
const BIGINT_METADATA: &str = "defines_mrb_bigint";

/// Whether `bindings` declare the arbitrary-width Integer read the
/// wrapper copies only under `MRB_USE_BIGINT`. The define carries no
/// value for the bindings to declare, so the declaration is what
/// records it.
fn declares_bigint(bindings: &Bindings) -> bool {
    bindings
        .text
        .lines()
        .any(|line| line.trim_start().starts_with("pub fn mrb_bint_as_uint64("))
}

/// The build-script directive publishing whether `bindings` come from a
/// bigint archive as the bigint metadata.
fn bigint_directive(bindings: &Bindings) -> String {
    format!("cargo:{BIGINT_METADATA}={}", declares_bigint(bindings))
}

#[cfg(test)]
mod tests {
    use super::{bigint_directive, bindings};

    #[test]
    fn bindings_with_the_arbitrary_width_read_come_from_a_bigint_archive() {
        let path = bindings(
            "bigint",
            "unsafe extern \"C\" {\n    pub fn mrb_bint_as_uint64(mrb: *mut mrb_state, x: mrb_value) -> u64;\n}\n",
        );
        assert_eq!(bigint_directive(&path), "cargo:defines_mrb_bigint=true");
    }

    #[test]
    fn bindings_without_it_come_from_any_other_archive() {
        let path = bindings(
            "fixed",
            "unsafe extern \"C\" {\n    pub fn mrb_int_value(mrb: *mut mrb_state, i: mrb_int) -> mrb_value;\n}\n",
        );
        assert_eq!(bigint_directive(&path), "cargo:defines_mrb_bigint=false");
    }
}
