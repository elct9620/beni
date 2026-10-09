use crate::attr::{
    beni_attribute, carrier_site, class_and_name, derive_attribute, reject_field_attributes,
    site_ident, variant_class, CarrierSite,
};
use proc_macro2::TokenStream;
use quote::quote;
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::{Data, DeriveInput, Error, Meta, Token};

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
    let attr = derive_attribute(&input, "TypedData")?;

    let (class, name) = class_and_name(attr)?;

    let ident = &input.ident;
    let own = carrier_site(site_ident("CLASS"), &class, "TypedData")?;
    let variants = variant_sites(&input.data)?;
    let class_for = class_for(&variants);
    reject_field_attributes(&input.data)?;

    let own_site = &own.ident;
    let sites = core::iter::once(&own).chain(variants.iter().map(|(_, site)| site));
    let decls = sites.clone().map(|site| &site.decl);
    let marks = sites.map(|CarrierSite { ident, path, .. }| {
        quote! { mrb.mark_carrier_site(&#ident, #path)?; }
    });

    // Every class the implementation names is marked as it is named,
    // which is the contract `unsafe impl TypedData` asks of it.
    Ok(quote! {
        const _: () = {
            #(#decls)*

            unsafe impl ::beni::TypedData for #ident {
                fn class(mrb: &::beni::Mrb) -> ::beni::RClass {
                    mrb.get_inner(&#own_site)
                }

                fn data_type() -> &'static ::beni::DataType<Self> {
                    static DATA_TYPE: ::beni::DataType<#ident> = ::beni::DataType::new(#name);
                    &DATA_TYPE
                }

                fn mark_carriers(mrb: &::beni::Mrb) -> ::core::result::Result<(), ::beni::Error> {
                    #(#marks)*
                    ::core::result::Result::Ok(())
                }

                #class_for
            }
        };
    })
}

/// The naming site of each enum variant carrying `#[beni(class = "...")]`,
/// beside the variant it names.
fn variant_sites(data: &Data) -> Result<Vec<(&syn::Ident, CarrierSite)>, Error> {
    let Data::Enum(data) = data else {
        return Ok(Vec::new());
    };
    let mut sites = Vec::new();
    for (index, variant) in data.variants.iter().enumerate() {
        if let Some(attr) = beni_attribute(&variant.attrs)? {
            let class = variant_class(attr)?;
            let ident = site_ident(&format!("VARIANT_{index}"));
            sites.push((&variant.ident, carrier_site(ident, &class, "TypedData")?));
        }
    }
    Ok(sites)
}

/// `class_for` answering each variant's own class, for an enum whose
/// variants carry `#[beni(class = "...")]`; nothing otherwise.
fn class_for(variants: &[(&syn::Ident, CarrierSite)]) -> TokenStream {
    if variants.is_empty() {
        return TokenStream::new();
    }
    let arms = variants.iter().map(|(variant, site)| {
        let site = &site.ident;
        quote! { Self::#variant { .. } => mrb.get_inner(&#site) }
    });
    quote! {
        fn class_for(mrb: &::beni::Mrb, value: &Self) -> ::beni::RClass {
            #[allow(unreachable_patterns)]
            match value {
                #(#arms,)*
                _ => <Self as ::beni::TypedData>::class(mrb),
            }
        }
    }
}
