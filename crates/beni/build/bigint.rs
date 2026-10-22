// Whether the archive is a bigint archive, taken from the metadata
// `beni-sys` publishes for it.
//
// A build script is outside `cargo test`'s reach, so the read lives
// here and both `build.rs` and the library's test build include it.

/// The variable the bigint metadata reaches this build through.
const BIGINT_METADATA: &str = "DEP_MRUBY_DEFINES_MRB_BIGINT";

/// Whether the archive was built with `MRB_USE_BIGINT`, from the
/// metadata's value. A build without the metadata, or with a value other
/// than `true` or `false`, fails loudly: the arbitrary-width surface calls
/// functions only such an archive links, so its presence is never guessed.
fn configured_mrb_bigint(metadata: Option<&str>) -> bool {
    match metadata {
        Some("true") => true,
        Some("false") => false,
        _ => unpublished_metadata(
            BIGINT_METADATA,
            "`true` or `false` for whether the archive is a bigint archive",
            metadata,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::configured_mrb_bigint;

    #[test]
    fn a_bigint_archive_is_read_from_the_metadata() {
        assert!(configured_mrb_bigint(Some("true")));
    }

    #[test]
    fn any_other_archive_is_read_from_the_metadata() {
        assert!(!configured_mrb_bigint(Some("false")));
    }

    #[test]
    #[should_panic(expected = "DEP_MRUBY_DEFINES_MRB_BIGINT is unset")]
    fn a_build_without_the_bigint_metadata_stops() {
        configured_mrb_bigint(None);
    }
}
