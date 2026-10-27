//! #184, the reproducible net-savings benchmark.
//!
//! Replays every `execution_traces.raw_input` through the CURRENT pipeline and
//! aggregates raw vs distilled bytes, so the published headline (docs/BENCHMARKS.md,
//! README) can be re-measured on the shipped binary rather than trusted from a run
//! nobody kept. This is the committed reproducer the README's "Numbers you can
//! reproduce" promises.
//!
//! Since #392 it also reports the two things the byte figure cannot express:
//! **line-level repetition**, which no filter touches and which the session ledger
//! exists to harvest, and **real tokens** from `cl100k_base` rather than a bytes
//! divisor. Both are printed for raw input and for post-filter output, because the
//! whole claim of the ledger is that the two mechanisms are orthogonal.
//!
//! Ignored by default, it needs a populated trace DB. Run it explicitly:
//!
//!   VENG_BENCH_DB=~/.veng/veng.db \
//!     cargo test --release --test bench_replay -- --ignored --nocapture
//!
//! Knobs, all optional:
//!   VENG_BENCH_ALL=1            replay terminal traces too (not the figure to publish)
//!   VENG_BENCH_SINCE=YYYY-MM-DD restrict to traces at or after that UTC date
//!   VENG_BENCH_NO_TOKENS=1      skip tokenization (it is the slow half)
//!   VENG_BENCH_RTK / _LEANCTX / _CAVEMAN / _HEADROOM
//!                               each names a competitor binary and adds one arm to
//!                               the head to head. Unset means the arm is absent from
//!                               the table rather than printed as a zero, so CI never
//!                               needs a competitor installed.
//!
//! Faithfulness to docs/BENCHMARKS.md's method:
//! - `session: None` + `store: None` → the scorer sees no history, i.e. the
//!   "fresh HOME per invocation" the method requires (a warm DB is non-deterministic).
//! - `HOME` is pointed at an empty temp dir so only the embedded signals load, not
//!   whatever `~/.veng/signals` the measuring machine happens to carry.
//! - `run_inner` is the same full pipeline (format gate → TOML → distill → guardrail)
//!   the hook and `veng exec` run, so this measures what an agent actually receives.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Cursor;

/// Below this a line is not evidence of repetition, it is punctuation. `}`,
/// `---`, a bare path fragment and a blank recur in every output and would
/// dominate the count while representing nothing a ledger could hand back as a
/// handle. Matches the 12 characters the corpus measurement in
/// `docs/specs/2026-08-08-veng-direction.md` section 2.3 used, so the two
/// numbers are comparable.
const MIN_REPEAT_LINE: usize = 12;

/// `ledger::Origin::Session`'s marker with a 16 character handle, in bytes.
///
/// `Origin` is private, so this is a copy, and a copy that drifts would make the
/// attribution below quietly wrong. `session_marker_len_matches_the_ledger`
/// re-derives it from the ledger's own output and fails if the two part ways.
const SESSION_MARKER_LEN: u64 = 62;

/// A trace, with the two identities repetition is scoped by.
struct Trace {
    command: String,
    raw: String,
    session: String,
    project: String,
}

#[derive(Default)]
struct Repetition {
    /// Bytes in lines long enough to be counted at all.
    accounted: u64,
    /// Of those, bytes whose line already appeared in the same session.
    same_session: u64,
    /// Bytes whose line is new to this session but was seen in another session
    /// of the same project. This is the share only a project-scoped ledger can
    /// reach, and it is the one the plan defers to P3.
    same_project: u64,
}

impl Repetition {
    fn pct(&self, part: u64) -> f64 {
        100.0 * part as f64 / self.accounted.max(1) as f64
    }
}

/// Exact line text, not a hash. The corpus is a few million bytes of distinct
/// lines, so a `HashSet<String>` costs less memory than the argument about
/// collision rates would cost time, and it cannot be wrong.
#[derive(Default)]
struct Seen {
    by_session: HashMap<String, HashSet<String>>,
    by_project: HashMap<String, HashSet<String>>,
}

impl Seen {
    fn account(&mut self, t: &Trace, text: &str, rep: &mut Repetition) {
        for line in text.lines() {
            let line = line.trim();
            if line.len() < MIN_REPEAT_LINE {
                continue;
            }
            let bytes = line.len() as u64;
            rep.accounted += bytes;

            // `insert` returns false when the line was already there, which is
            // the question being asked. Both sets are always updated, so a line
            // counted for the session also becomes known to the project.
            let repeat_in_session = !self
                .by_session
                .entry(t.session.clone())
                .or_default()
                .insert(line.to_string());
            let repeat_in_project = !self
                .by_project
                .entry(t.project.clone())
                .or_default()
                .insert(line.to_string());

            if repeat_in_session {
                rep.same_session += bytes;
            } else if repeat_in_project {
                rep.same_project += bytes;
            }
        }
    }
}

/// Repetition seen through the ledger's own eyes, so the bytes it declines can
/// be attributed to the gate that declined them (#450).
///
/// `Seen` above answers a different question: it counts lines of at least
/// `MIN_REPEAT_LINE` characters to describe the corpus. This one keys every line
/// with `ledger::line_key`, in payload order, so its notion of "already shown"
/// is byte-identical to the ledger's and the two totals can be subtracted.
#[derive(Default)]
struct GapSeen {
    by_session: HashMap<String, HashSet<String>>,
    by_project: HashMap<String, HashSet<String>>,
}

impl GapSeen {
    /// Per line, whether the ledger would already know it and how long it is,
    /// then records the payload in both scopes exactly as `project_inner` does.
    fn account(&mut self, t: &Trace, text: &str) -> Vec<(bool, u64)> {
        let keys: Vec<(String, u64)> = text
            .split_inclusive('\n')
            .map(|l| (veng::ledger::line_key(l), l.len() as u64))
            .collect();

        // Session first, project only for what the session did not answer,
        // which is the precedence `origin_of` applies.
        let mut seen: Vec<bool> = {
            let session = self.by_session.entry(t.session.clone()).or_default();
            keys.iter().map(|(k, _)| session.contains(k)).collect()
        };
        {
            let project = self.by_project.entry(t.project.clone()).or_default();
            for (i, (k, _)) in keys.iter().enumerate() {
                if !seen[i] && project.contains(k) {
                    seen[i] = true;
                }
            }
        }

        let session = self.by_session.entry(t.session.clone()).or_default();
        for (k, _) in &keys {
            session.insert(k.clone());
        }
        let project = self.by_project.entry(t.project.clone()).or_default();
        for (k, _) in &keys {
            project.insert(k.clone());
        }

        keys.iter()
            .enumerate()
            .map(|(i, (_, len))| (seen[i], *len))
            .collect()
    }
}

/// Real tokens, or `None` when tokenization is switched off.
///
/// `cl100k_base` is GPT's vocabulary, not Claude's, and saying so is the point:
/// it is a **measured proxy** for a vocabulary that is not published, which is
/// still strictly better than a bytes divisor presented as a token count. The
/// same encoding produced the 3.614 bytes/token calibration that
/// `util::token_estimate` ships, so this harness can also check that constant
/// against the corpus instead of trusting it.
///
/// It lives here and not in the binary on purpose. #174 removed exact counting
/// from the hook after measuring it at 34.3 ms of a 10 ms budget. A dev
/// dependency in an `--ignored` benchmark pays that cost where there is no
/// budget to blow.
struct Counter(Option<tiktoken_rs::CoreBPE>);

impl Counter {
    fn new() -> Self {
        if std::env::var("VENG_BENCH_NO_TOKENS").is_ok() {
            return Self(None);
        }
        Self(Some(
            tiktoken_rs::cl100k_base().expect("embedded cl100k_base vocabulary"),
        ))
    }

    fn count(&self, text: &str) -> u64 {
        match &self.0 {
            Some(bpe) => bpe.encode_with_special_tokens(text).len() as u64,
            None => 0,
        }
    }

    fn on(&self) -> bool {
        self.0.is_some()
    }
}

