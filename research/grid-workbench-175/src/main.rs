//! Headless typed Grid Workbench, with no renderer, window, Manager or network.
#![forbid(unsafe_code)]

use grid_generation::bounded::{
    CaNeighborhood2D, CaOutsideRead, CaRules2D, NoiseSample2D, ThresholdComparison,
    cellular_automata_step_2d, sample_noise_2d, threshold_2d,
};
use grid_math::{GridCoord2, GridSize2};
use grid_storage::DenseGrid2D;
use riesel::{
    Application, ExecutionPlanDraft, LocalPort, OperationKey, OperationNode, TypedEdge,
    TypedExecutionPlan, TypedOperationOutcome, TypedOperationRegistry, TypedOperationSpec,
    TypedOutputs, typed_operation,
};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, PoisonError};
use tile_topology::{VisualTile, form_visual_tiles};

const MAX_DIMENSION: usize = 512;
const MAX_ITERATIONS: usize = 16;
const MAX_RUN_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Config {
    width: usize,
    height: usize,
    seed: u32,
    frequency: f32,
    threshold: f32,
    border: usize,
    iterations: usize,
    revision: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            width: 64,
            height: 64,
            seed: 42,
            frequency: 0.09,
            threshold: 0.55,
            border: 1,
            iterations: 5,
            revision: 1,
        }
    }
}

impl Config {
    fn validate(self) -> Result<Self, String> {
        if self.width == 0
            || self.height == 0
            || self.width > MAX_DIMENSION
            || self.height > MAX_DIMENSION
        {
            return Err(format!("width and height must be in 1..={MAX_DIMENSION}"));
        }
        if self.iterations > MAX_ITERATIONS {
            return Err(format!("iterations must be <= {MAX_ITERATIONS}"));
        }
        if self.border > self.width || self.border > self.height {
            return Err("border cannot be thicker than the grid's dimensions".into());
        }
        if !self.frequency.is_finite() || self.frequency <= 0.0 {
            return Err("frequency must be positive and finite".into());
        }
        if !self.threshold.is_finite() || !(0.0..=1.0).contains(&self.threshold) {
            return Err("threshold must be finite and in 0..=1".into());
        }
        if self.revision == 0 {
            return Err("revision must be nonzero".into());
        }
        Ok(self)
    }

    fn parse_args() -> Result<Self, String> {
        let mut config = Self::default();
        let mut args = std::env::args().skip(1);
        while let Some(option) = args.next() {
            let value = args
                .next()
                .ok_or_else(|| format!("{option} requires a value"))?;
            match option.as_str() {
                "--width" => config.width = value.parse().map_err(|_| "invalid width")?,
                "--height" => config.height = value.parse().map_err(|_| "invalid height")?,
                "--seed" => config.seed = value.parse().map_err(|_| "invalid seed")?,
                "--frequency" => {
                    config.frequency = value.parse().map_err(|_| "invalid frequency")?
                }
                "--threshold" => {
                    config.threshold = value.parse().map_err(|_| "invalid threshold")?
                }
                "--border" => config.border = value.parse().map_err(|_| "invalid border")?,
                "--iterations" => {
                    config.iterations = value.parse().map_err(|_| "invalid iterations")?
                }
                "--revision" => config.revision = value.parse().map_err(|_| "invalid revision")?,
                _ => return Err(format!("unknown option {option}")),
            }
        }
        config.validate()
    }

    fn size(self) -> GridSize2 {
        GridSize2::new(self.width, self.height)
    }

    fn cells(self) -> usize {
        self.width * self.height // validated <= 512×512
    }

    fn scalar_bytes(self) -> usize {
        self.cells() * std::mem::size_of::<f32>()
    }

    fn occupancy_bytes(self) -> usize {
        self.cells() * std::mem::size_of::<bool>()
    }

    fn history_bytes(self) -> usize {
        self.occupancy_bytes() * (self.iterations + 1)
    }

    fn topology_bytes(self) -> usize {
        (self.width + 1) * (self.height + 1) * std::mem::size_of::<VisualTile>()
    }
}

