use crate::attr::{
    beni_attribute, carrier_path, class_and_name, reject_field_attributes, unmarked,
};
use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use syn::{spanned::Spanned, Data, DeriveInput, Error};

pub fn expand_derive(input: DeriveInput) -> Result<TokenStream, Error> {
    let attr = beni_attribute(&input.attrs)?
        .ok_or_else(|| Error::new(input.span(), "missing #[beni(class = \"...\")] attribute"))?;
    if !input.generics.to_token_stream().is_empty() {
        return Err(Error::new_spanned(
            &input.generics,
            "InlineStruct cannot be derived for a type with generic parameters or lifetimes",
        ));
    }
    if !matches!(input.data, Data::Struct(_)) {
        return Err(Error::new(
            input.span(),
            "InlineStruct can be derived for a struct alone",
        ));
    }
    let (class, name) = class_and_name(attr)?;
    reject_field_attributes(&input.data)?;

    let ident = &input.ident;
    let path = carrier_path(&class)?;
    let unmarked = unmarked(&class, "InlineStruct");

    // The bound is checked where the type is declared, so a payload too
    // large fails here rather than at its first use.
    Ok(quote! {
        const _: () = ::core::assert!(
            ::core::mem::size_of::<#ident>() <= 3 * ::core::mem::size_of::<*const ()>()
                && ::core::mem::align_of::<#ident>() <= ::core::mem::align_of::<*const ()>(),
            "an InlineStruct payload fits three pointer widths at pointer alignment",
        );

        unsafe impl ::beni::InlineStruct for #ident {
            fn class(mrb: &::beni::Mrb) -> ::beni::RClass {
                mrb.inline_carrier::<Self>().unwrap_or_else(|| #unmarked)
            }

            fn inline_type() -> &'static ::beni::InlineType<Self> {
                static INLINE_TYPE: ::beni::InlineType<#ident> = ::beni::InlineType::new(#name);
                &INLINE_TYPE
            }

            fn mark_carriers(mrb: &::beni::Mrb) -> ::core::result::Result<(), ::beni::Error> {
                mrb.mark_inline_carrier::<Self>(#path)?;
                ::core::result::Result::Ok(())
            }
        }

        impl ::beni::TryConvert for #ident {
            fn try_convert(
                val: ::beni::Value,
                mrb: &::beni::Mrb,
            ) -> ::core::result::Result<Self, ::beni::Error> {
                <::beni::Inline<Self> as ::beni::TryConvert>::try_convert(val, mrb)
                    .map(::beni::Inline::get)
            }
        }

        impl ::beni::IntoValue for #ident {
            fn into_value(self, mrb: &::beni::Mrb) -> ::beni::Value {
                ::beni::ReprValue::as_value(::beni::Inline::new(mrb, self))
            }
        }
    })
}
