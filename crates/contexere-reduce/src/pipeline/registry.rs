use crate::pipeline::{CollapseMode, SegmentationMode};

// Command parsing lives in `producer`, re-exported so callers still ask the router.
pub use crate::pipeline::producer::sole_output_command;
pub(crate) use crate::pipeline::producer::strip_assignments;

pub struct ToolProfile {
    pub segmentation: SegmentationMode,
    pub collapse: CollapseMode,
}

impl Default for ToolProfile {
    fn default() -> Self {
        Self {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Generic,
        }
    }
}

/// Git verbs, to tell a verb from a global flag's value (#805).
const GIT_VERBS: &[&str] = &[
    "diff",
    "show",
    "whatchanged",
    "log",
    "status",
    "add",
    "commit",
    "branch",
    "checkout",
    "switch",
    "restore",
    "merge",
    "rebase",
    "fetch",
    "pull",
    "push",
    "stash",
    "tag",
    "blame",
    "grep",
    "ls-files",
    "rev-parse",
    "remote",
    "config",
    "clean",
    "reset",
    "cherry-pick",
    "describe",
    "worktree",
    "apply",
    "bisect",
    "submodule",
    "shortlog",
    "reflog",
    "archive",
    "init",
    "clone",
];

/// The git subcommand past global flags; the verb list decides what is a value.
fn git_subcommand(command: &str) -> Option<&str> {
    // Quote aware, so `-C "path with spaces"` stays one word (PR #812 review).
    let mut tokens = crate::pipeline::producer::words(command)
        .skip_while(|t| t.trim_matches('"').rsplit('/').next() != Some("git"))
        .skip(1)
        .peekable();
    while let Some(raw) = tokens.next() {
        let token = raw.trim_matches('"');
        if !token.starts_with('-') {
            return Some(token);
        }
        // Raw token: a quoted word is a value, so `git -C "diff" status` is `status`.
        if !token.contains('=')
            && tokens
                .peek()
                .is_some_and(|next| !next.starts_with('-') && !unquoted_verb(next, GIT_VERBS))
        {
            tokens.next();
        }
    }
    None
}

/// Shell builtins that write nothing to stdout, so their presence in a chain
pub fn resolve_profile(command: &str) -> ToolProfile {
    if command.is_empty() {
        return ToolProfile::default();
    }

    let cmd = strip_assignments(command.trim());
    let base = crate::pipeline::producer::program_name(cmd);
    let cmd_lower = cmd.to_lowercase();

    if base == "git" {
        // The verb, not the word after `git`: `-C repo diff` once lost all 83 changed lines (#805).
        let sub = git_subcommand(&cmd_lower).unwrap_or("");
        match sub {
            "diff" | "show" | "whatchanged" if !cmd_lower.contains("--stat") => {
                return ToolProfile {
                    segmentation: SegmentationMode::GitHunk,
                    collapse: CollapseMode::Generic,
                };
            }
            _ => {}
        }
    }

    if matches!(
        base,
        "pytest" | "rspec" | "phpunit" | "jest" | "vitest" | "playwright"
    ) {
        return ToolProfile {
            segmentation: SegmentationMode::TestGroup,
            collapse: CollapseMode::Test,
        };
    }
    if (base == "go" || base == "npm" || base == "yarn" || base == "pnpm")
        && (cmd_lower.contains("test") || cmd_lower.contains("check"))
    {
        return ToolProfile {
            segmentation: SegmentationMode::TestGroup,
            collapse: CollapseMode::Test,
        };
    }

    if base == "cargo" {
        let sub = cmd_lower.split_whitespace().nth(1).unwrap_or("");
        return match sub {
            "test" | "nextest" => ToolProfile {
                segmentation: SegmentationMode::TestGroup,
                collapse: CollapseMode::Test,
            },
            "clippy" | "check" => ToolProfile {
                segmentation: SegmentationMode::Line,
                collapse: CollapseMode::Build, // clippy warnings treated like build
            },
            "bench" => ToolProfile {
                segmentation: SegmentationMode::TestGroup,
                collapse: CollapseMode::Test,
            },
            _ => ToolProfile {
                segmentation: SegmentationMode::Line,
                collapse: CollapseMode::Build,
            },
        };
    }

    if matches!(
        base,
        "rustc"
            | "make"
            | "cmake"
            | "gcc"
            | "g++"
            | "clang"
            | "go"
            | "pip"
            | "pip3"
            | "ruby"
            | "rake"
            | "bundle"
    ) {
        return ToolProfile {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Build,
        };
    }

    if matches!(
        base,
        "docker" | "podman" | "kubectl" | "helm" | "terraform" | "tofu" | "aws" | "gcloud" | "az"
    ) {
        return ToolProfile {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Infra,
        };
    }

    if matches!(base, "grep" | "rg" | "cat" | "tail" | "head" | "curl")
        || cmd_lower.contains(".log")
    {
        return ToolProfile {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Log,
        };
    }

    if matches!(
        base,
        "psql"
            | "mysql"
            | "sqlite3"
            | "pg_dump"
            | "pg_restore"
            | "mongodump"
            | "redis-cli"
            | "clickhouse"
    ) {
        return ToolProfile {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Log,
        };
    }

    if matches!(
        base,
        "java" | "javac" | "mvn" | "gradle" | "gradlew" | "mvnw" | "kotlin" | "kotlinc"
    ) {
        return ToolProfile {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Build,
        };
    }
    if matches!(base, "mvn" | "gradle" | "gradlew") && cmd_lower.contains("test") {
        return ToolProfile {
            segmentation: SegmentationMode::TestGroup,
            collapse: CollapseMode::Test,
        };
    }

    if matches!(base, "flutter" | "dart") {
        if cmd_lower.contains("test") || cmd_lower.contains("analyze") {
            return ToolProfile {
                segmentation: SegmentationMode::TestGroup,
                collapse: CollapseMode::Test,
            };
        }
        return ToolProfile {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Build,
        };
    }
    if matches!(base, "swift" | "xcodebuild" | "xcode-select") {
        return ToolProfile {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Build,
        };
    }

    if matches!(base, "nx" | "turbo" | "bazel" | "pants" | "buck") {
        return ToolProfile {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Build,
        };
    }

    if matches!(base, "gh" | "hub" | "glab") {
        return ToolProfile {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Log,
        };
    }

    if matches!(
        base,
        "skaffold"
            | "argocd"
            | "flux"
            | "k3s"
            | "k3d"
            | "kind"
            | "minikube"
            | "kustomize"
            | "cdk"
            | "pulumi"
            | "serverless"
            | "sam"
    ) {
        return ToolProfile {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Infra,
        };
    }

    if matches!(
        base,
        "semgrep" | "trivy" | "snyk" | "hadolint" | "gosec" | "bandit"
    ) {
        return ToolProfile {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Build, // treat like lint/build errors
        };
    }

    if base == "deno" {
        if cmd_lower.contains("test") || cmd_lower.contains("check") {
            return ToolProfile {
                segmentation: SegmentationMode::TestGroup,
                collapse: CollapseMode::Test,
            };
        }
        return ToolProfile {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Build,
        };
    }

    if matches!(
        base,
        "ping" | "traceroute" | "nmap" | "netstat" | "ss" | "tcpdump" | "htop" | "top"
    ) {
        return ToolProfile {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Log,
        };
    }

    if matches!(
        base,
        "alembic" | "flyway" | "liquibase" | "knex" | "typeorm" | "sequelize"
    ) {
        return ToolProfile {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Log,
        };
    }

    if matches!(base, "act" | "circleci" | "drone" | "woodpecker" | "tekton") {
        return ToolProfile {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Build,
        };
    }

    ToolProfile::default()
}

