use super::*;
use syn::{ItemFn, parse_quote};

#[test]
fn expands_native_handle_into_existing_typed_handler_and_local_component() {
    let args: AppArgs = parse_quote! {
        name = "Counter", components = [components::MemoryCounter]
    };
    let source: ItemFn = parse_quote! {
        async fn main(counter: contracts::CounterHandle) -> AppResult<()> {
            assert_eq!(counter.value().await?, 0);
            Ok(())
        }
    };
    let generated = expand_app(args, source).unwrap().to_string();
    assert!(generated.contains("async_handler"));
    assert!(generated.contains("component"));
    assert!(generated.contains("MemoryCounter"));
    assert!(generated.contains("run_from_env"));
    assert!(generated.contains("CounterHandle"));
    assert!(generated.contains("__riesel_app_entrypoint"));
    assert!(generated.contains("NativeHandleParam"));
}

#[test]
fn explicit_aliases_use_stable_names_not_parameter_identifiers() {
    let args: AppArgs = parse_quote! { components = [Provider], development };
    let source: ItemFn = parse_quote! {
        async fn main(
            #[riesel(alias = "primary")]
            left: contracts::CounterHandle,
            #[riesel(alias = "secondary")]
            right: contracts::CounterHandle
        ) -> AppResult<()> {
            Ok(())
        }
    };
    let generated = expand_app(args, source).unwrap().to_string();
    assert!(generated.contains("async_handler_with_aliases"));
    assert!(generated.contains("primary"));
    assert!(generated.contains("secondary"));
    assert!(!generated.contains("riesel (alias"));
    assert!(generated.contains("development"));
}

#[test]
fn rejects_invalid_signatures_and_component_declarations() {
    let args: AppArgs = parse_quote! {};
    let sync: ItemFn = parse_quote! { fn main() -> Result<(), ()> { Ok(()) } };
    assert!(
        expand_app(args, sync)
            .unwrap_err()
            .to_string()
            .contains("requires an async fn entrypoint")
    );

    let args: AppArgs = parse_quote! {};
    let no_result: ItemFn = parse_quote! { async fn main() {} };
    assert!(
        expand_app(args, no_result)
            .unwrap_err()
            .to_string()
            .contains("must return Result<(), E>")
    );

    let args: AppArgs = parse_quote! {};
    let generic: ItemFn = parse_quote! {
        async fn main<T>() -> Result<(), ()> { Ok(()) }
    };
    assert!(
        expand_app(args, generic)
            .unwrap_err()
            .to_string()
            .contains("concrete safe async fn")
    );

    let duplicate: syn::Result<AppArgs> = syn::parse_str("components = [A, A]");
    assert!(matches!(
        duplicate,
        Err(error) if error.to_string().contains("component was registered more than once")
    ));

    let duplicated_key: syn::Result<AppArgs> = syn::parse_str("components = [A], components = [B]");
    assert!(matches!(
        duplicated_key,
        Err(error) if error.to_string().contains("duplicate components list")
    ));

    let unknown: syn::Result<AppArgs> = syn::parse_str("providers = [A]");
    assert!(matches!(
        unknown,
        Err(error) if error.to_string().contains("expected components =")
    ));
}

#[test]
fn rejects_invalid_binding_aliases_and_more_than_four_parameters() {
    let args: AppArgs = parse_quote! {};
    let duplicate_alias: ItemFn = parse_quote! {
        async fn main(#[riesel(alias = "a", alias = "b")] a: A) -> AppResult<()> {
            Ok(())
        }
    };
    assert!(
        expand_app(args, duplicate_alias)
            .unwrap_err()
            .to_string()
            .contains("duplicate riesel alias")
    );

    let args: AppArgs = parse_quote! {};
    let wrong_shape: ItemFn = parse_quote! {
        async fn main((a, b): (A, B)) -> AppResult<()> { Ok(()) }
    };
    assert!(
        expand_app(args, wrong_shape)
            .unwrap_err()
            .to_string()
            .contains("requires a named parameter")
    );

    let args: AppArgs = parse_quote! {};
    let excess: ItemFn = parse_quote! {
        async fn main(a: A, b: B, c: C, d: D, e: E) -> AppResult<()> { Ok(()) }
    };
    assert!(
        expand_app(args, excess)
            .unwrap_err()
            .to_string()
            .contains("at most four typed parameters")
    );
}

#[test]
fn registered_parameter_type_alias_can_explicitly_skip_handle_adaptation() {
    let args: AppArgs = parse_quote! {};
    let source: ItemFn = parse_quote! {
        async fn main(#[riesel(parameter)] records: MyRecordsAlias) -> AppResult<()> {
            Ok(())
        }
    };
    let expanded = expand_app(args, source).unwrap().to_string();
    assert!(expanded.contains("__riesel_parameter_0 : MyRecordsAlias"));
    assert!(!expanded.contains("NativeHandleParam"));
}

#[test]
fn app_options_reject_duplicate_development_and_empty_names() {
    assert!(syn::parse_str::<AppArgs>("development, development").is_err());
    assert!(syn::parse_str::<AppArgs>("name = \"\"").is_err());
    assert!(syn::parse_str::<AppArgs>("name = \"One\", name = \"Two\"").is_err());
}

#[test]
fn catalog_factory_is_wired_through_existing_prepared_application() {
    let args: AppArgs = parse_quote! {
        components = [CounterState], catalog = approved_counter_providers
    };
    let source: ItemFn = parse_quote! {
        async fn main(counter: CounterHandle) -> AppResult<()> {
            Ok(())
        }
    };
    let generated = expand_app(args, source).unwrap().to_string();
    assert!(generated.contains("approved_counter_providers"));
    assert!(generated.contains(". catalog"));
    assert!(generated.contains("run_from_env"));
    assert!(syn::parse_str::<AppArgs>("catalog = first, catalog = second").is_err());
}

#[test]
fn explicit_modules_expand_before_individual_component_registration() {
    let args: AppArgs = parse_quote! {
        modules = [RequirementsModule, ProviderModule],
        components = [LocalProvider]
    };
    let source: ItemFn = parse_quote! {
        async fn main(counter: CounterHandle) -> AppResult<()> {
            Ok(())
        }
    };
    let expanded = expand_app(args, source).unwrap().to_string();
    assert!(expanded.find(". module").unwrap() < expanded.find(". component").unwrap());
    assert!(expanded.contains("RequirementsModule"));
    assert!(expanded.contains("ProviderModule"));
    assert!(matches!(
        syn::parse_str::<AppArgs>("modules = [A, A]"),
        Err(error) if error.to_string().contains("application module was registered more than once")
    ));
    assert!(matches!(
        syn::parse_str::<AppArgs>("modules = [A], modules = [B]"),
        Err(error) if error.to_string().contains("duplicate modules list")
    ));
}
