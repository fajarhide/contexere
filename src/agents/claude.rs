use crate::agents::AgentIntegration;
use colored::*;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

/// Which tool results Claude Code hands to the PostToolUse hook, as a regex.
/// One constant so the installer and the tests agree (#172).
const POST_TOOL_MATCHER: &str = "Bash|Read|Grep|WebFetch";

pub struct ClaudeIntegration;

impl AgentIntegration for ClaudeIntegration {
    fn tier(&self) -> crate::agents::Tier {
        crate::agents::Tier::Full
    }

    fn id(&self) -> &'static str {
        "claude"
    }

    fn name(&self) -> &'static str {
        "Claude Code"
    }

    fn install(&self, exe_path: &str) -> anyhow::Result<()> {
        install_hooks(exe_path)?;
        install_mcp_server(exe_path)?;
        crate::agent_report!(
            "  {} {} registered in .claude.json",
            "✓".green(),
            "MCP Server".bold()
        );
        Ok(())
    }

    fn uninstall(&self) -> anyhow::Result<()> {
        let settings_path = get_settings_path();
        if settings_path.exists() {
            let content = fs::read_to_string(&settings_path)?;
            if let Ok(mut val) = serde_json::from_str::<Value>(&content) {
                remove_omni_hooks(&mut val);
                fs::write(&settings_path, serde_json::to_string_pretty(&val)?)?;
                crate::agent_report!("  {} Removed Hooks from Claude settings", "✓".yellow());
            }
        }

        let mcp_path = get_claude_json_path();
        if mcp_path.exists() {
            let content = fs::read_to_string(&mcp_path)?;
            if let Ok(mut val) = serde_json::from_str::<Value>(&content) {
                let mut changed = false;

                if let Some(obj) = val.as_object_mut() {
                    if let Some(servers) = obj.get_mut("mcpServers").and_then(|v| v.as_object_mut())
                        && servers.remove("omni").is_some()
                    {
                        changed = true;
                    }

                    if let Some(projects) = obj.get_mut("projects").and_then(|p| p.as_object_mut())
                    {
                        for (_, p_val) in projects.iter_mut() {
                            if let Some(ps) =
                                p_val.get_mut("mcpServers").and_then(|s| s.as_object_mut())
                                && ps.remove("omni").is_some()
                            {
                                changed = true;
                            }
                        }
                    }

                    let top_level_keys: Vec<String> = obj.keys().cloned().collect();
                    for key in top_level_keys {
                        if key != "mcpServers"
                            && key != "projects"
                            && let Some(inner_obj) =
                                obj.get_mut(&key).and_then(|v| v.as_object_mut())
                            && let Some(ps) = inner_obj
                                .get_mut("mcpServers")
                                .and_then(|s| s.as_object_mut())
                            && ps.remove("omni").is_some()
                        {
                            changed = true;
                        }
                    }
                }

                if changed {
                    fs::write(&mcp_path, serde_json::to_string_pretty(&val)?)?;
                    crate::agent_report!("  {} Removed MCP Server from .claude.json", "✓".yellow());
                }
            }
        }

        Ok(())
    }

    fn doctor_check(&self, fix_mode: bool, warnings: &mut Vec<String>) -> bool {
        let mut all_ok = true;

        crate::agent_report!("  {}", "Claude Code:".cyan());
        let path = get_settings_path();
        if path.exists() {
            // Counts OMNI hooks, not presence: a double install ran twice per call (#454).
            if let Ok(content) = fs::read_to_string(&path)
                && let Ok(val) = serde_json::from_str::<Value>(&content)
            {
                for (event, count) in crate::agents::duplicate_omni_hooks(&val) {
                    all_ok = false;
                    crate::agent_report!(
                        "   {:<15} {} {}",
                        "Duplicate:".bright_black(),
                        format!("{event} runs OMNI {count} times per call"),
                        "[WARNING]".yellow().bold()
                    );
                    warnings.push(format!(
                        "{event} has {count} OMNI hooks. Re-run `omni init --claude` to collapse them."
                    ));
                }
            }
            if let Ok(content) = fs::read_to_string(&path) {
                if content.contains("--hook")
                    || content.contains("--post-hook")
                    || content.contains("--pre-hook")
                    || content.contains("--session-start")
                    || content.contains("--pre-compact")
                {
                    let fmt_hook = |name: &str, tag: &str| {
                        if content.contains(tag) {
                            crate::agent_report!(
                                "   {:<15} {}",
                                name.bright_black(),
                                "[OK] installed".green()
                            );
                            true
                        } else {
                            crate::agent_report!(
                                "   {:<15} {}",
                                name.bright_black(),
                                "[WARNING] missing".yellow()
                            );
                            false
                        }
                    };

                    if !fmt_hook("PreToolUse", "PreToolUse") {
                        all_ok = false;
                    }
                    if !fmt_hook("PostToolUse", "PostToolUse") {
                        all_ok = false;
                        warnings.push(
                            "PostToolUse hook is not installed. Run `omni init`.".to_string(),
                        );
                    }
                    if !fmt_hook("SessionStart", "SessionStart") {
                        all_ok = false;
                    }
                    if !fmt_hook("PreCompact", "PreCompact") {
                        all_ok = false;
                    }

                    if fix_mode && !all_ok {
                        if let Ok(exe_path) = std::env::current_exe() {
                            crate::agents::report_fix(
                                "Hooks:",
                                "missing hooks installed",
                                self.install(&exe_path.to_string_lossy()),
                                warnings,
                            );
                        }
                        all_ok = true;
                        warnings.retain(|w| {
                            !w.contains("hook") && !w.contains("Claude settings not found")
                        });
                    }

                    // After any repair, so a reinstall cannot clear a probe it never re-ran.
                    if let Ok(fresh) = fs::read_to_string(&path)
                        && let Ok(val) = serde_json::from_str::<Value>(&fresh)
                        && let Some(hook) =
                            crate::agents::probe::registered_post_hook(&val, "PostToolUse")
                        && !crate::agents::probe::report(&hook, true, warnings)
                    {
                        all_ok = false;
                    }
                } else if fix_mode {
                    if let Ok(exe_path) = std::env::current_exe() {
                        crate::agents::report_fix(
                            "Hooks:",
                            "installed",
                            self.install(&exe_path.to_string_lossy()),
                            warnings,
                        );
                    }
                } else {
                    crate::agent_report!(
                        "   {:<15} {}",
                        "Hooks:".bright_black(),
                        "[WARNING] no hooks found".yellow().bold()
                    );
                    warnings.push("OMNI hooks are not configured. Run `omni init`.".to_string());
                    all_ok = false;
                }
            }
        } else if fix_mode {
            if let Ok(exe_path) = std::env::current_exe() {
                crate::agents::report_fix(
                    "Hooks:",
                    "installed",
                    self.install(&exe_path.to_string_lossy()),
                    warnings,
                );
            }
        } else {
            crate::agent_report!(
                "   {:<15} {}",
                "Hooks:".bright_black(),
                "[ERROR] settings.json missing".red()
            );
            warnings.push("Claude settings not found. Have you installed Claude Code?".to_string());
            all_ok = false;
        }

        let mcp_path = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Library/Application Support/Claude/claude_desktop_config.json");
        let mcpa_path = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".claude.json");

        let mut mcp_found = false;
        for p in &[mcp_path, mcpa_path] {
            if p.exists()
                && let Ok(c) = fs::read_to_string(p)
                && (c.contains("omni --mcp") || c.contains("\"omni\":"))
            {
                mcp_found = true;
                crate::agent_report!(
                    "   {:<15} {} {}",
                    "MCP Server:".bright_black(),
                    p.display().to_string().bright_black(),
                    "[OK]".green().bold()
                );
                break;
            }
        }
        if !mcp_found {
            // Absence is a state, not a fault (#757). The hooks do the shortening; the
            // MCP server costs prefix bytes and a cache reset on connect, so leaving it
            // out is a choice that `--fix` must not undo.
            let _ = fix_mode;
            crate::agent_report!(
                "   {:<15} {}  {}",
                "MCP Server:".bright_black(),
                "[not registered]".bright_black(),
                "optional here, `omni init --mcp` adds it".bright_black()
            );
        }

        all_ok
    }
}