/// The shell shape a command was typed in, which section 2.2 of the direction
/// doc measures separately from what program ran.
///
/// It is here because that section's claim ("`cd` prefix and `VAR=` assignment
/// have not been addressed at all") was true of the corpus and is not true of
/// the code: `cd` has been in `registry::SILENT_BUILTINS` since 2026-08-02 and
/// `strip_assignments` landed with #341 on 2026-08-07, inside the same window
/// the 0.8% and 2.0% were measured over. Replaying through the current pipeline
/// is the only way to tell what is still open, so the harness reports it rather
/// than the plan asserting it.
fn command_form(cmd: &str) -> &'static str {
    let c = cmd.trim();
    if c.starts_with("cd ") {
        return "cd prefix";
    }
    let first = c.split_whitespace().next().unwrap_or("");
    if first
        .split_once('=')
        .is_some_and(|(name, _)| !name.is_empty() && !name.contains('/'))
    {
        return "VAR= assignment";
    }
    if c.contains("&&") || c.contains(';') || c.contains("||") {
        return "chain";
    }
    if c.contains('|') {
        return "pipe only";
    }
    "bare program"
}

/// The command classes section 2.1 of the direction doc is argued in, so the P1
/// gate ("file-read class above 15%") can be read straight off a replay.
///
/// Classified by the program that produced the output where one can be resolved,
/// so `cd x && cat y` counts as a file read rather than as "other". That is the
/// same resolver the pipeline routes with, which keeps this table honest about
/// what the pipeline saw.
fn trace_class(cmd: &str) -> &'static str {
    let producer = veng::pipeline::registry::sole_output_command(cmd).unwrap_or(cmd);
    match base_command(producer).as_str() {
        "cat" | "head" | "tail" | "sed" | "less" | "bat" => "file read",
        "grep" | "rg" | "ag" | "ack" | "find" | "fd" => "search",
        "git" | "gh" => "git",
        "cargo" | "npm" | "pnpm" | "yarn" | "make" | "pytest" | "go" | "mvn" | "gradle"
        | "jest" | "vitest" | "tsc" => "build and test",
        "kubectl" | "az" | "aws" | "docker" | "helm" | "terraform" => "infra",
        _ => "other",
    }
}

/// The rtk `pipe` filter that governs a command, if any.
///
/// Deliberately **generous to rtk**: it is handed the exact filter name, which
/// its own hook has to infer from the command line. Anything unmapped gets its
/// bytes back, which is what `rtk pipe` with no filter does anyway, and is the
/// same treatment veng's passthrough gives.
///
/// Names come from `resolve_filter` in rtk's `cmds/system/pipe_cmd.rs`.
fn rtk_filter(cmd: &str) -> Option<&'static str> {
    let producer = veng::pipeline::registry::sole_output_command(cmd).unwrap_or(cmd);
    let mut words = producer.split_whitespace();
    let base = words.next().unwrap_or("").rsplit('/').next().unwrap_or("");
    let sub = words.next().unwrap_or("");
    Some(match (base, sub) {
        ("cargo", _) => "cargo",
        ("git", "log") => "git-log",
        ("git", "diff") => "git-diff",
        ("git", "status") => "git-status",
        ("go", "test") => "go-test",
        ("go", "build") => "go-build",
        ("grep", _) | ("rg", _) => "grep",
        ("find", _) | ("fd", _) => "find",
        ("pytest", _) => "pytest",
        ("mypy", _) => "mypy",
        ("tsc", _) => "tsc",
        ("vitest", _) => "vitest",
        ("prettier", _) => "prettier",
        ("phpunit", _) => "phpunit",
        ("phpstan", _) => "phpstan",
        ("pint", _) => "pint",
        _ => return None,
    })
}
// Checked against rtk 0.45.0's own `resolve_filter` (`src/cmds/system/pipe_cmd.rs:12`).
// Seven of its 25 names are not mapped above: `log`, `ruff-check`, `ruff-format`,
// `pest`, `paratest`, `php-test`, `ecs`. Six have zero traces in this corpus, so
// adding them would be dead arms. `log` is the one that could matter, since the
// heaviest commands here are `tail` on log files, and it stays out on purpose:
// rtk's own hook (`src/hooks/rewrite_cmd.rs`) maps no command to it either, so
// routing `tail` there would be inventing a claim rtk does not make for itself.

/// Runs one payload through rtk, or hands it back when nothing claims it.
/// What rtk hands back, so the ledger can be measured on top of it.
fn rtk_out(rtk: &str, cmd: &str, raw: &str) -> String {
    let Some(filter) = rtk_filter(cmd) else {
        return raw.to_string();
    };
    use std::io::Write;
    let Ok(mut child) = std::process::Command::new(rtk)
        .args(["pipe", "--filter", filter])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        return raw.to_string();
    };
    if let Some(mut si) = child.stdin.take() {
        let _ = si.write_all(raw.as_bytes());
    }
    match child.wait_with_output() {
        // Empty stdout is a failure, not a payload that compressed to nothing, and
        // rtk exits that way on a filter name it does not know:
        //   $ rtk pipe --filter zzznotreal < cargo_test_500.txt | wc -c
        //   0
        //   rtk: Unknown filter 'zzznotreal'. Available: cargo-test, pytest, ...
        // Counted as delivered bytes that would score the payload at 100% saved and
        // silently inflate the competitor. One typo in the map below is all it takes.
        Ok(o) if !o.stdout.is_empty() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => raw.to_string(),
    }
}

/// What caveman delivers for one payload.
///
/// `caveman-engine compress` takes the payload on stdin and writes the compressed
/// text on **stdout**; its metrics JSON goes to stderr, so unlike the lean-ctx arm
/// nothing has to be parsed out of the bytes being counted, and our ledger can be
/// stacked on top the way it is on rtk.
///
/// **The arm was `caveman tools compress` until #712, and there is no longer a
/// `caveman` to run it.** `~/.caveman/bin` holds `caveman-engine`, `caveman-shrink`
/// and four others; nothing on this machine answers to a bare `caveman`. The 0.7.5
/// table's 6.8% came from a wrapper that is gone, so that row cannot be reproduced
/// and is history rather than a baseline.
///
/// What makes this worth a comment is how it fails now. Pointed at `caveman-shrink`,
/// the plausible guess, `tools compress` is not a subcommand it knows: it falls
/// through to shrink mode, types every payload as `toolschema` and returns the input
/// with `"ratio":0`. A competitor arm reporting zero on every trace reads as a
/// competitor that lost, which is #711's shape with a different binary.
///
/// `caveman-engine compress` is the real entry point, and it is self-contained: it
/// needs neither `CAVEMAN_HOME` nor the other binaries (verified by running it under
/// an empty `HOME` with the variable unset). The env is kept because it costs
/// nothing and the sibling binaries do read it.
///
/// **It is handed less than the other two arms.** rtk gets the filter name and
/// lean-ctx gets `--shell <cmd>`; caveman-engine takes no command hint at all
/// (`--shell`, `--command`, `--cmd` and `--hint` each make it exit with empty
/// stdout), so it infers the shape from content alone. That handicap runs against
/// it, which is worth saying out loud in the direction that does not flatter us.
fn caveman_out(bin: &str, home: &str, raw: &str) -> String {
    use std::io::Write;
    let Ok(mut child) = std::process::Command::new(bin)
        .args(["compress"])
        // The harness repoints HOME, which would leave caveman unable to find its
        // own binaries and silently passing everything through. See the call site.
        .env("CAVEMAN_HOME", home)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        return raw.to_string();
    };
    if let Some(mut si) = child.stdin.take() {
        let _ = si.write_all(raw.as_bytes());
    }
    match child.wait_with_output() {
        // An empty stdout means the process failed rather than that the payload
        // compressed to nothing, so it is a passthrough and not a free win.
        Ok(o) if !o.stdout.is_empty() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => raw.to_string(),
    }
}

