use super::managed::{ShutdownMode, await_ready};
use super::*;
use riesel_schema::{CapabilityRef, OperationDefinition, OperationKind, TypeRef};
use std::net::SocketAddr;
use std::sync::Arc;

fn contract(version: u32) -> CapabilityContract {
    CapabilityContract {
        capability: CapabilityRef {
            id: "example.counter".into(),
            version,
        },
        operations: BTreeMap::from([(
            "read".into(),
            OperationDefinition {
                kind: OperationKind::Query,
                description: "Read a value".into(),
                inputs: vec![],
                output: TypeRef::U64,
                error: TypeRef::Unit,
            },
        )]),
        types: BTreeMap::new(),
    }
}

struct Dummy {
    local_version: u32,
    remote_version: u32,
}

impl CapabilityConnector for Dummy {
    fn contract(&self) -> Result<CapabilityContract, ConnectorError> {
        Ok(contract(self.local_version))
    }
    fn connect(
        &self,
        _endpoint: &ServiceEndpoint,
        _session: &SessionToken,
    ) -> Result<ConnectedProvider, ConnectorError> {
        Ok(ConnectedProvider {
            definition: ProviderDefinition {
                id: "example.counter.local".into(),
                capabilities: vec![contract(self.remote_version).capability.clone()],
            },
            contract: contract(self.remote_version),
            service: Box::new(Arc::new(42_u64)),
        })
    }
}

fn app() -> App {
    App::from_schema(
        r#"{"version":1,"app":{"name":"Example"},"models":{},
            "requires":[{"name":"counter","capability":{
                "id":"example.counter","version":1}}]}"#,
    )
    .unwrap()
}

fn deployment() -> DeploymentConfig {
    DeploymentConfig::new(BTreeMap::from([(
        "counter".into(),
        ServiceEndpoint::Tcp {
            address: "127.0.0.1:47911".parse::<SocketAddr>().unwrap(),
        },
    )]))
}

#[test]
fn app_prepares_and_exposes_typed_provider() {
    let mut connectors = ConnectorRegistry::new();
    connectors
        .register(Dummy {
            local_version: 1,
            remote_version: 1,
        })
        .unwrap();
    let running = app().start(&deployment(), &connectors).unwrap();
    assert_eq!(running.resolved()[0].provider, "example.counter.local");
    assert_eq!(**running.service::<Arc<u64>>("counter").unwrap(), 42);
    assert!(matches!(
        running.service::<Arc<String>>("counter"),
        Err(RuntimeError::WrongServiceType(_))
    ));
}

#[test]
fn app_rejects_missing_and_mismatched_connectors() {
    let empty = ConnectorRegistry::new();
    assert!(matches!(
        app().start(&deployment(), &empty),
        Err(RuntimeError::MissingConnector { .. })
    ));

    let mut mismatched = ConnectorRegistry::new();
    mismatched
        .register(Dummy {
            local_version: 2,
            remote_version: 2,
        })
        .unwrap();
    assert!(matches!(
        app().start(&deployment(), &mismatched),
        Err(RuntimeError::MissingConnector { .. })
    ));

    let mut remote = ConnectorRegistry::new();
    remote
        .register(Dummy {
            local_version: 1,
            remote_version: 2,
        })
        .unwrap();
    assert!(matches!(
        app().start(&deployment(), &remote),
        Err(RuntimeError::RemoteContractMismatch { .. })
    ));
}

#[test]
fn registry_supports_multiple_versions_of_the_same_capability() {
    let mut connectors = ConnectorRegistry::new();
    for version in [1, 2] {
        connectors
            .register(Dummy {
                local_version: version,
                remote_version: version,
            })
            .unwrap();
    }
    assert!(connectors.has_connector(&contract(1).capability));
    assert!(connectors.has_connector(&contract(2).capability));
    assert!(!connectors.has_connector(&contract(3).capability));

    let app = App::from_schema(
        r#"{"version":1,"app":{"name":"TwoVersions"},"models":{},
            "requires":[
                {"name":"counter_v1","capability":{"id":"example.counter","version":1}},
                {"name":"counter_v2","capability":{"id":"example.counter","version":2}}
            ]}"#,
    )
    .unwrap();
    let deployment = DeploymentConfig::new(BTreeMap::from([
        (
            "counter_v1".into(),
            ServiceEndpoint::Tcp {
                address: "127.0.0.1:47911".parse().unwrap(),
            },
        ),
        (
            "counter_v2".into(),
            ServiceEndpoint::Tcp {
                address: "127.0.0.1:47912".parse().unwrap(),
            },
        ),
    ]));
    assert_eq!(
        app.preflight(&deployment, &connectors)
            .unwrap()
            .services
            .len(),
        2
    );
}