/// The profile for a chain: its most specific command, so
/// `npm install && npm test` profiles as `npm test` and `cd x && ls` as `ls`.
pub fn resolve_profile_for_chain(command: &str) -> ToolProfile {
    let segments: Vec<&str> = command
        .split(['|', '&', ';'])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && *s != "&" && *s != "|")
        .collect();

    if segments.is_empty() {
        return ToolProfile::default();
    }

    let scored: Vec<(usize, &str, u8)> = segments
        .iter()
        .enumerate()
        .map(|(i, seg)| {
            let seg = strip_assignments(seg);
            let base = seg
                .split_whitespace()
                .next()
                .map(|w| w.trim_matches(|c| c == '"' || c == '\''))
                .and_then(|w| std::path::Path::new(w).file_name()?.to_str())
                .unwrap_or("");
            let specificity = command_specificity(base, seg);
            (i, seg, specificity)
        })
        .collect();

    // Highest specificity wins: test runner, then build, then generic.
    let best = scored.iter().max_by_key(|(_, _, score)| score);

    if let Some((_, cmd, _)) = best {
        resolve_profile(cmd)
    } else {
        resolve_profile(segments[0])
    }
}

/// Specificity: test runners highest, plain shell lowest.
fn command_specificity(base: &str, full_cmd: &str) -> u8 {
    let cmd_lower = full_cmd.to_lowercase();
    if matches!(
        base,
        "pytest" | "jest" | "vitest" | "rspec" | "phpunit" | "playwright"
    ) {
        return 100;
    }
    if (base == "cargo" || base == "go" || base == "npm") && cmd_lower.contains("test") {
        return 95;
    }
    if matches!(base, "cargo" | "make" | "cmake" | "go" | "mvn" | "gradle") {
        return 80;
    }
    if matches!(base, "docker" | "kubectl" | "terraform" | "helm") {
        return 75;
    }
    if matches!(base, "grep" | "rg" | "awk" | "sed") {
        return 60;
    }
    if matches!(base, "cd" | "ls" | "cat" | "echo" | "true" | "false") {
        return 10;
    }
    50 // default
}

// Command to distiller, moved here from `distillers/mod.rs` (#194). The order
// is the behaviour: `mvn`/`gradle` are in two arms, `kubectl exec` must reach
// `wraps_another_command` before the cloud arm, and grep must answer before
// `passes_through_verbatim`. Reordering is a routing change.

fn cargo_subcommand(command: &str) -> Option<&str> {
    let mut tokens = command.split_whitespace().skip_while(|t| {
        // `RUST_BACKTRACE=1 cargo test`, and a full cargo path.
        let is_env_assignment = t.contains('=') && !t.starts_with('-');
        let is_cargo = std::path::Path::new(t.trim_matches(['"', '\'', '`']))
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n == "cargo" || n == "cargo.exe")
            .unwrap_or(false);
        is_env_assignment || !is_cargo
    });
    tokens.next()?; // the cargo token itself

    // `cargo +nightly tree`, `cargo --offline tree`
    tokens.find(|t| !t.starts_with('-') && !t.starts_with('+'))
}

fn extract_base_executable(command: &str) -> String {
    let tokens = shell_split_tokens(command, 8);
    if tokens.is_empty() {
        return String::new();
    }

    let mut i = 0usize;
    while i < tokens.len() {
        let t = tokens[i].as_str();
        if t == "env" || t == "command" {
            i += 1;
            continue;
        }
        if looks_like_env_assignment(t) {
            i += 1;
            continue;
        }
        return tokens[i].clone();
    }

    String::new()
}

