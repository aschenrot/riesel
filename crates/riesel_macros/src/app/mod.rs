//! High-level native async application entrypoint. The expansion reuses
//! the existing typed handler, application builder and provider lifecycle.
use proc_macro2::TokenStream;
use quote::quote;
use std::collections::BTreeSet;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Attribute, FnArg, Ident, ItemFn, LitStr, Pat, Path, ReturnType, Token, Type, bracketed};

pub(crate) struct AppArgs {
    components: Vec<Path>,
    name: Option<LitStr>,
    development: bool,
}

impl Parse for AppArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut components = None;
        let mut name = None;
        let mut development = false;
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            if key == "development" {
                if development {
                    return Err(syn::Error::new(key.span(), "duplicate development option"));
                }
                development = true;
            } else {
                input.parse::<Token![=]>()?;
                if key == "components" {
                    if components.is_some() {
                        return Err(syn::Error::new(key.span(), "duplicate components list"));
                    }
                    let contents;
                    bracketed!(contents in input);
                    let list = Punctuated::<Path, Token![,]>::parse_terminated(&contents)?;
                    let paths: Vec<Path> = list.into_iter().collect();
                    let mut seen = BTreeSet::new();
                    for path in &paths {
                        if !seen.insert(quote!(#path).to_string()) {
                            return Err(syn::Error::new_spanned(
                                path,
                                "component was registered more than once",
                            ));
                        }
                    }
                    components = Some(paths);
                } else if key == "name" {
                    if name.is_some() {
                        return Err(syn::Error::new(key.span(), "duplicate application name"));
                    }
                    let specified: LitStr = input.parse()?;
                    if specified.value().trim().is_empty() {
                        return Err(syn::Error::new_spanned(
                            specified,
                            "application name must not be empty",
                        ));
                    }
                    name = Some(specified);
                } else {
                    return Err(syn::Error::new(
                        key.span(),
                        "expected components = [...], name = \"...\", or development",
                    ));
                }
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(Self {
            components: components.unwrap_or_default(),
            name,
            development,
        })
    }
}

fn extract_binding_options(attrs: &mut Vec<Attribute>) -> syn::Result<(Option<LitStr>, bool)> {
    let mut alias = None;
    let mut parameter = false;
    let mut retained = Vec::new();
    for attr in attrs.drain(..) {
        if !attr.path().is_ident("riesel") {
            retained.push(attr);
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("parameter") {
                if parameter {
                    return Err(meta.error("duplicate riesel parameter option"));
                }
                parameter = true;
                return Ok(());
            }
            if meta.path.is_ident("alias") {
                if alias.is_some() {
                    return Err(meta.error("duplicate riesel alias"));
                }
                let value: LitStr = meta.value()?.parse()?;
                if value.value().is_empty() {
                    return Err(meta.error("requirement alias must not be empty"));
                }
                alias = Some(value);
                return Ok(());
            }
            Err(meta.error("expected riesel(alias = \"name\") or riesel(parameter)"))
        })?;
    }
    *attrs = retained;
    Ok((alias, parameter))
}

/// Built-in Riesel application parameter types already implement
/// ApplicationParam. Generated native async client handles use the explicit
/// NativeHandleParam bridge. A type alias to a custom parameter can opt into
/// the former with #[riesel(parameter)], without guessing its contract ID.
fn is_framework_parameter(ty: &Type) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    path.path.segments.last().is_some_and(|segment| {
        matches!(
            segment.ident.to_string().as_str(),
            "Records" | "Capability" | "AsyncCapability" | "AsyncTasks"
        )
    })
}

