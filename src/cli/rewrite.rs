// `rewrite_logic` has no CLI entry point: the `veng rewrite` subcommand was
// deleted in #164 (zero invocations on record). The module stays because
// `hooks::pre_tool` calls it on every command, see #157.

/// True when the command carries its own downstream stages, a pipe, a
/// redirect, or a chain operator standing outside quotes.
///
/// The rewrite wraps the **whole** command string, so `bash tidy.sh 2>&1 | tail -3`
/// ran as `veng exec bash tidy.sh 2>&1 | tail -3`: distillation landed upstream
/// of a pipeline the caller wrote deliberately, and `tail` returned VENG's
/// markers instead of the script's last three lines, the summary line the pipe
/// existed to read (#157). A redirect is the same defect with a file on the end:
/// `npm run build > build.log 2>&1` wrote a truncated log **plus VENG's banner**
/// into the file on disk, so the documented escape hatch from distillation
/// returned less than the terminal did (#170 for cargo, #207 for npm).
///
/// A caller that wrote its own stages has already said how it wants the output
/// shaped, so passing the command through untouched is the fail-open read. The
/// PostToolUse hook still distills whatever the pipeline finally produces, so
/// this costs no coverage on the path the agent actually reads.
fn has_downstream_stage(cmd: &str) -> bool {
    let mut quote: Option<char> = None;
    let mut chars = cmd.chars();

    while let Some(c) = chars.next() {
        match quote {
            // Inside single quotes bash treats every byte literally, so only a
            // double-quoted or backticked context honours the escape.
            Some(q) => {
                if c == '\\' && q != '\'' {
                    chars.next();
                } else if c == q {
                    quote = None;
                }
            }
            None => match c {
                '\\' => {
                    chars.next();
                }
                '\'' | '"' | '`' => quote = Some(c),
                '|' | '>' | '<' | ';' | '&' => return true,
                _ => {}
            },
        }
    }

    false
}

/// Strips the `<veng> exec [--agent <id>] ` wrapper the pre-hook writes.
///
/// `rewrite_logic` writes `current_exe()`, an absolute path, so matching the
/// literal prefix `"veng exec "` never fired: every rewritten command reached
/// the registry and the TOML filters still wrapped, so `^git\b` and friends
/// matched a path instead of the program and routing fell through to the
/// generic path. The `--agent` flag added in #360 rode along for the same
/// reason. Match the executable's file name instead, which is what stays
/// constant across a dev build, a Homebrew prefix and `veng.exe` on Windows.
pub fn strip_exec_wrapper(command: &str) -> &str {
    let mut rest = command.trim_start();

    let Some((program, after_program)) = split_token(rest) else {
        return command;
    };
    let file_name = program
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(program)
        .trim_end_matches(".exe");
    if file_name != "veng" {
        return command;
    }

    let Some((verb, after_verb)) = split_token(after_program) else {
        return command;
    };
    if verb != "exec" {
        return command;
    }
    let post_exec = after_verb;
    rest = post_exec;

    // `--agent <id>` and `--session <id>` only ever appear immediately after
    // `exec`, written by our own pre-hook. A loop rather than one `if`, because
    // there are two of them now: stripping only the first left `--session <uuid>`
    // on the front of the command the registry reads, which is #375 exactly
    // (`^git\b` stops matching) with a different flag.
    while let Some((flag, after_flag)) = split_token(rest)
        && matches!(flag, "--agent" | "--session")
        && let Some((_id, after_id)) = split_token(after_flag)
    {
        rest = after_id;
    }

    // A wrapper with no command behind it is a malformed rewrite. Fall back to
    // what followed `exec` rather than handing back the wrapper itself, which
    // would send an absolute path into the registry.
    if rest.is_empty() { post_exec } else { rest }
}

/// The first whitespace-delimited token and the remainder after it.
fn split_token(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    if s.is_empty() {
        return None;
    }
    // `split_once` rather than slicing by index: the boundary is a char boundary
    // here, but the crate denies `clippy::string_slice` precisely so nobody has
    // to re-derive that, and stdlib already does it.
    Some(match s.split_once(char::is_whitespace) {
        Some((token, rest)) => (token, rest.trim_start()),
        None => (s, ""),
    })
}

