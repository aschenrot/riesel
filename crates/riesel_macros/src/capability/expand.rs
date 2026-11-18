use super::CapabilityArgs;
use super::signature::result_parts;
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{Expr, FnArg, ItemTrait, Lit, Meta, Pat, TraitItem, Type};

pub(crate) fn expand_capability(
    args: CapabilityArgs,
    mut item: ItemTrait,
) -> syn::Result<proc_macro2::TokenStream> {
    let id = args.id;
    let version = args.version;
    let client_trait = args.client_trait;
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
    let client_name = format_ident!("{}RpcClient", trait_name);
    let dispatcher_name = format_ident!("{}RpcDispatcher", trait_name);
    let guarded_name = format_ident!("{}ScopedClient", trait_name);
    let visibility = item.vis.clone();
    let mut generated_operations = Vec::new();
    let mut client_methods = Vec::new();
    let mut trait_methods = Vec::new();
    let mut scoped_methods = Vec::new();
    let mut arc_methods = Vec::new();
    let mut dispatch_arms = Vec::new();

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
        let mut client_arguments = Vec::new();
        let mut arg_names = Vec::new();
        let mut client_encoding = Vec::new();
        let mut payload_fields = Vec::new();
        let mut dispatch_arguments = Vec::new();
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
            arg_names.push(quote!(#name));
            client_arguments.push(quote!(#name: #ty));
            client_encoding.push(quote! {
                arguments.insert(
                    stringify!(#name).to_owned(),
                    ::serde_json::to_value(&#name).map_err(|error| {
                        ::riesel_protocol::CallError::Rpc(
                            ::riesel_protocol::RpcError::Codec(error.to_string())
                        )
                    })?,
                );
            });

            // Borrowed strings have an owned wire representation. Other
            // references need explicit schema/ownership semantics and are
            // intentionally rejected instead of producing unsafe RPC code.
            match &**ty {
                Type::Reference(reference) => {
                    let is_str = matches!(
                        &*reference.elem,
                        Type::Path(path) if path.qself.is_none()
                            && path.path.is_ident("str")
                    );
                    if reference.mutability.is_some() || !is_str {
                        return Err(syn::Error::new(
                            ty.span(),
                            "RPC capability arguments support only &str references; use owned types",
                        ));
                    }
                    payload_fields.push(quote!(#name: ::std::string::String));
                    dispatch_arguments.push(quote!(arguments.#name.as_str()));
                }
                _ => {
                    payload_fields.push(quote!(#name: #ty));
                    dispatch_arguments.push(quote!(arguments.#name));
                }
            }
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
        trait_methods.push(quote! {
            fn #method_name(
                &self,
                #(#client_arguments),*
            ) -> Result<#result_type, #error_type> {
                #client_name::#method_name(self, #(#arg_names),*)
                    .map_err(|error| match error {
                        ::riesel_protocol::CallError::Domain(domain) => domain,
                        ::riesel_protocol::CallError::Rpc(transport) => {
                            <#error_type as ::std::convert::From<::riesel_protocol::RpcError>>::from(transport)
                        }
                    })
            }
        });
        arc_methods.push(quote! {
            fn #method_name(
                &self,
                #(#client_arguments),*
            ) -> Result<#result_type, #error_type> {
                self.as_ref().#method_name(#(#arg_names),*)
            }
        });
        scoped_methods.push(quote! {
            fn #method_name(
                &self,
                #(#client_arguments),*
            ) -> Result<#result_type, #error_type> {
                self.session
                    .check()
                    .map_err(<#error_type as ::std::convert::From<::riesel_protocol::RpcError>>::from)?;
                self.provider.#method_name(#(#arg_names),*)
            }
        });
        client_methods.push(quote! {
            #[doc = #docs]
            pub fn #method_name(
                &self,
                #(#client_arguments),*
            ) -> Result<#result_type, ::riesel_protocol::CallError<#error_type>> {
                let mut arguments = ::serde_json::Map::new();
                #(#client_encoding)*
                let response = self.transport.call(
                    &::riesel_schema::CapabilityRef {
                        id: #id.to_owned(),
                        version: #version,
                    },
                    stringify!(#method_name),
                    ::serde_json::Value::Object(arguments),
                )?;
                ::riesel_protocol::decode_result::<#result_type, #error_type>(response)
            }
        });
        dispatch_arms.push(quote! {
            stringify!(#method_name) => {
                #[derive(::serde::Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Arguments {
                    #(#payload_fields),*
                }
                let arguments: Arguments = ::serde_json::from_value(payload).map_err(|error| {
                    ::riesel_protocol::RpcError::InvalidArguments(error.to_string())
                })?;
                let output = self.provider.#method_name(#(#dispatch_arguments),*);
                ::serde_json::to_value(output).map_err(|error| {
                    ::riesel_protocol::RpcError::Codec(error.to_string())
                })
            }
        });
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
    let trait_impl = if client_trait {
        quote! {
            /// Checks session validity even when callers clone the trait Arc
            /// out of the original application Context.
            struct #guarded_name {
                provider: ::std::sync::Arc<dyn #trait_name + Send + Sync>,
                session: ::riesel_protocol::SessionToken,
            }

            impl #trait_name for #guarded_name {
                #(#scoped_methods)*
            }

            impl<T: ::riesel_protocol::RpcTransport> #trait_name for #client_name<T> {
                #(#trait_methods)*
            }

            impl ::riesel_protocol::RpcClientBinding for #descriptor_name {
                type Service = ::std::sync::Arc<dyn #trait_name + Send + Sync>;

                fn bind(
                    transport: ::std::sync::Arc<dyn ::riesel_protocol::RpcTransport>,
                ) -> Self::Service {
                    ::std::sync::Arc::new(#client_name::new(transport))
                }

                fn guard_local(
                    service: Self::Service,
                    session: ::riesel_protocol::SessionToken,
                ) -> Self::Service {
                    ::std::sync::Arc::new(#guarded_name {
                        provider: service,
                        session,
                    })
                }
            }
        }
    } else {
        quote!()
    };

    if generated_operations.is_empty() {
        return Err(syn::Error::new(
            trait_name.span(),
            "capability must contain operations",
        ));
    }

    Ok(quote! {
        #item

        /// Automatically forward capability methods through shared Arc
        /// implementations. Component authors never duplicate these methods.
        impl<T: #trait_name + ?Sized> #trait_name for ::std::sync::Arc<T> {
            #(#arc_methods)*
        }

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

        impl ::riesel_schema::CapabilityDescriptor for #descriptor_name {
            fn definition() -> Result<
                ::riesel_schema::CapabilityContract,
                ::riesel_schema::ContractError
            > {
                #descriptor_name::definition()
            }
        }

        /// Typed transport-independent RPC client generated from the trait.
        /// Transport failures and domain errors remain explicitly distinct.
        #visibility struct #client_name<T> {
            transport: T,
        }

        impl<T: ::riesel_protocol::RpcTransport> #client_name<T> {
            pub fn new(transport: T) -> Self {
                Self { transport }
            }

            pub fn into_inner(self) -> T {
                self.transport
            }

            #(#client_methods)*
        }

        /// RPC dispatcher generated from the same trait as the client.
        #visibility struct #dispatcher_name<T> {
            provider: T,
        }

        impl<T> #dispatcher_name<T> {
            pub fn new(provider: T) -> Self {
                Self { provider }
            }

            pub fn into_inner(self) -> T {
                self.provider
            }
        }

        #trait_impl

        impl<T: #trait_name + Send + Sync> ::riesel_protocol::RpcHandler
            for #dispatcher_name<T>
        {
            fn contract(&self) -> Result<
                ::riesel_schema::CapabilityContract,
                ::riesel_protocol::RpcError
            > {
                #descriptor_name::definition().map_err(|error| {
                    ::riesel_protocol::RpcError::Contract(error.to_string())
                })
            }

            fn dispatch(
                &self,
                operation: &str,
                payload: ::serde_json::Value,
            ) -> Result<::serde_json::Value, ::riesel_protocol::RpcError> {
                match operation {
                    #(#dispatch_arms,)*
                    _ => Err(::riesel_protocol::RpcError::UnknownOperation(
                        operation.to_owned(),
                    )),
                }
            }
        }
    })
}
