use std::env;

pub const DENYLIST: &[&str] = &[
    "BASH_ENV",
    "ENV",
    "ZDOTDIR",
    "BASH_PROFILE",
    "NODE_OPTIONS",
    "NODE_EXTRA_CA_CERTS",
    "PYTHONSTARTUP",
    "PYTHONPATH",
    "PYTHONHOME",
    "RUBYOPT",
    "RUBYLIB",
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "LD_AUDIT",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_FORCE_FLAT_NAMESPACE",
    "GIT_EXEC_PATH",
    "GIT_ASKPASS",
    "GIT_TEMPLATE_DIR",
    "JAVA_TOOL_OPTIONS",
    "_JAVA_OPTIONS",
    "IFS",
    "CDPATH",
    "PROMPT_COMMAND",
    "SSH_ASKPASS",
    "SSH_AUTH_SOCK",
    "GIT_SSH_COMMAND",
    "GIT_SSH",
    "SVN_SSH",
    "CVS_RSH",
    "PERL5LIB",
    "PERL5OPT",
    "PERLLIB",
    "AWKPATH",
    "AWKLIBPATH",
    "XAUTHORITY",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "LOCPATH",
    "NLSPATH",
    "GCONV_PATH",
    "GIT_EXTERNAL_DIFF",
    "GIT_MERGE_AUTOEDIT",
    "PAGER",
    "EDITOR",
    "VISUAL",
    "MANPAGER",
    "MANPATH",
    "HOSTALIASES",
];

pub fn sanitize_env() -> Vec<(String, String)> {
    sanitize_vars(env::vars())
}

pub fn sanitize_vars(vars: impl IntoIterator<Item = (String, String)>) -> Vec<(String, String)> {
    vars.into_iter()
        .filter(|(k, _)| !DENYLIST.iter().any(|d| d.eq_ignore_ascii_case(k)))
        .collect()
}

/// Returns true if VENG_QUIET=1 is set. Suppresses stderr stats in pipe mode.
pub fn is_quiet() -> bool {
    env::vars().any(|(k, _)| k.eq_ignore_ascii_case("VENG_QUIET"))
}

/// Returns true if VENG_PASSTHROUGH is enabled (1/true/yes).
/// When enabled, VENG will bypass distillation and emit raw output.
pub fn is_passthrough() -> bool {
    env::vars().any(|(k, v)| k.eq_ignore_ascii_case("VENG_PASSTHROUGH") && means_enabled(&v))
}