pub fn rewrite_logic(cmd_str: &str, agent: Option<&str>, session: Option<&str>) -> Option<String> {
    let allow_list = [
        "git ",
        "cargo ",
        "npm ",
        "pytest ",
        "kubectl ",
        "docker ",
        "terraform ",
        "make ",
        "node ",
        "python ",
        "go ",
        "bash ",
        "sh ",
    ];

    let wants_rewrite =
        allow_list.iter().any(|&p| cmd_str.starts_with(p)) && !has_downstream_stage(cmd_str);

    if wants_rewrite {
        // We always try to rewrite recognized tools to capture them.
        // run_exec will handle whether to use a shell or not.
        let exe_path = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("veng"));
        let exe_name = exe_path.to_string_lossy();
        // Claude Code on Windows runs hook output via Git Bash, which interprets
        // backslashes as escape characters and mangles the path
        // (`C:\Users\...` -> `C:Users...`). Use forward slashes so the path
        // survives bash unquoting; Windows accepts `/` in absolute paths.
        #[cfg(windows)]
        let exe_name = exe_name.replace('\\', "/");

        // Name the host that asked, so the child does not have to guess it from
        // ambient environment (#360). A flag rather than an `VENG_AGENT_ID=`
        // prefix: the prefix is shell dependent, and the command string is later
        // read back by the registry, which would have to strip it again.
        // The session id rides the same mechanism, and for a sharper reason than
        // the agent id: without it the child has no trustworthy scope for the
        // ledger, so `veng exec` ran no ledger stage at all while the post-hook
        // ran one (#416).
        let mut wrapped = format!("{exe_name} exec");
        if let Some(id) = agent.filter(|i| !i.is_empty()) {
            wrapped.push_str(&format!(" --agent {id}"));
        }
        if let Some(id) = session.filter(|i| !i.is_empty()) {
            wrapped.push_str(&format!(" --session {id}"));
        }
        wrapped.push(' ');
        wrapped.push_str(cmd_str);
        return Some(wrapped);
    }

    None
}

#[cfg(test)]
mod tests {

    use super::strip_exec_wrapper;

    /// Audit finding: `rewrite_logic` writes `current_exe()`, an absolute path,
    /// so matching the literal `"veng exec "` never fired on a real command.
    /// The earlier fixtures used the bare word and passed on a shape the writer
    /// never produces, which is why the defect survived two reviews.
    #[test]
    fn strips_the_wrapper_when_it_carries_a_full_path() {
        assert_eq!(
            strip_exec_wrapper("/usr/local/bin/veng exec git status"),
            "git status"
        );
        assert_eq!(
            strip_exec_wrapper("/home/u/.veng/bin/veng exec --agent gemini git status"),
            "git status"
        );
        assert_eq!(
            strip_exec_wrapper("C:/Users/u/veng.exe exec --agent codex_cli cargo test"),
            "cargo test"
        );
    }

    /// A command that merely mentions veng, or another program entirely, must
    /// come through untouched.
    #[test]
    fn leaves_a_command_that_is_not_our_wrapper() {
        assert_eq!(strip_exec_wrapper("git status"), "git status");
        assert_eq!(
            strip_exec_wrapper("/usr/bin/other exec git status"),
            "/usr/bin/other exec git status"
        );
        assert_eq!(
            strip_exec_wrapper("veng doctor --json"),
            "veng doctor --json"
        );
    }

    /// The flag with nothing behind it is a malformed rewrite; dropping the id
    /// would leave an empty command.
    #[test]
    fn leaves_a_flag_without_a_command_alone() {
        assert_eq!(
            strip_exec_wrapper("/usr/local/bin/veng exec --agent gemini"),
            "--agent gemini"
        );
    }
    use super::rewrite_logic;

    /// #416. The session id rides the same mechanism as `--agent`, so the
    /// stripper has to drop **both** or the registry reads a command that starts
    /// with a flag. That is #375 exactly, where an unstripped wrapper stopped
    /// `^git\b` filters matching, one flag later.
    #[test]
    fn strips_every_flag_the_pre_hook_writes() {
        assert_eq!(
            strip_exec_wrapper("/opt/veng exec --agent claude_code --session abc-123 git status"),
            "git status"
        );
        assert_eq!(
            strip_exec_wrapper("/opt/veng exec --session abc-123 git status"),
            "git status"
        );
    }

