//! Format-safe gate, structured payloads must leave the hooks byte-for-byte intact.
//!
//! Regression cover for the live repro: `az … -o json` and `git show <dashboard>.json`
//! had their repeated lines squashed into `[N similar lines collapsed]`, so the payload
//! no longer parsed and every downstream `jq` / `json.load` / `kubectl apply` broke.

use std::path::Path;

use contexere::hooks::{pipe, post_tool};
use serde_json::Value;

fn fixture(name: &str) -> String {
    let path = Path::new("tests").join("fixtures").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Run input through the pipe hook and return stdout.
fn pipe_through(input: &str, command: &str) -> String {
    let mut out = Vec::new();
    let mut err = Vec::new();
    pipe::run_inner(
        input.as_bytes(),
        &mut out,
        &mut err,
        None,
        None,
        Some(command),
    )
    .expect("pipe hook must not fail");
    String::from_utf8(out).expect("pipe output must stay valid UTF-8")
}

/// Build a Claude Code PostToolUse payload around a tool's raw output.
fn bash_payload(command: &str, content: &str) -> String {
    serde_json::json!({
        "tool_name": "Bash",
        "tool_input": { "command": command },
        "tool_response": { "content": content },
    })
    .to_string()
}

#[test]
fn az_json_output_survives_pipe_byte_for_byte() {
    // Arrange
    let raw = fixture("az_vm_list.json");

    // Act
    let out = pipe_through(&raw, "az vm list -o json");

    // Assert
    assert_eq!(out, raw, "structured output must pass through unmodified");
    assert_eq!(
        serde_json::from_str::<Value>(&out).expect("output must still parse"),
        serde_json::from_str::<Value>(&raw).expect("fixture must parse"),
        "output must parse to the same value"
    );
}

#[test]
fn json_dashboard_survives_pipe_byte_for_byte() {
    // The `git show` repro: a large JSON dashboard piped through the hook.
    let raw = fixture("grafana_dashboard.json");

    let out = pipe_through(&raw, "git show HEAD:dashboards/prod-k8s-overview.json");

    assert_eq!(out, raw);
    assert_eq!(
        serde_json::from_str::<Value>(&out).expect("dashboard must still parse"),
        serde_json::from_str::<Value>(&raw).expect("fixture must parse")
    );
}

#[test]
fn json_array_of_similar_objects_is_not_collapsed() {
    // 20 look-alike lines is exactly what collapse used to squash.
    let raw = format!(
        "[\n{}\n]",
        (0..20)
            .map(|i| format!(r#"  {{"name": "pod-{i}", "status": "Running", "restarts": 0}}"#))
            .collect::<Vec<_>>()
            .join(",\n")
    );

    let out = pipe_through(&raw, "kubectl get pods -o json");

    assert!(
        !out.contains("collapsed"),
        "collapse marker must never appear in structured output: {out}"
    );
    assert_eq!(
        serde_json::from_str::<Value>(&out).expect("output must still parse"),
        serde_json::from_str::<Value>(&raw).expect("input must parse")
    );
}

#[test]
fn ndjson_log_survives_pipe_byte_for_byte() {
    let raw = (0..30)
        .map(|i| format!(r#"{{"ts": {i}, "level": "info", "msg": "request served"}}"#))
        .collect::<Vec<_>>()
        .join("\n");

    let out = pipe_through(&raw, "kubectl logs deploy/api");

    assert_eq!(out, raw);
    for line in out.lines() {
        serde_json::from_str::<Value>(line).expect("every NDJSON line must still parse");
    }
}

#[test]
fn yaml_manifest_survives_pipe_byte_for_byte() {
    let raw = "---\n\
               apiVersion: apps/v1\n\
               kind: Deployment\n\
               metadata:\n  \
                 name: api\n  \
                 namespace: prod\n\
               spec:\n  \
                 replicas: 3\n  \
                 template:\n    \
                   spec:\n      \
                     containers:\n        \
                       - name: api\n          \
                         image: registry.example.com/api:1.2.3\n";

    let out = pipe_through(raw, "kubectl get deploy api -o yaml");

    assert_eq!(out, raw);
}

#[test]
fn post_tool_hook_leaves_structured_output_untouched() {
    // `None` means "no rewrite", the host keeps the original bytes at zero marker cost.
    let raw = fixture("az_vm_list.json");
    let payload = bash_payload("az vm list -o json", &raw);

    let out = post_tool::process_payload(&payload, None, None);

    assert!(
        out.is_none(),
        "structured output must not be rewritten, got: {out:?}"
    );
}

#[test]
fn post_tool_hook_leaves_json_dashboard_untouched() {
    let raw = fixture("grafana_dashboard.json");
    let payload = bash_payload("git show HEAD:dashboards/prod-k8s-overview.json", &raw);

    assert!(post_tool::process_payload(&payload, None, None).is_none());
}

#[test]
fn chatty_build_log_still_compresses_through_pipe() {
    // The gate must not over-trigger: plain text is still the pipeline's job.
    let raw = fixture("heavy_noise.txt");

    let out = pipe_through(&raw, "docker build .");

    assert!(
        out.len() < raw.len(),
        "text build log must still compress: {} → {} bytes",
        raw.len(),
        out.len()
    );
}

/// #832 moved the diff from the first group to the second. It used to be asked
/// to compress here, beside the build log above; a unified diff is parsed by
/// `git apply` and `patch`, and what came back failed `git apply --check` with
/// `patch fragment without header`.
///
/// The build log test above stays as it is, because it is the guard against this
/// gate swallowing plain text.
#[test]
fn git_diff_goes_through_the_pipe_unchanged() {
    let raw = fixture("git_diff_multi_file.txt");

    let out = pipe_through(&raw, "git diff");

    assert_eq!(out, raw, "a unified diff was rewritten on the pipe path");
}

#[test]
fn space_aligned_table_is_not_mistaken_for_a_delimited_format() {
    // `kubectl get pods` is column-aligned with spaces, not tabs. The delimited
    // sniffer must not read it as TSV, because gating it would hand back 386
    // bytes of pod table that a distiller summarises well.
    //
    // This asks the sniffer directly. It used to assert that the pipe's output
    // was smaller than its input, which is a proxy, and the proxy broke for a
    // reason that had nothing to do with sniffing: on a 386 byte fixture with no
    // store, the omission marker costs more than the cut saves, so the pipe
    // correctly hands the raw bytes back (#268, #269). Both outcomes look
    // identical from outside, so the size test could no longer tell "the sniffer
    // gated it" from "the marker was not worth it".
    let raw = fixture("kubectl_pods_mixed.txt");

    assert!(
        contexere::pipeline::format::sniff(&raw).is_none(),
        "a space-aligned table is not a delimited format"
    );
}

/// A text build log must not be claimed by the format gate, that gate exists
/// for payloads a later step parses, and gating this one would take every build
/// log off the lossy path.
///
/// This asserted `process_payload(...).is_some()` until #118 item 5. That could
/// not fail: a passthrough used to return the input under a banner, so *any*
/// payload over the 50-byte floor made the hook return something, whether it had
/// been distilled or not. The fixture is 317 bytes of error blocks, which
/// `BuildDistiller` keeps by design, so it was never distilled and the test
/// never said so. Assert the gate directly instead.
#[test]
fn a_text_build_log_is_not_claimed_by_the_format_gate() {
    let raw = fixture("cargo_build_errors.txt");

    assert!(
        contexere::pipeline::format::sniff(&raw).is_none(),
        "a build log is free text; gating it as structured would take every \
         build log off the lossy path"
    );
}

#[test]
fn malformed_json_passes_through_without_panicking() {
    // Unsure → structured. A truncated payload cannot be helped by compression.
    let raw = r#"{"items": [{"name": "a"}, {"name": "b"}, {"name": }"#;

    let out = pipe_through(raw, "az vm list -o json");

    assert_eq!(out, raw);
}
