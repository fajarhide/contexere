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

// ─── Mode Detection ─────────────────────────────────────

#[derive(Debug, PartialEq)]
enum Mode {
    PostHook,
    Mcp,
    SessionStart,
    PreCompact,
    PreHook,
    Cli,
}

/// The seven hidden flags are the only thing a parser ever decided here.
///
/// clap parsed the whole CLI and 15 of 17 arms matched `Some(VengCommand::X { .. })`,
/// discarding every field it filled in, then handed the raw `env::args()` to a module
/// that parsed it again; 18 files under `src/cli/` do that second parse (#506). What
/// is left is this scan. Help is intercepted before it, and version is
/// `env!("CARGO_PKG_VERSION")`.
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
    env::var("VENG_CMD").ok().or_else(|| env::var("CMD").ok())
}

// ─── Engine / Globals ───────────────────────────────────

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

// ─── Help Text ──────────────────────────────────────────

/// Every subcommand, grouped by what a user is trying to do, with the payoff
/// rather than the noun.
///
/// This is the **only** command list. `veng help` and `veng --help` used to be
/// two hand-maintained copies that had already drifted, six commands including
/// `exec`, the harness every issue in this tracker asks reporters to run, were
/// missing from the one a user gets by typing `veng` (#152).
/// `lists_every_subcommand` keeps this honest.
const COMMANDS: &[(&str, &str, &str)] = &[
    (
        "SET UP",
        "init",
        "Install VENG into your agent (hooks + MCP)",
    ),
    (
        "SET UP",
        "migrate",
        "Move an omni install across to veng (data, hooks, MCP)",
    ),
    (
        "SET UP",
        "doctor",
        "Check the install is healthy, and fix what isn't",
    ),
    ("SET UP", "update", "Upgrade VENG to the latest release"),
    (
        "SET UP",
        "reset",
        "Uninstall cleanly, keeping a backup of your config",
    ),
    (
        "SEE WHAT IT SAVED",
        "stats",
        "How many tokens VENG cut, and from which commands",
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
        "Run one command through VENG, to see what it would do",
    ),
    ("TUNE IT", "query", "Search past distillations (VengQL)"),
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
        "veng".bold().cyan(),
        version.bright_black()
    );

    println!("\n{}", "USAGE:".bold().bright_white());
    println!("  veng {} {}", "<COMMAND>".cyan(), "[FLAGS]".bright_black());
    println!(
        "  {} | veng       {}",
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
        "  veng stats            {}",
        "# what did VENG save me?".bright_black()
    );
    println!(
        "  veng stats -d         {}",
        "# ...just today".bright_black()
    );
    println!(
        "  veng init             {}",
        "# set up your agent (interactive)".bright_black()
    );
    println!(
        "  ls -R | veng          {}",
        "# distill a long output by hand".bright_black()
    );
    println!(
        "\n  {}",
        "veng <command> --help for that command's flags".bright_black()
    );
    println!();

    if let Some(latest) = crate::guard::update::check() {
        crate::guard::update::print_notification(&latest);
    }
}

// ─── Main ───────────────────────────────────────────────

