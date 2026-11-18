use syn::{Attribute, Result};

/// Generated type metadata must correspond to the JSON that Serde emits.
pub(crate) fn validate_container_serde(attributes: &[Attribute]) -> Result<()> {
    for attr in attributes.iter().filter(|attr| attr.path().is_ident("serde")) {
        let mut saw_denial = false;
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("deny_unknown_fields") && !saw_denial {
                saw_denial = true;
                Ok(())
            } else {
                Err(meta.error(
                    "Serde serialization modifiers are not supported by Riesel contract metadata",
                ))
            }
        })?;
    }
    Ok(())
}

pub(crate) fn reject_serde_field(attributes: &[Attribute]) -> Result<()> {
    if let Some(attr) = attributes.iter().find(|attr| attr.path().is_ident("serde")) {
        Err(syn::Error::new_spanned(
            attr,
            "Serde field or variant modifiers are not supported by Riesel metadata",
        ))
    } else {
        Ok(())
    }
}
pub(crate) mod model;
pub(crate) mod types;
