# beni-macros

Attribute and derive macros for [beni](https://crates.io/crates/beni),
the typed Rust wrapper over the mruby C API. Use them through `beni` —
`#[beni::wrap]`, `#[derive(beni::TypedData)]`, and
`#[derive(beni::InlineStruct)]` — rather than
depending on this crate directly; the generated code names `beni`'s
own paths.