/// Restore the default disposition of `SIGPIPE` (#155).
///
/// Rust sets `SIGPIPE` to `SIG_IGN` before `main` runs, so writing to a pipe
/// whose reader has closed returns `EPIPE` instead of killing the process, and
/// `println!` panics on that error. Every other Unix tool dies quietly, which is
/// why `veng --help | head -1` printed a panic and a backtrace note where `ls |
/// head` prints nothing.
///
/// Fixing it here rather than at each `println!` is deliberate: the panic is not
/// specific to the help text. `veng doctor | head` reproduces it too, and so
/// would any command whose output outlives its reader. One line at the entry
/// point covers every writer; guarding each call site would not.
///
/// `#[cfg(unix)]` because Windows has no `SIGPIPE`; a closed pipe there surfaces
/// as an ordinary write error.
#[cfg(unix)]
fn restore_default_sigpipe() {
    // SAFETY: called before any thread is spawned, and `SIG_DFL` is the
    // disposition the OS starts with, this restores it rather than installing
    // a handler.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn restore_default_sigpipe() {}

fn main() {
    // First, before any thread exists: let OMNI_* settings keep working as VENG_*.
    legacy::adopt_legacy_env();

    restore_default_sigpipe();

    let args: Vec<String> = env::args().collect();

    // Fast-path pipe mode
    if args.len() == 1 && !io::stdin().is_terminal() {
        let store_arc = Store::open().map(Arc::new).ok();
        let session_arc = store_arc.as_ref().map(|s| {
            let session = s.find_latest_session().unwrap_or_else(SessionState::new);
            Arc::new(Mutex::new(session))
        });
        let cmd_name = detect_pipe_command();
        if let Err(e) = hooks::pipe::run(store_arc, session_arc, cmd_name.as_deref()) {
            eprintln!("[veng] Pipe engine error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // One help text (#152, #166). `veng help` / bare `veng` rendered a
    // hand-written list while `veng --help` rendered clap's, and the two had
    // already drifted, six commands, `exec` among them, were missing from the
    // one a user gets by typing `veng`. Intercept before clap so every route
    // reaches the same renderer. A subcommand's own `--help` is untouched:
    // `args.len() == 2` means nothing but the flag was passed.
    if args.len() == 2 && matches!(args[1].as_str(), "--help" | "-h" | "help") {
        print_help();
        return;
    }

    // Same shape for `--version` / `-v`. clap used to answer these itself before
    // any dispatch, and routing them into `cli::version` instead handed the flag
    // to a module whose `check_flags` correctly rejects it: `veng --version`
    // exited 1 saying "unknown flag `--version` for `veng version`" (#506).
    // `veng version` as a subcommand still goes the long way, below.
    if args.len() == 2 && matches!(args[1].as_str(), "--version" | "-v") {
        println!("veng {}", env!("CARGO_PKG_VERSION"));
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
            // No store: the pre-hook rewrites a command and updates the in-memory
            // turn. The only thing it used the database for was `context_turns`,
            // which had no reader (#270). Not opening it also takes the SQLite
            // connection off this hook's path entirely.
            let (_, session) = init_globals();
            if let Err(e) = hooks::pre_tool::run(session) {
                eprintln!("[veng] Pre-Hook error: {}", e);
                std::process::exit(1);
            }
        }

        Mode::SessionStart => {
            // Legacy flag, route through dispatcher
            let (store, session) = init_globals();
            if let (Some(s), Some(ss)) = (store, session) {
                // Background cleanup to prevent DB bloating
                let s_clone = Arc::clone(&s);
                std::thread::spawn(move || {
                    /// Number of days to retain session history in the database
                    const SESSION_RETENTION_DAYS: u32 = 30;
                    s_clone.cleanup_old(SESSION_RETENTION_DAYS);
                });
                let _ = hooks::dispatcher::run(s, ss);
            }
        }

        Mode::PreCompact => {
            // Legacy flag, route through dispatcher
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
                    eprintln!("[veng] MCP Server error: {}", e);
                }
            } else {
                eprintln!("[veng] Failed to open SQLite store for MCP.");
            }
        }

        Mode::Cli => {
            let cmd_name = args.get(1).map(|s| s.as_str()).unwrap_or("help");
            match cmd_name {
                "version" | "-v" | "--version" => cli::version::run_version(&args),
                "help" | "-h" | "--help" => print_help(),
                "diff" => {
                    if let Err(e) = cli::diff::run_diff(&args) {
                        eprintln!("[veng] Diff error: {}", e);
                        std::process::exit(1);
                    }
                }
                "init" => {
                    // Not `let _ =`: a rejected flag has to reach the
                    // user, or `veng init --curser` installs nothing and
                    // exits 0 (#151). The clap-side arm got this right
                    // and this one did not, which is what a parser whose
                    // result is discarded 15 times out of 17 buys you.
                    if let Err(e) = cli::init::run_init(&args) {
                        eprintln!("[veng] Init error: {}", e);
                        std::process::exit(1);
                    }
                }
                "migrate" => {
                    if let Err(e) = cli::migrate::run(&args) {
                        eprintln!("[veng] Migrate error: {}", e);
                        std::process::exit(1);
                    }
                }
                "reset" => {
                    if let Err(e) = cli::reset::handle_reset() {
                        eprintln!("[veng] Reset error: {}", e);
                        std::process::exit(1);
                    }
                }
                "stats" => match Store::open() {
                    Ok(store) => {
                        if let Err(e) = cli::stats::run(&args, &store) {
                            eprintln!("[veng] Stats error: {}", e);
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("[veng] Cannot open database for stats: {}", e);
                        std::process::exit(1);
                    }
                },
                "session" | "sessions" => match Store::open() {
                    Ok(store) => {
                        let store_arc = Arc::new(store);
                        if let Err(e) = cli::session::run_session(&args, store_arc) {
                            eprintln!("[veng] Session error: {}", e);
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("[veng] Cannot open database for session: {}", e);
                        std::process::exit(1);
                    }
                },
                "engram" | "engrams" => match Store::open() {
                    Ok(store) => {
                        let store_arc = Arc::new(store);
                        if let Err(e) = cli::engram::run_engram(&args, store_arc) {
                            eprintln!("[veng] Engram error: {}", e);
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("[veng] Cannot open database for engrams: {}", e);
                        std::process::exit(1);
                    }
                },
                "query" => match Store::open() {
                    Ok(store) => {
                        if let Err(e) = cli::query::run_query(&args, &store) {
                            eprintln!("[veng] Query error: {}", e);
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("[veng] Cannot open database for query: {}", e);
                        std::process::exit(1);
                    }
                },
                "retrieve" => match Store::open() {
                    Ok(store) => {
                        if let Err(e) = cli::retrieve::run(&args, &store) {
                            eprintln!("[veng] {}", e);
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("[veng] DB error: {}", e);
                        std::process::exit(1);
                    }
                },
                "dashboard" => match Store::open() {
                    Ok(store) => {
                        if let Err(e) = cli::dashboard::run(&args, &store) {
                            eprintln!("[veng] Dashboard error: {}", e);
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("[veng] DB error: {}", e);
                        std::process::exit(1);
                    }
                },
                "context" => {
                    // #609 moved this off the MCP surface, where it cost 189 B in
                    // the prefix of every request and was never once called. The
                    // session is best effort: it only decides the "hot in session"
                    // line, so a store that will not open still gives an answer.
                    let session = Store::open().ok().and_then(|s| s.find_latest_session());
                    if let Err(e) = cli::context::run_context(&args, session.as_ref()) {
                        eprintln!("[veng] Context error: {}", e);
                        std::process::exit(1);
                    }
                }
                "patterns" => match Store::open() {
                    Ok(store) => {
                        if let Err(e) = cli::patterns::run_patterns(&args, &store) {
                            eprintln!("[veng] Patterns error: {}", e);
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("[veng] Cannot open database for patterns: {}", e);
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
                        eprintln!("[veng] Exec error: {}", e);
                        std::process::exit(1);
                    }
                }
                // `remember` and `goal` reached only the clap arm and
                // were never in this fallback, so anything clap did not
                // classify as a subcommand hit `unknown` and exited 1.
                // Collapsing the two tables is what surfaced it (#506).
                // `extra` is what clap used to hand these two: argv after
                // the subcommand name.
                "remember" => match Store::open() {
                    Ok(store) => {
                        if let Err(e) = cli::remember::run(&args[2..], Arc::new(store)) {
                            eprintln!("[veng] Remember error: {}", e);
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("[veng] Cannot open database for remember: {}", e);
                        std::process::exit(1);
                    }
                },
                "goal" => match Store::open() {
                    Ok(store) => {
                        if let Err(e) = cli::goal::run(&args[2..], &store) {
                            eprintln!("[veng] Goal error: {}", e);
                            std::process::exit(1);
                        }
                    }
                    Err(e) => {
                        eprintln!("[veng] Cannot open database for goal: {}", e);
                        std::process::exit(1);
                    }
                },
                "doctor" => {
                    if let Err(e) = cli::doctor::run(&args) {
                        eprintln!("[veng] Doctor error: {}", e);
                        std::process::exit(1);
                    }
                }
                "update" => {
                    if let Err(e) = cli::update::run(&args) {
                        eprintln!("[veng] Update error: {}", e);
                        std::process::exit(1);
                    }
                }
                unknown => {
                    eprintln!(
                        "veng: unknown command '{}'\nRun 'veng help' for usage.",
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

    /// `COMMANDS` drives the only help a user sees. It used to be checked against
    /// clap's subcommand enum; with clap gone (#506) the dispatcher is the truth,
    /// and `hook_e2e::every_advertised_command_routes` drives the real binary to
    /// prove every name in here reaches a module. This half is the cheap one:
    /// nothing may be advertised under a heading the renderer does not print.
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
