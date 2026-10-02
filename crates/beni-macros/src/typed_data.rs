use proc_macro2::{Literal, TokenStream};
use quote::{quote, ToTokens};
use std::ffi::CString;
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::{spanned::Spanned, Attribute, Data, DeriveInput, Error, Field, LitStr, Meta, Token};

pub fn expand_wrap(attrs: TokenStream, item: TokenStream) -> TokenStream {
    let (derive, attrs) = match inline_requested(attrs.clone()) {
        Some(rest) => (quote!(::beni::InlineStruct), rest),
        None => (quote!(::beni::TypedData), attrs),
    };
    quote! {
        #[derive(#derive)]
        #[beni(#attrs)]
        #item
    }
}

/// The arguments left once a bare `inline` is taken out of them, and
/// nothing when `inline` is absent. Arguments that do not parse go on
/// to the derive untouched, which reports them.
fn inline_requested(attrs: TokenStream) -> Option<TokenStream> {
    let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
    let metas = parser.parse2(attrs).ok()?;
    let (inline, rest): (Vec<_>, Vec<_>) = metas
        .into_iter()
        .partition(|meta| matches!(meta, Meta::Path(path) if path.is_ident("inline")));
    if inline.is_empty() {
        return None;
    }
    Some(quote!(#(#rest),*))
}

pub fn expand_derive(input: DeriveInput) -> Result<TokenStream, Error> {
    let attr = beni_attribute(&input.attrs)?
        .ok_or_else(|| Error::new(input.span(), "missing #[beni(class = \"...\")] attribute"))?;
    if !input.generics.to_token_stream().is_empty() {
        return Err(Error::new_spanned(
            &input.generics,
            "TypedData cannot be derived for a type with generic parameters or lifetimes",
        ));
    }

    let (class, name) = class_and_name(attr)?;

    let ident = &input.ident;
    let read_class = read_carrier(&class, "TypedData")?;
    let class_for = class_for(&input.data)?;
    let mark_carriers = mark_carriers(&class, &input.data)?;
    reject_field_attributes(&input.data)?;

    // Every class the implementation names is marked as it is named,
    // which is the contract `unsafe impl TypedData` asks of it.
    Ok(quote! {
        unsafe impl ::beni::TypedData for #ident {
            fn class(mrb: &::beni::Mrb) -> ::beni::RClass {
                #read_class
            }

            fn data_type() -> &'static ::beni::DataType<Self> {
                static DATA_TYPE: ::beni::DataType<#ident> = ::beni::DataType::new(#name);
                &DATA_TYPE
            }

            fn mark_carriers(mrb: &::beni::Mrb) -> ::core::result::Result<(), ::beni::Error> {
                #mark_carriers
                ::core::result::Result::Ok(())
            }

            #class_for
        }
    })
}

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

/// Read the class `path` was marked as in the interpreter at hand.
/// Nothing resolves here: the path resolved when the type's carriers
/// were marked, so what a Ruby program binds over it reaches no wrap.
pub fn read_carrier(path: &LitStr, trait_name: &str) -> Result<TokenStream, Error> {
    let text = path.value();
    let literal = carrier_path(path)?;
    let message = format!(
        "{{}} was never marked as a carrier class in this interpreter; \
         call <Self as ::beni::{trait_name}>::mark_carriers while the gem installs"
    );
    Ok(quote! {
        mrb.carrier(#literal).unwrap_or_else(|| panic!(#message, #text))
    })
}

/// Mark every class this implementation names — the type's own and
/// each variant's — so naming one afterwards reads a prepared class.
fn mark_carriers(class: &LitStr, data: &Data) -> Result<TokenStream, Error> {
    let mut paths = vec![carrier_path(class)?];
    if let Data::Enum(data) = data {
        for variant in &data.variants {
            if let Some(path) = variant_class(variant)? {
                paths.push(carrier_path(&path)?);
            }
        }
    }
    Ok(quote! { #(mrb.mark_carrier(#paths)?;)* })
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

/// `class_for` answering each variant's own class, for an enum whose
/// variants carry `#[beni(class = "...")]`; nothing otherwise.
fn class_for(data: &Data) -> Result<TokenStream, Error> {
    let Data::Enum(data) = data else {
        return Ok(TokenStream::new());
    };
    let mut arms = Vec::new();
    for variant in &data.variants {
        let Some(class) = variant_class(variant)? else {
            continue;
        };
        let ident = &variant.ident;
        let read_class = read_carrier(&class, "TypedData")?;
        arms.push(quote! { Self::#ident { .. } => #read_class });
    }
    if arms.is_empty() {
        return Ok(TokenStream::new());
    }
    Ok(quote! {
        fn class_for(mrb: &::beni::Mrb, value: &Self) -> ::beni::RClass {
            #[allow(unreachable_patterns)]
            match value {
                #(#arms,)*
                _ => <Self as ::beni::TypedData>::class(mrb),
            }
        }
    })
}

/// The class path one enum variant names, and nothing when it carries
/// no `#[beni]` attribute.
fn variant_class(variant: &syn::Variant) -> Result<Option<LitStr>, Error> {
    let Some(attr) = beni_attribute(&variant.attrs)? else {
        return Ok(None);
    };
    let mut class = None;
    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("class") {
            class = Some(nul_free(meta.value()?.parse()?)?);
            Ok(())
        } else {
            Err(unsupported(&meta, "`class`"))
        }
    })?;
    class
        .ok_or_else(|| Error::new(attr.span(), "missing attribute: `class = ...`"))
        .map(Some)
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

fn nul_free(lit: LitStr) -> Result<LitStr, Error> {
    if lit.value().contains('\0') {
        return Err(Error::new(lit.span(), "must not contain a NUL byte"));
    }
    Ok(lit)
}

/// An attribute the macros do not accept: mruby's data type carries a
/// name and a release hook and nothing else to configure.
fn unsupported(meta: &syn::meta::ParseNestedMeta, accepted: &str) -> Error {
    let name = meta.path.to_token_stream().to_string();
    meta.error(format!(
        "unsupported attribute `{name}`; accepted here: {accepted}"
    ))
}
