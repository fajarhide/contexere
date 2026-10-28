// No CLI entry point since #164; `hooks::pre_tool` calls it on every command (#157).

/// True when the command has its own pipe, redirect or chain operator outside
/// quotes. Wrapping the whole string put distillation upstream of the caller's
/// `tail` (#157) and wrote OMNI's banner into redirected files (#170, #207).
/// The post-hook still distills what the pipeline produces.
fn has_downstream_stage(cmd: &str) -> bool {
    let mut quote: Option<char> = None;
    let mut chars = cmd.chars();

    while let Some(c) = chars.next() {
        match quote {
            // Escapes count only outside single quotes.
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

/// Strips the `<omni> exec [--agent <id>] ` wrapper the pre-hook writes, by
/// the executable's file name: the writer uses an absolute path.
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
    if file_name != "omni" {
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

    // Both flags, in any order, or `^git\b` stops matching (#375).
    while let Some((flag, after_flag)) = split_token(rest)
        && matches!(flag, "--agent" | "--session")
        && let Some((_id, after_id)) = split_token(after_flag)
    {
        rest = after_id;
    }

    // A wrapper with nothing after it: fall back to what followed `exec`.
    if rest.is_empty() { post_exec } else { rest }
}

fn split_token(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start();
    if s.is_empty() {
        return None;
    }
    // `split_once`: the crate denies `clippy::string_slice`.
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
        let exe_path = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("omni"));
        let exe_name = exe_path.to_string_lossy();
        // Forward slashes: Git Bash on Windows strips backslashes from the path.
        #[cfg(windows)]
        let exe_name = exe_name.replace('\\', "/");

        // Name the host (#360) and session (#416) as flags, not env prefixes: the
        // registry reads this string back. Without the session the child ran no
        // ledger stage.
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

    /// Matches the absolute path the writer actually produces.
    #[test]
    fn strips_the_wrapper_when_it_carries_a_full_path() {
        assert_eq!(
            strip_exec_wrapper("/usr/local/bin/omni exec git status"),
            "git status"
        );
        assert_eq!(
            strip_exec_wrapper("/home/u/.omni/bin/omni exec --agent gemini git status"),
            "git status"
        );
        assert_eq!(
            strip_exec_wrapper("C:/Users/u/omni.exe exec --agent codex_cli cargo test"),
            "cargo test"
        );
    }

    #[test]
    fn leaves_a_command_that_is_not_our_wrapper() {
        assert_eq!(strip_exec_wrapper("git status"), "git status");
        assert_eq!(
            strip_exec_wrapper("/usr/bin/other exec git status"),
            "/usr/bin/other exec git status"
        );
        assert_eq!(
            strip_exec_wrapper("omni doctor --json"),
            "omni doctor --json"
        );
    }

    #[test]
    fn leaves_a_flag_without_a_command_alone() {
        assert_eq!(
            strip_exec_wrapper("/usr/local/bin/omni exec --agent gemini"),
            "--agent gemini"
        );
    }
    use super::rewrite_logic;

    /// Both `--agent` and `--session` are stripped (#416, #375).
    #[test]
    fn strips_every_flag_the_pre_hook_writes() {
        assert_eq!(
            strip_exec_wrapper("/opt/omni exec --agent claude_code --session abc-123 git status"),
            "git status"
        );
        assert_eq!(
            strip_exec_wrapper("/opt/omni exec --session abc-123 git status"),
            "git status"
        );
    }

    /// No session id is fine: it costs the ledger stage, not correctness.
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

    /// The child is told which host rewrote it (#360).
    #[test]
    fn names_the_host_that_asked_for_the_rewrite() {
        let rewritten =
            rewrite_logic("cargo tree", Some("gemini"), None).expect("cargo should rewrite");

        assert!(
            rewritten.contains(" exec --agent gemini cargo tree"),
            "the host and the original command must both survive: {rewritten}"
        );
    }

    #[test]
    fn omits_the_flag_when_the_host_is_unknown() {
        let rewritten = rewrite_logic("cargo tree", None, None).expect("cargo should rewrite");

        assert!(
            !rewritten.contains("--agent"),
            "no host means no claim about one: {rewritten}"
        );
        assert!(rewritten.ends_with(" exec cargo tree"), "{rewritten}");
    }

    #[test]
    fn leaves_a_command_that_pipes_into_its_own_stage() {
        assert_eq!(
            rewrite_logic("bash tidy.sh 2>&1 | tail -3", None, None),
            None
        );
        assert_eq!(rewrite_logic("git log | head -5", None, None), None);
    }

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

    /// An operator inside quotes is data.
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
        let rewritten = rewrite_logic("git status", None, None).expect("git should rewrite");
        let exe_part = rewritten.trim_end_matches(" exec git status");
        assert!(
            !exe_part.contains('\\'),
            "rewritten exe path should not contain backslashes: {exe_part}"
        );
    }
}