/// What lean-ctx would deliver for one payload, in bytes.
///
/// `compress diff - --shell "<cmd>" --json` is its own preview path: the payload
/// arrives on stdin and `--shell` names the command that produced it, which is
/// the same courtesy the rtk arm extends. It is not in `lean-ctx --help`, only in
/// `lean-ctx help all`.
///
/// **It reports rather than emits.** The command prints `compressed_bytes` and a
/// line diff, never the compressed text, so this arm can measure lean-ctx and
/// cannot stack our ledger on top of it the way the rtk arm does. That asymmetry
/// is why there is no `lean-ctx + our ledger` row: the row would have to be
/// estimated, and an estimate next to four measurements is the kind of blend this
/// harness exists to avoid.
///
/// Anything it declines to claim gets its bytes back, matching the rtk arm and
/// veng's own passthrough.
fn lean_ctx_bytes(bin: &str, cmd: &str, raw: &str) -> usize {
    use std::io::Write;
    let producer = veng::pipeline::registry::sole_output_command(cmd).unwrap_or(cmd);
    let Ok(mut child) = std::process::Command::new(bin)
        .args(["compress", "diff", "-", "--shell", producer, "--json"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        return raw.len();
    };
    if let Some(mut si) = child.stdin.take() {
        let _ = si.write_all(raw.as_bytes());
    }
    let Ok(out) = child.wait_with_output() else {
        return raw.len();
    };
    // A missing or unparseable field means the payload was not claimed, not that
    // it compressed to nothing. Falling back to `raw.len()` keeps an unclaimed
    // command a passthrough instead of a free win.
    serde_json::from_slice::<serde_json::Value>(&out.stdout)
        .ok()
        .and_then(|v| v.get("compressed_bytes")?.as_u64())
        .map_or(raw.len(), |n| n as usize)
}

/// Bytes headroom's cross-turn dedup leaves, over the same blocks the ledger saw.
///
/// One python process for the whole corpus rather than one per payload: the arm
/// is a whole-conversation transform, so it needs every session's blocks in order
/// anyway, and the lean-ctx arm's per-trace spawning is what makes a full run take
/// tens of minutes.
///
/// Three things the module makes you learn the hard way, all of them cheap to
/// state and expensive to find:
///
/// - the dataclass field is `text`, not `output`, and `turn` must be a stable
///   absolute ordinal because it is printed into the pointer it leaves behind.
/// - the module has to be registered in `sys.modules` **before** `exec_module`,
///   or `@dataclass` raises `AttributeError: 'NoneType' object has no attribute
///   '__dict__'`, which reads like a corrupt file and is not.
/// - it needs python3.13. The 3.8 commonly first on `PATH` cannot parse it.
///
/// Returns `None` when anything at all goes wrong, because a competitor arm that
/// silently reports zero would read as a win.
fn headroom_total(named: &str, blocks: &BTreeMap<String, Vec<String>>) -> Option<u64> {
    let module = match headroom_module(named) {
        Some(m) => m,
        None => {
            eprintln!("headroom arm failed: no module file behind {named}");
            return None;
        }
    };
    let dir = tempfile::tempdir().ok()?;
    let payload = dir.path().join("blocks.json");
    std::fs::write(&payload, serde_json::to_vec(blocks).ok()?).ok()?;

    let driver = dir.path().join("drive.py");
    std::fs::write(
        &driver,
        r#"import importlib.util, json, sys
spec = importlib.util.spec_from_file_location("ctd", sys.argv[1])
m = importlib.util.module_from_spec(spec)
sys.modules["ctd"] = m          # @dataclass resolves its owning module by name
spec.loader.exec_module(m)

total = 0
for _session, texts in json.load(open(sys.argv[2])).items():
    blocks = [m.DedupBlock(text=t, turn=i) for i, t in enumerate(texts)]
    folded, _stats = m.dedup_blocks(blocks)
    total += sum(len(b.text) for b in folded)
print(total)
"#,
    )
    .ok()?;

    let out = std::process::Command::new("python3.13")
        .arg(&driver)
        .arg(&module)
        .arg(&payload)
        .output()
        .ok()?;
    if !out.status.success() {
        eprintln!(
            "headroom arm failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// The module file behind an installed `headroom`, given whatever the caller
/// named: the file itself, or the command they actually have.
///
/// `VENG_BENCH_HEADROOM` documents a path inside headroom's own tree, and what a
/// person has is the console script, so naming `headroom` handed
/// `spec_from_file_location` something it cannot read and the arm never ran once
/// (#711). The script's shebang names the interpreter whose site-packages hold
/// the module, and that interpreter is the only thing that can be asked where it
/// is. python3.13 can exec the file afterwards wherever it came from, which is
/// what lets an install on an older python still answer.
fn headroom_module(named: &str) -> Option<String> {
    if named.ends_with(".py") {
        return Some(named.to_string());
    }
    // PATH walked here rather than through `sh -c "command -v"`: the name comes
    // from the environment, and a shell would evaluate whatever it contains.
    let exe = if named.contains(std::path::MAIN_SEPARATOR) {
        std::path::PathBuf::from(named)
    } else {
        std::env::split_paths(&std::env::var_os("PATH")?)
            .map(|dir| dir.join(named))
            .find(|candidate| is_executable(candidate))?
    };
    let script = std::fs::read_to_string(&exe).ok()?;
    let mut words = script
        .lines()
        .next()?
        .strip_prefix("#!")?
        .split_whitespace();
    let mut interpreter = words.next()?;
    // `#!/usr/bin/env python3.13` puts the interpreter one token along.
    if interpreter.ends_with("/env") {
        interpreter = words.next()?;
    }
    let out = std::process::Command::new(interpreter)
        .arg("-c")
        .arg("import headroom.transforms.cross_turn_dedup as m; print(m.__file__)")
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|p| !p.is_empty())
}

/// What `command -v` means by a hit, which `is_file` alone does not: a
/// non-executable file earlier on `PATH` would otherwise shadow the real
/// command and be read for its shebang (review of #730).
///
/// `access` rather than the mode bits, on the second pass of the same review:
/// `mode & 0o111` is true for a file executable by a group this process is not
/// in, so an unusable shadow would still stop the search. The kernel is the only
/// thing that can answer the question this is asking. libc is already a direct
/// unix dependency here.
#[cfg(unix)]
fn is_executable(path: &std::path::Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `c_path` is a NUL-terminated string that outlives the call, and
    // `access` only reads it.
    path.is_file() && unsafe { libc::access(c_path.as_ptr(), libc::X_OK) } == 0
}

#[cfg(not(unix))]
fn is_executable(path: &std::path::Path) -> bool {
    path.is_file()
}

fn base_command(cmd: &str) -> String {
    cmd.split_whitespace()
        .find(|t| !t.contains('=') && !t.starts_with('-'))
        .unwrap_or("")
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string()
}

/// `VENG_BENCH_SINCE=YYYY-MM-DD` as a unix timestamp, or 0 for the whole corpus.
///
/// The doc's headline (43.3% all-time) and its recent window (5.6% for
/// 2026-08-01 onward) are the same measurement over different rows, and
/// conflating them is the mistake section 1 of the direction doc exists to
/// correct. One env var keeps both reproducible from one harness.
fn since_ts() -> i64 {
    let Ok(day) = std::env::var("VENG_BENCH_SINCE") else {
        return 0;
    };
    chrono::NaiveDate::parse_from_str(&day, "%Y-%m-%d")
        .expect("VENG_BENCH_SINCE must be YYYY-MM-DD")
        .and_hms_opt(0, 0, 0)
        .expect("midnight")
        .and_utc()
        .timestamp()
}

#[test]
#[ignore = "needs a populated trace DB; run with --ignored (see file header)"]
fn replay_execution_traces_net_savings() {
    // Resolve the corpus DB from the REAL home before we repoint HOME below.
    // Build the path with PathBuf, not a hardcoded `/` (CLAUDE.md cross-platform
    // rule 1), review of #202.
    let db = std::env::var("VENG_BENCH_DB").unwrap_or_else(|_| {
        std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join(".veng")
            .join("veng.db")
            .to_string_lossy()
            .into_owned()
    });
    // #704. `VENG_BENCH_CORPUS` names a frozen corpus and replaces the DB as the
    // source. It exists because `TRACE_RETENTION_DAYS` is 7, so replaying the DB
    // measures a different input every week and no release-over-release delta was
    // ever attributable to the code. Resolved from the real HOME, like the two
    // paths above, because HOME is repointed at a temp dir a few lines down.
    let corpus = std::env::var("VENG_BENCH_CORPUS").ok();
    let since = since_ts();
    // Also from the real home, and for the same reason. caveman resolves its own
    // Go binaries under `$HOME/.caveman/bin`, so with HOME repointed below it
    // finds nothing and hands every payload back unchanged. That failure is
    // silent: the arm would print a clean 0.0% and read as a measurement rather
    // than as a competitor that never ran. Verified before this line existed, on
    // `heavy_noise.txt`: 9,207 bytes back under an empty HOME against 5,524 with
    // `CAVEMAN_HOME` set.
    //
    // An operator who already set `CAVEMAN_HOME` has a non-default install, so that
    // value wins and only the fallback is derived from HOME. Deriving it either way
    // would break exactly the case this line exists to protect.
    let caveman_home = std::env::var("CAVEMAN_HOME").unwrap_or_else(|_| {
        std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join(".caveman")
            .to_string_lossy()
            .into_owned()
    });

    // Match the method: no user config, no passthrough shortcut.
    let tmp_home = tempfile::tempdir().expect("temp home");
    // SAFETY: this integration binary holds exactly one test, so no other test
    // reads the environment concurrently while these mutations run (review of
    // #202). If a second test is ever added here, serialize env access first.
    unsafe {
        std::env::set_var("HOME", tmp_home.path());
        std::env::remove_var("VENG_PASSTHROUGH");
    }

    // The frozen corpus is already filtered to model-facing traces and already in
    // `ts, id` order, both done when it was built. Nothing here re-derives either:
    // the file is the corpus, and re-sorting or re-filtering it would make the
    // replay disagree with the manifest that names it.
    let from_corpus = corpus.as_ref().map(|path| {
        let text =
            std::fs::read_to_string(path).unwrap_or_else(|e| panic!("read corpus {path}: {e}"));
        let rows: Vec<Trace> = text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .enumerate()
            .map(|(i, line)| {
                let v: serde_json::Value = serde_json::from_str(line).expect("corpus line is JSON");
                // Reject, never default. `unwrap_or_default` on a missing field
                // hands back an empty string, and an empty `session` collapses
                // unrelated traces into one ledger scope, which is #118's defect
                // exactly, while an empty payload silently moves the byte totals.
                // A corpus that cannot be read is a failure, not a measurement.
                let s = |k: &str| -> String {
                    v.get(k)
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_else(|| panic!("corpus line {i} has no string `{k}`"))
                        .to_string()
                };
                // `as_str` accepts `""`, so presence is not enough. An empty
                // `session` or `project` puts unrelated traces in one ledger
                // scope, and an empty `payload` moves the byte totals with
                // nothing to show it did. The builder cannot emit either, so this
                // is the reader refusing to trust a file it did not write.
                let required = |k: &str| -> String {
                    let v = s(k);
                    assert!(!v.is_empty(), "corpus line {i} has an empty `{k}`");
                    v
                };
                Trace {
                    // `command` may legitimately be empty: a trace can carry a
                    // payload with no command string, and the builder writes it
                    // through rather than inventing one.
                    command: s("command"),
                    raw: required("payload"),
                    session: required("session"),
                    project: required("project"),
                }
            })
            .collect();
        assert!(!rows.is_empty(), "corpus at {path} has no traces");
        rows
    });

    if from_corpus.is_none() && !std::path::Path::new(&db).exists() {
        eprintln!("SKIP: no trace DB at {db} (set VENG_BENCH_DB or VENG_BENCH_CORPUS)");
        return;
    }

    let conn = rusqlite::Connection::open(if from_corpus.is_some() {
        ":memory:"
    } else {
        db.as_str()
    })
    .expect("open trace db");
    // Two populations, because the difference between them is the number this
    // project keeps having to correct. `terminal` rows are TTY output no model
    // ever receives, and on the reporting installation they are 888 traces
    // carrying 86 MB, 68% of the corpus by bytes. Replaying everything reported
    // **79.1%** where the model-facing population reports **43.1%**, and the
    // README quoted the harness. #212 fixed exactly this in `veng stats`, which
    // now prints "Terminal output is excluded" on its own line; the harness that
    // produces the published figure never got the same fix (#324).
    //
    // Both are printed rather than one being chosen, so the gap stays visible
    // instead of being a decision someone has to remember.
    //
    // Ordered by time because repetition is a question about what came before.
    // Replayed out of order, the first sighting of a line can be counted as the
    // repeat and the number is quietly wrong.
    let fetch = |where_clause: &str| -> Vec<Trace> {
        let sql = format!(
            "SELECT command, raw_input, session_id, project_path FROM execution_traces \
             WHERE ts >= ?1 {where_clause} ORDER BY ts, id"
        );
        let mut stmt = conn.prepare(&sql).expect("prepare");
        stmt.query_map([since], |r| {
            Ok(Trace {
                command: r.get(0)?,
                raw: r.get(1)?,
                session: r.get(2)?,
                project: r.get(3)?,
            })
        })
        .expect("query")
        .filter_map(Result::ok)
        .collect()
    };

    // A frozen corpus answers all three of these itself: it was filtered to
    // model-facing traces when it was built, so there is no terminal population
    // to choose between and `VENG_BENCH_ALL` has nothing to widen to. Asking the
    // empty in-memory DB instead would return zero rows and report a clean 0.0%,
    // which is the shape of failure this file already documents twice.
    let (rows, total_traces, use_everything) = match from_corpus {
        Some(rows) => {
            let n = rows.len();
            (rows, n, false)
        }
        None => {
            let model_facing = fetch("AND agent_id IS NOT NULL AND agent_id != 'terminal'");
            let everything = fetch("");
            let total = everything.len();
            let all = std::env::var("VENG_BENCH_ALL").is_ok() || model_facing.is_empty();
            (if all { everything } else { model_facing }, total, all)
        }
    };
    // Ask the flag, not the row count. Comparing lengths reads "no terminal rows
    // were excluded" as "terminal rows were included", and a corpus with no
    // terminal traces at all then reports itself under the label that exists to
    // warn against publishing it. Caught on the first real run of #392, on a
    // corpus that is 7,067 traces of `claude_code` and nothing else.
    let excluded = total_traces - rows.len();
    let population = if use_everything {
        "every trace, terminal included, which is not the figure to publish"
    } else {
        "traces whose result reached a model, terminal excluded per #212"
    };

    assert!(!rows.is_empty(), "trace DB has no rows to replay");
    assert!(
        rows.iter().any(|t| !t.raw.is_empty()),
        "all trace rows have empty raw_input, nothing to measure"
    );

    // The ledger's gate (#394) needs the ledger in the loop, and `run_inner`
    // takes `store: None` on purpose so the scorer stays history-free. So the
    // ledger gets its own throwaway store, keyed by each trace's real session
    // id, applied to what the filters produced. That is exactly where the hook
    // applies it, and it keeps the filter figure above unaffected.
    let ledger_dir = tempfile::tempdir().expect("ledger home");
    let ledger_store =
        veng::store::sqlite::Store::open_path(&ledger_dir.path().join("ledger.db")).ok();
    let (mut ledger_total, mut ledger_calls) = (0u64, 0u64);
    // #448. `VENG_BENCH_PROJECT=off` drops the project scope so the two can be
    // measured apart. There is no env var for the floor: it is
    // `guard::limits::PROJECT_FLOOR_MULT`, and changing the arm means changing
    // the constant. An `VENG_PROJECT_FLOOR_MULT` was advertised here and read by
    // nothing but this file's own reporting line, so setting it relabelled the
    // output without moving the arm (#472).
    let project_scope = std::env::var("VENG_BENCH_PROJECT").ok().as_deref() != Some("off");
    // #450's three gates, in repeated bytes the ledger never got to claim.
    let mut gap_seen = GapSeen::default();
    let (mut gap_structured, mut gap_under_floor, mut gap_processed) = (0u64, 0u64, 0u64);
    let (mut n_structured, mut n_under_floor) = (0u64, 0u64);
    // M1 split by the bound that rejected the run.
    let (mut m1_under_bar, mut m1_eligible) = (0u64, 0u64);
    let (mut m1_under_bar_runs, mut m1_eligible_runs) = (0u64, 0u64);
    // Every run of already-seen lines, so the marker's own size can be priced
    // as the variable it is rather than assumed.
    let mut run_sizes: Vec<u64> = Vec::new();
    // P4's head-to-head. Off unless `VENG_BENCH_RTK` names an rtk binary, so CI
    // never needs a competitor installed to run this benchmark.
    let rtk = std::env::var("VENG_BENCH_RTK").ok();
    let (mut rtk_total, mut rtk_claimed, mut rtk_marked) = (0u64, 0u64, 0u64);
    let rtk_ledger_dir = tempfile::tempdir().expect("rtk ledger home");
    let rtk_ledger_store =
        veng::store::sqlite::Store::open_path(&rtk_ledger_dir.path().join("ledger.db")).ok();
    let mut rtk_ledger_total = 0u64;
    // Second competitor arm, same rule: off unless the binary is named.
    let lean_ctx = std::env::var("VENG_BENCH_LEANCTX").ok();
    let (mut lc_total, mut lc_claimed) = (0u64, 0u64);
    // Fourth arm. `VENG_BENCH_CAVEMAN` names `~/.caveman/bin/caveman-engine`, not a
    // bare `caveman`, which does not exist. It is the first competitor that ships
    // both halves of VENG's shape: `compress` is the filter tier and `retrieve
    // <handle>` recovers the content behind the handle it leaves in the output, so
    // it earns the same ledger-stacked row rtk gets rather than a filters-only row.
    let caveman = std::env::var("VENG_BENCH_CAVEMAN").ok();
    let (mut cm_total, mut cm_claimed) = (0u64, 0u64);
    let cm_ledger_dir = tempfile::tempdir().expect("caveman ledger home");
    let cm_ledger_store =
        veng::store::sqlite::Store::open_path(&cm_ledger_dir.path().join("ledger.db")).ok();
    let mut cm_ledger_total = 0u64;
    // Third arm, and the only one that is not a filter. `VENG_BENCH_HEADROOM`
    // names headroom's `transforms/cross_turn_dedup.py`, which is the same idea
    // as our ledger rather than the same idea as rtk: whole-conversation verbatim
    // dedup, not per-payload pattern stripping. So it is fed exactly what the
    // ledger is fed, post-filter output grouped by session in recorded order,
    // and compared against `ledger_total` rather than against `out_total`.
    let headroom = std::env::var("VENG_BENCH_HEADROOM").ok();
    let mut hr_blocks: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut hr_structured = 0u64;

    let counter = Counter::new();
    let (mut n, mut raw_total, mut out_total) = (0u64, 0u64, 0u64);
    let (mut raw_tokens, mut out_tokens) = (0u64, 0u64);
    let (mut shrank, mut unchanged, mut grew, mut errored) = (0u64, 0u64, 0u64, 0u64);
    let mut grew_detail: Vec<String> = Vec::new();
    // Separate ledgers: the filtered stream must not be told a line is a repeat
    // because the raw stream saw it.
    let (mut seen_raw, mut seen_out) = (Seen::default(), Seen::default());
    let (mut rep_raw, mut rep_out) = (Repetition::default(), Repetition::default());
    // base command -> (calls, raw_bytes, out_bytes, raw_tokens, out_tokens)
    let mut per_cmd: BTreeMap<String, (u64, u64, u64, u64, u64)> = BTreeMap::new();
    // shell shape -> (calls, raw_bytes, out_bytes), the P2 gate
    let mut per_form: BTreeMap<&'static str, (u64, u64, u64)> = BTreeMap::new();
    // command class -> (calls, raw, after filters, after ledger), the P1 gate
    // (calls, raw, post-filter, post-ledger, repeated bytes available, accounted).
    // The last two are the ceiling #708 reports against: the ledger cannot fold a
    // line that was never delivered before, so a savings percentage swings with how
    // repetitive the week was while the ratio against the ceiling does not.
    type ClassRow = (u64, u64, u64, u64, u64, u64);
    let mut per_class: BTreeMap<&'static str, ClassRow> = BTreeMap::new();
    let started = std::time::Instant::now();

    for t in &rows {
        let mut out = Vec::new();
        let mut err = std::io::sink();
        // None store + None session = fresh, deterministic, no persistence. A
        // failed replay would leave `out` empty and be counted as 100% savings,
        // inflating the honest figure (review of #202), exclude it entirely and
        // report the count instead.
        if veng::hooks::pipe::run_inner(
            Cursor::new(t.raw.as_bytes()),
            &mut out,
            &mut err,
            None,
            None,
            Some(&t.command),
        )
        .is_err()
        {
            errored += 1;
            continue;
        }

        let distilled = String::from_utf8_lossy(&out);
        let (r, o) = (t.raw.len() as u64, out.len() as u64);
        n += 1;
        raw_total += r;
        out_total += o;
        let (rt, ot) = (counter.count(&t.raw), counter.count(&distilled));
        raw_tokens += rt;
        out_tokens += ot;
        seen_raw.account(t, &t.raw, &mut rep_raw);
        seen_out.account(t, &distilled, &mut rep_out);
        match o.cmp(&r) {
            std::cmp::Ordering::Less => shrank += 1,
            std::cmp::Ordering::Equal => unchanged += 1,
            // Named, not counted. "2 calls grew" is a number nobody can act on,
            // and the published claim it contradicts stood for months (#398).
            std::cmp::Ordering::Greater => {
                grew += 1;
                if grew_detail.len() < 10 {
                    grew_detail.push(format!("  +{} bytes  {:.90}", o - r, t.command));
                }
            }
        }
        let e = per_cmd.entry(base_command(&t.command)).or_default();
        e.0 += 1;
        e.1 += r;
        e.2 += o;
        e.3 += rt;
        e.4 += ot;
        let f = per_form.entry(command_form(&t.command)).or_default();
        f.0 += 1;
        f.1 += r;
        f.2 += o;

        // #450. Attribute the repetition in this payload to the gate that will
        // decide its fate, before the ledger runs, and in the same order the
        // ledger will see the payloads.
        {
            let lines = gap_seen.account(t, &distilled);
            let repeated: u64 = lines.iter().filter(|(s, _)| *s).map(|(_, l)| l).sum();
            // #708's ceiling, accumulated here rather than in a second pass so it
            // uses `GapSeen`'s keys and `trace_class`'s definition, the same two
            // the columns beside it use. Measuring the ceiling with a separate
            // classifier is how 4.5% and 6.3% ended up describing populations of
            // 1,056 and 672 traces and being divided anyway.
            {
                let accounted: u64 = lines.iter().map(|(_, l)| l).sum();
                let c = per_class.entry(trace_class(&t.command)).or_default();
                c.4 += repeated;
                c.5 += accounted;
            }
            if veng::pipeline::format::sniff(&distilled).is_some() {
                gap_structured += repeated;
                n_structured += 1;
            } else if distilled.len() < veng::guard::limits::MIN_LEDGER_INPUT {
                gap_under_floor += repeated;
                n_under_floor += 1;
            } else {
                gap_processed += repeated;
                // Why each run of already-seen lines could not fold, using the
                // same two bounds `substitute` applies. A run that clears both
                // is one the ledger should have taken, so whatever is left in
                // that bucket is the gain gate or a failed archive.
                let mut i = 0;
                while i < lines.len() {
                    if !lines[i].0 {
                        i += 1;
                        continue;
                    }
                    let mut bytes = 0u64;
                    while i < lines.len() && lines[i].0 {
                        bytes += lines[i].1;
                        i += 1;
                    }
                    run_sizes.push(bytes);
                    // The shipped rule, mirrored: a run folds only when it saves
                    // `MIN_LEDGER_RUN_GAIN` after paying for its marker. The
                    // marker length is not reachable from a test binary, so the
                    // session form's 65 bytes is written here and pinned by
                    // `session_marker_len_matches_the_ledger` below.
                    let bar = SESSION_MARKER_LEN + veng::guard::limits::MIN_LEDGER_RUN_GAIN as u64;
                    if bytes < bar {
                        m1_under_bar += bytes;
                        m1_under_bar_runs += 1;
                    } else {
                        m1_eligible += bytes;
                        m1_eligible_runs += 1;
                    }
                }
            }
        }

        // Same three gates the hook applies: structured payloads are never
        // projected, the scope is the session the trace really belongs to, and
        // the ledger is told which command produced the payload.
        //
        // #760. That last one was missing, and it made this harness blind to
        // every rule that reads the command. The `from` clause never rendered
        // (#622), the line budget never fired (#735, #742, #750, #751), and the
        // re-run wording never appeared (#755), so a release could change all of
        // them and the artifact came back byte-identical, which it did between
        // 0.7.8 and this commit. The corpus carries the command and the routing
        // half above already uses it; only the ledger was left guessing.
        let after_ledger = ledger_store
            .as_ref()
            .filter(|_| veng::pipeline::format::sniff(&distilled).is_none())
            .and_then(|s| {
                let ledger = veng::ledger::Ledger::new(s, &t.session).from(&t.command);
                let ledger = if project_scope {
                    ledger.with_project(&t.project)
                } else {
                    ledger
                };
                ledger.project(&distilled)
            });
        let l = match after_ledger {
            Some(view) => {
                ledger_calls += 1;
                // No marker tally here any more, and the deletion is the point.
                //
                // Counting by string cannot be made correct from this side. A
                // substring scan reads a surviving line that quotes the wording;
                // filtering on the `[OMNI: ` prefix reads a surviving line that
                // starts with it, and this repository writes exactly those lines
                // into its changelog and its manual, so the corpus carries them.
                // #557 is the same lesson one guard earlier: identity has to come
                // from the side that produced the string, never from parsing it
                // back.
                //
                // The ledger does not report which marker shape it rendered yet.
                // That is #766, and the split comes back when it lands. What is
                // reported below is `ledger_calls`, which is the count of replies
                // the ledger folded, taken from `Option::Some` rather than from
                // text, and which content cannot spoof.
                view.len() as u64
            }
            None => o,
        };
        ledger_total += l;

        if headroom.is_some() {
            // The same bytes the ledger saw, behind the same gate. A structured
            // payload is never handed to either engine, so its bytes are counted
            // verbatim rather than offered for dedup: passing it would credit
            // headroom for bytes our own gate refuses to touch, which would
            // flatter it on a rule it never agreed to.
            //
            // Ordering is the loop's, which is the recorded order, and
            // prefix-monotonicity depends on that as much for their design as
            // for ours.
            if veng::pipeline::format::sniff(&distilled).is_none() {
                hr_blocks
                    .entry(t.session.clone())
                    .or_default()
                    .push(distilled.to_string());
            } else {
                hr_structured += o;
            }
        }

        if let Some(rtk) = &rtk {
            let rtk_text = rtk_out(rtk, &t.command, &t.raw);
            rtk_total += rtk_text.len() as u64;
            if rtk_filter(&t.command).is_some() {
                rtk_claimed += 1;
                // How often the ratio is bought by dropping the tail, in their
                // own words. Marker strings read from their `core/tee.rs` and
                // the cap sites in `cmds/`.
                if rtk_text.contains(" more")
                    || rtk_text.contains("truncated")
                    || rtk_text.contains("see remaining")
                {
                    rtk_marked += 1;
                }
            }
            // Their filters, our ledger, on its own store so the two ledger arms
            // cannot see each other's history.
            let after = rtk_ledger_store
                .as_ref()
                .filter(|_| veng::pipeline::format::sniff(&rtk_text).is_none())
                .and_then(|s| {
                    veng::ledger::Ledger::new(s, &t.session)
                        .from(&t.command)
                        .with_project(&t.project)
                        .project(&rtk_text)
                });
            rtk_ledger_total += after.map_or(rtk_text.len() as u64, |v| v.len() as u64);
        }

        if let Some(bin) = &caveman {
            let cm_text = caveman_out(bin, &caveman_home, &t.raw);
            cm_total += cm_text.len() as u64;
            // No filter name to key on, so this counts an actual reduction, the
            // stricter of the two counts already in this table. Same rule as the
            // lean-ctx arm, and not comparable to rtk's mapped-filter count.
            if cm_text.len() < t.raw.len() {
                cm_claimed += 1;
            }
            let after = cm_ledger_store
                .as_ref()
                .filter(|_| veng::pipeline::format::sniff(&cm_text).is_none())
                .and_then(|s| {
                    veng::ledger::Ledger::new(s, &t.session)
                        .from(&t.command)
                        .with_project(&t.project)
                        .project(&cm_text)
                });
            cm_ledger_total += after.map_or(cm_text.len() as u64, |v| v.len() as u64);
        }

        if let Some(bin) = &lean_ctx {
            let got = lean_ctx_bytes(bin, &t.command, &t.raw);
            lc_total += got as u64;
            // "Claimed" here means it actually shortened the payload, since
            // lean-ctx reports no filter name to key on the way rtk does. That
            // makes this count stricter than rtk's, which counts a mapped filter
            // whether or not it saved a byte, and the two are not comparable.
            if got < t.raw.len() {
                lc_claimed += 1;
            }
        }

        let c = per_class.entry(trace_class(&t.command)).or_default();
        c.0 += 1;
        c.1 += r;
        c.2 += o;
        c.3 += l;
    }

    assert!(
        n > 0,
        "every replayed trace errored ({errored} of {})",
        rows.len()
    );
    let saved = |before: u64, after: u64| {
        100.0 * (before - after.min(before)) as f64 / before.max(1) as f64
    };
    let net = saved(raw_total, out_total);
    let pct = |part: u64| 100.0 * part as f64 / n as f64;

    println!("\n=== #184 net-savings replay (current pipeline) ===");
    println!("population:        {population}");
    // Name the source that was actually read. Printing the DB path while
    // replaying a frozen corpus is a provenance line that lies, which is the
    // exact defect class #704 exists to remove.
    let source = corpus.as_deref().unwrap_or(db.as_str());
    println!("corpus:            {n} traces from {source} ({errored} errored, excluded)");
    println!("terminal rows:     {excluded} excluded from {total_traces}");
    // The corpus is pruned to `TRACE_RETENTION_DAYS`, so an all-time figure
    // stops being reproducible the week after it is published. Print the window
    // the number actually covers next to the number.
    if let Ok((from, to)) = conn.query_row(
        "SELECT datetime(MIN(ts),'unixepoch'), datetime(MAX(ts),'unixepoch') FROM execution_traces",
        [],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    ) {
        println!("covers:            {from} to {to} UTC");
    }
    if since > 0 {
        println!(
            "window:            traces at or after {}",
            std::env::var("VENG_BENCH_SINCE").unwrap_or_default()
        );
    }
    println!("bytes:             {raw_total} -> {out_total}");
    println!("NET SAVINGS:       {net:.1}%");
    println!(
        "saved nothing:     {:.1}% ({} passthrough + {} grew)",
        pct(unchanged + grew),
        unchanged,
        grew
    );
    println!("actually shrank:   {:.1}% ({shrank})", pct(shrank));
    println!("ADDED BYTES:       {grew} calls");
    for line in &grew_detail {
        println!("{line}");
    }

    // #392. Tokens, not bytes divided by anything. The ranking below is by
    // tokens for the same reason: a byte sink and a token sink are not
    // necessarily the same command, and until this was measured nobody here
    // could say whether they were.
    if counter.on() {
        println!(
            "\n--- tokens (cl100k_base, a proxy for a vocabulary Anthropic does not publish) ---"
        );
        println!("tokens:            {raw_tokens} -> {out_tokens}");
        println!("NET SAVINGS:       {:.1}%", saved(raw_tokens, out_tokens));
        println!(
            "bytes per token:   {:.3} raw, {:.3} distilled",
            raw_total as f64 / raw_tokens.max(1) as f64,
            out_total as f64 / out_tokens.max(1) as f64
        );
        println!(
            "                   (util::token_estimate ships 3.6 for Mixed; this is the check on it)"
        );
    } else {
        println!("\n--- tokens: skipped (VENG_BENCH_NO_TOKENS) ---");
    }

    // #392. The ledger's input. Filtering and repetition are orthogonal: the
    // post-filter figure is what is still recoverable by reference after every
    // distiller has had its turn, and it is the honest ceiling for P1.
    println!("\n--- repetition (lines >= {MIN_REPEAT_LINE} chars, exact match) ---");
    for (label, rep) in [("raw input", &rep_raw), ("post-filter", &rep_out)] {
        println!(
            "{label:<18} {:>12} bytes accounted, {:.1}% repeated ({:.1}% same session, {:.1}% earlier session, same project)",
            rep.accounted,
            rep.pct(rep.same_session + rep.same_project),
            rep.pct(rep.same_session),
            rep.pct(rep.same_project),
        );
    }

    // #394's gate: file-read class above 15%, aggregate above 20%, and no trace
    // worse than the filters left it. The last column is the whole argument:
    // filtering and the ledger are orthogonal, so the interesting number is what
    // the second one adds on top of the first.
    println!("\n--- by command class, filters then ledger (the P1 gate) ---");
    println!(
        "{:<16} {:>7} {:>12} {:>10} {:>10} {:>10} {:>9}",
        "class", "calls", "input", "filters", "+ ledger", "available", "captured"
    );
    // `available` is the share of accounted bytes whose line had already been
    // delivered in that scope, and `captured` is what the ledger took of it.
    // `captured` is the figure that does not move with the workload, which is the
    // whole of #708: on this corpus file reads read 4.5% saved against 89.6% in a
    // week of large repeated reads, while the ratio against the ceiling holds.
    let ratio = |part: u64, whole: u64| 100.0 * part as f64 / whole.max(1) as f64;
    let mut classes: Vec<_> = per_class.into_iter().collect();
    classes.sort_by_key(|(_, v)| std::cmp::Reverse(v.1));
    let (mut avail_total, mut acct_total) = (0u64, 0u64);
    for (class, (calls, r, f, l, avail, acct)) in classes {
        avail_total += avail;
        acct_total += acct;
        // The ledger only ever substitutes lines it has already delivered, so it
        // cannot remove more bytes than were repeated. Claiming more means the
        // two are measured over different populations, which is the defect that
        // produced a 71% estimate where the answer was 25%. Asserted rather than
        // printed: a capture rate over 100% is not a surprising number, it is a
        // broken denominator, and nothing else in this file would catch it.
        assert!(
            f.saturating_sub(l) <= avail,
            "{class}: ledger claimed {} bytes but only {avail} were repeated",
            f.saturating_sub(l)
        );
        // The ledger runs after the filters, so its claim is `f - l`, and the
        // denominator is the class's own repeated bytes rather than its input:
        // what was actually foldable, not what happened to arrive.
        let captured = ratio(f.saturating_sub(l), avail);
        println!(
            "{class:<16} {calls:>7} {r:>12} {:>9.1}% {:>9.1}% {:>9.1}% {:>8.1}%",
            saved(r, f),
            saved(r, l),
            ratio(avail, acct),
            captured
        );
    }
    println!(
        "aggregate        {n:>7} {raw_total:>12} {:>9.1}% {:>9.1}% {:>9.1}% {:>8.1}%  ({ledger_calls} calls projected)",
        saved(raw_total, out_total),
        saved(raw_total, ledger_total),
        ratio(avail_total, acct_total),
        ratio(out_total.saturating_sub(ledger_total), avail_total)
    );
    println!(
        "ledger arm:      project_scope={project_scope} floor_mult={} bytes={ledger_total} folds: {ledger_calls} replies (marker shapes pending #766)",
        veng::guard::limits::PROJECT_FLOOR_MULT
    );

    // #450. Every repeated byte the ledger was handed, attributed to the gate
    // that decided it. `claimed` is what the arm above actually removed, so
    // `M1` is the remainder inside payloads the ledger did process: runs too
    // short to fold, runs the gain gate rejected, and the markers' own bytes.
    let claimed = out_total.saturating_sub(ledger_total);
    let gap_total = gap_structured + gap_under_floor + gap_processed;
    let m1 = gap_processed.saturating_sub(claimed);
    let of_raw = |b: u64| 100.0 * b as f64 / raw_total.max(1) as f64;
    println!("\n--- #450: where the repetition goes (ledger's own line keys) ---");
    println!(
        "repeated bytes handed to the ledger: {gap_total} ({:.1}% of raw)",
        of_raw(gap_total)
    );
    println!(
        "  claimed by the ledger:             {claimed} ({:.1}%)",
        of_raw(claimed)
    );
    println!(
        "  M1 processed but unclaimed:        {m1} ({:.1}%)",
        of_raw(m1)
    );
    println!(
        "  M2 payload under MIN_LEDGER_INPUT: {gap_under_floor} ({:.1}%) over {n_under_floor} traces",
        of_raw(gap_under_floor)
    );
    println!(
        "  M3 structured, gate declined:      {gap_structured} ({:.1}%) over {n_structured} traces",
        of_raw(gap_structured)
    );
    println!("M1 split, against the gain bar the ledger applies:");
    println!(
        "  under the bar, cannot pay:         {m1_under_bar} ({:.1}%) over {m1_under_bar_runs} runs",
        of_raw(m1_under_bar)
    );
    println!(
        "  over the bar, folded or not:       {m1_eligible} ({:.1}%) over {m1_eligible_runs} runs",
        of_raw(m1_eligible)
    );
    println!("run sizes: {} runs, median {} bytes", run_sizes.len(), {
        let mut v = run_sizes.clone();
        v.sort_unstable();
        v.get(v.len() / 2).copied().unwrap_or(0)
    });
    if let Ok(path) = std::env::var("VENG_BENCH_RUNS_OUT") {
        let dump: String = run_sizes.iter().map(|b| format!("{b}\n")).collect();
        let _ = std::fs::write(path, dump);
    }
    println!("what a smaller marker would be worth, no line or byte floor at all:");
    for marker in [87u64, 60, 40, 28, 20] {
        let saved: u64 = run_sizes
            .iter()
            .filter(|b| **b > marker)
            .map(|b| b - marker)
            .sum();
        let folds = run_sizes.iter().filter(|b| **b > marker).count();
        println!(
            "  marker {marker:>3} bytes: {saved:>8} ({:.1}% of raw) over {folds} folds",
            of_raw(saved)
        );
    }

    // P4. One corpus, one command that reproduces it, and every competitor handed
    // the command that produced each payload, which their own hooks have to infer.
    // The comparison is tilted in their favour rather than ours, deliberately.
    //
    // Each arm is off unless its binary is named, so CI never needs a competitor
    // installed. A missing arm prints nothing rather than a zero, because a zero
    // in this table reads as a measurement.
    if rtk.is_some() || lean_ctx.is_some() || headroom.is_some() || caveman.is_some() {
        println!("\n--- head to head, same corpus (each tool given its command) ---");
        println!(
            "{:<22} {:>12} -> {:>12}  {:>7}",
            "veng, filters only",
            raw_total,
            out_total,
            format!("{:.1}%", saved(raw_total, out_total))
        );
        println!(
            "{:<22} {:>12} -> {:>12}  {:>7}",
            "veng, with ledger",
            raw_total,
            ledger_total,
            format!("{:.1}%", saved(raw_total, ledger_total))
        );
        if rtk.is_some() {
            println!(
                "{:<22} {:>12} -> {:>12}  {:>7}   ({rtk_claimed} of {n} claimed by a filter)",
                "rtk pipe",
                raw_total,
                rtk_total,
                format!("{:.1}%", saved(raw_total, rtk_total))
            );
            println!(
                "{:<22} {:>12} -> {:>12}  {:>7}",
                "rtk pipe + our ledger",
                raw_total,
                rtk_ledger_total,
                format!("{:.1}%", saved(raw_total, rtk_ledger_total))
            );
            println!("rtk marked a cut in {rtk_marked} of the {rtk_claimed} it claimed");
        }
        if caveman.is_some() {
            println!(
                "{:<22} {:>12} -> {:>12}  {:>7}   ({cm_claimed} of {n} shortened, no command hint)",
                "caveman compress",
                raw_total,
                cm_total,
                format!("{:.1}%", saved(raw_total, cm_total))
            );
            println!(
                "{:<22} {:>12} -> {:>12}  {:>7}",
                "caveman + our ledger",
                raw_total,
                cm_ledger_total,
                format!("{:.1}%", saved(raw_total, cm_ledger_total))
            );
            // A caveman that cannot find its Go binaries returns every payload
            // unchanged, which is indistinguishable from a competitor that simply
            // did not win. Say so rather than publishing the zero.
            if cm_claimed == 0 {
                println!(
                    "caveman shortened nothing: check CAVEMAN_HOME resolves, this is not a result"
                );
            }
        }
        if let Some(module) = &headroom {
            match headroom_total(module, &hr_blocks) {
                Some(deduped) => {
                    let total = deduped + hr_structured;
                    println!(
                        "{:<22} {:>12} -> {:>12}  {:>7}   (their dedup, our filters)",
                        "headroom dedup",
                        raw_total,
                        total,
                        format!("{:.1}%", saved(raw_total, total))
                    );
                    // The row that decides anything. Both arms are the same
                    // filters and the same blocks; only the dedup engine differs,
                    // so a gap here is the engine and nothing else.
                    println!(
                        "our ledger {:.1}% against their dedup {:.1}% on identical input",
                        saved(raw_total, ledger_total),
                        saved(raw_total, total)
                    );
                }
                // Naming the arm and getting no number is how this arm spent
                // its whole life absent while the run still read as a result
                // (#711). A named arm that cannot answer fails the bench.
                None => panic!("headroom arm was named as {module} and produced no number"),
            }
        }

        if lean_ctx.is_some() {
            println!(
                "{:<22} {:>12} -> {:>12}  {:>7}   ({lc_claimed} of {n} shortened)",
                "lean-ctx compress",
                raw_total,
                lc_total,
                format!("{:.1}%", saved(raw_total, lc_total))
            );
            // No `lean-ctx + our ledger` row on purpose: its preview reports byte
            // counts and never hands back the compressed text, so that row could
            // only be estimated. See `lean_ctx_bytes`.
            println!("lean-ctx reports bytes rather than emitting them, so it has no ledger row");
        }
    }

    // #395's gate. Printed next to the classes rather than in place of them,
    // because the question P2 asks is whether the shape a command was typed in
    // still costs anything once the pipeline has had its turn.
    println!("\n--- by shell shape (the P2 gate) ---");
    println!(
        "{:<18} {:>7} {:>14} {:>8}",
        "form", "calls", "input", "saved"
    );
    let mut forms: Vec<_> = per_form.into_iter().collect();
    forms.sort_by_key(|(_, v)| std::cmp::Reverse(v.1));
    for (form, (calls, r, o)) in forms {
        println!("{form:<18} {calls:>7} {r:>14} {:>7.1}%", saved(r, o));
    }

    println!("\ntop commands by input bytes:");
    let mut cmds: Vec<_> = per_cmd.into_iter().filter(|(k, _)| !k.is_empty()).collect();
    cmds.sort_by_key(|(_, v)| std::cmp::Reverse(v.1));
    let by_bytes: Vec<String> = cmds.iter().take(15).map(|(k, _)| k.clone()).collect();
    println!(
        "{:<12} {:>7} {:>14} {:>14} {:>8} {:>12} {:>8}",
        "command", "calls", "input", "output", "saved", "in tokens", "saved"
    );
    for (cmd, (calls, r, o, rt, ot)) in cmds.iter().take(15) {
        println!(
            "{cmd:<12} {calls:>7} {r:>14} {o:>14} {:>7.1}% {rt:>12} {:>7.1}%",
            saved(*r, *o),
            saved(*rt, *ot)
        );
    }

    if counter.on() {
        cmds.sort_by_key(|(_, v)| std::cmp::Reverse(v.3));
        let by_tokens: Vec<String> = cmds.iter().take(15).map(|(k, _)| k.clone()).collect();
        println!(
            "\nbyte-sink and token-sink rankings {}",
            if by_bytes == by_tokens {
                "agree".to_string()
            } else {
                format!("DISAGREE:\n  by bytes:  {by_bytes:?}\n  by tokens: {by_tokens:?}")
            }
        );
    }
    println!("\nreplayed in {:.1}s\n", started.elapsed().as_secs_f64());

    // The one invariant that must hold: VENG never adds bytes across the corpus.
    assert!(
        out_total <= raw_total,
        "VENG added bytes across the corpus: {raw_total} -> {out_total}"
    );
    // And the ledger only ever takes bytes off what the filters left. A
    // projection that grew the payload would mean a marker cost more than the
    // run it replaced, which the run bounds exist to prevent.
    assert!(
        ledger_total <= out_total,
        "the ledger added bytes: {out_total} -> {ledger_total}"
    );
    // Repetition is a share of the bytes it was measured over, so a figure above
    // 100% means the accounting double-counted a line and every number printed
    // above it is suspect.
    for (label, rep) in [("raw", &rep_raw), ("post-filter", &rep_out)] {
        assert!(
            rep.same_session + rep.same_project <= rep.accounted,
            "{label} repetition exceeds the bytes it was measured over"
        );
    }
}

/// The repetition accounting is the number P1 is judged on, so it is tested on
/// input whose answer can be counted by hand rather than only on the corpus.
#[test]
fn resolves_the_headroom_module_from_the_installed_command() {
    // #711: the arm was pointed at `headroom`, which is a console script and not
    // the module, so it resolved nothing and never ran.
    assert_eq!(headroom_module("veng-no-such-command-711"), None);

    // A machine with no headroom is the CI machine, and there the line above is
    // the whole test. Where it is installed, the command has to lead to the file.
    if let Some(path) = headroom_module("headroom") {
        assert!(path.ends_with("cross_turn_dedup.py"), "resolved to {path}");
        assert!(std::path::Path::new(&path).is_file(), "not a file: {path}");
    }
}

#[test]
fn counts_a_repeated_line_once_per_scope() {
    let mut seen = Seen::default();
    let mut rep = Repetition::default();
    let line = "a line well over the twelve character floor";

    let in_session = |s: &str| Trace {
        command: "cat a.rs".into(),
        raw: String::new(),
        session: s.into(),
        project: "p".into(),
    };

    seen.account(&in_session("s1"), line, &mut rep);
    assert_eq!(rep.same_session, 0, "a first sighting is not a repeat");
    assert_eq!(rep.accounted, line.len() as u64);

    seen.account(&in_session("s1"), line, &mut rep);
    assert_eq!(rep.same_session, line.len() as u64);
    assert_eq!(rep.same_project, 0);

    // New session, same project: this is the share only a project ledger reaches.
    seen.account(&in_session("s2"), line, &mut rep);
    assert_eq!(rep.same_session, line.len() as u64, "still one session hit");
    assert_eq!(rep.same_project, line.len() as u64);
}

/// Short lines are punctuation, not evidence. Counting them would put `}` and
/// `---` at the top of every repetition figure the ledger is judged by.
#[test]
fn ignores_lines_under_the_floor() {
    let mut seen = Seen::default();
    let mut rep = Repetition::default();
    let t = Trace {
        command: "cat a.rs".into(),
        raw: String::new(),
        session: "s1".into(),
        project: "p".into(),
    };

    seen.account(&t, "}\n---\n}\n---\n", &mut rep);

    assert_eq!(rep.accounted, 0);
    assert_eq!(rep.same_session, 0);
}

/// The ledger's own marker, measured rather than assumed, so
/// `SESSION_MARKER_LEN` cannot drift away from the string it stands for.
///
/// **The fixture folds a run, not a whole payload, and that is load-bearing.**
/// Since #519 a fold that covers the entire output renders a different, longer
/// marker, because `N lines already shown` reads as a claim about part of the
/// output and leaves a fully elided reply indistinguishable from a command that
/// printed nothing. `SESSION_MARKER_LEN` stands for the run marker, which is what
/// the attribution above is counting, so the payload carries one line this
/// session has not seen. An earlier fixture repeated the whole text and started
/// measuring the wrong string the moment the second one existed.
#[test]
fn session_marker_len_matches_the_ledger() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = veng::store::sqlite::Store::open_path(&dir.path().join("l.db")).expect("store");
    let text: String = (0..40)
        .map(|i| format!("2026-08-11T00:00:00Z  handler finished request {i} in 12ms\n"))
        .collect();
    let ledger = veng::ledger::Ledger::new(&store, "s1");
    ledger.project(&text);
    let view = ledger
        .project(&format!(
            "{text}a line this session has never been shown before\n"
        ))
        .expect("the repeated head folds");

    let marker = view
        .lines()
        .find(|l| l.starts_with("[OMNI:"))
        .expect("the fold emits a marker");
    // 40 lines, so the count is two digits, which is what the constant assumes.
    assert_eq!(marker.len() as u64, SESSION_MARKER_LEN);
}
