use crate::pipeline::{CollapseMode, SegmentationMode};

// Command parsing lives in `producer` now (spec section 5.4). Re-exported rather
// than moved at every call site: the six callers ask registry "which command
// produced this output", which is still the right question to ask a module that
// routes commands, and churning them would have hidden a behaviour-preserving
// move inside a large diff.
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

/// The git verbs this has to tell apart from a global flag's value, the same
/// question `KUBECTL_VERBS` answers for `kubectl` (#805).
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

/// The git subcommand, with the global flags in front of it stepped over.
///
/// `-C <dir>` and `-c <key>=<value>` take a value, `--no-pager` does not, and a
/// verb after either has to survive both. The verb list decides which of the two
/// a token is rather than enumerating every flag git accepts.
fn git_subcommand(command: &str) -> Option<&str> {
    // Quote aware: `split_whitespace` cuts `-C "path with spaces"` into three
    // words, and the third then reads as the verb (PR #812 review).
    let mut tokens = crate::pipeline::producer::words(command)
        .skip_while(|t| t.trim_matches('"').rsplit('/').next() != Some("git"))
        .skip(1)
        .peekable();
    while let Some(raw) = tokens.next() {
        let token = raw.trim_matches('"');
        if !token.starts_with('-') {
            return Some(token);
        }
        // The verb test reads the raw token: `git -C "diff" status` names a
        // directory, and a quoted word is a value however it is spelled
        // (PR #812 review).
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

    // 1. Git: Hunk based
    if base == "git" {
        // The verb, not the word after `git`: `git -C repo diff` read `-C` as the
        // subcommand, fell to line segmentation, and the diff distiller then had
        // no hunk header inside a segment to walk, so it kept the `@@` lines and
        // dropped all 83 changed ones (#805).
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

    // 2. Test Runners: Outcome based
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

    // Cargo subcommand awareness
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

    // 3. Build Tools: Build collapse
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

    // 4. Cloud & Infra: Infra collapse
    if matches!(
        base,
        "docker" | "podman" | "kubectl" | "helm" | "terraform" | "tofu" | "aws" | "gcloud" | "az"
    ) {
        return ToolProfile {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Infra,
        };
    }

    // 5. System Ops & Logs: Log collapse
    if matches!(base, "grep" | "rg" | "cat" | "tail" | "head" | "curl")
        || cmd_lower.contains(".log")
    {
        return ToolProfile {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Log,
        };
    }

    // 6. Database Tools: Log/tabular collapse
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

    // 7. Java/JVM Ecosystem: Build collapse
    if matches!(
        base,
        "java" | "javac" | "mvn" | "gradle" | "gradlew" | "mvnw" | "kotlin" | "kotlinc"
    ) {
        return ToolProfile {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Build,
        };
    }
    // JVM test runners
    if matches!(base, "mvn" | "gradle" | "gradlew") && cmd_lower.contains("test") {
        return ToolProfile {
            segmentation: SegmentationMode::TestGroup,
            collapse: CollapseMode::Test,
        };
    }

    // 8. Mobile Development
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

    // 9. Monorepo & Modern Build Tools
    if matches!(base, "nx" | "turbo" | "bazel" | "pants" | "buck") {
        return ToolProfile {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Build,
        };
    }

    // 10. GitHub & VCS Tools
    if matches!(base, "gh" | "hub" | "glab") {
        return ToolProfile {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Log,
        };
    }

    // 11. Extended Cloud & K8s Dev Tools
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

    // 12. Additional Security & Quality Tools
    if matches!(
        base,
        "semgrep" | "trivy" | "snyk" | "hadolint" | "gosec" | "bandit"
    ) {
        return ToolProfile {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Build, // treat like lint/build errors
        };
    }

    // 13. Deno & Bun: Runtime tests
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

    // 14. Network & System Monitoring
    if matches!(
        base,
        "ping" | "traceroute" | "nmap" | "netstat" | "ss" | "tcpdump" | "htop" | "top"
    ) {
        return ToolProfile {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Log,
        };
    }

    // 15. Database Migration Tools
    if matches!(
        base,
        "alembic" | "flyway" | "liquibase" | "knex" | "typeorm" | "sequelize"
    ) {
        return ToolProfile {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Log,
        };
    }

    // 16. CI/CD Tools
    if matches!(base, "act" | "circleci" | "drone" | "woodpecker" | "tekton") {
        return ToolProfile {
            segmentation: SegmentationMode::Line,
            collapse: CollapseMode::Build,
        };
    }

    ToolProfile::default()
}

/// For command chains (&&, ||, |, ;), return the profile of the most relevant command.
/// “Most relevant” = the last command that is not a simple pipe filter,
/// or the first command if all are equally important.
/// Example:
///   "cargo build && ./app"      → profile from "cargo build"
///   "npm install && npm test"   → profile from "npm test" (test more spesifik)
///   "cat file.log | grep error" → profile from "grep" (spesifik, not cat)
///   "cd /project && ls -la"     → profile from "ls" (action command)
pub fn resolve_profile_for_chain(command: &str) -> ToolProfile {
    // Split on shell operators
    let segments: Vec<&str> = command
        .split(['|', '&', ';'])
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && *s != "&" && *s != "|")
        .collect();

    if segments.is_empty() {
        return ToolProfile::default();
    }

    // Score tiap segment, pilih yang paling spesifik
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

    // Pilih command dengan specificity tertinggi (test runner > build > generic)
    let best = scored.iter().max_by_key(|(_, _, score)| score);

    if let Some((_, cmd, _)) = best {
        resolve_profile(cmd)
    } else {
        resolve_profile(segments[0])
    }
}

/// Specificity score, test runner lebih spesifik dari generic shell command
fn command_specificity(base: &str, full_cmd: &str) -> u8 {
    let cmd_lower = full_cmd.to_lowercase();
    // Test runners, paling spesifik
    if matches!(
        base,
        "pytest" | "jest" | "vitest" | "rspec" | "phpunit" | "playwright"
    ) {
        return 100;
    }
    if (base == "cargo" || base == "go" || base == "npm") && cmd_lower.contains("test") {
        return 95;
    }
    // Build tools
    if matches!(base, "cargo" | "make" | "cmake" | "go" | "mvn" | "gradle") {
        return 80;
    }
    // Cloud/infra
    if matches!(base, "docker" | "kubectl" | "terraform" | "helm") {
        return 75;
    }
    // Grep/find (filter commands, biasanya pipe akhir)
    if matches!(base, "grep" | "rg" | "awk" | "sed") {
        return 60;
    }
    // Generic navigation
    if matches!(base, "cd" | "ls" | "cat" | "echo" | "true" | "false") {
        return 10;
    }
    50 // default
}

// ---------------------------------------------------------------------------
// Command to distiller. `CONTRIBUTING.md` puts every command-to-behaviour mapping in
// this file, and this half of it lived in `distillers/mod.rs` as twenty
// `if matches!(base, …)` blocks until #194.
//
// The move is behaviour-preserving, and the *order* is the behaviour: `mvn` and
// `gradle` appear in both the JVM arm and the build-tool arm, `kubectl … exec`
// has to reach `wraps_another_command` before the cloud arm claims it, and the
// grep arm has to answer before `passes_through_verbatim`. Reordering these is
// not a tidy-up, it is a routing change, which is the class this repo has the
// most defects in (#105, #110, #112, #170, #190, #264, #326).
// ---------------------------------------------------------------------------

fn cargo_subcommand(command: &str) -> Option<&str> {
    let mut tokens = command.split_whitespace().skip_while(|t| {
        // `RUST_BACKTRACE=1 cargo test`, and the cargo path itself.
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

/// Commands whose output `distill_with_command` hands back byte-for-byte, and
/// which the hooks must therefore also exempt from the collapse fallback.
///
/// The four halves reach the same rule for different reasons:
///
/// * **Enumeration** (`ls`, `find`, `ps`, `wc`, `df`, `du`, `stat`, `tree`, bare
///   `env`) lists distinct data one datum per line. The items *are* the answer
///   and there is no noise to drop (#198/#200).
/// * **Bare interpreters** (`python`, `python3`, `ruby`) run an arbitrary
///   program whose stdout is the answer, not a build log (#190).
/// * **File readers** (`cat`, `head`, `tail`, `sed`, `awk`) emit whatever the
///   file holds, so any shape a summariser recognises in it is a coincidence
///   (#235/#236).
/// * **The caller's own filter** (`grep`, `rg`, `ag`) already selected these
///   lines by pattern, so a second selection cannot know what the first was
///   after (#316/#326).
///
/// The single list is the point. A passthrough returns its input, so it can
/// never beat `beats_guardrail`, so the hooks treat it as a distiller that
/// punted and collapse it anyway, which put 40 distinct data rows behind one
/// `[N similar lines collapsed]` marker and reported 95.7% saved (#214). Adding
/// a passthrough arm without adding it here is that bug, so both live in one
/// predicate rather than in a routing arm and a guard that can drift apart.
///
/// The filter half is the one exception to "hands back byte-for-byte", and it is
/// deliberate. `route` reaches the grep arm **before** this predicate, so the
/// grep distiller still runs: it hoists repeated paths losslessly when that
/// shrinks the payload, and returns the input when it does not. Membership here
/// is what stops the hooks collapsing the payload on the second outcome, and it
/// is load-bearing rather than defensive: with these three names removed, 121
/// matched lines came back as `120 INFO entries (collapsed from 120 lines)`.
/// Measured over 214 recorded `grep`/`rg` traces before adding it: the hoist
/// fires on 46 of them for 13.4 KB, which is why the arm stays, and 28 more came
/// back carrying a `[Partial signal]` marker over lines the pattern had matched.
/// `-o tsv`, `--output json`, `-o name` and friends: the value is the tell, not
/// the flag, so `-o wide` and `-o table` stay human-facing and compressible.
///
/// `grep -o` takes a pattern rather than a format, so a `grep -o json` would
/// match here. Harmless: the cost is a missed compression on a search whose
/// pattern happens to be a format name, against reading a truncated payload that
/// something was about to parse.
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

pub fn passes_through_verbatim(command: &str) -> bool {
    let base_exec = extract_base_executable(command);
    let base = std::path::Path::new(&base_exec)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(base_exec.as_str());
    matches!(
        base,
        "ls" | "tree"
            // Query clients. Their rows are the data the caller asked for: a
            // sampled result labelled its first row as the column list (#886),
            // and an error summary read `error:` inside a row as a failed query
            // (#891). Collapse folds identical rows, so it stays off them too.
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
            // `make` is a composite runner, not a build tool. A target runs
            // whatever its recipe says, which is usually several programs, and
            // their output arrives concatenated with no delimiter, so `make check`
            // is tsc, then eslint, then pytest. `BuildDistiller` owned the whole
            // buffer and could only speak for one of them: on a run where each
            // gate found something, 431 B came back as 111 B headed
            // `Build: 1 errors, 0 warnings`, with the eslint finding deleted and
            // a headline that counted neither the warning nor the failed test
            // (#129). That is the same rule already applied to `python`, `ruby`
            // and `az aks command invoke`: a thing that runs an arbitrary
            // program has no grammar of its own, so the honest answer is the
            // input. `gcc`, `clang`, `cmake` and `cargo` still route to the
            // distiller when invoked directly, so a compile still compresses;
            // only the wrapper passes through. Measured before choosing: 0 of
            // 9,832 recorded commands start with `make `, so this trades a
            // compression that has never fired here for a false claim that does.
            | "make"
            // File readers. Their stdout is whatever the file holds, so there is
            // no grammar to distil and every shape a summariser recognises in it
            // is a coincidence. `cat docs/DEVELOPMENT.md`, 127 lines of prose,
            // came back as `tree: 127 entries` because one code block inside the
            // document drew a directory layout (#236), and `sed -n '285,340p'` of
            // a Swift file lost 37 of 56 lines to the enumeration cut (#235).
            // Same reasoning as the interpreters above: an arbitrary payload.
            | "cat"
            | "head"
            | "tail"
            | "sed"
            | "awk"
            // The caller's own filter. The pattern already picked these lines, so
            // the collapse fallback is a second filter that cannot know what the
            // first was looking for (#316/#326). Measured with these three names
            // taken back out: 121 matched lines came back as `120 INFO entries
            // (collapsed from 120 lines)` over a `119 lines omitted` marker. The
            // grep distiller still runs, because `route` reaches it above the
            // call to this predicate, so a lossless hoist is unaffected; this
            // only governs what happens when it punts.
            | "grep"
            | "rg"
            | "ag"
            // Format renderers. Their whole job is to emit something a later step
            // parses, which is the format-safe contract stated in `CONTRIBUTING.md`,
            // and the collapse fallback would leave that payload unparseable.
            // `kubectl get pod -o json | jq -r '...'` lost three of four lines
            // before this (#269).
    )
        // The reshaping tails `registry::owning_tail` routes to, read from the
        // one list rather than repeated here (#277, #194). Naming a stage as the
        // payload's owner only helps if that stage is then handled, and none of
        // these has a grammar: `cut` and `column` project or lay out columns,
        // `tr` and `base64` rewrite bytes, `xargs` runs some other program.
        // Keeping the two in step by comment is what let `gh api … | base64 -d`
        // reach the generic distiller and get cut, on the first run of #277.
        || crate::pipeline::producer::RESHAPING_TAILS.contains(&base)
        // The caller named a parseable output format, which is a statement that
        // something downstream reads this. `format::sniff` covers the shapes it
        // can recognise, and a single-column `-o tsv` has no delimiter to
        // recognise: `az acr repository show-tags … -o tsv` came back as 10 of 25
        // rows, and a time-ordered listing cut at the head keeps what the caller
        // already knew and drops the history they ran it for (#346). Measured
        // over the recorded corpus: 48 traces carry the flag and 3 of them were
        // being visibly shortened, for 1.4 KB that was loss rather than saving.
        || names_a_machine_readable_format(command)
        // `extract_base_executable` strips a leading `env`/`command` wrapper, so
        // bare `env` and `command env` both leave base empty, so match on any `env`
        // token in that case rather than only the first word (review of #203).
        || (base.is_empty() && command.split_whitespace().any(|t| t == "env"))
        // A git command asked for a file list by flag rather than by name (#231).
        // It belongs here and not only in the routing arm: a passthrough that the
        // collapse fallback then folds up is #214, and the file rows of a
        // `--stat` all share a shape.
        || (base == "git" && git_enumerates_files(command))
        // A container listing, for the same reason (#233).
        || lists_containers(command)
        // A kubectl listing that has no columns to summarise (#301).
        || lists_kubectl_names(command)
        // A `gh` or `glab` listing, for the same reason (#833).
        || lists_vcs_items(command)
        // A log stream is the answer, and the generic summariser kept five copies
        // of one ERROR while the INFO lines walking a pool from 1/5 to 5/5 went
        // (#789). 148 of 155 recorded `kubectl logs` in 30 days already passed
        // through; the other seven saved about 22 KB.
        || kubectl_subcommand(command) == Some("logs")
        // A wrapper whose stdout belongs to whatever it ran inside (#234).
        || wraps_another_command(command)
}

/// Commands whose stdout is some other program's, not their own.
///
/// `az aks command invoke` does not produce `az` output: it runs an arbitrary
/// shell command inside a cluster and hands back that program's stdout. Routing
/// on the first executable sent it to a cloud distiller with no grammar for
/// whatever ran, and a four-section run came back as nine rows of the first
/// section plus `[Partial signal]`, with the TLS probe that was the whole point
/// of the call discarded. It cost a 40 second round trip against a private
/// cluster to find that out (#234). `kubectl exec` has form here too: #112 was
/// its output summarised as `docker logs: 9 lines, no errors detected`.
///
/// Same rule as `python`, `python3` and `ruby` in #190: a thing that runs an
/// arbitrary program has no grammar of its own, so the honest answer is the
/// input.
///
/// The alternative was parsing the inner command and routing on that, which
/// keeps the distillation on `ssh host 'cargo build'`. Measured before choosing:
/// 59 of 8,032 recorded commands are wrappers, 0.7%, so that parser and its
/// failure modes would be bought for less than one call in a hundred. If the
/// share grows, revisit it with the same query.
pub(crate) fn wraps_another_command(command: &str) -> bool {
    let mut tokens = command.split_whitespace().map(|t| t.trim_matches('"'));
    let base = tokens
        .next()
        .and_then(|w| std::path::Path::new(w).file_name()?.to_str());

    match base {
        // `ssh host '<cmd>'`, but not `ssh host` alone, which is interactive and
        // produces nothing to distil either way.
        Some("ssh") => true,
        // `kubectl exec … -- <cmd>`, `docker exec … <cmd>`, `podman exec …`,
        // and `run`, which is the same thing with a container created first.
        //
        // `kubectl run --rm -i -- <cmd>` is the standard way to probe from inside
        // a cluster, and its stdout is arbitrary program output. Routed here it
        // was judged against kubectl's grammar, which `curl` headers, `nc`
        // results and `openssl` output were never going to match, so all eight
        // payload lines were discarded and the one line kubectl prints itself,
        // `pod "…" deleted`, was kept (#497).
        //
        // That reads like a probe that ran and found nothing, which is a
        // plausible answer, so there is no way to tell it from the endpoint
        // actually being silent without a retrieval on every call.
        //
        // `run` is included for docker and podman too. The reasoning does not
        // change with the binary, and fixing one door and leaving the others is
        // how #112 and #234 kept coming back. The corpus cannot arbitrate:
        // 6,656 recorded commands hold two `podman run` and no `kubectl run`.
        Some("kubectl") | Some("docker") | Some("podman") => command
            .split_whitespace()
            .any(|t| t == "exec" || t == "run"),
        // `az aks command invoke -c '<cmd>'`, and `az ssh vm|arc … -- <cmd>`,
        // which is ssh behind another binary. Judged as az output, the remote
        // command's 21 lines were cut and the two ssh client warnings kept (#858).
        // `az ssh config` only writes a file, so it stays az's own.
        Some("az") => {
            command.contains("command invoke")
                || (tokens.next() == Some("ssh") && matches!(tokens.next(), Some("vm" | "arc")))
        }
        _ => false,
    }
}

/// `docker ps`, `podman ps` and `docker container ls` list containers one per
/// row, and every column on that row is the answer: name, image, port mapping,
/// uptime.
///
/// The summariser kept the wrong half. `docker: 5 containers | 3 running, 2
/// exited` names only the exited ones by construction, so a fixture with three
/// running containers lost every name, image and port it had, at a reported 91.3%
/// saving and with no marker, because nothing thought anything was lost (#233).
/// An agent runs `docker ps` to find a running container's port far more often
/// than to find a dead one.
///
/// Same reasoning that put `ls`, `find`, `ps`, `df`, `du`, `stat` and `wc` on the
/// list above, and `ps` is the closest relative: a busy machine's process table
/// dwarfs any container list and has been verbatim since 0.6.6. The `-a` case on
/// a host with hundreds of dead containers is the one with real noise, and it
/// stays bounded by `MAX_OUTPUT_BYTES`, which cuts with a marker that says what
/// it removed.
///
/// Deliberately narrow. `docker build` and `docker logs` are logs with real
/// noise and keep their summarisers, and `docker compose ps` is a different
/// command whose output shape has not been measured here.
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

/// A `gh` or `glab` enumeration: `gh issue list`, `gh run list`, `gh secret list`.
///
/// Every row is an item the caller asked to see, and `VcsDistiller` cuts them to
/// the first ten and appends `... [N more items, use --limit to see more]`. That
/// advice names a flag the command already carried: `gh issue list --limit 20`
/// returns twenty rows from `gh`, the reader gets ten, and raising the limit
/// returns more rows and still shows ten (#833).
///
/// The same rule `find` (#198), `ls` and `ps` (#200), `docker ps` (#233) and
/// `kubectl` (#301) already have. #314 closed this class as a recorded
/// non-decision for `aws s3 ls` and `gcloud`, at one occurrence each, and said a
/// family earns a predicate here once it shows real volume. Measured on the
/// reporting machine over 7 days: 219 `gh` commands, 37 of them list shaped,
/// against 14 `kubectl`.
///
/// Shaped as noun then verb rather than "contains `list`", so `gh api` keeps the
/// narrowing #235 gave it and a path or a label called `list` is not a verb.
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
    // Noun, then verb. `gh issue list` and `gh run list` qualify; `gh api
    // repos/o/r/issues` does not, whatever the path spells.
    words.next().is_some() && words.next() == Some("list")
}

/// Whether a raw token is a verb rather than a flag's value.
///
/// Quoted means value: `git -C "diff" status` names a directory called `diff`
/// and `kubectl --context "logs" get pods` a context called `logs`, and reading
/// either as the verb picks the wrong profile entirely (PR #812 review).
fn unquoted_verb(raw: &str, verbs: &[&str]) -> bool {
    !raw.starts_with(['"', '\'']) && verbs.contains(&raw)
}

/// The kubectl verbs this has to tell apart from a flag's value. Not the whole
/// surface: a verb missing from here only means the token after an unlisted
/// value-taking flag is read as the verb, which is what the list is for.
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

/// The kubectl verb, with global flags and the values they take stepped over.
///
/// Matching `logs` anywhere in the string also claims `kubectl get pod logs`,
/// where it is a resource name and the reply is a table worth shortening
/// (PR #809 review).
fn kubectl_subcommand(command: &str) -> Option<&str> {
    // Found by name rather than by position: a recorded command is as often
    // `C=cluster-a kubectl …` or `cd repo && kubectl …` as it is bare.
    let mut tokens = crate::pipeline::producer::words(command)
        .skip_while(|t| t.trim_matches('"').rsplit('/').next() != Some("kubectl"))
        .skip(1)
        .peekable();
    while let Some(raw) = tokens.next() {
        let token = raw.trim_matches('"');
        if !token.starts_with('-') {
            return Some(token);
        }
        // A flag's value is stepped over, and the verb list is what decides
        // which of the two a token is. Enumerating the flags that take a value
        // missed `--request-timeout 60s logs`, and `60s` then read as the verb,
        // which put a log stream back through the summariser (PR #809 review).
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

/// `kubectl` asked for a list of names rather than a table of state.
///
/// `kubectl config get-contexts -o name` prints one identifier per row and
/// nothing else, so there is no status column to summarise and no noise to drop:
/// the identifiers *are* the answer, the same rule as `ls`, `find`, `docker ps`
/// and every other enumeration on this list. `distill_kubectl_generic` keeps only
/// rows that match its critical or `configured|created|unchanged|deleted`
/// vocabulary, and a context name matches neither, so a 20 row list came back as
/// the first 10 with the rest dropped. Because the estate sorts alphabetically,
/// the cut landed on the tail and every `k8s-*` production cluster was the half
/// that went, which is worse than a random ten: the delivered list looks complete
/// and contains no production context at all (#301).
///
/// Narrow on purpose. `kubectl get pods` keeps its distiller, because a pod table
/// has a `STATUS` column with real noise in it and a fingerprint (`READY` +
/// `RESTARTS`) that says so. This covers the forms whose output has no columns:
/// `-o name` on any subcommand, and the three subcommands that only ever
/// enumerate.
fn lists_kubectl_names(command: &str) -> bool {
    let mut tokens = command.split_whitespace().map(|t| t.trim_matches('"'));
    let base = tokens
        .next()
        .and_then(|w| std::path::Path::new(w).file_name()?.to_str());
    if base != Some("kubectl") {
        return false;
    }
    let rest: Vec<&str> = tokens.collect();

    // `-o name`, `--output name`, `-oname`, `--output=name`.
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

/// `--stat`, `--numstat`, `--name-only` and `--name-status` exist only to make
/// git emit a file list. The flag is a reliable signal in a way the subcommand
/// is not: `git show` without one prints a diff and is legitimately distillable,
/// and with one it prints the enumeration that *is* the answer.
///
/// `git show --stat` reached `distill_log`, because the input holds no
/// `diff --git` and no `On branch`. There the `--oneline` subject matched
/// `RE_GIT_LOG_HASH` and was kept, while the stat rows fell past every arm to
/// the rule that drops body lines before the next commit (#199): correct for
/// `git log`, wrong for `git show`, where that position holds the payload. The
/// fail-open guard below `distill_log` then did not fire, because `result` was
/// non-empty: one line *had* been recognised. That is #228 again in a different
/// distiller, a partial recognition disarming a guard whose condition is
/// "recognised nothing", and it published a 79.9% saving for a commit whose
/// four files, three of them screenshots, had all vanished from the answer
/// (#231).
fn git_enumerates_files(command: &str) -> bool {
    command.split_whitespace().any(|t| {
        matches!(
            t,
            "--stat" | "--numstat" | "--shortstat" | "--name-only" | "--name-status"
        )
    })
}

/// Which distiller owns a command's output, or that nothing does.
///
/// A plain `&str -> distiller` map cannot express this, which is why #194 sat
/// open: four arms route on more than the executable. `cargo` routes by
/// subcommand, `git` and `gh` decline for some flag combinations, and the JVM
/// and Dart arms split on whether the command line mentions a test. So the
/// registry answers with a decision rather than a name, and `Passthrough` is one
/// of the answers it can give.
pub enum Distillation {
    /// Nothing here has a grammar for this output. The dispatch boundary hands
    /// the raw bytes back.
    Passthrough,
    Git,
    Security,
    Test,
    Build,
    JsTs,
    /// Carries the base executable, which `CloudDistiller` reads to pick its
    /// per-tool arm.
    Cloud(String),
    /// A `grep`/`rg`/`ag` result set, which only the lossless grep path may touch
    /// (#316/#326).
    UserFiltered,
    SystemOps,
    Generic,
}

/// The one place a command is turned into a distiller.
///
/// Reads the whole command string, not just the executable, because several arms
/// need it. Order is load-bearing; see the banner above.
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

    // `gh` and `glab` are wrappers. `gh api` prints whatever the endpoint
    // returned, and `gh <noun> list` prints an enumeration where every row is an
    // item the caller asked to see. #235 narrowed the summariser to the `list`
    // subcommands on the grounds that `--limit` was the tell; #833 is that the
    // advice is wrong on the one surface it was left: `gh issue list --limit 20`
    // returns twenty rows, the reader gets ten, and raising the limit returns
    // more rows and still shows ten. Nothing here has a summary to give that the
    // caller did not already ask for, so the whole family passes through.
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
        // `cargo` is routed by subcommand, not by executable (#170). Most of
        // cargo is not a build: `cargo tree` prints a dependency tree that *is*
        // the answer, and `BuildDistiller` summarised 21.4 KB of it as
        // `Build: ok`, 9 bytes, reported as a 100% reduction. A command whose
        // output is data must not be handed to a distiller that only knows how
        // to count compile steps.
        if base == "cargo" {
            return match cargo_subcommand(command) {
                Some("test" | "bench") => Distillation::Test,
                Some(
                    "build" | "check" | "run" | "clippy" | "rustc" | "fix" | "doc" | "install",
                ) => Distillation::Build,
                // tree, metadata, search, pkgid, locate-project, vendor, add,
                // remove, update, publish, … output is the answer, hand it back.
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

    // npm/pnpm/yarn/bun. Both halves of the old arm returned the same distiller,
    // so the subcommand check it carried decided nothing; it is gone rather than
    // preserved as decoration.
    if matches!(base, "npm" | "pnpm" | "yarn" | "bun") {
        return Distillation::JsTs;
    }

    // `npx` runs whatever it is handed, and `tsx`, `prisma` or a script print no
    // summary line: a 7 line report came back as its last line (#797). Route by
    // the program it launches, and hand back anything that is not a JS tool.
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
            // `-p typescript tsc` names the package to fetch, not the program to
            // run, so its value is stepped over or it reads as the program and
            // the real tool loses its distiller (PR #809 review).
            // The value-taking options npx documents. An unlisted one leaves its
            // value read as the program, which resolves to passthrough: a missed
            // compression rather than a lost answer (PR #809 review).
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

    // The caller's own filter, which is in `passes_through_verbatim` and so has
    // to be answered before it. `SystemOpsDistiller` cannot serve this: it
    // dispatches on the *shape* of the payload, and a grep result has whatever
    // shape the file had. When none of its detectors matched it fell to
    // `distill_fallback`, which scores by noise and cut 8 of 15 lines a pattern
    // had explicitly matched, under a `[Partial signal]` marker (#316/#326).
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
    // The decomposition these exercise lives in `producer` now; the tests stay
    // here because they were written against the routing they feed.
    use crate::pipeline::producer::is_assignment;

    // Moved out of `distillers/mod.rs` with the functions they cover (#194).
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
        // No subcommand at all, so it routes to passthrough, which is the honest
        // answer for output we cannot classify.
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

    /// The wrapper rule must not swallow the tools themselves.
    #[test]
    fn claims_only_the_wrapping_subcommands() {
        assert!(wraps_another_command("kubectl exec pod -- ls"));
        assert!(wraps_another_command("az aks command invoke -c 'ls'"));
        assert!(wraps_another_command("ssh host 'ls'"));
        // #858. `az ssh vm -- <cmd>` is ssh behind another binary, and its
        // stdout is the remote command's.
        assert!(wraps_another_command(
            "az ssh vm --ip 10.0.0.5 -- -o BatchMode=yes 'sudo du -xsh /data'"
        ));
        assert!(wraps_another_command("az ssh arc -n box -g rg-a -- uptime"));
        // `az ssh config` writes a file and prints az's own output.
        assert!(!wraps_another_command(
            "az ssh config --ip 10.0.0.5 -f ./cfg"
        ));

        // `run` creates the container first and then hands back the program's
        // stdout, which is the same thing `exec` does (#497). The reported case
        // lost all eight payload lines and kept `pod "…" deleted`, which reads
        // like a probe that found nothing.
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

    /// The counter-case, so this is not "never distil docker". A build log and a
    /// container log both have real noise and keep their summarisers.
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

    /// #833. The counter-case matters more than the case here: `gh api` was
    /// narrowed out of this distiller once already (#235), when a Swift file
    /// fetched through it lost 37 of 56 lines under an invitation to raise a
    /// `--limit` that `gh api` does not have.
    #[test]
    fn claims_only_the_gh_listing_subcommands() {
        assert!(lists_vcs_items("gh issue list"));
        assert!(lists_vcs_items("gh pr list --repo owner/name"));
        assert!(lists_vcs_items("gh run list -R owner/name --limit 40"));
        assert!(lists_vcs_items("gh secret list"));
        assert!(lists_vcs_items("glab mr list"));
        assert!(lists_vcs_items("/opt/homebrew/bin/gh release list"));

        // A flag's value never sits between the noun and the verb, because `gh`
        // takes its flags after the subcommand. The filter drops the flag and
        // keeps the value, so a form that did would need position tracking; the
        // CLI does not offer one, and inventing it here would be a guard for a
        // command nobody can type.
        assert!(lists_vcs_items("gh issue list --limit 40 --state open"));

        assert!(!lists_vcs_items("gh api repos/owner/name/issues"));
        assert!(!lists_vcs_items("gh issue view 833"));
        assert!(!lists_vcs_items("gh pr diff 835"));
        assert!(!lists_vcs_items("gh repo clone owner/name"));
        // The word without the shape: a label, not a verb.
        assert!(!lists_vcs_items("gh issue create --label list"));
        assert!(!lists_vcs_items("kubectl get pods"));
    }

    /// #277: a reshaping tail owns the payload, and the list is the measured one
    /// rather than the two names #269 needed. `sed` and `sort` stay out on
    /// purpose: they leave the shape intact and are 334 of the recorded tails
    /// between them, so treating them as reshapers would stop distilling a pod
    /// table because somebody sorted it.
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

    /// #264. `git status && echo === && find .` was routed to the git distiller,
    /// which emits a fixed one-liner whatever the input, so the 40 lines of
    /// `find` the command was run for were deleted with no marker and the ratio
    /// read as a 99% win.
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

    /// The whole reason this is not "any chain is a passthrough": `cd` writes
    /// nothing, so the output still came from one program.
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

    /// A positional filter selects rows out of a shape it leaves intact, so the
    /// output still belongs to whatever fed it and routing does not move.
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

    /// #326. A `grep` tail is not a positional filter: the pattern is the
    /// caller's own selection, so whatever it returned was asked for by name and
    /// nothing downstream may score it again. `kubectl logs … | grep -iE …` was
    /// routed by `kubectl` into a distiller that keeps `is_critical` lines only,
    /// and 14 of 15 matched lines went. The one that survived was an `ERROR`, so
    /// what arrived said the pod had failed while the lines it dropped said it
    /// had come up.
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

    /// #269. `jq` rewrites the payload rather than selecting from it, so the
    /// output is its own. Routed upstream, `kubectl get pod -o json | jq -r '...'`
    /// reached the cloud distiller, which kept `created:` and dropped the pod
    /// phase, the node and the zone: the three fields the command was run to
    /// check.
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

    /// A pipe inside a quoted argument is not a pipe.
    #[test]
    fn does_not_split_a_pipe_inside_quotes() {
        assert_eq!(
            sole_output_command("grep -E \"jenkins|atlantis\" values.yaml"),
            Some("grep -E \"jenkins|atlantis\" values.yaml")
        );
    }

    /// Measured on the maintainer's 5,143 distinct recorded commands: without
    /// this, 202 of them stop being distilled because an operator inside a quoted
    /// argument reads as a chain. Splitting too eagerly only costs savings, so
    /// this is worth its lines and not more.
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

    /// An unbalanced quote leaves the scanner inside a string. One segment out is
    /// the safe answer: it routes as it does today rather than inventing a split.
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

    /// A shell one-liner is one program, and the `;` between its clauses is the
    /// same character that separates two commands. Reading the clauses as
    /// producers turned every loop into a passthrough, which `exec_fail_passthrough`
    /// caught on CI: `while [ $i -lt 60 ]; do echo x; i=$((i+1)); done` writes
    /// stdout from `echo` and from nothing else.
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

    /// #779 review. Routing reads `do cat $f` as `do` and has since the loop
    /// rule shipped, so the keyword stays on the routing answer. A marker names
    /// the command to a reader, and a reader cannot run `do cat $f`.
    #[test]
    fn names_the_command_inside_a_clause_without_its_keyword() {
        use crate::pipeline::producer::without_clause_prefix;
        assert_eq!(without_clause_prefix("do cat $f"), "cat $f");
        assert_eq!(without_clause_prefix("then echo hi"), "echo hi");
        assert_eq!(without_clause_prefix("cat $f"), "cat $f");
        // Nothing but keywords, so there is no command to name.
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

    /// #339: an env-var prefix is not a chain, so `is_silent` never saw it and
    /// the head read as `VENG_DB_PATH=/tmp/d.db`. `kubectl` lost `Infra` and
    /// `cargo test` lost the test profile, silently, on 1,082 of 9,812 recorded
    /// commands.
    #[test]
    fn resolves_the_program_behind_an_env_assignment() {
        assert_eq!(
            resolve_profile_for_chain("VENG_DB_PATH=/tmp/d.db kubectl get pods").collapse,
            CollapseMode::Infra,
        );
        assert_eq!(
            resolve_profile("FOO=bar cargo test").collapse,
            resolve_profile("cargo test").collapse,
        );
        assert_eq!(
            sole_output_command("VENG_DB_PATH=/tmp/d.db kubectl get pods"),
            Some("kubectl get pods"),
        );
        // Several prefixes, and a bare assignment still prints nothing.
        assert_eq!(
            sole_output_command("A=1 B=2 kubectl get pods"),
            Some("kubectl get pods"),
        );
        assert_eq!(
            sole_output_command("A=1 && kubectl get pods"),
            Some("kubectl get pods")
        );
        // A path that merely contains `=` is still a program, not an assignment.
        assert_eq!(
            sole_output_command("./bin/x=y --flag"),
            Some("./bin/x=y --flag")
        );
    }

    /// #338: `kubectl logs … | awk … | sort | uniq -c` is a histogram by the time
    /// it reaches VENG. Routing it by the `kubectl` head handed an already
    /// aggregated 40-row answer to the pod-table distiller, which delivered 10
    /// rows and dropped both traffic spikes the query existed to find.
    #[test]
    fn a_uniq_c_tail_owns_the_payload_it_aggregated() {
        assert_eq!(
            sole_output_command("kubectl -n ns-a logs pod-a | awk '{print $1}' | sort | uniq -c"),
            Some("uniq -c"),
        );
        // Without an aggregating tail the pipeline still belongs to its filter.
        assert_eq!(
            sole_output_command("kubectl -n ns-a logs pod-a | grep err"),
            Some("grep err"),
        );
    }

    /// #346: `az acr repository show-tags … -o tsv` came back as 10 of 25 rows.
    /// `format::sniff` cannot help, because a single-column TSV has no delimiter
    /// to recognise, and a time-ordered listing cut at the head keeps what the
    /// caller already knew and drops the history they ran it for.
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
        // The value is the tell, not the flag: these render for a human and stay
        // compressible.
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
        // grep profile
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

    /// #805, the other half. A hook that resolves the profile from the first
    /// token reads `cd repo && git diff` as a `cd`, which is generic line
    /// segmentation, and the diff distiller then keeps only its headers. #339
    /// fixed that for `post_tool` and left `pipe` on the direct resolver, so the
    /// same payload compressed differently depending on which door it came
    /// through. Asserted over the source because the call site is the defect.
    #[test]
    fn no_hook_resolves_a_profile_from_the_first_token() {
        let hooks = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/hooks");
        for entry in std::fs::read_dir(hooks).expect("src/hooks") {
            let path = entry.expect("entry").path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let body = std::fs::read_to_string(&path).expect("read");
            // Any spelling of the call, not only the qualified one: a `use` of
            // the function walks past the first version of this guard (PR #812
            // review). `resolve_profile_for_chain(` does not contain this
            // needle, so the allowed call stays allowed.
            assert!(
                !body.contains("resolve_profile("),
                "{}: a chain is the shape a hook receives, so it has to use \
                 resolve_profile_for_chain",
                path.display()
            );
        }
    }

    /// #805. The verb decides the profile, and a global flag in front of it hid
    /// it: `git -C repo diff` segmented by line, which left the diff distiller
    /// with no hunk header inside a segment and cost all 83 changed lines.
    #[test]
    fn a_git_global_flag_does_not_hide_the_subcommand() {
        for cmd in [
            "git diff",
            "git -C /tmp/repo diff",
            "git --no-pager -C /tmp/repo diff",
            "git -c core.pager=cat diff",
            // A path with spaces is one word to the shell, and has to be one here.
            "git -C \"/tmp/some path/repo\" diff",
            // A directory called `diff` is a value, not the verb.
            "git -C \"/tmp/diff\" diff",
            "cd /tmp/repo && git diff",
        ] {
            assert_eq!(
                resolve_profile_for_chain(cmd).segmentation,
                SegmentationMode::GitHunk,
                "{cmd}"
            );
        }
        // `--stat` is a file list rather than hunks, and stays where it was.
        assert_eq!(
            resolve_profile_for_chain("git -C /tmp/repo diff --stat").segmentation,
            SegmentationMode::Line
        );
        // The verb after a quoted value that spells one: `status`, not `diff`.
        assert_eq!(
            resolve_profile_for_chain("git -C \"diff\" status").segmentation,
            SegmentationMode::Line
        );
    }

    /// #789. A log stream passes through however the command is prefixed, and a
    /// resource table still reaches the kubectl distiller.
    #[test]
    fn kubectl_logs_pass_through_and_kubectl_get_does_not() {
        for cmd in [
            "kubectl -n demo logs payment-api-0 --tail=60",
            "kubectl --context cluster-a logs -l app=api",
            "kubectl --request-timeout 60s logs payment-api-0",
            // A boolean flag right in front of the verb: the value rule alone
            // eats `logs` here, and the verb list is what keeps it.
            "kubectl --insecure-skip-tls-verify logs payment-api-0",
            "C=cluster-a kubectl --context $C logs -n argocd sts/controller",
        ] {
            assert!(
                matches!(resolve_distiller(cmd), Distillation::Passthrough),
                "{cmd}"
            );
        }
        // `logs` as a resource name is not a log stream.
        for cmd in [
            "kubectl get pods -n demo",
            "kubectl get pod logs",
            "kubectl describe pod logs",
            // A context called `logs` is a value, so this is a `get`.
            "kubectl --context \"logs\" get pods",
        ] {
            assert!(
                matches!(resolve_distiller(cmd), Distillation::Cloud(_)),
                "{cmd}"
            );
        }
    }

    /// #797. A JS tool behind `npx` keeps its distiller; a script runner with no
    /// summary line is handed back rather than cut to its last line.
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