/// The CA operation owns its internal iterations and immutable snapshots.
#[derive(Debug)]
struct CaHistory {
    steps: Vec<DenseGrid2D<bool>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ScalarStats {
    min: f32,
    max: f32,
    mean: f64,
}

struct WorkbenchResult {
    revision: u64,
    scalar: Arc<DenseGrid2D<f32>>,
    occupancy: Arc<DenseGrid2D<bool>>,
    history: Arc<CaHistory>,
    topology: Arc<DenseGrid2D<VisualTile>>,
    stats: Arc<ScalarStats>,
    run_succeeded: bool,
    started: usize,
}

fn key(name: &str) -> OperationKey {
    OperationKey::new(name, 1).expect("fixed application operation key")
}

fn register_nodes(config: Config) -> Result<TypedOperationRegistry, Box<dyn std::error::Error>> {
    let mut registry = TypedOperationRegistry::new();
    registry.register(
        TypedOperationSpec {
            binding: key("noise"),
            inputs: vec![],
            outputs: vec![LocalPort::of::<DenseGrid2D<f32>>(
                "scalar",
                "grid.scalar.f32",
                1,
                config.scalar_bytes(),
            )],
        },
        typed_operation(move |_| async move {
            match sample_noise_2d(
                config.size(),
                NoiseSample2D::legacy(config.seed, config.frequency, GridCoord2::ZERO),
                config.scalar_bytes(),
            ) {
                Ok(grid) => {
                    let mut outputs = TypedOutputs::new();
                    if let Err(error) = outputs.insert("scalar", grid, config.scalar_bytes()) {
                        return TypedOperationOutcome::Failed(error);
                    }
                    TypedOperationOutcome::Completed(outputs)
                }
                Err(error) => TypedOperationOutcome::Failed(format!("Noise2D: {error:?}")),
            }
        }),
    )?;

    registry.register(
        TypedOperationSpec {
            binding: key("threshold"),
            inputs: vec![LocalPort::of::<DenseGrid2D<f32>>(
                "scalar",
                "grid.scalar.f32",
                1,
                config.scalar_bytes(),
            )],
            outputs: vec![LocalPort::of::<DenseGrid2D<bool>>(
                "occupancy",
                "grid.occupancy.bool",
                1,
                config.occupancy_bytes(),
            )],
        },
        typed_operation(move |input| async move {
            let Some(scalar) = input.get::<DenseGrid2D<f32>>("scalar") else {
                return TypedOperationOutcome::Failed("Threshold missing scalar input".into());
            };
            match threshold_2d(
                &scalar,
                config.threshold,
                ThresholdComparison::GreaterThan,
                config.occupancy_bytes(),
            ) {
                Ok(grid) => {
                    let mut outputs = TypedOutputs::new();
                    if let Err(error) = outputs.insert("occupancy", grid, config.occupancy_bytes()) {
                        return TypedOperationOutcome::Failed(error);
                    }
                    TypedOperationOutcome::Completed(outputs)
                }
                Err(error) => TypedOperationOutcome::Failed(format!("Threshold: {error:?}")),
            }
        }),
    )?;

    registry.register(
        TypedOperationSpec {
            binding: key("ca"),
            inputs: vec![LocalPort::of::<DenseGrid2D<bool>>(
                "occupancy",
                "grid.occupancy.bool",
                1,
                config.occupancy_bytes(),
            )],
            outputs: vec![LocalPort::of::<CaHistory>(
                "history",
                "grid.ca.history.bool",
                1,
                config.history_bytes(),
            )],
        },
        typed_operation(move |input| async move {
            let Some(occupancy) = input.get::<DenseGrid2D<bool>>("occupancy") else {
                return TypedOperationOutcome::Failed("CA missing occupancy input".into());
            };
            // Domain-owned optional protected write mask: an interior stripe.
            let protected = DenseGrid2D::try_from_fn(config.size(), |coord| {
                coord.x == (config.width / 3) as i32
                    && coord.y >= 1
                    && (coord.y as usize) < config.height.saturating_sub(1)
            });
            let Ok(protected) = protected else {
                return TypedOperationOutcome::Failed("CA cannot allocate write mask".into());
            };
            let rule = CaRules2D {
                neighborhood: CaNeighborhood2D::Moore8,
                birth_at_least: 5,
                survive_at_least: 4,
                outside_read: CaOutsideRead::Occupied,
                protected_border: config.border,
            };
            let mut snapshots = Vec::with_capacity(config.iterations + 1);
            snapshots.push((*occupancy).clone());
            for _ in 0..config.iterations {
                let previous = snapshots.last().expect("initial grid");
                match cellular_automata_step_2d(
                    previous,
                    rule,
                    Some(&protected),
                    config.occupancy_bytes(),
                ) {
                    Ok(grid) => snapshots.push(grid),
                    Err(error) => {
                        return TypedOperationOutcome::Failed(format!("CA: {error:?}"));
                    }
                }
            }
            let mut outputs = TypedOutputs::new();
            if let Err(error) = outputs.insert(
                "history",
                CaHistory { steps: snapshots },
                config.history_bytes(),
            ) {
                return TypedOperationOutcome::Failed(error);
            }
            TypedOperationOutcome::Completed(outputs)
        }),
    )?;

    registry.register(
        TypedOperationSpec {
            binding: key("dual"),
            inputs: vec![LocalPort::of::<CaHistory>(
                "history",
                "grid.ca.history.bool",
                1,
                config.history_bytes(),
            )],
            outputs: vec![LocalPort::of::<DenseGrid2D<VisualTile>>(
                "topology",
                "grid.dual.visual-tile",
                1,
                config.topology_bytes(),
            )],
        },
        typed_operation(move |input| async move {
            let Some(history) = input.get::<CaHistory>("history") else {
                return TypedOperationOutcome::Failed("DualGrid missing CA history".into());
            };
            let grid = history.steps.last().expect("CA produces initial snapshot");
            // The upstream Grid topology module remains the only owner of
            // the 16 mask cases, corner sampling order and rotation rules.
            let topology = form_visual_tiles(grid);
            let mut outputs = TypedOutputs::new();
            if let Err(error) = outputs.insert("topology", topology, config.topology_bytes()) {
                return TypedOperationOutcome::Failed(error);
            }
            TypedOperationOutcome::Completed(outputs)
        }),
    )?;

    registry.register(
        TypedOperationSpec {
            binding: key("statistics"),
            inputs: vec![LocalPort::of::<DenseGrid2D<f32>>(
                "scalar",
                "grid.scalar.f32",
                1,
                config.scalar_bytes(),
            )],
            outputs: vec![LocalPort::of::<ScalarStats>(
                "stats",
                "grid.scalar.stats",
                1,
                std::mem::size_of::<ScalarStats>(),
            )],
        },
        typed_operation(move |input| async move {
            let Some(scalar) = input.get::<DenseGrid2D<f32>>("scalar") else {
                return TypedOperationOutcome::Failed("Statistics missing scalar input".into());
            };
            let (mut min, mut max, mut sum) = (f32::INFINITY, f32::NEG_INFINITY, 0.0_f64);
            for sample in scalar.cells() {
                min = min.min(*sample);
                max = max.max(*sample);
                sum += f64::from(*sample);
            }
            let stats = ScalarStats {
                min,
                max,
                mean: sum / scalar.len() as f64,
            };
            let mut outputs = TypedOutputs::new();
            if let Err(error) = outputs.insert(
                "stats",
                stats,
                std::mem::size_of::<ScalarStats>(),
            ) {
                return TypedOperationOutcome::Failed(error);
            }
            TypedOperationOutcome::Completed(outputs)
        }),
    )?;
    Ok(registry)
}

fn run_workbench(config: Config) -> Result<WorkbenchResult, Box<dyn std::error::Error>> {
    let config = config.validate()?;
    let result = Arc::new(Mutex::new(None));
    let output_for_app = Arc::clone(&result);
    Application::new("Headless Grid Workbench").run(|context| {
        let registry = register_nodes(config)?;
        // Deliberately reverse-declared graph to exercise actual validation
        // of data-flow ordering and concurrent independent consumers.
        let validated = ExecutionPlanDraft::new("grid.workbench", config.revision)
            .operation(OperationNode::new("dual", key("dual")))
            .operation(OperationNode::new("statistics", key("statistics")))
            .operation(OperationNode::new("ca", key("ca")))
            .operation(OperationNode::new("threshold", key("threshold")))
            .operation(OperationNode::new("noise", key("noise")))
            .validate()?;
        let graph = TypedExecutionPlan::new(validated)
            .connect(TypedEdge::new("noise", "scalar", "threshold", "scalar"))
            .connect(TypedEdge::new("noise", "scalar", "statistics", "scalar"))
            .connect(TypedEdge::new("threshold", "occupancy", "ca", "occupancy"))
            .connect(TypedEdge::new("ca", "history", "dual", "history"));
        let prepared = graph.prepare(&registry, MAX_RUN_BYTES)?;
        let runner = context.typed_execution_graph(
            prepared,
            NonZeroUsize::new(2).expect("nonzero"),
        )?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let completed = runtime.block_on(runner.execute());
        if !completed.report.succeeded() {
            return Err(std::io::Error::other(format!(
                "graph failed: {:?}",
                completed.report.operations
            ))
            .into());
        }
        let get = |node: &str, port: &str| {
            std::io::Error::other(format!("missing output {node}.{port}"))
        };
        let summary = WorkbenchResult {
            revision: completed.report.revision,
            scalar: completed.get("noise", "scalar").ok_or_else(|| get("noise", "scalar"))?,
            occupancy: completed
                .get("threshold", "occupancy")
                .ok_or_else(|| get("threshold", "occupancy"))?,
            history: completed
                .get("ca", "history")
                .ok_or_else(|| get("ca", "history"))?,
            topology: completed
                .get("dual", "topology")
                .ok_or_else(|| get("dual", "topology"))?,
            stats: completed
                .get("statistics", "stats")
                .ok_or_else(|| get("statistics", "stats"))?,
            run_succeeded: completed.report.succeeded(),
            started: completed.report.started,
        };
        *output_for_app
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(summary);
        Ok(())
    })?;
    let mut guard = result.lock().unwrap_or_else(PoisonError::into_inner);
    guard
        .take()
        .ok_or_else(|| std::io::Error::other("Riesel did not publish a headless workbench result").into())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::parse_args()?;
    let result = run_workbench(config)?;
    let last = result.history.steps.last().expect("initial snapshot");
    let walls = last.cells().iter().filter(|cell| **cell).count();
    println!(
        "grid.workbench revision={} size={}x{} seed={} nodes={} snapshots={} occupancy={} mean={:.6} dual={}x{} succeeded={}",
        result.revision,
        config.width,
        config.height,
        config.seed,
        result.started,
        result.history.steps.len(),
        walls,
        result.stats.mean,
        result.topology.width(),
        result.topology.height(),
        result.run_succeeded
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reproduces_real_64x64_pipeline_with_snapshots_and_branching() {
        let config = Config::default();
        let a = run_workbench(config).unwrap();
        let b = run_workbench(config).unwrap();
        assert!(a.run_succeeded);
        assert_eq!(a.started, 5);
        assert_eq!(a.revision, 1);
        assert_eq!(a.scalar.size(), GridSize2::new(64, 64));
        assert_eq!(a.history.steps.len(), 6);
        assert_eq!(a.topology.size(), GridSize2::new(65, 65));
        assert_eq!(a.scalar.cells(), b.scalar.cells());
        assert_eq!(a.occupancy.cells(), b.occupancy.cells());
        assert_eq!(a.topology.cells(), b.topology.cells());
        assert_eq!(*a.stats, *b.stats);
        for snapshot in &a.history.steps {
            for (coord, value) in snapshot.iter_cells() {
                let border = coord.x == 0 || coord.y == 0 || coord.x == 63 || coord.y == 63;
                let stripe = coord.x == (config.width / 3) as i32
                    && coord.y > 0
                    && coord.y < 63;
                if border || stripe {
                    assert_eq!(Some(value), a.occupancy.get(coord));
                }
            }
        }
    }

    #[test]
    fn zero_iterations_and_new_revision_leave_old_run_result_unchanged() {
        let previous = run_workbench(Config::default()).unwrap();
        let revised = run_workbench(Config {
            revision: 2,
            iterations: 0,
            threshold: 0.70,
            ..Config::default()
        })
        .unwrap();
        assert_eq!(previous.revision, 1);
        assert_eq!(previous.history.steps.len(), 6);
        assert_eq!(revised.revision, 2);
        assert_eq!(revised.history.steps.len(), 1);
        assert_ne!(previous.occupancy.cells(), revised.occupancy.cells());
        assert_eq!(previous.topology.size(), revised.topology.size());
    }

    #[test]
    fn editable_parameters_fail_closed_on_invalid_resource_requests() {
        assert!(Config {
            width: usize::MAX,
            ..Config::default()
        }
        .validate()
        .is_err());
        assert!(Config {
            frequency: f32::NAN,
            ..Config::default()
        }
        .validate()
        .is_err());
        assert!(Config {
            iterations: MAX_ITERATIONS + 1,
            ..Config::default()
        }
        .validate()
        .is_err());
    }
}
