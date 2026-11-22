//! Research-only #175: real Riesel-generated handles with bounded grid-shaped DTOs.
//!
//! The noise provider remains synthetic; the independent test-local cellular
//! automata implementation is an actual synchronous rules probe, not the
//! production domain library. This probes SDK composition and bounded DTOs.
//! Domain correctness must be verified against the independently owned grid crates.
#![forbid(unsafe_code)]

use riesel::{AppResult, RieselType, capability, component};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const MAX_CELLS: u64 = 512 * 512;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, RieselType)]
pub struct ScalarGridDto {
    width: u64,
    height: u64,
    samples: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RieselType)]
pub struct OccupancyGridDto {
    width: u64,
    height: u64,
    occupied: Vec<bool>,
}

/// Research-only public rule DTO; no public CA contract has been accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RieselType)]
pub struct CaRuleDto {
    birth_min: u64,
    survive_min: u64,
    protected_border: u64,
    outside_occupied: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize, RieselType)]
pub enum ProbeError {
    #[error("invalid, unbounded or mismatched grid dimensions")]
    InvalidDimensions,
    #[error("non-finite sample or threshold")]
    NonFinite,
    #[error("unsupported iteration count")]
    InvalidIterations,
    #[error("invalid cellular automata rule")]
    InvalidRule,
    #[error("transport error: {0}")]
    Transport(String),
}

impl From<riesel_protocol::RpcError> for ProbeError {
    fn from(value: riesel_protocol::RpcError) -> Self {
        Self::Transport(value.to_string())
    }
}

/// Contract feasibility only: no actual procedural-noise guarantee.
#[capability(id = "research.grid175.noise", version = 1)]
pub trait NoiseGeneration {
    /// Return a bounded, synthetic scalar-grid payload for contract verification.
    #[query]
    async fn generate(&self, width: u64, height: u64, seed: u64)
        -> Result<ScalarGridDto, ProbeError>;
}

/// Contract feasibility: typed per-operation rules and bounded grid materialization.
#[capability(id = "research.grid175.ca", version = 1)]
pub trait CellularAutomata {
    /// Verify that typed occupancy payloads can enter and leave a capability.
    #[command]
    async fn apply(&self, grid: OccupancyGridDto, iterations: u64, rule: CaRuleDto)
        -> Result<OccupancyGridDto, ProbeError>;
}

#[component(id = "research.grid175.synthetic_noise", provides = [NoiseGenerationContract])]
#[derive(Default)]
pub struct SyntheticNoise;

impl NoiseGeneration for SyntheticNoise {
    async fn generate(
        &self,
        width: u64,
        height: u64,
        seed: u64,
    ) -> Result<ScalarGridDto, ProbeError> {
        let size = bounded_size(width, height)?;
        let samples = (0..size)
            .map(|i| {
                let x = (i as u64) % width;
                let y = (i as u64) / width;
                // Simple deterministic test pattern, NOT procedural noise.
                seed.wrapping_add(x.wrapping_mul(17))
                    .wrapping_add(y.wrapping_mul(31))
                    .wrapping_rem(100) as f64
                    / 100.0
            })
            .collect();
        Ok(ScalarGridDto { width, height, samples })
    }
}

#[component(id = "research.grid175.rule_ca", provides = [CellularAutomataContract])]
#[derive(Default)]
pub struct RuleCa;

impl CellularAutomata for RuleCa {
    async fn apply(
        &self,
        grid: OccupancyGridDto,
        iterations: u64,
        rule: CaRuleDto,
    ) -> Result<OccupancyGridDto, ProbeError> {
        apply_ca(grid, iterations, &rule)
    }
}

