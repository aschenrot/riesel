//! Research-only #175: real Riesel-generated handles with bounded grid-shaped DTOs.
//!
//! This is NOT a noise algorithm or a cellular-automata implementation.
//! The synthetic noise and identity-CA providers isolate SDK capability composition,
//! public type support, local data conversion, and bounded value transfer.
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

#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize, RieselType)]
pub enum ProbeError {
    #[error("invalid, unbounded or mismatched grid dimensions")]
    InvalidDimensions,
    #[error("non-finite sample or threshold")]
    NonFinite,
    #[error("unsupported iteration count")]
    InvalidIterations,
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

/// Contract feasibility only: the implementation deliberately leaves input unchanged.
#[capability(id = "research.grid175.ca", version = 1)]
pub trait CellularAutomata {
    /// Verify that typed occupancy payloads can enter and leave a capability.
    #[command]
    async fn apply(&self, grid: OccupancyGridDto, iterations: u64)
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

#[component(id = "research.grid175.identity_ca", provides = [CellularAutomataContract])]
#[derive(Default)]
pub struct IdentityCa;

impl CellularAutomata for IdentityCa {
    async fn apply(
        &self,
        grid: OccupancyGridDto,
        iterations: u64,
    ) -> Result<OccupancyGridDto, ProbeError> {
        let expected = bounded_size(grid.width, grid.height)?;
        if grid.occupied.len() != expected {
            return Err(ProbeError::InvalidDimensions);
        }
        if iterations > 256 {
            return Err(ProbeError::InvalidIterations);
        }
        // Intentionally NO state transition: SDK transport/contract probe only.
        Ok(grid)
    }
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

#[riesel::app(name = "Grid175CapabilityProof", components = [SyntheticNoise, IdentityCa])]
async fn main(
    noise: NoiseGenerationHandle,
    ca: CellularAutomataHandle,
) -> AppResult<()> {
    let field = noise.generate(64, 64, 42).await?;
    let grid = threshold(field, 0.55)?;
    let actual = ca.apply(grid.clone(), 4).await?;

    assert_eq!(actual, grid);
    assert_eq!(actual.occupied.len(), 64 * 64);
    println!("Riesel #175 SDK composition proof: {}x{} cells", actual.width, actual.height);
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
}
