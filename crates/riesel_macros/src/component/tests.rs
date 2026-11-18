use super::*;
use syn::{ItemStruct, parse_quote};

#[test]
fn component_requires_explicit_capability_declarations() {
    assert!(syn::parse_str::<ComponentArgs>("").is_err());
    assert!(syn::parse_str::<ComponentArgs>("provides = []").is_err());
    assert!(syn::parse_str::<ComponentArgs>("provides = [Contract, Contract]").is_ok());

    let args: ComponentArgs = syn::parse_quote! { provides = [Contract, Contract] };
    let item: ItemStruct = parse_quote! { struct Example; };
    assert!(expand_component(args, item).is_err());
}

#[test]
fn component_defaults_to_private_identity_and_default_factory() {
    let args: ComponentArgs = syn::parse_quote! { provides = [CounterContract] };
    let item: ItemStruct = parse_quote! { pub struct MemoryCounter; };
    let generated = expand_component(args, item).unwrap().to_string();
    assert!(generated.contains("private_rust_id"));
    assert!(generated.contains("exposes_contract"));
    assert!(generated.contains("Default"));
}

#[test]
fn component_supports_stable_identity_and_custom_factory() {
    let args: ComponentArgs = syn::parse_quote! {
        id = "example.counter.memory",
        provides = [CounterContract],
        factory = build_counter
    };
    let item: ItemStruct = parse_quote! { struct MemoryCounter; };
    let generated = expand_component(args, item).unwrap().to_string();
    assert!(generated.contains("example.counter.memory"));
    assert!(generated.contains("build_counter"));
    assert!(!generated.contains("private_rust_id"));
}

#[test]
fn component_rejects_invalid_identity_and_generic_state() {
    let args: ComponentArgs = syn::parse_quote! {
        id = "Invalid", provides = [CounterContract]
    };
    let item: ItemStruct = parse_quote! { struct MemoryCounter; };
    assert!(expand_component(args, item).is_err());

    let args: ComponentArgs = syn::parse_quote! { provides = [CounterContract] };
    let item: ItemStruct = parse_quote! { struct Generic<T>(T); };
    assert!(expand_component(args, item).is_err());
}
