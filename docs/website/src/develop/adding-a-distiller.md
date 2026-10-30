# Adding a distiller

The most common change here. Five steps, and the fourth is the one that matters.

## 1. The module

`crates/contexere-reduce/src/distillers/my_type.rs`:

```rust
use crate::pipeline::{OutputSegment, SessionState};
use super::Distiller;

pub struct MyDistiller;

impl Distiller for MyDistiller {
    fn distill(
        &self,
        segments: &[OutputSegment],
        input: &str,
        session: Option<&SessionState>,
    ) -> Option<String> {
        // Return None the moment you are not sure you parsed this.
        todo!()
    }
}
```

Return `None` whenever parsing failed. That hands back the raw bytes, which is the
correct answer and the one the whole design rests on.

Never return a success string from a zero state. `vitest: ✓ 0/0 passed` for output
that was actually a dev server is failing **closed**, confidently, and it is the exact
defect this project keeps fixing.

## 2. Register it

`crates/contexere-reduce/src/distillers/mod.rs`:

```rust
pub mod my_type;

// in get_distiller():
ContentType::MyType => Box::new(my_type::MyDistiller),
```

Routing belongs in `pipeline/registry.rs`. Do not add a `matches!(cmd, ...)` block
inside the distiller; that duplication is what the registry exists to prevent.

## 3. A realistic fixture

`tests/fixtures/my_type_example.txt`. Real output from the real tool, not something
hand-written to be easy to parse.

## 4. A snapshot test, and prove it can fail

```rust
snapshot_test!(test_my_type_distillation, "my_type_example.txt", ContentType::MyType);
```

```sh
cargo test
cargo insta review
```

Then **break the rule deliberately and watch the test go red**, before restoring it. A
check that cannot fail proves nothing, and this repo has shipped two regression tests
that could not fail.

Two specific ways a test here passes for the wrong reason:

- Your fixture reaches a different collapse mode than you think. A `kubectl … | grep`
  fixture exercises Infra, not Log, so a guard you are testing may never be consulted.
- "No rewrite from the hook" is not proof the distiller punted. It can mean the format
  gate fired, or the guardrail rejected the result.

A distiller can also return a near-copy rather than the exact input, so detect "this
did not help" with `beats_guardrail` rather than comparing against the input.

## 5. Gates

```sh
cargo fmt
cargo clippy -- -D warnings
OMNI_DB_PATH=/tmp/t.db cargo test
```

`OMNI_DB_PATH` is not optional. Parallel tests competing for `~/.omni/omni.db` cause
SQLite locks: 79 seconds green against an isolated database, 433 seconds and then a
hang against the live one.

## Before you write any of it

Measure the workload. `~/.omni/omni.db` prices a proposal in one query, and the answer
is often the opposite of the request.

"Improve the python3 distiller" turned into two facts in two queries: `python3` was
already reporting 97.2%, and the savings were the collapse fallback deleting data
rows. The obvious feature, a traceback distiller, died on **9 of 7,506** traces
containing a traceback.

```sql
-- distillations.filter_name is the command's first token
-- execution_traces holds raw_input and distilled_output in full
```

Read the rows before quoting an aggregate over them. A `LIKE` filter that caught the
wrong rows has already put a wrong figure into a published issue.

And never read `sqlite3` output through the Bash hook while doing this. The pipeline
can fold the rows you are counting.

## The bar the result has to clear

Not "did it compress". These:

1. Would the agent still have the answer?
2. Does anything dropped leave a marker?
3. Does the reported number describe what actually happened?

A patch that raises reduction percentage while removing signal is the project's own
recurring defect, shipped again with your name on it.