#[test]
fn registry_rejects_duplicate_connectors() {
    let mut connectors = ConnectorRegistry::new();
    connectors
        .register(Dummy {
            local_version: 1,
            remote_version: 1,
        })
        .unwrap();
    assert!(matches!(
        connectors.register(Dummy {
            local_version: 1,
            remote_version: 1,
        }),
        Err(RuntimeError::DuplicateConnector { .. })
    ));
}
struct Unavailable;

impl CapabilityConnector for Unavailable {
    fn contract(&self) -> Result<CapabilityContract, ConnectorError> {
        Ok(contract(1))
    }

    fn connect(
        &self,
        _endpoint: &ServiceEndpoint,
        _session: &SessionToken,
    ) -> Result<ConnectedProvider, ConnectorError> {
        Err(io::Error::new(io::ErrorKind::ConnectionRefused, "service not ready").into())
    }
}

#[test]
fn runtime_propagates_entrypoint_errors() {
    let mut connectors = ConnectorRegistry::new();
    connectors
        .register(Dummy {
            local_version: 1,
            remote_version: 1,
        })
        .unwrap();

    let result = app().run(&deployment(), &connectors, |_running| {
        Err(io::Error::other("application failed").into())
    });
    assert!(matches!(result, Err(AppRunError::Entrypoint(_))));
}

#[test]
fn child_exits_immediately() {
    // Invoked as a subprocess to test early managed-service termination.
}

#[test]
fn runtime_rejects_managed_service_without_ready_announcement() {
    let mut config = deployment();
    config.services.insert(
        "counter".into(),
        ServiceEndpoint::Managed {
            instance: None,
            program: std::env::current_exe().unwrap(),
            args: vec![
                "--exact".into(),
                "runtime::tests::child_exits_immediately".into(),
            ],
            startup_timeout_ms: 5_000,
            shutdown_timeout_ms: 2_000,
        },
    );
    let mut connectors = ConnectorRegistry::new();
    connectors.register(Unavailable).unwrap();

    assert!(matches!(
        app().start(&config, &connectors),
        Err(RuntimeError::ServiceStart { .. })
    ));
}

/// Construct an app backed by a real OS child that exits immediately.
/// No fake process status or forced error injection is involved.
fn with_exiting_managed_child() -> RunningApp {
    let key = ProcessKey::new(1, Some("counter".into()));
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "runtime::tests::child_exits_immediately"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut registry = ProcessRegistry::new();
    registry.spawn(key.clone(), &mut command).unwrap();
    let processes = Arc::new(Mutex::new(registry));
    let supervisor = ServiceMonitor::start(
        Arc::clone(&processes),
        BTreeMap::from([("counter".into(), key)]),
        SessionToken::new(),
    )
    .unwrap();
    let tasks = TaskGroup::new(supervisor.cancellation_token());
    RunningApp {
        app: app(),
        resolved: vec![],
        services: BTreeMap::new(),
        processes,
        supervisor,
        tasks,
        #[cfg(feature = "async-tcp")]
        async_tasks: AsyncTasks::new(SessionToken::new(), CancellationToken::default()),
        #[cfg(feature = "async-tcp")]
        local_blocking: Vec::new(),
        session: SessionToken::new(),
    }
}

#[test]
fn managed_child_exit_cancels_application_and_returns_supervision_error() {
    let running = with_exiting_managed_child();
    let token = running.cancellation_token();
    let result = RunningApp::execute(running, |app| {
        assert!(token.wait_cancelled_timeout(Duration::from_secs(8)));
        assert!(matches!(
            app.check_processes(),
            Err(RuntimeError::ServiceExited { requirement, .. })
                if requirement == "counter"
        ));
        Ok(())
    });
    match result {
        Err(AppRunError::Supervision {
            failure,
            entry: None,
            shutdown: None,
        }) => assert!(matches!(
            *failure,
            RuntimeError::ServiceExited { requirement, .. }
                if requirement == "counter"
        )),
        other => panic!("expected managed service failure, received {other:?}"),
    }
}

