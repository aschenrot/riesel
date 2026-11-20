use super::*;
#[cfg(feature = "records")]
use crate::RieselModel;
#[cfg(feature = "records")]
use serde::Deserialize;

#[cfg(feature = "records")]
#[derive(Debug, Serialize, Deserialize, RieselModel)]
#[riesel(model = "todo")]
struct TodoModel {
    title: String,
    completed: bool,
}

#[cfg(feature = "dev-memory-storage")]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, RieselModel)]
#[riesel(model = "extended")]
struct ExtendedModel {
    count: u64,
    delta: i64,
    ratio: f64,
    note: Option<String>,
    tags: Vec<String>,
    payload: serde_json::Value,
}

#[cfg(feature = "dev-memory-storage")]
#[test]
fn rust_model_numeric_and_container_fields_round_trip_through_storage() {
    let model = ExtendedModel {
        count: 10,
        delta: -2,
        ratio: 0.75,
        note: None,
        tags: vec!["alpha".into(), "beta".into()],
        payload: serde_json::json!({"nested": true}),
    };
    let generated = ExtendedModel::model_definition();
    generated
        .validate_record(&serde_json::to_value(&model).unwrap())
        .unwrap();

    Application::new("ExtendedModel")
        .records::<ExtendedModel>()
        .unwrap()
        .development()
        .unwrap()
        .run(move |ctx| {
            let records = ctx.records::<ExtendedModel>()?;
            let created = records.create(model.clone())?;
            assert_eq!(created.value, model);
            let retrieved = records.get(created.id)?.unwrap();
            assert_eq!(retrieved.value, model);
            Ok(())
        })
        .unwrap();
}

#[cfg(feature = "dev-memory-storage")]
#[test]
fn records_can_bind_explicitly_to_named_storage_requirements() {
    Application::new("Aliases")
        .records_at::<TodoModel>("primary")
        .unwrap()
        .records_at::<TodoModel>("secondary")
        .unwrap()
        .development()
        .unwrap()
        .run(|ctx| {
            assert!(matches!(
                ctx.records::<TodoModel>(),
                Err(SdkError::AmbiguousStorage)
            ));
            let primary = ctx.records_at::<TodoModel>("primary")?;
            let secondary = ctx.records_at::<TodoModel>("secondary")?;
            let created = primary.create(TodoModel {
                title: "Shared".into(),
                completed: false,
            })?;
            assert_eq!(secondary.get(created.id)?.unwrap().value.title, "Shared");
            Ok(())
        })
        .unwrap();
}

#[cfg(feature = "dev-memory-storage")]
#[test]
fn rust_first_app_runs_without_json_or_tcp() {
    Application::new("Todo")
        .records::<TodoModel>()
        .unwrap()
        .development()
        .unwrap()
        .run(|ctx| {
            let records = ctx.records::<TodoModel>()?;
            let created = records.create(TodoModel {
                title: "Build Riesel".into(),
                completed: false,
            })?;
            assert_eq!(created.revision, 1);
            let got = records.get(created.id)?.unwrap();
            assert_eq!(got.value.title, "Build Riesel");
            assert_eq!(records.list()?.len(), 1);
            let storage = ctx.capability::<RecordStoreContract>("storage")?;
            assert_eq!(storage.list_page("todo", None, 32)?.records.len(), 1);
            Ok(())
        })
        .unwrap();
}

