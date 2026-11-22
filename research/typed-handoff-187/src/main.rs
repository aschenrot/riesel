//! Research #187 / Grid Workbench #175 — the *current* Riesel graph runner.
//!
//! Crucial distinction: Riesel validates control dependencies and handles
//! operation *outcomes*. It does NOT yet validate executable typed ports or
//! transfer node output values to node inputs. This fixture deliberately
//! supplies owned typed data through application-owned Arc snapshots, with
//! the real Riesel runner deciding outer operation admission.
//!
//! No production S3 format, scheduler, data-transfer protocol, node registry,
//! cross-process borrowing, automatic cache or visual editor is implemented.
#![forbid(unsafe_code)]

#[cfg(test)]
mod typed_ports;

use riesel::{
    Application, AsyncOperation, ExecutionPlanDraft, OperationKey, OperationNode, OperationOutcome,
    OperationRegistry, async_operation,
};
use std::{
    io, num::NonZeroUsize,
    sync::{Arc, Mutex},
};

const DIM: usize = 64;

#[derive(Debug, Clone, PartialEq)]
struct ScalarGrid {
    width: usize,
    height: usize,
    samples: Arc<Vec<f64>>,
}

impl ScalarGrid {
    fn synthetic() -> Self {
        // Deliberately not an implementation of Noise2D.
        Self {
            width: DIM,
            height: DIM,
            samples: Arc::new((0..DIM * DIM).map(|i| (i % 100) as f64 / 100.0).collect()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct OccupancyGrid {
    occupied: Vec<bool>,
}

#[derive(Debug, Clone)]
struct Snapshot<T> {
    source_revision: u64,
    value: T,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PortType {
    ScalarGrid2D,
    OccupancyGrid2D,
    Scalar,
}

fn validate_local_port_edge(from: PortType, to: PortType) -> Result<(), &'static str> {
    if from == to {
        Ok(())
    } else {
        Err("incompatible typed data ports")
    }
}

fn run_current_production_graph_with_domain_owned_data() -> riesel::Result<()> {
    Application::new("S3 Feasibility: No Runtime Value Ports").run(|context| {
        // Domain-owned value references are NOT Riesel graph outputs.
        let shared = Arc::new(Mutex::new(None::<Snapshot<ScalarGrid>>));
        let occupancy = Arc::new(Mutex::new(None::<Snapshot<OccupancyGrid>>));
        let heat = Arc::new(Mutex::new(None::<Snapshot<f64>>));
        let mut registry = OperationRegistry::<AsyncOperation>::new();

        registry.register(
            OperationKey::new("research.scalar_source", 1)?,
            async_operation({
                let shared = Arc::clone(&shared);
                move || {
                    let shared = Arc::clone(&shared);
                    async move {
                        *shared.lock().unwrap() = Some(Snapshot {
                            source_revision: 1,
                            value: ScalarGrid::synthetic(),
                        });
                        OperationOutcome::Completed
                    }
                }
            }),
        )?;

        registry.register(
            OperationKey::new("research.threshold", 1)?,
            async_operation({
                let shared = Arc::clone(&shared);
                let occupancy = Arc::clone(&occupancy);
                move || {
                    let shared = Arc::clone(&shared);
                    let occupancy = Arc::clone(&occupancy);
                    async move {
                        let source = shared.lock().unwrap().clone();
                        let Some(source) = source else {
                            return OperationOutcome::Failed("missing typed scalar artifact".into());
                        };
                        if source.source_revision != 1 {
                            return OperationOutcome::Failed("stale source revision".into());
                        }
                        if source.value.samples.len() != source.value.width * source.value.height {
                            return OperationOutcome::Failed("malformed source grid".into());
                        }
                        let result = OccupancyGrid {
                            occupied: source.value.samples.iter().map(|v| *v > 0.55).collect(),
                        };
                        *occupancy.lock().unwrap() = Some(Snapshot {
                            source_revision: source.source_revision,
                            value: result,
                        });
                        OperationOutcome::Completed
                    }
                }
            }),
        )?;

        registry.register(
            OperationKey::new("research.heat_summary", 1)?,
            async_operation({
                let shared = Arc::clone(&shared);
                let heat = Arc::clone(&heat);
                move || {
                    let shared = Arc::clone(&shared);
                    let heat = Arc::clone(&heat);
                    async move {
                        let source = shared.lock().unwrap().clone();
                        let Some(source) = source else {
                            return OperationOutcome::Failed("missing scalar data".into());
                        };
                        if source.source_revision != 1 || source.value.samples.is_empty() {
                            return OperationOutcome::Failed("invalid scalar summary input".into());
                        }
                        let mean = source.value.samples.iter().sum::<f64>()
                            / source.value.samples.len() as f64;
                        *heat.lock().unwrap() = Some(Snapshot {
                            source_revision: source.source_revision,
                            value: mean,
                        });
                        OperationOutcome::Completed
                    }
                }
            }),
        )?;

        // The after() edges are the CURRENT scheduler's control prerequisites.
        // The actual typed data are only in the app-owned snapshots above.
        let plan = ExecutionPlanDraft::new("research.187.fanout", 1)
            .operation(OperationNode::new(
                "scalar", OperationKey::new("research.scalar_source", 1)?,
            ))
            .operation(OperationNode::new(
                "threshold", OperationKey::new("research.threshold", 1)?,
            ).after("scalar"))
            .operation(OperationNode::new(
                "heat", OperationKey::new("research.heat_summary", 1)?,
            ).after("scalar"))
            .validate()?
            .bind(&registry)?;

        let runner = context.execution_graph(plan, NonZeroUsize::new(3).unwrap())?;
        let observer = runner.observer();
        let tokio = tokio::runtime::Builder::new_current_thread()
            .enable_all().build()?;
        let report = tokio.block_on(runner.execute());

        if !report.succeeded() {
            return Err(io::Error::other(format!(
                "actual runner failed: {:?}", report.operations,
            )).into());
        }
        let source = shared.lock().unwrap().clone().ok_or_else(|| io::Error::other("no source"))?;
        let occupied = occupancy.lock().unwrap().clone().ok_or_else(|| io::Error::other("no occupancy"))?;
        let temperature = heat.lock().unwrap().clone().ok_or_else(|| io::Error::other("no heat summary"))?;
        if occupied.source_revision != report.revision
            || source.source_revision != report.revision
            || temperature.source_revision != report.revision
            || occupied.value.occupied.len() != DIM * DIM
            || !(0.0..=1.0).contains(&temperature.value)
            || observer.snapshot().identity != report.identity
        {
            return Err(io::Error::other("typed sidecar and runtime identity differ").into());
        }
        if !Arc::ptr_eq(&source.value.samples, &shared.lock().unwrap().as_ref().unwrap().value.samples) {
            return Err(io::Error::other("source sample buffer identity changed").into());
        }

        println!("Real Riesel control graph: scalar -> occupancy + heat-summary, revision {}", report.revision);
        println!("Typed values are app-owned, NOT Riesel executable value ports");
        Ok(())
    })
}

fn main() -> riesel::Result<()> {
    run_current_production_graph_with_domain_owned_data()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_ports_reject_implicit_scalar_to_occupancy_conversion() {
        assert!(validate_local_port_edge(PortType::ScalarGrid2D, PortType::ScalarGrid2D).is_ok());
        assert!(validate_local_port_edge(PortType::Scalar, PortType::Scalar).is_ok());
        assert_eq!(
            validate_local_port_edge(PortType::ScalarGrid2D, PortType::OccupancyGrid2D),
            Err("incompatible typed data ports"),
        );
        // This is a **local toy preflight**, not any existing production
        // Riesel validator capability.
    }

    #[test]
    fn explicit_threshold_is_not_implicit_port_cast() {
        let scalar = ScalarGrid::synthetic();
        let occupancy = OccupancyGrid {
            occupied: scalar.samples.iter().map(|v| *v > 0.55).collect(),
        };
        assert_eq!(occupancy.occupied.len(), 4096);
        assert!(!occupancy.occupied[0]);
        assert!(occupancy.occupied[56]);
    }

    /// Current S2 control completion is not a guarantee that a typed output
    /// exists. This deliberately faulty producer is marked Completed without
    /// publishing a value, and the dependent callback must reject it itself.
    #[test]
    fn control_completion_without_data_must_fail_inside_consumer() {
        use riesel::OperationState;

        Application::new("Research S3 Missing Data").run(|context| {
            let value = Arc::new(Mutex::new(None::<u64>));
            let visited = Arc::new(Mutex::new(false));
            let mut registry = OperationRegistry::<AsyncOperation>::new();

            registry.register(
                OperationKey::new("source.completes_without_data", 1)?,
                async_operation(|| async { OperationOutcome::Completed }),
            )?;
            registry.register(
                OperationKey::new("consumer.must_validate_data", 1)?,
                async_operation({
                    let value = Arc::clone(&value);
                    let visited = Arc::clone(&visited);
                    move || {
                        let value = Arc::clone(&value);
                        let visited = Arc::clone(&visited);
                        async move {
                            *visited.lock().unwrap() = true;
                            if value.lock().unwrap().is_some() {
                                OperationOutcome::Completed
                            } else {
                                OperationOutcome::Failed(
                                    "control dependency completed but input artifact absent".into(),
                                )
                            }
                        }
                    }
                }),
            )?;
            let plan = ExecutionPlanDraft::new("research.187.missing-data", 1)
                .operation(OperationNode::new(
                    "source", OperationKey::new("source.completes_without_data", 1)?,
                ))
                .operation(OperationNode::new(
                    "consumer", OperationKey::new("consumer.must_validate_data", 1)?,
                ).after("source"))
                .validate()?
                .bind(&registry)?;
            let runner = context.execution_graph(plan, NonZeroUsize::new(1).unwrap())?;
            let observer = runner.observer();
            let tokio = tokio::runtime::Builder::new_current_thread()
                .enable_all().build()?;
            let report = tokio.block_on(runner.execute());

            assert!(!report.succeeded());
            assert!(*visited.lock().unwrap(), "consumer was admitted after control completion");
            assert!(
                matches!(
                    &observer.snapshot().operations["consumer"].state,
                    OperationState::Failed(_),
                ),
                "consumer must fail when its expected value is absent",
            );
            Ok(())
        }).unwrap();
    }

    /// Demonstrate why an application-wide mutable "latest output" is not
    /// the same as immutable run-scoped typed artifacts. The actual Riesel
    /// session runner permits two eligible independent plan runs; our local
    /// snapshot owner must decline a late older revision.
    #[test]
    fn late_older_graph_run_cannot_overwrite_newer_domain_artifact() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::Duration;
        use tokio::sync::Semaphore;

        Application::new("Research #187 Stale Artifact").run(|context| {
            let output = Arc::new(Mutex::new((0u64, 0u64))); // (revision, value)
            let entered_old = Arc::new(AtomicBool::new(false));
            let old_release = Arc::new(Semaphore::new(0));
            let mut registry = OperationRegistry::<AsyncOperation>::new();

            registry.register(
                OperationKey::new("research.old_producer", 1)?,
                async_operation({
                    let output = Arc::clone(&output);
                    let entered_old = Arc::clone(&entered_old);
                    let old_release = Arc::clone(&old_release);
                    move || {
                        let output = Arc::clone(&output);
                        let entered_old = Arc::clone(&entered_old);
                        let old_release = Arc::clone(&old_release);
                        async move {
                            entered_old.store(true, Ordering::SeqCst);
                            let _permit = old_release.acquire().await
                                .expect("test-only gate must remain available");
                            let mut current = output.lock().unwrap();
                            if current.0 > 1 {
                                // Local artifact owner, not Riesel S2, fences
                                // the stale write from an old plan revision.
                                return OperationOutcome::Failed(
                                    "stale output from old plan revision".into(),
                                );
                            }
                            *current = (1, 111);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )?;
            registry.register(
                OperationKey::new("research.new_producer", 1)?,
                async_operation({
                    let output = Arc::clone(&output);
                    move || {
                        let output = Arc::clone(&output);
                        async move {
                            *output.lock().unwrap() = (2, 222);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )?;

            let old_plan = ExecutionPlanDraft::new("research.187.revision-race", 1)
                .operation(OperationNode::new("source", OperationKey::new("research.old_producer", 1)?))
                .validate()?
                .bind(&registry)?;
            let new_plan = ExecutionPlanDraft::new("research.187.revision-race", 2)
                .operation(OperationNode::new("source", OperationKey::new("research.new_producer", 1)?))
                .validate()?
                .bind(&registry)?;

            let old_runner = context.execution_graph(old_plan, NonZeroUsize::new(1).unwrap())?;
            let new_runner = context.execution_graph(new_plan, NonZeroUsize::new(1).unwrap())?;
            let executor = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            executor.block_on(async {
                let old_task = tokio::spawn(old_runner.execute());
                tokio::time::timeout(Duration::from_secs(3), async {
                    while !entered_old.load(Ordering::SeqCst) {
                        tokio::task::yield_now().await;
                    }
                }).await.expect("old operation not admitted");

                let new_report = new_runner.execute().await;
                assert!(new_report.succeeded());
                assert_eq!(*output.lock().unwrap(), (2, 222));

                old_release.add_permits(1);
                let old_report = tokio::time::timeout(
                    Duration::from_secs(3), old_task,
                ).await.expect("older operation did not terminate").unwrap();
                assert!(!old_report.succeeded(), "superseded write must fail");
                assert_eq!(*output.lock().unwrap(), (2, 222));
                assert_eq!(old_report.revision, 1);
                assert_eq!(new_report.revision, 2);
                assert_ne!(old_report.identity, new_report.identity);
            });
            Ok(())
        }).unwrap();
    }

    #[test]
    fn actual_riesel_graph_schedules_control_edges_but_data_is_app_owned() {
        run_current_production_graph_with_domain_owned_data().unwrap();
    }
}