/// A command naming a parseable output format (`-o tsv`, `--output json`): the
/// value decides, so `-o wide` stays compressible. A `grep -o json` matching
/// too only costs a missed compression.
fn names_a_machine_readable_format(command: &str) -> bool {
    const MACHINE_READABLE: &[&str] = &[
        "tsv",
        "json",
        "jsonc",
        "yaml",
        "name",
        "jsonpath",
        "go-template",
        "custom-columns",
    ];
    let is_format = |v: &str| {
        let v = v.trim_matches(|c| c == '"' || c == '\'');
        MACHINE_READABLE
            .iter()
            .any(|f| v == *f || v.starts_with(&format!("{f}=")))
    };

    let mut words = command.split_whitespace().peekable();
    while let Some(w) = words.next() {
        if let Some(v) = w
            .strip_prefix("--output=")
            .or_else(|| w.strip_prefix("-o="))
        {
            if is_format(v) {
                return true;
            }
        } else if (w == "-o" || w == "--output") && words.peek().is_some_and(|v| is_format(v)) {
            return true;
        }
    }
    false
}

/// Commands whose output is handed back byte for byte, and which the hooks must
/// also keep out of the collapse fallback: enumerations (#198, #200), bare
/// interpreters (#190), file readers (#235, #236), the caller's own filter
/// (#316, #326). One predicate, because a passthrough never beats the guardrail
/// and was then collapsed anyway (#214). For grep the distiller still runs
/// first (`route` reaches it before this) and may hoist losslessly.
pub fn passes_through_verbatim(command: &str) -> bool {
    let base_exec = extract_base_executable(command);
    let base = std::path::Path::new(&base_exec)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(base_exec.as_str());
    matches!(
        base,
        "ls" | "tree"
            // Query clients: the rows are the data (#886, #891).
            | "psql"
            | "mysql"
            | "sqlite3"
            | "pg_dump"
            | "redis-cli"
            | "find"
            | "ps"
            | "df"
            | "du"
            | "stat"
            | "wc"
            | "python"
            | "python3"
            | "ruby"
            // `make` runs whatever its recipe says, concatenated; one distiller cannot
            // speak for tsc, eslint and pytest at once (#129). Compilers invoked directly
            // still route.
            | "make"
            // File readers: the stdout is the file, with no grammar to distil (#235, #236).
            | "cat"
            | "head"
            | "tail"
            | "sed"
            | "awk"
            // The caller's own filter (#316, #326); the grep distiller still runs before this.
            | "grep"
            | "rg"
            | "ag"
            // Format renderers emit something a later step parses (#269).
    )
        // The reshaping tails `owning_tail` routes to, from the one shared list (#277, #194).
        || crate::pipeline::producer::RESHAPING_TAILS.contains(&base)
        // A named parseable format: something downstream reads it (#346).
        || names_a_machine_readable_format(command)
        // A bare `env` leaves the base empty after unwrapping, so match any `env` token (review of #203).
        || (base.is_empty() && command.split_whitespace().any(|t| t == "env"))
        // A git file list by flag (#231); also here, or the fallback folds it (#214).
        || (base == "git" && git_enumerates_files(command))
        // A container listing (#233).
        || lists_containers(command)
        // A kubectl listing with no columns to summarise (#301).
        || lists_kubectl_names(command)
        // A `gh` or `glab` listing (#833).
        || lists_vcs_items(command)
        // A log stream is the answer (#789).
        || kubectl_subcommand(command) == Some("logs")
        // A wrapper: the stdout belongs to what it ran (#234).
        || wraps_another_command(command)
}

/// Commands whose stdout is another program's (`ssh`, `kubectl exec`,
/// `az aks command invoke`), so there is no grammar to distil (#234, #112).
/// Parsing the inner command was rejected: wrappers are 0.7% of recorded
/// commands.
pub(crate) fn wraps_another_command(command: &str) -> bool {
    let mut tokens = command.split_whitespace().map(|t| t.trim_matches('"'));
    let base = tokens
        .next()
        .and_then(|w| std::path::Path::new(w).file_name()?.to_str());

    match base {
        // `ssh host '<cmd>'`; bare `ssh host` is interactive.
        Some("ssh") => true,
        // `exec` and `run` for kubectl, docker and podman: a probe's output judged as
        // kubectl's kept only `pod "…" deleted`, which reads as a silent endpoint
        // (#497).
        Some("kubectl") | Some("docker") | Some("podman") => command
            .split_whitespace()
            .any(|t| t == "exec" || t == "run"),
        // `az aks command invoke` and `az ssh vm|arc -- <cmd>` (#858); `az ssh config` stays az's.
        Some("az") => {
            command.contains("command invoke")
                || (tokens.next() == Some("ssh") && matches!(tokens.next(), Some("vm" | "arc")))
        }
        _ => false,
    }
}

/// `docker ps`, `podman ps` and `docker container ls`: every column is the
/// answer, and the summariser kept only the exited ones (#233). Narrow:
/// `docker build` and `docker logs` keep their summarisers.
fn lists_containers(command: &str) -> bool {
    let mut tokens = command.split_whitespace().map(|t| t.trim_matches('"'));
    let base = tokens
        .next()
        .and_then(|w| std::path::Path::new(w).file_name()?.to_str());
    if !matches!(base, Some("docker") | Some("podman")) {
        return false;
    }
    match tokens.next() {
        Some("ps") => true,
        Some("container") => matches!(tokens.next(), Some("ls") | Some("ps")),
        _ => false,
    }
}

/// A `gh` or `glab` enumeration: every row was asked for, and the cut advised a
/// `--limit` the command already had (#833). Noun then verb, so `gh api` and a
/// label called `list` do not match.
fn lists_vcs_items(command: &str) -> bool {
    let mut words = crate::pipeline::producer::words(command)
        .map(|t| t.trim_matches(['"', '\'']))
        .filter(|t| !t.starts_with('-'));
    let base = words
        .next()
        .and_then(|w| std::path::Path::new(w).file_name()?.to_str());
    if !matches!(base, Some("gh") | Some("glab")) {
        return false;
    }
    words.next().is_some() && words.next() == Some("list")
}

