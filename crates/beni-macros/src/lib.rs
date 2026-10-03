//! Attribute and derive macros for the `beni` crate, named through its
//! re-exports — `beni::wrap`, `beni::TypedData`, and `beni::InlineStruct`
//! — as magnus's are through `magnus`.

use proc_macro::TokenStream;

mod attr;
mod inline_struct;
mod typed_data;

/// Implement `beni::TypedData` for a struct or enum, so its values
/// wrap as instances of the class `class` names. Mirrors magnus's
/// `wrap`: `#[beni::wrap(...)]` is `#[derive(beni::TypedData)]` with
/// the same arguments in a `#[beni(...)]` attribute, and with `inline`
/// among them it is `#[derive(beni::InlineStruct)]` instead.
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

/// Derive `beni::InlineStruct` for a plain-data struct from a
/// `#[beni(class = "...")]` attribute, together with the by-value
/// `TryConvert` and `IntoValue` its copies cross through.
#[proc_macro_derive(InlineStruct, attributes(beni))]
pub fn derive_inline_struct(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    inline_struct::expand_derive(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
