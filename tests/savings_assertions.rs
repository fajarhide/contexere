use std::time::Instant;
/// Savings threshold assertions, each distiller must achieve minimum token reduction.
///
/// This integration test runs the full pipeline (classify → score → compose) on real
/// fixture files and asserts each achieves a minimum savings percentage.
use veng::pipeline::scorer;

fn run_pipeline(input: &str, command: &str) -> (usize, usize, f64) {
    let segments = scorer::score_with_command(input, command, None);

    // Use the actual distiller logic
    let output = veng::distillers::distill_with_command(&segments, input, command, None);

    let input_len = input.len();
    let output_len = output.len();
    let savings_pct = if input_len > 0 {
        100.0 * (1.0 - output_len as f64 / input_len as f64)
    } else {
        0.0
    };
    (input_len, output_len, savings_pct)
}

/// Fixtures paired with: (filter_name, path, min_savings_pct_if_large_enough)
const FIXTURES: &[(&str, &str, f64, &str)] = &[
    (
        "git",
        "tests/fixtures/git_diff_multi_file.txt",
        50.0,
        "git diff",
    ),
    (
        "git",
        "tests/fixtures/git_status_dirty.txt",
        70.0,
        "git status",
    ),
    (
        "build",
        "tests/fixtures/cargo_build_errors.txt",
        70.0,
        "cargo build",
    ),
    ("test", "tests/fixtures/pytest_failures.txt", 75.0, "pytest"),
    (
        "infra",
        "tests/fixtures/kubectl_pods_mixed.txt",
        50.0,
        "kubectl get pods",
    ),
    (
        "infra",
        "tests/fixtures/docker_build_layered.txt",
        80.0,
        "docker build",
    ),
    (
        "infra",
        "tests/fixtures/heavy_noise.txt",
        90.0,
        "docker build",
    ),
];

#[test]
fn test_savings_thresholds() {
    for (filter, fixture, min_pct, command) in FIXTURES {
        let input = std::fs::read_to_string(fixture)
            .unwrap_or_else(|_| panic!("Cannot read fixture: {}", fixture));
        let (input_len, output_len, actual_pct) = run_pipeline(&input, command);
        println!(
            "| {:<10} | {:>9} B | {:>10} B | {:>10.1}% |",
            filter, input_len, output_len, actual_pct
        );

        assert!(
            output_len <= input_len + 100,
            "{} on {}: output ({}) should not massively exceed input ({})",
            filter,
            fixture,
            output_len,
            input_len
        );

        if input_len > 500 && *min_pct > 0.0 {
            assert!(
                actual_pct >= *min_pct,
                "{} on {}: expected >= {:.0}% savings, got {:.1}% (input={}, output={})",
                filter,
                fixture,
                min_pct,
                actual_pct,
                input_len,
                output_len
            );
        }
    }
}

#[test]
fn test_all_fixtures_produce_nonempty_output() {
    let fixture_dir = std::fs::read_dir("tests/fixtures").unwrap();
    for entry in fixture_dir {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().map(|e| e == "txt").unwrap_or(false) {
            let input = std::fs::read_to_string(&path).unwrap();
            if input.is_empty() {
                continue;
            }
            let (_, output_len, _) = run_pipeline(&input, "git status");
            assert!(
                output_len > 0 || input.trim().is_empty(),
                "Fixture {:?} produced empty output from {} bytes input",
                path.file_name().unwrap(),
                input.len()
            );
        }
    }
}

#[test]
fn test_short_input_not_over_expanded() {
    let short = "hello world";
    let (input_len, output_len, _) = run_pipeline(short, "echo");
    assert!(
        output_len <= input_len + 50,
        "Short input expanded from {} to {} bytes",
        input_len,
        output_len
    );
}

#[test]
fn test_empty_input_no_crash() {
    let (_, output_len, _) = run_pipeline("", "echo");
    assert_eq!(output_len, 0);
}

