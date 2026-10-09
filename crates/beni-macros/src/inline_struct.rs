use crate::attr::{
    carrier_site, class_and_name, derive_attribute, reject_field_attributes, site_ident,
};
use proc_macro2::TokenStream;
use quote::quote;
use syn::{spanned::Spanned, Data, DeriveInput, Error};

pub fn expand_derive(input: DeriveInput) -> Result<TokenStream, Error> {
    let attr = derive_attribute(&input, "InlineStruct")?;
    if !matches!(input.data, Data::Struct(_)) {
        return Err(Error::new(
            input.span(),
            "InlineStruct can be derived for a struct alone",
        ));
    }
    let (class, name) = class_and_name(attr)?;
    reject_field_attributes(&input.data)?;

    let ident = &input.ident;
    let site = carrier_site(site_ident("CLASS"), &class, "InlineStruct")?;
    let (site_decl, site_ident, path) = (&site.decl, &site.ident, &site.path);

    // The bound is checked where the type is declared, so a payload too
    // large fails here rather than at its first use.
    Ok(quote! {
        const _: () = ::core::assert!(
            ::core::mem::size_of::<#ident>() <= 3 * ::core::mem::size_of::<*const ()>()
                && ::core::mem::align_of::<#ident>() <= ::core::mem::align_of::<*const ()>(),
            "an InlineStruct payload fits three pointer widths at pointer alignment",
        );

        const _: () = {
        #site_decl

        unsafe impl ::beni::InlineStruct for #ident {
            fn class(mrb: &::beni::Mrb) -> ::beni::RClass {
                mrb.get_inner(&#site_ident)
            }

            fn inline_type() -> &'static ::beni::InlineType<Self> {
                static INLINE_TYPE: ::beni::InlineType<#ident> = ::beni::InlineType::new(#name);
                &INLINE_TYPE
            }

            fn mark_carriers(mrb: &::beni::Mrb) -> ::core::result::Result<(), ::beni::Error> {
                mrb.mark_inline_carrier_site::<Self>(&#site_ident, #path)?;
                ::core::result::Result::Ok(())
            }
        }
        };

        impl ::beni::TryConvert for #ident {
            fn try_convert(
                val: ::beni::Value,
                mrb: &::beni::Mrb,
            ) -> ::core::result::Result<Self, ::beni::Error> {
                <::beni::Inline<Self> as ::beni::TryConvert>::try_convert(val, mrb)
                    .map(::beni::Inline::get)
            }
        }

        // SAFETY: an `InlineStruct` is `bytemuck::Pod`, which no type
        // holding a `Value` is.
        unsafe impl ::beni::TryConvertOwned for #ident {}

        impl ::beni::IntoValue for #ident {
            fn into_value(self, mrb: &::beni::Mrb) -> ::beni::Value {
                ::beni::ReprValue::as_value(::beni::Inline::new(mrb, self))
            }
        }
    })
}