/// Domain-independent rule reference, deliberately limited to binary 8-neighbor
/// synchronous updates. This does not replace the separately owned grid crate.
fn apply_ca(
    mut grid: OccupancyGridDto,
    iterations: u64,
    rule: &CaRuleDto,
) -> Result<OccupancyGridDto, ProbeError> {
    let len = bounded_size(grid.width, grid.height)?;
    if grid.occupied.len() != len {
        return Err(ProbeError::InvalidDimensions);
    }
    if rule.birth_min > 8 || rule.survive_min > 8 {
        return Err(ProbeError::InvalidRule);
    }
    if iterations > 64 {
        return Err(ProbeError::InvalidIterations);
    }
    let width = usize::try_from(grid.width).map_err(|_| ProbeError::InvalidDimensions)?;
    let height = usize::try_from(grid.height).map_err(|_| ProbeError::InvalidDimensions)?;
    let border = usize::try_from(rule.protected_border).unwrap_or(usize::MAX);

    for _ in 0..iterations {
        // Read only the *previous* iteration; no in-place propagation.
        let previous = &grid.occupied;
        let mut next = previous.clone();
        for y in 0..height {
            for x in 0..width {
                if x < border || y < border ||
                    x >= width.saturating_sub(border) ||
                    y >= height.saturating_sub(border) {
                    continue; // Preserve original value, not forced walls.
                }
                let mut neighbors = 0u64;
                for dy in [-1i64, 0, 1] {
                    for dx in [-1i64, 0, 1] {
                        if dx == 0 && dy == 0 { continue; }
                        let nx = x as i64 + dx;
                        let ny = y as i64 + dy;
                        let filled = if nx < 0 || ny < 0 ||
                            nx >= width as i64 || ny >= height as i64 {
                            rule.outside_occupied
                        } else {
                            previous[ny as usize * width + nx as usize]
                        };
                        neighbors += u64::from(filled);
                    }
                }
                let index = y * width + x;
                let required = if previous[index] { rule.survive_min } else { rule.birth_min };
                next[index] = neighbors >= required;
            }
        }
        grid.occupied = next;
    }
    Ok(grid)
}

fn bounded_size(width: u64, height: u64) -> Result<usize, ProbeError> {
    let count = width.checked_mul(height).ok_or(ProbeError::InvalidDimensions)?;
    if width == 0 || height == 0 || count > MAX_CELLS {
        return Err(ProbeError::InvalidDimensions);
    }
    usize::try_from(count).map_err(|_| ProbeError::InvalidDimensions)
}

/// An ordinary typed Rust conversion; no extra Riesel component is created.
fn threshold(input: ScalarGridDto, value: f64) -> Result<OccupancyGridDto, ProbeError> {
    if !value.is_finite() || input.samples.iter().any(|sample| !sample.is_finite()) {
        return Err(ProbeError::NonFinite);
    }
    if input.samples.len() != bounded_size(input.width, input.height)? {
        return Err(ProbeError::InvalidDimensions);
    }
    Ok(OccupancyGridDto {
        width: input.width,
        height: input.height,
        occupied: input.samples.into_iter().map(|sample| sample > value).collect(),
    })
}