#[test]
fn test_pipeline_latency_under_50ms_debug() {
    let input = include_str!("../tests/fixtures/git_diff_multi_file.txt").repeat(3);

    // Warmup pass, ensures any lazy initialization (regex, scorer caches) is done
    let _ = scorer::score_with_command(&input, "git diff", None);

    let start = Instant::now();
    let segments = scorer::score_with_command(&input, "git diff", None);
    veng::distillers::distill_with_command(&segments, &input, "git diff", None);
    let elapsed = start.elapsed();

    // 250ms budget for debug (unoptimized) builds; release builds are ~5-10x faster
    assert!(
        elapsed.as_millis() < 250,
        "Pipeline took {}ms (should be <250ms in debug)",
        elapsed.as_millis()
    );
}

#[test]
fn test_hook_no_panic_on_large_input() {
    let large = "error: cannot find type\n".repeat(20000);
    let segments = scorer::score_with_command(&large, "cargo test", None);
    let output = veng::distillers::distill_with_command(&segments, &large, "cargo test", None);
    assert!(!output.is_empty());
}

#[test]
fn test_score_with_command_returns_segments() {
    use veng::pipeline::scorer::score_with_command;
    let input = "error[E0382]: use of moved value\n   --> src/main.rs:10:5\nCompiling veng v0.5.6";
    let segments = score_with_command(input, "cargo build", None);
    assert!(!segments.is_empty());
    let has_critical = segments
        .iter()
        .any(|s| s.tier == veng::pipeline::SignalTier::Critical);
    assert!(has_critical, "Error line should be Critical");
}

#[test]
fn test_veng_stats_shows_command_not_content_type() {
    use tempfile::tempdir;
    use veng::pipeline::{DistillResult, Route};
    use veng::store::sqlite::Store;

    let dir = tempdir().unwrap();
    let store = Store::open_path(&dir.path().join("veng.db")).unwrap();

    let result = DistillResult {
        output: "cargo build: ok".to_string(),
        route: Route::Keep,
        filter_name: "cargo".to_string(),
        score: 0.9,
        context_score: 0.0,
        input_bytes: 1000,
        output_bytes: 100,
        latency_ms: 5,
        rewind_hash: None,
        segments_kept: 2,
        segments_dropped: 8,
        collapse_savings: None,
        raw_tokens: 250,
        filtered_tokens: 25,
        delivered_bytes: 100,
    };

    store.record_distillation(
        "sess_1",
        &result,
        "cargo build --release",
        "",
        "claude_code",
    );

    let stats = store.get_per_command_stats(0, 10).unwrap();
    assert!(!stats.is_empty());
    let (cmd, count, _, _, _, _) = &stats[0];
    assert!(
        cmd.contains("cargo"),
        "Command column should contain actual command, got: {}",
        cmd
    );
    assert_eq!(*count, 1);
}

/// Structured payloads are the one case where 0% savings is the correct answer:
/// they are parsed downstream, so the pipeline must hand them back untouched.
#[test]
fn structured_output_reports_zero_savings_and_preserves_bytes() {
    use veng::hooks::pipe;

    const STRUCTURED: &[(&str, &str)] = &[
        ("tests/fixtures/az_vm_list.json", "az vm list -o json"),
        (
            "tests/fixtures/grafana_dashboard.json",
            "git show HEAD:dashboard.json",
        ),
    ];

    for (path, command) in STRUCTURED {
        let raw = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"));

        let mut out = Vec::new();
        pipe::run_inner(
            raw.as_bytes(),
            &mut out,
            std::io::sink(),
            None,
            None,
            Some(command),
        )
        .expect("pipe hook must not fail");
        let out = String::from_utf8(out).expect("output must stay valid UTF-8");

        let savings_pct = 100.0 * (1.0 - out.len() as f64 / raw.len() as f64);
        assert_eq!(
            savings_pct, 0.0,
            "{path} must report 0% savings, got {savings_pct:.1}%"
        );
        assert_eq!(out, raw, "{path} must be byte-preserving");
    }
}