/// Whether a raw token is a verb; quoted means value (PR #812 review).
fn unquoted_verb(raw: &str, verbs: &[&str]) -> bool {
    !raw.starts_with(['"', '\'']) && verbs.contains(&raw)
}

/// Kubectl verbs, to tell a verb from a flag's value.
const KUBECTL_VERBS: &[&str] = &[
    "logs",
    "get",
    "describe",
    "apply",
    "delete",
    "create",
    "edit",
    "patch",
    "exec",
    "run",
    "rollout",
    "scale",
    "top",
    "events",
    "port-forward",
    "cp",
    "attach",
    "explain",
    "diff",
    "wait",
    "label",
    "annotate",
    "set",
    "replace",
    "debug",
    "drain",
    "cordon",
    "uncordon",
    "taint",
    "proxy",
    "auth",
    "config",
    "version",
    "api-resources",
    "api-versions",
    "cluster-info",
];

/// The kubectl verb past global flags, so `get pod logs` is `get` (PR #809 review).
fn kubectl_subcommand(command: &str) -> Option<&str> {
    // Found by name: commands often start with an assignment or a `cd`.
    let mut tokens = crate::pipeline::producer::words(command)
        .skip_while(|t| t.trim_matches('"').rsplit('/').next() != Some("kubectl"))
        .skip(1)
        .peekable();
    while let Some(raw) = tokens.next() {
        let token = raw.trim_matches('"');
        if !token.starts_with('-') {
            return Some(token);
        }
        // The verb list decides what is a value; enumerating flags missed `--request-timeout 60s` (PR #809 review).
        if !token.contains('=')
            && tokens
                .peek()
                .is_some_and(|next| !next.starts_with('-') && !unquoted_verb(next, KUBECTL_VERBS))
        {
            tokens.next();
        }
    }
    None
}

/// `kubectl` asked for names, not a table: `-o name` and the subcommands that
/// only enumerate. A 20-row context list once lost every production cluster to
/// a cut (#301).
fn lists_kubectl_names(command: &str) -> bool {
    let mut tokens = command.split_whitespace().map(|t| t.trim_matches('"'));
    let base = tokens
        .next()
        .and_then(|w| std::path::Path::new(w).file_name()?.to_str());
    if base != Some("kubectl") {
        return false;
    }
    let rest: Vec<&str> = tokens.collect();

    let asks_for_names = rest
        .windows(2)
        .any(|w| matches!(w[0], "-o" | "--output") && w[1] == "name")
        || rest.iter().any(|t| *t == "-oname" || *t == "--output=name");

    asks_for_names
        || matches!(
            rest.first().copied(),
            Some("api-resources") | Some("api-versions")
        )
        || (rest.first().copied() == Some("config")
            && matches!(
                rest.get(1).copied(),
                Some("get-contexts") | Some("get-clusters") | Some("get-users")
            ))
}

fn looks_like_env_assignment(token: &str) -> bool {
    let Some((key, _value)) = token.split_once('=') else {
        return false;
    };
    if key.is_empty() {
        return false;
    }
    key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn shell_split_tokens(input: &str, max_tokens: usize) -> Vec<String> {
    #[derive(Clone, Copy)]
    enum Mode {
        None,
        Single,
        Double,
        Backtick,
    }

    let mut tokens: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut mode = Mode::None;
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if tokens.len() >= max_tokens {
            break;
        }

        match mode {
            Mode::None => match ch {
                '\'' => mode = Mode::Single,
                '"' => mode = Mode::Double,
                '`' => mode = Mode::Backtick,
                '\\' => {
                    if let Some(next) = chars.next() {
                        current.push(next);
                    }
                }
                c if c.is_whitespace() => {
                    if !current.is_empty() {
                        tokens.push(std::mem::take(&mut current));
                    }
                    while matches!(chars.peek(), Some(p) if p.is_whitespace()) {
                        chars.next();
                    }
                }
                _ => current.push(ch),
            },
            Mode::Single => {
                if ch == '\'' {
                    mode = Mode::None;
                } else {
                    current.push(ch);
                }
            }
            Mode::Double => match ch {
                '"' => mode = Mode::None,
                '\\' => {
                    if let Some(next) = chars.next() {
                        current.push(next);
                    }
                }
                _ => current.push(ch),
            },
            Mode::Backtick => match ch {
                '`' => mode = Mode::None,
                '\\' => {
                    if let Some(next) = chars.next() {
                        current.push(next);
                    }
                }
                _ => current.push(ch),
            },
        }
    }

    if !current.is_empty() && tokens.len() < max_tokens {
        tokens.push(current);
    }

    tokens
}

/// `--stat`, `--numstat`, `--name-only` and `--name-status`: a file list is the
/// answer. `git show --stat` once lost every file row (#231).
fn git_enumerates_files(command: &str) -> bool {
    command.split_whitespace().any(|t| {
        matches!(
            t,
            "--stat" | "--numstat" | "--shortstat" | "--name-only" | "--name-status"
        )
    })
}

/// Which distiller owns a command's output, or that nothing does. A decision,
/// not a name, since several arms route on more than the executable (#194).
pub enum Distillation {
    /// No grammar for this output; the raw bytes go back.
    Passthrough,
    Git,
    Security,
    Test,
    Build,
    JsTs,
    /// Carries the base executable for `CloudDistiller`'s per-tool arm.
    Cloud(String),
    /// A `grep`/`rg`/`ag` result, for the lossless grep path only (#316, #326).
    UserFiltered,
    SystemOps,
    Generic,
}

