use riesel::{AppRunError, Application, AsyncCapability, LifecycleFailure, SdkError};
use riesel_counter::{AsyncCounterContract, AsyncCounterObserverContract, CounterError};
use riesel_counter_component::AsyncMemoryCounter;
use riesel_protocol::CallError;
use std::error::Error;
use std::sync::{Arc, Mutex};

type Events = Arc<Mutex<Vec<&'static str>>>;

fn app() -> Application {
    Application::new("Lifecycle")
        .async_rpc::<AsyncCounterContract>("counter")
        .unwrap()
        .async_rpc::<AsyncCounterObserverContract>("observer")
        .unwrap()
        .depends_on("observer", ["counter"])
        .unwrap()
        .component::<AsyncMemoryCounter>()
        .unwrap()
}

#[test]
fn dependency_order_controls_hooks_and_stop_hooks_have_live_capabilities() {
    let events: Events = Arc::new(Mutex::new(Vec::new()));
    let marks = Arc::clone(&events);
    let prepared = app()
        .on_start_with_aliases(
            "observer",
            move |observer: AsyncCapability<AsyncCounterObserverContract>| {
                let marks = Arc::clone(&marks);
                async move {
                    assert_eq!(observer.observe().await?, 2);
                    marks.lock().unwrap().push("observer:start");
                    Ok::<(), CallError<CounterError>>(())
                }
            },
            ["observer"],
        )
        .unwrap();
    let marks = Arc::clone(&events);
    let prepared = prepared
        .on_start_with_aliases(
            "counter",
            move |counter: AsyncCapability<AsyncCounterContract>| {
                let marks = Arc::clone(&marks);
                async move {
                    assert_eq!(counter.add(2).await?, 2);
                    marks.lock().unwrap().push("counter:start");
                    Ok::<(), CallError<CounterError>>(())
                }
            },
            ["counter"],
        )
        .unwrap();
    let marks = Arc::clone(&events);
    let prepared = prepared
        .on_stop_with_aliases(
            "counter",
            move |counter: AsyncCapability<AsyncCounterContract>| {
                let marks = Arc::clone(&marks);
                async move {
                    assert_eq!(counter.value().await?, 2);
                    marks.lock().unwrap().push("counter:stop");
                    Ok::<(), CallError<CounterError>>(())
                }
            },
            ["counter"],
        )
        .unwrap();
    let marks = Arc::clone(&events);
    let prepared = prepared
        .on_stop_with_aliases(
            "observer",
            move |observer: AsyncCapability<AsyncCounterObserverContract>| {
                let marks = Arc::clone(&marks);
                async move {
                    assert_eq!(observer.observe().await?, 2);
                    marks.lock().unwrap().push("observer:stop");
                    Ok::<(), CallError<CounterError>>(())
                }
            },
            ["observer"],
        )
        .unwrap();
    let marks = Arc::clone(&events);
    prepared
        .async_handler_with_aliases(
            move |counter: AsyncCapability<AsyncCounterContract>| {
                let marks = Arc::clone(&marks);
                async move {
                    assert_eq!(counter.value().await?, 2);
                    marks.lock().unwrap().push("entry");
                    Ok::<(), CallError<CounterError>>(())
                }
            },
            ["counter"],
        )
        .unwrap()
        .run()
        .unwrap();
    assert_eq!(
        *events.lock().unwrap(),
        [
            "counter:start",
            "observer:start",
            "entry",
            "observer:stop",
            "counter:stop"
        ]
    );
}

#[test]
fn failed_start_runs_only_completed_matching_stop_hooks() {
    let events: Events = Arc::new(Mutex::new(Vec::new()));
    let marks = Arc::clone(&events);
    let configured = app()
        .on_start_with_aliases(
            "ready",
            move |counter: AsyncCapability<AsyncCounterContract>| {
                let marks = Arc::clone(&marks);
                async move {
                    assert_eq!(counter.add(1).await?, 1);
                    marks.lock().unwrap().push("ready:start");
                    Ok::<(), CallError<CounterError>>(())
                }
            },
            ["counter"],
        )
        .unwrap()
        .on_start("failure", || async {
            Err::<(), std::io::Error>(std::io::Error::other("start failed"))
        })
        .unwrap();
    let marks = Arc::clone(&events);
    let configured = configured
        .on_stop_with_aliases(
            "ready",
            move |counter: AsyncCapability<AsyncCounterContract>| {
                let marks = Arc::clone(&marks);
                async move {
                    assert_eq!(counter.value().await?, 1);
                    marks.lock().unwrap().push("ready:stop");
                    Ok::<(), CallError<CounterError>>(())
                }
            },
            ["counter"],
        )
        .unwrap()
        .on_stop("failure", || async { Ok::<(), std::io::Error>(()) })
        .unwrap();
    let result = configured
        .async_handler(|| async {
            panic!("entrypoint may not run after start failure");
            #[allow(unreachable_code)]
            Ok::<(), std::io::Error>(())
        })
        .unwrap()
        .run();
    let Err(SdkError::Execution(AppRunError::Lifecycle { failure, .. })) = result else {
        panic!("expected a structured lifecycle error");
    };
    let failure = failure.downcast_ref::<LifecycleFailure>().unwrap();
    assert_eq!(failure.hooks.len(), 1);
    assert_eq!(failure.hooks[0].name, "failure");
    assert_eq!(*events.lock().unwrap(), ["ready:start", "ready:stop"]);
}

#[test]
fn entrypoint_and_stop_failures_remain_separate() {
    let prepared = app()
        .on_stop("cleanup", || async {
            Err::<(), std::io::Error>(std::io::Error::other("stop failed"))
        })
        .unwrap()
        .async_handler(|| async {
            Err::<(), std::io::Error>(std::io::Error::other("entry failed"))
        })
        .unwrap();
    let error = prepared.run().unwrap_err();
    let mut cause: Option<&(dyn Error + 'static)> = Some(&error);
    let mut found_lifecycle = false;
    while let Some(source) = cause {
        if source.is::<LifecycleFailure>() {
            found_lifecycle = true;
            break;
        }
        cause = source.source();
    }
    assert!(found_lifecycle, "lifecycle error must appear in the source chain");
    let SdkError::Execution(AppRunError::Lifecycle { failure, .. }) = error else {
        panic!("expected combined lifecycle error");
    };
    let failure = failure.downcast_ref::<LifecycleFailure>().unwrap();
    assert!(
        failure
            .entry
            .as_ref()
            .unwrap()
            .to_string()
            .contains("entry failed")
    );
    assert_eq!(failure.hooks.len(), 1);
    assert_eq!(failure.hooks[0].name, "cleanup");
}

#[test]
fn duplicate_hooks_and_dependency_declarations_are_rejected() {
    let duplicate = app()
        .on_start("duplicate", || async { Ok::<(), std::io::Error>(()) })
        .unwrap()
        .on_start("duplicate", || async { Ok::<(), std::io::Error>(()) });
    assert!(matches!(
        duplicate,
        Err(SdkError::DuplicateLifecycleHook { phase: "start", .. })
    ));

    let duplicate_dependency = app().depends_on("observer", ["counter"]);
    assert!(matches!(
        duplicate_dependency,
        Err(SdkError::DuplicateDependency { .. })
    ));
}