#[test]
fn supervision_preserves_concurrent_business_error() {
    let running = with_exiting_managed_child();
    let token = running.cancellation_token();
    let result = RunningApp::execute(running, |_| {
        assert!(token.wait_cancelled_timeout(Duration::from_secs(8)));
        Err(io::Error::other("entrypoint failed after cancellation").into())
    });
    assert!(matches!(
        result,
        Err(AppRunError::Supervision {
            entry: Some(_),
            shutdown: None,
            ..
        })
    ));
}

#[test]
fn delayed_exit_child() {
    if std::env::var_os("RIESEL_RUNTIME_TEST_DELAYED_EXIT").is_some() {
        std::thread::sleep(Duration::from_millis(400));
    }
}

#[test]
fn managed_failure_during_task_join_is_not_missed() {
    let key = ProcessKey::new(55, Some("counter".into()));
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "runtime::tests::delayed_exit_child"])
        .env("RIESEL_RUNTIME_TEST_DELAYED_EXIT", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut registry = ProcessRegistry::new();
    registry.spawn(key.clone(), &mut command).unwrap();
    let processes = Arc::new(Mutex::new(registry));
    let supervisor = ServiceMonitor::start(
        Arc::clone(&processes),
        BTreeMap::from([("counter".into(), key)]),
        SessionToken::new(),
    )
    .unwrap();
    let tasks = TaskGroup::new(supervisor.cancellation_token());
    let running = RunningApp {
        app: app(),
        resolved: vec![],
        services: BTreeMap::new(),
        processes,
        supervisor,
        tasks,
        #[cfg(feature = "async-tcp")]
        async_tasks: AsyncTasks::new(SessionToken::new(), CancellationToken::default()),
        #[cfg(feature = "async-tcp")]
        local_blocking: Vec::new(),
        session: SessionToken::new(),
    };
    let result = RunningApp::execute(running, |running| {
        running.spawn_task("finishing", |cancel| {
            assert!(cancel.wait_cancelled_timeout(Duration::from_secs(5)));
            // The child exits while this task is still joining. The
            // monitor must not stop until joining finishes.
            std::thread::sleep(Duration::from_millis(1_200));
            Ok(())
        })?;
        Ok(())
    });
    assert!(matches!(
        result,
        Err(AppRunError::Supervision {
            failure,
            entry: None,
            shutdown: None,
        }) if matches!(*failure, RuntimeError::ServiceExited { .. })
    ));
}

