//! `init` writes back what it read, so a config it cannot parse must survive
//! byte for byte (#972). Drives the binary because the paths come from `HOME`.
#![cfg(unix)]

use std::path::Path;
use std::process::Command;

const WITH_COMMENT: &str = "{\n  // keep me\n  \"theme\": \"dark\"\n}\n";

fn assert_init_leaves_alone(flag: &str, config: &str) {
    let home = tempfile::tempdir().expect("tempdir");
    let path = home.path().join(config);
    let parent = Path::new(&path).parent().expect("config has a parent");
    std::fs::create_dir_all(parent).expect("create config dir");
    std::fs::write(&path, WITH_COMMENT).expect("write fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_contexere"))
        .args(["init", flag])
        .env("HOME", home.path())
        .env("OMNI_DB_PATH", home.path().join("d.db"))
        .output()
        .expect("run init");

    let after = std::fs::read_to_string(&path).expect("config still readable");
    assert_eq!(after, WITH_COMMENT, "{flag} rewrote {config}");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(said.contains("not plain JSON"), "{flag} said: {said}");
}

/// A script reads the exit code, not the `Failed` line (#984). Claude runs before
/// Gemini, so its failure is the one that proves `init` keeps going.
#[test]
fn exits_1_after_installing_the_remaining_hosts() {
    let home = tempfile::tempdir().expect("tempdir");
    let claude = home.path().join(".claude/settings.json");
    std::fs::create_dir_all(claude.parent().expect("parent")).expect("create config dir");
    std::fs::write(&claude, WITH_COMMENT).expect("write fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_contexere"))
        .args(["init", "--claude", "--gemini"])
        .env("HOME", home.path())
        .env_remove("CLAUDE_CONFIG_DIR")
        .env("OMNI_DB_PATH", home.path().join("d.db"))
        .output()
        .expect("run init");

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not installed: Claude Code"), "{stderr}");
    let gemini = std::fs::read_to_string(home.path().join(".gemini/settings.json"))
        .expect("the later host still installed");
    assert!(gemini.contains("--pre-hook"), "{gemini}");
}

#[test]
fn exits_0_after_a_clean_install() {
    let home = tempfile::tempdir().expect("tempdir");
    let status = Command::new(env!("CARGO_BIN_EXE_contexere"))
        .args(["init", "--gemini"])
        .env("HOME", home.path())
        .env("OMNI_DB_PATH", home.path().join("d.db"))
        .output()
        .expect("run init")
        .status;
    assert_eq!(status.code(), Some(0));
}

/// Needs no network and no Pi, which is what every fresh machine looks like (#984, #994).
#[test]
fn init_all_exits_0_with_no_pi_and_writes_the_embedded_plugin() {
    let home = tempfile::tempdir().expect("tempdir");
    let output = Command::new(env!("CARGO_BIN_EXE_contexere"))
        .args(["init", "--all"])
        .current_dir(home.path())
        .env("HOME", home.path())
        .env("PATH", home.path())
        .env_remove("CLAUDE_CONFIG_DIR")
        .env("OMNI_DB_PATH", home.path().join("d.db"))
        .output()
        .expect("run init");

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let plugin = home.path().join(".openclaw/plugins/contexere");
    assert_eq!(
        std::fs::read_to_string(plugin.join("index.ts")).expect("the plugin was written"),
        include_str!("../plugins/openclaw/index.ts")
    );
}

#[test]
fn gemini_init_leaves_an_unparsed_config_alone() {
    assert_init_leaves_alone("--gemini", ".gemini/settings.json");
}

#[test]
fn cursor_init_leaves_an_unparsed_config_alone() {
    assert_init_leaves_alone("--cursor", ".cursor/mcp.json");
}

#[test]
fn claude_init_leaves_unparsed_settings_alone() {
    assert_init_leaves_alone("--claude", ".claude/settings.json");
}

/// An install from before 0.8.6 sits under the old id; leaving it would load the
/// plugin twice (#1065).
#[test]
fn openclaw_init_replaces_the_plugin_under_the_old_id() {
    let home = tempfile::tempdir().expect("tempdir");
    let plugins = home.path().join(".openclaw/plugins");
    let old = plugins.join("omni-signal-engine");
    std::fs::create_dir_all(&old).expect("old plugin dir");
    std::fs::write(old.join("index.ts"), "// old").expect("old plugin file");
    let config = home.path().join(".openclaw/openclaw.json");
    let fixture = serde_json::json!({ "plugins": { "load": { "paths": [old] } } });
    std::fs::write(&config, fixture.to_string()).expect("config");

    let output = Command::new(env!("CARGO_BIN_EXE_contexere"))
        .args(["init", "--openclaw"])
        .env("HOME", home.path())
        .env("OMNI_DB_PATH", home.path().join("d.db"))
        .output()
        .expect("run init");

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(!old.exists(), "the old plugin is still on disk");
    assert!(plugins.join("contexere/index.ts").exists());
    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&config).expect("read config"))
            .expect("config parses");
    assert_eq!(
        config["plugins"]["load"]["paths"],
        serde_json::json!([plugins.join("contexere")])
    );
}

#[test]
fn hermes_init_replaces_the_plugin_under_the_old_name() {
    let home = tempfile::tempdir().expect("tempdir");
    let plugins = home.path().join(".hermes/plugins");
    let old = plugins.join("omni-signal-engine");
    std::fs::create_dir_all(&old).expect("old plugin dir");
    std::fs::write(old.join("__init__.py"), "# old").expect("old plugin file");
    let config = home.path().join(".hermes/config.yaml");
    std::fs::write(&config, "plugins:\n  enabled:\n    - omni-signal-engine\n").expect("config");

    let output = Command::new(env!("CARGO_BIN_EXE_contexere"))
        .args(["init", "--hermes"])
        .env("HOME", home.path())
        .env("OMNI_DB_PATH", home.path().join("d.db"))
        .output()
        .expect("run init");

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(!old.exists(), "the old plugin is still on disk");
    assert!(plugins.join("contexere/__init__.py").exists());
    let config = std::fs::read_to_string(&config).expect("read config");
    assert!(config.contains("    - contexere\n"), "{config}");
    assert!(!config.contains("omni-signal-engine"), "{config}");
}
