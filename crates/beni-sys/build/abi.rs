// The calling convention the bindings declare mruby's functions with.
//
// A build script is outside `cargo test`'s reach, so the rewrite lives
// here and both `build.rs` and the library's test build include it.

/// Reopen each foreign block bindgen wrote as a `mruby_extern!`
/// invocation, which declares the block's functions under the ABI the
/// crate's panic strategy calls for. Function-pointer types keep the ABI
/// bindgen gave them: mruby calls those, and they never unwind into it.
fn route_foreign_blocks(bindings: &str) -> String {
    let mut routed = String::with_capacity(bindings.len());
    for line in bindings.lines() {
        match line.trim() {
            "extern \"C\" {" | "unsafe extern \"C\" {" => routed.push_str("mruby_extern! {"),
            _ => routed.push_str(line),
        }
        routed.push('\n');
    }
    routed
}

#[cfg(test)]
mod tests {
    use super::route_foreign_blocks;

    #[test]
    fn a_foreign_block_opens_as_the_routing_macro_in_either_spelling() {
        let routed = route_foreign_blocks(
            "unsafe extern \"C\" {\n    pub fn mrb_raise();\n}\nextern \"C\" {\n    pub fn mrb_exc_raise();\n}\n",
        );

        assert_eq!(
            routed,
            "mruby_extern! {\n    pub fn mrb_raise();\n}\nmruby_extern! {\n    pub fn mrb_exc_raise();\n}\n"
        );
    }

    #[test]
    fn a_function_pointer_type_keeps_its_abi() {
        let source = "pub type mrb_protect_error_func =\n    Option<unsafe extern \"C\" fn(mrb: *mut mrb_state) -> mrb_value>;\n";

        assert_eq!(route_foreign_blocks(source), source);
    }
}
