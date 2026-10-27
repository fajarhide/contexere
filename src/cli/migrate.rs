//! `veng migrate`: bring an omni install across to veng.
//!
//! Three things move, and none of them is destroyed:
//!
//! 1. **Data.** omni's database and config are copied, never moved. The old
//!    `~/.omni` stays exactly as it was, so going back is `omni` still working.
//! 2. **Claude Code hooks.** omni's entries in `~/.claude/settings.json` are
//!    replaced by veng's in one write, after the original is saved as
//!    `settings.json.bak`.
//! 3. **Claude Code MCP.** Same for the `omni` server in `~/.claude.json`.
//!
//! Claude Code is touched only if omni was installed there. A machine that never
//! had omni gets nothing from this command; `veng init` is the way in for that.
//! `--dry-run` reports everything above and writes nothing.

use crate::agents::claude;
use crate::legacy::{self, Database};
use anyhow::Result;
use colored::*;
use serde_json::Value;
use std::path::Path;

const FLAGS: super::Flags = &[("--dry-run", "Show what would change and change nothing")];

fn print_help() {
    println!(
        "\n{} {}: Move an omni install across to veng",
        "veng".bold().cyan(),
        "migrate".bold().yellow()
    );
    println!("\n{}", "USAGE:".bold().bright_white());
    println!("  veng {} {}", "migrate".cyan(), "[FLAGS]".bright_black());
    println!();
    super::print_flags(FLAGS);
    println!(
        "Copies omni's database and config, and replaces omni's Claude Code hooks and\n\
         MCP entry with veng's. Deletes nothing: the old tree stays, and settings are\n\
         backed up first.\n"
    );
}

/// What was found and done in Claude Code's files.
#[derive(Debug, Default, PartialEq, Eq)]
struct ClaudeReport {
    hooks: usize,
    mcp: usize,
}

pub fn run(args: &[String]) -> Result<()> {
    if super::wants_help(args) {
        print_help();
        return Ok(());
    }
    super::check_flags("migrate", args, FLAGS)?;
    let dry_run = super::has_flag(args, "--dry-run");

    let source = legacy::legacy_dir();
    let data = crate::paths::data_home();
    let config = crate::paths::config_home();

    let tag = if dry_run {
        " (dry run, nothing written)"
    } else {
        ""
    };
    println!(
        "\n  {}{}",
        "veng migrate".bold().bright_white(),
        tag.bright_black()
    );

    // 1. Data.
    let report = legacy::migrate_data(&source, &data, &config, dry_run)?;
    println!("\n  {}", "Data".bold());
    match report.database {
        Database::Copied { bytes } => println!(
            "    {} database copied from {} ({} bytes)",
            "✓".green(),
            source.display(),
            bytes
        ),
        Database::WouldCopy { bytes } => println!(
            "    {} would copy {}/{} (at least {} bytes)",
            "·".yellow(),
            source.display(),
            legacy::LEGACY_DB,
            bytes
        ),
        Database::AlreadyPresent => println!(
            "    {} veng already has a database, left alone",
            "·".bright_black()
        ),
        Database::NoLegacyDatabase => println!(
            "    {} no omni database at {}",
            "·".bright_black(),
            source.display()
        ),
    }
    for item in &report.config {
        let verb = if dry_run { "would copy" } else { "copied" };
        println!("    {} {verb} {item}", "✓".green());
    }

    // 2 and 3. Claude Code.
    let exe = std::env::current_exe()?.to_string_lossy().to_string();
    let claude = migrate_claude(&exe, dry_run)?;
    println!("\n  {}", "Claude Code".bold());
    if claude == ClaudeReport::default() {
        println!(
            "    {} no omni hooks or MCP entry found. `veng init --claude` installs veng fresh.",
            "·".bright_black()
        );
    } else {
        let verb = if dry_run { "would replace" } else { "replaced" };
        println!(
            "    {} {verb} {} hook entr{} and {} MCP entr{} with veng's",
            "✓".green(),
            claude.hooks,
            if claude.hooks == 1 { "y" } else { "ies" },
            claude.mcp,
            if claude.mcp == 1 { "y" } else { "ies" },
        );
    }

    if !dry_run {
        if claude != ClaudeReport::default() {
            println!(
                "\n  Restart Claude Code (CLI or desktop) so it reloads the hooks, then run {}.\n",
                "veng doctor".cyan()
            );
        } else {
            println!();
        }
    } else {
        println!("\n  Run again without {} to apply.\n", "--dry-run".cyan());
    }
    Ok(())
}

