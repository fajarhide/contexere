//! Guards against pathological slowdowns, a hang, a deadlock, an accidental
//! O(n²). Not benchmarks: these run under `cargo test` on shared CI runners,
//! where wall-clock time measures the runner's load as much as our code.
//!
//! So every bound here is **catastrophic-only**: orders of magnitude above the
//! design target named in each test. A tight bound cannot tell "the code got
//! slower" from "the runner was busy", the since-deleted
//! `bench_handoff_export_latency` proved it, failing at 100ms on Windows and
//! passing on a rerun of the same commit, reddening an unrelated PR.
//!
//! Real performance tracking lives in `benches/pipeline.rs` (criterion,
//! `cargo bench`), which measures against a baseline instead of a magic number.

use std::time::Instant;
use veng::pipeline::scorer;

/// Upper bound for every timing assertion here. Anything this slow is broken,
/// not merely unlucky, these operations are all designed to run in ~milliseconds.
const CATASTROPHIC_MS: u128 = 2000;

#[test]
fn bench_distillation_latency() {
    let input = "a\n".repeat(100);
    let start = Instant::now();
    let segments = scorer::score_with_command(&input, "ls", None);
    veng::distillers::distill_with_command(&segments, &input, "ls", None);
    let elapsed = start.elapsed();

    // Design target: well under a second, even in an unoptimized debug build.
    assert!(
        elapsed.as_millis() < CATASTROPHIC_MS,
        "Distillation latency too high: {elapsed:?}"
    );
}

#[test]
fn bench_pressure_calculation_latency() {
    let mut state = veng::pipeline::SessionState::new();
    state.estimated_current_tokens = 150_000;

    let start = Instant::now();
    for _ in 0..100 {
        state.recalculate_pressure();
    }
    let elapsed = start.elapsed();

    // Design target: 100 in-memory recalcs in < 1ms.
    assert!(
        elapsed.as_millis() < CATASTROPHIC_MS,
        "Pressure latency too high: {elapsed:?}"
    );
}

#[test]
fn bench_environment_sanitization_latency() {
    let start = Instant::now();
    let mut malicious_env = Vec::new();
    for i in 0..1000 {
        malicious_env.push((format!("BAD_VAR_{}", i), "evil".to_string()));
    }

    let _sanitized = veng::guard::env::sanitize_vars(malicious_env);
    let elapsed = start.elapsed();

    // Design target: 1000 vars sanitized in a few ms.
    assert!(
        elapsed.as_millis() < CATASTROPHIC_MS,
        "Security scan too slow: {elapsed:?}"
    );
}