#[riesel::app(name = "Grid175CapabilityProof", components = [SyntheticNoise, RuleCa])]
async fn main(
    noise: NoiseGenerationHandle,
    ca: CellularAutomataHandle,
) -> AppResult<()> {
    let field = noise.generate(64, 64, 42).await?;
    let grid = threshold(field, 0.55)?;
    let rule = CaRuleDto {
        birth_min: 5, survive_min: 5, protected_border: 1, outside_occupied: true,
    };
    let actual = ca.apply(grid.clone(), 4, rule).await?;

    assert_eq!(actual.occupied.len(), 64 * 64);
    for y in 0..64usize {
        for x in 0..64usize {
            if x == 0 || y == 0 || x == 63 || y == 63 {
                let idx = y * 64 + x;
                assert_eq!(actual.occupied[idx], grid.occupied[idx]);
            }
        }
    }
    println!("Riesel #175 SDK composition + rule CA proof: {}x{} cells", actual.width, actual.height);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_is_strict_and_preserves_shape() {
        let result = threshold(
            ScalarGridDto { width: 2, height: 2, samples: vec![0.0, 0.55, 0.56, 1.0] },
            0.55,
        )
        .unwrap();
        assert_eq!(result.occupied, vec![false, false, true, true]);
        assert_eq!((result.width, result.height), (2, 2));
    }

    #[test]
    fn invalid_dimensions_and_nonfinite_values_fail_explicitly() {
        assert_eq!(bounded_size(0, 64), Err(ProbeError::InvalidDimensions));
        assert_eq!(bounded_size(u64::MAX, 64), Err(ProbeError::InvalidDimensions));
        let invalid = ScalarGridDto { width: 1, height: 1, samples: vec![f64::NAN] };
        assert_eq!(threshold(invalid, 0.5), Err(ProbeError::NonFinite));
    }
    #[test]
    fn border_preservation_is_per_iteration_and_keeps_mixed_values() {
        let width = 8usize;
        let grid = OccupancyGridDto {
            width: 8, height: 8,
            occupied: (0..64usize).map(|i| (i % width + i / width) % 2 == 0).collect(),
        };
        let rule = CaRuleDto {
            birth_min: 5, survive_min: 5, protected_border: 1, outside_occupied: true,
        };
        let mut current = grid.clone();
        for _ in 0..4 {
            current = apply_ca(current, 1, &rule).unwrap();
            for y in 0..8usize {
                for x in 0..8usize {
                    if x == 0 || y == 0 || x == 7 || y == 7 {
                        assert_eq!(current.occupied[y * 8 + x], grid.occupied[y * 8 + x]);
                    }
                }
            }
        }
        assert_ne!(current, grid); // The test must exercise actual state changes.
    }

    #[test]
    fn outside_read_policy_is_not_border_write_policy() {
        let input = OccupancyGridDto { width: 1, height: 1, occupied: vec![false] };
        let rule = CaRuleDto {
            birth_min: 8, survive_min: 8, protected_border: 0, outside_occupied: false,
        };
        assert_eq!(apply_ca(input.clone(), 1, &rule).unwrap().occupied, vec![false]);
        let filled_outside = CaRuleDto { outside_occupied: true, ..rule };
        assert_eq!(apply_ca(input, 1, &filled_outside).unwrap().occupied, vec![true]);
    }

    #[test]
    fn synchronous_updates_do_not_propagate_two_cells_in_one_iteration() {
        let mut grid = OccupancyGridDto {
            width: 5, height: 5, occupied: vec![false; 25],
        };
        grid.occupied[12] = true;
        let rule = CaRuleDto {
            birth_min: 1, survive_min: 1, protected_border: 0, outside_occupied: false,
        };
        let once = apply_ca(grid, 1, &rule).unwrap();
        assert_eq!(once.occupied[13], true); // Adjacent cell sees old central wall.
        assert_eq!(once.occupied[14], false); // Two steps away must remain empty.
    }

    #[test]
    fn invalid_rules_lengths_and_iterations_fail_closed() {
        let grid = OccupancyGridDto {
            width: 2, height: 2, occupied: vec![false; 4],
        };
        let rule = CaRuleDto {
            birth_min: 9, survive_min: 5, protected_border: 0, outside_occupied: false,
        };
        assert_eq!(apply_ca(grid.clone(), 1, &rule), Err(ProbeError::InvalidRule));
        let valid = CaRuleDto { birth_min: 5, ..rule };
        assert_eq!(apply_ca(grid, 65, &valid), Err(ProbeError::InvalidIterations));
        let malformed = OccupancyGridDto { width: 2, height: 2, occupied: vec![false] };
        assert_eq!(apply_ca(malformed, 1, &valid), Err(ProbeError::InvalidDimensions));
    }

    #[test]
    fn serialized_scalar_payload_growth_is_visible() {
        for (width, height) in [(64u64, 64u64), (512, 512)] {
            let field = ScalarGridDto {
                width, height, samples: vec![0.12345678912345678; (width * height) as usize],
            };
            let bytes = serde_json::to_vec(&field).unwrap();
            eprintln!("scalar JSON payload {width}x{height}: {} bytes", bytes.len());
            assert!(bytes.len() >= (width * height) as usize * 8);
            if width == 512 {
                // Baseline Riesel TCP uses a 4 MiB JSON frame cap. This
                // particular valid full-precision scalar grid exceeds it.
                assert!(bytes.len() > 4 * 1024 * 1024);
            }
        }
    }

}
