//! #157 feature-gated runtime ownership acceptance: an admitted publication
//! operation must be owned by the existing application AsyncTasks scope.
//! No external caller authentication, exported interface or credential here.
use crate::{App, ConnectorError, ConnectorRegistry, RunningApp, RuntimeError};
use riesel_protocol::session::{InternalAdmissionError, InternalDrain};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

fn start_runtime_session() -> RunningApp {
    App::from_schema(r#"{"version":1,"app":{"name":"OwnedIngress"},"models":{},"requires":[]}"#)
        .unwrap()
        .start_local(&ConnectorRegistry::new())
        .unwrap()
}

#[test]
fn admitted_async_work_is_joined_before_new_app_session_and_ordinary_stop() {
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    executor.block_on(async {
        let mut first = start_runtime_session();
        let old_session = first.session_token();
        let first_publication = old_session.internal_publication_session();
        let old_generation = first_publication.generation();
        let tasks = first.async_tasks();
        tasks.begin_scope().unwrap();

        let effects = Arc::new(AtomicUsize::new(0));
        let attempted_effects = Arc::clone(&effects);
        let first_worker = first_publication.clone();
        let expected = old_generation.clone();
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        tasks
            .spawn("owned-inflight-request", async move {
                // A real runtime-owned task holds its ingress permit for the
                // whole dispatched future, including its pending duration.
                let _permit = first_worker.try_admit(&expected).unwrap();
                entered_tx.send(()).unwrap();
                std::future::pending::<()>().await;
                attempted_effects.fetch_add(1, Ordering::SeqCst);
                Ok::<(), ConnectorError>(())
            })
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), entered_rx)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first_publication.in_flight(), 1);
        assert_eq!(effects.load(Ordering::SeqCst), 0);

        // Normal async app stop must drop and join the owned worker while
        // closing admission. It must not wait indefinitely for a pending
        // future or drop its permit before the request future ends.
        tokio::time::timeout(Duration::from_secs(2), tasks.finish_scope())
            .await
            .expect("application-owned request must be cancelled and joined")
            .unwrap();
        assert_eq!(effects.load(Ordering::SeqCst), 0);
        assert_eq!(first_publication.in_flight(), 0);
        assert_eq!(
            first_publication.drain_for(Duration::from_secs(1)).await,
            InternalDrain::Drained
        );
        assert_eq!(
            first_publication.try_admit(&old_generation).unwrap_err(),
            InternalAdmissionError::Expired
        );
        assert!(matches!(
            tasks.spawn("late-request", async { Ok::<(), ConnectorError>(()) }),
            Err(RuntimeError::TaskGroupClosed)
        ));
        first.shutdown().unwrap();

        // Same OS process, new actual runtime; neither an old attachment nor
        // an old worker can be promoted into the successor application.
        let mut second = start_runtime_session();
        let second_publication = second.session_token().internal_publication_session();
        let second_generation = second_publication.generation();
        assert_ne!(old_generation, second_generation);
        assert_eq!(
            second_publication.try_admit(&old_generation).unwrap_err(),
            InternalAdmissionError::WrongGeneration
        );
        assert_eq!(
            first_publication.try_admit(&old_generation).unwrap_err(),
            InternalAdmissionError::Expired
        );

        let current_tasks = second.async_tasks();
        current_tasks.begin_scope().unwrap();
        let current_publication = second_publication.clone();
        let current_effects = Arc::clone(&effects);
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        current_tasks
            .spawn("fresh-owned-request", async move {
                let _permit = current_publication.try_admit(&second_generation).unwrap();
                current_effects.fetch_add(1, Ordering::SeqCst);
                done_tx.send(()).unwrap();
                Ok::<(), ConnectorError>(())
            })
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), done_rx)
            .await
            .unwrap()
            .unwrap();
        current_tasks.finish_scope().await.unwrap();
        assert_eq!(effects.load(Ordering::SeqCst), 1);
        assert_eq!(second_publication.in_flight(), 0);
        second.shutdown().unwrap();
    });
}

