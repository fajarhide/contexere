use crate::agents::AgentIntegration;
use colored::*;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

/// Which tool results Claude Code hands to the PostToolUse hook.
///
/// Claude Code matches this as a regex, so alternation is how one registration
/// covers several tools. It is a constant because `install_veng_hooks` writes it
/// and the tests assert on it, and a widened matcher that only half the code
/// knows about is worse than a narrow one (#172).
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
                remove_veng_hooks(&mut val);
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
                        && servers.remove("veng").is_some()
                    {
                        changed = true;
                    }

                    if let Some(projects) = obj.get_mut("projects").and_then(|p| p.as_object_mut())
                    {
                        for (_, p_val) in projects.iter_mut() {
                            if let Some(ps) =
                                p_val.get_mut("mcpServers").and_then(|s| s.as_object_mut())
                                && ps.remove("veng").is_some()
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
                            && ps.remove("veng").is_some()
                        {
                            changed = true;
                        }
                    }
                }

                changed |= remove_legacy_mcp(&mut val) > 0;
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
            // An install that registered VENG twice ran the pipeline twice per
            // call and reported [OK] throughout, because this check asked
            // whether a hook was present and never how many (#454). The
            // installer no longer creates that state; this is what tells the
            // machines already in it.
            if let Ok(content) = fs::read_to_string(&path)
                && let Ok(val) = serde_json::from_str::<Value>(&content)
            {
                for (event, count) in crate::agents::duplicate_veng_hooks(&val) {
                    all_ok = false;
                    crate::agent_report!(
                        "   {:<15} {} {}",
                        "Duplicate:".bright_black(),
                        format!("{event} runs VENG {count} times per call"),
                        "[WARNING]".yellow().bold()
                    );
                    warnings.push(format!(
                        "{event} has {count} VENG hooks. Re-run `veng init --claude` to collapse them."
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
                            "PostToolUse hook is not installed. Run `veng init`.".to_string(),
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
                    warnings.push("VENG hooks are not configured. Run `veng init`.".to_string());
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
                && (c.contains("veng --mcp") || c.contains("\"veng\":") || c.contains("omni --mcp"))
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
            // #757. Absence is a state here, not a fault. The hooks are what
            // shortens output on this host; the MCP server is a convenience whose
            // two tool definitions sit in the prefix of every request, and the
            // host discards the whole prompt cache when an MCP server connects or
            // disconnects with its tools loaded. Someone who ran
            // `veng init --hook`, or who removed the entry on purpose after
            // reading that, was being told their install was broken and then
            // having the decision undone by the next `doctor --fix`.
            //
            // So it reports, and says how to add it, and neither fails the check
            // nor repairs what nobody broke.
            let _ = fix_mode;
            crate::agent_report!(
                "   {:<15} {}  {}",
                "MCP Server:".bright_black(),
                "[not registered]".bright_black(),
                "optional here, `veng init --mcp` adds it".bright_black()
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

    install_veng_hooks(&mut val, ""); // Temp to ensure object exists
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

pub fn remove_veng_hooks(val: &mut Value) {
    if let Some(obj) = val.as_object_mut()
        && let Some(hooks) = obj.get_mut("hooks").and_then(|h| h.as_object_mut())
    {
        for (_key, arr_val) in hooks.iter_mut() {
            if let Some(arr) = arr_val.as_array_mut() {
                arr.retain(|v| {
                    if let Some(inner) = v.get("hooks").and_then(|h| h.as_array()) {
                        !inner.iter().any(|h| {
                            h.get("command").and_then(|c| c.as_str()).is_some_and(|c| {
                                super::names_us(c)
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

pub fn install_veng_hooks(val: &mut Value, exe_path: &str) {
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
                // Bring the path up to date as well as the matcher. Matching on
                // the exact string meant a reinstall from a different binary
                // matched nothing and pushed a second entry, so VENG ran twice
                // per call (#454).
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
                // Already installed, but the matcher still has to be brought up to
                // date. Returning here unconditionally is why widening it in #172
                // would have reached new installs only: every existing settings
                // file names `Bash` and nothing would have rewritten it.
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

    // Core hooks (blocking)
    ensure_hook(
        hooks.entry("PreToolUse").or_insert_with(|| json!([])),
        "Bash",
        &pre_cmd,
    );
    // `Bash|Read|Grep|WebFetch`, widened on the maintainer's call (#172).
    //
    // The `Read`/`Grep`/`WebFetch` arms in `hooks::post_tool` had been written,
    // gated and tested since the Rust rewrite and had **never executed on Claude
    // Code**, because this matcher named one tool. What that widening does was
    // measured before it was taken, and it is not free: driven through the built
    // binary's `--post-hook` with a real payload, `src/pipeline/collapse.rs`, 878
    // lines, comes back as **20** of them, an import list, three signatures and a
    // marker. `readfile.rs`'s only floor is `MIN_DISTILL_TOKENS = 2000`, and 7.6%
    // of 1,770 real `Read` calls clear it, `.rs` only 3.1%. An earlier version of
    // this comment said "nearly every real source file", which is a
    // generalisation from one example that the 0.6.8 changelog had already
    // measured and withdrawn (#284). The figure that carries the decision is the
    // other half of the same measurement: those 7.6% of calls hold 44% of all
    // `Read` bytes, so this changes what "read a file" means on exactly the reads
    // that matter, on the tool an agent edits from. The maintainer chose that
    // trade with those numbers in front of them.
    //
    // Three prerequisites landed first, each of which would have made this fail
    // silently rather than loudly: `normalize` could not reach a `Read` payload's
    // text at all (`file.content` matched no arm), `tool_input.file_path` was
    // never read (so the distiller saw `"unknown"` and could not pick a
    // language), and `shape_for_host` had no `Read` shape. #246 stopped the
    // `readfile` path reporting a document as a clean log, and #273 made these
    // arms archive and mark what they cut, so nothing they drop is unrecoverable.
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

    //  New hooks (async, non-blocking, no output needed)
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

/// The half of the install that does the work.
///
/// Separate from the MCP registration because `veng init --hook` has promised
/// "Only install hooks" since it shipped and installed both anyway, which is the
/// shape of #151: a flag the parser accepts and the code ignores. On this host
/// the hooks are what shortens output and the MCP server is a convenience whose
/// tool definitions sit in the prefix of every request, so wanting one without
/// the other is an ordinary preference and not a broken install (#757).
pub fn install_hooks(exe_path: &str) -> anyhow::Result<()> {
    let (path, mut val) = initialize_settings()?;
    let _ = backup_settings(&path);

    install_veng_hooks(&mut val, exe_path);
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
        "veng".to_string(),
        json!({
            "type": "stdio",
            "command": exe_path,
            "args": ["--mcp"],
            "env": {
                "VENG_AGENT_ID": "claude_code"
            },
        }),
    );
    super::drop_omni_server(servers);

    if let Some(projects) = obj.get_mut("projects").and_then(|p| p.as_object_mut()) {
        for (_path, p_val) in projects.iter_mut() {
            if let Some(ps) = p_val.get_mut("mcpServers").and_then(|s| s.as_object_mut())
                && (super::drop_omni_server(ps) | ps.contains_key("veng"))
            {
                ps.insert(
                    "veng".to_string(),
                    json!({
                        "type": "stdio",
                        "command": exe_path,
                        "args": ["--mcp"],
                        "env": {
                            "VENG_AGENT_ID": "claude_code"
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
            && (super::drop_omni_server(ps) | ps.contains_key("veng"))
        {
            ps.insert(
                "veng".to_string(),
                json!({
                    "type": "stdio",
                    "command": exe_path,
                    "args": ["--mcp"],
                    "env": {
                        "VENG_AGENT_ID": "claude_code"
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

    /// #876: `veng init` on a machine set up by omni replaces omni's hooks in
    /// place. Left beside ours, both would distil every call.
    #[test]
    fn installing_over_an_omni_install_replaces_its_hooks() {
        let mut val = json!({});
        install_veng_hooks(&mut val, "/opt/homebrew/bin/omni");
        install_veng_hooks(&mut val, "/opt/homebrew/bin/veng");
        let text = val.to_string();
        assert!(!text.contains("bin/omni"), "{text}");
        assert!(
            crate::agents::duplicate_veng_hooks(&val).is_empty(),
            "{text}"
        );
    }
    /// #454, found on a real machine: a working install was repointed from a
    /// development build to the released one, and every VENG hook ended up
    /// registered twice, once per path. Two VENG processes per hooked call, and
    /// `veng doctor` reported `[OK]` throughout because it asks whether a hook is
    /// present, never how many.
    #[test]
    fn reinstalling_from_another_path_moves_the_hook_rather_than_adding_one() {
        let mut val = json!({});
        install_veng_hooks(&mut val, "/repo/target/debug/veng");
        install_veng_hooks(&mut val, "/opt/homebrew/bin/veng");

        let hooks = val["hooks"].as_object().expect("hooks written");
        assert!(!hooks.is_empty(), "no hooks were written at all");
        for (event, arr) in hooks {
            let ours: Vec<&str> = arr
                .as_array()
                .expect("event is an array")
                .iter()
                .flat_map(|m| m["hooks"].as_array().cloned().unwrap_or_default())
                .filter_map(|h| h["command"].as_str().map(str::to_string))
                .filter(|c| c.contains("veng"))
                .map(|c| Box::leak(c.into_boxed_str()) as &str)
                .collect();
            assert_eq!(
                ours.len(),
                1,
                "{event} carries {} VENG hooks, so VENG would run {} times: {ours:?}",
                ours.len(),
                ours.len()
            );
            assert!(
                ours[0].starts_with("/opt/homebrew/bin/veng"),
                "{event} still points at the old path: {ours:?}"
            );
        }
    }

    /// The identity test is the binary plus the flag, and the flag is compared as
    /// a whole token. `--pre-hook` ends with `-hook`, so a `contains` would make
    /// the two the same hook and one would overwrite the other.
    #[test]
    fn tells_the_hook_flags_apart_even_when_one_ends_with_another() {
        assert!(crate::agents::is_our_hook(
            Some("/a/veng --pre-hook"),
            "/b/veng --pre-hook"
        ));
        assert!(!crate::agents::is_our_hook(
            Some("/a/veng --pre-hook"),
            "/b/veng --hook"
        ));
        assert!(!crate::agents::is_our_hook(
            Some("/a/veng --post-hook"),
            "/b/veng --hook"
        ));
        assert!(!crate::agents::is_our_hook(
            Some("/usr/bin/other --pre-hook"),
            "/b/veng --pre-hook"
        ));
        assert!(!crate::agents::is_our_hook(None, "/b/veng --pre-hook"));
    }

    use super::*;

    #[test]
    fn test_init_hook_creates_valid_settings_json() {
        let mut val = json!({});
        install_veng_hooks(&mut val, "/usr/bin/veng");

        let hooks = val.get("hooks").unwrap().as_object().unwrap();
        assert!(hooks.contains_key("PostToolUse"));
        assert!(hooks.contains_key("SessionStart"));
        assert!(hooks.contains_key("PreCompact"));
    }

    #[test]
    fn test_init_hook_idempotent_run_2x_not_duplicate() {
        let mut val = json!({});
        install_veng_hooks(&mut val, "/usr/bin/veng");

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

        install_veng_hooks(&mut val, "/usr/bin/veng");
        assert_eq!(get_count(&val), 1, "Should be idempotent");
    }

    /// #172. The `Read`, `Grep` and `WebFetch` arms in `hooks::post_tool` had
    /// been written, gated and tested since the Rust rewrite and had never once
    /// executed on Claude Code, because this matcher named a single tool.
    #[test]
    fn registers_post_tool_for_the_tools_it_can_distil() {
        let mut val = json!({});
        install_veng_hooks(&mut val, "/usr/bin/veng");

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

    /// An install from before #172 names `Bash` and would never be rewritten,
    /// because `ensure_hook` returned as soon as it recognised the command. Every
    /// existing user would have kept the narrow matcher and seen no change at
    /// all, which is the quietest way for a fix to not ship.
    #[test]
    fn brings_an_existing_narrow_matcher_up_to_date() {
        let mut val = json!({
            "hooks": {
                "PostToolUse": [{
                    "matcher": "Bash",
                    "hooks": [{"type": "command", "command": "/usr/bin/veng --post-hook"}]
                }]
            }
        });

        install_veng_hooks(&mut val, "/usr/bin/veng");

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
        let exe = "/usr/bin/veng";
        install_veng_hooks(&mut val, exe);

        // Check status with correct path
        let (post, sess, pre) = check_status(&val, exe);
        assert!(post && sess && pre);

        // Check status with incorrect path
        let (post_f, sess_f, pre_f) = check_status(&val, "/different/veng");
        assert!(!post_f && !sess_f && !pre_f);
    }

    #[test]
    fn test_init_hook_writes_matcher_for_all_events() {
        let mut val = json!({});
        install_veng_hooks(&mut val, "/usr/bin/veng");

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
        let exe = "/usr/bin/veng";
        install_veng_hooks(&mut val, exe);

        assert!(check_status(&val, exe).0); // terpasang

        remove_veng_hooks(&mut val);

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

// ─── Leftovers from omni ─────────────────────────────────────────────────────
//
// veng replaced omni. A machine that had omni installed still has omni's hooks
// and MCP entry in Claude Code's files, and `veng migrate` has to find exactly
// those and nothing else.

/// The flags omni's hooks were registered with.
const LEGACY_HOOK_FLAGS: &[&str] = &[
    "--hook",
    "--post-hook",
    "--pre-hook",
    "--session-start",
    "--pre-compact",
];

fn is_legacy_hook(hook: &Value) -> bool {
    hook.get("command")
        .and_then(|c| c.as_str())
        .is_some_and(|c| {
            super::runs_omni_binary(c)
                && c.split_whitespace()
                    .any(|token| LEGACY_HOOK_FLAGS.contains(&token))
        })
}

/// Remove omni's hook entries from a settings object, returning how many went.
pub fn remove_legacy_hooks(val: &mut Value) -> usize {
    let mut removed = 0;
    if let Some(hooks) = val.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        for (_event, entries) in hooks.iter_mut() {
            if let Some(arr) = entries.as_array_mut() {
                arr.retain(|entry| {
                    let legacy = entry
                        .get("hooks")
                        .and_then(|h| h.as_array())
                        .is_some_and(|inner| inner.iter().any(is_legacy_hook));
                    if legacy {
                        removed += 1;
                    }
                    !legacy
                });
            }
        }
    }
    removed
}

/// Remove an `omni` MCP server from `.claude.json`, wherever Claude Code keeps
/// one (global, per project, or under another top-level key). Only an entry
/// whose command really is the omni binary goes: a server someone else named
/// `omni` is theirs.
pub fn remove_legacy_mcp(val: &mut Value) -> usize {
    fn drop_from(servers: Option<&mut Value>) -> usize {
        servers
            .and_then(|s| s.as_object_mut())
            .is_some_and(super::drop_omni_server)
            .into()
    }

    let Some(obj) = val.as_object_mut() else {
        return 0;
    };
    let mut removed = drop_from(obj.get_mut("mcpServers"));
    if let Some(projects) = obj.get_mut("projects").and_then(|p| p.as_object_mut()) {
        for (_path, project) in projects.iter_mut() {
            removed += drop_from(project.get_mut("mcpServers"));
        }
    }
    for (key, inner) in obj.iter_mut() {
        if key != "mcpServers" && key != "projects" {
            removed += drop_from(inner.get_mut("mcpServers"));
        }
    }
    removed
}

#[cfg(test)]
mod legacy_tests {
    use super::*;

    #[test]
    fn removes_only_omni_hooks_and_keeps_veng_and_user_hooks() {
        let mut val = json!({ "hooks": { "PostToolUse": [
            { "hooks": [{ "type": "command", "command": "/usr/bin/omni --post-hook" }] },
            { "hooks": [{ "type": "command", "command": "/usr/bin/veng --post-hook" }] },
            { "hooks": [{ "type": "command", "command": "/home/me/bin/lint.sh" }] }
        ]}});

        assert_eq!(remove_legacy_hooks(&mut val), 1);

        let left = val["hooks"]["PostToolUse"].as_array().expect("array");
        assert_eq!(left.len(), 2);
        let text = val.to_string();
        assert!(text.contains("veng --post-hook") && text.contains("lint.sh"));
        assert!(!text.contains("omni"), "{text}");
    }

    #[test]
    fn a_path_that_merely_contains_omni_is_not_omni() {
        let mut val = json!({ "hooks": { "PostToolUse": [
            { "hooks": [{ "type": "command", "command": "/home/omni/bin/veng --post-hook" }] }
        ]}});

        assert_eq!(remove_legacy_hooks(&mut val), 0);
        assert_eq!(
            val["hooks"]["PostToolUse"].as_array().expect("array").len(),
            1
        );
    }

    #[test]
    fn removes_the_mcp_entry_in_all_three_places() {
        let omni = json!({ "type": "stdio", "command": "/usr/local/bin/omni", "args": ["--mcp"] });
        let mut val = json!({
            "mcpServers": { "omni": omni.clone(), "other": { "command": "/bin/other" } },
            "projects": { "/work/a": { "mcpServers": { "omni": omni.clone() } } },
            "extra": { "mcpServers": { "omni": omni } }
        });

        assert_eq!(remove_legacy_mcp(&mut val), 3);

        let text = val.to_string();
        assert!(!text.contains("omni"), "{text}");
        assert!(
            text.contains("/bin/other"),
            "an unrelated server was removed: {text}"
        );
    }

    #[test]
    fn a_server_someone_else_named_omni_is_left_alone() {
        let mut val = json!({ "mcpServers": { "omni": { "command": "/opt/their-tool" } } });

        assert_eq!(remove_legacy_mcp(&mut val), 0);
        assert!(val["mcpServers"]["omni"].is_object());
    }
}
