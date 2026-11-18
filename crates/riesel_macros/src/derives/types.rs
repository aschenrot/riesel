use quote::quote;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields};

pub(crate) fn expand_riesel_type(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    crate::derives::validate_container_serde(&input.attrs)?;
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
                crate::derives::reject_serde_field(&field.attrs)?;
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
                crate::derives::reject_serde_field(&variant.attrs)?;
                let variant_name = variant.ident;
                let payload = match variant.fields {
                    Fields::Unit => quote!(None),
                    Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => {
                        let field = unnamed.unnamed.first().expect("one field");
                        crate::derives::reject_serde_field(&field.attrs)?;
                        let ty = &field.ty;
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


#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn rejects_serde_contract_metadata_mismatches() {
        let renamed: DeriveInput = parse_quote! {
            #[serde(rename_all = "camelCase")]
            struct Response { status_code: String }
        };
        assert!(expand_riesel_type(renamed).is_err());

        let field_renamed: DeriveInput = parse_quote! {
            struct Response {
                #[serde(rename = "statusCode")]
                status_code: String
            }
        };
        assert!(expand_riesel_type(field_renamed).is_err());

        let variant_renamed: DeriveInput = parse_quote! {
            enum Status {
                #[serde(rename = "ready")]
                Ready
            }
        };
        assert!(expand_riesel_type(variant_renamed).is_err());

        let model_renamed: DeriveInput = parse_quote! {
            #[riesel(model = "response")]
            struct Response {
                #[serde(skip)]
                status_code: String
            }
        };
        assert!(crate::derives::model::expand_riesel_model(model_renamed).is_err());
    }

    #[test]
    fn standard_struct_and_enum_metadata_still_generate() {
        let struct_type: DeriveInput = parse_quote! {
            #[serde(deny_unknown_fields)]
            struct Response { status_code: String }
        };
        assert!(expand_riesel_type(struct_type).is_ok());
        let enum_type: DeriveInput = parse_quote! {
            enum Status { Ready, Failed(String) }
        };
        assert!(expand_riesel_type(enum_type).is_ok());
    }
}
