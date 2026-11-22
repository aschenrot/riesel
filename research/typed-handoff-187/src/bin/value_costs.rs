//! Research #187: *process-local* value-movement costs for dense 2D scalar grids.
//! Same JSON format family used by current Riesel TCP; NOT a managed RPC or
//! end-to-end scheduler/provider benchmark. No public production API.
#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};
use std::hint::black_box;
use std::sync::Arc;
use std::time::Instant;

const FRAME_MAX_BYTES: usize = 4 * 1024 * 1024; // current riesel_protocol TCP wire.rs

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct ScalarGridDto {
    width: usize,
    height: usize,
    samples: Vec<f64>,
}

fn make_grid(side: usize) -> ScalarGridDto {
    ScalarGridDto {
        width: side,
        height: side,
        // Deterministic high precision sample values; NOT the Grid noise
        // algorithm, and not representative of all compressible data.
        samples: (0..side * side).map(|i| {
            let offset = (i % 1024) as f64 * 0.000000000001;
            0.12345678912345678 + offset
        }).collect(),
    }
}

fn median_ns_per_op<F: FnMut()>(mut execute: F, iterations: usize) -> u128 {
    assert!(iterations > 0);
    // Warm-up and black_box prevent eliminating the observed payload work.
    for _ in 0..3 { execute(); }
    let mut batches = [0u128; 5];
    for sample in &mut batches {
        let begin = Instant::now();
        for _ in 0..iterations { execute(); }
        *sample = begin.elapsed().as_nanos() / iterations as u128;
    }
    batches.sort_unstable();
    batches[2]
}

fn float_error(original: &[f64], decoded: &[f64]) -> (usize, u64, f64) {
    assert_eq!(original.len(), decoded.len());
    let mut differences = 0usize;
    let mut worst_ulp = 0u64;
    let mut largest_abs = 0.0f64;
    for (&a, &b) in original.iter().zip(decoded.iter()) {
        assert!(a.is_finite() && b.is_finite());
        if a.to_bits() != b.to_bits() {
            differences += 1;
            worst_ulp = worst_ulp.max(a.to_bits().abs_diff(b.to_bits()));
            largest_abs = largest_abs.max((a - b).abs());
        }
    }
    (differences, worst_ulp, largest_abs)
}

fn bench_one(side: usize, shared_loops: usize, copy_loops: usize, json_loops: usize) {
    let grid = make_grid(side);
    let element_count = side * side;
    let arc = Arc::new(grid.clone());
    let encoded = serde_json::to_vec(&grid).expect("reference serialization");
    let decoded: ScalarGridDto = serde_json::from_slice(&encoded).expect("reference decoding");
    assert_eq!((decoded.width, decoded.height), (side, side));
    let (float_mismatches, worst_ulp, max_abs_error) =
        float_error(&grid.samples, &decoded.samples);
    if cfg!(feature = "json-float-roundtrip") {
        assert_eq!(float_mismatches, 0, "exact-float feature must round-trip exactly");
    }
    let logical_data_bytes = element_count * std::mem::size_of::<f64>();
    let arc_ns = median_ns_per_op(|| {
        let cloned = black_box(Arc::clone(&arc));
        black_box(cloned.samples.len());
    }, shared_loops);
    let copy_ns = median_ns_per_op(|| {
        let copied = black_box(black_box(&grid).clone());
        black_box(copied.samples[side]);
    }, copy_loops);
    let encode_ns = median_ns_per_op(|| {
        let result = serde_json::to_vec(black_box(&grid)).expect("serialize");
        black_box(result.len());
    }, json_loops);
    let decode_ns = median_ns_per_op(|| {
        let result: ScalarGridDto = serde_json::from_slice(black_box(&encoded)).expect("deserialize");
        black_box(result.samples[side]);
    }, json_loops);
    // Report measurement, not a universal copy count. Arc cloning is only
    // zero-copy while payload is immutable and process-local.
    println!(
        "VALUE_COSTS side={side} cells={element_count} scalar_bytes={logical_data_bytes} json_bytes={} exceeds_rpc_raw_frame={} float_mismatches={} worst_ulp={} max_abs_error={:.3e} arc_clone_median_ns={} vec_deep_clone_median_ns={} json_encode_median_ns={} json_decode_median_ns={} loops_arc={} loops_clone={} loops_json={}",
        encoded.len(),
        encoded.len() + 1 > FRAME_MAX_BYTES,
        float_mismatches, worst_ulp, max_abs_error,
        arc_ns, copy_ns, encode_ns, decode_ns,
        shared_loops, copy_loops, json_loops,
    );
}

fn main() {
    println!(
        "VALUE_COSTS method=median_of_5_batches mode=release host=CI single_process_no_traffic codec=serde_json float_roundtrip={} frame_bytes={FRAME_MAX_BYTES}",
        cfg!(feature = "json-float-roundtrip"),
    );
    bench_one(64, 10_000, 1_000, 100);
    bench_one(512, 10_000, 250, 10);
    bench_one(1024, 10_000, 50, 3);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_decoded_values_and_frame_size_assertion() {
        for side in [64, 512, 1024] {
            let source = make_grid(side);
            let bytes = serde_json::to_vec(&source).unwrap();
            let restored: ScalarGridDto = serde_json::from_slice(&bytes).unwrap();
            assert_eq!((source.width, source.height), (restored.width, restored.height));
            let (mismatches, _max_ulp, max_abs) =
                float_error(&source.samples, &restored.samples);
            assert!(max_abs < 1e-12, "synthetic sample exceeded experiment tolerance");
            if cfg!(feature = "json-float-roundtrip") {
                assert_eq!(mismatches, 0, "exact feature did not roundtrip floats");
            }
            assert_eq!(source.samples.len(), side * side);
            assert_eq!(source.samples.capacity() * std::mem::size_of::<f64>() >= side*side*8, true);
            if side >= 512 {
                assert!(bytes.len() + 1 > FRAME_MAX_BYTES);
            } else {
                assert!(bytes.len() + 1 < FRAME_MAX_BYTES);
            }
        }
    }

    #[test]
    fn strict_threshold_can_flip_after_a_non_exact_json_handoff() {
        let original = make_grid(64);
        let bytes = serde_json::to_vec(&original).unwrap();
        let decoded: ScalarGridDto = serde_json::from_slice(&bytes).unwrap();

        let mut changed = 0usize;
        for (&a, &b) in original.samples.iter().zip(decoded.samples.iter()) {
            if a.to_bits() == b.to_bits() {
                continue;
            }
            // A threshold exactly between two representable neighboring
            // values, or equal to the lower one, can classify them differently.
            let threshold = a.min(b);
            assert_ne!(
                a > threshold,
                b > threshold,
                "strict threshold must expose the codec's changed value",
            );
            changed += 1;
        }
        if cfg!(feature = "json-float-roundtrip") {
            assert_eq!(changed, 0, "exact-float decode must preserve thresholds");
        }
        eprintln!(
            "threshold-sensitive f64 values after JSON handoff: {changed} of {}",
            original.samples.len()
        );
        // No claim that every ordinary threshold is unstable: the failure
        // requires the threshold to lie at a numerical decision boundary.
    }

    #[test]
    fn immutable_arc_fanout_reuses_underlying_allocation() {
        let source = Arc::new(make_grid(512));
        let first = Arc::clone(&source);
        let second = Arc::clone(&source);
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first.samples.as_ptr(), second.samples.as_ptr());
        assert_eq!(first.samples.len(), 512 * 512);
    }
}