/// The one place a command becomes a distiller. Order is load-bearing; see above.
pub fn resolve_distiller(command: &str) -> Distillation {
    let base_exec = extract_base_executable(command);
    let base = std::path::Path::new(&base_exec)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(base_exec.as_str());
    let cmd_lower = command.to_lowercase();

    if base == "git" {
        return if git_enumerates_files(command) {
            Distillation::Passthrough
        } else {
            Distillation::Git
        };
    }

    if matches!(
        base,
        "semgrep" | "trivy" | "snyk" | "hadolint" | "gosec" | "bandit"
    ) {
        return Distillation::Security;
    }

    // `gh` and `glab` pass through: `gh api` is the endpoint's output, lists are enumerations (#833).
    if matches!(base, "gh" | "hub" | "glab") {
        return Distillation::Passthrough;
    }

    if matches!(
        base,
        "java" | "javac" | "mvn" | "mvnw" | "gradle" | "gradlew"
    ) {
        return if cmd_lower.contains("test") {
            Distillation::Test
        } else {
            Distillation::Build
        };
    }

    if matches!(base, "flutter" | "dart") {
        return if cmd_lower.contains("test") || cmd_lower.contains("analyze") {
            Distillation::Test
        } else {
            Distillation::Build
        };
    }

    if matches!(
        base,
        "cargo"
            | "cmake"
            | "gcc"
            | "g++"
            | "clang"
            | "rustc"
            | "go"
            | "pip"
            | "pip3"
            | "ruff"
            | "mypy"
            | "black"
            | "rake"
            | "rubocop"
            | "dotnet"
            | "gradle"
            | "mvn"
            | "pytest"
            | "rspec"
            | "phpunit"
    ) {
        // `cargo` routes by subcommand: `cargo tree` is data, not a build (#170).
        if base == "cargo" {
            return match cargo_subcommand(command) {
                Some("test" | "bench") => Distillation::Test,
                Some(
                    "build" | "check" | "run" | "clippy" | "rustc" | "fix" | "doc" | "install",
                ) => Distillation::Build,
                // Data subcommands: the output is the answer.
                _ => Distillation::Passthrough,
            };
        }

        if cmd_lower.contains("test")
            || cmd_lower.contains("pytest")
            || matches!(base, "pytest" | "rspec" | "phpunit")
        {
            return Distillation::Test;
        }
        return Distillation::Build;
    }

    if matches!(
        base,
        "vitest" | "playwright" | "tsc" | "eslint" | "prettier" | "jest" | "esbuild" | "vite"
    ) {
        return Distillation::JsTs;
    }

    if matches!(base, "npm" | "pnpm" | "yarn" | "bun") {
        return Distillation::JsTs;
    }

    // `npx` routes by the program it launches; non-JS tools pass through (#797).
    if base == "npx" {
        let mut rest = command
            .split_whitespace()
            .skip_while(|t| t.rsplit('/').next() != Some("npx"))
            .skip(1);
        let mut launched: Vec<&str> = Vec::new();
        while let Some(token) = rest.next() {
            if !token.starts_with('-') {
                launched.push(token);
                break;
            }
            // Step over the documented value-taking options (`-p typescript tsc`); an
            // unlisted one only costs a compression (PR #809 review).
            if matches!(
                token,
                "-p" | "--package"
                    | "-c"
                    | "--call"
                    | "--node-options"
                    | "--registry"
                    | "--userconfig"
                    | "--cache"
                    | "--shell"
                    | "--npm"
            ) {
                rest.next();
            }
        }
        launched.extend(rest);
        return match resolve_distiller(&launched.join(" ")) {
            d @ (Distillation::JsTs | Distillation::Test) => d,
            _ => Distillation::Passthrough,
        };
    }

    // Before `passes_through_verbatim`: shape-based dispatch cannot serve a grep result (#316, #326).
    if matches!(base, "grep" | "rg" | "ag") {
        return Distillation::UserFiltered;
    }

    if passes_through_verbatim(command) {
        return Distillation::Passthrough;
    }

    if matches!(
        base,
        "docker"
            | "podman"
            | "kubectl"
            | "helm"
            | "terraform"
            | "tofu"
            | "aws"
            | "gcloud"
            | "az"
            | "doctl"
    ) {
        return Distillation::Cloud(base.to_string());
    }

    if matches!(
        base,
        "curl"
            | "wget"
            | "sort"
            | "uniq"
            | "tar"
            | "zip"
            | "unzip"
            | "apt"
            | "apt-get"
            | "brew"
            | "yum"
            | "dnf"
    ) {
        return Distillation::SystemOps;
    }

    Distillation::Generic
}

#[cfg(test)]
mod tests {
    use crate::pipeline::producer::is_assignment;

    #[test]
    fn reads_the_cargo_subcommand_past_prefixes_and_flags() {
        assert_eq!(cargo_subcommand("cargo tree"), Some("tree"));
        assert_eq!(cargo_subcommand("cargo tree --depth 1"), Some("tree"));
        assert_eq!(
            cargo_subcommand("RUST_BACKTRACE=1 cargo test"),
            Some("test")
        );
        assert_eq!(cargo_subcommand("cargo +nightly build"), Some("build"));
        assert_eq!(cargo_subcommand("cargo --offline tree"), Some("tree"));
        assert_eq!(
            cargo_subcommand("/usr/local/bin/cargo clippy"),
            Some("clippy")
        );
        assert_eq!(cargo_subcommand("cargo"), None);
        assert_eq!(cargo_subcommand("cargo --version"), None);
    }

    #[test]
    fn test_extract_base_executable_handles_quotes_and_env_prefixes() {
        assert_eq!(extract_base_executable("git diff"), "git");
        assert_eq!(
            extract_base_executable("\"/usr/local/bin/cargo\" build"),
            "/usr/local/bin/cargo"
        );
        assert_eq!(extract_base_executable("'cargo' test"), "cargo");
        assert_eq!(
            extract_base_executable("`/usr/bin/python3` -V"),
            "/usr/bin/python3"
        );
        assert_eq!(
            extract_base_executable("RUST_BACKTRACE=1 cargo test"),
            "cargo"
        );
        assert_eq!(
            extract_base_executable("env FOO=1 \"/path/to/git\" status"),
            "/path/to/git"
        );
    }

