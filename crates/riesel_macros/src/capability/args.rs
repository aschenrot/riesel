use syn::parse::{Parse, ParseStream};
use syn::{Ident, LitInt, LitStr, Token};

pub(crate) struct CapabilityArgs {
    pub(crate) id: LitStr,
    pub(crate) version: LitInt,
    pub(crate) client_trait: bool,
}

impl Parse for CapabilityArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut id = None;
        let mut version = None;
        let mut client_trait = false;
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            if key == "client_trait" {
                if client_trait {
                    return Err(syn::Error::new(key.span(), "duplicate client_trait"));
                }
                client_trait = true;
            } else if key == "id" {
                input.parse::<Token![=]>()?;
                if id.is_some() {
                    return Err(syn::Error::new(key.span(), "duplicate id"));
                }
                id = Some(input.parse::<LitStr>()?);
            } else if key == "version" {
                input.parse::<Token![=]>()?;
                if version.is_some() {
                    return Err(syn::Error::new(key.span(), "duplicate version"));
                }
                version = Some(input.parse::<LitInt>()?);
            } else {
                return Err(syn::Error::new(
                    key.span(),
                    "expected id, version or client_trait",
                ));
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(Self {
            id: id.ok_or_else(|| input.error("missing id"))?,
            version: version.ok_or_else(|| input.error("missing version"))?,
            client_trait,
        })
    }
}
