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
    let emitted = expand_capability(args, async_method).unwrap().to_string();
    assert!(emitted.contains("SampleAsyncRpcDispatcher"));
    assert!(emitted.contains("SampleAsyncService"));
    assert!(emitted.contains("Future"));
    assert!(emitted.contains("Send"));
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

#[test]
fn capability_emits_async_client_using_async_transport() {
    let args: CapabilityArgs = syn::parse_quote! {};
    let source: ItemTrait = parse_quote! {
        trait Probe {
            /// Get a value.
            #[query]
            fn value(&self) -> Result<u64, String>;
        }
    };
    let tokens = expand_capability(args, source).unwrap().to_string();
    assert!(tokens.contains("ProbeAsyncClient"));
    assert!(tokens.contains("AsyncRpcTransport"));
    assert!(tokens.contains(". await"));
    assert!(tokens.contains("CallError"));
}

#[test]
fn mixed_sync_and_async_capability_methods_are_rejected() {
    let args: CapabilityArgs = syn::parse_quote! {};
    let method: ItemTrait = parse_quote! {
        trait Mixed {
            /// Return an asynchronous value.
            #[query]
            async fn read(&self) -> Result<u64, String>;
            /// Increment synchronously.
            #[command]
            fn write(&self, amount: u64) -> Result<u64, String>;
        }
    };
    let err = expand_capability(args, method).unwrap_err();
    assert!(
        err.to_string()
            .contains("mixing synchronous and asynchronous")
    );
}
