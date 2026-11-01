# Contexere tests

This directory contains integration tests, fixtures, and snapshot data for Contexere.

## Structure

```
tests/
├── fixtures/              # 45 realistic tool output samples
│   ├── git_diff_multi_file.txt
│   ├── cargo_build_errors.txt
│   ├── pytest_failures.txt
│   └── ...
├── savings_assertions.rs  # Per-filter savings threshold tests
├── hook_e2e.rs            # Binary spawn E2E tests
├── security_tests.rs      # Security validation tests
└── smoke_test.sh          # Shell smoke test script
```

## Running Tests

```bash
# All tests
cargo test

# Specific suites
cargo test --test hook_e2e            # E2E tests
cargo test --test savings_assertions  # savings tests
cargo test --test security_tests      # security tests

# Snapshot tests
cargo test distillers::tests
cargo insta review                    # Review changes

# Smoke tests
chmod +x tests/smoke_test.sh
tests/smoke_test.sh ./target/debug/contexere
```

## Adding a New Fixture

1. Save realistic CLI output to `tests/fixtures/my_tool_output.txt`
2. Reference it in a snapshot test or savings assertion
3. Run `cargo test` to verify

## Critical Guardrails (Avoid Common Issues)

### 1. Database Isolation in Tests
- **Issue**: Parallel integration tests competing for `~/.omni/omni.db` cause SQLite locks and hangs.
- **Rule**: Never use the default DB path in tests.
- **Solution**: Use the `omni_cmd()` helper in `tests/hook_e2e.rs` to spawn the `omni` binary with a unique `OMNI_DB_PATH` (using `tempfile::NamedTempFile`).

### 2. Mutex Locking Strategy
- **Issue**: Nested or redundant `lock()` calls on `session_arc` cause deadlocks (Rust Mutexes are not reentrant).
- **Rule**: Lock early, release fast. Do not hold a lock while calling a function that might try to acquire it again.
- **Solution**: Open a scope `{ ... }` for the lock, extract needed data, and let the guard drop before proceeding to other database or processing operations.

### 3. CI Performance
- **Warning**: If `cargo test` takes > 1 minute on MacOS or Linux, a deadlock or resource contention is likely present.
- **Action**: Check `Pipe Mode` and `E2E tests` first, as they are the most resource-heavy components.