#[test]
fn whole_application_restart_rebuilds_services_after_real_child_exit() {
    use crate::RecoveryPolicy;
    let mut connectors = ConnectorRegistry::new();
    connectors
        .register_local(LocalCounter("example.counter.memory"))
        .unwrap();
    let policy = RecoveryPolicy::RestartApplication {
        max_restarts: 2,
        initial_backoff_ms: 1,
        max_backoff_ms: 2,
    };
    let mut starts = 0;
    let mut entries = 0;
    crate::recovery::run_with_recovery(
        &policy,
        || {
            starts += 1;
            if starts == 1 {
                Ok(with_exiting_managed_child())
            } else {
                app().start_local(&connectors)
            }
        },
        |running| {
            entries += 1;
            if entries == 1 {
                assert!(
                    running
                        .cancellation_token()
                        .wait_cancelled_timeout(Duration::from_secs(8))
                );
            } else {
                assert_eq!(**running.service::<Arc<u64>>("counter").unwrap(), 123);
                assert!(!running.cancellation_token().is_cancelled());
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(starts, 2);
    assert_eq!(entries, 2);
}

#[test]
fn whole_application_restart_is_bounded_after_repeated_real_process_failures() {
    use crate::RecoveryPolicy;
    let policy = RecoveryPolicy::RestartApplication {
        max_restarts: 2,
        initial_backoff_ms: 1,
        max_backoff_ms: 2,
    };
    let mut starts = 0;
    let result = crate::recovery::run_with_recovery(
        &policy,
        || {
            starts += 1;
            Ok(with_exiting_managed_child())
        },
        |running| {
            assert!(
                running
                    .cancellation_token()
                    .wait_cancelled_timeout(Duration::from_secs(8))
            );
            Ok(())
        },
    );
    assert_eq!(starts, 3);
    assert!(matches!(
        result,
        Err(AppRunError::RecoveryExhausted {
            restarts: 2,
            last,
        }) if matches!(*last, AppRunError::Supervision { .. })
    ));
}

#[test]
fn session_restart_does_not_replay_concurrent_business_errors() {
    use crate::RecoveryPolicy;
    let policy = RecoveryPolicy::RestartApplication {
        max_restarts: 3,
        initial_backoff_ms: 1,
        max_backoff_ms: 2,
    };
    let mut starts = 0;
    let result = crate::recovery::run_with_recovery(
        &policy,
        || {
            starts += 1;
            Ok(with_exiting_managed_child())
        },
        |running| {
            assert!(
                running
                    .cancellation_token()
                    .wait_cancelled_timeout(Duration::from_secs(8))
            );
            Err(io::Error::other("business work cannot be replayed").into())
        },
    );
    assert_eq!(starts, 1);
    assert!(matches!(
        result,
        Err(AppRunError::Supervision {
            entry: Some(_),
            shutdown: None,
            ..
        })
    ));
}

#[test]
fn structured_task_failure_cancels_application_and_preserves_error() {
    let mut registry = ConnectorRegistry::new();
    registry
        .register_local(LocalCounter("example.counter.memory"))
        .unwrap();
    let result = app().run_local(&registry, |running| {
        running.spawn_task("failure", |_| {
            Err(io::Error::other("injected worker error").into())
        })?;
        assert!(
            running
                .cancellation_token()
                .wait_cancelled_timeout(Duration::from_secs(5))
        );
        assert!(matches!(
            running.check_processes(),
            Err(RuntimeError::TaskFailed { name, .. }) if name == "failure"
        ));
        Ok(())
    });
    assert!(matches!(
        result,
        Err(AppRunError::TaskFailure {
            failure,
            entry: None,
            ..
        }) if matches!(*failure, RuntimeError::TaskFailed { .. })
    ));
}

#[test]
fn structured_tasks_are_joined_before_session_resources_are_destroyed() {
    let mut registry = ConnectorRegistry::new();
    registry
        .register_local(LocalCounter("example.counter.memory"))
        .unwrap();
    let joined = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = Arc::clone(&joined);
    app()
        .run_local(&registry, |running| {
            running.spawn_task("cooperative", move |token| {
                token.wait_cancelled_timeout(Duration::from_secs(5));
                flag.store(true, std::sync::atomic::Ordering::Release);
                Ok(())
            })?;
            Ok(())
        })
        .unwrap();
    assert!(joined.load(std::sync::atomic::Ordering::Acquire));
}
#[test]
fn preflight_orders_all_dependencies_without_connecting_or_starting() {
    let application = App::from_schema(
        r#"{"version":1,"app":{"name":"Example"},"models":{},
            "requires":[
                {"name":"consumer","capability":{"id":"example.counter","version":1},
                 "depends_on":["storage"]},
                {"name":"storage","capability":{"id":"example.counter","version":1}}
            ]}"#,
    )
    .unwrap();
    let deployment = DeploymentConfig {
        services: BTreeMap::from([
            (
                "consumer".into(),
                ServiceEndpoint::Tcp {
                    address: "127.0.0.1:43911".parse().unwrap(),
                },
            ),
            (
                "storage".into(),
                ServiceEndpoint::Managed {
                    instance: None,
                    program: "file-that-does-not-exist".into(),
                    args: vec![],
                    startup_timeout_ms: 1_000,
                    shutdown_timeout_ms: 2_000,
                },
            ),
        ]),
        recovery: Default::default(),
    };
    let mut connectors = ConnectorRegistry::new();
    connectors
        .register(Dummy {
            local_version: 1,
            remote_version: 1,
        })
        .unwrap();
    let plan = application.preflight(&deployment, &connectors).unwrap();
    assert_eq!(plan.services.len(), 2);
    assert_eq!(plan.services[0].requirement, "storage");
    assert_eq!(plan.services[1].requirement, "consumer");
    assert_eq!(plan.services[1].depends_on, ["storage"]);
    assert!(matches!(
        &plan.services[0].target,
        PlannedTarget::Managed { program, .. }
            if program.to_str() == Some("file-that-does-not-exist")
    ));
    let json = serde_json::to_string(&plan).unwrap();
    assert!(json.contains("example.counter"));
    assert!(json.contains("\"args\""));
}

#[test]
fn preflight_rejects_a_late_missing_connector_before_any_process_spawn() {
    let application = App::from_schema(
        r#"{"version":1,"app":{"name":"Example"},"models":{},
            "requires":[
                {"name":"first","capability":{"id":"example.counter","version":1}},
                {"name":"second","capability":{"id":"example.other","version":1}}
            ]}"#,
    )
    .unwrap();
    let config = DeploymentConfig::from_json(
        r#"{"services":{
            "first":{"transport":"managed","program":"nonexistent-first-child"},
            "second":{"transport":"managed","program":"nonexistent-second-child"}
        }}"#,
    )
    .unwrap();
    let mut connectors = ConnectorRegistry::new();
    connectors
        .register(Dummy {
            local_version: 1,
            remote_version: 1,
        })
        .unwrap();
    assert!(matches!(
        application.start(&config, &connectors),
        Err(RuntimeError::MissingConnector { requirement, .. })
            if requirement == "second"
    ));
}