    #[test]
    fn claims_only_the_wrapping_subcommands() {
        assert!(wraps_another_command("kubectl exec pod -- ls"));
        assert!(wraps_another_command("az aks command invoke -c 'ls'"));
        assert!(wraps_another_command("ssh host 'ls'"));
        // #858
        assert!(wraps_another_command(
            "az ssh vm --ip 10.0.0.5 -- -o BatchMode=yes 'sudo du -xsh /data'"
        ));
        assert!(wraps_another_command("az ssh arc -n box -g rg-a -- uptime"));
        assert!(!wraps_another_command(
            "az ssh config --ip 10.0.0.5 -f ./cfg"
        ));

        // #497
        assert!(wraps_another_command(
            "kubectl -n probe run p --rm -i --restart=Never --image=busybox --command -- sh -c 'echo hi'"
        ));
        assert!(wraps_another_command("docker run --rm busybox echo hi"));
        assert!(wraps_another_command("podman run --rm alpine ls"));

        assert!(!wraps_another_command("kubectl get pods"));
        assert!(!wraps_another_command("az aks show -n cluster"));
        assert!(!wraps_another_command("docker build ."));
        assert!(!wraps_another_command("cargo test"));
    }

    /// Build and container logs keep their summarisers.
    #[test]
    fn claims_only_the_container_listing_subcommands() {
        assert!(lists_containers("docker ps"));
        assert!(lists_containers("/usr/local/bin/docker ps -a"));
        assert!(lists_containers("podman container ls"));

        assert!(!lists_containers("docker build ."));
        assert!(!lists_containers("docker logs app"));
        assert!(!lists_containers("docker compose ps"));
        assert!(!lists_containers("kubectl get pods"));
        assert!(!lists_containers("ps aux"));
    }

    /// `gh api` stays out of the list rule (#833, #235).
    #[test]
    fn claims_only_the_gh_listing_subcommands() {
        assert!(lists_vcs_items("gh issue list"));
        assert!(lists_vcs_items("gh pr list --repo owner/name"));
        assert!(lists_vcs_items("gh run list -R owner/name --limit 40"));
        assert!(lists_vcs_items("gh secret list"));
        assert!(lists_vcs_items("glab mr list"));
        assert!(lists_vcs_items("/opt/homebrew/bin/gh release list"));

        // `gh` takes flags after the subcommand, so nothing sits between noun and verb.
        assert!(lists_vcs_items("gh issue list --limit 40 --state open"));

        assert!(!lists_vcs_items("gh api repos/owner/name/issues"));
        assert!(!lists_vcs_items("gh issue view 833"));
        assert!(!lists_vcs_items("gh pr diff 835"));
        assert!(!lists_vcs_items("gh repo clone owner/name"));
        assert!(!lists_vcs_items("gh issue create --label list"));
        assert!(!lists_vcs_items("kubectl get pods"));
    }

    /// A reshaping tail owns the payload; `sed` and `sort` do not (#277).
    #[test]
    fn a_reshaping_tail_owns_the_payload_and_a_selecting_one_does_not() {
        for (cmd, expect) in [
            (
                "kubectl get pod -o json | jq -r '.items[].metadata.name'",
                "jq -r '.items[].metadata.name'",
            ),
            ("kubectl get pods | cut -d' ' -f1", "cut -d' ' -f1"),
            ("cat access.log | tr -d '\\r'", "tr -d '\\r'"),
            (
                "gh api repos/o/r/contents/f --jq '.content' | base64 -d",
                "base64 -d",
            ),
            ("ps aux | wc -l", "wc -l"),
        ] {
            assert_eq!(
                sole_output_command(cmd).map(str::trim),
                Some(expect),
                "`{cmd}` ends in a stage that rewrites the payload"
            );
        }

        for cmd in [
            "kubectl get pods | head -20",
            "cargo test | tail -30",
            "kubectl get pods | sort",
            "kubectl get pods | sed 's/Running/UP/'",
        ] {
            let owner = sole_output_command(cmd).map(str::trim).unwrap_or("");
            assert!(
                owner.starts_with("kubectl")
                    || owner.starts_with("cargo")
                    || owner.starts_with("git"),
                "`{cmd}` only selects from or rewrites within its input's shape, so the producer keeps it; got {owner:?}"
            );
        }
    }

    use super::*;

    /// A chain with two producers passes through (#264).
    #[test]
    fn declines_a_chain_written_to_by_several_commands() {
        assert_eq!(
            sole_output_command("git status && echo '=== tree ===' && find . -type f"),
            None
        );
        assert_eq!(sole_output_command("git log --oneline -12 && ls"), None);
        assert_eq!(sole_output_command("npm run build; npm test"), None);
    }

    #[test]
    fn routes_a_lone_command_to_itself() {
        assert_eq!(sole_output_command("cargo test"), Some("cargo test"));
    }

    /// `cd` writes nothing, so one producer remains.
    #[test]
    fn looks_past_a_shell_builtin_that_prints_nothing() {
        assert_eq!(
            sole_output_command("cd /project && cargo test"),
            Some("cargo test")
        );
        assert_eq!(
            sole_output_command("export CI=1 && npm test"),
            Some("npm test")
        );
    }

    /// A positional filter keeps the shape, so routing does not move.
    #[test]
    fn keeps_a_pipeline_intact() {
        assert_eq!(
            sole_output_command("kubectl get pods | head -20"),
            Some("kubectl get pods | head -20")
        );
        assert_eq!(
            sole_output_command("cargo test | tail -30"),
            Some("cargo test | tail -30")
        );
    }

