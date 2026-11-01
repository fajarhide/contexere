use colored::*;
use serde_json::Value;
use std::env;
use std::fs;
use std::io::IsTerminal;

/// Read by `print_help` and `super::check_flags` (#151); the first `AGENT_FLAGS` entries are
/// agents.
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
    ("--uninstall", "Remove Contexere hooks and MCP server"),
];

/// Where `FLAGS` stops listing agents and starts listing Claude-specific flags.
// ponytail: a hand-kept split index, because `check_flags` takes one flat list.
// `splits_flags_between_the_two_help_groups` guards it. Split the consts if a
// third group appears.
const AGENT_FLAGS: usize = 15;

fn print_help() {
    println!(
        "\n{} {}: Setup Contexere for your preferred AI Agent",
        "omni".bold().cyan(),
        "init".bold().yellow()
    );
    println!("\n{}", "USAGE:".bold().bright_white());
    println!("  omni {}", "init [FLAGS]".cyan());

    let entries: Vec<_> = FLAGS
        .iter()
        .chain(std::iter::once(&super::HELP_FLAG))
        .collect();
    super::print_flag_group("SUPPORTED AGENTS:", &entries[..AGENT_FLAGS]);
    // `--all` configures every host, so the group is not named "Claude" (#455).
    super::print_flag_group(
        "EVERY HOST, AND CLAUDE SPECIFIC FLAGS:",
        &entries[AGENT_FLAGS..],
    );

    println!("\n{}", "EXAMPLES:".bold().bright_white());
    println!(
        "  omni init             {}",
        "# Interactive menu".bright_black()
    );
    println!(
        "  omni init --claude    {}",
        "# Setup for Claude Code".bright_black()
    );
    println!(
        "  omni init --all       {}",
        "# Every host, including a .vscode/mcp.json here".bright_black()
    );
    println!();
}

/// The `init` target for a host `detect_agent_id` recognises, or `None`. The
/// two id vocabularies differ: some detected agents have nothing to install.
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

/// Which host to configure with no terminal to ask on: agents run `omni init`
/// without a tty (#528). The host running it, or an error listing the flags;
/// never a guess.
fn non_interactive_host() -> anyhow::Result<&'static str> {
    let agent = crate::agents::multiagent::detect_agent_id();
    init_id_for_agent(&agent).ok_or_else(|| {
        anyhow::anyhow!(
            "no terminal to prompt on, and the host here reads as `{agent}`. \
             Name one instead: `omni init --claude`, every host with \
             `omni init --all`, or see them all with `omni init --help`."
        )
    })
}

/// Which halves of the Claude Code install a flag set asks for, as (hooks, mcp).
/// A single half means that half only (#757); otherwise both.
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

    // No tty for the menu: install into the host that ran it (#528).
    let mut detected: Option<&'static str> = None;

    if no_flags {
        println!(
            "\n{} {}: Setup Contexere for your preferred AI Agent\n",
            "omni".bold().cyan(),
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
                "Pick another with omni init --cursor, or take every host with omni init --all"
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
        println!(" {} Contexere Before & After Preview", "⚡".yellow());
        super::print_rule();
        println!("{}", "Without Contexere:".red());
        println!("  npm WARN deprecated ... (300 lines of warnings)");
        println!("  git log (2000 lines of history)");
        println!("{}", "\nWith Contexere:".green());
        println!("  npm WARN deprecated ... [OMNI: ⚠️ 300 repetitive lines dropped]");
        println!("  git log [OMNI: ⚠️ truncated to latest 50 lines]");
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
            "Claude Code Contexere Installation Status:"
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

        crate::agents::claude::remove_omni_hooks(&mut val);

        let mcp_path = crate::agents::claude::get_claude_json_path();
        if mcp_path.exists()
            && let Ok(content) = fs::read_to_string(&mcp_path)
            && let Ok(mut mcp_val) = serde_json::from_str::<Value>(&content)
        {
            crate::agents::claude::remove_mcp_entries(&mut mcp_val);
            let _ = fs::write(&mcp_path, serde_json::to_string_pretty(&mcp_val)?);
        }

        let new_content = serde_json::to_string_pretty(&val)?;
        fs::write(&path, new_content)?;
        println!("✓ Contexere hooks and MCP server uninstalled from Claude");
        return Ok(());
    }

    let integrations = crate::agents::all_integrations();
    let mut failed = Vec::new();

    for agent in integrations {
        if target_ids.contains(&agent.id()) {
            println!("{}", format!("🤖 {} Setup", agent.name()).bold().cyan());

            // Pi alone refuses when its binary is absent, and `--all` means the
            // hosts on this machine, so that absence must not fail the run.
            if is_all && agent.id() == "pi" && !crate::agents::pi::is_installed() {
                println!("  {} Skipped: Pi is not on PATH\n", "ℹ".blue());
                continue;
            }

            // `--hook` and `--mcp` each mean one half (#757).
            let outcome = match (
                agent.id(),
                claude_halves(is_claude, is_all, is_hook, is_mcp),
            ) {
                ("claude", (true, false)) => crate::agents::claude::install_hooks(&exe_path),
                ("claude", (false, true)) => crate::agents::claude::install_mcp_server(&exe_path),
                _ => agent.install(&exe_path),
            };
            let installed = outcome.is_ok();
            match outcome {
                Err(e) => {
                    eprintln!("  {} Failed: {}", "✗".red(), e);
                    failed.push(agent.name());
                }
                // Say what this host's tier lets OMNI do, for every tier: installation is
                // when the expectation forms (#684).
                Ok(()) => println!(
                    "  {} {}: {}",
                    "ℹ".blue(),
                    agent.tier().name().bold(),
                    agent.tier().label().bright_black()
                ),
            }

            if installed && agent.id() == "claude" {
                println!("\n  {} Binary: {}", "ℹ".blue(), exe_path.bright_black());
                println!(
                    "  {} Restart Claude Code to activate.\n",
                    "✓".green().bold()
                );
            }
            println!();
        }
    }

    // Keep going first: one bad config must not stop the other hosts installing.
    if !failed.is_empty() {
        anyhow::bail!("not installed: {}", failed.join(", "));
    }
    Ok(())
}

#[cfg(test)]
mod tests {

    /// Only a single half means a single half (#757).
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

    /// Every host the no-tty path picks must be an install target (#528).
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

        // These four must reach the error that names the flags.
        for agent in ["terminal", "windsurf", "aider", "vscode_continue"] {
            assert_eq!(init_id_for_agent(agent), None, "{agent}");
        }
    }

    /// The install loop's success arm consults `agent.tier()` (#684). Read from
    /// source, because the defect was a caller that never asked.
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

        // The success arm only: the next branch is Claude's, so a wider scan passes wrongly.
        let ok_arm = after
            .find("Ok(())")
            .map(|i| after.get(i..).unwrap_or(""))
            .expect("the install call no longer matches on its result");
        let ok_arm = ok_arm.split("\n            }").next().unwrap_or(ok_arm);

        assert!(
            ok_arm.contains("agent.tier()"),
            "the success arm does not consult `agent.tier()`, so every host gets \
             the same line whatever Contexere can do there. Arm: {ok_arm}"
        );
        assert!(
            ok_arm.contains("println!"),
            "the success arm consults the tier and prints nothing. Arm: {ok_arm}"
        );
    }

    /// `AGENT_FLAGS` must split help correctly; nothing else would notice.
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
