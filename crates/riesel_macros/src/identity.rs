//! Shared validation of user-supplied published identities and
//! generation of deterministic internal identities for Rust declarations.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, LitStr};

pub(crate) fn validate_explicit_id(id: &LitStr) -> syn::Result<()> {
    let segments: Vec<_> = id.value().split('.').map(str::to_owned).collect();
    if segments.len() < 2
        || segments.iter().any(|segment| {
            let mut chars = segment.chars();
            !matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
                || !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        })
    {
        return Err(syn::Error::new(
            id.span(),
            "invalid dotted capability/provider identifier",
        ));
    }
    Ok(())
}

pub(crate) fn declaration_id(id: Option<&LitStr>, name: &Ident) -> TokenStream {
    match id {
        Some(explicit) => quote!(#explicit.to_owned()),
        None => quote!(::riesel_schema::CapabilityRef::private_rust_id(
            module_path!(),
            stringify!(#name),
        )),
    }
}