fn migrate_claude(exe: &str, dry_run: bool) -> Result<ClaudeReport> {
    Ok(ClaudeReport {
        hooks: migrate_settings(&claude::get_settings_path(), exe, dry_run)?,
        mcp: migrate_mcp(&claude::get_claude_json_path(), exe, dry_run)?,
    })
}

/// Swap omni's hooks for veng's in one pass, so the single `.bak` the backup
/// writes is the original and not an already edited copy.
fn migrate_settings(path: &Path, exe: &str, dry_run: bool) -> Result<usize> {
    let Some(mut val) = read_json(path) else {
        return Ok(0);
    };
    let removed = claude::remove_legacy_hooks(&mut val);
    if removed > 0 && !dry_run {
        claude::install_veng_hooks(&mut val, exe);
        write_with_backup(path, &val)?;
    }
    Ok(removed)
}

fn migrate_mcp(path: &Path, exe: &str, dry_run: bool) -> Result<usize> {
    let Some(mut val) = read_json(path) else {
        return Ok(0);
    };
    let removed = claude::remove_legacy_mcp(&mut val);
    if removed > 0 && !dry_run {
        write_with_backup(path, &val)?;
        claude::install_mcp_server(exe)?;
    }
    Ok(removed)
}

/// `None` for a missing file and for one that is not JSON: a file this command
/// cannot read is a file it must not rewrite.
fn read_json(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_with_backup(path: &Path, val: &Value) -> Result<()> {
    claude::backup_settings(&path.to_path_buf())?;
    std::fs::write(path, serde_json::to_string_pretty(val)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn settings_with_omni() -> Value {
        json!({
            "model": "keep-me",
            "hooks": { "PostToolUse": [
                { "matcher": "Bash", "hooks": [{ "type": "command", "command": "/usr/bin/omni --post-hook" }] }
            ]}
        })
    }

    #[test]
    fn a_dry_run_reports_the_hooks_and_leaves_the_file_alone() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("settings.json");
        let original = serde_json::to_string_pretty(&settings_with_omni()).expect("json");
        std::fs::write(&path, &original).expect("write");

        let n = migrate_settings(&path, "/opt/veng", true).expect("dry run");

        assert_eq!(n, 1);
        assert_eq!(std::fs::read_to_string(&path).expect("read"), original);
        assert!(
            !dir.path().join("settings.json.bak").exists(),
            "a dry run wrote a backup"
        );
    }

    #[test]
    fn swaps_the_hook_keeps_everything_else_and_backs_up_the_original() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("settings.json");
        let original = serde_json::to_string_pretty(&settings_with_omni()).expect("json");
        std::fs::write(&path, &original).expect("write");

        let n = migrate_settings(&path, "/opt/veng", false).expect("migrate");

        assert_eq!(n, 1);
        let after = std::fs::read_to_string(&path).expect("read");
        assert!(!after.contains("omni"), "omni is still referenced: {after}");
        assert!(
            after.contains("/opt/veng"),
            "veng's hook was not installed: {after}"
        );
        assert!(
            after.contains("keep-me"),
            "an unrelated setting was lost: {after}"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("settings.json.bak")).expect("backup"),
            original,
            "the backup must be the file as it was before this command"
        );
    }

    #[test]
    fn leaves_a_machine_without_omni_untouched() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("settings.json");
        let text = r#"{"hooks":{"PostToolUse":[{"hooks":[{"type":"command","command":"/bin/lint.sh"}]}]}}"#;
        std::fs::write(&path, text).expect("write");

        assert_eq!(
            migrate_settings(&path, "/opt/veng", false).expect("migrate"),
            0
        );
        assert_eq!(std::fs::read_to_string(&path).expect("read"), text);
        assert!(!dir.path().join("settings.json.bak").exists());
    }

    #[test]
    fn a_file_that_is_not_json_is_never_rewritten() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{ not json").expect("write");

        assert_eq!(
            migrate_settings(&path, "/opt/veng", false).expect("migrate"),
            0
        );
        assert_eq!(std::fs::read_to_string(&path).expect("read"), "{ not json");
    }
}
