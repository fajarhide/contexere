use colored::*;
use serde_json::Value;
use std::env;
use std::fs;
use std::io::IsTerminal;

/// Read by `print_help` and `super::check_flags` (#151). The first
/// `AGENT_FLAGS` entries are the agents; the rest are Claude-specific, which is
/// how help groups them.
const FLAGS: super::Flags = &[
    ("--claude", "Configure Claude Code (Anthropic)"),
    ("--cursor", "Configure Cursor AI"),
    ("--zed", "Configure Zed Editor"),
    ("--cline", "Configure Cline"),
    ("--roo", "Configure Roo Code"),
    ("--roo-code", "Configure Roo Code (alias)"),
    ("--copilot", "Configure GitHub Copilot CLI"),
    ("--gemini", "Configure Gemini CLI"),
    ("--opencode", "Configure OpenCode"),
    ("--codex", "Configure Codex CLI"),
    ("--openclaw", "Configure OpenClaw"),
    (
        "--antigravity",
        "Configure Antigravity IDE / Generic Webhook",
    ),
    ("--hermes", "Configure Hermes Agent"),
    ("--vscode", "Configure VS Code (MCP)"),
    ("--pi", "Configure Pi Agent"),
    (
        "--all",
        "Every host above, not just Claude. Writes .vscode/mcp.json in the current directory",
    ),
    ("--hook", "Only install hooks"),
    ("--mcp", "Only register MCP server"),
    ("--status", "Check current installation status"),
    ("--uninstall", "Remove VENG hooks and MCP server"),
];

/// Where `FLAGS` stops listing agents and starts listing Claude-specific flags.
///
// ponytail: hand-maintained split index rather than two lists, because
// `check_flags` wants one flat list and a second const would mean teaching it to
// take several. `splits_flags_between_the_two_help_groups` below is what keeps
// it honest. Upgrade path: separate consts + a `check_flags` that accepts a
// slice of lists, worth it the moment a third group appears.
const AGENT_FLAGS: usize = 15;

fn print_help() {
    println!(
        "\n{} {}: Setup VENG for your preferred AI Agent",
        "veng".bold().cyan(),
        "init".bold().yellow()
    );
    println!("\n{}", "USAGE:".bold().bright_white());
    println!("  veng {}", "init [FLAGS]".cyan());

    let entries: Vec<_> = FLAGS
        .iter()
        .chain(std::iter::once(&super::HELP_FLAG))
        .collect();
    super::print_flag_group("SUPPORTED AGENTS:", &entries[..AGENT_FLAGS]);
    // `--all` sits at the head of this group and is the one flag in it that is
    // not Claude-specific: it configures every host above. It was documented as
    // "full Claude setup" while doing exactly that, which is the class of defect
    // this project files issues about, so the group is named for what it holds
    // rather than for what most of it does (#455).
    super::print_flag_group(
        "EVERY HOST, AND CLAUDE SPECIFIC FLAGS:",
        &entries[AGENT_FLAGS..],
    );

    println!("\n{}", "EXAMPLES:".bold().bright_white());
    println!(
        "  veng init             {}",
        "# Interactive menu".bright_black()
    );
    println!(
        "  veng init --claude    {}",
        "# Setup for Claude Code".bright_black()
    );
    println!(
        "  veng init --all       {}",
        "# Every host, including a .vscode/mcp.json here".bright_black()
    );
    println!();
}

/// The `init` target for a host `detect_agent_id` recognises, or `None` when
/// there is no integration to point it at.
///
/// Two id vocabularies exist and neither can be derived from the other:
/// `detect_agent_id` names agents so `veng stats` can group rows, and `init`
/// names install targets. `windsurf`, `aider` and `vscode_continue` are real
/// answers over there with nothing to install over here, and `terminal` is a
/// plain shell.
fn init_id_for_agent(agent_id: &str) -> Option<&'static str> {
    match agent_id {
        "claude_code" => Some("claude"),
        "cursor" => Some("cursor"),
        "cline" => Some("cline"),
        "codex_cli" => Some("codex"),
        "antigravity" => Some("antigravity"),
        "vscode" => Some("vscode"),
        _ => None,
    }
}

