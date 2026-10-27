//! Every gate names itself in `passthrough_events.reason`.
//!
//! The reason used to be a `[tag]` suffix on the command because there was no
//! column for it (#254), and two of the four gates wrote the bare command, which
//! made a signal file that stripped its whole input and a distiller too weak to
//! keep indistinguishable in the table this project queries to decide what to
//! build. #441 gave it a column; these tests moved with it and assert the same
//! property, that no recorded passthrough is ambiguous between the seven gates.

use std::sync::Arc;
use veng::hooks::post_tool::process_payload;
use veng::store::sqlite::Store;

fn bash_payload(command: &str, content: &str) -> String {
    serde_json::json!({
        "tool_name": "Bash",
        "tool_input": { "command": command },
        "tool_response": { "content": content },
    })
    .to_string()
}

fn store_in(dir: &tempfile::TempDir) -> Arc<Store> {
    Arc::new(Store::open_path(&dir.path().join("veng.db")).expect("temp store"))
}

/// Prose no distiller has a grammar for, past `MIN_DISTILL_TOKENS` so the
/// pipeline runs and lands on the sub-10% measurement rather than an early exit.
///
/// Deliberately comma-free: `format::sniff` calls any text CSV when every line
/// carries the same non-zero comma count, so prose with a steady comma rhythm
/// exits at the format gate and never reaches the gate under test.
fn undistillable_prose() -> String {
    (0..60)
        .map(|i| {
            format!(
                "The quarterly review notes that section {i} remains open pending \
                 a decision from the working group which has not yet met.\n"
            )
        })
        .collect()
}

#[test]
fn tags_the_gate_when_a_distiller_misses_the_guardrail() {
    // Arrange
    let dir = tempfile::tempdir().expect("temp dir");
    let store = store_in(&dir);
    let payload = bash_payload("date", &undistillable_prose());

    // Act
    let _ = process_payload(&payload, Some(store.clone()), None);

    // Assert
    let rows = store.passthrough_reasons(0);
    assert!(
        rows.iter().any(|(reason, _)| reason == "below guardrail"),
        "a weak distiller must name its gate, got: {rows:?}"
    );
}

#[test]
fn tags_the_gate_when_the_payload_is_structured() {
    // The format sniff already tagged before #254. Kept so the assertion above
    // cannot pass by an empty table if the sub-10% path stops recording.
    let dir = tempfile::tempdir().expect("temp dir");
    let store = store_in(&dir);
    let payload = bash_payload("az vm list -o json", r#"[{"name":"vm-1","id":"/subs/x"}]"#);

    let _ = process_payload(&payload, Some(store.clone()), None);

    let rows = store.passthrough_reasons(0);
    assert!(
        rows.iter().any(|(reason, _)| reason == "structured:json"),
        "the format gate must name the format, got: {rows:?}"
    );
}

#[test]
fn every_recorded_passthrough_names_a_gate() {
    let dir = tempfile::tempdir().expect("temp dir");
    let store = store_in(&dir);

    for payload in [
        bash_payload("date", &undistillable_prose()),
        bash_payload("az vm list -o json", r#"[{"name":"vm-1"}]"#),
        bash_payload(
            "kubectl get pods -o yaml",
            "apiVersion: v1\nkind: List\nitems: []\n",
        ),
    ] {
        let _ = process_payload(&payload, Some(store.clone()), None);
    }

    let rows = store.passthrough_reasons(0);
    assert!(!rows.is_empty(), "no passthrough was recorded at all");
    for (reason, count) in &rows {
        assert!(
            !reason.is_empty() && reason != "unrecorded",
            "a row with no gate named is ambiguous between all seven: {reason:?} x{count}"
        );
    }
}