    /// A `grep` tail owns the payload (#326).
    #[test]
    fn hands_a_pipeline_to_the_grep_that_filtered_it() {
        for (cmd, expect) in [
            (
                "kubectl -n devops logs jean-0 --tail=60 2>&1 | grep -iE 'error|ready'",
                "grep -iE 'error|ready'",
            ),
            ("cat app.log | grep ERROR", "grep ERROR"),
            ("git log --oneline | grep fix", "grep fix"),
            ("kubectl get pods -A | rg -i crashloop", "rg -i crashloop"),
        ] {
            assert_eq!(
                sole_output_command(cmd).map(str::trim),
                Some(expect),
                "`{cmd}` ends in the caller's own filter"
            );
        }
    }

    /// `jq` owns its output (#269).
    #[test]
    fn hands_a_pipeline_to_the_stage_that_reshaped_it() {
        assert_eq!(
            sole_output_command("kubectl get pod x -o json | jq -r '.status.phase'"),
            Some("jq -r '.status.phase'")
        );
        assert_eq!(
            sole_output_command("kubectl kustomize . | yq '.spec'"),
            Some("yq '.spec'")
        );
    }

    #[test]
    fn does_not_split_a_pipe_inside_quotes() {
        assert_eq!(
            sole_output_command("grep -E \"jenkins|atlantis\" values.yaml"),
            Some("grep -E \"jenkins|atlantis\" values.yaml")
        );
    }

    /// Quoted operators are not chains: 202 of 5,143 commands depend on it.
    #[test]
    fn does_not_split_on_an_operator_inside_quotes() {
        assert_eq!(
            sole_output_command("git commit -m \"fix: a && b\""),
            Some("git commit -m \"fix: a && b\"")
        );
        assert_eq!(
            sole_output_command("awk '/^kind:/{f=1} f && /^---/{f=0}' out.yaml"),
            Some("awk '/^kind:/{f=1} f && /^---/{f=0}' out.yaml")
        );
    }

    /// An unbalanced quote yields one segment.
    #[test]
    fn treats_an_unbalanced_quote_as_one_command() {
        assert_eq!(
            sole_output_command("echo \"oops && ls"),
            Some("echo \"oops && ls")
        );
    }

    #[test]
    fn returns_none_for_an_empty_command() {
        assert_eq!(sole_output_command(""), None);
        assert_eq!(sole_output_command("   "), None);
        assert_eq!(sole_output_command(" && ; "), None);
    }

    /// A loop's `;` does not split it into producers.
    #[test]
    fn reads_a_shell_loop_as_the_one_command_that_prints() {
        assert_eq!(
            sole_output_command(
                "i=0; while [ $i -lt 60 ]; do echo noise; i=$((i+1)); done; exit 0"
            ),
            Some("do echo noise")
        );
        assert_eq!(
            sole_output_command("for f in *.yaml; do cat $f; done"),
            Some("do cat $f")
        );
    }

    /// Markers name the program; routing keeps the keyword (#779 review).
    #[test]
    fn names_the_command_inside_a_clause_without_its_keyword() {
        use crate::pipeline::producer::without_clause_prefix;
        assert_eq!(without_clause_prefix("do cat $f"), "cat $f");
        assert_eq!(without_clause_prefix("then echo hi"), "echo hi");
        assert_eq!(without_clause_prefix("cat $f"), "cat $f");
        assert_eq!(without_clause_prefix("do then"), "do then");
    }

    #[test]
    fn treats_a_variable_assignment_as_printing_nothing() {
        assert_eq!(
            sole_output_command("f=deploy.yaml && cat $f"),
            Some("cat $f")
        );
        assert!(!is_assignment("./bin/tool"));
        assert!(!is_assignment("=leading"));
        assert!(is_assignment("PATH=/usr/bin"));
    }

    /// An env prefix does not change the producer (#339).
    #[test]
    fn resolves_the_program_behind_an_env_assignment() {
        assert_eq!(
            resolve_profile_for_chain("OMNI_DB_PATH=/tmp/d.db kubectl get pods").collapse,
            CollapseMode::Infra,
        );
        assert_eq!(
            resolve_profile("FOO=bar cargo test").collapse,
            resolve_profile("cargo test").collapse,
        );
        assert_eq!(
            sole_output_command("OMNI_DB_PATH=/tmp/d.db kubectl get pods"),
            Some("kubectl get pods"),
        );
        assert_eq!(
            sole_output_command("A=1 B=2 kubectl get pods"),
            Some("kubectl get pods"),
        );
        assert_eq!(
            sole_output_command("A=1 && kubectl get pods"),
            Some("kubectl get pods")
        );
        assert_eq!(
            sole_output_command("./bin/x=y --flag"),
            Some("./bin/x=y --flag")
        );
    }

    /// `uniq -c` owns its histogram (#338).
    #[test]
    fn a_uniq_c_tail_owns_the_payload_it_aggregated() {
        assert_eq!(
            sole_output_command("kubectl -n ns-a logs pod-a | awk '{print $1}' | sort | uniq -c"),
            Some("uniq -c"),
        );
        assert_eq!(
            sole_output_command("kubectl -n ns-a logs pod-a | grep err"),
            Some("grep err"),
        );
    }

    /// A named parseable format passes through (#346).
    #[test]
    fn a_machine_readable_output_flag_passes_the_payload_through() {
        for cmd in [
            "az acr repository show-tags -n r --repository p --top 25 -o tsv",
            "az acr repository show-tags --output=tsv",
            "kubectl get pods -o name",
            "gh pr list --output json",
            "kubectl get pod x -o jsonpath='{.status.phase}'",
        ] {
            assert!(
                passes_through_verbatim(cmd),
                "`{cmd}` names a format something downstream parses"
            );
        }
        // Human formats stay compressible.
        for cmd in ["kubectl get pods -o wide", "az vm list -o table"] {
            assert!(
                !passes_through_verbatim(cmd),
                "`{cmd}` is human-facing and must still compress"
            );
        }
    }

