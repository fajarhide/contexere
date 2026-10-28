/// True for a leading shell variable assignment, `KEY=value`. The key must look
/// like an environment name, so `--out=file` and paths with `=` do not match
/// (#512).
fn is_env_assignment(token: &str) -> bool {
    match token.split_once('=') {
        Some((key, _)) => {
            !key.is_empty()
                && !key.starts_with(|c: char| c.is_ascii_digit())
                && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        None => false,
    }
}

pub fn command_family(cmd: &str) -> String {
    let c = cmd.trim();
    if c.is_empty() {
        return "unknown".to_string();
    }

    let mut parts = c.split_whitespace().skip_while(|t| is_env_assignment(t));
    let first = parts.next().unwrap_or("");
    let second = parts.next().unwrap_or("");
    if first.is_empty() {
        return "unknown".to_string();
    }

    match first {
        "git" => match second {
            "status" | "diff" | "log" | "show" | "grep" | "blame" => format!("git {}", second),
            _ => "git".to_string(),
        },
        "cargo" => match second {
            "build" | "test" | "check" | "run" | "clippy" => format!("cargo {}", second),
            _ => "cargo".to_string(),
        },
        "npm" | "yarn" | "pnpm" | "bun" => match second {
            "install" | "test" | "build" | "run" | "lint" => format!("{} {}", first, second),
            _ => first.to_string(),
        },
        "kubectl" => match second {
            "get" | "describe" | "logs" | "apply" | "delete" => format!("kubectl {}", second),
            _ => "kubectl".to_string(),
        },
        "docker" => match second {
            "build" | "ps" | "logs" | "run" | "images" => format!("docker {}", second),
            _ => "docker".to_string(),
        },
        _ => first.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::command_family;

    #[test]
    fn normalizes_git_commands() {
        assert_eq!(command_family("git diff -- src/main.rs"), "git diff");
        assert_eq!(command_family("git status -s"), "git status");
    }

    #[test]
    fn normalizes_cargo_commands() {
        assert_eq!(command_family("cargo build --release"), "cargo build");
        assert_eq!(command_family("cargo test foo"), "cargo test");
    }

    #[test]
    fn falls_back_to_binary() {
        assert_eq!(command_family("python script.py"), "python");
        assert_eq!(command_family(""), "unknown");
    }

    #[test]
    fn skips_leading_environment_assignments() {
        assert_eq!(
            command_family("OMNI_DB_PATH=/tmp/a1b2/d.db cargo test foo"),
            "cargo test"
        );
        assert_eq!(command_family("FOO=1 BAR=2 git status -s"), "git status");
        // Not assignments: a long flag, and a bare assignment with nothing after it.
        assert_eq!(command_family("rg --replace=x pattern"), "rg");
        assert_eq!(command_family("FOO=1"), "unknown");
    }
}