/// Whether an environment value reads as "on".
///
/// Split out so the truth table can be tested without touching the process
/// environment. The test used to set `VENG_PASSTHROUGH=1` for real, under a
/// comment claiming the suite is single-threaded. It is not: cargo runs tests in
/// parallel, and `hooks::pipe::run_inner` reads this on entry, so any test
/// distilling at that moment took the passthrough branch and archived nothing.
/// That is what turned `archives_the_raw_output_it_shortens` red on CI while it
/// passed on every local run.
fn means_enabled(value: &str) -> bool {
    matches!(
        value.trim().to_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

#[derive(Debug, PartialEq)]
pub enum ValidationError {
    InvalidLoopId,
    GoalTooLong,
    GoalContainsShellMetachars,
    BudgetTooLarge,
}

pub fn validate_loop_context(
    loop_id: Option<&str>,
    goal: Option<&str>,
    budget_tokens: Option<u64>,
) -> Result<(), ValidationError> {
    // loop_id: only alphanumeric and dash, max 64 chars
    if let Some(id) = loop_id
        && (!id.chars().all(|c| c.is_alphanumeric() || c == '-') || id.len() > 64)
    {
        return Err(ValidationError::InvalidLoopId);
    }

    // goal: max 500 chars, no shell metacharacters
    if let Some(goal) = goal {
        if goal.len() > 500 {
            return Err(ValidationError::GoalTooLong);
        }
        // Block shell injection
        let blocked = ['`', '$', ';', '|', '&', '>', '<', '(', ')'];
        if goal.chars().any(|c| blocked.contains(&c)) {
            return Err(ValidationError::GoalContainsShellMetachars);
        }
    }

    // budget: max 10M tokens (sanity check)
    if budget_tokens.unwrap_or(0) > 10_000_000 {
        return Err(ValidationError::BudgetTooLarge);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_env_removes_ld_preload() {
        let mock_env = vec![
            ("LD_PRELOAD".to_string(), "bad.so".to_string()),
            ("NORMAL_VAR".to_string(), "123".to_string()),
        ];
        let sanitized = sanitize_vars(mock_env);
        let contains = sanitized.iter().any(|(k, _)| k == "LD_PRELOAD");
        assert!(!contains);
    }

    #[test]
    fn test_sanitize_env_removes_all_denylist_entries() {
        let mock_env: Vec<(String, String)> = DENYLIST
            .iter()
            .map(|key| (key.to_string(), "malicious_payload".to_string()))
            .collect();

        let sanitized = sanitize_vars(mock_env);

        for (k, _) in sanitized {
            assert!(!DENYLIST.iter().any(|d| d.eq_ignore_ascii_case(&k)));
        }
    }

    #[test]
    fn test_sanitize_env_preserves_path_and_normal_vars() {
        let mock_env = vec![
            ("PATH".to_string(), "/usr/bin:/bin".to_string()),
            ("NORMAL_VAR".to_string(), "123".to_string()),
        ];

        let sanitized = sanitize_vars(mock_env);
        let has_path = sanitized.iter().any(|(k, _)| k.to_uppercase() == "PATH");
        let has_normal = sanitized
            .iter()
            .any(|(k, v)| k == "NORMAL_VAR" && v == "123");

        assert!(has_path);
        assert!(has_normal);
    }

    /// Drives the predicate rather than the process environment. Setting
    /// `VENG_PASSTHROUGH` for real made every concurrently running test that
    /// distils take the passthrough branch, because `hooks::pipe::run_inner`
    /// reads it on entry. The comment that used to sit here said the suite was
    /// single-threaded; cargo runs tests in parallel, so it was an assumption
    /// stated as a fact, and it cost a red CI run that passed locally every time.
    #[test]
    fn reads_the_usual_spellings_of_on_and_off() {
        for on in ["1", "true", "TRUE", "yes", "on", " on "] {
            assert!(means_enabled(on), "{on:?} should enable passthrough");
        }
        for off in ["0", "false", "no", "off", "", "maybe"] {
            assert!(!means_enabled(off), "{off:?} should not enable passthrough");
        }
    }

    #[test]
    fn test_loop_id_injection_rejected() {
        assert_eq!(
            validate_loop_context(Some("abc-123;rm-rf"), None, None),
            Err(ValidationError::InvalidLoopId)
        );
        assert_eq!(
            validate_loop_context(Some("a".repeat(65).as_str()), None, None),
            Err(ValidationError::InvalidLoopId)
        );
        assert_eq!(
            validate_loop_context(Some("valid-id-123"), None, None),
            Ok(())
        );
    }

    #[test]
    fn test_goal_shell_metachar_sanitized() {
        assert_eq!(
            validate_loop_context(None, Some("fix this; echo 'pwned'"), None),
            Err(ValidationError::GoalContainsShellMetachars)
        );
        assert_eq!(
            validate_loop_context(None, Some("fix the auth (bug)"), None),
            Err(ValidationError::GoalContainsShellMetachars)
        );
        assert_eq!(
            validate_loop_context(None, Some("fix the auth issue"), None),
            Ok(())
        );
    }

    #[test]
    fn test_budget_overflow_clamped() {
        assert_eq!(
            validate_loop_context(None, None, Some(10_000_001)),
            Err(ValidationError::BudgetTooLarge)
        );
        assert_eq!(validate_loop_context(None, None, Some(10_000_000)), Ok(()));
    }
}

/// `VENG_PASSTHROUGH=1 <cmd>` written on the command line, which the hook can
/// only ever see as text (#534).
///
/// The manual documents that prefix as "raw output, every time", and on
/// `veng exec` and the pipe it is, because the variable is read by the same
/// process. The Claude Code `PostToolUse` hook is a separate process spawned by
/// the host: it inherits the host's environment and an assignment typed in
/// front of the command never reaches it. The prefix was inert on the one host
/// the promise matters on, and nothing said so.
///
/// The command string does reach the hook, so the assignment is honoured by
/// reading it there. Only leading assignments count, the same shape a shell
/// would apply them in: `echo VENG_PASSTHROUGH=1` mentions the name and sets
/// nothing, and must keep being distilled.
pub fn command_asks_for_passthrough(command: &str) -> bool {
    for token in command.split_whitespace() {
        match token.split_once('=') {
            Some((key, value)) if key.eq_ignore_ascii_case("VENG_PASSTHROUGH") => {
                return means_enabled(value);
            }
            // Another leading assignment: keep looking past it.
            Some((key, _))
                if !key.is_empty()
                    && !key.starts_with(|c: char| c.is_ascii_digit())
                    && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => {}
            // The command itself has started; anything after this is an argument.
            _ => return false,
        }
    }
    false
}

#[cfg(test)]
mod command_passthrough_tests {
    use super::command_asks_for_passthrough;

    #[test]
    fn honours_the_prefix_the_manual_documents() {
        assert!(command_asks_for_passthrough(
            "VENG_PASSTHROUGH=1 cat secrets.env"
        ));
        assert!(command_asks_for_passthrough(
            "VENG_PASSTHROUGH=true kubectl get pods -o yaml"
        ));
        // Past another assignment, the way a shell applies them.
        assert!(command_asks_for_passthrough(
            "FOO=1 VENG_PASSTHROUGH=on cargo test"
        ));
    }

    #[test]
    fn an_off_value_is_not_an_escape_hatch() {
        assert!(!command_asks_for_passthrough("VENG_PASSTHROUGH=0 cat file"));
        assert!(!command_asks_for_passthrough("cat file"));
    }

    /// The line between an assignment and a mention. Naming the variable as an
    /// argument sets nothing, so it must not turn distillation off.
    #[test]
    fn a_mention_is_not_an_assignment() {
        assert!(!command_asks_for_passthrough("echo VENG_PASSTHROUGH=1"));
        assert!(!command_asks_for_passthrough(
            "grep -r VENG_PASSTHROUGH=1 src/"
        ));
    }
}