#[test]
fn preflight_rejects_invalid_local_provider_before_remote_io() {
    let application = App::from_schema(
        r#"{"version":1,"app":{"name":"Example"},"models":{},
            "requires":[
                {"name":"first","capability":{"id":"example.counter","version":1}},
                {"name":"second","capability":{"id":"example.counter","version":1}}
            ]}"#,
    )
    .unwrap();
    let config = DeploymentConfig {
        services: BTreeMap::from([
            (
                "first".into(),
                ServiceEndpoint::Tcp {
                    address: "127.0.0.1:43911".parse().unwrap(),
                },
            ),
            (
                "second".into(),
                ServiceEndpoint::Local {
                    provider: "missing.provider".into(),
                },
            ),
        ]),
        recovery: Default::default(),
    };
    let mut connectors = ConnectorRegistry::new();
    connectors
        .register(Dummy {
            local_version: 1,
            remote_version: 1,
        })
        .unwrap();
    assert!(matches!(
        application.start(&config, &connectors),
        Err(RuntimeError::UnknownLocalProvider(provider))
            if provider == "missing.provider"
    ));
}

struct LocalCounter(&'static str);

impl LocalProviderFactory for LocalCounter {
    fn definition(&self) -> Result<ProviderDefinition, ConnectorError> {
        Ok(ProviderDefinition {
            id: self.0.into(),
            capabilities: vec![contract(1).capability],
        })
    }

    fn contracts(&self) -> Result<Vec<CapabilityContract>, ConnectorError> {
        Ok(vec![contract(1)])
    }

    fn create(&self) -> Result<LocalProviderInstance, ConnectorError> {
        Ok(LocalProviderInstance {
            definition: self.definition()?,
            bindings: vec![LocalCapabilityBinding::raw(
                contract(1),
                Box::new(Arc::new(123_u64)),
            )],
        })
    }
}

struct TrackedLocal {
    id: &'static str,
    created: Arc<std::sync::Mutex<Vec<String>>>,
}

impl LocalProviderFactory for TrackedLocal {
    fn definition(&self) -> Result<ProviderDefinition, ConnectorError> {
        Ok(ProviderDefinition {
            id: self.id.into(),
            capabilities: vec![contract(1).capability],
        })
    }

    fn contracts(&self) -> Result<Vec<CapabilityContract>, ConnectorError> {
        Ok(vec![contract(1)])
    }

    fn create(&self) -> Result<LocalProviderInstance, ConnectorError> {
        self.created.lock().unwrap().push(self.id.into());
        Ok(LocalProviderInstance {
            definition: self.definition()?,
            bindings: vec![LocalCapabilityBinding::raw(
                contract(1),
                Box::new(Arc::new(123_u64)),
            )],
        })
    }
}

#[test]
fn dependency_order_controls_actual_provider_startup() {
    let app = App::from_schema(
        r#"{"version":1,"app":{"name":"Example"},"models":{},
            "requires":[
                {"name":"frontend","capability":{"id":"example.counter","version":1},
                 "depends_on":["worker"]},
                {"name":"worker","capability":{"id":"example.counter","version":1},
                 "depends_on":["storage"]},
                {"name":"storage","capability":{"id":"example.counter","version":1}}
            ]}"#,
    )
    .unwrap();
    let created = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut registry = ConnectorRegistry::new();
    let mut services = BTreeMap::new();
    for alias in ["frontend", "worker", "storage"] {
        let provider = format!("example.provider.{alias}");
        registry
            .register_local(TrackedLocal {
                id: match alias {
                    "frontend" => "example.provider.frontend",
                    "worker" => "example.provider.worker",
                    _ => "example.provider.storage",
                },
                created: Arc::clone(&created),
            })
            .unwrap();
        services.insert(alias.into(), ServiceEndpoint::Local { provider });
    }
    let running = app
        .start(&DeploymentConfig::new(services), &registry)
        .unwrap();
    assert_eq!(
        *created.lock().unwrap(),
        [
            "example.provider.storage",
            "example.provider.worker",
            "example.provider.frontend"
        ]
    );
    assert_eq!(running.resolved().len(), 3);
}

