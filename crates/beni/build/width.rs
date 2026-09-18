// The configured integer and float widths, taken from the metadata
// `beni-sys` publishes for each.
//
// A build script is outside `cargo test`'s reach, so the read lives
// here and both `build.rs` and the library's test build include it.

/// The variable the integer-width metadata reaches this build through.
const INTEGER_WIDTH_METADATA: &str = "DEP_MRUBY_DEFINES_MRB_INT64";

/// Whether the configured integer width is 64 bits, from the metadata's
/// value. A build without the metadata, or with a value other than
/// `true` or `false`, fails loudly: the integer conversions this crate
/// offers depend on the width, so none of them is guessed.
fn configured_mrb_int64(metadata: Option<&str>) -> bool {
    match metadata {
        Some("true") => true,
        Some("false") => false,
        _ => panic!(
            "beni: {INTEGER_WIDTH_METADATA} is {}, where `beni-sys` publishes \
             `true` or `false` for the configured integer width. Build `beni` \
             against the `beni-sys` released beside it.",
            metadata.map_or("unset".to_owned(), |value| format!("`{value}`"))
        ),
    }
}

/// The variable the float-width metadata reaches this build through.
const FLOAT_WIDTH_METADATA: &str = "DEP_MRUBY_DEFINES_MRB_FLOAT32";

/// Whether the configured float width is 32 bits, from the metadata's
/// value. A build without the metadata, or with a value other than
/// `true` or `false`, fails loudly for the same reason the integer read
/// does: the float conversions this crate offers depend on the width.
fn configured_mrb_float32(metadata: Option<&str>) -> bool {
    match metadata {
        Some("true") => true,
        Some("false") => false,
        _ => panic!(
            "beni: {FLOAT_WIDTH_METADATA} is {}, where `beni-sys` publishes \
             `true` or `false` for the configured float width. Build `beni` \
             against the `beni-sys` released beside it.",
            metadata.map_or("unset".to_owned(), |value| format!("`{value}`"))
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{configured_mrb_float32, configured_mrb_int64};

    #[test]
    fn a_64_bit_width_is_read_from_the_metadata() {
        assert!(configured_mrb_int64(Some("true")));
    }

    #[test]
    fn a_32_bit_width_is_read_from_the_metadata() {
        assert!(!configured_mrb_int64(Some("false")));
    }

    #[test]
    #[should_panic(expected = "DEP_MRUBY_DEFINES_MRB_INT64 is unset")]
    fn a_build_without_the_metadata_stops() {
        configured_mrb_int64(None);
    }

    #[test]
    #[should_panic(expected = "DEP_MRUBY_DEFINES_MRB_INT64 is `64`")]
    fn a_value_other_than_true_or_false_stops_the_build() {
        configured_mrb_int64(Some("64"));
    }

    #[test]
    fn a_32_bit_float_width_is_read_from_the_metadata() {
        assert!(configured_mrb_float32(Some("true")));
    }

    #[test]
    fn a_64_bit_float_width_is_read_from_the_metadata() {
        assert!(!configured_mrb_float32(Some("false")));
    }

    #[test]
    #[should_panic(expected = "DEP_MRUBY_DEFINES_MRB_FLOAT32 is unset")]
    fn a_build_without_the_float_metadata_stops() {
        configured_mrb_float32(None);
    }
}