/// Which host to configure when there is no terminal to ask on.
///
/// `veng init` is the line the README, the Homebrew caption, the landing page and
/// `install.sh` all print, and the audience for this tool is agents, which run it
/// with no tty. `dialoguer` can only fail there, so the one documented setup
/// command exited 1 on `IO error: not a terminal` and named no remedy (#528).
///
/// The host running the command is the host to configure, and VENG already reads
/// that from the environment on the exec and pipe paths. Anything it cannot name
/// is an error that lists the flags, rather than a guess: installing into a host
/// nobody asked for is the worse failure of the two.
fn non_interactive_host() -> anyhow::Result<&'static str> {
    let agent = crate::agents::multiagent::detect_agent_id();
    init_id_for_agent(&agent).ok_or_else(|| {
        anyhow::anyhow!(
            "no terminal to prompt on, and the host here reads as `{agent}`. \
             Name one instead: `veng init --claude`, every host with \
             `veng init --all`, or see them all with `veng init --help`."
        )
    })
}

/// Which halves of the Claude Code install a flag set asks for, as (hooks, mcp).
///
/// #757. `--hook` has said "Only install hooks" since it shipped and installed
/// the MCP server as well, which is the shape of #151: a flag the parser accepts
/// and the code ignores. On this host the hooks are what shortens output and the
/// MCP server is a convenience whose tool definitions sit in the prefix of every
/// request, so one without the other is a preference somebody can hold.
///
/// Asking for both, by naming the host or by naming both halves, gets both.
fn claude_halves(is_claude: bool, is_all: bool, is_hook: bool, is_mcp: bool) -> (bool, bool) {
    if is_claude || is_all || (is_hook && is_mcp) || (!is_hook && !is_mcp) {
        return (true, true);
    }
    (is_hook, is_mcp)
}