#[cfg(feature = "records")]
#[test]
fn production_does_not_silently_fall_back_to_memory() {
    let result = Application::new("Todo")
        .records::<TodoModel>()
        .unwrap()
        .run(|_| Ok(()));
    assert!(matches!(
        result,
        Err(SdkError::Execution(AppRunError::Startup(
            RuntimeError::NoLocalProvider { .. }
        )))
    ));
}
#[cfg(feature = "records")]
#[test]
fn sdk_generated_definition_round_trips_without_handwritten_json() {
    let app = Application::new("Todo").records::<TodoModel>().unwrap();
    let json = app.definition_json().unwrap();
    let decoded = riesel_schema::parse_schema(&json).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded.app.name, "Todo");
    assert_eq!(decoded.models["todo"], TodoModel::model_definition());
    assert_eq!(decoded.model_bindings["todo"], ["storage"]);
    assert_eq!(decoded.requires[0].capability.id, "riesel.storage.records");
}
#[cfg(feature = "records")]
#[test]
fn generated_rpc_capability_is_registered_once_for_multiple_aliases() {
    let app = Application::new("Example")
        .rpc::<RecordStoreContract>("primary")
        .unwrap()
        .rpc::<RecordStoreContract>("secondary")
        .unwrap();
    assert_eq!(app.definition().requires.len(), 2);
    assert!(
        app.providers
            .has_connector(&RecordStoreContract::definition().unwrap().capability)
    );
}

#[cfg(feature = "records")]
#[test]
fn arbitrary_rust_capability_is_declared_by_its_generated_type() {
    let app = Application::new("Example")
        .require::<RecordStoreContract>("records")
        .unwrap();
    assert_eq!(app.definition().requires[0].name, "records");
    assert_eq!(
        app.definition().requires[0].capability,
        RecordStoreContract::definition().unwrap().capability
    );
}
#[cfg(feature = "records")]
#[test]
fn rust_sdk_exports_validated_startup_dependencies() {
    let app = Application::new("Example")
        .rpc::<RecordStoreContract>("consumer")
        .unwrap()
        .rpc::<RecordStoreContract>("storage")
        .unwrap()
        .depends_on("consumer", ["storage"])
        .unwrap();
    let generated = app.definition_json().unwrap();
    let schema = riesel_schema::parse_schema(&generated).unwrap();
    let order: Vec<_> = schema
        .startup_order()
        .unwrap()
        .into_iter()
        .map(|req| req.name.as_str())
        .collect();
    assert_eq!(order, ["storage", "consumer"]);
    assert!(matches!(
        Application::new("Example")
            .rpc::<RecordStoreContract>("consumer")
            .unwrap()
            .depends_on("consumer", ["missing"])
            .unwrap()
            .definition_json(),
        Err(SdkError::Schema(
            riesel_schema::SchemaError::UnknownDependency { .. }
        ))
    ));
}

#[cfg(feature = "records")]
#[test]
fn rust_sdk_inspects_deployment_without_starting_configured_executable() {
    let app = Application::new("Example")
        .rpc::<RecordStoreContract>("consumer")
        .unwrap()
        .rpc::<RecordStoreContract>("storage")
        .unwrap()
        .depends_on("consumer", ["storage"])
        .unwrap();
    let config = DeploymentConfig::from_json(
        r#"{"services":{
            "consumer":{"transport":"tcp","address":"127.0.0.1:41911"},
            "storage":{"transport":"managed","program":"not-a-real-service"}
        }}"#,
    )
    .unwrap();
    let plan = app.deployment_plan(&config).unwrap();
    let ordered: Vec<_> = plan
        .services
        .iter()
        .map(|service| service.requirement.as_str())
        .collect();
    assert_eq!(ordered, ["storage", "consumer"]);
    assert!(
        serde_json::to_string(&plan)
            .unwrap()
            .contains("not-a-real-service")
    );
}

