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
    let version: syn::LitInt = args.version.unwrap_or_else(|| syn::parse_quote!(1));
    let client_trait = args.client_trait;
    let parsed_version: u32 = version.base10_parse()?;
    if parsed_version == 0 {
        return Err(syn::Error::new(
            version.span(),
            "version must be greater than zero",
        ));
    }
    if let Some(id) = &id {
        crate::identity::validate_explicit_id(id)?;
    }
    if !item.generics.params.is_empty() || item.generics.where_clause.is_some() {
        return Err(syn::Error::new(
            item.generics.span(),
            "generic capability traits are not supported",
        ));
    }
    let trait_name = item.ident.clone();
    // Private IDs derive from the declaration site, not from the call site.
    // Published contracts must retain an explicit stable ID across renames.
    let id_expr = crate::identity::declaration_id(id.as_ref(), &trait_name);
    let descriptor_name = format_ident!("{}Contract", trait_name);
    let client_name = format_ident!("{}RpcClient", trait_name);
    let async_client_name = format_ident!("{}AsyncClient", trait_name);
    let dispatcher_name = format_ident!("{}RpcDispatcher", trait_name);
    let async_dispatcher_name = format_ident!("{}AsyncRpcDispatcher", trait_name);
    let async_service_name = format_ident!("{}AsyncService", trait_name);
    let local_async_name = format_ident!("{}LocalClient", trait_name);
    let guarded_name = format_ident!("{}ScopedClient", trait_name);
    let visibility = item.vis.clone();
    let mut generated_operations = Vec::new();
    let mut client_methods = Vec::new();
    let mut async_client_methods = Vec::new();
    let mut trait_methods = Vec::new();
    let mut scoped_methods = Vec::new();
    let mut arc_methods = Vec::new();
    let mut dispatch_arms = Vec::new();
    let mut async_dispatch_arms = Vec::new();
    let mut async_service_methods = Vec::new();
    let mut async_service_impls = Vec::new();
    let mut local_async_methods = Vec::new();
    let mut is_async: Option<bool> = None;

    for entry in &mut item.items {
        let TraitItem::Fn(method) = entry else {
            return Err(syn::Error::new(
                entry.span(),
                "only methods are supported in capability traits",
            ));
        };
        let method_async = method.sig.asyncness.is_some();
        if let Some(existing) = is_async {
            if existing != method_async {
                return Err(syn::Error::new(
                    method.sig.span(),
                    "mixing synchronous and asynchronous capability operations is not supported",
                ));
            }
        } else {
            is_async = Some(method_async);
        }
        if !method.sig.generics.params.is_empty()
            || method.sig.generics.where_clause.is_some()
            || method.sig.unsafety.is_some()
            || method.sig.abi.is_some()
            || method.default.is_some()
        {
            return Err(syn::Error::new(
                method.sig.span(),
                "capability methods must be non-generic, safe declarations without default bodies",
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
        let mut service_arguments = Vec::new();
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
            if matches!(&**ty, Type::Reference(_)) {
                service_arguments.push(quote!(#name: &'a str));
            } else {
                service_arguments.push(quote!(#name: #ty));
            }
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

        // Own these AST nodes because native async methods rewrite
        // their signature below; references would borrow method.sig.output.
        let (result_type, error_type) = result_parts(&method.sig.output)?;
        let (result_type, error_type) = (result_type.clone(), error_type.clone());
        let method_name = &method.sig.ident;
        if method_async {
            // Keep the generated adapter's methods disjoint from the user
            // trait; implementations may freely call self.value() etc.
            let service_method_name = format_ident!("__riesel_{}", method_name);
            // The user can still implement this trait using ordinary
            // async fn syntax. The Send bound makes object-safe adapters
            // and Tokio-managed dispatch possible without an async-trait
            // dependency or a blocking Future wrapper.
            method.sig.asyncness = None;
            method.sig.output = syn::parse_quote!(
                -> impl ::std::future::Future<Output = Result<#result_type, #error_type>> + Send
            );

            async_service_methods.push(quote! {
                fn #service_method_name<'a>(
                    &'a self,
                    #(#service_arguments),*
                ) -> ::std::pin::Pin<::std::boxed::Box<
                    dyn ::std::future::Future<Output = Result<#result_type, #error_type>>
                        + Send + 'a
                >>;
            });
            async_service_impls.push(quote! {
                fn #service_method_name<'a>(
                    &'a self,
                    #(#service_arguments),*
                ) -> ::std::pin::Pin<::std::boxed::Box<
                    dyn ::std::future::Future<Output = Result<#result_type, #error_type>>
                        + Send + 'a
                >> {
                    ::std::boxed::Box::pin(#trait_name::#method_name(
                        self, #(#arg_names),*
                    ))
                }
            });
            local_async_methods.push(quote! {
                #[doc = #docs]
                pub async fn #method_name(
                    &self,
                    #(#client_arguments),*
                ) -> Result<#result_type, ::riesel_protocol::CallError<#error_type>> {
                    self.session.check()?;
                    let result = #async_service_name::#service_method_name(
                        self.provider.as_ref(), #(#arg_names),*
                    ).await;
                    self.session.check()?;
                    result.map_err(::riesel_protocol::CallError::Domain)
                }
            });
            async_dispatch_arms.push(quote! {
                stringify!(#method_name) => {
                    #[derive(::serde::Deserialize)]
                    #[serde(deny_unknown_fields)]
                    struct Arguments {
                        #(#payload_fields),*
                    }
                    let arguments: Arguments =
                        ::serde_json::from_value(payload).map_err(|error| {
                            ::riesel_protocol::RpcError::InvalidArguments(error.to_string())
                        })?;
                    let output = #trait_name::#method_name(
                        &self.provider, #(#dispatch_arguments),*
                    ).await;
                    ::serde_json::to_value(output).map_err(|error| {
                        ::riesel_protocol::RpcError::Codec(error.to_string())
                    })
                }
            });
        }
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
                        id: #id_expr,
                        version: #version,
                    },
                    stringify!(#method_name),
                    ::serde_json::Value::Object(arguments),
                )?;
                ::riesel_protocol::decode_result::<#result_type, #error_type>(response)
            }
        });
        async_client_methods.push(quote! {
            #[doc = #docs]
            pub async fn #method_name(
                &self,
                #(#client_arguments),*
            ) -> Result<#result_type, ::riesel_protocol::CallError<#error_type>> {
                let mut arguments = ::serde_json::Map::new();
                #(#client_encoding)*
                let response = self.transport.call(
                    &::riesel_schema::CapabilityRef {
                        id: #id_expr,
                        version: #version,
                    },
                    stringify!(#method_name),
                    ::serde_json::Value::Object(arguments),
                ).await?;
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
    let trait_impl = if client_trait && !is_async.unwrap_or(false) {
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

            impl<T> ::riesel_protocol::ComponentAdapter<T> for #descriptor_name
            where
                T: #trait_name + Send + Sync + 'static,
            {
                type Handler = #dispatcher_name<::std::sync::Arc<T>>;

                fn local(state: ::std::sync::Arc<T>) -> <Self as ::riesel_protocol::RpcClientBinding>::Service {
                    state
                }

                fn hosted(state: ::std::sync::Arc<T>) -> Self::Handler {
                    #dispatcher_name::new(state)
                }
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

    let async_provider_impl = if is_async.unwrap_or(false) {
        quote! {
            /// Object-safe adapter generated from the native async trait.
            /// Component implementations only need ordinary Rust async fn.
            #visibility trait #async_service_name: Send + Sync {
                #(#async_service_methods)*
            }

            impl<T: #trait_name + Send + Sync> #async_service_name for T {
                #(#async_service_impls)*
            }

            /// In-process, typed, revocable async handle.
            #visibility struct #local_async_name {
                provider: ::std::sync::Arc<dyn #async_service_name>,
                session: ::riesel_protocol::SessionToken,
            }

            impl #local_async_name {
                pub fn new(
                    provider: ::std::sync::Arc<dyn #async_service_name>,
                    session: ::riesel_protocol::SessionToken,
                ) -> Self {
                    Self { provider, session }
                }

                #(#local_async_methods)*
            }

            /// Async dispatcher generated from the same versioned contract.
            #visibility struct #async_dispatcher_name<T> {
                provider: T,
            }

            impl<T> #async_dispatcher_name<T> {
                pub fn new(provider: T) -> Self {
                    Self { provider }
                }
            }

            impl<T: #trait_name + Send + Sync> ::riesel_protocol::AsyncRpcHandler
                for #async_dispatcher_name<T>
            {
                fn contract(
                    &self,
                ) -> Result<::riesel_schema::CapabilityContract, ::riesel_protocol::RpcError> {
                    #descriptor_name::definition().map_err(|error| {
                        ::riesel_protocol::RpcError::Contract(error.to_string())
                    })
                }

                fn dispatch<'a>(
                    &'a self,
                    operation: &'a str,
                    payload: ::serde_json::Value,
                ) -> ::riesel_protocol::AsyncRpcFuture<'a> {
                    ::std::boxed::Box::pin(async move {
                        match operation {
                            #(#async_dispatch_arms,)*
                            _ => Err(::riesel_protocol::RpcError::UnknownOperation(
                                operation.to_owned(),
                            )),
                        }
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

    let sync_provider_impl = if !is_async.unwrap_or(false) {
        quote! {
            /// Automatically forward capability methods through shared Arc
            /// implementations.
            impl<T: #trait_name + ?Sized> #trait_name for ::std::sync::Arc<T> {
                #(#arc_methods)*
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
        }
    } else {
        quote!()
    };

    Ok(quote! {
        #item
        #sync_provider_impl
        #async_provider_impl

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
                        id: #id_expr,
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

        /// Async client generated from the same capability contract as the
        /// synchronous API. Uses a real AsyncRpcTransport, never blocking
        /// RpcTransport calls inside a Future.
        #[derive(Clone)]
        #visibility struct #async_client_name<T> {
            transport: T,
        }

        impl<T: ::riesel_protocol::AsyncRpcTransport> #async_client_name<T> {
            pub fn new(transport: T) -> Self {
                Self { transport }
            }

            pub fn into_inner(self) -> T {
                self.transport
            }

            #(#async_client_methods)*
        }

        #trait_impl

        impl ::riesel_protocol::AsyncRpcClientBinding for #descriptor_name {
            type Service = #async_client_name<
                ::std::sync::Arc<dyn ::riesel_protocol::AsyncRpcTransport>
            >;

            fn bind_async(
                transport: ::std::sync::Arc<dyn ::riesel_protocol::AsyncRpcTransport>,
            ) -> Self::Service {
                #async_client_name::new(transport)
            }
        }

    })
}
