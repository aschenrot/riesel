use super::*;
use crate::RieselModel;
use serde::Deserialize;

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
