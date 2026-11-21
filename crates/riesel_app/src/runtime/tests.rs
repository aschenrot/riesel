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
fn pinned_provider_identity_rejects_unexpected_remote_metadata() {
    let mut connectors = ConnectorRegistry::new();
    connectors
        .register(Dummy {
            local_version: 1,
            remote_version: 1,
        })
        .unwrap();
    let mut config = deployment();
    config.expected_providers.insert(
        "counter".into(),
        ProviderDefinition {
            id: "example.counter.expected".into(),
            capabilities: vec![contract(1).capability],
        },
    );
    assert!(matches!(
        app().start(&config, &connectors),
        Err(RuntimeError::ProviderIdentityMismatch {
            requirement,
            actual,
            ..
        }) if requirement == "counter" && actual.id == "example.counter.local"
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
    let tasks = TaskGroup::new(supervisor.cancellation_token(), SessionToken::new());
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
        graph_admission: Arc::new(crate::execution_runner::SessionGraphAdmission::default()),
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
    let tasks = TaskGroup::new(supervisor.cancellation_token(), SessionToken::new());
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
        graph_admission: Arc::new(crate::execution_runner::SessionGraphAdmission::default()),
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
        expected_providers: BTreeMap::new(),
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
        expected_providers: BTreeMap::new(),
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
            #[cfg(feature = "async-tcp")]
            blocking: None,
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
            #[cfg(feature = "async-tcp")]
            blocking: None,
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
            #[cfg(feature = "async-tcp")]
            blocking: None,
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
        expected_providers: BTreeMap::new(),
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

#[cfg(feature = "async-tcp")]
struct PanickingBlockingLocal;

#[cfg(feature = "async-tcp")]
impl LocalProviderFactory for PanickingBlockingLocal {
    fn definition(&self) -> Result<ProviderDefinition, ConnectorError> {
        LocalCounter("example.counter.panicking").definition()
    }

    fn contracts(&self) -> Result<Vec<CapabilityContract>, ConnectorError> {
        Ok(vec![contract(1)])
    }

    fn create(&self) -> Result<LocalProviderInstance, ConnectorError> {
        LocalCounter("example.counter.panicking").create()
    }

    fn create_with_session(
        &self,
        session: SessionToken,
    ) -> Result<LocalProviderInstance, ConnectorError> {
        let blocking = ProviderBlocking::new(session);
        // Keep the fixture valid for synchronous and asynchronous startup:
        // an async start already has an active Tokio runtime on this thread.
        let worker = blocking.clone();
        let error = std::thread::spawn(move || {
            let executor = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            executor.block_on(worker.run("fixture-failure", |_| -> () {
                panic!("injected provider worker panic")
            }))
        })
        .join()
        .unwrap();
        assert!(error.is_err());
        Ok(self.create()?.with_blocking(blocking))
    }
}

#[cfg(feature = "async-tcp")]
struct InvalidAsyncRemote;

#[cfg(feature = "async-tcp")]
impl CapabilityConnector for InvalidAsyncRemote {
    fn contract(&self) -> Result<CapabilityContract, ConnectorError> {
        Ok(contract(1))
    }

    fn connect(
        &self,
        _endpoint: &ServiceEndpoint,
        _session: &SessionToken,
    ) -> Result<ConnectedProvider, ConnectorError> {
        Ok(ConnectedProvider {
            definition: ProviderDefinition {
                id: "example.invalid.remote".into(),
                capabilities: vec![contract(2).capability],
            },
            contract: contract(2),
            service: Box::new(Arc::new(23_u64)),
        })
    }

    fn supports_async_startup(&self) -> bool {
        true
    }

    fn connect_async<'a>(
        &'a self,
        endpoint: &'a ServiceEndpoint,
        session: &'a SessionToken,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<ConnectedProvider, ConnectorError>> + Send + 'a,
        >,
    > {
        // This fixture produces a value in memory; there is no blocking I/O.
        Box::pin(async move { self.connect(endpoint, session) })
    }
}

#[cfg(feature = "async-tcp")]
fn rollback_fixture() -> (App, DeploymentConfig, ConnectorRegistry) {
    let app = App::from_schema(
        r#"{"version":1,"app":{"name":"Rollback"},"models":{},
            "requires":[
                {"name":"first","capability":{"id":"example.counter","version":1}},
                {"name":"later","capability":{"id":"example.counter","version":1},
                 "depends_on":["first"]}
            ]}"#,
    )
    .unwrap();
    let config = DeploymentConfig::new(BTreeMap::from([
        (
            "first".into(),
            ServiceEndpoint::Local {
                provider: "example.counter.panicking".into(),
            },
        ),
        (
            "later".into(),
            ServiceEndpoint::Tcp {
                address: "127.0.0.1:47911".parse().unwrap(),
            },
        ),
    ]));
    let mut registry = ConnectorRegistry::new();
    registry.register_local(PanickingBlockingLocal).unwrap();
    registry.register(InvalidAsyncRemote).unwrap();
    (app, config, registry)
}

#[cfg(feature = "async-tcp")]
fn assert_startup_and_rollback_errors(error: RuntimeError) {
    use std::error::Error;

    let RuntimeError::StartupRollback { startup, cleanup } = &error else {
        panic!("the primary startup and provider cleanup errors must both survive: {error}");
    };
    assert!(matches!(
        startup.as_ref(),
        RuntimeError::RemoteContractMismatch { requirement }
            if requirement == "later"
    ));
    assert!(matches!(
        cleanup.as_ref(),
        RuntimeError::ProviderBlockingShutdown { source }
            if source.to_string().contains("fixture-failure")
    ));
    assert!(Error::source(&error).is_some_and(|source| source.to_string().contains("later")));
}

#[cfg(feature = "async-tcp")]
#[test]
fn synchronous_startup_preserves_original_and_provider_cleanup_failures() {
    let (app, config, registry) = rollback_fixture();
    let Err(error) = app.start(&config, &registry) else {
        panic!("late contract mismatch must fail after local provider construction");
    };
    assert_startup_and_rollback_errors(error);
}

#[cfg(feature = "async-tcp")]
#[test]
fn asynchronous_startup_preserves_original_and_provider_cleanup_failures() {
    let (app, config, registry) = rollback_fixture();
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let Err(error) = executor.block_on(app.start_async(&config, &registry)) else {
        panic!("late async contract mismatch must report provider cleanup failures");
    };
    assert_startup_and_rollback_errors(error);
}

#[cfg(feature = "async-tcp")]
#[test]
fn session_owned_graph_owns_thread_confined_domain_without_duplicate_scheduler() {
    use crate::execution_plan::{
        ExecutionPlanDraft, OperationKey, OperationNode, OperationRegistry,
    };
    use crate::execution_runner::{
        AsyncOperation, GraphRunError, OperationOutcome, async_operation,
    };
    use std::num::NonZeroUsize;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    let mut connectors = ConnectorRegistry::new();
    connectors
        .register(Dummy {
            local_version: 1,
            remote_version: 1,
        })
        .unwrap();
    let mut running = app().start(&deployment(), &connectors).unwrap();
    let resolved = Arc::clone(running.service::<Arc<u64>>("counter").unwrap());
    let observed_ticks = Arc::new(AtomicUsize::new(0));
    let world_observation = Arc::clone(&observed_ticks);

    // The world is deliberately !Send; it is created and retained entirely
    // inside the domain-owned thread, never moved into Tokio's graph runner.
    let (actor_sender, actor_receiver) =
        std::sync::mpsc::channel::<tokio::sync::oneshot::Sender<usize>>();
    let actor = std::thread::spawn(move || {
        let world = std::rc::Rc::new(std::cell::Cell::new(0usize));
        while let Ok(reply) = actor_receiver.recv() {
            let next = world.get() + 1;
            world.set(next);
            world_observation.store(next, Ordering::SeqCst);
            let _ = reply.send(next);
        }
    });

    let tick = OperationKey::new("example.domain.tick", 1).unwrap();
    let background = OperationKey::new("example.capability.read", 1).unwrap();
    let tick_sender = actor_sender.clone();
    let background_completed = Arc::new(AtomicUsize::new(0));
    let background_observer = Arc::clone(&background_completed);
    let mut registry = OperationRegistry::<AsyncOperation>::new();
    registry
        .register(
            tick.clone(),
            async_operation(move || {
                let sender = tick_sender.clone();
                async move {
                    let (reply, result) = tokio::sync::oneshot::channel();
                    if sender.send(reply).is_err() {
                        return OperationOutcome::Failed("domain actor unavailable".into());
                    }
                    match result.await {
                        Ok(_) => OperationOutcome::Completed,
                        Err(_) => OperationOutcome::OutcomeUnknown,
                    }
                }
            }),
        )
        .unwrap();
    registry
        .register(
            background.clone(),
            async_operation(move || {
                let capability = Arc::clone(&resolved);
                let completed = Arc::clone(&background_observer);
                async move {
                    if *capability == 42 {
                        completed.store(1, Ordering::SeqCst);
                        OperationOutcome::Completed
                    } else {
                        OperationOutcome::Failed("wrong resolved capability".into())
                    }
                }
            }),
        )
        .unwrap();

    let plan = ExecutionPlanDraft::new("example.domain-run", 1)
        .operation(OperationNode::new("tick-one", tick.clone()).on_signal("domain.tick.one"))
        .operation(
            OperationNode::new("tick-two", tick)
                .after("tick-one")
                .on_signal("domain.tick.two"),
        )
        .operation(OperationNode::new("background", background))
        .validate()
        .unwrap()
        .bind(&registry)
        .unwrap();
    drop(registry);
    drop(actor_sender);

    let async_runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let report = async_runtime.block_on(async {
        let runner = running
            .execution_graph(plan, NonZeroUsize::new(3).unwrap())
            .unwrap();
        let admission = runner.signal_sender();
        let running_graph = tokio::spawn(runner.execute());

        // The independent capability operation runs while no domain tick is
        // admitted; the graph cannot implicitly schedule the domain itself.
        tokio::time::timeout(Duration::from_secs(3), async {
            while background_completed.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(observed_ticks.load(Ordering::SeqCst), 0);
        admission.fire("domain.tick.one").unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while observed_ticks.load(Ordering::SeqCst) != 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(!running_graph.is_finished());
        admission.fire("domain.tick.two").unwrap();
        let result = tokio::time::timeout(Duration::from_secs(3), running_graph)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(observed_ticks.load(Ordering::SeqCst), 2);
        assert_eq!(result.peak_in_flight, 1);
        assert_eq!(result.started, 3);
        assert!(result.succeeded());
        (result, admission)
    });
    assert_eq!(report.0.revision, 1);
    running.shutdown().unwrap();
    assert!(matches!(
        report.1.fire("domain.tick.one"),
        Err(GraphRunError::SessionExpired)
    ));
    actor.join().unwrap();
}

#[cfg(feature = "async-tcp")]
#[test]
fn graph_runner_real_local_session_shutdown_preserves_completed_prefix_and_unknown_work() {
    use crate::execution_plan::{
        ExecutionPlanDraft, OperationKey, OperationNode, OperationRegistry,
    };
    use crate::execution_runner::{
        AsyncOperation, GraphRunError, OperationOutcome, OperationState, async_operation,
    };
    use std::num::NonZeroUsize;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let mut connectors = ConnectorRegistry::new();
    connectors
        .register_local(LocalCounter("example.counter.memory"))
        .unwrap();
    let mut running = app().start_local(&connectors).unwrap();
    let resolved = Arc::clone(running.service::<Arc<u64>>("counter").unwrap());
    let committed = Arc::new(AtomicUsize::new(0));
    let dispatched = Arc::new(AtomicUsize::new(0));
    let blocked_executed = Arc::new(AtomicUsize::new(0));
    let mut handlers = OperationRegistry::<AsyncOperation>::new();
    handlers
        .register(
            OperationKey::new("commit", 1).unwrap(),
            async_operation({
                let committed = Arc::clone(&committed);
                move || {
                    let resolved = Arc::clone(&resolved);
                    let committed = Arc::clone(&committed);
                    async move {
                        if *resolved != 123 {
                            return OperationOutcome::Failed(
                                "wrong already-resolved capability".into(),
                            );
                        }
                        committed.fetch_add(1, Ordering::SeqCst);
                        OperationOutcome::Completed
                    }
                }
            }),
        )
        .unwrap();
    handlers
        .register(
            OperationKey::new("dispatch", 1).unwrap(),
            async_operation({
                let dispatched = Arc::clone(&dispatched);
                move || {
                    let dispatched = Arc::clone(&dispatched);
                    async move {
                        // Sending an external mutation is not undone by
                        // dropping the future before its response arrives.
                        dispatched.fetch_add(1, Ordering::SeqCst);
                        std::future::pending::<()>().await;
                        OperationOutcome::Completed
                    }
                }
            }),
        )
        .unwrap();
    handlers
        .register(
            OperationKey::new("dependent", 1).unwrap(),
            async_operation({
                let executed = Arc::clone(&blocked_executed);
                move || {
                    let executed = Arc::clone(&executed);
                    async move {
                        executed.fetch_add(1, Ordering::SeqCst);
                        OperationOutcome::Completed
                    }
                }
            }),
        )
        .unwrap();
    handlers
        .register(
            OperationKey::new("signal", 1).unwrap(),
            async_operation(|| async { OperationOutcome::Completed }),
        )
        .unwrap();

    let plan = ExecutionPlanDraft::new("lifecycle.committed-prefix", 1)
        .operation(OperationNode::new(
            "commit",
            OperationKey::new("commit", 1).unwrap(),
        ))
        .operation(
            OperationNode::new("dispatch", OperationKey::new("dispatch", 1).unwrap())
                .after("commit"),
        )
        .operation(
            OperationNode::new("dependent", OperationKey::new("dependent", 1).unwrap())
                .after("dispatch"),
        )
        .operation(
            OperationNode::new("signal", OperationKey::new("signal", 1).unwrap())
                .on_signal("never"),
        )
        .validate()
        .unwrap()
        .bind(&handlers)
        .unwrap();

    let runner = running
        .execution_graph(plan, NonZeroUsize::new(2).unwrap())
        .unwrap();
    let stale_admission = runner.signal_sender();
    let worker = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(runner.execute())
    });

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while dispatched.load(Ordering::SeqCst) == 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "graph did not dispatch"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(committed.load(Ordering::SeqCst), 1);
    running.shutdown().unwrap();
    let report = worker.join().unwrap();

    assert!(report.session_expired);
    assert_eq!(report.operations["commit"], OperationState::Completed);
    assert_eq!(
        report.operations["dispatch"],
        OperationState::OutcomeUnknown
    );
    assert_eq!(report.operations["dependent"], OperationState::Blocked);
    assert_eq!(report.operations["signal"], OperationState::Cancelled);
    assert_eq!(report.started, 2);
    assert_eq!(committed.load(Ordering::SeqCst), 1);
    assert_eq!(dispatched.load(Ordering::SeqCst), 1);
    assert_eq!(blocked_executed.load(Ordering::SeqCst), 0);
    assert!(matches!(
        stale_admission.fire("never"),
        Err(GraphRunError::SessionExpired)
    ));

    // Restarting the same app does not automatically replay a graph or
    // reauthorize handles retained from its previous generation.
    let new_session = app().start_local(&connectors).unwrap();
    assert_eq!(**new_session.service::<Arc<u64>>("counter").unwrap(), 123);
    assert!(matches!(
        stale_admission.fire("never"),
        Err(GraphRunError::SessionExpired)
    ));
    assert_eq!(dispatched.load(Ordering::SeqCst), 1);
}

#[cfg(feature = "async-tcp")]
#[test]
fn structured_background_graph_drains_before_local_provider_teardown() {
    use crate::execution_plan::{
        ExecutionPlanDraft, OperationKey, OperationNode, OperationRegistry,
    };
    use crate::execution_runner::{AsyncOperation, OperationOutcome, async_operation};
    use std::num::NonZeroUsize;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct DropProbe(Arc<AtomicBool>);
    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    let mut connectors = ConnectorRegistry::new();
    connectors
        .register_local(LocalCounter("example.counter.memory"))
        .unwrap();
    let mut running = app().start_local(&connectors).unwrap();
    let started = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicBool::new(false));
    let resolved = Arc::clone(running.service::<Arc<u64>>("counter").unwrap());
    let mut handlers = OperationRegistry::<AsyncOperation>::new();
    handlers
        .register(
            OperationKey::new("pending", 1).unwrap(),
            async_operation({
                let started = Arc::clone(&started);
                let dropped = Arc::clone(&dropped);
                move || {
                    let started = Arc::clone(&started);
                    let dropped = Arc::clone(&dropped);
                    let resolved = Arc::clone(&resolved);
                    async move {
                        let _guard = DropProbe(dropped);
                        assert_eq!(*resolved, 123);
                        started.store(true, Ordering::SeqCst);
                        std::future::pending::<()>().await;
                        OperationOutcome::Completed
                    }
                }
            }),
        )
        .unwrap();
    let runner = running
        .execution_graph(
            ExecutionPlanDraft::new("structured-background", 1)
                .operation(OperationNode::new(
                    "pending",
                    OperationKey::new("pending", 1).unwrap(),
                ))
                .validate()
                .unwrap()
                .bind(&handlers)
                .unwrap(),
            NonZeroUsize::new(1).unwrap(),
        )
        .unwrap();
    let tasks = running.async_tasks();
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    executor.block_on(async {
        tasks.begin_scope().unwrap();
        tasks.spawn_graph("graph", runner).unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !started.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        tasks.prepare_stop().await.unwrap();
        assert!(dropped.load(Ordering::SeqCst));
        // The resolved provider is still present after the graph drains.
        assert_eq!(**running.service::<Arc<u64>>("counter").unwrap(), 123);
        tasks.finish_scope().await.unwrap();
    });
    running.shutdown().unwrap();
}

#[cfg(feature = "async-tcp")]
#[test]
fn background_graph_failure_retains_named_task_error_and_report() {
    use crate::execution_plan::{
        ExecutionPlanDraft, OperationKey, OperationNode, OperationRegistry,
    };
    use crate::execution_runner::{
        AsyncOperation, GraphExecutionFailure, OperationOutcome, OperationState, async_operation,
    };
    use std::num::NonZeroUsize;

    let mut running = App::from_schema(
        r#"{"version":1,"app":{"name":"BackgroundGraphFailure"},"models":{},"requires":[]}"#,
    )
    .unwrap()
    .start_local(&ConnectorRegistry::new())
    .unwrap();
    let mut handlers = OperationRegistry::<AsyncOperation>::new();
    handlers
        .register(
            OperationKey::new("fail", 1).unwrap(),
            async_operation(|| async { OperationOutcome::Failed("explicit failure".into()) }),
        )
        .unwrap();
    let runner = running
        .execution_graph(
            ExecutionPlanDraft::new("background-failure", 1)
                .operation(OperationNode::new(
                    "fail",
                    OperationKey::new("fail", 1).unwrap(),
                ))
                .validate()
                .unwrap()
                .bind(&handlers)
                .unwrap(),
            NonZeroUsize::new(1).unwrap(),
        )
        .unwrap();
    let tasks = running.async_tasks();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            tasks.begin_scope().unwrap();
            tasks.spawn_graph("failed-graph", runner).unwrap();
            tokio::time::timeout(Duration::from_secs(3), running.session_token().cancelled())
                .await
                .unwrap();
            let failure = tasks.finish_scope().await.unwrap_err();
            match failure {
                RuntimeError::TaskFailed { name, source } => {
                    assert_eq!(name, "failed-graph");
                    let graph = source
                        .downcast_ref::<GraphExecutionFailure>()
                        .expect("failed background task must retain the local graph result");
                    assert_eq!(
                        graph.report.operations["fail"],
                        OperationState::Failed("explicit failure".into())
                    );
                    assert!(!graph.report.succeeded());
                }
                error => panic!("expected a named background graph failure: {error}"),
            }
        });
    assert!(matches!(
        running.shutdown(),
        Err(RuntimeError::TaskFailed { name, .. }) if name == "failed-graph"
    ));
}
