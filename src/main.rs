pub mod agents;
pub mod analytics;
mod cli;
mod distillers;
mod graph;
mod guard;
mod hooks;
mod ledger;
mod legacy;
mod mcp;
mod paths;
pub mod pipeline;
mod session;
mod store;
mod util;

use colored::*;
use std::env;
use std::io::{self, IsTerminal};
use std::sync::{Arc, Mutex};

use crate::pipeline::SessionState;
use crate::store::sqlite::Store;

#[derive(Debug, PartialEq)]
enum Mode {
    PostHook,
    Mcp,
    SessionStart,
    PreCompact,
    PreHook,
    Cli,
}

/// The hidden hook flags. Everything else is dispatched by name, parsed once
/// in its own module (#506).
fn detect_mode(args: &[String]) -> Mode {
    for a in &args[1..] {
        match a.as_str() {
            "--hook" | "--post-hook" => return Mode::PostHook,
            "--mcp" => return Mode::Mcp,
            "--session-start" | "--before-agent-start" => return Mode::SessionStart,
            "--pre-compact" => return Mode::PreCompact,
            "--pre-hook" => return Mode::PreHook,
            _ => {}
        }
    }
    Mode::Cli
}

fn detect_pipe_command() -> Option<String> {
    env::var("OMNI_CMD").ok().or_else(|| env::var("CMD").ok())
}

fn init_globals() -> (Option<Arc<Store>>, Option<Arc<Mutex<SessionState>>>) {
    match Store::open() {
        Ok(store) => {
            let session = store
                .find_latest_session()
                .unwrap_or_else(SessionState::new);
            let store_arc = Arc::new(store);
            let session_arc = Arc::new(Mutex::new(session));
            (Some(store_arc), Some(session_arc))
        }
        Err(_) => (None, None),
    }
}

/// The only command list: `omni help`, bare `omni` and `--help` all render it
/// (#152). `lists_every_subcommand` keeps it complete.
const COMMANDS: &[(&str, &str, &str)] = &[
    (
        "SET UP",
        "init",
        "Install Contexere into your agent (hooks + MCP)",
    ),
    (
        "SET UP",
        "doctor",
        "Check the install is healthy, and fix what isn't",
    ),
    (
        "SET UP",
        "update",
        "Upgrade Contexere to the latest release",
    ),
    (
        "SET UP",
        "reset",
        "Uninstall cleanly, keeping a backup of your config",
    ),
    (
        "SEE WHAT IT SAVED",
        "stats",
        "How many tokens Contexere cut, and from which commands",
    ),
    (
        "SEE WHAT IT SAVED",
        "retrieve",
        "Print the content a marker archived, by its handle",
    ),
    (
        "SEE WHAT IT SAVED",
        "dashboard",
        "The same numbers in a browser, on 127.0.0.1",
    ),
    (
        "SEE WHAT IT SAVED",
        "diff",
        "The last command's output, before vs after",
    ),
    (
        "SEE WHAT IT SAVED",
        "session",
        "What this session has spent, and on what",
    ),
    (
        "TUNE IT",
        "exec",
        "Run one command through Contexere, to see what it would do",
    ),
    ("TUNE IT", "query", "Search past distillations (OmniQL)"),
    ("TUNE IT", "patterns", "Errors that keep coming back"),
    (
        "TUNE IT",
        "context",
        "Which files import this one, and which it imports",
    ),
    ("MEMORY", "remember", "Save a fact for future sessions"),
    ("MEMORY", "engram", "Digests of finished subtasks"),
    (
        "MEMORY",
        "goal",
        "Pin a north-star goal so scoring favours it",
    ),
    ("MEMORY", "version", "Version and environment details"),
];

/// The order groups render in. A group not listed here would silently vanish
/// from help, so `lists_every_subcommand` rejects one.
const GROUPS: &[&str] = &["SET UP", "SEE WHAT IT SAVED", "TUNE IT", "MEMORY"];

fn print_help() {
    let version = env!("CARGO_PKG_VERSION");

    println!(
        "\n{} {}: Less noise. More signal. Right signal.",
        "omni".bold().cyan(),
        version.bright_black()
    );

    println!("\n{}", "USAGE:".bold().bright_white());
    println!("  omni {} {}", "<COMMAND>".cyan(), "[FLAGS]".bright_black());
    println!(
        "  {} | omni       {}",
        "cmd".bright_black(),
        "# distill any command's output".bright_black()
    );

    let width = COMMANDS.iter().map(|(_, n, _)| n.len()).max().unwrap_or(0);
    for group in GROUPS {
        println!("\n{}", format!("{group}:").bold().bright_white());
        for (_, name, payoff) in COMMANDS.iter().filter(|(g, _, _)| g == group) {
            println!("  {} {}", format!("{name:<width$}").cyan(), payoff);
        }
    }

    println!("\n{}", "EXAMPLES:".bold().bright_white());
    println!(
        "  omni stats            {}",
        "# what did Contexere save me?".bright_black()
    );
    println!(
        "  omni stats -d         {}",
        "# ...just today".bright_black()
    );
    println!(
        "  omni init             {}",
        "# set up your agent (interactive)".bright_black()
    );
    println!(
        "  ls -R | omni          {}",
        "# distill a long output by hand".bright_black()
    );
    println!(
        "\n  {}",
        "omni <command> --help for that command's flags".bright_black()
    );
    println!();

    if let Some(latest) = crate::guard::update::check() {
        crate::guard::update::print_notification(&latest);
    }
}

