use super::*;
use syn::{ItemTrait, parse_quote};

#[test]
fn invalid_capability_signatures_are_rejected_early() {
    let args: CapabilityArgs = syn::parse_quote! {
        id = "example.test", version = 1
    };
    let no_marker: ItemTrait = parse_quote! {
        trait Sample {
            fn read(&self) -> Result<u64, String>;
        }
    };
    assert!(expand_capability(args, no_marker).is_err());
    let args: CapabilityArgs = syn::parse_quote! {
        id = "example.test", version = 1
    };
    let async_method: ItemTrait = parse_quote! {
        trait Sample {
            /// Fetch a value.
            #[query]
            async fn read(&self) -> Result<u64, String>;
        }
    };
    assert!(expand_capability(args, async_method).is_err());
}

#[test]
fn private_capabilities_require_no_manual_id_or_version() {
    let args: CapabilityArgs = syn::parse_quote! {};
    let source: ItemTrait = parse_quote! {
        trait Sample {
            /// Return the sample value.
            #[query]
            fn read(&self) -> Result<u64, String>;
        }
    };
    let output = expand_capability(args, source).unwrap().to_string();
    assert!(output.contains("private_rust_id"));
    assert!(output.contains("module_path"));
}

#[test]
fn published_contracts_keep_explicit_stable_identity() {
    let args: CapabilityArgs = syn::parse_quote! {
        id = "example.contract", version = 2
    };
    let source: ItemTrait = parse_quote! {
        trait Sample {
            /// Return the sample value.
            #[query]
            fn read(&self) -> Result<u64, String>;
        }
    };
    let output = expand_capability(args, source).unwrap().to_string();
    assert!(!output.contains("private_rust_id"));
    assert!(output.contains("example.contract"));
}