fn other_contract() -> CapabilityContract {
    let mut other = contract(1);
    other.capability.id = "example.other".into();
    other
}

struct MultiVersionLocal {
    created: Arc<std::sync::atomic::AtomicUsize>,
}

impl LocalProviderFactory for MultiVersionLocal {
    fn definition(&self) -> Result<ProviderDefinition, ConnectorError> {
        Ok(ProviderDefinition {
            id: "example.counter.multi".into(),
            capabilities: vec![
                contract(1).capability,
                contract(2).capability,
                other_contract().capability,
            ],
        })
    }

    fn contracts(&self) -> Result<Vec<CapabilityContract>, ConnectorError> {
        Ok(vec![contract(1), contract(2), other_contract()])
    }

    fn create(&self) -> Result<LocalProviderInstance, ConnectorError> {
        self.created
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let shared = Arc::new(47_u64);
        Ok(LocalProviderInstance {
            definition: self.definition()?,
            bindings: vec![contract(1), contract(2), other_contract()]
                .into_iter()
                .map(|contract| {
                    LocalCapabilityBinding::raw(contract, Box::new(Arc::clone(&shared)))
                })
                .collect(),
        })
    }
}

#[test]
fn local_provider_exposes_multiple_capabilities_from_one_instance() {
    let app = App::from_schema(
        r#"{"version":1,"app":{"name":"Many"},"models":{},
            "requires":[
                {"name":"counter_v1","capability":{"id":"example.counter","version":1}},
                {"name":"counter_v2","capability":{"id":"example.counter","version":2}},
                {"name":"other","capability":{"id":"example.other","version":1}}
            ]}"#,
    )
    .unwrap();
    let created = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut registry = ConnectorRegistry::new();
    registry
        .register_local(MultiVersionLocal {
            created: Arc::clone(&created),
        })
        .unwrap();
    let running = app.start_local(&registry).unwrap();
    assert_eq!(created.load(std::sync::atomic::Ordering::SeqCst), 1);
    let first = running.service::<Arc<u64>>("counter_v1").unwrap();
    let second = running.service::<Arc<u64>>("counter_v2").unwrap();
    let third = running.service::<Arc<u64>>("other").unwrap();
    assert!(Arc::ptr_eq(first, second));
    assert!(Arc::ptr_eq(first, third));
}

#[test]
fn local_provider_rejects_unadvertised_or_duplicate_bindings() {
    struct Broken;
    impl LocalProviderFactory for Broken {
        fn definition(&self) -> Result<ProviderDefinition, ConnectorError> {
            Ok(ProviderDefinition {
                id: "example.broken.provider".into(),
                capabilities: vec![contract(1).capability],
            })
        }
        fn contracts(&self) -> Result<Vec<CapabilityContract>, ConnectorError> {
            Ok(vec![contract(1)])
        }
        fn create(&self) -> Result<LocalProviderInstance, ConnectorError> {
            Ok(LocalProviderInstance::new(self.definition()?))
        }
    }
    let mut registry = ConnectorRegistry::new();
    registry.register_local(Broken).unwrap();
    assert!(matches!(
        app().start_local(&registry),
        Err(RuntimeError::LocalProviderChanged(id))
            if id == "example.broken.provider"
    ));
}

#[test]
fn starts_local_service_without_tcp_or_deployment_json() {
    let mut connectors = ConnectorRegistry::new();
    connectors
        .register_local(LocalCounter("example.counter.memory"))
        .unwrap();
    let running = app().start_local(&connectors).unwrap();
    assert_eq!(running.resolved()[0].provider, "example.counter.memory");
    assert!(matches!(
        &running.resolved()[0].instance,
        Some(crate::InstanceIdentity::Local { provider })
            if provider == "example.counter.memory"
    ));
    assert_eq!(**running.service::<Arc<u64>>("counter").unwrap(), 123);
}

