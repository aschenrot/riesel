//! Generate the local and managed bindings of one component from the
//! explicit capability contract list. Rust trait impls remain ordinary Rust.

use proc_macro2::TokenStream;
use quote::quote;
use std::collections::BTreeSet;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Ident, ItemStruct, LitStr, Path, Token, bracketed};

pub(crate) struct ComponentArgs {
    id: Option<LitStr>,
    provides: Vec<Path>,
    factory: Option<Path>,
    factory_with_blocking: Option<Path>,
}

impl Parse for ComponentArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut id = None;
        let mut provides = None;
        let mut factory = None;
        let mut factory_with_blocking = None;
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            if key == "id" {
                if id.is_some() {
                    return Err(syn::Error::new(key.span(), "duplicate component id"));
                }
                id = Some(input.parse::<LitStr>()?);
            } else if key == "provides" {
                if provides.is_some() {
                    return Err(syn::Error::new(key.span(), "duplicate provides list"));
                }
                let content;
                bracketed!(content in input);
                let paths = Punctuated::<Path, Token![,]>::parse_terminated(&content)?;
                provides = Some(paths.into_iter().collect::<Vec<_>>());
            } else if key == "factory" {
                if factory.is_some() {
                    return Err(syn::Error::new(key.span(), "duplicate component factory"));
                }
                factory = Some(input.parse::<Path>()?);
            } else if key == "factory_with_blocking" {
                if factory_with_blocking.is_some() {
                    return Err(syn::Error::new(
                        key.span(),
                        "duplicate factory_with_blocking",
                    ));
                }
                factory_with_blocking = Some(input.parse::<Path>()?);
            } else {
                return Err(syn::Error::new(
                    key.span(),
                    "expected id, provides, factory or factory_with_blocking",
                ));
            }

            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        let provides = provides.ok_or_else(|| input.error("missing provides = [Contract, ...]"))?;
        if provides.is_empty() {
            return Err(input.error("a component must provide at least one capability"));
        }
        if factory.is_some() && factory_with_blocking.is_some() {
            return Err(input.error("choose either factory or factory_with_blocking, not both"));
        }
        Ok(Self {
            id,
            provides,
            factory,
            factory_with_blocking,
        })
    }
}

pub(crate) fn expand_component(args: ComponentArgs, item: ItemStruct) -> syn::Result<TokenStream> {
    if !item.generics.params.is_empty() || item.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &item.generics,
            "component attributes currently support concrete structs, not generic component types",
        ));
    }

    if let Some(id) = &args.id {
        crate::identity::validate_explicit_id(id)?;
    }
    let mut registered = BTreeSet::new();
    for provided in &args.provides {
        if !registered.insert(quote!(#provided).to_string()) {
            return Err(syn::Error::new_spanned(
                provided,
                "duplicate provided capability",
            ));
        }
    }

    let name = &item.ident;
    let id = crate::identity::declaration_id(args.id.as_ref(), name);
    let mut definition = if let Some(factory) = args.factory_with_blocking {
        quote!(::riesel_app::ComponentDefinition::with_blocking(#id, #factory))
    } else {
        let constructor = match args.factory {
            Some(path) => quote!(#path),
            None => quote!(<Self as ::std::default::Default>::default),
        };
        quote!(::riesel_app::ComponentDefinition::new(#id, #constructor))
    };
    let mut contracts = args.provides.into_iter().peekable();
    while let Some(contract) = contracts.next() {
        // The final registration returns Result directly. Do not generate
        // Ok(expression?) and trigger needless_question_mark under Clippy.
        if contracts.peek().is_some() {
            definition = quote!(#definition.exposes_contract::<#contract>()?);
        } else {
            definition = quote!(#definition.exposes_contract::<#contract>());
        }
    }

    Ok(quote! {
        #item

        impl ::riesel_app::DeclarativeComponent for #name {
            fn component_definition()
                -> ::std::result::Result<
                    ::riesel_app::ComponentDefinition<Self>,
                    ::riesel_app::ConnectorError
                >
            {
                #definition
            }
        }
    })
}

#[cfg(test)]
mod tests;