/// Restore the default `SIGPIPE` disposition so `omni ... | head` exits quietly
/// instead of panicking on `EPIPE` (#155). Done once here because every writer
/// hits it.
#[cfg(unix)]
fn restore_default_sigpipe() {
    // SAFETY: before any thread is spawned, and it restores the OS default.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn restore_default_sigpipe() {}

fn main() {
    legacy::adopt_env();
    restore_default_sigpipe();

    let args: Vec<String> = env::args().collect();

    if args.len() == 1 && !io::stdin().is_terminal() {
        let store_arc = Store::open().map(Arc::new).ok();
        let session_arc = store_arc.as_ref().map(|s| {
            let session = s.find_latest_session().unwrap_or_else(SessionState::new);
            Arc::new(Mutex::new(session))
        });
        let cmd_name = detect_pipe_command();
        if let Err(e) = hooks::pipe::run(store_arc, session_arc, cmd_name.as_deref()) {
            eprintln!("[omni] Pipe engine error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // Intercepted before dispatch so every help route renders the same text
    // (#152). `len() == 2`: a subcommand's own `--help` is left alone.
    if args.len() == 2 && matches!(args[1].as_str(), "--help" | "-h" | "help") {
        print_help();
        return;
    }

    // Answered here: routed into `cli::version`, the flag was rejected as
    // unknown (#506).
    if args.len() == 2 && matches!(args[1].as_str(), "--version" | "-v") {
        // Follows the name it runs under, like the MCP tool prefix (#958).
        println!(
            "{} {}",
            contexere::legacy::mcp_name(),
            env!("CARGO_PKG_VERSION")
        );
        return;
    }

    let mode = detect_mode(&args);

    match mode {
        Mode::PostHook => {
            let (store, session) = init_globals();
            if let (Some(s), Some(ss)) = (store, session) {
                let _ = hooks::dispatcher::run(s, ss);
            }
        }

        Mode::PreHook => {
            // No store: the pre-hook never needed the database (#270), and skipping
            // it keeps SQLite off this hook's path.
            let (_, session) = init_globals();
            if let Err(e) = hooks::pre_tool::run(session) {
                eprintln!("[omni] Pre-Hook error: {}", e);
                std::process::exit(1);
            }
        }

        Mode::SessionStart => {
            let (store, session) = init_globals();
            if let (Some(s), Some(ss)) = (store, session) {
                let s_clone = Arc::clone(&s);
                std::thread::spawn(move || {
                    const SESSION_RETENTION_DAYS: u32 = 30;
                    s_clone.cleanup_old(SESSION_RETENTION_DAYS);
                });
                let _ = hooks::dispatcher::run(s, ss);
            }
        }

        Mode::PreCompact => {
            let (store, session) = init_globals();
            if let (Some(s), Some(ss)) = (store, session) {
                let _ = hooks::dispatcher::run(s, ss);
            }
        }

        Mode::Mcp => {
            let (store, session) = init_globals();
            if let (Some(s), Some(ss)) = (store, session) {
                let rt = tokio::runtime::Runtime::new().unwrap();
                if let Err(e) = rt.block_on(async { mcp::server::run(s, ss).await }) {
                    eprintln!("[omni] MCP Server error: {}", e);
                }
            } else {
                eprintln!("[omni] Failed to open SQLite store for MCP.");
            }
        }

        Mode::Cli => {
            let cmd_name = args.get(1).map(|s| s.as_str()).unwrap_or("help");
            match cmd_name {
                "version" | "-v" | "--version" => cli::version::run_version(&args),
                "help" | "-h" | "--help" => print_help(),
                "diff" => {
                    if let Err(e) = cli::diff::run_diff(&args) {
                        eprintln!("[omni] Diff error: {}", e);
                        std::process::exit(1);
                    }
                }
                "init" => {
                    // Not `let _ =`: a rejected flag has to reach the user, or a
                    // typo installs nothing and exits 0 (#151).
                    if let Err(e) = cli::init::run_init(&args) {
                        eprintln!("[omni] Init error: {}", e);
                        std::process::exit(1);
                    }
                }
                "reset" => {
                    if let Err(e) = cli::reset::handle_reset() {
                        eprintln!("[omni] Reset error: {}", e);
                        std::process::exit(1);
                    }
                }
                "stats" => match Store::open() {
                    Ok(store) => {
                        if let Err(e) = cli::stats::run(&args, &store) {
                            eprintln!("[omni] Stats error: {}", e);
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("[omni] Cannot open database for stats: {}", e);
                        std::process::exit(1);
                    }
                },
                "session" | "sessions" => match Store::open() {
                    Ok(store) => {
                        let store_arc = Arc::new(store);
                        if let Err(e) = cli::session::run_session(&args, store_arc) {
                            eprintln!("[omni] Session error: {}", e);
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("[omni] Cannot open database for session: {}", e);
                        std::process::exit(1);
                    }
                },
                "engram" | "engrams" => match Store::open() {
                    Ok(store) => {
                        let store_arc = Arc::new(store);
                        if let Err(e) = cli::engram::run_engram(&args, store_arc) {
                            eprintln!("[omni] Engram error: {}", e);
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("[omni] Cannot open database for engrams: {}", e);
                        std::process::exit(1);
                    }
                },
                "query" => match Store::open() {
                    Ok(store) => {
                        if let Err(e) = cli::query::run_query(&args, &store) {
                            eprintln!("[omni] Query error: {}", e);
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("[omni] Cannot open database for query: {}", e);
                        std::process::exit(1);
                    }
                },
                "retrieve" => match Store::open() {
                    Ok(store) => {
                        if let Err(e) = cli::retrieve::run(&args, &store) {
                            eprintln!("[omni] {}", e);
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("[omni] DB error: {}", e);
                        std::process::exit(1);
                    }
                },
                "dashboard" => match Store::open() {
                    Ok(store) => {
                        if let Err(e) = cli::dashboard::run(&args, &store) {
                            eprintln!("[omni] Dashboard error: {}", e);
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("[omni] DB error: {}", e);
                        std::process::exit(1);
                    }
                },
                "context" => {
                    // Best effort: the session only decides the "hot in session" line.
                    let session = Store::open().ok().and_then(|s| s.find_latest_session());
                    if let Err(e) = cli::context::run_context(&args, session.as_ref()) {
                        eprintln!("[omni] Context error: {}", e);
                        std::process::exit(1);
                    }
                }
                "patterns" => match Store::open() {
                    Ok(store) => {
                        if let Err(e) = cli::patterns::run_patterns(&args, &store) {
                            eprintln!("[omni] Patterns error: {}", e);
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("[omni] Cannot open database for patterns: {}", e);
                        std::process::exit(1);
                    }
                },
                "exec" => {
                    let store_arc = Store::open().map(Arc::new).ok();
                    let session_arc = store_arc.as_ref().map(|s| {
                        let session = s.find_latest_session().unwrap_or_else(SessionState::new);
                        Arc::new(Mutex::new(session))
                    });
                    if let Err(e) = cli::exec::run_exec(&args, store_arc, session_arc) {
                        eprintln!("[omni] Exec error: {}", e);
                        std::process::exit(1);
                    }
                }
                // `extra` is argv after the subcommand name.
                "remember" => match Store::open() {
                    Ok(store) => {
                        if let Err(e) = cli::remember::run(&args[2..], Arc::new(store)) {
                            eprintln!("[omni] Remember error: {}", e);
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("[omni] Cannot open database for remember: {}", e);
                        std::process::exit(1);
                    }
                },
                "goal" => match Store::open() {
                    Ok(store) => {
                        if let Err(e) = cli::goal::run(&args[2..], &store) {
                            eprintln!("[omni] Goal error: {}", e);
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("[omni] Cannot open database for goal: {}", e);
                        std::process::exit(1);
                    }
                },
                "doctor" => {
                    if let Err(e) = cli::doctor::run(&args) {
                        eprintln!("[omni] Doctor error: {}", e);
                        std::process::exit(1);
                    }
                }
                "update" => {
                    if let Err(e) = cli::update::run(&args) {
                        eprintln!("[omni] Update error: {}", e);
                        std::process::exit(1);
                    }
                }
                unknown => {
                    eprintln!(
                        "omni: unknown command '{}'\nRun 'omni help' for usage.",
                        unknown
                    );
                    std::process::exit(1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nothing may be advertised under a heading the renderer does not print.
    /// `hook_e2e::every_advertised_command_routes` proves each name routes.
    #[test]
    fn every_listed_command_sits_in_a_group_that_renders() {
        for (group, name, _) in COMMANDS {
            assert!(
                GROUPS.contains(group),
                "`{name}` is in group `{group}`, which GROUPS does not render"
            );
        }
    }
}
