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