pub fn get_settings_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude/settings.json")
}

pub fn get_claude_json_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".claude.json")
}

pub fn backup_settings(path: &PathBuf) -> anyhow::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let backup_path = path.with_extension("json.bak");
    fs::copy(path, backup_path)?;
    Ok(())
}

pub fn initialize_settings() -> anyhow::Result<(PathBuf, Value)> {
    let path = get_settings_path();

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut val = if path.exists() {
        let content = fs::read_to_string(&path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    install_omni_hooks(&mut val, ""); // Temp to ensure object exists
    Ok((path, val))
}

pub fn check_status(val: &Value, exe_path: &str) -> (bool, bool, bool) {
    let hooks = match val.get("hooks").and_then(|v| v.as_object()) {
        Some(h) => h,
        None => return (false, false, false),
    };

    let check = |event: &str| -> bool {
        if let Some(arr) = hooks.get(event).and_then(|v| v.as_array()) {
            for v in arr {
                if let Some(inner_arr) = v.get("hooks").and_then(|v2| v2.as_array()) {
                    for hook_def in inner_arr {
                        if let Some(cmd) = hook_def.get("command").and_then(|c| c.as_str())
                            && cmd.contains(exe_path)
                            && (cmd.contains("--hook")
                                || cmd.contains("--post-hook")
                                || cmd.contains("--pre-hook")
                                || cmd.contains("--session-start")
                                || cmd.contains("--pre-compact"))
                        {
                            return true;
                        }
                    }
                }
            }
        }
        false
    };

    (
        check("PostToolUse"),
        check("SessionStart"),
        check("PreCompact"),
    )
}

pub fn remove_omni_hooks(val: &mut Value) {
    if let Some(obj) = val.as_object_mut()
        && let Some(hooks) = obj.get_mut("hooks").and_then(|h| h.as_object_mut())
    {
        for (_key, arr_val) in hooks.iter_mut() {
            if let Some(arr) = arr_val.as_array_mut() {
                arr.retain(|v| {
                    if let Some(inner) = v.get("hooks").and_then(|h| h.as_array()) {
                        !inner.iter().any(|h| {
                            h.get("command").and_then(|c| c.as_str()).is_some_and(|c| {
                                c.contains("omni")
                                    && (c.contains("--hook")
                                        || c.contains("--post-hook")
                                        || c.contains("--pre-hook")
                                        || c.contains("--session-start")
                                        || c.contains("--pre-compact"))
                            })
                        })
                    } else {
                        true
                    }
                });
            }
        }
    }
}

pub fn install_omni_hooks(val: &mut Value, exe_path: &str) {
    let obj = match val.as_object_mut() {
        Some(o) => o,
        None => {
            *val = json!({});
            val.as_object_mut().unwrap()
        }
    };

    let hooks = obj
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .unwrap();

    if exe_path.is_empty() {
        return;
    }

    let ensure_hook = |arr_val: &mut serde_json::Value, matcher: &str, hook_cmd: &str| {
        let arr = arr_val.as_array_mut().unwrap();
        for v in arr.iter_mut() {
            let installed = v
                .get("hooks")
                .and_then(|h| h.as_array())
                .is_some_and(|inner| {
                    inner.iter().any(|h| {
                        crate::agents::is_our_hook(
                            h.get("command").and_then(|c| c.as_str()),
                            hook_cmd,
                        )
                    })
                });
            if installed {
                // Update the path too, or a reinstall from another binary appends (#454).
                if let Some(inner) = v
                    .get_mut("hooks")
                    .and_then(|h| h.as_array_mut())
                    .and_then(|inner| {
                        inner.iter_mut().find(|h| {
                            crate::agents::is_our_hook(
                                h.get("command").and_then(|c| c.as_str()),
                                hook_cmd,
                            )
                        })
                    })
                    .and_then(|h| h.as_object_mut())
                {
                    inner.insert("command".to_string(), json!(hook_cmd));
                }
                // Already installed, but an old matcher still has to be widened (#172).
                if let Some(obj) = v.as_object_mut() {
                    obj.insert("matcher".to_string(), json!(matcher));
                }
                return;
            }
        }

        arr.push(json!({
            "matcher": matcher,
            "hooks": [
                {
                    "type": "command",
                    "command": hook_cmd
                }
            ]
        }));
    };

    let ensure_async_hook = |arr_val: &mut serde_json::Value, hook_cmd: &str| {
        let arr = arr_val.as_array_mut().unwrap();
        for v in arr.iter_mut() {
            if let Some(inner) = v.get_mut("hooks").and_then(|h| h.as_array_mut()) {
                for h in inner.iter_mut() {
                    if crate::agents::is_our_hook(
                        h.get("command").and_then(|c| c.as_str()),
                        hook_cmd,
                    ) {
                        if let Some(obj) = h.as_object_mut() {
                            obj.insert("command".to_string(), json!(hook_cmd));
                        }
                        return;
                    }
                }
            }
        }
        arr.push(json!({
            "matcher": "",
            "hooks": [{
                "type": "command",
                "command": hook_cmd,
                "async": true
            }]
        }));
    };

    let pre_cmd = format!("{} --pre-hook", exe_path);
    let post_cmd = format!("{} --post-hook", exe_path);
    let session_cmd = format!("{} --session-start", exe_path);
    let compact_cmd = format!("{} --pre-compact", exe_path);
    let hook_cmd = format!("{} --hook", exe_path);

    ensure_hook(
        hooks.entry("PreToolUse").or_insert_with(|| json!([])),
        "Bash",
        &pre_cmd,
    );
    // `Bash|Read|Grep|WebFetch` (#172). Not free: the 7.6% of `Read` calls over
    // `MIN_DISTILL_TOKENS` hold 44% of `Read` bytes, so this changes the reads
    // that matter. Taken knowingly; everything cut is archived (#273).
    ensure_hook(
        hooks.entry("PostToolUse").or_insert_with(|| json!([])),
        POST_TOOL_MATCHER,
        &post_cmd,
    );
    ensure_hook(
        hooks.entry("SessionStart").or_insert_with(|| json!([])),
        "",
        &session_cmd,
    );
    ensure_hook(
        hooks.entry("PreCompact").or_insert_with(|| json!([])),
        "",
        &compact_cmd,
    );

    // Async hooks; no output needed.
    ensure_async_hook(
        hooks.entry("SessionEnd").or_insert_with(|| json!([])),
        &hook_cmd,
    );
    ensure_async_hook(
        hooks
            .entry("PostToolUseFailure")
            .or_insert_with(|| json!([])),
        &hook_cmd,
    );
    ensure_hook(
        hooks.entry("SubagentStart").or_insert_with(|| json!([])),
        "",
        &session_cmd,
    );
    ensure_async_hook(
        hooks.entry("FileChanged").or_insert_with(|| json!([])),
        &hook_cmd,
    );
}

/// The half of the install that does the work. Separate from MCP registration
/// so `omni init --hook` installs only hooks, as it always promised (#757).
pub fn install_hooks(exe_path: &str) -> anyhow::Result<()> {
    let (path, mut val) = initialize_settings()?;
    let _ = backup_settings(&path);

    install_omni_hooks(&mut val, exe_path);
    fs::write(&path, serde_json::to_string_pretty(&val)?)?;
    crate::agent_report!(
        "  {} {} installed in Claude settings",
        "✓".green(),
        "Hooks".bold()
    );
    Ok(())
}

pub fn install_mcp_server(exe_path: &str) -> anyhow::Result<()> {
    let path = get_claude_json_path();
    let mut val = if path.exists() {
        let content = fs::read_to_string(&path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    let obj = val
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("Invalid .claude.json format"))?;

    let servers = obj
        .entry("mcpServers")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow::anyhow!("mcpServers is not an object"))?;

    servers.insert(
        "omni".to_string(),
        json!({
            "type": "stdio",
            "command": exe_path,
            "args": ["--mcp"],
            "env": {
                "OMNI_AGENT_ID": "claude_code"
            },
        }),
    );

    if let Some(projects) = obj.get_mut("projects").and_then(|p| p.as_object_mut()) {
        for (_path, p_val) in projects.iter_mut() {
            if let Some(ps) = p_val.get_mut("mcpServers").and_then(|s| s.as_object_mut())
                && ps.contains_key("omni")
            {
                ps.insert(
                    "omni".to_string(),
                    json!({
                        "type": "stdio",
                        "command": exe_path,
                        "args": ["--mcp"],
                        "env": {
                            "OMNI_AGENT_ID": "claude_code"
                        },
                    }),
                );
            }
        }
    }

    let top_level_keys: Vec<String> = obj.keys().cloned().collect();
    for key in top_level_keys {
        if key != "mcpServers"
            && key != "projects"
            && let Some(inner_obj) = obj.get_mut(&key).and_then(|v| v.as_object_mut())
            && let Some(ps) = inner_obj
                .get_mut("mcpServers")
                .and_then(|s| s.as_object_mut())
            && ps.contains_key("omni")
        {
            ps.insert(
                "omni".to_string(),
                json!({
                    "type": "stdio",
                    "command": exe_path,
                    "args": ["--mcp"],
                    "env": {
                        "OMNI_AGENT_ID": "claude_code"
                    },
                }),
            );
        }
    }

    fs::write(&path, serde_json::to_string_pretty(&val)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn reinstalling_from_another_path_moves_the_hook_rather_than_adding_one() {
        let mut val = json!({});
        install_omni_hooks(&mut val, "/repo/target/debug/omni");
        install_omni_hooks(&mut val, "/opt/homebrew/bin/omni");

        let hooks = val["hooks"].as_object().expect("hooks written");
        assert!(!hooks.is_empty(), "no hooks were written at all");
        for (event, arr) in hooks {
            let ours: Vec<&str> = arr
                .as_array()
                .expect("event is an array")
                .iter()
                .flat_map(|m| m["hooks"].as_array().cloned().unwrap_or_default())
                .filter_map(|h| h["command"].as_str().map(str::to_string))
                .filter(|c| c.contains("omni"))
                .map(|c| Box::leak(c.into_boxed_str()) as &str)
                .collect();
            assert_eq!(
                ours.len(),
                1,
                "{event} carries {} OMNI hooks, so OMNI would run {} times: {ours:?}",
                ours.len(),
                ours.len()
            );
            assert!(
                ours[0].starts_with("/opt/homebrew/bin/omni"),
                "{event} still points at the old path: {ours:?}"
            );
        }
    }

    #[test]
    fn tells_the_hook_flags_apart_even_when_one_ends_with_another() {
        assert!(crate::agents::is_our_hook(
            Some("/a/omni --pre-hook"),
            "/b/omni --pre-hook"
        ));
        assert!(!crate::agents::is_our_hook(
            Some("/a/omni --pre-hook"),
            "/b/omni --hook"
        ));
        assert!(!crate::agents::is_our_hook(
            Some("/a/omni --post-hook"),
            "/b/omni --hook"
        ));
        assert!(!crate::agents::is_our_hook(
            Some("/usr/bin/other --pre-hook"),
            "/b/omni --pre-hook"
        ));
        assert!(!crate::agents::is_our_hook(None, "/b/omni --pre-hook"));
    }

    use super::*;

    #[test]
    fn test_init_hook_creates_valid_settings_json() {
        let mut val = json!({});
        install_omni_hooks(&mut val, "/usr/bin/omni");

        let hooks = val.get("hooks").unwrap().as_object().unwrap();
        assert!(hooks.contains_key("PostToolUse"));
        assert!(hooks.contains_key("SessionStart"));
        assert!(hooks.contains_key("PreCompact"));
    }

    #[test]
    fn test_init_hook_idempotent_run_2x_not_duplicate() {
        let mut val = json!({});
        install_omni_hooks(&mut val, "/usr/bin/omni");

        let get_count = |v: &Value| -> usize {
            v.get("hooks")
                .unwrap()
                .get("PostToolUse")
                .unwrap()
                .as_array()
                .unwrap()
                .len()
        };

        assert_eq!(get_count(&val), 1);

        install_omni_hooks(&mut val, "/usr/bin/omni");
        assert_eq!(get_count(&val), 1, "Should be idempotent");
    }

    #[test]
    fn registers_post_tool_for_the_tools_it_can_distil() {
        let mut val = json!({});
        install_omni_hooks(&mut val, "/usr/bin/omni");

        let matcher = val["hooks"]["PostToolUse"][0]["matcher"]
            .as_str()
            .expect("a string matcher");

        for tool in ["Bash", "Read", "Grep", "WebFetch"] {
            assert!(
                matcher.contains(tool),
                "{tool} results never reach the hook: matcher is {matcher:?}"
            );
        }
    }

    #[test]
    fn brings_an_existing_narrow_matcher_up_to_date() {
        let mut val = json!({
            "hooks": {
                "PostToolUse": [{
                    "matcher": "Bash",
                    "hooks": [{"type": "command", "command": "/usr/bin/omni --post-hook"}]
                }]
            }
        });

        install_omni_hooks(&mut val, "/usr/bin/omni");

        assert_eq!(
            val["hooks"]["PostToolUse"][0]["matcher"].as_str(),
            Some(POST_TOOL_MATCHER),
            "an existing install must be migrated, not left behind"
        );
        assert_eq!(
            val["hooks"]["PostToolUse"].as_array().unwrap().len(),
            1,
            "migrating must not duplicate the registration"
        );
    }

    #[test]
    fn test_init_status_reports_expected_status() {
        let mut val = json!({});
        let exe = "/usr/bin/omni";
        install_omni_hooks(&mut val, exe);

        let (post, sess, pre) = check_status(&val, exe);
        assert!(post && sess && pre);

        let (post_f, sess_f, pre_f) = check_status(&val, "/different/omni");
        assert!(!post_f && !sess_f && !pre_f);
    }

    #[test]
    fn test_init_hook_writes_matcher_for_all_events() {
        let mut val = json!({});
        install_omni_hooks(&mut val, "/usr/bin/omni");

        let hooks = val.get("hooks").unwrap().as_object().unwrap();
        for (event, entries) in hooks {
            for entry in entries.as_array().unwrap() {
                let matcher = entry.get("matcher");
                assert!(
                    matches!(matcher, Some(v) if v.is_string()),
                    "{event} entry missing string matcher: {entry}"
                );
            }
        }
    }

    #[test]
    fn test_init_uninstall_removes_all_entries() {
        let mut val = json!({});
        let exe = "/usr/bin/omni";
        install_omni_hooks(&mut val, exe);

        assert!(check_status(&val, exe).0); // terpasang

        remove_omni_hooks(&mut val);

        assert!(!check_status(&val, exe).0); // hilang

        let arr = val
            .get("hooks")
            .unwrap()
            .get("PostToolUse")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(
            arr.len(),
            0,
            "Array must be empty after retain cleans it out"
        );
    }
}