#[test]
fn local_selection_rejects_missing_and_ambiguous_providers() {
    let empty = ConnectorRegistry::new();
    assert!(matches!(
        app().start_local(&empty),
        Err(RuntimeError::NoLocalProvider { .. })
    ));
    let mut connectors = ConnectorRegistry::new();
    connectors
        .register_local(LocalCounter("example.counter.memory"))
        .unwrap();
    connectors
        .register_local(LocalCounter("example.counter.other"))
        .unwrap();
    assert!(matches!(
        app().start_local(&connectors),
        Err(RuntimeError::AmbiguousLocalProvider { .. })
    ));
    let explicit = DeploymentConfig {
        services: BTreeMap::from([(
            "counter".into(),
            ServiceEndpoint::Local {
                provider: "example.counter.other".into(),
            },
        )]),
        recovery: Default::default(),
    };
    let running = app().start(&explicit, &connectors).unwrap();
    assert_eq!(running.resolved()[0].provider, "example.counter.other");
}

#[test]
fn local_registry_rejects_duplicate_providers() {
    let mut connectors = ConnectorRegistry::new();
    connectors
        .register_local(LocalCounter("example.counter.memory"))
        .unwrap();
    assert!(matches!(
        connectors.register_local(LocalCounter("example.counter.memory")),
        Err(RuntimeError::DuplicateLocalProvider(_))
    ));
}

#[test]
fn two_local_requirements_reuse_the_same_provider_instance() {
    let app = App::from_schema(
        r#"{"version":1,"app":{"name":"Example"},"models":{},
            "requires":[
                {"name":"first","capability":{"id":"example.counter","version":1}},
                {"name":"second","capability":{"id":"example.counter","version":1}}
            ]}"#,
    )
    .unwrap();
    let mut registry = ConnectorRegistry::new();
    registry
        .register_local(LocalCounter("example.counter.memory"))
        .unwrap();

    let running = app.start_local(&registry).unwrap();
    assert_eq!(running.resolved().len(), 2);
    let first = running.service::<Arc<u64>>("first").unwrap();
    let second = running.service::<Arc<u64>>("second").unwrap();
    assert!(Arc::ptr_eq(first, second));
}

#[test]
fn startup_message_accepts_only_versioned_loopback_endpoints() {
    let valid = b"RIESEL_READY {\"protocol\":1,\"address\":\"127.0.0.1:52111\"}\n";
    let ready = await_ready(std::io::Cursor::new(valid.to_vec()), Duration::from_secs(1)).unwrap();
    assert_eq!(ready.address.port(), 52111);
    assert!(ready.shutdown.is_none());

    let cooperative = b"RIESEL_READY {\"protocol\":1,\"address\":\"127.0.0.1:52111\",\"shutdown\":\"stdin_line\"}\n";
    let ready = await_ready(
        std::io::Cursor::new(cooperative.to_vec()),
        Duration::from_secs(1),
    )
    .unwrap();
    assert!(matches!(ready.shutdown, Some(ShutdownMode::StdinLine)));

    for invalid in [
        "RIESEL_READY {\"protocol\":2,\"address\":\"127.0.0.1:52111\"}\n",
        "RIESEL_READY {\"protocol\":1,\"address\":\"0.0.0.0:52111\"}\n",
        "RIESEL_READY {\"protocol\":1,\"address\":\"127.0.0.1:0\"}\n",
        "UNTRUSTED_READY {\"protocol\":1,\"address\":\"127.0.0.1:52111\"}\n",
        "RIESEL_READY {\"protocol\":1,\"address\":\"127.0.0.1:52111\",\"shutdown\":\"unknown\"}\n",
    ] {
        assert!(
            await_ready(
                std::io::Cursor::new(invalid.as_bytes().to_vec()),
                Duration::from_secs(1),
            )
            .is_err()
        );
    }
}

#[test]
fn startup_message_is_length_bounded() {
    let mut payload = vec![b'a'; 8192];
    payload.push(b'\n');
    assert!(await_ready(std::io::Cursor::new(payload), Duration::from_secs(1),).is_err());
}
