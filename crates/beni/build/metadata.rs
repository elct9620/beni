// The failure every metadata read shares.
//
// A build script is outside `cargo test`'s reach, so the helper lives
// here and both `build.rs` and the library's test build include it.

/// Stop the build on metadata `beni-sys` did not publish as `published`
/// describes. `metadata` is the value the build received, if any.
fn unpublished_metadata(variable: &str, published: &str, metadata: Option<&str>) -> ! {
    panic!(
        "beni: {variable} is {}, where `beni-sys` publishes {published}. Build \
         `beni` against the `beni-sys` released beside it.",
        metadata.map_or("unset".to_owned(), |value| format!("`{value}`"))
    )
}
