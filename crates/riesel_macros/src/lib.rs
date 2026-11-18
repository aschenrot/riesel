//! Rust-first API declarations. Generated metadata uses riesel_schema types.
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;
use syn::{
    Data, DeriveInput, Expr, Fields, FnArg, GenericArgument, Ident, ItemTrait, Lit, LitInt, LitStr,
    Meta, Pat, PathArguments, ReturnType, Token, TraitItem, Type, parse_macro_input,
};

struct CapabilityArgs {
    id: LitStr,
    version: LitInt,
}

impl Parse for CapabilityArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let mut id = None;
        let mut version = None;
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            if key == "id" {
                if id.is_some() {
                    return Err(syn::Error::new(key.span(), "duplicate id"));
                }
                id = Some(input.parse::<LitStr>()?);
            } else if key == "version" {
                if version.is_some() {
                    return Err(syn::Error::new(key.span(), "duplicate version"));
                }
                version = Some(input.parse::<LitInt>()?);
            } else {
                return Err(syn::Error::new(key.span(), "expected id or version"));
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        Ok(Self {
            id: id.ok_or_else(|| input.error("missing id"))?,
            version: version.ok_or_else(|| input.error("missing version"))?,
        })
    }
}

/// Generates a NameContract::definition() method from the annotated Rust trait.
/// Each method must use #[command] or #[query] and return Result<Output, Error>.
#[proc_macro_attribute]
pub fn capability(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as CapabilityArgs);
    let item = parse_macro_input!(input as ItemTrait);
    match expand_capability(args, item) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn expand_capability(
    args: CapabilityArgs,
    mut item: ItemTrait,
) -> syn::Result<proc_macro2::TokenStream> {
    let id = args.id;
    let version = args.version;
    let parsed_version: u32 = version.base10_parse()?;
    if parsed_version == 0 {
        return Err(syn::Error::new(
            version.span(),
            "version must be greater than zero",
        ));
    }
    let segments: Vec<_> = id.value().split('.').map(str::to_owned).collect();
    if segments.len() < 2
        || segments.iter().any(|s| {
            let mut chars = s.chars();
            !matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
                || !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        })
    {
        return Err(syn::Error::new(
            id.span(),
            "invalid dotted capability identifier",
        ));
    }
    if !item.generics.params.is_empty() || item.generics.where_clause.is_some() {
        return Err(syn::Error::new(
            item.generics.span(),
            "generic capability traits are not supported",
        ));
    }
    let trait_name = item.ident.clone();
    let descriptor_name = format_ident!("{}Contract", trait_name);
    let visibility = item.vis.clone();
    let mut generated_operations = Vec::new();

    for entry in &mut item.items {
        let TraitItem::Fn(method) = entry else {
            return Err(syn::Error::new(
                entry.span(),
                "only methods are supported in capability traits",
            ));
        };
        if !method.sig.generics.params.is_empty()
            || method.sig.generics.where_clause.is_some()
            || method.sig.asyncness.is_some()
            || method.sig.unsafety.is_some()
            || method.sig.abi.is_some()
            || method.default.is_some()
        {
            return Err(syn::Error::new(
                method.sig.span(),
                "capability methods must be synchronous, non-generic declarations without default bodies",
            ));
        }

        let mut markers = Vec::new();
        for attr in &method.attrs {
            if attr.path().is_ident("command") || attr.path().is_ident("query") {
                if !matches!(&attr.meta, Meta::Path(_)) {
                    return Err(syn::Error::new(
                        attr.span(),
                        "operation marker takes no arguments",
                    ));
                }
                markers.push(if attr.path().is_ident("command") {
                    "command"
                } else {
                    "query"
                });
            }
        }
        if markers.len() != 1 {
            return Err(syn::Error::new(
                method.sig.span(),
                "each capability method requires exactly one #[command] or #[query]",
            ));
        }
        let kind = if markers[0] == "command" {
            quote!(::riesel_schema::OperationKind::Command)
        } else {
            quote!(::riesel_schema::OperationKind::Query)
        };

        let docs = method
            .attrs
            .iter()
            .filter(|attr| attr.path().is_ident("doc"))
            .filter_map(|attr| match &attr.meta {
                Meta::NameValue(value) => match &value.value {
                    Expr::Lit(lit) => match &lit.lit {
                        Lit::Str(value) => Some(value.value().trim().to_owned()),
                        _ => None,
                    },
                    _ => None,
                },
                _ => None,
            })
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if docs.is_empty() {
            return Err(syn::Error::new(
                method.sig.span(),
                "capability operation needs a Rust doc comment",
            ));
        }
        method
            .attrs
            .retain(|attr| !attr.path().is_ident("command") && !attr.path().is_ident("query"));

        let Some(FnArg::Receiver(receiver)) = method.sig.inputs.first() else {
            return Err(syn::Error::new(
                method.sig.span(),
                "capability methods require &self",
            ));
        };
        if receiver.reference.is_none()
            || receiver.mutability.is_some()
            || receiver.colon_token.is_some()
        {
            return Err(syn::Error::new(
                receiver.span(),
                "capability methods require an immutable &self receiver",
            ));
        }

        let mut arguments = Vec::new();
        let mut type_collection = Vec::new();
        for argument in method.sig.inputs.iter().skip(1) {
            let FnArg::Typed(arg) = argument else {
                return Err(syn::Error::new(argument.span(), "unexpected receiver"));
            };
            let Pat::Ident(param) = &*arg.pat else {
                return Err(syn::Error::new(
                    arg.pat.span(),
                    "parameters must be named identifiers",
                ));
            };
            if param.by_ref.is_some() || param.subpat.is_some() {
                return Err(syn::Error::new(
                    param.span(),
                    "unsupported parameter pattern",
                ));
            }
            let name = &param.ident;
            let ty = &arg.ty;
            arguments.push(quote! {
                ::riesel_schema::ParameterDefinition {
                    name: stringify!(#name).to_owned(),
                    kind: <#ty as ::riesel_schema::RieselType>::type_ref(),
                }
            });
            type_collection.push(quote! {
                <#ty as ::riesel_schema::RieselType>::collect_definitions(&mut types)?;
            });
        }

        let (result_type, error_type) = result_parts(&method.sig.output)?;
        let method_name = &method.sig.ident;
        generated_operations.push(quote! {
            operations.insert(
                stringify!(#method_name).to_owned(),
                ::riesel_schema::OperationDefinition {
                    kind: #kind,
                    description: #docs.to_owned(),
                    inputs: vec![#(#arguments),*],
                    output: <#result_type as ::riesel_schema::RieselType>::type_ref(),
                    error: <#error_type as ::riesel_schema::RieselType>::type_ref(),
                },
            );
            #(#type_collection)*
            <#result_type as ::riesel_schema::RieselType>::collect_definitions(&mut types)?;
            <#error_type as ::riesel_schema::RieselType>::collect_definitions(&mut types)?;
        });
    }
    if generated_operations.is_empty() {
        return Err(syn::Error::new(
            trait_name.span(),
            "capability must contain operations",
        ));
    }

    Ok(quote! {
        #item

        #[derive(Debug, Clone, Copy)]
        #visibility struct #descriptor_name;

        impl #descriptor_name {
            /// Inspect the API contract generated from the Rust trait.
            pub fn definition() -> Result<
                ::riesel_schema::CapabilityContract,
                ::riesel_schema::ContractError
            > {
                let mut operations = ::std::collections::BTreeMap::new();
                let mut types = ::std::collections::BTreeMap::new();
                #(#generated_operations)*
                let contract = ::riesel_schema::CapabilityContract {
                    capability: ::riesel_schema::CapabilityRef {
                        id: #id.to_owned(),
                        version: #version,
                    },
                    operations,
                    types,
                };
                contract.validate()?;
                Ok(contract)
            }
        }
    })
}

fn result_parts(output: &ReturnType) -> syn::Result<(&Type, &Type)> {
    let ReturnType::Type(_, result) = output else {
        return Err(syn::Error::new(
            output.span(),
            "capability methods must return Result<T, E>",
        ));
    };
    let Type::Path(path) = &**result else {
        return Err(syn::Error::new(
            result.span(),
            "capability methods must return Result<T, E>",
        ));
    };
    let Some(last) = path.path.segments.last() else {
        return Err(syn::Error::new(path.span(), "expected Result<T, E>"));
    };
    if last.ident != "Result" {
        return Err(syn::Error::new(
            last.span(),
            "capability methods must return Result<T, E>",
        ));
    }
    let PathArguments::AngleBracketed(args) = &last.arguments else {
        return Err(syn::Error::new(
            last.span(),
            "Result requires output and error types",
        ));
    };
    let types: Vec<_> = args
        .args
        .iter()
        .filter_map(|item| match item {
            GenericArgument::Type(ty) => Some(ty),
            _ => None,
        })
        .collect();
    if types.len() != 2 || args.args.len() != 2 {
        return Err(syn::Error::new(
            args.span(),
            "Result requires two concrete types",
        ));
    }
    Ok((types[0], types[1]))
}

/// Generates RieselType implementations for public API structs and enums.
/// Supports named structs, unit enum variants, and one-field tuple variants.
#[proc_macro_derive(RieselType)]
pub fn derive_riesel_type(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_riesel_type(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn expand_riesel_type(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = input.ident;
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        return Err(syn::Error::new(
            input.generics.span(),
            "generic public API types are not supported yet",
        ));
    }

    let mut dependencies = Vec::new();
    let definition = match input.data {
        Data::Struct(data) => {
            let Fields::Named(named) = data.fields else {
                return Err(syn::Error::new(
                    name.span(),
                    "RieselType requires a struct with named fields",
                ));
            };
            let mut fields = Vec::new();
            for field in named.named {
                let field_name = field.ident.expect("named field");
                let ty = field.ty;
                fields.push(quote! {
                    fields.insert(
                        stringify!(#field_name).to_owned(),
                        <#ty as ::riesel_schema::RieselType>::type_ref(),
                    );
                });
                dependencies.push(quote! {
                    <#ty as ::riesel_schema::RieselType>::collect_definitions(types)?;
                });
            }
            quote! {
                let mut fields = ::std::collections::BTreeMap::new();
                #(#fields)*
                ::riesel_schema::TypeDefinition::Struct { fields }
            }
        }
        Data::Enum(data) => {
            let mut variants = Vec::new();
            for variant in data.variants {
                let variant_name = variant.ident;
                let payload = match variant.fields {
                    Fields::Unit => quote!(None),
                    Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => {
                        let ty = &unnamed.unnamed.first().expect("one field").ty;
                        dependencies.push(quote! {
                            <#ty as ::riesel_schema::RieselType>::collect_definitions(types)?;
                        });
                        quote!(Some(<#ty as ::riesel_schema::RieselType>::type_ref()))
                    }
                    _ => {
                        return Err(syn::Error::new(
                            variant_name.span(),
                            "RieselType supports only unit or single-field tuple enum variants",
                        ));
                    }
                };
                variants.push(quote! {
                    variants.insert(stringify!(#variant_name).to_owned(), #payload);
                });
            }
            quote! {
                let mut variants = ::std::collections::BTreeMap::new();
                #(#variants)*
                ::riesel_schema::TypeDefinition::Enum { variants }
            }
        }
        Data::Union(_) => {
            return Err(syn::Error::new(
                name.span(),
                "unions cannot be public capability types",
            ));
        }
    };

    Ok(quote! {
        impl ::riesel_schema::RieselType for #name {
            fn type_ref() -> ::riesel_schema::TypeRef {
                ::riesel_schema::TypeRef::Named {
                    name: stringify!(#name).to_owned(),
                }
            }

            fn collect_definitions(
                types: &mut ::std::collections::BTreeMap<
                    String,
                    ::riesel_schema::TypeDefinition
                >,
            ) -> Result<(), ::riesel_schema::ContractError> {
                let definition = { #definition };
                let name = stringify!(#name).to_owned();
                if let Some(previous) = types.get(&name) {
                    if previous != &definition {
                        return Err(::riesel_schema::ContractError::ConflictingTypeName(name));
                    }
                    return Ok(());
                }
                types.insert(name, definition);
                #(#dependencies)*
                Ok(())
            }
        }
    })
}

/// Generates a portable Riesel model definition from a named Rust struct.
///
/// Requires #[riesel(model = "name")] and supports simple fields whose types
/// implement ModelField. Serde field mapping options are rejected until Riesel
/// can represent them faithfully, avoiding silent schema/serializer divergence.
#[proc_macro_derive(RieselModel, attributes(riesel))]
pub fn derive_riesel_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand_riesel_model(input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_riesel_model(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = input.ident;
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        return Err(syn::Error::new(
            input.generics.span(),
            "generic Rust models are not supported yet",
        ));
    }
    let mut model = None;
    for attribute in &input.attrs {
        if attribute.path().is_ident("riesel") {
            attribute.parse_nested_meta(|meta| {
                if !meta.path.is_ident("model") {
                    return Err(meta.error("expected model = \"name\""));
                }
                if model.is_some() {
                    return Err(meta.error("duplicate model attribute"));
                }
                model = Some(meta.value()?.parse::<LitStr>()?);
                Ok(())
            })?;
        } else if attribute.path().is_ident("serde")
            && attribute.meta.require_list()?.tokens.to_string() != "deny_unknown_fields"
        {
            return Err(syn::Error::new(
                attribute.span(),
                "RieselModel does not yet support Serde struct renaming or flattening",
            ));
        }
    }

    let model = model.ok_or_else(|| {
        syn::Error::new(
            name.span(),
            "RieselModel requires #[riesel(model = \"name\")]",
        )
    })?;
    let value = model.value();
    let mut chars = value.chars();
    if !matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
        || !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(syn::Error::new(model.span(), "invalid model identifier"));
    }

    let Data::Struct(data) = input.data else {
        return Err(syn::Error::new(
            name.span(),
            "RieselModel requires a named struct",
        ));
    };
    let Fields::Named(fields) = data.fields else {
        return Err(syn::Error::new(
            name.span(),
            "RieselModel requires named fields",
        ));
    };

    let mut generated_fields = Vec::new();
    for field in fields.named {
        if field
            .attrs
            .iter()
            .any(|attribute| attribute.path().is_ident("serde"))
        {
            return Err(syn::Error::new(
                field.span(),
                "Serde field attributes are not supported by RieselModel yet",
            ));
        }
        let identifier = field.ident.expect("named field");
        let ty = field.ty;
        generated_fields.push(quote! {
            fields.insert(
                stringify!(#identifier).to_owned(),
                <#ty as ::riesel::ModelField>::field_type(),
            );
        });
    }

    Ok(quote! {
        impl ::riesel::RieselModel for #name {
            const MODEL_NAME: &'static str = #model;

            fn model_definition() -> ::riesel::schema::ModelDefinition {
                let mut fields = ::std::collections::BTreeMap::new();
                #(#generated_fields)*
                ::riesel::schema::ModelDefinition { fields }
            }
        }
    })
}
