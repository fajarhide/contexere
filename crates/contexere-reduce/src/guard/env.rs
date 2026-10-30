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

/// `OMNI_QUIET` suppresses the stderr stats in pipe mode.
pub fn is_quiet() -> bool {
    env::vars().any(|(k, _)| k.eq_ignore_ascii_case("OMNI_QUIET"))
}

pub fn is_passthrough() -> bool {
    env::vars().any(|(k, v)| k.eq_ignore_ascii_case("OMNI_PASSTHROUGH") && means_enabled(&v))
}

/// Split from `is_passthrough` so tests never set the variable for real: cargo
/// runs tests in parallel, and every test distilling at that moment would pass
/// through.
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
    if let Some(id) = loop_id
        && (!id.chars().all(|c| c.is_alphanumeric() || c == '-') || id.len() > 64)
    {
        return Err(ValidationError::InvalidLoopId);
    }

    if let Some(goal) = goal {
        if goal.len() > 500 {
            return Err(ValidationError::GoalTooLong);
        }
        let blocked = ['`', '$', ';', '|', '&', '>', '<', '(', ')'];
        if goal.chars().any(|c| blocked.contains(&c)) {
            return Err(ValidationError::GoalContainsShellMetachars);
        }
    }

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

/// `OMNI_PASSTHROUGH=1 <cmd>` typed on the command line. The hook is a separate
/// process that never inherits that assignment, so it reads the command text
/// instead (#534). Only leading assignments count, as in a shell:
/// `echo OMNI_PASSTHROUGH=1` sets nothing.
pub fn command_asks_for_passthrough(command: &str) -> bool {
    for token in command.split_whitespace() {
        match token.split_once('=') {
            Some((key, value)) if key.eq_ignore_ascii_case("OMNI_PASSTHROUGH") => {
                return means_enabled(value);
            }
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
            "OMNI_PASSTHROUGH=1 cat secrets.env"
        ));
        assert!(command_asks_for_passthrough(
            "OMNI_PASSTHROUGH=true kubectl get pods -o yaml"
        ));
        assert!(command_asks_for_passthrough(
            "FOO=1 OMNI_PASSTHROUGH=on cargo test"
        ));
    }

    #[test]
    fn an_off_value_is_not_an_escape_hatch() {
        assert!(!command_asks_for_passthrough("OMNI_PASSTHROUGH=0 cat file"));
        assert!(!command_asks_for_passthrough("cat file"));
    }

    #[test]
    fn a_mention_is_not_an_assignment() {
        assert!(!command_asks_for_passthrough("echo OMNI_PASSTHROUGH=1"));
        assert!(!command_asks_for_passthrough(
            "grep -r OMNI_PASSTHROUGH=1 src/"
        ));
    }
}
