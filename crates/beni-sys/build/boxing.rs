// The value boxing the bindings a build uses declare.
//
// A build script is outside `cargo test`'s reach, so the check lives
// here and both `build.rs` and the library's test build include it.

/// Stop the build when `bindings` declare an `mrb_value` in a boxing the
/// crates above do not support. mruby lays the value out as one word `w`
/// under word boxing and as a `value` union beside its type tag under no
/// boxing; any other first field is another boxing.
fn refuse_unsupported_boxing(bindings: &Bindings) {
    let first_field = bindings
        .text
        .lines()
        .skip_while(|line| line.trim_start() != "pub struct mrb_value {")
        .nth(1)
        .and_then(|line| line.trim_start().strip_prefix("pub ")?.split_once(':'))
        .map(|(name, _)| name);
    if !matches!(first_field, Some("w" | "value")) {
        panic!(
            "beni-sys: {} declares an `mrb_value` in neither word boxing nor \
             no boxing, as NaN boxing (MRB_NAN_BOXING) lays it out. An archive \
             configured that way is outside what the crates above support.",
            bindings.path.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{bindings, refuse_unsupported_boxing};

    #[test]
    fn word_boxing_builds() {
        refuse_unsupported_boxing(&bindings(
            "word",
            "pub struct mrb_value {\n    pub w: usize,\n}\n",
        ));
    }

    #[test]
    fn no_boxing_builds() {
        refuse_unsupported_boxing(&bindings(
            "no",
            "pub struct mrb_value {\n    pub value: mrb_value_union,\n    \
             pub tt: mrb_vtype,\n}\n",
        ));
    }

    #[test]
    #[should_panic(expected = "declares an `mrb_value` in neither word boxing nor no boxing")]
    fn nan_boxing_stops_the_build() {
        refuse_unsupported_boxing(&bindings(
            "nan",
            "pub struct mrb_value {\n    pub u: u64,\n}\n",
        ));
    }
}
