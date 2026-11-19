//! Rust-first API declarations. Generated metadata uses riesel_schema types.
use proc_macro::TokenStream;
use syn::parse_macro_input;

mod app;
mod capability;
mod component;
mod derives;
mod identity;

/// Generate one application entrypoint from a native async Rust function.
/// The actual runtime and dependency resolver remain owned by riesel.
#[proc_macro_attribute]
pub fn app(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as app::AppArgs);
    let item = parse_macro_input!(input as syn::ItemFn);
    match app::expand_app(args, item) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

#[proc_macro_attribute]
pub fn capability(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as capability::CapabilityArgs);
    let item = parse_macro_input!(input as syn::ItemTrait);
    match capability::expand_capability(args, item) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Derive one shared component definition from its explicit capability
/// contracts. A private provider ID is derived from the Rust item path
/// unless a stable published `id` is supplied.
#[proc_macro_attribute]
pub fn component(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as component::ComponentArgs);
    let item = parse_macro_input!(input as syn::ItemStruct);
    match component::expand_component(args, item) {
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
