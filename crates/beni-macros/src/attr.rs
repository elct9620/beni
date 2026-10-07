//! The `#[beni(...)]` attribute reading both derives share.

use proc_macro2::{Ident, Literal, TokenStream};
use quote::{quote, ToTokens};
use std::ffi::CString;
use syn::{spanned::Spanned, Attribute, Data, Error, Field, LitStr};

/// The `class` and `name` a type-level `#[beni]` attribute gives, `name`
/// defaulting to `class`, as the C string the descriptor carries.
pub fn class_and_name(attr: &Attribute) -> Result<(LitStr, Literal), Error> {
    let mut class = None;
    let mut name = None;
    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("class") {
            class = Some(nul_free(meta.value()?.parse()?)?);
        } else if meta.path.is_ident("name") {
            name = Some(nul_free(meta.value()?.parse()?)?);
        } else {
            return Err(unsupported(&meta, "`class` and `name`"));
        }
        Ok(())
    })?;
    let class = class.ok_or_else(|| Error::new(attr.span(), "missing attribute: `class = ...`"))?;
    let name = name.unwrap_or_else(|| class.clone());
    let name = Literal::c_string(&CString::new(name.value()).expect("checked NUL-free"));
    Ok((class, name))
}

/// A naming site: the `Lazy` static `ident` holding, in each
/// interpreter, the class `path` was marked as. Read before it is marked,
/// it panics naming the `mark_carriers` call that marks it. Nothing
/// resolves on a read: the path resolved when the type's carriers were
/// marked, so what a Ruby program binds over it reaches no wrap.
pub struct CarrierSite {
    pub ident: Ident,
    pub path: Literal,
    pub decl: TokenStream,
}

pub fn carrier_site(ident: Ident, path: &LitStr, trait_name: &str) -> Result<CarrierSite, Error> {
    let literal = carrier_path(path)?;
    let unmarked = unmarked(path, trait_name);
    let decl = quote! {
        static #ident: ::beni::value::Lazy<::beni::RClass> =
            ::beni::value::Lazy::new(|_| #unmarked);
    };
    Ok(CarrierSite {
        ident,
        path: literal,
        decl,
    })
}

/// The panic naming the `mark_carriers` call that holds `path`'s class.
pub fn unmarked(path: &LitStr, trait_name: &str) -> TokenStream {
    let text = path.value();
    let message = format!(
        "{{}} was never marked as a carrier class in this interpreter; \
         call <Self as ::beni::{trait_name}>::mark_carriers while the gem installs"
    );
    quote! { panic!(#message, #text) }
}

/// A class path as the C string keying it, rejecting a path holding a
/// segment no constant fetch could resolve.
pub fn carrier_path(path: &LitStr) -> Result<Literal, Error> {
    let text = path.value();
    if text.split("::").any(str::is_empty) {
        return Err(Error::new(path.span(), "class path holds an empty segment"));
    }
    Ok(Literal::c_string(
        &CString::new(text).expect("checked NUL-free"),
    ))
}

/// A field takes no `#[beni]` attribute.
pub fn reject_field_attributes(data: &Data) -> Result<(), Error> {
    let mut fields: Box<dyn Iterator<Item = &Field>> = match data {
        Data::Struct(data) => Box::new(data.fields.iter()),
        Data::Enum(data) => Box::new(data.variants.iter().flat_map(|v| v.fields.iter())),
        Data::Union(data) => Box::new(data.fields.named.iter()),
    };
    match fields.find_map(|field| field.attrs.iter().find(|a| a.path().is_ident("beni"))) {
        Some(attr) => Err(Error::new(attr.span(), "unsupported attribute on a field")),
        None => Ok(()),
    }
}

pub fn beni_attribute(attrs: &[Attribute]) -> Result<Option<&Attribute>, Error> {
    let mut found = attrs.iter().filter(|attr| attr.path().is_ident("beni"));
    let first = found.next();
    if let Some(duplicate) = found.next() {
        return Err(Error::new(duplicate.span(), "duplicate #[beni] attribute"));
    }
    Ok(first)
}

pub fn nul_free(lit: LitStr) -> Result<LitStr, Error> {
    if lit.value().contains('\0') {
        return Err(Error::new(lit.span(), "must not contain a NUL byte"));
    }
    Ok(lit)
}

/// An attribute the macros do not accept: mruby's data type carries a
/// name and a release hook and nothing else to configure.
pub fn unsupported(meta: &syn::meta::ParseNestedMeta, accepted: &str) -> Error {
    let name = meta.path.to_token_stream().to_string();
    meta.error(format!(
        "unsupported attribute `{name}`; accepted here: {accepted}"
    ))
}