#[test]
fn runtime_publication_graph_nested_and_linked_record_owners_remain_distinct() {
    use crate::execution_plan::{
        ExecutionPlanDraft, OperationKey, OperationNode, OperationRegistry,
    };
    use crate::execution_runner::{AsyncOperation, OperationOutcome, async_operation};
    use crate::NestedDomainOwner;
    use std::num::NonZeroUsize;

    fn no_provider_graph() -> crate::BoundExecutionPlan<AsyncOperation> {
        let key = OperationKey::new("test.epoch", 1).unwrap();
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key.clone(),
                async_operation(|| async { OperationOutcome::Completed }),
            )
            .unwrap();
        ExecutionPlanDraft::new("independent-run", 1)
            .operation(OperationNode::new("op", key))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap()
    }

    // A Query data owner is a verified relationship among model, reader and
    // optional writer roles, NOT a runtime, graph or nested observation epoch.
    // A separate #160 integration verifies a real local shared provider.
    let linked = App::from_schema(
        r#"{"version":2,"app":{"name":"StableModelAuthority"},
           "models":{"task":{"fields":{"title":{"kind":"string"}}}},
           "model_bindings":{"task":["writer"]},
           "record_query_links":{"task":{"reader":"read","writer":"writer",
                                        "consistency":"linked_session_live"}},
           "requires":[
             {"name":"read","capability":{"id":"example.counter","version":1}},
             {"name":"writer","capability":{"id":"example.records.writer","version":1}}
           ]}"#,
    )
    .unwrap();
    let link = linked.definition().record_query_links["task"].clone();
    assert_eq!(link.reader, "read");
    assert_eq!(link.writer.as_deref(), Some("writer"));

    let domain = NestedDomainOwner::new("simulation");
    let domain_epoch = domain.generation();

    let mut first = start_runtime_session();
    let first_publication = first.session_token().internal_publication_session();
    let first_generation = uuid::Uuid::parse_str(&first_publication.generation()).unwrap();

    let graph_a = first
        .execution_graph(no_provider_graph(), NonZeroUsize::new(1).unwrap())
        .unwrap();
    let graph_b = first
        .execution_graph(no_provider_graph(), NonZeroUsize::new(1).unwrap())
        .unwrap();
    let graph_a_id = graph_a.observer().snapshot().identity;
    let graph_b_id = graph_b.observer().snapshot().identity;

    assert_eq!(
        graph_a_id.session_generation(),
        graph_b_id.session_generation(),
        "two separate runs of one application use one graph admission generation"
    );
    assert_ne!(
        graph_a_id.run_sequence(),
        graph_b_id.run_sequence(),
        "each graph run is independently sequenced within that generation"
    );
    assert_ne!(
        graph_a_id.session_generation(),
        first_generation,
        "graph-run identity is not an application publication credential"
    );
    assert_ne!(
        domain_epoch,
        first_generation,
        "nested domain ownership is not the Riesel publication generation"
    );
    drop(graph_a);
    drop(graph_b);
    first.shutdown().unwrap();

    let mut second = start_runtime_session();
    let second_publication = second.session_token().internal_publication_session();
    let second_generation = uuid::Uuid::parse_str(&second_publication.generation()).unwrap();
    assert_ne!(first_generation, second_generation);
    let successor_graph = second
        .execution_graph(no_provider_graph(), NonZeroUsize::new(1).unwrap())
        .unwrap();
    let successor_graph_id = successor_graph.observer().snapshot().identity;
    assert_ne!(
        graph_a_id.session_generation(),
        successor_graph_id.session_generation(),
        "whole-app recovery must not reuse graph admission identity"
    );
    assert_eq!(
        domain.generation(),
        domain_epoch,
        "a domain-owned epoch cannot be silently substituted by app recovery"
    );
    assert_eq!(
        linked.definition().record_query_links["task"],
        link,
        "linked logical data authority is declared independently of runtime epochs"
    );
    assert_eq!(
        first_publication
            .try_admit(&first_generation.to_string())
            .unwrap_err(),
        InternalAdmissionError::Expired,
    );
    assert!(
        second_publication.try_admit(&second_generation.to_string()).is_ok(),
        "the new app publication can admit without changing query or nested owner roles"
    );

    drop(successor_graph);
    second.shutdown().unwrap();
}
