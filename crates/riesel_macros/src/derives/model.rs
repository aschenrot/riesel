use quote::quote;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields, LitStr};

pub(crate) fn expand_riesel_model(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = input.ident;
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        return Err(syn::Error::new(
            input.generics.span(),
            "generic Rust models are not supported yet",
        ));
    }
    crate::derives::validate_container_serde(&input.attrs)?;
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
        crate::derives::reject_serde_field(&field.attrs)?;
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