    #[test]
    fn splits_on_newlines_and_double_pipes_too() {
        assert_eq!(sole_output_command("git status\nls -la"), None);
        assert_eq!(sole_output_command("cargo build || cargo clean"), None);
    }

    #[test]
    fn test_registry_flutter_test_gets_test_profile() {
        let p = resolve_profile("flutter test");
        assert_eq!(p.segmentation, SegmentationMode::TestGroup);
        assert_eq!(p.collapse, CollapseMode::Test);
    }

    #[test]
    fn test_registry_cargo_clippy_gets_build_profile() {
        let p = resolve_profile("cargo clippy -- -D warnings");
        assert_eq!(p.collapse, CollapseMode::Build);
    }

    #[test]
    fn test_registry_psql_gets_log_profile() {
        let p = resolve_profile("psql -U myuser mydb");
        assert_eq!(p.collapse, CollapseMode::Log);
    }

    #[test]
    fn test_registry_nx_test_gets_build_profile() {
        let p = resolve_profile("nx test my-app");
        assert_eq!(p.collapse, CollapseMode::Build);
    }

    #[test]
    fn test_registry_unknown_command_gets_default() {
        let p = resolve_profile("my_custom_script.sh --verbose");
        assert_eq!(p.segmentation, SegmentationMode::Line);
        assert_eq!(p.collapse, CollapseMode::Generic);
    }

    #[test]
    fn test_chain_npm_install_then_test_picks_test() {
        let p = resolve_profile_for_chain("npm install && npm test");
        assert_eq!(p.segmentation, SegmentationMode::TestGroup);
    }

    #[test]
    fn test_chain_cat_pipe_grep_picks_grep() {
        let p = resolve_profile_for_chain("cat app.log | grep ERROR");
        assert_eq!(p.segmentation, SegmentationMode::Line);
    }

    #[test]
    fn test_chain_cargo_build_then_run_picks_cargo() {
        let p = resolve_profile_for_chain("cargo build && ./target/debug/app");
        assert_eq!(p.collapse, CollapseMode::Build);
    }

    #[test]
    fn test_single_command_unchanged() {
        let p1 = resolve_profile("pytest");
        let p2 = resolve_profile_for_chain("pytest");
        assert_eq!(p1.segmentation, p2.segmentation);
        assert_eq!(p1.collapse, p2.collapse);
    }

    /// Hooks resolve a chain's profile, never the first token's (#805). A source
    /// scan, because the call site was the defect.
    #[test]
    fn no_hook_resolves_a_profile_from_the_first_token() {
        let hooks = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/hooks");
        for entry in std::fs::read_dir(hooks).expect("src/hooks") {
            let path = entry.expect("entry").path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let body = std::fs::read_to_string(&path).expect("read");
            // Any spelling of the call; the chain resolver does not contain this needle.
            assert!(
                !body.contains("resolve_profile("),
                "{}: a chain is the shape a hook receives, so it has to use \
                 resolve_profile_for_chain",
                path.display()
            );
        }
    }

    /// The verb decides the profile, past global flags (#805).
    #[test]
    fn a_git_global_flag_does_not_hide_the_subcommand() {
        for cmd in [
            "git diff",
            "git -C /tmp/repo diff",
            "git --no-pager -C /tmp/repo diff",
            "git -c core.pager=cat diff",
            "git -C \"/tmp/some path/repo\" diff",
            "git -C \"/tmp/diff\" diff",
            "cd /tmp/repo && git diff",
        ] {
            assert_eq!(
                resolve_profile_for_chain(cmd).segmentation,
                SegmentationMode::GitHunk,
                "{cmd}"
            );
        }
        assert_eq!(
            resolve_profile_for_chain("git -C /tmp/repo diff --stat").segmentation,
            SegmentationMode::Line
        );
        assert_eq!(
            resolve_profile_for_chain("git -C \"diff\" status").segmentation,
            SegmentationMode::Line
        );
    }

    /// A log stream passes through; a resource table still distils (#789).
    #[test]
    fn kubectl_logs_pass_through_and_kubectl_get_does_not() {
        for cmd in [
            "kubectl -n demo logs payment-api-0 --tail=60",
            "kubectl --context cluster-a logs -l app=api",
            "kubectl --request-timeout 60s logs payment-api-0",
            // The verb list keeps `logs` after a boolean flag.
            "kubectl --insecure-skip-tls-verify logs payment-api-0",
            "C=cluster-a kubectl --context $C logs -n argocd sts/controller",
        ] {
            assert!(
                matches!(resolve_distiller(cmd), Distillation::Passthrough),
                "{cmd}"
            );
        }
        for cmd in [
            "kubectl get pods -n demo",
            "kubectl get pod logs",
            "kubectl describe pod logs",
            "kubectl --context \"logs\" get pods",
        ] {
            assert!(
                matches!(resolve_distiller(cmd), Distillation::Cloud(_)),
                "{cmd}"
            );
        }
    }

    /// JS tools behind `npx` keep their distiller; scripts pass through (#797).
    #[test]
    fn npx_routes_by_the_program_it_launches() {
        for cmd in [
            "npx vitest run",
            "npx -y tsc --noEmit",
            "npx jest",
            "npx -p typescript tsc --noEmit",
            "npx --registry https://registry.npmjs.org tsc --noEmit",
        ] {
            assert!(
                matches!(resolve_distiller(cmd), Distillation::JsTs),
                "{cmd}"
            );
        }
        for cmd in [
            "npx tsx /tmp/report.mts",
            "npx prisma migrate status",
            "npx",
        ] {
            assert!(
                matches!(resolve_distiller(cmd), Distillation::Passthrough),
                "{cmd}"
            );
        }
    }
}
