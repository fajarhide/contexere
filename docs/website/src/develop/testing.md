# Testing

```sh
OMNI_DB_PATH=/tmp/omni-test.db cargo test
```

Start with that line. It is not a suggestion.

## The two guardrails that waste the most time

**Isolate the database.** Parallel integration tests competing for `~/.omni/omni.db`
cause SQLite locks and hangs. Measured: 79 seconds green against an isolated database,
433 seconds and then a hang against the live one. `tests/hook_e2e.rs` has an
`omni_cmd()` helper that spawns the binary with a unique `OMNI_DB_PATH` from a
`NamedTempFile`. Use it.

**Lock early, release fast.** Rust mutexes are not reentrant, so nested or redundant
`lock()` calls on `session_arc` deadlock. Open a scope, take what you need, let the
guard drop before doing anything that might lock again.

If `cargo test` runs over a minute on macOS or Linux, suspect one of those two. Check
pipe mode and the E2E tests first; they are the heaviest.

## Suites

```sh
cargo test                              # everything
cargo test --test hook_e2e              # binary spawn, end to end
cargo test --test savings_assertions    # per-filter savings thresholds
cargo test --test security_tests
cargo test distillers::tests            # snapshots
cargo insta review                      # approve snapshot changes
tests/smoke_test.sh ./target/debug/omni
```

`tests/fixtures/` holds 45 realistic tool outputs. Add real output from the real tool,
not something shaped to be easy to parse.

## Naming

Inside `#[cfg(test)]`, drop the `test_` prefix. The attribute already says it is a
test.

```rust
fn returns_default_when_config_missing()
fn excludes_sensitive_data_from_summary()
fn preserves_errors_during_collapse()
fn renders_identical_bytes_for_identical_state()
```

Not `test_config_ok`, not `handles_it`, not `valid_json`. Start with a verb, say what
the behaviour is, English only.

## Design

One behavioural assertion per test. Arrange, act, assert, with the sections visible.
Test observable behaviour, not internals: `assert_eq!(result.status, Status::Ready)`
rather than `assert!(internal_cache.len() > 0)`.

Every non-trivial feature carries a happy path, an edge case, a malformed input case,
a regression case if it is a fix, and an explicit no-panic case. Malformed input must
return `Err`, never panic.

## Prove the test can fail

Break the rule deliberately, watch it go red, restore it.

This repo has shipped two regression tests that could not fail. Both looked correct.
Both passed with the fix reverted.

Three specific ways a green test here means nothing:

**Your fixture reaches a different code path than you think.** Collapse mode is picked
by specificity: a `kubectl … | grep` fixture exercises Infra, not Log, so a
collapse-guard test passes with the guard removed.

**"No rewrite from the hook" is not proof the distiller punted.** It can equally mean
the format gate fired or the guardrail rejected the output.

**A distiller can return a near-copy rather than the exact input.** Detect "this did
not help" with `beats_guardrail`, not `output == input`.

## Proving a refactor changed nothing

For behaviour-preserving work, diff distiller output over the whole recorded corpus,
about 5,100 commands times 11 probes, then **break one arm deliberately** to show the
harness has teeth. A differential harness that cannot detect a planted difference is
not evidence.

## Gates

```sh
make ci      # fmt + clippy + test + security + binary-check
```

Or individually:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
```

Zero clippy warnings. Not "few".

`source "$HOME/.cargo/env"` first, so you get the pinned 1.99.1 rather than a Homebrew
cargo that ignores `rust-toolchain.toml` and will keep drifting from CI.

## Never weaken a check to make it pass

Not an assertion, not a security check, not a threshold. If a test is in the way, it
is either wrong, in which case fix it and say why, or right, in which case the code is
wrong.
