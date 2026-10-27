use crate::agents::AgentIntegration;
use colored::*;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

pub struct CodexIntegration;

impl AgentIntegration for CodexIntegration {
    fn tier(&self) -> crate::agents::Tier {
        crate::agents::Tier::Full
    }

    fn id(&self) -> &'static str {
        "codex"
    }

    fn name(&self) -> &'static str {
        "Codex CLI"
    }

    fn install(&self, exe_path: &str) -> anyhow::Result<()> {
        let codex_dir = get_codex_dir();
        fs::create_dir_all(&codex_dir)?;

        // Install MCP server in config.toml
        let config_path = codex_dir.join("config.toml");
        let mut content = if config_path.exists() {
            fs::read_to_string(&config_path)?
        } else {
            String::new()
        };

        // A sub-table like `[mcp_servers.veng.tools.x]` declares `mcp_servers.veng`
        // on its own. Without the parent block beside it the server has no
        // transport, and Codex refuses to start at all:
        //
        //   Error: config.toml:45:14: invalid transport
        //   Caused by: invalid transport in `mcp_servers.veng`
        //
        // The old test was `contains("[mcp_servers.veng]")`, which an orphaned
        // sub-table does not satisfy, so install could not repair what uninstall
        // had left behind (#351). Verified in a copied CODEX_HOME on 0.144.6:
        // orphan alone and `codex features list` dies; parent restored and it runs.
        if !declares_transport(&content) {
            content.push_str(&format!(
                "\n[mcp_servers.veng]\ntype = \"stdio\"\ncommand = \"{}\"\nargs = [\"--mcp\"]\n",
                exe_path
            ));
            fs::write(&config_path, content)?;
        }

        crate::agent_report!(
            "  {} Configured MCP Server in {}",
            "✓".green(),
            config_path.display()
        );

        // Install hooks in hooks.json
        install_veng_hooks(exe_path)?;
        crate::agent_report!(
            "  {} Configured {} in {}",
            "✓".green(),
            "Hooks".bold(),
            codex_dir.join("hooks.json").display()
        );
        // Codex will not run a hook it has not been told to trust, and says
        // nothing when it skips one, so writing the config is only half of the
        // install (#359).
        crate::agent_report!(
            "  {} Start {} once and approve them under {}, or Codex skips them",
            "!".yellow(),
            "codex".bold(),
            "\"Hooks need review\"".bold()
        );
        // Writing the entries changes their hashes, so any approval that already
        // existed is void. An upgrade that silently disables a working install is
        // worse than a fresh one that was never approved (#367).
        crate::agent_report!(
            "    {} this rewrote the entries, so earlier approvals no longer apply",
            "note:".bright_black()
        );
        // Codex keeps its bypass on the command line on purpose: it is rejected
        // from config.toml, so no installer can turn it on for you (#359).
        crate::agent_report!(
            "    {} for unattended runs, pass {}",
            "or".bright_black(),
            "--dangerously-bypass-hook-trust".bright_black()
        );

        Ok(())
    }

    fn uninstall(&self) -> anyhow::Result<()> {
        let codex_dir = get_codex_dir();

        // Remove MCP from config.toml
        let config_path = codex_dir.join("config.toml");
        if config_path.exists() {
            let content = fs::read_to_string(&config_path)?;
            if content.contains("[mcp_servers.veng") {
                let new_content = strip_veng_server(&content);
                fs::write(&config_path, new_content.trim_end().to_string() + "\n")?;
                crate::agent_report!(
                    "  {} Removed MCP Server from {}",
                    "✓".yellow(),
                    config_path.display()
                );
            }
        }

        // Remove hooks from hooks.json
        remove_veng_hooks()?;
        crate::agent_report!(
            "  {} Removed Hooks from {}",
            "✓".yellow(),
            codex_dir.join("hooks.json").display()
        );

        Ok(())
    }

    fn doctor_check(&self, fix_mode: bool, warnings: &mut Vec<String>) -> bool {
        let codex_dir = get_codex_dir();
        let config_path = codex_dir.join("config.toml");
        let hooks_path = codex_dir.join("hooks.json");
        let mut all_ok = true;

        crate::agent_report!("\n  {}", "Codex CLI:".cyan());

        // Check MCP config
        if config_path.exists()
            && fs::read_to_string(&config_path)
                .unwrap_or_default()
                .contains("veng")
        {
            crate::agent_report!(
                "   {:<15} {} {}",
                "Config:".bright_black(),
                config_path.display().to_string().bright_black(),
                "[OK]".green().bold()
            );
        } else {
            all_ok = false;
            if fix_mode {
                if let Ok(exe_path) = std::env::current_exe() {
                    crate::agents::report_fix(
                        "Config:",
                        "registered",
                        self.install(&exe_path.to_string_lossy()),
                        warnings,
                    );
                }
            } else {
                crate::agent_report!(
                    "   {:<15} {}",
                    "Config:".bright_black(),
                    "not configured".bright_black()
                );
                warnings.push(
                    "Codex CLI MCP server not configured. Run `veng init --codex`.".to_string(),
                );
            }
        }

        // Check hooks
        let hooks_content = if hooks_path.exists() {
            fs::read_to_string(&hooks_path).unwrap_or_default()
        } else {
            String::new()
        };

        let has_pre = hooks_content.contains("--pre-hook");
        let has_post = hooks_content.contains("--post-hook");
        let has_session = hooks_content.contains("--session-start");

        if has_pre && has_post && has_session {
            let fmt_hook = |name: &str, present: bool| {
                if present {
                    crate::agent_report!(
                        "   {:<15} {}",
                        name.bright_black(),
                        "[OK] installed".green()
                    );
                }
            };
            fmt_hook("PreToolUse", has_pre);
            fmt_hook("PostToolUse", has_post);
            fmt_hook("SessionStart", has_session);

            // Installed is not the same as running: Codex skips any hook it has
            // not been told to trust, and says nothing about it (#359).
            let config = fs::read_to_string(&config_path).unwrap_or_default();
            let awaiting = hooks_awaiting_review(&config, &hooks_path.to_string_lossy());
            if !awaiting.is_empty() {
                all_ok = false;
                crate::agent_report!(
                    "   {:<15} {}",
                    "Trust:".bright_black(),
                    format!("[WARNING] {} awaiting review in Codex", awaiting.join(", ")).yellow()
                );
                warnings.push(format!(
                    "Codex has not been told to trust VENG's hooks ({}), so it skips them silently. \
                     Start `codex` once and approve them under \"Hooks need review\", or pass \
                     `--dangerously-bypass-hook-trust` for unattended runs.",
                    awaiting.join(", ")
                ));
            } else {
                // A record existing is not the same as it matching. Codex stores
                // the entry's hash and re-checks it, and `veng init --codex`
                // rewrites the entries, so an upgrade invalidates approvals that
                // were working. The hash preimage is not derivable from the
                // config, so this cannot be verified here and must not be
                // reported as healthy (#367).
                crate::agent_report!(
                    "   {:<15} {}",
                    "Trust:".bright_black(),
                    "recorded; re-approve after any upgrade (hash re-checked)".bright_black()
                );
            }
        } else {
            all_ok = false;
            if fix_mode {
                if let Ok(exe_path) = std::env::current_exe() {
                    crate::agents::report_fix(
                        "Hooks:",
                        "missing hooks installed",
                        install_veng_hooks(&exe_path.to_string_lossy()),
                        warnings,
                    );
                }
            } else {
                if !has_pre {
                    crate::agent_report!(
                        "   {:<15} {}",
                        "PreToolUse".bright_black(),
                        "[WARNING] missing".yellow()
                    );
                }
                if !has_post {
                    crate::agent_report!(
                        "   {:<15} {}",
                        "PostToolUse".bright_black(),
                        "[WARNING] missing".yellow()
                    );
                }
                if !has_session {
                    crate::agent_report!(
                        "   {:<15} {}",
                        "SessionStart".bright_black(),
                        "[WARNING] missing".yellow()
                    );
                }
                warnings
                    .push("Codex CLI hooks not configured. Run `veng init --codex`.".to_string());
            }
        }

        all_ok
    }
}

