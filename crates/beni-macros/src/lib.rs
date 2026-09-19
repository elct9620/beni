//! Attribute and derive macros for the `beni` crate, named through its
//! re-exports — `beni::wrap` and `beni::TypedData` — as magnus's are
//! through `magnus`.

use proc_macro::TokenStream;

mod typed_data;

/// Implement `beni::TypedData` for a struct or enum, so its values
/// wrap as instances of the class `class` names. Mirrors magnus's
/// `wrap`: `#[beni::wrap(...)]` is `#[derive(beni::TypedData)]` with
/// the same arguments in a `#[beni(...)]` attribute.
#[proc_macro_attribute]
pub fn wrap(attrs: TokenStream, item: TokenStream) -> TokenStream {
    typed_data::expand_wrap(attrs.into(), item.into()).into()
}

/// Derive `beni::TypedData` from a `#[beni(class = "...")]` attribute.
/// Mirrors magnus's `TypedData` derive.
#[proc_macro_derive(TypedData, attributes(beni))]
pub fn derive_typed_data(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    typed_data::expand_derive(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
