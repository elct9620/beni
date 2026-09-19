use proc_macro2::{Literal, TokenStream};
use quote::{quote, ToTokens};
use std::ffi::CString;
use syn::{spanned::Spanned, Attribute, Data, DeriveInput, Error, Field, LitStr};

pub fn expand_wrap(attrs: TokenStream, item: TokenStream) -> TokenStream {
    quote! {
        #[derive(::beni::TypedData)]
        #[beni(#attrs)]
        #item
    }
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

    let ident = &input.ident;
    let fetch_class = resolve_class(&class);
    let name = Literal::c_string(&CString::new(name.value()).expect("checked NUL-free"));
    let class_for = class_for(&input.data)?;
    reject_field_attributes(&input.data)?;

    // Every class the implementation names is marked as it is named,
    // which is the contract `unsafe impl TypedData` asks of it.
    Ok(quote! {
        unsafe impl ::beni::TypedData for #ident {
            fn class(mrb: &::beni::Mrb) -> ::beni::RClass {
                #fetch_class
            }

            fn data_type() -> &'static ::beni::DataType<Self> {
                static DATA_TYPE: ::beni::DataType<#ident> = ::beni::DataType::new(#name);
                &DATA_TYPE
            }

            #class_for
        }
    })
}

/// Resolve `path` from `Object` in the interpreter at hand and prepare
/// it to carry data. Nothing is cached: one process may run several
/// interpreters, each with its own class.
fn resolve_class(path: &LitStr) -> TokenStream {
    quote! {{
        let path = #path;
        let class = ::beni::ReprValue::as_value(mrb.object_class())
            .funcall(mrb, c"const_get", &[::beni::ReprValue::as_value(mrb.str_new(path.as_bytes()))])
            .and_then(|value| <::beni::RClass as ::beni::TryConvert>::try_convert(value, mrb))
            .unwrap_or_else(|err| panic!("{path} does not name a class: {}", err.message(mrb)));
        class
            .set_instance_data_tt(mrb)
            .unwrap_or_else(|err| panic!("{path} cannot carry Rust data: {}", err.message(mrb)));
        class.undef_default_alloc_func(mrb);
        class
    }}
}

/// `class_for` answering each variant's own class, for an enum whose
/// variants carry `#[beni(class = "...")]`; nothing otherwise.
fn class_for(data: &Data) -> Result<TokenStream, Error> {
    let Data::Enum(data) = data else {
        return Ok(TokenStream::new());
    };
    let mut arms = Vec::new();
    for variant in &data.variants {
        let Some(attr) = beni_attribute(&variant.attrs)? else {
            continue;
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
        let class =
            class.ok_or_else(|| Error::new(attr.span(), "missing attribute: `class = ...`"))?;
        let ident = &variant.ident;
        let fetch_class = resolve_class(&class);
        arms.push(quote! { Self::#ident { .. } => #fetch_class });
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

/// A field takes no `#[beni]` attribute.
fn reject_field_attributes(data: &Data) -> Result<(), Error> {
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

fn beni_attribute(attrs: &[Attribute]) -> Result<Option<&Attribute>, Error> {
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