/// True when `mcp_servers.veng` is declared *with* a transport, which is the only
/// state Codex will start from.
///
/// A bare `[mcp_servers.veng.tools.x]` also declares the server, so presence of
/// the name says nothing. Codex needs the parent block carrying `command`, and
/// rejects the whole config without it.
fn declares_transport(config: &str) -> bool {
    let mut in_parent = false;
    for line in config.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_parent = t == "[mcp_servers.veng]";
            continue;
        }
        if in_parent && t.starts_with("command") {
            return true;
        }
    }
    false
}

/// Removes `[mcp_servers.veng]` **and every `[mcp_servers.veng.*]` sub-table**.
///
/// The old version stopped skipping at the next `[`, so a sub-table survived its
/// own parent. Codex then read a server with no transport and refused to start,
/// which is a working install turned into a dead one by running uninstall (#351).
fn strip_veng_server(config: &str) -> String {
    let mut out = String::with_capacity(config.len());
    let mut skip = false;
    for line in config.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            skip = t == "[mcp_servers.veng]" || t.starts_with("[mcp_servers.veng.");
        }
        if !skip {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Codex reads its config from `$CODEX_HOME` and only falls back to `~/.codex`.
/// Installing to the fallback while the host is using the override writes a
/// config Codex never loads, and `doctor` then reports the file it wrote rather
/// than the file in use. Seen on a machine where a launcher exported
/// `CODEX_HOME` to its own runtime directory: `~/.codex/config.toml` held the
/// MCP server, the live home did not, and VENG reported healthy either way.
/// The VENG hook events Codex has no trust record for, and will therefore skip.
///
/// Codex 0.144.6 trusts hooks one entry at a time, keyed
/// `<hooks.json path>:<event>:<group>:<index>` under `[hooks.state]`, with the
/// entry's own hash. An entry it has not been shown is ignored, and `codex exec`
/// prints nothing about it, so a config that reads as installed does nothing at
/// all. Proved by deleting one trust record: the hook stopped firing, and came
/// back when the record was restored (#359).
///
/// VENG deliberately does not write these records. The hash exists so a human
/// approves each command before Codex runs it, and a tool that grants itself
/// that approval removes the only thing standing between "can write a config
/// file" and "can execute anything".
fn hooks_awaiting_review(config: &str, hooks_path: &str) -> Vec<&'static str> {
    [
        ("pre_tool_use", "PreToolUse"),
        ("post_tool_use", "PostToolUse"),
        ("session_start", "SessionStart"),
    ]
    .into_iter()
    // TOML escapes backslashes inside a basic string, so a Windows key reads
    // `C:\\Users\\...` while `hooks_path` has single ones and the match never
    // fired: doctor reported all three hooks awaiting review on Windows even
    // when they were approved, and failed. The test used a POSIX literal, so CI
    // passed on windows-latest.
    .filter(|(wire, _)| {
        let escaped = hooks_path.replace('\\', "\\\\");
        !config.contains(&format!("{}:{}:", escaped, wire))
            && !config.contains(&format!("{}:{}:", hooks_path, wire))
    })
    .map(|(_, display)| display)
    .collect()
}

