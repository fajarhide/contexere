//! #1189: 0.9.0 installs no `omni` command, so a plugin that falls back to it
//! fails to start the binary and, failing open, quietly stops distilling.

fn read(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read {path}: {e}"))
}

#[test]
fn openclaw_falls_back_to_contexere() {
    let index = read("plugins/openclaw/index.ts");
    assert!(
        index.contains(r#".omniPath || "contexere""#),
        "OpenClaw's default binary"
    );
    let manifest: serde_json::Value =
        serde_json::from_str(&read("plugins/openclaw/openclaw.plugin.json")).expect("json");
    assert_eq!(
        manifest["requiredBinaries"],
        serde_json::json!(["contexere"])
    );
}

#[test]
fn pi_falls_back_to_contexere() {
    let index = read("plugins/pi/index.ts");
    assert!(
        index.contains(r#"const DEFAULT_CONTEXERE_PATH = "contexere";"#),
        "Pi's default binary"
    );
}