#[cfg(feature = "dev-memory-storage")]
#[test]
fn record_lookup_uses_model_binding_even_with_other_storage_requirements() {
    Application::new("Bindings")
        .records_at::<TodoModel>("primary")
        .unwrap()
        .rpc::<RecordStoreContract>("secondary")
        .unwrap()
        .development()
        .unwrap()
        .run(|ctx| {
            let primary = ctx.records::<TodoModel>()?;
            let created = primary.create(TodoModel {
                title: "Bound".into(),
                completed: false,
            })?;
            assert_eq!(
                ctx.records_at::<TodoModel>("primary")?
                    .get(created.id)?
                    .unwrap()
                    .value
                    .title,
                "Bound"
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn application_arguments_coexist_with_framework_flags() {
    use std::ffi::OsString;

    let mut invoked = false;
    Application::new("ArgDemo")
        .run_with_args(
            ["--mode", "review", "--", "--deployment", "not-a-file"],
            |_, args| {
                invoked = true;
                assert_eq!(
                    args,
                    [
                        OsString::from("--mode"),
                        OsString::from("review"),
                        OsString::from("--deployment"),
                        OsString::from("not-a-file"),
                    ]
                );
                Ok(())
            },
        )
        .unwrap();
    assert!(invoked);
}

#[cfg(feature = "records")]
#[test]
fn catalog_cli_selects_pinned_provider_and_preserves_recovery_without_starting_it() {
    use std::ffi::OsString;

    fn app() -> Application {
        let mut catalog = ProviderCatalog::new();
        catalog
            .register(
                "managed",
                riesel_schema::ProviderDefinition {
                    id: "riesel.storage.approved".into(),
                    capabilities: vec![
                        <RecordStoreContract as riesel_schema::CapabilityDescriptor>::definition()
                            .unwrap()
                            .capability,
                    ],
                },
                riesel_app::ServiceEndpoint::Managed {
                    instance: None,
                    program: "deliberately-missing-binary".into(),
                    args: Vec::new(),
                    startup_timeout_ms: 2_000,
                    shutdown_timeout_ms: 2_000,
                },
            )
            .unwrap();
        Application::new("CatalogCli")
            .rpc::<RecordStoreContract>("storage")
            .unwrap()
            .catalog(catalog)
    }

    let filename = std::env::temp_dir().join(format!(
        "riesel-catalog-cli-{}-{}.json",
        std::process::id(),
        line!()
    ));
    std::fs::write(
        &filename,
        r#"{"services":{"storage":"managed"},"recovery":{"mode":"restart_application","max_restarts":1}}"#,
    )
    .unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _cleanup = Cleanup(filename.clone());
    let path = filename.as_os_str().to_os_string();

    for args in [
        vec![
            OsString::from("--inspect-catalog"),
            OsString::from("--select-providers"),
            path.clone(),
        ],
        vec![
            OsString::from("--select-providers"),
            path.clone(),
            OsString::from("--inspect-catalog"),
        ],
    ] {
        assert!(app().configure_args(&args, false).unwrap().is_none());
    }

    let (selected, _) = app()
        .configure_args(&[OsString::from("--select-providers"), path], false)
        .unwrap()
        .unwrap();
    let deployment = selected.selected_deployment().unwrap().unwrap();
    assert!(matches!(
        deployment.recovery,
        riesel_app::RecoveryPolicy::RestartApplication {
            max_restarts: 1,
            ..
        }
    ));
    let plan = selected.catalog_plan().unwrap();
    assert_eq!(
        plan.services[0].expected_provider.as_ref().unwrap().id,
        "riesel.storage.approved"
    );
}

#[cfg(feature = "records")]
#[test]
fn deployment_inspection_is_order_independent_and_rejects_conflicting_modes() {
    use std::ffi::OsString;

    let prefix = std::env::temp_dir().join(format!(
        "riesel-inspect-modes-{}-{}",
        std::process::id(),
        line!()
    ));
    let selected_path = prefix.with_extension("select.json");
    let deploy_path = prefix.with_extension("deployment.json");
    std::fs::write(&selected_path, r#"{"services":{}}"#).unwrap();
    std::fs::write(
        &deploy_path,
        r#"{"services":{"storage":{"transport":"managed","program":"deliberately-missing-binary"}}}"#,
    )
    .unwrap();
    struct Cleanup(Vec<std::path::PathBuf>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            for path in &self.0 {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    let _cleanup = Cleanup(vec![selected_path.clone(), deploy_path.clone()]);
    let selection = selected_path.as_os_str().to_os_string();
    let deployment = deploy_path.as_os_str().to_os_string();
    let app = || {
        Application::new("Inspect")
            .rpc::<RecordStoreContract>("storage")
            .unwrap()
    };

    // Preflight inspection must not spawn the deliberately missing executable.
    assert!(
        app()
            .configure_args(
                &[OsString::from("--inspect-deployment"), deployment.clone()],
                false,
            )
            .unwrap()
            .is_none()
    );
    for args in [
        vec![
            OsString::from("--inspect-deployment"),
            deployment.clone(),
            OsString::from("--select-providers"),
            selection.clone(),
        ],
        vec![
            OsString::from("--select-providers"),
            selection,
            OsString::from("--inspect-deployment"),
            deployment,
        ],
    ] {
        assert!(matches!(
            app().configure_args(&args, false),
            Err(SdkError::ConflictingDeploymentSource)
        ));
    }
}

#[test]
fn empty_selection_manifest_is_not_silently_reapplied_or_combined_with_deployment() {
    let empty = ProviderSelectionConfig::new(BTreeMap::new());
    let application = Application::new("Duplicate")
        .provider_selections(empty.clone())
        .unwrap();
    assert!(matches!(
        application.provider_selections(empty.clone()),
        Err(SdkError::ProviderSelectionConflict(_))
    ));
    let application = Application::new("ManualAfterManifest")
        .provider_selections(empty.clone())
        .unwrap();
    assert!(matches!(
        application.select_provider("counter", "manual"),
        Err(SdkError::ProviderSelectionConflict(_))
    ));
    let application = Application::new("Conflict")
        .deployment(DeploymentConfig::new(BTreeMap::new()))
        .provider_selections(empty.clone())
        .unwrap();
    assert!(matches!(
        application.selected_deployment(),
        Err(SdkError::ConflictingDeploymentSource)
    ));
    let application = Application::new("NoCatalog")
        .provider_selections(empty)
        .unwrap();
    assert!(matches!(
        application.selected_deployment(),
        Err(SdkError::MissingProviderCatalog)
    ));
}

#[test]
fn strict_cli_rejects_unknown_arguments_before_execution() {
    use std::ffi::OsString;

    assert!(matches!(
        Application::new("Strict").configure_args(&[OsString::from("--unknown")], false),
        Err(SdkError::Arguments(_))
    ));
    assert!(matches!(
        Application::new("Strict").configure_args(&[OsString::from("--deployment")], true),
        Err(SdkError::Arguments(_))
    ));
}

#[cfg(feature = "dev-memory-storage")]
#[test]
fn development_storage_validates_models_even_for_direct_capability_calls() {
    Application::new("Checked")
        .records::<TodoModel>()
        .unwrap()
        .development()
        .unwrap()
        .run(|ctx| {
            let store = ctx.capability::<RecordStoreContract>("storage")?;
            assert!(matches!(
                store.create("todo", serde_json::json!({"title": "Missing completed"})),
                Err(crate::storage::StoreError::Failed(_))
            ));
            assert!(matches!(
                store.create(
                    "unregistered",
                    serde_json::json!({"title": "Unknown", "completed": false})
                ),
                Err(crate::storage::StoreError::Failed(_))
            ));
            Ok(())
        })
        .unwrap();
}

#[cfg(feature = "dev-memory-storage")]
#[test]
fn typed_capability_parameter_declares_and_extracts_owned_session_handle() {
    use crate::{ApplicationParam, Capability};

    type Store = Capability<RecordStoreContract>;
    let alias = Store::default_alias().unwrap();
    let app = Store::declare(Application::new("TypedStore"), &alias)
        .unwrap()
        .development()
        .unwrap();
    assert_eq!(app.definition().requires.len(), 1);
    assert_eq!(app.definition().requires[0].name, alias);

    let mut escaped = None;
    app.run(|context| {
        let store = Store::extract(context, &alias)?;
        assert!(store.list_page("task", None, 1)?.records.is_empty());
        escaped = Some(store.clone());
        Ok(())
    })
    .unwrap();
    assert!(matches!(
        escaped.unwrap().list_page("task", None, 1),
        Err(crate::storage::StoreError::Transport(_))
    ));
}

#[cfg(feature = "dev-memory-storage")]
#[test]
fn typed_records_parameter_derives_storage_requirement_and_model_binding() {
    use crate::{ApplicationParam, Records};

    type Tasks = Records<TodoModel>;
    let alias = Tasks::default_alias().unwrap();
    let app = Tasks::declare(Application::new("TypedTodo"), &alias)
        .unwrap()
        .development()
        .unwrap();
    assert_eq!(app.definition().requires.len(), 1);
    assert_eq!(app.definition().model_bindings["todo"], [alias.as_str()]);
    app.run(|context| {
        let tasks = Tasks::extract(context, &alias)?;
        let created = tasks.create(TodoModel {
            title: "Injected".into(),
            completed: false,
        })?;
        assert_eq!(tasks.get(created.id)?.unwrap().value.title, "Injected");
        Ok(())
    })
    .unwrap();
}

#[test]
fn zero_parameter_handler_derives_empty_requirements() {
    let mut invoked = false;
    let handler = Application::new("Empty")
        .handler(|| -> std::io::Result<()> {
            invoked = true;
            Ok(())
        })
        .unwrap();
    assert!(handler.definition().requires.is_empty());
    handler.run().unwrap();
    assert!(invoked);
}

#[cfg(feature = "dev-memory-storage")]
#[test]
fn handler_signature_registers_and_extracts_counterpart_capability() {
    use crate::Capability;

    let mut captured = None;
    let prepared = Application::new("Signature")
        .handler(
            |store: Capability<RecordStoreContract>| -> std::io::Result<()> {
                assert!(store.list_page("task", None, 1).unwrap().records.is_empty());
                captured = Some(store);
                Ok(())
            },
        )
        .unwrap();
    let expected = RecordStoreContract::definition()
        .unwrap()
        .capability
        .default_requirement_alias();
    assert_eq!(prepared.definition().requires.len(), 1);
    assert_eq!(prepared.definition().requires[0].name, expected);
    prepared.development().unwrap().run().unwrap();
    assert!(matches!(
        captured.unwrap().list_page("task", None, 1),
        Err(crate::storage::StoreError::Transport(_))
    ));
}

#[cfg(feature = "dev-memory-storage")]
#[test]
fn handler_multiple_models_share_one_storage_provider() {
    use crate::{Capability, Records};

    let prepared = Application::new("TwoModels")
        .handler(
            |tasks: Records<TodoModel>, notes: Records<ExtendedModel>| -> crate::Result<()> {
                let task = tasks.create(TodoModel {
                    title: "One".into(),
                    completed: false,
                })?;
                let note = notes.create(ExtendedModel {
                    count: 1,
                    delta: 0,
                    ratio: 1.0,
                    note: None,
                    tags: Vec::new(),
                    payload: serde_json::Value::Null,
                })?;
                assert_eq!(tasks.get(task.id)?.unwrap().value.title, "One");
                assert_eq!(notes.get(note.id)?.unwrap().value.count, 1);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(prepared.definition().requires.len(), 1);
    assert_eq!(prepared.definition().model_bindings["todo"], ["storage"]);
    assert_eq!(
        prepared.definition().model_bindings["extended"],
        ["storage"]
    );
    prepared.development().unwrap().run().unwrap();

    let result = Application::new("Duplicate").handler(
        |_first: Capability<RecordStoreContract>,
         _second: Capability<RecordStoreContract>|
         -> crate::Result<()> { Ok(()) },
    );
    assert!(matches!(
        result,
        Err(SdkError::DuplicateParameterBinding(_))
    ));
}

#[cfg(feature = "records")]
#[test]
fn handler_binding_validation_runs_before_provider_startup() {
    use crate::Records;
    let wrong_count = Application::new("WrongCount").handler_with_aliases(
        |_tasks: Records<TodoModel>| -> crate::Result<()> { Ok(()) },
        ["first", "second"],
    );
    assert!(matches!(
        wrong_count,
        Err(SdkError::ParameterCount {
            expected: 1,
            actual: 2
        })
    ));

    let invalid = Application::new("Invalid").handler_with_aliases(
        |_tasks: Records<TodoModel>| -> crate::Result<()> { Ok(()) },
        ["Bad Alias"],
    );
    assert!(matches!(
        invalid,
        Err(SdkError::Schema(
            riesel_schema::SchemaError::InvalidRequirementName { .. }
        ))
    ));

    let prepared = Application::new("Named")
        .handler_with_aliases(
            |_tasks: Records<TodoModel>| -> crate::Result<()> { Ok(()) },
            ["primary"],
        )
        .unwrap();
    assert_eq!(prepared.definition().model_bindings["todo"], ["primary"]);
    let expected = Application::new("Named")
        .records_at::<TodoModel>("primary")
        .unwrap();
    assert_eq!(
        prepared.definition_json().unwrap(),
        expected.definition_json().unwrap()
    );
}

#[cfg(feature = "async-tcp")]
#[test]
fn async_handler_runs_without_user_owned_executor_or_separate_registry() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let called = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&called);
    let prepared = Application::new("AsyncNoDependencies")
        .async_handler(move || {
            let counter = Arc::clone(&observed);
            async move {
                counter.fetch_add(1, Ordering::AcqRel);
                Ok::<_, std::io::Error>(())
            }
        })
        .unwrap();
    assert!(prepared.definition().requires.is_empty());
    prepared.run().unwrap();
    assert_eq!(called.load(Ordering::Acquire), 1);
}

#[cfg(all(feature = "async-tcp", feature = "records"))]
#[test]
fn async_handler_alias_validation_runs_before_any_provider_start() {
    use super::AsyncCapability;
    use riesel_storage::RecordStoreContract;

    let wrong_count = Application::new("WrongCount").async_handler_with_aliases(
        |_store: AsyncCapability<RecordStoreContract>| async { Ok::<_, std::io::Error>(()) },
        ["first", "second"],
    );
    assert!(matches!(
        wrong_count,
        Err(SdkError::ParameterCount {
            expected: 1,
            actual: 2
        })
    ));
}

#[cfg(feature = "sqlite-storage")]
#[derive(Debug, Serialize, Deserialize, RieselModel)]
#[riesel(model = "todo")]
struct IncompatibleTodoModel {
    title: String,
    completed: bool,
    priority: u64,
}

#[cfg(feature = "sqlite-storage")]
#[test]
fn explicit_sqlite_storage_persists_without_duplicate_model_metadata() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("app.db");
    let app = Application::new("LinkedTodo")
        .records::<TodoModel>()
        .unwrap()
        .sqlite_storage(path.clone())
        .unwrap();
    assert!(!path.exists(), "registration must not open the database");
    let definition = app.definition_json().unwrap();
    assert!(definition.contains("model_bindings"));
    assert!(
        !path.exists(),
        "schema inspection must remain side-effect free"
    );

    let mut id = None;
    app.run(|context| {
        let records = context.records::<TodoModel>()?;
        let created = records.create(TodoModel {
            title: "Persistent".into(),
            completed: false,
        })?;
        id = Some(created.id);
        Ok(())
    })
    .unwrap();
    assert!(path.exists());

    Application::new("LinkedTodo")
        .records::<TodoModel>()
        .unwrap()
        .sqlite_storage(path.clone())
        .unwrap()
        .run(|context| {
            let loaded = context.records::<TodoModel>()?.get(id.unwrap())?.unwrap();
            assert_eq!(loaded.value.title, "Persistent");
            assert_eq!(loaded.revision, 1);
            Ok(())
        })
        .unwrap();

    let failure = Application::new("LinkedTodo")
        .records::<IncompatibleTodoModel>()
        .unwrap()
        .sqlite_storage(path)
        .unwrap()
        .run(|_| Ok(()));
    assert!(
        failure.is_err(),
        "incompatible persisted model must reject startup"
    );
}

#[cfg(feature = "sqlite-storage")]
#[test]
fn sqlite_storage_rejects_missing_and_ambiguous_model_bindings() {
    let db = std::path::PathBuf::from("not-opened.db");
    assert!(matches!(
        Application::new("Missing").sqlite_storage(db.clone()),
        Err(SdkError::MissingStorage)
    ));
    assert!(matches!(
        Application::new("Ambiguous")
            .records_at::<TodoModel>("first")
            .unwrap()
            .records_at::<TodoModel>("second")
            .unwrap()
            .sqlite_storage(db),
        Err(SdkError::AmbiguousStorage)
    ));
}

#[cfg(feature = "sqlite-storage")]
#[test]
fn prepared_handler_can_use_explicit_sqlite_storage() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("prepared.db");
    Application::new("PreparedTodo")
        .handler(
            |records: Records<TodoModel>| -> std::result::Result<(), SdkError> {
                let created = records.create(TodoModel {
                    title: "Prepared".into(),
                    completed: false,
                })?;
                assert_eq!(created.revision, 1);
                Ok(())
            },
        )
        .unwrap()
        .sqlite_storage(path.clone())
        .unwrap()
        .run()
        .unwrap();
    assert!(path.exists());
}
#[cfg(feature = "records")]
#[derive(Debug, Clone, Serialize, Deserialize, RieselModel)]
#[riesel(model = "historical_task")]
struct HistoricalTask {
    title: String,
    completed: bool,
}

#[cfg(feature = "records")]
#[derive(Debug, Clone, Serialize, Deserialize, RieselModel)]
#[riesel(model = "historical_task")]
struct EvolvedTask {
    title: String,
    completed: bool,
    priority: i64,
}

#[cfg(feature = "records")]
impl From<HistoricalTask> for EvolvedTask {
    fn from(old: HistoricalTask) -> Self {
        Self {
            title: old.title,
            completed: old.completed,
            priority: 0,
        }
    }
}

/// Research-only typed-authoring proof for Stage 6 specification #41.
/// It does not register a migration or update a database.
#[cfg(feature = "records")]
#[test]
fn historical_rust_types_can_describe_explicit_transitions_without_duplicate_schemas() {
    assert_eq!(HistoricalTask::MODEL_NAME, EvolvedTask::MODEL_NAME);
    let source_schema = HistoricalTask::model_definition();
    let target_schema = EvolvedTask::model_definition();
    assert_ne!(source_schema, target_schema);
    assert_ne!(
        serde_json::to_vec(&source_schema).unwrap(),
        serde_json::to_vec(&target_schema).unwrap()
    );

    let source_json = serde_json::json!({"title":"legacy","completed":false});
    source_schema.validate_record(&source_json).unwrap();
    assert!(target_schema.validate_record(&source_json).is_err());

    // Historical Rust types remain separate from the current live app.
    let old: HistoricalTask = serde_json::from_value(source_json).unwrap();
    let evolved: EvolvedTask = old.into();
    let target_json = serde_json::to_value(&evolved).unwrap();
    target_schema.validate_record(&target_json).unwrap();
    assert_eq!(target_json["title"], "legacy");
    assert_eq!(target_json["priority"], 0);

    // Riesel's existing builder prevents conflicting active registrations
    // even though both Rust types share the persistent storage model name.
    let duplicate = Application::new("InvalidActiveModels")
        .records::<HistoricalTask>()
        .unwrap()
        .records::<EvolvedTask>()
        .err()
        .unwrap();
    assert!(matches!(duplicate, SdkError::DuplicateModel(_)));

    // A current application needs only the target Rust-derived model.
    let active = Application::new("Current")
        .records::<EvolvedTask>()
        .unwrap();
    assert_eq!(
        active.definition().models["historical_task"],
        EvolvedTask::model_definition()
    );
}

#[cfg(feature = "sqlite-storage")]
#[test]
fn typed_offline_model_evolution_preview_is_read_only_and_explicit() {
    use crate::storage::RecordStore;
    use riesel_storage_sqlite::{
        EvolutionLimits, ModelEvolution, SnapshotBudget, SqliteMaintenance, SqliteStore,
    };
    use std::{collections::BTreeMap, time::Duration};

    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("preview.db");
    let source = BTreeMap::from([(
        HistoricalTask::MODEL_NAME.into(),
        HistoricalTask::model_definition(),
    )]);
    let target = BTreeMap::from([(
        EvolvedTask::MODEL_NAME.into(),
        EvolvedTask::model_definition(),
    )]);
    let original = serde_json::json!({"title":"historical","completed":false});
    let store = SqliteStore::open(&path, source.clone()).unwrap();
    let record = store.create(HistoricalTask::MODEL_NAME, original.clone()).unwrap();
    drop(store);

    let plan = ModelEvolution::new(source.clone(), target.clone())
        .unwrap()
        .transform::<HistoricalTask, EvolvedTask, _>(|old| Ok(old.into()))
        .unwrap();
    let limits = EvolutionLimits {
        snapshot: SnapshotBudget {
            max_records: 5,
            max_json_bytes: 65_536,
            max_snapshot_bytes: 1_048_576,
        },
        max_elapsed: Duration::from_secs(5),
    };
    let maintenance = SqliteMaintenance::open_existing(&path, source.clone(), limits.snapshot)
        .unwrap();
    let preview = maintenance.preview_evolution(&plan, limits).unwrap();
    assert_eq!(preview.records, 1);
    assert_eq!(preview.affected_records, 1);
    assert_eq!(preview.changed_models, 1);
    assert!(preview.projected_json_bytes > 0);
    drop(maintenance);

    let source_store = SqliteStore::open(&path, source.clone()).unwrap();
    let persisted = source_store.get(HistoricalTask::MODEL_NAME, record.id).unwrap().unwrap();
    assert_eq!(persisted.data, original);
    assert_eq!(persisted.revision, 1);
    drop(source_store);
    assert!(SqliteStore::open(&path, target).is_err());
}

#[cfg(feature = "sqlite-storage")]
#[test]
fn typed_evolution_preview_rejects_missing_or_failed_transform_without_mutation() {
    use crate::storage::{RecordStore, StoreError};
    use riesel_storage_sqlite::{
        EvolutionLimits, ModelEvolution, SnapshotBudget, SqliteMaintenance, SqliteStore,
    };
    use std::{collections::BTreeMap, time::Duration};

    let temp = tempfile::tempdir().unwrap();
    let db = temp.path().join("invalid-preview.db");
    let source = BTreeMap::from([(
        HistoricalTask::MODEL_NAME.into(),
        HistoricalTask::model_definition(),
    )]);
    let target = BTreeMap::from([(
        EvolvedTask::MODEL_NAME.into(),
        EvolvedTask::model_definition(),
    )]);
    let store = SqliteStore::open(&db, source.clone()).unwrap();
    let row = store
        .create(
            HistoricalTask::MODEL_NAME,
            serde_json::json!({"title":"keep","completed":true}),
        )
        .unwrap();
    drop(store);
    let limits = EvolutionLimits {
        snapshot: SnapshotBudget {
            max_records: 10,
            max_json_bytes: 65_536,
            max_snapshot_bytes: 1_048_576,
        },
        max_elapsed: Duration::from_secs(5),
    };
    let maintenance = SqliteMaintenance::open_existing(&db, source.clone(), limits.snapshot)
        .unwrap();
    let missing = ModelEvolution::new(source.clone(), target.clone()).unwrap();
    assert!(maintenance.preview_evolution(&missing, limits).is_err());

    let failing = ModelEvolution::new(source.clone(), target)
        .unwrap()
        .transform::<HistoricalTask, EvolvedTask, _>(|_old| {
            Err(StoreError::Failed("deliberate transform failure".into()))
        })
        .unwrap();
    assert!(maintenance.preview_evolution(&failing, limits).is_err());
    drop(maintenance);

    let reopened = SqliteStore::open(&db, source).unwrap();
    let loaded = reopened
        .get(HistoricalTask::MODEL_NAME, row.id)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.id, row.id);
    assert_eq!(loaded.data, row.data);
    assert_eq!(loaded.revision, row.revision);
}