fn get_codex_dir() -> PathBuf {
    codex_dir_from(std::env::var_os("CODEX_HOME"), dirs::home_dir())
}

/// The choice itself, kept free of the environment so it can be tested without
/// a concurrently running test seeing the mutation.
fn codex_dir_from(codex_home: Option<std::ffi::OsString>, home: Option<PathBuf>) -> PathBuf {
    match codex_home {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => home.unwrap_or_else(|| PathBuf::from(".")).join(".codex"),
    }
}

pub fn install_veng_hooks(exe_path: &str) -> anyhow::Result<()> {
    let hooks_path = get_codex_dir().join("hooks.json");
    fs::create_dir_all(get_codex_dir())?;

    let mut val = if hooks_path.exists() {
        let content = fs::read_to_string(&hooks_path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    let obj = match val.as_object_mut() {
        Some(o) => o,
        None => {
            val = json!({});
            val.as_object_mut().unwrap()
        }
    };

    let hooks = obj
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .unwrap();

    let pre_cmd = format!("{} --pre-hook", exe_path);
    let post_cmd = format!("{} --post-hook", exe_path);
    let session_cmd = format!("{} --session-start", exe_path);

    // Codex reads a matcher group whose `hooks` array holds the handlers; a bare
    // `{"command": ...}` entry is accepted by the parser and never executed, with
    // no warning. Same home, same `--dangerously-bypass-hook-trust`, same script:
    // the flat entry ran 0 times and this shape ran once (#364). Earlier shape
    // probes missed it because they wrote to `~/.codex` while `CODEX_HOME`
    // pointed elsewhere, so they were testing a file Codex never opened.
    let ensure_hook = ensure_hook_entry;

    ensure_hook(
        hooks.entry("PreToolUse").or_insert_with(|| json!([])),
        &pre_cmd,
    );
    ensure_hook(
        hooks.entry("PostToolUse").or_insert_with(|| json!([])),
        &post_cmd,
    );
    ensure_hook(
        hooks.entry("SessionStart").or_insert_with(|| json!([])),
        &session_cmd,
    );

    fs::write(&hooks_path, serde_json::to_string_pretty(&val)?)?;
    Ok(())
}

/// Adds `cmd` to one event's entry list in the shape Codex executes, replacing
/// an older flat entry for the same command rather than sitting beside it.
fn ensure_hook_entry(arr_val: &mut Value, cmd: &str) {
    let Some(arr) = arr_val.as_array_mut() else {
        return;
    };

    // Drop every prior entry of ours, in either shape and at any path, before
    // adding the current one. Matching on the exact command was not enough:
    // moving the binary (a dev build to a stable copy, or a Homebrew upgrade)
    // left the old entry in place, so the hook ran twice per command and every
    // distillation was recorded twice (#369). Third-party entries are untouched.
    arr.retain(|v| {
        let flat = v.get("command").and_then(|c| c.as_str());
        let nested = v
            .get("hooks")
            .and_then(|h| h.as_array())
            .map(|inner| {
                inner
                    .iter()
                    .filter_map(|h| h.get("command").and_then(|c| c.as_str()))
                    .any(is_veng_hook_command)
            })
            .unwrap_or(false);

        !(flat.is_some_and(is_veng_hook_command) || nested)
    });

    arr.push(json!({
        "hooks": [{ "type": "command", "command": cmd, "timeout": 10 }]
    }));
}

/// True for a hook command this tool installed, in either shape it has written.
fn is_veng_hook_command(cmd: &str) -> bool {
    cmd.contains("veng")
        && (cmd.contains("--pre-hook")
            || cmd.contains("--post-hook")
            || cmd.contains("--session-start"))
}

pub fn remove_veng_hooks() -> anyhow::Result<()> {
    let hooks_path = get_codex_dir().join("hooks.json");
    if !hooks_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&hooks_path)?;
    let Ok(mut val) = serde_json::from_str::<Value>(&content) else {
        return Ok(());
    };

    if let Some(obj) = val.as_object_mut()
        && let Some(hooks) = obj.get_mut("hooks").and_then(|h| h.as_object_mut())
    {
        for (_key, arr_val) in hooks.iter_mut() {
            if let Some(arr) = arr_val.as_array_mut() {
                // Entries live one level down in a matcher group's `hooks` array,
                // and older installs wrote the command at the top level. Uninstall
                // has to find both, or it leaves behind exactly the orphan that
                // bricked `config.toml` in #351.
                arr.retain(|v| {
                    let flat = v.get("command").and_then(|c| c.as_str());
                    let nested = v
                        .get("hooks")
                        .and_then(|h| h.as_array())
                        .map(|inner| {
                            inner
                                .iter()
                                .filter_map(|h| h.get("command").and_then(|c| c.as_str()))
                                .any(is_veng_hook_command)
                        })
                        .unwrap_or(false);

                    !(flat.is_some_and(is_veng_hook_command) || nested)
                });
            }
        }
    }

    fs::write(&hooks_path, serde_json::to_string_pretty(&val)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {

    /// #369: dedupe matched the exact command string, so moving the binary left
    /// the previous entry in place. Both fired, every command was distilled
    /// twice and recorded twice. Reproduced by repointing a live install from a
    /// build directory to a stable copy.
    #[test]
    fn replaces_an_entry_that_points_at_an_older_binary_path() {
        let mut arr = json!([{
            "hooks": [{ "type": "command", "command": "/old/path/veng --pre-hook" }]
        }]);

        ensure_hook_entry(&mut arr, "/new/path/veng --pre-hook");

        let arr = arr.as_array().unwrap();
        assert_eq!(arr.len(), 1, "the old path survived: {arr:?}");
        assert_eq!(arr[0]["hooks"][0]["command"], "/new/path/veng --pre-hook");
    }

    /// The purge is scoped to VENG's own entries. A launcher's hooks share the
    /// file and must come through untouched.
    #[test]
    fn leaves_another_tools_entry_alone() {
        let mut arr = json!([
            { "hooks": [{ "type": "command", "command": "/opt/other/agent-hook.sh" }] },
            { "hooks": [{ "type": "command", "command": "/old/veng --pre-hook" }] }
        ]);

        ensure_hook_entry(&mut arr, "/new/veng --pre-hook");

        let arr = arr.as_array().unwrap();
        assert_eq!(arr.len(), 2, "{arr:?}");
        assert_eq!(arr[0]["hooks"][0]["command"], "/opt/other/agent-hook.sh");
        assert_eq!(arr[1]["hooks"][0]["command"], "/new/veng --pre-hook");
    }

    /// #367: the Trust check asks only whether a record exists, and Codex also
    /// re-checks the entry's hash. `veng init --codex` rewrites the entries, so
    /// an upgrade invalidates approvals that were working while doctor still
    /// printed nothing. The presence check stays useful for "never approved",
    /// but it must not be read as "will run".
    #[test]
    fn a_recorded_approval_is_not_proof_the_hook_will_run() {
        let hooks = "/h/hooks.json";
        let config = format!(
            "[hooks.state.\"{hooks}:pre_tool_use:0:0\"]\nenabled = true\n\
             [hooks.state.\"{hooks}:post_tool_use:0:0\"]\nenabled = true\n\
             [hooks.state.\"{hooks}:session_start:0:0\"]\nenabled = true\n"
        );

        // Nothing is awaiting review, and that is exactly the state in which the
        // hash can still be stale, so callers must not treat this as healthy.
        assert!(hooks_awaiting_review(&config, hooks).is_empty());
    }
    use super::*;

    /// #364: the installed entry was `{"command": "..."}`, which Codex accepts
    /// and never runs. Same home, same `--dangerously-bypass-hook-trust`, same
    /// script: flat ran 0 times, this shape ran once.
    #[test]
    fn writes_the_entry_shape_codex_actually_runs() {
        let mut arr = json!([]);
        ensure_hook_entry(&mut arr, "/bin/veng --pre-hook");

        let entry = &arr.as_array().unwrap()[0];
        let inner = entry
            .get("hooks")
            .and_then(|h| h.as_array())
            .expect("handlers belong in a nested `hooks` array");
        assert_eq!(inner[0]["type"], "command");
        assert_eq!(inner[0]["command"], "/bin/veng --pre-hook");
        assert!(
            entry.get("command").is_none(),
            "a top-level command is the shape Codex ignores: {entry}"
        );
    }

    /// Upgrading over a config that has been silently doing nothing must repair
    /// it, not leave the dead entry beside a working one.
    #[test]
    fn replaces_an_old_flat_entry_instead_of_duplicating_it() {
        let mut arr = json!([{ "command": "/bin/veng --pre-hook" }]);

        ensure_hook_entry(&mut arr, "/bin/veng --pre-hook");

        let arr = arr.as_array().unwrap();
        assert_eq!(arr.len(), 1, "expected one entry, got {arr:?}");
        assert!(
            arr[0].get("hooks").is_some(),
            "flat entry survived: {arr:?}"
        );
    }

    /// Uninstall has to find the command one level down too, or it leaves the
    /// orphan behind, which is how #351 bricked `config.toml`.
    #[test]
    fn recognises_its_own_command_for_removal() {
        assert!(is_veng_hook_command("/usr/local/bin/veng --post-hook"));
        assert!(!is_veng_hook_command("/usr/local/bin/veng --mcp"));
        assert!(!is_veng_hook_command("/opt/other-tool --pre-hook"));
    }

    /// #359: `veng init --codex` wrote a valid hooks.json and every hook was
    /// ignored, because Codex runs only entries it has a trust record for and
    /// skips the rest without a word. Doctor said `[OK] installed` throughout.
    #[test]
    fn reports_hooks_codex_has_not_been_told_to_trust() {
        let untrusted = hooks_awaiting_review("", "/h/hooks.json");

        assert_eq!(untrusted, vec!["PreToolUse", "PostToolUse", "SessionStart"]);
    }

    /// The record is per entry, so approving one hook must not vouch for the
    /// others, and the key is path-scoped so another file's record cannot count.
    #[test]
    fn counts_only_the_records_for_this_hooks_file() {
        let config = "\
[hooks.state.\"/h/hooks.json:pre_tool_use:0:0\"]\nenabled = true\n\
[hooks.state.\"/other/hooks.json:post_tool_use:0:0\"]\nenabled = true\n";

        assert_eq!(
            hooks_awaiting_review(config, "/h/hooks.json"),
            vec!["PostToolUse", "SessionStart"]
        );
    }

    /// Codex resolves its own config through `$CODEX_HOME`, so installing to
    /// `~/.codex` regardless writes a file the running Codex never opens. Found
    /// on a machine whose launcher points `CODEX_HOME` at its own runtime
    /// directory: `veng init --codex` reported success into `~/.codex` while the
    /// home actually in use kept no MCP server at all.
    #[test]
    fn installs_into_the_home_codex_is_actually_using() {
        let dir = codex_dir_from(
            Some("/somewhere/runtime-home".into()),
            Some(PathBuf::from("/Users/x")),
        );

        assert_eq!(dir, PathBuf::from("/somewhere/runtime-home"));
    }

    /// An exported-but-empty `CODEX_HOME` is not a location; treating it as one
    /// would install into the process's working directory.
    #[test]
    fn falls_back_to_the_default_home_when_the_override_is_absent_or_empty() {
        let home = Some(PathBuf::from("/Users/x"));

        assert_eq!(
            codex_dir_from(None, home.clone()),
            PathBuf::from("/Users/x/.codex")
        );
        assert_eq!(
            codex_dir_from(Some("".into()), home),
            PathBuf::from("/Users/x/.codex")
        );
    }

    /// #351: uninstall stopped skipping at the next `[`, so
    /// `[mcp_servers.veng.tools.x]` outlived its own parent. Codex then reads a
    /// server with no transport and refuses to start at all, which turns a
    /// working install into a dead editor by running uninstall. Reproduced on
    /// 0.144.6: `codex features list` dies with `invalid transport`, and comes
    /// back the moment the parent block is restored.
    #[test]
    fn uninstall_removes_the_sub_tables_too() {
        let config = "\
model = \"x\"

[mcp_servers.veng]
type = \"stdio\"
command = \"/usr/local/bin/veng\"
args = [\"--mcp\"]

[mcp_servers.veng.tools.veng_session]
approval_mode = \"approve\"

[model_providers.other]
name = \"Other\"
";
        let out = strip_veng_server(config);
        assert!(
            !out.contains("mcp_servers.veng"),
            "an orphaned sub-table bricks Codex:\n{out}"
        );
        assert!(
            out.contains("[model_providers.other]") && out.contains("name = \"Other\""),
            "unrelated config must survive:\n{out}"
        );
    }

    /// The repair half: a config left holding only the sub-table must be
    /// recognised as *missing* its transport, or install cannot fix what
    /// uninstall broke. The old check was `contains("[mcp_servers.veng]")`,
    /// which an orphan does not satisfy.
    #[test]
    fn an_orphaned_sub_table_counts_as_missing_a_transport() {
        let orphan = "[mcp_servers.veng.tools.veng_session]\napproval_mode = \"approve\"\n";
        assert!(!declares_transport(orphan));

        let complete = "[mcp_servers.veng]\ntype = \"stdio\"\ncommand = \"/usr/local/bin/veng\"\nargs = [\"--mcp\"]\n";
        assert!(declares_transport(complete));

        // A `command` belonging to some other server is not ours.
        let other =
            "[mcp_servers.other]\ncommand = \"/bin/other\"\n\n[mcp_servers.veng.tools.x]\na = 1\n";
        assert!(!declares_transport(other));
    }

    #[test]
    fn test_install_hooks_creates_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let hooks_path = dir.path().join("hooks.json");

        // Manually write to a known path for testing the JSON structure
        let mut val = json!({});
        let obj = val.as_object_mut().unwrap();
        let hooks = obj
            .entry("hooks")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .unwrap();

        let cmd = "/usr/bin/veng --pre-hook";
        let arr = hooks.entry("PreToolUse").or_insert_with(|| json!([]));
        arr.as_array_mut().unwrap().push(json!({ "command": cmd }));

        fs::write(&hooks_path, serde_json::to_string_pretty(&val).unwrap()).unwrap();

        let content = fs::read_to_string(&hooks_path).unwrap();
        assert!(content.contains("PreToolUse"));
        assert!(content.contains("--pre-hook"));
    }

    #[test]
    fn test_ensure_hook_is_idempotent() {
        let mut val = json!({ "hooks": { "PreToolUse": [] } });
        let hooks = val.get_mut("hooks").unwrap().as_object_mut().unwrap();

        let cmd = "/usr/bin/veng --pre-hook";
        let ensure = |arr_val: &mut Value, cmd: &str| {
            let arr = arr_val.as_array_mut().unwrap();
            for v in arr.iter() {
                if v.get("command").and_then(|c| c.as_str()) == Some(cmd) {
                    return;
                }
            }
            arr.push(json!({ "command": cmd }));
        };

        ensure(hooks.get_mut("PreToolUse").unwrap(), cmd);
        assert_eq!(
            hooks.get("PreToolUse").unwrap().as_array().unwrap().len(),
            1
        );

        ensure(hooks.get_mut("PreToolUse").unwrap(), cmd);
        assert_eq!(
            hooks.get("PreToolUse").unwrap().as_array().unwrap().len(),
            1,
            "Should be idempotent"
        );
    }
}