    /// The pre-hook writes the session id only when the host sent one, and a
    /// missing one is not an error: it costs the child its ledger stage, which is
    /// a missed reduction rather than a wrong claim.
    #[test]
    fn carries_the_session_id_only_when_there_is_one() {
        let with = rewrite_logic("git status", Some("claude_code"), Some("abc-123"))
            .expect("git should rewrite");
        assert!(with.contains(" --session abc-123 git status"), "{with}");

        let without =
            rewrite_logic("git status", Some("claude_code"), None).expect("git should rewrite");
        assert!(!without.contains("--session"), "{without}");
    }

    #[test]
    fn rewrites_a_plain_command() {
        assert_eq!(
            rewrite_logic("git status", None, None)
                .expect("git should rewrite")
                .split(" exec ")
                .nth(1),
            Some("git status")
        );
    }

    /// #360: the child process inherits no evidence of who spawned it, so a
    /// command Gemini's hook rewrote was filed under whatever the ambient
    /// environment suggested: `claude_code` inside a Claude shell, `terminal`
    /// once that was stripped, never `gemini`.
    #[test]
    fn names_the_host_that_asked_for_the_rewrite() {
        let rewritten =
            rewrite_logic("cargo tree", Some("gemini"), None).expect("cargo should rewrite");

        assert!(
            rewritten.contains(" exec --agent gemini cargo tree"),
            "the host and the original command must both survive: {rewritten}"
        );
    }

    /// Hosts whose payload does not identify them get no flag rather than a
    /// guessed one, so `veng exec` falls back instead of recording a lie.
    #[test]
    fn omits_the_flag_when_the_host_is_unknown() {
        let rewritten = rewrite_logic("cargo tree", None, None).expect("cargo should rewrite");

        assert!(
            !rewritten.contains("--agent"),
            "no host means no claim about one: {rewritten}"
        );
        assert!(rewritten.ends_with(" exec cargo tree"), "{rewritten}");
    }

    /// #157: the rewrite wrapped the whole string, so distillation sat upstream
    /// of the caller's `tail` and the script's summary line was deleted before
    /// `tail` ever saw it.
    #[test]
    fn leaves_a_command_that_pipes_into_its_own_stage() {
        assert_eq!(
            rewrite_logic("bash tidy.sh 2>&1 | tail -3", None, None),
            None
        );
        assert_eq!(rewrite_logic("git log | head -5", None, None), None);
    }

    /// #170 / #207: `npm run build > build.log 2>&1` wrote a truncated log plus
    /// VENG's banner into the file the shell was told to write.
    #[test]
    fn leaves_a_command_that_redirects_to_a_file() {
        assert_eq!(
            rewrite_logic("npm run build > build.log 2>&1", None, None),
            None
        );
        assert_eq!(rewrite_logic("cargo tree > tree.log", None, None), None);
        assert_eq!(rewrite_logic("make ci >> ci.log", None, None), None);
    }

    #[test]
    fn leaves_a_chained_command() {
        assert_eq!(rewrite_logic("npm ci && npm test", None, None), None);
        assert_eq!(rewrite_logic("git fetch; git status", None, None), None);
    }

    /// An operator inside quotes is data, not a stage, blocking on it would
    /// give up coverage for nothing.
    #[test]
    fn still_rewrites_when_the_operator_is_quoted() {
        for cmd in [
            "git commit -m \"fix: a | b\"",
            "git log --grep 'a > b'",
            "git commit -m \"escaped \\\" quote; still one arg\"",
        ] {
            assert!(
                rewrite_logic(cmd, None, None).is_some(),
                "quoted operator should not block the rewrite: {cmd}"
            );
        }
    }

    #[test]
    #[cfg(windows)]
    fn test_rewrite_uses_forward_slashes_on_windows() {
        // On Windows, the rewritten command must not contain backslashes from
        // the veng exe path; Git Bash strips them as escape characters.
        let rewritten = rewrite_logic("git status", None, None).expect("git should rewrite");
        let exe_part = rewritten.trim_end_matches(" exec git status");
        assert!(
            !exe_part.contains('\\'),
            "rewritten exe path should not contain backslashes: {exe_part}"
        );
    }
}