pub fn run_init(args: &[String]) -> anyhow::Result<()> {
    if super::wants_help(args) {
        print_help();
        return Ok(());
    }
    super::check_flags("init", args, FLAGS)?;

    let mut is_claude = super::has_flag(args, "--claude");
    let mut is_cursor = super::has_flag(args, "--cursor");
    let mut is_zed = super::has_flag(args, "--zed");
    let mut is_cline = super::has_flag(args, "--cline");
    let mut is_roo = super::has_flag(args, "--roo") || super::has_flag(args, "--roo-code");
    let mut is_copilot = super::has_flag(args, "--copilot");
    let mut is_gemini = super::has_flag(args, "--gemini");
    let mut is_opencode = super::has_flag(args, "--opencode");
    let mut is_codex = super::has_flag(args, "--codex");
    let mut is_openclaw = super::has_flag(args, "--openclaw");
    let mut is_antigravity = super::has_flag(args, "--antigravity");
    let mut is_hermes = super::has_flag(args, "--hermes");
    let mut is_vscode = super::has_flag(args, "--vscode");
    let mut is_pi = super::has_flag(args, "--pi");

    let mut is_hook = super::has_flag(args, "--hook");
    let mut is_mcp = super::has_flag(args, "--mcp");
    let is_all = super::has_flag(args, "--all");
    let is_status = super::has_flag(args, "--status");
    let is_uninstall = super::has_flag(args, "--uninstall");

    if is_all {
        is_claude = true;
        is_hook = true;
        is_mcp = true;
    }

    // No flags -> Interactive Mode
    let no_flags = !is_claude
        && !is_cursor
        && !is_zed
        && !is_cline
        && !is_roo
        && !is_copilot
        && !is_gemini
        && !is_opencode
        && !is_codex
        && !is_openclaw
        && !is_antigravity
        && !is_hermes
        && !is_vscode
        && !is_pi
        && !is_status
        && !is_uninstall
        && !is_hook
        && !is_mcp;

    // Set when the menu could not be shown, so `target_ids` below installs into
    // the host that ran the command instead of erroring on the absent tty (#528).
    let mut detected: Option<&'static str> = None;

    if no_flags {
        println!(
            "\n{} {}: Setup VENG for your preferred AI Agent\n",
            "veng".bold().cyan(),
            "init".bold().yellow()
        );

        if !std::io::stdin().is_terminal() {
            let host = non_interactive_host()?;
            detected = Some(host);
            println!(
                "  {} No terminal to prompt on, so the host running this command is the one being configured: {}",
                "ℹ".blue(),
                host.bold()
            );
            println!(
                "  {}\n",
                "Pick another with veng init --cursor, or take every host with veng init --all"
                    .bright_black()
            );
        }
    }

    if no_flags && detected.is_none() {
        let items = vec![
            "Claude Code (Anthropic)",
            "Cursor AI",
            "Zed Editor",
            "Cline",
            "Roo Code",
            "GitHub Copilot CLI",
            "Gemini CLI",
            "OpenCode",
            "Codex CLI",
            "OpenClaw",
            "Antigravity IDE",
            "Hermes Agent",
            "VS Code (MCP)",
            "Pi Agent",
            "Quit",
        ];

        let selection = dialoguer::Select::new()
            .with_prompt("Select an AI Agent to configure")
            .items(&items)
            .default(0)
            .interact()?;

        match selection {
            0 => {
                is_claude = true;
                is_hook = true;
                is_mcp = true;
            }
            1 => is_cursor = true,
            2 => is_zed = true,
            3 => is_cline = true,
            4 => is_roo = true,
            5 => is_copilot = true,
            6 => is_gemini = true,
            7 => is_opencode = true,
            8 => is_codex = true,
            9 => is_openclaw = true,
            10 => is_antigravity = true,
            11 => is_hermes = true,
            12 => is_vscode = true,
            13 => is_pi = true,
            _ => return Ok(()),
        }

        println!();
        super::print_rule();
        println!(" {} VENG Before & After Preview", "⚡".yellow());
        super::print_rule();
        println!("{}", "Without VENG:".red());
        println!("  npm WARN deprecated ... (300 lines of warnings)");
        println!("  git log (2000 lines of history)");
        println!("{}", "\nWith VENG:".green());
        println!("  npm WARN deprecated ... [VENG: ⚠️ 300 repetitive lines dropped]");
        println!("  git log [VENG: ⚠️ truncated to latest 50 lines]");
        super::print_rule();
        println!();

        let proceed = dialoguer::Confirm::new()
            .with_prompt("Proceed with installation?")
            .default(true)
            .interact()?;

        if !proceed {
            return Ok(());
        }
    }

    let target_ids = if let Some(host) = detected {
        vec![host]
    } else if is_all {
        vec![
            "claude",
            "cursor",
            "zed",
            "cline",
            "roo-code",
            "copilot",
            "gemini",
            "opencode",
            "codex",
            "openclaw",
            "antigravity",
            "hermes",
            "vscode",
            "pi",
        ]
    } else {
        let mut ids = Vec::new();
        if is_claude || is_hook || is_mcp {
            ids.push("claude");
        }
        if is_cursor {
            ids.push("cursor");
        }
        if is_zed {
            ids.push("zed");
        }
        if is_cline {
            ids.push("cline");
        }
        if is_roo {
            ids.push("roo-code");
        }
        if is_copilot {
            ids.push("copilot");
        }
        if is_gemini {
            ids.push("gemini");
        }
        if is_opencode {
            ids.push("opencode");
        }
        if is_codex {
            ids.push("codex");
        }
        if is_openclaw {
            ids.push("openclaw");
        }
        if is_antigravity {
            ids.push("antigravity");
        }
        if is_hermes {
            ids.push("hermes");
        }
        if is_vscode {
            ids.push("vscode");
        }
        if is_pi {
            ids.push("pi");
        }
        ids
    };

    let exe_path = env::current_exe()?.to_string_lossy().to_string();

    if is_status {
        let (_, val) = crate::agents::claude::initialize_settings()?;
        let (post_ok, session_ok, pre_ok) = crate::agents::claude::check_status(&val, &exe_path);

        println!(
            "\n{}",
            "Claude Code VENG Installation Status:"
                .bold()
                .bright_white()
        );

        let fmt_status = |ok: bool| {
            if ok {
                "✓ installed".green()
            } else {
                "✗ not installed".red()
            }
        };

        println!("  PostToolUse:  {}", fmt_status(post_ok));
        println!("  SessionStart: {}", fmt_status(session_ok));
        println!("  PreCompact:   {}", fmt_status(pre_ok));
        println!();
        return Ok(());
    }

    if is_uninstall {
        let (path, mut val) = crate::agents::claude::initialize_settings()?;
        if path.exists() {
            crate::agents::claude::backup_settings(&path)?;
        }

        crate::agents::claude::remove_veng_hooks(&mut val);

        let mcp_path = crate::agents::claude::get_claude_json_path();
        if mcp_path.exists()
            && let Ok(content) = fs::read_to_string(&mcp_path)
            && let Ok(mut mcp_val) = serde_json::from_str::<Value>(&content)
        {
            if let Some(obj) = mcp_val.as_object_mut() {
                if let Some(servers) = obj.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
                    servers.remove("veng");
                }
                if let Some(projects) = obj.get_mut("projects").and_then(|p| p.as_object_mut()) {
                    for (_path, p_val) in projects.iter_mut() {
                        if let Some(ps) =
                            p_val.get_mut("mcpServers").and_then(|s| s.as_object_mut())
                        {
                            ps.remove("veng");
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
                    {
                        ps.remove("veng");
                    }
                }
            }
            let _ = fs::write(&mcp_path, serde_json::to_string_pretty(&mcp_val)?);
        }

        let new_content = serde_json::to_string_pretty(&val)?;
        fs::write(&path, new_content)?;
        println!("✓ VENG hooks and MCP server uninstalled from Claude");
        return Ok(());
    }

    let integrations = crate::agents::all_integrations();

    for agent in integrations {
        if target_ids.contains(&agent.id()) {
            println!("{}", format!("🤖 {} Setup", agent.name()).bold().cyan());

            // #757. `--hook` and `--mcp` have promised "Only install hooks" and
            // "Only register MCP server" since they shipped, and both installed
            // the pair. On Claude Code the hooks are what shortens output and the
            // MCP server is a convenience that puts two tool definitions in the
            // prefix of every request, so one without the other is a preference
            // somebody can hold, and the flag that says so has to mean it.
            let outcome = match (
                agent.id(),
                claude_halves(is_claude, is_all, is_hook, is_mcp),
            ) {
                ("claude", (true, false)) => crate::agents::claude::install_hooks(&exe_path),
                ("claude", (false, true)) => crate::agents::claude::install_mcp_server(&exe_path),
                _ => agent.install(&exe_path),
            };
            match outcome {
                Err(e) => eprintln!("  {} Failed: {}", "✗".red(), e),
                // #684. The success line was the same shape on every host, so
                // configuring an MCP-only one read as "VENG is now shortening
                // your tool output". It is not, and the user found out from an
                // empty `veng stats` days later. `doctor` says this correctly and
                // is the wrong place to say it first: installation is the moment
                // the expectation is set.
                //
                // Printed for every tier, not only the one that disappoints. A
                // line that appears only on bad news is a line users learn to
                // read as bad news, and `Full` is worth confirming.
                Ok(()) => println!(
                    "  {} {}: {}",
                    "ℹ".blue(),
                    agent.tier().name().bold(),
                    agent.tier().label().bright_black()
                ),
            }

            if agent.id() == "claude" {
                println!("\n  {} Binary: {}", "ℹ".blue(), exe_path.bright_black());
                println!(
                    "  {} Restart Claude Code to activate.\n",
                    "✓".green().bold()
                );
            }
            println!();
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {

    /// #757. Both flags have promised a half since they shipped and delivered
    /// the pair, so `veng init --hook` registered an MCP server the user had
    /// just said they did not want. The parser accepted the flag and the code
    /// ignored it, which is #151 wearing a different name.
    ///
    /// The table is the point: naming the host, or naming both halves, or naming
    /// neither, all still get both. Only a single half means a single half.
    #[test]
    fn a_half_flag_installs_that_half_and_nothing_else() {
        // (is_claude, is_all, is_hook, is_mcp) -> (hooks, mcp)
        for (args, want) in [
            ((false, false, true, false), (true, false)),
            ((false, false, false, true), (false, true)),
            ((false, false, true, true), (true, true)),
            ((true, false, true, false), (true, true)),
            ((false, true, true, false), (true, true)),
            ((false, false, false, false), (true, true)),
        ] {
            let (is_claude, is_all, is_hook, is_mcp) = args;
            assert_eq!(
                claude_halves(is_claude, is_all, is_hook, is_mcp),
                want,
                "claude_halves{args:?} has to be {want:?}"
            );
        }
    }
    use super::*;

    /// Every host the no-tty path can pick has to be a real install target, or
    /// `veng init` reports it and installs nothing. The two id vocabularies are
    /// maintained in different files, so a rename on either side breaks this
    /// quietly (#528).
    #[test]
    fn maps_detected_hosts_onto_real_install_targets() {
        let known: Vec<&str> = crate::agents::all_integrations()
            .iter()
            .map(|a| a.id())
            .collect();

        for agent in [
            "claude_code",
            "cursor",
            "cline",
            "codex_cli",
            "antigravity",
            "vscode",
        ] {
            let target = init_id_for_agent(agent).expect("detected host lost its install target");
            assert!(
                known.contains(&target),
                "{agent} maps to `{target}`, which no integration answers to: {known:?}"
            );
        }

        // A plain shell, and three hosts with nothing here to install into. All
        // four have to reach the error that names the flags.
        for agent in ["terminal", "windsurf", "aider", "vscode_continue"] {
            assert_eq!(init_id_for_agent(agent), None, "{agent}");
        }
    }

    /// #684. `veng init --antigravity` printed a green tick and nothing else, so
    /// the expectation it set was distillation. Antigravity has no hook, records
    /// nothing, and the user found out from an empty `veng stats` twelve days
    /// later. `doctor` had said so all along, correctly, and is the wrong place
    /// to say it first: installation is when the expectation is formed.
    ///
    /// Asserted on the source of the install loop rather than on a `Tier` value,
    /// because the thing that broke was a caller that never asked. A test that
    /// calls `tier()` itself passes while the loop ignores it, which is the same
    /// shape as #663's `measurement_method` and #688's plugin key: the contract
    /// holds and nobody reads it.
    #[test]
    fn the_install_loop_reports_the_tier_it_already_knows() {
        let src = include_str!("init.rs");
        let loop_start = src
            .find("for agent in integrations {")
            .expect("the install loop moved; this test is looking for the wrong thing");
        let loop_body = src.get(loop_start..).unwrap_or("");
        let install = loop_body
            .find("agent.install(&exe_path)")
            .expect("the install call moved");
        let after = loop_body.get(install..).unwrap_or("");

        // The success arm alone, not "anywhere after the install call". A scan
        // that wide passes with the tier reported from an unrelated branch, and
        // the branch next door is `if agent.id() == "claude"`, so the hole is not
        // hypothetical: the line could report the tier for one host and stay
        // silent for the other thirteen.
        let ok_arm = after
            .find("Ok(())")
            .map(|i| after.get(i..).unwrap_or(""))
            .expect("the install call no longer matches on its result");
        let ok_arm = ok_arm.split("\n            }").next().unwrap_or(ok_arm);

        assert!(
            ok_arm.contains("agent.tier()"),
            "the success arm does not consult `agent.tier()`, so every host gets \
             the same line whatever VENG can do there. Arm: {ok_arm}"
        );
        assert!(
            ok_arm.contains("println!"),
            "the success arm consults the tier and prints nothing. Arm: {ok_arm}"
        );
    }

    /// `AGENT_FLAGS` is a hand-maintained index into `FLAGS`. Getting it wrong
    /// files an agent under "CLAUDE SPECIFIC FLAGS" in help and nothing else
    /// breaks, so this is the only thing that would notice.
    #[test]
    fn splits_flags_between_the_two_help_groups() {
        assert_eq!(
            FLAGS[AGENT_FLAGS - 1].0,
            "--pi",
            "last agent flag moved; AGENT_FLAGS is stale"
        );
        assert_eq!(
            FLAGS[AGENT_FLAGS].0, "--all",
            "first non-agent flag moved; AGENT_FLAGS is stale"
        );
    }
}