pub(crate) fn expand_app(args: AppArgs, mut item: ItemFn) -> syn::Result<TokenStream> {
    let sig = &item.sig;
    if sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            &sig.fn_token,
            "riesel::app requires an async fn entrypoint",
        ));
    }
    if sig.constness.is_some()
        || sig.unsafety.is_some()
        || sig.abi.is_some()
        || sig.variadic.is_some()
        || !sig.generics.params.is_empty()
        || sig.generics.where_clause.is_some()
    {
        return Err(syn::Error::new_spanned(
            sig,
            "riesel::app requires a concrete safe async fn without generics or an ABI",
        ));
    }
    if matches!(&sig.output, ReturnType::Default) {
        return Err(syn::Error::new_spanned(
            sig,
            "riesel::app entrypoints must return Result<(), E>; use AppResult<()>",
        ));
    }
    if sig.inputs.len() > 4 {
        return Err(syn::Error::new_spanned(
            &sig.inputs,
            "riesel::app currently supports at most four typed parameters",
        ));
    }

    let mut bindings: Vec<(Type, Option<LitStr>, bool)> = Vec::new();
    for argument in &mut item.sig.inputs {
        let FnArg::Typed(binding) = argument else {
            return Err(syn::Error::new_spanned(
                argument,
                "riesel::app accepts typed parameters, not a self receiver",
            ));
        };
        if !matches!(&*binding.pat, Pat::Ident(_)) {
            return Err(syn::Error::new_spanned(
                &binding.pat,
                "riesel::app requires a named parameter: name: Type",
            ));
        }
        let (alias, parameter) = extract_binding_options(&mut binding.attrs)?;
        bindings.push(((*binding.ty).clone(), alias, parameter));
    }

    let attrs = std::mem::take(&mut item.attrs);
    let name = item.sig.ident.clone();
    let visibility = item.vis.clone();
    let app_name = match args.name {
        Some(name) => quote!(#name),
        None => quote!(env!("CARGO_PKG_NAME")),
    };
    item.sig.ident = Ident::new("__riesel_app_entrypoint", name.span());
    item.vis = syn::Visibility::Inherited;

    let mut arguments = Vec::new();
    let mut call_arguments = Vec::new();
    let mut registered_types = Vec::new();
    for (index, (ty, _, parameter)) in bindings.iter().enumerate() {
        let binding_name = quote::format_ident!("__riesel_parameter_{index}");
        if *parameter || is_framework_parameter(ty) {
            arguments.push(quote!(#binding_name: #ty));
            call_arguments.push(quote!(#binding_name));
            registered_types.push(quote!(#ty));
        } else {
            let registered = quote!(::riesel::NativeHandleParam<#ty>);
            arguments.push(quote!(#binding_name: #registered));
            call_arguments.push(quote!(#binding_name.into_inner()));
            registered_types.push(registered);
        }
    }
    let handler = quote! {
        |#(#arguments),*| async move {
            __riesel_app_entrypoint(#(#call_arguments),*).await
        }
    };
    let builder = if bindings.iter().any(|(_, alias, _)| alias.is_some()) {
        let expressions = registered_types.iter().zip(&bindings).map(|(ty, (_, alias, _))| {
            match alias {
                Some(alias) => quote!(::std::string::String::from(#alias)),
                None => quote!(<#ty as ::riesel::ApplicationParam>::default_alias()?),
            }
        });
        quote! {
            ::riesel::Application::new(#app_name)
                .async_handler_with_aliases(#handler, [#(#expressions),*])?
        }
    } else {
        quote! {
            ::riesel::Application::new(#app_name)
                .async_handler(#handler)?
        }
    };
    let components = args.components;
    let development = if args.development {
        quote! {
            let __riesel_prepared = __riesel_prepared.development()?;
        }
    } else {
        quote!()
    };
    Ok(quote! {
        #(#attrs)*
        #visibility fn #name() -> ::riesel::Result<()> {
            #item

            let __riesel_prepared = #builder;
            #(
                let __riesel_prepared = __riesel_prepared.component::<#components>()?;
            )*
            #development
            __riesel_prepared.run_from_env()
        }
    })
}

#[cfg(test)]
mod tests;
