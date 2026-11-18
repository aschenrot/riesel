//! Rust-first API declarations. Generated metadata uses riesel_schema types.
use proc_macro::TokenStream;
use syn::parse_macro_input;

mod capability;
mod derives;

#[proc_macro_attribute]
pub fn capability(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as capability::CapabilityArgs);
    let item = parse_macro_input!(input as syn::ItemTrait);
    match capability::expand_capability(args, item) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Generates portable capability API type definitions.
#[proc_macro_derive(RieselType)]
pub fn derive_riesel_type(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::DeriveInput);
    match derives::types::expand_riesel_type(input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Generates model metadata from an annotated Rust struct.
#[proc_macro_derive(RieselModel, attributes(riesel))]
pub fn derive_riesel_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::DeriveInput);
    match derives::model::expand_riesel_model(input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}
