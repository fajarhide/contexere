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
