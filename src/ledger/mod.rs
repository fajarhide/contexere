//! The session ledger: what this agent has already been shown.
//!
//! Every filter in `src/distillers/` answers the same question, one command at a
//! time: given this output, what can be dropped. The ledger answers a different
//! one: given everything shown in this session, what is this output repeating.
//! The two are **orthogonal**, and the corpus says so. Replayed over 7,019
//! model-facing traces on 2026-08-10, 25.3% of raw bytes were lines already seen
//! and **22.9% still were after every distiller had run**. Filtering removes 5.2%
//! of bytes and barely dents repetition.
//!
//! It also reaches the class nothing else can. File reads are the largest class
//! in the corpus at 1.53 MB, and the filters save **0.0%** of it, because you
//! cannot strip lines from a file the agent asked to see without guessing which
//! parts it meant. The ledger takes 24.8% of the same class without guessing
//! anything: those lines were already delivered once.
//!
//! ## What it does
//!
//! A **run** of consecutive lines that were all emitted earlier in this session
//! becomes one marker naming the count and an `omni_retrieve` handle. Everything
//! else passes through byte for byte.
//!
//! ## The rules it inherits
//!
//! - **Append-only.** It only ever reduces the output of the command in flight,
//!   and never rewrites anything already delivered. That is what keeps the
//!   upstream prompt cache intact: a cache works on a prefix, and shortening the
//!   suffix costs nothing. Retroactive compaction is the move that would destroy
//!   it, and nothing here reaches backwards.
//! - **Deterministic.** The same ledger state must render byte-identical output.
//!   The handle is the content address and carries no timestamp. An earlier
//!   `{ts_ns}_{hash}` handle shipped and made 4 of 73 repeated inputs emit
//!   different bytes; it was fixed with no test behind it, so
//!   `renders_identical_bytes_for_identical_state` is that test.
//! - **Nothing is lost.** A run is archived before its marker is written, and a
//!   failed archive means the run stays verbatim. A handle that does not resolve
//!   is the one defect this whole mechanism cannot have (#388).
//! - **Unknown means untouched.** Structured payloads never reach here; the
//!   caller gates on `pipeline::format::sniff` exactly as collapse does.

use std::collections::{HashMap, HashSet};

use crate::guard::limits::{
    MIN_LEDGER_INPUT, MIN_LEDGER_RUN_GAIN, MIN_WHOLE_OUTPUT_FOLD, PROJECT_FLOOR_MULT,
};
use crate::pipeline::format::without_ansi_prefix;
use crate::pipeline::{producer, registry};
use crate::store::sqlite::Store;

/// How much of a source name a marker will carry.
///
/// The fold gate weighs the rendered marker, so every byte here is a byte a run
/// must beat before folding is worth it (#450). Forty is enough for a file name
/// and the tail of its directory, which is what a reader needs to tell "this came
/// from the other file" from "this came from the one I asked for" (#622).
const MAX_SOURCE_IN_MARKER: usize = 40;

/// One line of a source name, safe to interpolate into a marker.
///
/// A marker is a single line, and the count of markers standing above the first
/// surviving line is computed from that (#573). A source is a raw command, and
/// **46.9% of recorded commands carry a newline** (4,961 of 10,578 traces:
/// heredocs, chained builds, pasted scripts), so interpolating one unfiltered
/// would let the command split the marker into lines of its own choosing and
/// corrupt both the framing and the accounting.
///
/// First line only, control characters dropped, runs of whitespace collapsed,
/// then cut at a char boundary. Truncation alone is not enough: it preserves
/// every newline before the cut.
fn source_label(source: &str) -> String {
    let first = source.lines().next().unwrap_or("");
    let mut out = String::with_capacity(first.len());
    let mut in_space = false;
    for c in first.chars() {
        if c.is_control() || c.is_whitespace() {
            if !out.is_empty() && !in_space {
                out.push(' ');
                in_space = true;
            }
        } else {
            out.push(c);
            in_space = false;
        }
    }
    crate::util::text::safe_slice(out.trim_end(), MAX_SOURCE_IN_MARKER).to_string()
}

/// Which history a run was found in, because the two cannot make the same claim.
///
/// This distinction is the whole of the project scope. An earlier draft cancelled
/// it on the grounds that a handle for another session's content is a lie, which
/// was right about the wording and wrong about the remedy: the fix is to stop
/// saying "already shown", not to stop remembering.
///
/// - `Session` means the agent is still holding those bytes, so the handle is
///   free unless it chooses to re-read them.
/// - `Project` means the bytes went to a different session of this project and
///   this agent has **never seen them**. The marker says so, and the trade stops
///   being free: it is a handle plus a possible retrieval against the block
///   itself, which is the same bet `store_rewind` already makes everywhere else.
///   That is why it carries its own, much higher floor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Origin {
    Session,
    Project,
}

impl Origin {
    /// What a replaced run is worth saying about itself.
    ///
    /// Every word here is paid for on every fold, so the wording is as short as
    /// it can be while still saying which history the lines came from and how to
    /// get them back. The session form was 87 bytes and is 65; trimming it moved
    /// the aggregate by 0.3 points on its own, because a shorter marker makes
    /// smaller runs worth folding (#450).
    fn marker(
        self,
        lines: usize,
        handle: &str,
        source: &SourceOfSighting,
    ) -> (String, &'static str) {
        // Only ever present when the source differs from the command being
        // answered (#622). `Seen` decides that; this only renders it, and keeps
        // it short because the fold gate weighs this string.
        let from = match source {
            SourceOfSighting::Another(s) => format!(" from {}", source_label(s)),
            _ => String::new(),
        };
        match self {
            // #755. A poll re-run verbatim is asking whether the value moved,
            // and `N lines already shown` answers a different question: it says
            // the bytes are behind you without saying they are the same bytes
            // this command printed last time. The reported reply kept its first
            // half, which read as a complete result, and put the seven metric
            // lines the caller was watching behind a handle. The identity is
            // what the run was asking for, so it is what the marker states, at
            // 15 bytes more than the wording it replaces on this arm only.
            Self::Session if matches!(source, SourceOfSighting::ThisCommandAgain) => (
                format!(
                    "[OMNI: {lines} lines identical to an earlier run, omni retrieve {handle}]"
                ),
                "session_rerun",
            ),
            Self::Session => (
                format!("[OMNI: {lines} lines already shown{from}, omni retrieve {handle}]"),
                "session_run",
            ),
            // #567. `from an earlier session` states provenance and reads as
            // "you have seen this", which is the opposite of what it means: the
            // project scope only answers for lines the session scope did not, so
            // these lines were never delivered here. A reader acted on that,
            // took a help page missing its `Commands:` block as complete, and
            // concluded the CLI had no uninstall.
            //
            // The actionable half goes first and the provenance is dropped, which
            // is also nine bytes shorter. Marker length gates folding, and the
            // session form's trim was worth 0.3 points on its own (#450), so the
            // honest wording is the cheaper one here rather than a trade.
            Self::Project => (
                format!("[OMNI: {lines} lines not shown here{from}, omni retrieve {handle}]"),
                "project_run",
            ),
        }
    }

    /// What to say when the fold covered the payload and nothing else came back.
    ///
    /// `N lines already shown` is a claim about a *run*, and it reads as one:
    /// some of this was shown before, the rest is new. When the run is the whole
    /// output there is no rest, and the same wording leaves a reader unable to
    /// tell a fully elided reply from a command that printed nothing. Re-running
    /// a command is how a failure gets verified and how a fix gets confirmed, so
    /// reading `no output` there is the expensive misreading (#519).
    ///
    /// It states the identity instead, which is the fact the re-run was asking
    /// for and is strictly more information than the run wording carried.
    fn whole_output_marker(
        self,
        lines: usize,
        handle: &str,
        source: &SourceOfSighting,
    ) -> (String, &'static str) {
        // No `ThisCommandAgain` arm: this wording has stated the identity since
        // #519, which is the fact a re-run is asking for.
        let from = match source {
            SourceOfSighting::Another(s) => format!(" from {}", source_label(s)),
            _ => String::new(),
        };
        match self {
            Self::Session => (
                format!(
                    "[OMNI: identical to the {lines} lines already shown{from}, omni retrieve {handle}]"
                ),
                "session_whole",
            ),
            // The whole payload, and none of it delivered here, so the agent
            // holds nothing at all. Worth the extra bytes to say both: this
            // marker is already gated at `MIN_WHOLE_OUTPUT_FOLD` (#567).
            Self::Project => (
                format!(
                    "[OMNI: identical to {lines} lines from an earlier session{from}, none shown here, omni retrieve {handle}]"
                ),
                "project_whole",
            ),
        }
    }

    /// The bytes a fold has to save, after paying for its own marker.
    ///
    /// Session scope pays a marker. Project scope pays a marker **and** the
    /// expected cost of a retrieval the agent has no choice about if it needs
    /// the content, because it has never seen those lines. Three times the
    /// session gain is where the replay put the knee: at that bar the project
    /// scope keeps the runs worth several hundred bytes each and drops the tail
    /// that was earning 179 bytes per interruption (#448).
    fn min_gain(self) -> usize {
        match self {
            Self::Session => MIN_LEDGER_RUN_GAIN,
            Self::Project => MIN_LEDGER_RUN_GAIN * PROJECT_FLOOR_MULT,
        }
    }
}

/// A handle is always this long, so a marker's length is known before the run is
/// archived and the profitability test can weigh the real thing.
///
/// `store_rewind` slices the hex digest to 16 characters. If that ever changes,
/// `renders_a_marker_the_gain_test_can_predict` fails rather than the ledger
/// silently folding runs that do not pay.
const HANDLE_LEN: usize = 16;

/// What a fold did to the numbers of the lines still under it.
///
/// Only a caller that controls where its host starts counting can act on this,
/// which today is the `Read` path: the host renders `file.content` with `cat -n`
/// numbering counted from `startLine`, so it numbers whatever it is handed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldShift {
    /// Nothing survives below the fold, so no number moves. A whole-output fold
    /// and any fold reaching the end of the payload are this.
    None,
    /// The survivors are one block with folded lines above them. `bump` is what
    /// the caller must add to its host's starting number so the first survivor
    /// lands on the line the file gives it, and with it every line below.
    ///
    /// Not "folded lines minus one": adjacent runs of different origin emit one
    /// marker each, so the number of lines standing above the survivors is the
    /// number of markers, which is what this counts. Review found the earlier
    /// arithmetic putting every survivor `k - 1` lines too high.
    Leading { bump: usize },
    /// Content above the fold and content below it. Nothing can correct that,
    /// because one starting number cannot describe two different offsets.
    Interior,
}

impl FoldShift {
    /// The question is not where the folds are, it is whether what survives them
    /// is one block. Contiguous survivors all sit at the same distance from
    /// where they started, so a single starting number puts every one of them
    /// back; split survivors sit at two different distances and no single number
    /// describes both.
    ///
    /// `markers_above` comes from `substitute`, which is the only place that
    /// knows: a run becomes one marker or stays verbatim. Three attempts to infer
    /// it from the output were wrong before it was simply reported (#573), and
    /// each is worth not repeating. `folded.len() - 1` assumes one marker per
    /// fold and adjacent runs of different origin emit one each. Searching the
    /// view for the surviving block matches inside a marker when the block's text
    /// is something a marker also says. Recognising markers by their prefix is
    /// defeated by a file whose own lines start with it.
    fn of(folded: &HashSet<usize>, total: usize, markers_above: usize) -> Self {
        let survivors: Vec<usize> = (0..total).filter(|i| !folded.contains(i)).collect();
        let (Some(&first), Some(&last)) = (survivors.first(), survivors.last()) else {
            // Nothing survived, so nothing can be misnumbered.
            return Self::None;
        };
        if last - first + 1 != survivors.len() {
            return Self::Interior;
        }
        if first == 0 {
            // The survivors still start where the payload does, so the numbers
            // the host will give them are already right.
            return Self::None;
        }
        // A fold below the survivors changes nothing about the survivors: they are
        // still `first` lines into the file and `markers_above` lines into the
        // view, so one number closes that gap whatever follows them.
        Self::Leading {
            bump: first.saturating_sub(markers_above),
        }
    }
}

/// The session scope for one reader, which is not the same as one session.
///
/// The scope answers "has this reader already been shown these bytes", and
/// `Origin::Session`'s marker asserts exactly that. A subagent runs in its own
/// context and carries **the parent's** `session_id`, so keying on the session
/// alone answered a subagent with the parent's history and told it 200 lines
/// were "already shown" when that context had received none of them (#581).
///
/// The main agent has no `agent_id`, so its scope is unchanged and no history is
/// orphaned by this. What a subagent loses is only the parent's lines; its own
/// repeats still fold under its own scope, and genuinely repeated project bytes
/// still fold through the project scope, whose marker says "not shown here" and
/// is honest for a reader that never held them (#567, #575).
///
/// This does not fix the other reader the premise fails for: after compaction
/// the session id is unchanged and the context is gone, and nothing in the hook
/// payload says so.
pub fn scope_for(session: &str, agent: Option<&str>) -> String {
    match agent {
        Some(a) if !a.is_empty() => format!("{session}/{a}"),
        _ => session.to_string(),
    }
}

/// The session half of a scope key, which is the inverse of `scope_for`.
///
/// Kept beside it so the two cannot drift: a fold row records the project in its
/// `scope` column by design, and `omni_explain_savings` still has to be able to
/// ask "what did this session fold" (#602). Agent ids carry no `/`, so the first
/// segment is the session.
pub fn session_of(scope: &str) -> &str {
    scope.split('/').next().unwrap_or(scope)
}

/// Addresses the ledger for one session, and optionally for its project.
///
/// Cross-session repetition measures 3.7% of post-filter bytes against 19.1%
/// within a session, so the project scope is worth about a fifth of the session
/// scope at best, and less than that after its higher floor. It is built anyway
/// because the plan asks for it and because the data is what settles whether the
/// trade is worth taking.
pub struct Ledger<'a> {
    store: &'a Store,
    scope: String,
    /// `None` disables project scope entirely, which is what every caller that
    /// cannot name a project passes.
    project: Option<String>,
    /// What command is being answered, so a fold can say when the bytes it is
    /// replacing came from a different one (#622). Empty means the caller could
    /// not name one, which suppresses the clause rather than guessing.
    source: String,
    /// Whether the caller's host numbers the lines it is handed, counting from a
    /// start line the caller controls. A host that does cannot take a view whose
    /// survivors sit in two blocks, so that view is never built and never booked
    /// (#657).
    /// The tool whose reply this is, as the hook normalised it.
    ///
    /// Two questions were passed in beside each other before, both derived
    /// from this one string: whether the host renumbers what it is handed
    /// (#657) and whether the caller's own pattern picked the lines (#814).
    /// A third would have followed. The string is also what a fold records,
    /// so a report can group by the surface it happened on (#822).
    tool: String,
    /// Whether the caller picked a window of its source, which a `Read` path
    /// cannot say the way `tail -5` does (#796). Same guard as `rations_its_output`.
    windowed: bool,
    /// Who is being shown these lines, recorded and read by nothing (#509).
    ///
    /// The project scope is keyed on the directory alone, so two agents in one
    /// repo already share a history and a fold cannot tell whose bytes it is
    /// replacing. Recording the agent is what makes that measurable; changing
    /// the key is the decision the measurement is for.
    agent: String,
}

/// Where a run was seen, and whose bytes it is replacing.
///
/// `source` is `Some` only when the ledger recorded one **and** it differs from
/// the command being answered. That is the case a reader cannot resolve from the
/// marker alone: reading file B and having a block elided because file A showed
/// it earlier, which is what #622 reported. Same-source folds are the common case
/// and carry no clause, which matters because marker length gates folding: the
/// gate weighs the rendered string, so every byte added here is a byte a run must
/// beat before it is worth replacing (#450).
///
/// It is part of the grouping key, so a stretch whose lines came from two
/// different commands splits into two runs rather than picking one of them to
/// name.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Seen {
    origin: Origin,
    source: SourceOfSighting,
}

/// Which command put these lines in front of the agent, relative to the one
/// being answered now.
///
/// Three cases and they say different things, which is why this is not an
/// `Option<String>`. Before #755 the middle and the last were one `None`, and
/// the last is the one a reader most needs named: a poll re-run verbatim whose
/// output has not changed is answered by the fact that it has not changed, and
/// `N lines already shown` does not say that.
#[derive(Clone, PartialEq, Eq, Debug)]
enum SourceOfSighting {
    /// Nothing recorded, so nothing claimed.
    Unrecorded,
    /// A different command (#622). The marker names it so the reader can tell
    /// whether the lines it is missing came from something relevant.
    Another(String),
    /// This same command, run again, printing these same lines (#755).
    ThisCommandAgain,
}

/// What one substitution produced.
///
/// A tuple until #766 added the fifth field, at which point clippy called it a
/// very complex type and was right: five positional fields is a shape nobody can
/// read at the call site.
struct Substitution {
    view: String,
    folded: HashSet<usize>,
    /// Markers standing above the first surviving line, which is what decides
    /// whether a starting line number still describes the view.
    markers_above: usize,
    padded: bool,
    /// One row per marker actually emitted, for the books (#766).
    emitted: Vec<crate::store::sqlite::MarkerRecord>,
}

/// One stretch of output, and where it was seen before if it was.
struct Run {
    start: usize,
    end: usize,
    seen: Option<Seen>,
}

/// One folded line, kept so the delivered line count matches the file's.
///
/// A host that numbers what it is handed cannot take a view with fewer lines than
/// the payload, and folding to a single marker is exactly that. Emitting the
/// marker plus one of these per remaining line keeps every survivor on its own
/// number without a `startLine` bump, which is what makes an interior fold legal
/// at all (#664).
///
/// `⋮` and not a blank or a tilde: editors already read it as elided content, so a
/// reader who skips the marker above does not take it for a line of the file. It
/// costs 4 bytes against the ~50 a real line of source runs to.
const PAD_LINE: &str = "⋮\n";

impl<'a> Ledger<'a> {
    pub fn new(store: &'a Store, scope: impl Into<String>) -> Self {
        Self {
            store,
            scope: scope.into(),
            project: None,
            source: String::new(),
            tool: String::new(),
            windowed: false,
            agent: "unknown".to_string(),
        }
    }

    /// Adds the project history to what this ledger can draw on.
    pub fn with_project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }

    /// Names the command being answered, which is what a fold compares against
    /// before claiming a different source in its marker.
    pub fn from(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }

    /// Names the tool whose reply this is, which is what the two questions
    /// below are asked of.
    ///
    /// `Read` is renumbered: its payload goes back as `file.content` and the
    /// host renders it with `cat -n` from `startLine`, so one starting number
    /// cannot describe survivors at two offsets and such a view is never built
    /// (#657). `Grep` filters: the caller's pattern already chose every line,
    /// and the command string cannot say so because it carries a pattern or a
    /// path (#814).
    pub fn tool(mut self, name: &str) -> Self {
        self.tool = name.to_string();
        self
    }

    /// Whether the host numbers the lines it is handed.
    fn renumbers(&self) -> bool {
        self.tool == "Read"
    }

    /// Whether the caller's own pattern picked every line in this reply.
    fn filters_its_reply(&self) -> bool {
        self.tool == "Grep"
    }

    /// Says the caller asked for a window of its source rather than all of it.
    pub fn windowed(mut self, yes: bool) -> Self {
        self.windowed = yes;
        self
    }

    /// Names the agent these lines are being delivered to.
    ///
    /// Resolved by the caller rather than from the environment here: a Codex
    /// payload arriving while `CLAUDECODE` is set answers `claude_code` to the
    /// naive check, and this column exists precisely to tell hosts apart.
    pub fn by(mut self, agent: impl Into<String>) -> Self {
        self.agent = agent.into();
        self
    }

    /// The view of `text` this session has earned, or `None` when there is
    /// nothing to gain.
    ///
    /// `None` means "leave the caller's bytes alone", which is the honest answer
    /// whenever the projection did not actually shorten anything. The lines are
    /// recorded either way: a block is worth remembering because it may be seen
    /// again, not because it compressed today. That is the widened gate the plan
    /// asks for, and it is why the recording is unconditional while the
    /// substitution is not.
    pub fn project(&self, text: &str) -> Option<String> {
        self.project_reporting_shift(text).map(|(view, _)| view)
    }

    /// The view, and what the fold did to the numbering of the lines under it.
    ///
    /// A caller whose host numbers the lines it is handed needs the second half:
    /// a marker with content under it moves every one of those numbers, and the
    /// payload says nothing about it (#557). The question is answered from the
    /// folded indices rather than by recognising markers in the output, because
    /// a file whose own lines begin with the marker prefix would defeat that.
    pub fn project_reporting_shift(&self, text: &str) -> Option<(String, FoldShift)> {
        let shift = std::cell::Cell::new(FoldShift::None);
        // The gain gate wraps the projection rather than being re-derived inside
        // it (spec 5.4). `MIN_LEDGER_INPUT` is this projection's own floor and is
        // higher than the gate's, so both apply and the stricter one decides.
        let view = crate::pipeline::gate::gain(text, |text| {
            self.project_inner(text).map(|(view, shifted)| {
                shift.set(shifted);
                view
            })
        })?;
        Some((view, shift.get()))
    }

    fn project_inner(&self, text: &str) -> Option<(String, FoldShift)> {
        if text.len() < MIN_LEDGER_INPUT {
            return None;
        }

        // `split_inclusive`, not `lines()`. `lines()` drops the terminator, so
        // rebuilding with `\n` would rewrite every CRLF payload on Windows into
        // LF, silently, on content the ledger did not replace. Keeping the
        // terminator on the slice makes the untouched runs byte-exact by
        // construction rather than by promise (`CONTRIBUTING.md` cross-platform rule 2).
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        let hashes: Vec<String> = lines.iter().map(|l| line_key(l)).collect();

        // Session first, and the project scope only answers for what the session
        // did not. A line in both belongs to the session: that is the claim that
        // is free, and taking the weaker one would cost the agent a retrieval for
        // content it is holding.
        // The session scope has no age bound: the reader is holding those bytes
        // however long ago they arrived, which is the whole claim.
        let in_session = self.store.ledger_seen(&self.scope, &hashes, 0);
        // The project scope does. Those lines went somewhere else, and a sighting
        // from yesterday is a bet on a context that has been gone for hours
        // (#795).
        let fresh_enough =
            chrono::Utc::now().timestamp() - crate::guard::limits::PROJECT_SIGHTING_MAX_AGE_SECS;
        let in_project = match &self.project {
            Some(p) => self.store.ledger_seen(p, &hashes, fresh_enough),
            None => HashMap::new(),
        };
        // A line that states a failure is never folded, however often it has been
        // shown (#458). The heuristic "you have seen this already" is sound for
        // informational lines and wrong for the error channel, where the
        // repetition *is* the signal: the same TypeError appearing on a re-run
        // means the bug is still there, and that is the one line worth the
        // tokens. Eliding it delivers source context and no statement of what
        // went wrong, which an agent reasonably reads as the failure being gone.
        //
        // Marking the line unseen rather than filtering it afterwards also
        // splits the run around it, so the frames either side still fold.
        // Identical hash means identical trimmed text, so the verdict is a
        // property of the hash and this set answers in O(1) rather than the
        // closure searching the payload per line. The hook has a 10 ms budget.
        //
        // A diff's changed lines are the same kind of line (#788). `+ foo` shown
        // by an earlier `git show` is a different fact from `+ foo` staged now, and
        // folding them left a reviewed diff holding only its context.
        // `git diff --color` paints the hunk header and the changed lines, and a
        // combined diff opens `@@@`, so both are read past the colour (PR #809
        // review).
        let is_diff = lines
            .iter()
            .any(|l| without_ansi_prefix(l).starts_with("@@"));
        let never_fold: HashSet<&String> = hashes
            .iter()
            .zip(lines.iter())
            .filter(|(_, line)| {
                crate::pipeline::semantic::carries_failure(line)
                    || (is_diff && without_ansi_prefix(line).starts_with(['+', '-', '@']))
            })
            .map(|(hash, _)| hash)
            .collect();

        // The source clause is decided here, once, so the grouping key and the
        // rendered marker cannot disagree: a run only claims a source when the
        // ledger recorded one and it is not the command being answered (#622).
        let differing = |seen: &crate::store::sqlite::SeenLine| {
            if seen.source.is_empty() {
                SourceOfSighting::Unrecorded
            } else if seen.source == self.source {
                SourceOfSighting::ThisCommandAgain
            } else {
                // A recorded source is the whole command string, and naming a
                // prefix of it names a different command. 28.7% of sourced lines
                // here were recorded against a multi-line command and 86.6% of
                // those begin with `cd <path>`, so a quarter of every `from`
                // clause was crediting the bytes to a chdir, which writes no
                // stdout at all (#779). `sole_output_command` is the same
                // predicate that decides which segment a distiller may claim, so
                // the marker and the routing agree by construction, and its
                // `None` is the honest answer for a chain with two producers:
                // stdout is one stream and nothing marks which of them wrote a
                // given line, so no clause is printed rather than picking one.
                match producer::sole_output_command(&seen.source) {
                    Some(cmd) => {
                        SourceOfSighting::Another(producer::without_clause_prefix(cmd).to_string())
                    }
                    None => SourceOfSighting::Unrecorded,
                }
            }
        };
        let origin_of = |h: &String| {
            if never_fold.contains(h) {
                return None;
            }
            if let Some(seen) = in_session.get(h) {
                Some(Seen {
                    origin: Origin::Session,
                    source: differing(seen),
                })
            } else {
                in_project.get(h).map(|seen| Seen {
                    origin: Origin::Project,
                    source: differing(seen),
                })
            }
        };

        // Record before projecting. The order matters only for a line that
        // repeats inside this very payload: recording first would let the second
        // copy see the first, which is true but not useful, since both copies are
        // in front of the agent already. Recording after keeps the projection a
        // function of what *earlier commands* showed, which is what makes it
        // deterministic for a caller replaying the same session.
        let projected = self
            .substitute(&lines, &hashes, &origin_of)
            .filter(|s| s.view.len() < text.len());

        // What the fold did to the numbering below it (#557), read from the folded
        // indices where the answer is known. Decided here rather than after the
        // recording below, because a host that renumbers will drop a split view and
        // the books must not carry a fold the agent never received (#657).
        // A padded view has the payload's own line count, so every survivor is
        // already where the file has it and no starting number moves (#664).
        let shifts = projected
            .as_ref()
            .map(|s| {
                let (folded, above, padded) = (&s.folded, s.markers_above, s.padded);
                if padded {
                    FoldShift::None
                } else {
                    FoldShift::of(folded, lines.len(), above)
                }
            })
            .unwrap_or(FoldShift::None);
        let projected = projected.filter(|_| !(self.renumbers() && shifts == FoldShift::Interior));

        // Record what the agent was handed, which is not always what it was
        // given (#465). A run replaced by a marker never reached the context, so
        // recording it here would let the *next* occurrence in this session read
        // as `Origin::Session` and say "already shown" about bytes that were
        // never shown. It also halves the bar: session origin charges
        // `MIN_LEDGER_RUN_GAIN` where project origin charges three times that,
        // so the false claim makes the ledger more willing to fold, not less.
        //
        // When the projection is discarded, by `substitute` finding nothing or by
        // the gain filter above, the caller emits `text` verbatim and every line
        // was delivered. That is the empty-set case and needs no special arm.
        let delivered: Vec<String> = match &projected {
            Some(Substitution { folded, .. }) => hashes
                .iter()
                .enumerate()
                .filter(|(i, _)| !folded.contains(i))
                .map(|(_, h)| h.clone())
                .collect(),
            None => hashes.clone(),
        };
        // What the folds drew on, before the write below makes every line look
        // like this session's (#533). `PROJECT_FLOOR_MULT` prices cross-agent
        // reuse and was calibrated on the single-agent case, and nothing recorded
        // enough to tell the two apart after the fact.
        if let Some(Substitution {
            folded,
            emitted: markers,
            ..
        }) = &projected
        {
            // Every line folded means the agent holds markers and nothing else,
            // which is the case `MIN_WHOLE_OUTPUT_FOLD` refuses below 1 KB. Read
            // here rather than threaded out of `substitute`, because that flag is
            // per run while this table is already one row per (origin, source
            // agent) per call. The two differ only when adjacent runs of
            // different origins tile the payload, and for the question this
            // column answers, whether the agent kept any content at all, the
            // call-level reading is the right one.
            let whole_output = folded.len() == lines.len();
            let mut tally: HashMap<(&'static str, &str), (usize, usize)> = HashMap::new();
            for &i in folded {
                let Some(origin) = origin_of(&hashes[i]) else {
                    continue;
                };
                let (label, source) = match origin.origin {
                    Origin::Session => ("session", in_session.get(&hashes[i])),
                    Origin::Project => ("project", in_project.get(&hashes[i])),
                };
                // A hit with no agent cannot happen through `ledger_record`, which
                // always writes one. Counting it as `unknown` beats dropping the
                // row and quietly understating the total.
                let entry = tally
                    .entry((label, source.map_or("unknown", |s| s.agent.as_str())))
                    .or_default();
                entry.0 += 1;
                entry.1 += lines[i].len();
            }
            let folds: Vec<crate::store::sqlite::FoldRecord> = tally
                .into_iter()
                .map(
                    |((origin, source_agent), (lines, bytes))| crate::store::sqlite::FoldRecord {
                        source_agent: source_agent.to_string(),
                        origin,
                        lines,
                        bytes,
                        whole_output,
                        payload_bytes: text.len(),
                    },
                )
                .collect();
            // Keyed on the project when there is one: the question is about a
            // repository two agents share, and a session scope cannot be compared
            // across agents by definition.
            let scope = self.project.as_deref().unwrap_or(&self.scope);
            // `scope` above is the project when there is one, so the session
            // has to travel separately or the row cannot answer for itself.
            self.store.ledger_record_folds(
                scope,
                &self.agent,
                session_of(&self.scope),
                &self.tool,
                &folds,
                markers,
            );
        }

        self.store
            .ledger_record(&self.scope, &delivered, &self.agent, &self.source);
        // The project history is written too, so a later session can draw on this
        // one. Same rows, a second scope key. A folded line is already in both
        // scopes, since that is what made it foldable, so filtering it out of
        // this write changes nothing and keeps the two calls saying one thing.
        if let Some(p) = &self.project {
            self.store
                .ledger_record(p, &delivered, &self.agent, &self.source);
        }

        projected.map(|s| (s.view, shifts))
    }

    /// The marker one run would be replaced by, rendered rather than estimated
    /// so the test that weighs it and the string that is sent cannot drift (#450).
    ///
    /// A run covering every line means the reply is this marker and nothing else,
    /// which needs different wording (#519), and that wording is longer: an
    /// earlier draft weighed the short one and emitted the long one.
    fn marker_for(
        &self,
        run: &Run,
        seen: &Seen,
        total: usize,
        handle: &str,
    ) -> (String, &'static str) {
        let src = &seen.source;
        let lines = run.end - run.start;
        if run.start == 0 && run.end == total {
            seen.origin.whole_output_marker(lines, handle, src)
        } else {
            seen.origin.marker(lines, handle, src)
        }
    }

    /// The view, and the indices of the lines it replaced with a marker.
    ///
    /// The caller needs the second half to record only what it delivered (#465);
    /// returning it beats recomputing which runs folded, which would mean
    /// re-deciding profitability and could disagree with what was emitted.
    fn substitute(
        &self,
        lines: &[&str],
        hashes: &[String],
        origin_of: &dyn Fn(&String) -> Option<Seen>,
    ) -> Option<Substitution> {
        let runs = group_runs(hashes, origin_of);
        if !runs.iter().any(|r| r.seen.is_some()) {
            return None;
        }

        let payload_bytes: usize = lines.iter().map(|l| l.len()).sum();

        // How much of this reply the ledger could take, asked once for the
        // payload rather than once per run (#601, and the review of it).
        //
        // The first draft weighed each run against the payload on its own, which
        // is the wrong denominator and reopens the hole it was closing: three
        // seen blocks of thirty percent, separated by a line or two of new
        // content, each pass the test individually and together fold ninety
        // percent of the reply. The reader is left with markers and the
        // separators, which is the reported case wearing three markers instead
        // of one.
        //
        // Coverage is a property of the payload, so it is computed from the
        // payload. This also subsumes the whole-output floor exactly: one run
        // covering everything is a hundred percent coverage, and the byte test
        // #567 used to make is the same test this makes.
        //
        // Three quarters, not four fifths, since #643. That report is a 658 byte
        // reply whose fourteen lines of `grep` output were the entire question
        // and whose remaining four lines were shell scaffolding: two `echo`
        // headers, a version string and an `ls -l`. At 78.7% it cleared four
        // fifths, so the answer became a handle and the scaffolding was all the
        // reader got. The number is not derived from anything: the fixtures that
        // must still fold sit at 72.1% and 63.9%, that report must not, and
        // three quarters is the only simple fraction in the gap. Priced over the
        // recorded corpus it refuses 9 more of 108 sub-1 KB calls and gives up
        // 3,768 bytes across five days.
        let seen_bytes: usize = runs
            .iter()
            .filter(|r| r.seen.is_some())
            .map(|r| lines[r.start..r.end].iter().map(|l| l.len()).sum::<usize>())
            .sum();
        if payload_bytes < MIN_WHOLE_OUTPUT_FOLD && seen_bytes * 4 >= payload_bytes * 3 {
            return None;
        }

        // What every run would do on its own, decided before any of them is
        // emitted so the shape of the survivors is known in advance. Only the
        // store can still turn a planned fold back into a verbatim run below,
        // and that direction is safe: the caller then refuses the view exactly
        // as it did before any of this existed.
        let mut planned: Vec<bool> = runs
            .iter()
            .map(|run| {
                run.seen.as_ref().is_some_and(|seen| {
                    let marker = self
                        .marker_for(run, seen, lines.len(), &"0".repeat(HANDLE_LEN))
                        .0
                        .len();
                    lines[run.start..run.end]
                        .iter()
                        .map(|l| l.len())
                        .sum::<usize>()
                        >= marker + seen.origin.min_gain()
                })
            })
            .collect();

        // #705. The project scope may fold part of a reply, never the whole of
        // it. A whole-output project fold leaves the reader holding one marker
        // and no content, and the claim in that marker is the one thing it
        // cannot check: it was not there. A subagent's first command came back
        // as `identical to 40 lines from an earlier session, none shown here`
        // and nothing else, and a reviewer dispatched onto a pull request had to
        // pipe files through `base64` to read the code it was sent to review.
        //
        // Not the reader's own set, which is what the report proposed. That
        // condition never fires: of the four recorded whole-output project folds
        // that name a session, every one happened in a session already holding
        // between 261 and 1,369 lines of its own, the 1,319-byte case in the
        // report included. A reader having seen *something* says nothing about
        // whether it has seen *these lines*, which is precisely what the project
        // scope exists to answer for.
        //
        // Every run planned means every line goes, since an unseen or unaffordable
        // run survives verbatim. Unplanning only the project runs rather than
        // refusing outright keeps an honest session fold in the mixed payload, and
        // leaves the reader real lines to weigh the marker against. Priced over
        // the recorded corpus: 10 folds, 32,104 bytes, 1.51% of all folded bytes.
        //
        // #735 widens the same remedy to a second case the coverage floor above
        // cannot see. That floor asks what share of the *payload* went, and a
        // compound reply hides the answer: `cat notes.md; echo ---; tail -5 log`
        // folded all five lines of the `tail` and still measured 45%, because the
        // `cat` before it paid for the other 55%. Nothing here can find the
        // command boundary inside a joined payload, so this asks the command
        // instead of the bytes.
        //
        // A command that rations its own output has already named the answer.
        // `tail -5` means those five lines are the question and not the
        // background, so retrieval is not a risk the project scope is taking, it
        // is the outcome. That is why the bet is off here and nowhere else: the
        // session scope keeps folding, because there the reader is holding the
        // bytes and the handle costs it nothing.
        //
        // Deliberately narrower than `registry::passes_through_verbatim`, which
        // reaches the same conclusion one stage earlier for the collapse
        // fallback. Its list carries `cat` and `grep`, and those are where the
        // project scope earns most of what it earns, so borrowing it wholesale
        // would pay for this report with the feature.
        if planned.iter().all(|&fold| fold) || self.windowed || rations_its_output(&self.source) {
            for (fold, run) in planned.iter_mut().zip(&runs) {
                if run
                    .seen
                    .as_ref()
                    .is_some_and(|s| s.origin == Origin::Project)
                {
                    *fold = false;
                }
            }
        }

        // #814, the separable half of #795. `grep` is the caller's own filter, so
        // the pattern already picked every line in the reply and a partial fold
        // answers a different question rather than the same one shorter: 9 of 11
        // matches folded came back reading as a file with two matches. Whole or
        // nothing, which is what `rations_its_output` gives `head` and `tail`. A
        // re-run printing the identical result still folds, because there the
        // identity is the answer.
        // Narrowed to a fold that draws on something else. A re-run of the same
        // command printing the same lines is the case #755 settled: there the
        // identity is the answer and a partial fold says so in words. What the
        // report hit was another command's output subtracted from a fresh grep.
        if (self.filters_its_reply() || filters_its_own_output(&self.source))
            && !planned.iter().all(|&fold| fold)
            && planned.iter().zip(&runs).any(|(&fold, run)| {
                fold && run
                    .seen
                    .as_ref()
                    .is_some_and(|s| !matches!(s.source, SourceOfSighting::ThisCommandAgain))
            })
        {
            return None;
        }

        // #658, then #664. A host that renumbers what it is handed cannot take
        // survivors sitting in two blocks. #658 answered that by folding only down
        // to the first survivor, which left everything below the first change on
        // the table: 4.4% of a 434 KB CHANGELOG against the 77% its runs were
        // worth. Padding each fold back to its own line count removes the
        // constraint instead of working around it, since the survivors then keep
        // the numbers they came with and no bump is needed.
        //
        // `markers_above` is irrelevant to the question the plan asks, so it asks
        // with zero; the bump is counted for real during the emit below.
        let planned_folds: HashSet<usize> = runs
            .iter()
            .zip(&planned)
            .filter(|(_, fold)| **fold)
            .flat_map(|(run, _)| run.start..run.end)
            .collect();
        let pad = self.renumbers()
            && FoldShift::of(&planned_folds, lines.len(), 0) == FoldShift::Interior;

        let mut out = String::with_capacity(payload_bytes);
        let mut folded: HashSet<usize> = HashSet::new();
        let mut replaced_any = false;
        // How many marker lines stand above the first line that survives. Counted
        // here because this is the only place that knows: a run becomes one marker
        // or stays verbatim, and adjacent runs of different origin are separate
        // runs. Every attempt to infer it downstream was wrong (#573).
        let mut markers_above = 0usize;
        // #766. What was actually handed over, at the grain the marker lives at.
        // `ledger_folds` groups a call by (origin, source agent), so it cannot
        // answer which wording a reader met or which handle they were given.
        let mut emitted: Vec<crate::store::sqlite::MarkerRecord> = Vec::new();
        let mut still_above = true;
        let mut padded_any = false;
        for (i, run) in runs.iter().enumerate() {
            let body = lines[run.start..run.end].concat();
            // The only question that decides a fold: does this run save more
            // than the marker replacing it costs. The marker is rendered rather
            // than estimated, so the test cannot drift from the string it is
            // weighing, and the handle's length is fixed (#450).
            // One run covering every line means the reply is this marker and
            // nothing else, which needs different wording (#519). Decided here
            // rather than at the emit site so the affordability test below weighs
            // the string that will actually be sent: the whole-output wording is
            // longer, and the first draft of this weighed the short one and
            // emitted the long one, which is the drift the rest of this comment
            // exists to prevent.
            let render =
                |seen: &Seen, handle: &str| self.marker_for(run, seen, lines.len(), handle);

            // Two questions, not one. The first is whether the run outgrows the
            // marker replacing it, which is what every floor here has ever asked.
            // The second only applies when the fold leaves nothing behind: a
            // partial fold hands the agent context it can work from and a handle
            // it may decline, while a whole-output fold hands it a handle and
            // nothing, so needing any part of the payload costs a round trip it
            // has no say in. Four of four recorded under 1 KB were retrieved
            // within nine seconds (#543), so below that floor the trade is
            // negative and the run stays verbatim.
            // What is left to ask per run, now that coverage is settled for the
            // payload above: does this run save more than the marker replacing
            // it costs. The marker is rendered rather than estimated, so the
            // test cannot drift from the string it is weighing (#450).
            // Padding is part of what the fold costs, so the run has to beat it
            // as well as the marker. Without this a short run "saves" bytes it
            // then spends on its own filler.
            let padding = if pad {
                PAD_LINE.len() * (run.end - run.start - 1)
            } else {
                0
            };
            let long_enough = planned[i]
                && run.seen.as_ref().is_some_and(|seen| {
                    // The marker is rendered without its terminator and the emit
                    // adds one back for a run that ended a line, so the test has to
                    // count it: at the boundary a run folded one byte short of the
                    // gain it is required to make (review of #664).
                    let marker = self
                        .marker_for(run, seen, lines.len(), &"0".repeat(HANDLE_LEN))
                        .0
                        .len()
                        + usize::from(body.ends_with('\n'));
                    body.len() >= marker + padding + seen.origin.min_gain()
                });

            // A handle is only offered for content that is provably retrievable.
            // `store_rewind` returns `None` when the row did not land, and the
            // run then stays verbatim rather than becoming a promise nobody can
            // keep (#388).
            // A run whose handle was just pulled stays verbatim. That delivery is
            // the answer to the pull, and folding it hands the reader back the
            // marker it was following (#581). The flag is consumed here, so this
            // costs one delivery rather than exempting the content for good.
            // The run, and the payload it was cut out of. A fold archives one
            // block of a reply, never the reply, so a handle that reported its
            // own length as the whole let `omni retrieve` present ten of
            // fourteen lines as a complete answer (#627).
            match long_enough
                .then(|| self.store.store_rewind(&body, payload_bytes))
                .flatten()
                .filter(|handle| !self.store.take_owed_delivery(handle))
                .zip(run.seen.as_ref())
            {
                Some((handle, seen)) => {
                    let (marker, kind) = render(seen, &handle);
                    out.push_str(&marker);
                    emitted.push(crate::store::sqlite::MarkerRecord {
                        kind,
                        handle: handle.clone(),
                        lines: run.end - run.start,
                        bytes: body.len(),
                    });
                    // The run carried its own terminator, so the marker needs one
                    // only when the text it replaced ended a line. A run at the
                    // very end of an output with no trailing newline does not.
                    if body.ends_with('\n') {
                        out.push('\n');
                    }
                    if pad {
                        for _ in 1..(run.end - run.start) {
                            out.push_str(PAD_LINE);
                        }
                        padded_any = true;
                    }
                    folded.extend(run.start..run.end);
                    replaced_any = true;
                    if still_above {
                        markers_above += 1;
                    }
                }
                None => {
                    out.push_str(&body);
                    still_above = false;
                }
            }
        }
        replaced_any.then_some(Substitution {
            view: out,
            folded,
            markers_above,
            padded: padded_any,
            emitted,
        })
    }
}

/// Consecutive lines that agree about where they were seen.
///
/// Grouping by `Option<Origin>` rather than by a boolean is what stops a session
/// run and a project run merging into one marker, which would have to pick one of
/// two claims for content that is half and half.
fn group_runs(hashes: &[String], origin_of: &dyn Fn(&String) -> Option<Seen>) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    for (i, h) in hashes.iter().enumerate() {
        let seen = origin_of(h);
        match runs.last_mut() {
            Some(last) if last.seen == seen => last.end = i + 1,
            _ => runs.push(Run {
                start: i,
                end: i + 1,
                seen,
            }),
        }
    }
    runs
}

/// Whether the command rationed its own output to a set of lines it named.
///
/// Three forms, and they are one idea: the caller said which lines it wants, so
/// those lines are the answer rather than the background. `head` and `tail`
/// counting lines, and a `sed` address naming line numbers. `tail -f` never ends
/// and `head -c` counts bytes, so neither names a set of lines. A bare
/// `head file` is as explicit as `head -10 file`, since the default is what the
/// reader accepted.
///
/// **Only in command position** (#738). The first shipped version matched the
/// name anywhere, so `grep -rn head src/` and `ls | grep tail` read as a reader
/// rationing itself when the word was somebody else's argument. That is a fold
/// given up for nothing, on two shapes this repository runs constantly.
///
/// **Per segment, and a newline is a separator** (#750). The first version of
/// that narrowing read command position off the previous token, which
/// `split_whitespace` had already stripped the newline from, so a two-line
/// command was one command and nothing on the second line could open one.
/// `cd repo` then `sed -n 58,95p src/app.tsx` is the #741 shape again, and it is
/// 136 of the 168 commands the narrowing cost across the recorded corpus.
///
/// **`sed` ranges count** (#741). The first version covered `head` and `tail`
/// only, and shipped one command family too narrow: `sed -n 60,200p` of a source
/// file names its lines exactly as `tail -5` does, and folding it handed the
/// reader a marker where it was about to edit that code.
/// `registry::passes_through_verbatim` already groups `sed` with `head` and
/// `tail` for the collapse fallback, under this same reasoning.
///
/// **`awk` is deliberately not covered.** Its line selection is an expression
/// rather than a flag, no report has produced one, and the obvious guess is
/// wrong: `NR` appears in `{print NR}`, which prints line numbers rather than
/// selecting them. It gets added when a payload demands it, not before.
///
/// Matched on the basename, so `/usr/bin/tail` counts and a `--tail` flag or a
/// `tailwind` argument does not.
fn rations_its_output(command: &str) -> bool {
    command
        .split(['\n', ';', '|', '&'])
        .any(segment_rations_its_output)
}

/// The same question for one command of the reply, separators already removed.
///
/// A wrapper (`docker exec app tail -5 x`, `ssh host 'tail -6 x'`) runs a
/// program this cannot parse, so nothing in the segment is at a position we can
/// read and the name counts wherever it appears. That errs toward keeping the
/// lines the caller named, which is the direction this predicate exists to
/// protect (#751).
fn segment_rations_its_output(segment: &str) -> bool {
    let tokens: Vec<&str> = segment.split_whitespace().collect();
    let opaque = registry::wraps_another_command(segment);
    tokens.iter().enumerate().any(|(i, tok)| {
        if !opaque && !opens_a_command(&tokens, i) {
            return false;
        }
        match tok.rsplit('/').next().unwrap_or(tok) {
            "head" | "tail" => tokens.get(i + 1).is_none_or(|next| {
                !(next.starts_with('-') && next.trim_start_matches('-').starts_with(['c', 'f']))
            }),
            "sed" => tokens[i + 1..].iter().any(|a| names_line_numbers(a)),
            _ => false,
        }
    })
}

/// Whether the caller's own pattern picked these lines (#814).
///
/// `registry::passes_through_verbatim` reaches the same conclusion one stage
/// earlier, for the collapse fallback, and for the same reason: a second filter
/// cannot know what the first was looking for.
fn filters_its_own_output(command: &str) -> bool {
    command.split(['\n', ';', '|', '&']).any(|segment| {
        let tokens: Vec<&str> = segment.split_whitespace().collect();
        tokens.iter().enumerate().any(|(i, tok)| {
            opens_a_command(&tokens, i)
                && matches!(tok.rsplit('/').next().unwrap_or(tok), "grep" | "rg" | "ag")
        })
    })
}

/// Whether the token at `i` is the first word of its segment's command.
///
/// The segment already ends at the next separator, so this only has to step over
/// the words that introduce a command without being one: `sudo`, `env` and the
/// `VAR=value` assignments a shell strips before exec.
fn opens_a_command(tokens: &[&str], i: usize) -> bool {
    tokens[..i].iter().all(|t| introduces_a_command(t))
}

/// A word that precedes a command without being the command.
fn introduces_a_command(token: &str) -> bool {
    matches!(
        token.rsplit('/').next().unwrap_or(token),
        // `command` is the shell builtin that runs the next word as a program,
        // bypassing a function or alias of the same name. It introduces a
        // command exactly as `env` does (#814 review).
        "sudo" | "env" | "time" | "nohup" | "command"
    ) || token.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    })
}

/// A `sed` address that names line numbers: `5p`, `60,200p`, quoted or not.
///
/// A pattern address (`/foo/p`) and a substitution (`s/a/b/`) are both refused.
/// They filter, which is a different claim from naming the lines wanted, and the
/// ledger only steps aside for the second.
fn names_line_numbers(arg: &str) -> bool {
    let Some(body) = arg.trim_matches(['\'', '"']).strip_suffix('p') else {
        return false;
    };
    let (start, end) = body.split_once(',').unwrap_or((body, body));
    !start.is_empty()
        && !end.is_empty()
        && start.bytes().all(|b| b.is_ascii_digit())
        && end.bytes().all(|b| b.is_ascii_digit())
}

/// A line's identity in the ledger.
///
/// Trimmed, so the same line reached through `sed -n` and through `cat` is one
/// line rather than two. Hashed rather than stored whole because the table is
/// keyed on it and a 4 KB line would otherwise become a 4 KB index entry.
pub fn line_key(line: &str) -> String {
    crate::util::text::short_hash(line.trim())
}

#[cfg(test)]
mod tests {
    /// #581. The main agent's scope has to stay the bare session id, byte for
    /// byte. Composing it differently still folds correctly inside one run,
    /// because both calls agree, so the hook-level tests cannot see a change
    /// here. What it would break is an upgrade: rows written by the previous
    /// binary are keyed on the bare id, and a new formula orphans them mid
    /// session and silently stops folding against them.
    #[test]
    fn the_main_agents_scope_is_the_session_id_unchanged() {
        assert_eq!(super::scope_for("sess-1", None), "sess-1");
        assert_eq!(super::scope_for("sess-1", Some("")), "sess-1");
        assert_eq!(
            super::scope_for("sess-1", Some("agent-9")),
            "sess-1/agent-9"
        );
    }

    use super::*;

    fn temp_store() -> (Store, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open_path(&dir.path().join("omni.db")).expect("store");
        (store, dir)
    }

    /// Long enough to clear `MIN_LEDGER_INPUT`, with a body that clears the run
    /// bounds. Sized against the constants rather than by eye, because a fixture
    /// under a threshold tests the early return instead of the logic.
    fn payload() -> String {
        (0..60)
            .map(|i| format!("2026-08-10T00:00:{i:02}Z  handler finished request {i} in 12ms"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn leaves_the_first_sighting_of_everything_alone() {
        let (store, _d) = temp_store();
        let text = payload();

        assert_eq!(Ledger::new(&store, "s1").project(&text), None);
    }

    #[test]
    fn hands_back_a_handle_for_output_it_already_showed() {
        let (store, _d) = temp_store();
        let text = payload();
        let ledger = Ledger::new(&store, "s1");

        ledger.project(&text);
        let second = ledger.project(&text).expect("a repeat is projectable");

        assert!(second.len() < text.len());
        assert!(second.contains("lines already shown"));
    }

    /// #755. A poll is re-run to find out whether the value moved, and when it
    /// has not, that is the answer. `N lines already shown` does not give it:
    /// it says the bytes are behind the reader without saying they are this
    /// command's own output, unchanged. In the report the surviving half of the
    /// reply was an unrelated table that read as a complete result, and the
    /// seven metric lines the caller was watching went behind a handle.
    ///
    /// Two arms, because the wording moves only for the command that ran again.
    /// A different command still names itself (#622), which is what a reader
    /// needs there: it tells them whether the lines they cannot see came from
    /// something relevant.
    #[test]
    fn a_command_run_again_is_told_its_lines_are_unchanged() {
        let (store, _d) = temp_store();
        let poll = "for p in 0 1 2; do wget -qO- 127.0.0.1:8482/metrics | grep ^is_read_only; done";
        let metrics = fresh_block("is_read_only");

        Ledger::new(&store, "s1").from(poll).project(&metrics);
        let again = Ledger::new(&store, "s1")
            .from(poll)
            .project(&format!("{}{metrics}", fresh_block("capacity")))
            .expect("a verbatim re-run of a recorded block is projectable");

        assert!(
            again.contains("identical to an earlier run"),
            "a re-run has to be told its own output did not change: {again}"
        );
        assert!(
            !again.contains("already shown"),
            "the wording that answers a different question survived: {again}"
        );

        let elsewhere = Ledger::new(&store, "s1")
            .from("kubectl get pvc")
            .project(&format!("{}{metrics}", fresh_block("phase")))
            .expect("the same block under another command is projectable");

        // `from for p in` until #779, which is the loop header and printed
        // nothing. The clause names the stage that wrote the bytes instead.
        assert!(
            elsewhere.contains("already shown") && elsewhere.contains("from grep ^is_read_only"),
            "a different command still has to name the source it is standing in \
             for (#622): {elsewhere}"
        );
    }

    /// #766. Every wording change so far was argued from examples, because
    /// nothing recorded which shape a reader met. `ledger_folds` groups a call by
    /// (origin, source agent) and holds no handle, so it can say how much was
    /// folded and never which marker did it or whether anyone went back for the
    /// bytes.
    ///
    /// The kind comes from the arm that renders the string, so this asserts the
    /// pairing rather than a label somebody typed twice: a re-run records
    /// `session_rerun` and says `identical to an earlier run`, and a different
    /// command records `session_run` and says `already shown`. A parser over the
    /// rendered text would be the second copy #765 just deleted.
    #[test]
    fn a_fold_records_which_marker_shape_it_rendered() {
        let (store, _d) = temp_store();
        let poll = "wget -qO- 127.0.0.1:8482/metrics | grep ^is_read_only";
        let block = fresh_block("is_read_only");

        Ledger::new(&store, "s1").from(poll).project(&block);
        let again = Ledger::new(&store, "s1")
            .from(poll)
            .project(&format!("{}{block}", fresh_block("capacity")))
            .expect("a verbatim re-run is projectable");
        let elsewhere = Ledger::new(&store, "s1")
            .from("kubectl get pvc")
            .project(&format!("{}{block}", fresh_block("phase")))
            .expect("the same block under another command is projectable");

        let rates = store.marker_retrieve_rates(0);
        let by_kind: std::collections::HashMap<&str, i64> =
            rates.iter().map(|r| (r.kind.as_str(), r.folds)).collect();

        assert_eq!(
            by_kind.get("session_rerun").copied(),
            Some(1),
            "a re-run has to be recorded as one: {:?}",
            rates.iter().map(|r| (&r.kind, r.folds)).collect::<Vec<_>>()
        );
        assert_eq!(
            by_kind.get("session_run").copied(),
            Some(1),
            "a different command has to be recorded as one: {:?}",
            rates.iter().map(|r| (&r.kind, r.folds)).collect::<Vec<_>>()
        );
        assert!(
            again.contains("identical to an earlier run") && elsewhere.contains("already shown"),
            "the recorded kinds have to name the wording that was rendered:\n{again}\n{elsewhere}"
        );
        assert!(
            rates.iter().all(|r| r.retrieved == 0),
            "nothing was retrieved in this test, so the rate has to read zero"
        );

        // The join is the deliverable, so it is asserted rather than assumed. An
        // earlier draft of this test passed with the handle column written blank,
        // which would have shipped a retrieve rate that is always zero.
        let handle = again
            .split("omni retrieve ")
            .nth(1)
            .and_then(|rest| rest.split(']').next())
            .expect("the marker carries a handle");
        store.record_retrieve_event("omni retrieve", handle, "claude_code");

        let after = store.marker_retrieve_rates(0);
        let rerun = after
            .iter()
            .find(|r| r.kind == "session_rerun")
            .expect("the re-run row is still there");
        assert_eq!(
            rerun.retrieved,
            1,
            "following the handle back has to land on the shape that printed it: {:?}",
            after
                .iter()
                .map(|r| (&r.kind, r.folds, r.retrieved))
                .collect::<Vec<_>>()
        );

        // #771 review. A handle is the SHA of its content, so one pulled twice
        // used to multiply its own marker through the join: the fold count
        // stopped counting markers and the retrieval sum stopped counting
        // answered questions.
        store.record_retrieve_event("omni retrieve", handle, "claude_code");
        let twice = store.marker_retrieve_rates(0);
        let after_two = twice
            .iter()
            .find(|r| r.kind == "session_rerun")
            .expect("the re-run row survives a second pull");
        assert_eq!(
            (after_two.folds, after_two.retrieved),
            (1, 1),
            "pulling one handle twice has to stay one marker and one answered question: {:?}",
            twice
                .iter()
                .map(|r| (&r.kind, r.folds, r.retrieved))
                .collect::<Vec<_>>()
        );
    }

    /// #627. A fold archives the run it replaced, never the reply, so the handle
    /// has to carry the size of the payload it came out of. Without it
    /// `omni retrieve` frames ten folded lines of a fourteen line output as
    /// `10 lines · 227 B`, which reads as the whole answer.
    #[test]
    fn a_folded_run_records_the_payload_it_was_cut_from() {
        let (store, _d) = temp_store();
        let seen: Vec<String> = (0..10)
            .map(|i| format!("2026-08-10T00:00:{i:02}Z  handler finished request {i} in 12ms"))
            .collect();
        let first = seen.join("\n");
        let mut both = seen.clone();
        for i in 0..4 {
            both.push(format!(
                "2026-08-10T00:01:{i:02}Z  a line nobody has seen, number {i}"
            ));
        }
        let second_input = both.join("\n");

        let ledger = Ledger::new(&store, "s1");
        ledger.project(&first);
        let view = ledger.project(&second_input).expect("the repeat folds");

        let handle = view
            .split("omni retrieve ")
            .nth(1)
            .and_then(|s| s.split(']').next())
            .expect("the marker names a handle");
        let (content, whole) = store
            .retrieve_rewind_sized(handle)
            .expect("the handle resolves");

        assert!(
            whole > content.len(),
            "a folded run is one block of the reply, not the reply: {} vs {}",
            whole,
            content.len()
        );
    }

    /// #519. A re-run of an identical command folded into one run covering the
    /// whole payload, and `N lines already shown` reads as a claim about part of
    /// the output. A reader could not tell a fully elided reply from a command
    /// that printed nothing, and re-running is exactly how a fix gets verified.
    #[test]
    fn a_whole_payload_fold_says_it_is_identical() {
        let (store, _d) = temp_store();
        let text = payload();
        let ledger = Ledger::new(&store, "s1");

        ledger.project(&text);
        let second = ledger.project(&text).expect("a repeat is projectable");

        assert!(
            second.contains("identical to"),
            "a fully folded reply must state the identity: {second:?}"
        );
        assert_eq!(
            second.lines().count(),
            1,
            "one marker, not a marker plus commentary: {second:?}"
        );
    }

    /// The count in that marker is the reader's only check on it, so it has to be
    /// the payload's own line count. #519 shipped `43 lines already shown` beside
    /// `42 lines omitted` for a 43 line payload, and neither number was wrong on
    /// its own.
    #[test]
    fn the_whole_payload_marker_counts_the_whole_payload() {
        let (store, _d) = temp_store();
        let text = payload();
        let ledger = Ledger::new(&store, "s1");

        ledger.project(&text);
        let second = ledger.project(&text).expect("a repeat is projectable");

        assert!(
            second.contains(&format!("the {} lines", text.lines().count())),
            "expected {} lines in {second:?}",
            text.lines().count()
        );
    }

    /// The partial case keeps the run wording, because there it is true: some of
    /// this was shown before and the rest is new. Guarding the fix from becoming
    /// "every fold claims the output is identical".
    #[test]
    fn a_partial_fold_keeps_the_run_wording() {
        let (store, _d) = temp_store();
        let text = payload();
        let ledger = Ledger::new(&store, "s1");

        ledger.project(&text);
        let extended = format!("{text}\nsomething this session has never seen before\n");
        let second = ledger.project(&extended).expect("the repeated head folds");

        assert!(
            second.contains("lines already shown") && !second.contains("identical to"),
            "a partial fold must not claim identity: {second:?}"
        );
        assert!(
            second.contains("never seen before"),
            "the new line has to survive: {second:?}"
        );
    }

    /// #543 priced whole-output folds and `MIN_WHOLE_OUTPUT_FOLD` refuses them
    /// under 1 KB, but nothing recorded which folds were whole-output, so the
    /// floor could not be checked against the corpus that calibrated it. Both
    /// shapes are asserted in one test because the column is only useful if it
    /// separates them: a flag that is always 1 answers the query the same way as
    /// no flag at all.
    #[test]
    fn records_whether_a_fold_covered_the_whole_output() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("omni.db");
        let store = Store::open_path(&db).expect("store");
        let text = payload();
        let ledger = Ledger::new(&store, "s1");

        ledger.project(&text);
        let whole = ledger.project(&text).expect("a repeat is projectable");
        assert!(
            whole.contains("identical to"),
            "this arm has to be a whole-output fold: {whole:?}"
        );

        // One line this session has never seen, so the head folds and the tail
        // stays. Same ledger, so the only difference is the shape of the fold.
        let extended = format!("{text}\nsomething this session has never seen before\n");
        let partial = ledger.project(&extended).expect("the repeated head folds");
        assert!(
            !partial.contains("identical to"),
            "this arm has to be a partial fold: {partial:?}"
        );

        let conn = rusqlite::Connection::open(&db).expect("open");
        let rows: Vec<(i64, i64)> = conn
            .prepare("SELECT whole_output, payload_bytes FROM ledger_folds ORDER BY id")
            .expect("prepare")
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("query")
            .collect::<Result<_, _>>()
            .expect("rows");

        assert_eq!(
            rows.iter().map(|r| r.0).collect::<Vec<_>>(),
            vec![1, 0],
            "a whole-output fold then a partial one; \
             without the split the floor stays unverifiable"
        );
        // Both calls land in the same second, so a `GROUP BY ts` audit would add
        // these two payloads together and compare the total with the floor. The
        // size has to be readable per row for the audit to mean anything.
        assert_eq!(
            rows[0].1,
            text.len() as i64,
            "the whole-output row must carry its own payload size"
        );
        assert_eq!(
            rows[1].1,
            extended.len() as i64,
            "each call carries its own size, not the pair's total"
        );
    }

    /// Six lines, 341 bytes. Above `MIN_LEDGER_INPUT` and above a whole-output
    /// marker plus the session gain, so without the #543 floor this folds
    /// entirely; below `MIN_WHOLE_OUTPUT_FOLD`, so with it the run stays verbatim.
    /// Sized against all three constants because a fixture that clears only one of
    /// them tests a different branch than the one named here.
    fn under_the_whole_output_floor() -> String {
        (0..6)
            .map(|i| format!("2026-08-10T00:00:{i:02}Z  handler finished request {i} in 12ms"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// A repeated block and a fresh one, which since #705 is the only shape a
    /// project fold may take: the first is foldable, the second is not, so the
    /// reader keeps real lines to weigh the marker against. Returns the block to
    /// prime the first session with, then the payload the second session sends.
    ///
    /// The repeated half is six times the session floor so the project scope's
    /// own bar is cleared and a fold really happens. Under it the second reader is
    /// simply shown the lines and a test asserting on an absent fold passes for
    /// the wrong reason.
    fn project_repeat_then_fresh() -> (String, String) {
        let repeated = project_repeat();
        let payload = format!("{repeated}{}", fresh_block("cache probe"));
        (repeated, payload)
    }

    /// The block a first session is primed with, six times the session floor so
    /// the project scope's higher bar is cleared and a fold really happens.
    fn project_repeat() -> String {
        (0..200)
            .map(|i| format!("2026-08-10T00:00:00Z  handler finished request {i} in 12ms\n"))
            .collect()
    }

    /// Runs git in `dir`, hermetically: nothing the surrounding environment says
    /// about git reaches it, and a missing config file is an empty one to git.
    ///
    /// Every inherited `GIT_*` goes, rather than a list of the ones that bite.
    /// `GIT_DIR` alone is enough to point the whole test at another repository,
    /// and it fails as `fatal: this operation must be run in a work tree` from a
    /// test that never mentions a work tree (PR #754 review).
    fn git(dir: &std::path::Path, args: &[&str]) {
        let mut cmd = std::process::Command::new("git");
        cmd.current_dir(dir).args(args);
        for (name, _) in std::env::vars_os() {
            // Uppercased first: Windows env names are case-insensitive to git,
            // so a `git_dir` would survive a case-sensitive prefix check and
            // point the test at another repository (PR #754 review).
            if name
                .to_string_lossy()
                .to_ascii_uppercase()
                .starts_with("GIT_")
            {
                cmd.env_remove(&name);
            }
        }
        let out = cmd
            .env("GIT_CONFIG_GLOBAL", dir.join("no-such-gitconfig"))
            .env("GIT_CONFIG_SYSTEM", dir.join("no-such-gitconfig"))
            .env("GIT_AUTHOR_NAME", "omni test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "omni test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .output()
            .expect("git has to be on PATH for this test to mean anything");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Lines no scope has seen, tagged so two calls can each carry their own.
    fn fresh_block(tag: &str) -> String {
        (0..20)
            .map(|i| format!("2026-08-10T00:00:00Z  {tag} {i} missed and refilled\n"))
            .collect()
    }

    /// #543. A whole-output fold leaves the agent a handle and no content, so any
    /// part of the payload it still needs costs a retrieval round trip. Four of
    /// the four recorded under 1 KB were retrieved within nine seconds, against a
    /// 0.85% retrieve rate across the store, so the fold spent four extra tool
    /// calls to save 2,680 bytes it then handed back.
    ///
    /// The partial arm is asserted in the same test on purpose: the floor must not
    /// become "no small payload ever folds", which would cost the runs that are
    /// still profitable because the agent keeps context beside them.
    #[test]
    fn never_folds_a_whole_output_that_cannot_pay_for_the_round_trip() {
        let (store, _d) = temp_store();
        let text = under_the_whole_output_floor();
        assert!(
            text.len() > MIN_LEDGER_INPUT && text.len() < MIN_WHOLE_OUTPUT_FOLD,
            "fixture must sit between the two floors, got {} bytes",
            text.len()
        );
        let ledger = Ledger::new(&store, "s1");

        ledger.project(&text);
        assert_eq!(
            ledger.project(&text),
            None,
            "a sub-1 KB repeat must stay verbatim rather than become a bare handle"
        );

        // Four new lines, not one. #601 widened the floor to cover a fold that
        // takes three quarters or more of its payload, on the evidence that three
        // surviving lines out of twenty-six were not content anyone could use.
        // One appended line put this fixture on the wrong side of that, so the
        // arm below was asserting the floor rather than the absence of it, which
        // is the failure this test's own prose warns about.
        let extended = format!(
            "{text}\n{}",
            (0..4)
                .map(|i| format!("something this session has never seen before, {i}\n"))
                .collect::<String>()
        );
        assert!(
            text.len() * 4 < extended.len() * 3,
            "the repeated head must be under three quarters of the payload, or this \
             arm tests the #601 floor instead of a partial fold"
        );
        let partial = ledger
            .project(&extended)
            .expect("the repeated head still folds beside content the agent can use");
        assert!(
            partial.contains("lines already shown") && partial.contains("never seen before"),
            "the floor must not reach partial folds: {partial:?}"
        );
    }

    /// The promise the marker makes. A handle that does not resolve to the exact
    /// bytes it replaced makes every other guarantee here worthless.
    #[test]
    fn every_handle_resolves_to_the_bytes_it_replaced() {
        let (store, _d) = temp_store();
        let text = payload();
        let ledger = Ledger::new(&store, "s1");
        ledger.project(&text);

        let second = ledger.project(&text).expect("projectable");

        let handle = second
            .split_once("omni retrieve ")
            .and_then(|(_, rest)| rest.split_once(']'))
            .map(|(h, _)| h.to_string())
            .expect("a handle was offered");
        assert_eq!(store.retrieve_rewind(&handle), Some(text.clone()));
    }

    /// Non negotiable, per the header. A nondeterministic view breaks the
    /// agent's upstream prompt cache, and a cache miss costs more than the
    /// compaction saved.
    #[test]
    fn renders_identical_bytes_for_identical_state() {
        let (store, _d) = temp_store();
        let text = payload();
        Ledger::new(&store, "s1").project(&text);

        let first = Ledger::new(&store, "s1").project(&text);
        let second = Ledger::new(&store, "s1").project(&text);

        assert_eq!(first, second);
        assert!(first.is_some(), "the fixture must reach the substitution");
    }

    /// One session's history is not another's. Without this the ledger would
    /// tell a fresh session it had already seen output it never received, which
    /// is the false-claim defect this project is named after.
    /// #610, the other half. Prompt caching is prefix-matched, so what the model
    /// already holds has to serialise the same way every time it is replayed. The
    /// ledger is the only stateful stage here: the same command's output is
    /// rewritten differently depending on what earlier commands showed, which is
    /// correct for a new insertion and would be a defect if it were not
    /// reproducible.
    ///
    /// Two stores, same sequence, byte for byte. The fixture asserts the fold
    /// actually happened first, because three identical passthroughs would
    /// satisfy the comparison while testing nothing.
    #[test]
    fn the_same_sequence_against_the_same_history_replays_byte_for_byte() {
        fn sequence(store: &Store) -> Vec<Option<String>> {
            let text = payload();
            let ledger = Ledger::new(store, "s1").with_project("/repo");
            (0..3).map(|_| ledger.project(&text)).collect()
        }

        let (a, _da) = temp_store();
        let (b, _db) = temp_store();
        let first = sequence(&a);
        let second = sequence(&b);

        assert!(
            first[0].is_none() && first[1].is_some(),
            "the fixture has to reach the fold: the first sighting stays verbatim \
             and the repeat folds, got {:?}",
            first
                .iter()
                .map(|t| t.as_ref().map(String::len))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            first, second,
            "the same history replayed to different bytes, so what the model holds \
             is not reproducible and cannot be cached"
        );
    }

    #[test]
    fn never_claims_another_sessions_history() {
        let (store, _d) = temp_store();
        let text = payload();
        Ledger::new(&store, "s1").project(&text);

        assert_eq!(Ledger::new(&store, "s2").project(&text), None);
    }

    /// #509. The project scope is one history per directory, so the column is
    /// the only thing that can say whose lines it holds. `INSERT OR IGNORE`
    /// keeps the first writer, which is what makes "a different agent was shown
    /// this" answerable at all: overwriting on a repeat would report the last
    /// reader as the one who saw it.
    #[test]
    fn records_which_agent_was_shown_each_line() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("omni.db");
        let store = Store::open_path(&db).expect("store");
        // Above `MIN_LEDGER_INPUT` and below the project scope's fold bar, so
        // the second agent is *shown* every line rather than handed a marker.
        // Sized against the constants because the obvious fixture, the same
        // 200-line payload the floor test uses, folds the whole run: the second
        // agent then delivers nothing, writes nothing, and the test passes with
        // `INSERT OR REPLACE` in place of `INSERT OR IGNORE`.
        let text: String = (0..6)
            .map(|i| format!("2026-08-10T00:00:00Z  handler finished request {i} in 12ms\n"))
            .collect();
        assert!(
            text.len() > MIN_LEDGER_INPUT && text.len() < MIN_LEDGER_RUN_GAIN * PROJECT_FLOOR_MULT,
            "fixture sits on the wrong side of a threshold: {} bytes",
            text.len()
        );

        Ledger::new(&store, "s1")
            .with_project("/repo")
            .by("claude_code")
            .project(&text);
        Ledger::new(&store, "s2")
            .with_project("/repo")
            .by("codex")
            .project(&text);

        let conn = rusqlite::Connection::open(&db).expect("open");
        let rows = |agent: &str| -> i64 {
            conn.query_row(
                "SELECT COUNT(*) FROM ledger_lines WHERE scope = '/repo' AND agent_id = ?1",
                [agent],
                |r| r.get(0),
            )
            .expect("count")
        };

        assert!(
            rows("claude_code") > 0,
            "the agent the lines were delivered to was not recorded"
        );
        assert_eq!(
            rows("codex"),
            0,
            "a repeat rewrote the agent that was actually shown the line"
        );
    }

    /// #533. Whether the project scope stays shared across agents or becomes
    /// `(repo, agent)` cannot be argued, only measured, and it could not be
    /// measured: `ledger_lines` says a line was *seen*, never that a marker was
    /// issued against it or whose bytes that marker replaced. This is the row
    /// the corpus query needs, and the cross-agent case is the only one that
    /// decides anything, so it is the one pinned here.
    #[test]
    fn records_the_agent_whose_lines_a_project_fold_replaced() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("omni.db");
        let store = Store::open_path(&db).expect("store");
        let (repeated, payload) = project_repeat_then_fresh();

        Ledger::new(&store, "s1")
            .with_project("/repo")
            .by("claude_code")
            .project(&repeated);
        let view = Ledger::new(&store, "s2")
            .with_project("/repo")
            .by("codex")
            .project(&payload)
            .expect("no fold on the second agent, so there is nothing to record");
        assert!(
            view.contains("not shown here"),
            "this has to be a project fold to be the case under test: {view}"
        );

        let conn = rusqlite::Connection::open(&db).expect("open");
        let (agent, source, lines, bytes): (String, String, i64, i64) = conn
            .query_row(
                "SELECT agent_id, source_agent, lines, bytes
                 FROM ledger_folds WHERE origin = 'project'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .expect("a project fold left no row, so cross-agent reuse stays unpriceable");

        assert_eq!(agent, "codex", "the agent the marker was delivered to");
        assert_eq!(
            source, "claude_code",
            "the agent whose lines were replaced, which is the whole measurement"
        );
        assert!(lines > 0 && bytes > 0, "{lines} lines, {bytes} bytes");
    }

    /// #705. A reader the project scope answers for has not seen those bytes, so
    /// replacing the whole payload leaves it holding one marker and nothing it
    /// can check the marker against. A subagent's first command came back as
    /// `identical to 40 lines from an earlier session, none shown here` and
    /// nothing else, and it is not a subagent property: a second session on the
    /// same project gets the identical empty reply.
    ///
    /// The counter-case is asserted in the same test on purpose. The rule is
    /// "never the whole of a reply", not "the project scope stops folding": the
    /// partial arm is 635 folds and 678,585 bytes against the 10 folds and
    /// 32,104 bytes this gives up.
    #[test]
    fn never_replaces_a_whole_payload_from_the_project_scope() {
        let (store, _d) = temp_store();
        let repeated = project_repeat();
        Ledger::new(&store, "s1")
            .with_project("/repo")
            .by("claude_code")
            .project(&repeated);

        for (label, reader) in [
            (
                "a fresh session",
                Ledger::new(&store, "s2").with_project("/repo"),
            ),
            (
                "a subagent of the first",
                Ledger::new(&store, scope_for("s1", Some("sub-agent-1"))).with_project("/repo"),
            ),
        ] {
            assert!(
                reader.project(&repeated).is_none(),
                "{label} was handed a marker and zero content lines"
            );
        }

        // Same repeated block, now inside a payload that carries lines nobody has
        // seen. This must still fold, or the fix reads as "the project scope is
        // off" rather than "it may not take the whole reply".
        let view = Ledger::new(&store, "s3")
            .with_project("/repo")
            .project(&format!("{repeated}{}", fresh_block("cache probe")))
            .expect("a project run inside a payload is still projectable");
        assert!(
            view.contains("not shown here"),
            "the partial project fold stopped firing: {view}"
        );
        assert!(
            view.contains("cache probe 0"),
            "the reader must keep the lines the fold did not claim: {view}"
        );
    }

    /// The whole licence for the project scope. A second session may reuse the
    /// first session's history, but it has **not seen those bytes**, so the
    /// marker must not say it has. An earlier draft cancelled this phase over
    /// exactly that wording; the remedy was the wording.
    ///
    /// Since #705 the licence is narrower: a project fold is a run inside a
    /// payload, never the whole of one, so the fixture carries fresh lines the
    /// second session keeps. What is asserted here is unchanged.
    #[test]
    fn never_tells_a_new_session_it_has_already_seen_the_project_history() {
        let (store, _d) = temp_store();
        let (repeated, payload) = project_repeat_then_fresh();
        Ledger::new(&store, "s1")
            .with_project("/repo")
            .project(&repeated);

        let view = Ledger::new(&store, "s2")
            .with_project("/repo")
            .project(&payload)
            .expect("a project repeat above the floor is projectable");

        assert!(
            view.contains("not shown here"),
            "the marker claimed a sighting this session never had: {view}"
        );
        assert!(
            !view.contains("already shown"),
            "a project repeat was reported as a session repeat: {view}"
        );
    }

    /// #735. The coverage floor asks what share of the *payload* a fold took, and
    /// a compound reply hides the answer. The report folded all five lines of a
    /// `tail -5` and still measured 45%, because the `cat` in front of it paid for
    /// the other 55%, so the reader was handed a header announcing content and
    /// then a marker instead of it.
    ///
    /// Both arms in one test on purpose. The guard has to be the command and not
    /// the project scope: unbudgeted commands must still fold, or the fix costs
    /// the feature rather than the defect.
    ///
    /// Each arm carries its own fresh block, and that is load-bearing. Sharing
    /// one payload lets the first arm record the other's survivors, which leaves
    /// every run project-seen and hands the refusal to #705's whole-output guard
    /// instead. The test then passes with this guard deleted, which is how the
    /// first draft of it read.
    #[test]
    fn a_line_budgeted_command_keeps_the_lines_it_asked_for() {
        let (store, _d) = temp_store();
        let repeated = project_repeat();
        let unbudgeted = format!("{repeated}{}", fresh_block("cache probe"));
        let budgeted = format!("{repeated}{}", fresh_block("queue probe"));
        Ledger::new(&store, "s1")
            .with_project("/repo")
            .project(&repeated);

        let folded = Ledger::new(&store, "s2")
            .with_project("/repo")
            .from("cat handlers.log")
            .project(&unbudgeted)
            .expect("a command that named no line budget still folds");
        assert!(
            folded.contains("not shown here"),
            "the unbudgeted arm stopped folding, so this test no longer proves \
             the guard reads the command: {folded}"
        );

        // Same store, same project history, survivors of its own. Only the
        // command differs.
        let view = Ledger::new(&store, "s3")
            .with_project("/repo")
            .from("cat notes.md; echo ---; tail -5 handlers.log")
            .project(&budgeted)
            .unwrap_or_else(|| budgeted.clone());
        assert!(
            !view.contains("not shown here"),
            "a tail -5 was answered with a handle for lines this session never held: {view}"
        );
        assert!(
            view.contains("handler finished request 0"),
            "the budgeted arm kept the marker out but lost the lines anyway: {view}"
        );
    }

    /// #795. The project scope is a bet that another session is alive, or was
    /// minutes ago. A sighting from yesterday is a different claim: that context
    /// has been gone for hours, the marker names a command the reader cannot
    /// place, and in the report it named a directory that no longer existed.
    ///
    /// Both arms on one store, because the age is the only variable that may
    /// separate them.
    #[test]
    fn a_project_fold_needs_a_fresh_sighting() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("omni.db");
        let store = Store::open_path(&db).expect("store");
        let repeated = project_repeat();

        // A scope per arm, each with its own writer and its own fresh block.
        // Sharing one scope makes the second fold a whole-output repeat, which
        // #705 refuses on its own, and the test then passes with the bound
        // removed. Sharing one session scope makes the second
        // call a repeat, and what gets recorded is then the marker that was
        // delivered rather than the lines, so the second project scope stays
        // empty and the arm below proves nothing.
        for (session, scope) in [("writer-a", "/repo-fresh"), ("writer-b", "/repo-stale")] {
            Ledger::new(&store, session)
                .with_project(scope)
                .project(&repeated);
        }

        let fresh_payload = format!("{repeated}{}", fresh_block("cache probe"));
        let fresh = Ledger::new(&store, "reader-1")
            .with_project("/repo-fresh")
            .project(&fresh_payload)
            .expect("a sighting from a session still running folds");
        assert!(fresh.contains("not shown here"), "{fresh}");

        let aged = crate::guard::limits::PROJECT_SIGHTING_MAX_AGE_SECS + 60;
        let conn = rusqlite::Connection::open(&db).expect("open");
        let moved = conn
            .execute(
                "UPDATE ledger_lines SET ts = ts - ?1 WHERE scope = '/repo-stale'",
                [aged],
            )
            .expect("age the rows");
        drop(conn);
        assert!(moved > 0, "nothing was aged, so the arm proves nothing");

        let stale_payload = format!("{repeated}{}", fresh_block("queue probe"));
        let stale = Ledger::new(&store, "reader-2")
            .with_project("/repo-stale")
            .project(&stale_payload);
        assert!(
            stale.is_none(),
            "a sighting older than the bound was folded anyway: {stale:?}"
        );
    }

    /// #795, PR #825 review. The bound asks when these bytes last reached an
    /// agent, so a line shown again now is fresh again however old the first
    /// sighting is. Recording it with `INSERT OR IGNORE` would keep the first
    /// timestamp and refuse the fold the day after a repository is first read.
    ///
    /// The attribution is asserted in the same test because only `ts` may move:
    /// the marker names the command that first showed the line (#622).
    #[test]
    fn a_line_shown_again_is_fresh_again() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("omni.db");
        let store = Store::open_path(&db).expect("store");
        let repeated = project_repeat();

        Ledger::new(&store, "writer-a")
            .with_project("/repo")
            .by("claude_code")
            .project(&repeated);

        let aged = crate::guard::limits::PROJECT_SIGHTING_MAX_AGE_SECS + 60;
        let conn = rusqlite::Connection::open(&db).expect("open");
        let moved = conn
            .execute("UPDATE ledger_lines SET ts = ts - ?1", [aged])
            .expect("age the rows");
        drop(conn);
        assert!(moved > 0, "nothing was aged, so the test proves nothing");

        // Stale, so this one is refused and the lines are delivered verbatim.
        // That delivery is the repeat sighting.
        let refused = Ledger::new(&store, "writer-b")
            .with_project("/repo")
            .by("codex")
            .project(&format!("{repeated}{}", fresh_block("cache probe")));
        assert!(refused.is_none(), "the aged sighting folded: {refused:?}");

        let after = Ledger::new(&store, "reader-1")
            .with_project("/repo")
            .project(&format!("{repeated}{}", fresh_block("queue probe")))
            .expect("the repeat sighting is minutes old and folds");
        assert!(after.contains("not shown here"), "{after}");

        let conn = rusqlite::Connection::open(&db).expect("open");
        let first: String = conn
            .query_row(
                "SELECT DISTINCT agent_id FROM ledger_lines WHERE scope = '/repo'",
                [],
                |r| r.get(0),
            )
            .expect("one agent recorded");
        assert_eq!(
            first, "claude_code",
            "the repeat sighting overwrote whose bytes the fold is replacing"
        );
    }

    /// #814, split out of #795. Every line of a `grep` reply is the answer the
    /// pattern asked for, so a partial fold reads as a different answer: 9 of 11
    /// matches folded came back looking like a file with two matches. A re-run
    /// that prints the identical result still folds, because there the identity
    /// is what was asked.
    #[test]
    fn a_grep_folds_whole_or_not_at_all() {
        let (store, _d) = temp_store();
        let line = |i: usize| {
            format!(
                "{}:  test(\"case {i} keeps the platform admin check honest\", async () => {{\n",
                100 + i * 7
            )
        };
        let shown: String = (0..14).map(line).collect();
        let grep = "grep -n \"^  test(\" apps/server/src/http/platform-orgs.test.ts";

        // `command grep` is the same filter: the builtin only decides which
        // program runs (#814 review).
        assert!(filters_its_own_output("command grep -rn handler src/"));
        assert!(filters_its_own_output("sudo rg --hidden pattern ."));
        assert!(!filters_its_own_output("cat notes.md"));

        // The reported shape: the lines were printed by something else, and the
        // grep that follows is a fresh question about the file.
        Ledger::new(&store, "s1")
            .from("cd /tmp/pr-928 && cat apps/server/src/http/platform-orgs.test.ts")
            .project(&shown);
        let partial: String = (0..28).map(line).collect();
        assert!(
            Ledger::new(&store, "s1")
                .from(grep)
                .project(&partial)
                .is_none(),
            "half the matches came from another command, so the reply goes whole"
        );

        // The same command run again, printing what it printed before: the
        // identity is the answer, which is #755 and stays.
        Ledger::new(&store, "s2").from(grep).project(&shown);
        let again = Ledger::new(&store, "s2")
            .from(grep)
            .project(&format!("{}{shown}", fresh_block("new match")))
            .expect("a re-run of the same command still folds");
        assert!(again.contains("identical to an earlier run"), "{again}");
    }

    /// #796. A `Read` names its window in `offset` and `limit`, never in the path
    /// the ledger is handed as its source, so the flag has to carry it. Both arms
    /// use the same path, so only the flag can separate them.
    #[test]
    fn a_read_window_keeps_the_lines_it_asked_for() {
        let (store, _d) = temp_store();
        let repeated = project_repeat();
        Ledger::new(&store, "s1")
            .with_project("/repo")
            .project(&repeated);

        let whole = format!("{repeated}{}", fresh_block("cache probe"));
        let folded = Ledger::new(&store, "s2")
            .with_project("/repo")
            .from("prisma/schema.prisma")
            .project(&whole)
            .expect("an unwindowed read still folds");
        assert!(folded.contains("not shown here"), "{folded}");

        let window = format!("{repeated}{}", fresh_block("queue probe"));
        let view = Ledger::new(&store, "s3")
            .with_project("/repo")
            .from("prisma/schema.prisma")
            .windowed(true)
            .project(&window)
            .unwrap_or_else(|| window.clone());
        assert!(!view.contains("not shown here"), "{view}");
    }

    /// #788. The changed lines of a diff are never folded, however recently the
    /// same text was shown. The arm without a hunk header folds those very lines,
    /// so the header is what the guard reads.
    #[test]
    fn a_diff_keeps_its_changed_lines() {
        let (store, _d) = temp_store();
        let line = |i: usize| format!("    let value_{i} = compute({i});");
        // Plain, coloured and combined. A `git diff --color` paints both the
        // header and the changed lines, so the arms differ in what they paint.
        for (scope, header, paint) in [
            ("plain", "@@ -1,3 +1,33 @@\n".to_string(), ""),
            (
                "color",
                "\u{1b}[36m@@ -1,3 +1,33 @@\u{1b}[m\n".to_string(),
                "\u{1b}[32m",
            ),
            ("combined", "@@@ -1,3 -1,3 +1,33 @@@\n".to_string(), ""),
        ] {
            let added: String = (0..30).map(|i| format!("{paint}+{}\n", line(i))).collect();
            Ledger::new(&store, scope).project(&added);
            let diff = format!("{header}{added}{}", fresh_block(scope));
            let view = Ledger::new(&store, scope)
                .project(&diff)
                .unwrap_or_else(|| diff.clone());
            assert!(
                view.contains(&added),
                "{scope}: a changed line was folded out of a diff: {view}"
            );
        }

        // The control: the same lines with no hunk header still fold, so the
        // guard reads the diff shape rather than refusing every repeat.
        let added: String = (0..30).map(|i| format!("+{}\n", line(i))).collect();
        Ledger::new(&store, "flat").project(&added);
        let folded = Ledger::new(&store, "flat")
            .project(&format!("{added}{}", fresh_block("flat")))
            .expect("the same lines without a hunk header fold");
        assert!(folded.contains("already shown"), "{folded}");
    }

    /// The predicate on its own, in both directions. A line budget counts and a
    /// byte budget or a follow does not, and neither does a word that merely
    /// contains one of the names.
    #[test]
    fn rations_its_output_reads_a_line_budget_and_nothing_else() {
        for cmd in [
            "tail -5 notes.md",
            "tail -n 5 notes.md",
            "cargo test | tail -30",
            "head -20 src/main.rs",
            "head notes.md",
            "/usr/bin/tail -1 notes.md",
            // After a separator, which is normally glued to the token before it.
            "cat notes.md; echo ---; tail -5 handlers.log",
            "cat a.md && head -3 b.md",
            // #741, the reported command, unquoted and quoted.
            "sed -n 60,200p app/run-panel.tsx",
            "sed -n '285,340p' src/lib.rs",
            "sed -n 5p notes.md",
            // #750. A newline separates two commands, and the reply is written
            // that way far more often than with a `;`.
            "cd /path/to/repo\nsed -n '58,95p' src/app.tsx",
            "cd /path/to/repo\ntail -20 /tmp/dev.log",
            // A separator glued to the following token, which the token-position
            // version could not see either.
            "cat a.md;tail -5 b.md",
            // #751. Words that introduce a command without being one.
            "sudo tail -20 /var/log/app.log",
            "env OMNI_PASSTHROUGH=1 sed -n '335,360p' src/hooks/post_tool.rs",
            // #751. A wrapper's argument list is somebody else's command line,
            // so the name counts anywhere inside it.
            "docker exec app tail -5 /var/log/app.log",
            "kubectl exec pod -- head -30 /etc/config.yaml",
        ] {
            assert!(rations_its_output(cmd), "should ration: {cmd}");
        }
        for cmd in [
            "tail -f /var/log/app.log",
            "head -c 200 notes.md",
            "cat notes.md",
            "kubectl logs pod --tail=20",
            "npm run build -- --watch tailwind.config.js",
            // #738. The word is somebody else's argument, not a reader
            // rationing itself, and both of these run constantly here.
            "grep tail notes.md",
            "grep -rn head src/",
            "ls | grep tail",
            "cat tail",
            // A `sed` that filters rather than naming lines.
            "sed -n '/failed/p' build.log",
            "sed 's/a/b/' notes.md",
            // Deliberately uncovered, so a future change that starts matching
            // it is a decision rather than a side effect.
            "awk 'NR>=60 && NR<=200' src/lib.rs",
            // #751. The `sed` scan stops at its own command's end, so a later
            // command's `5p` is not this one's address.
            "sed -n '/failed/p' build.log; echo 5p",
            // A newline is a separator, not a licence to match anywhere.
            "cd /path/to/repo\ngrep -rn head src/",
        ] {
            assert!(!rations_its_output(cmd), "should not ration: {cmd}");
        }
    }

    /// #739. The host's prompt cache is pinned to the git snapshot it started on:
    /// a branch switch or a new commit means the next session shares no prefix and
    /// starts cold, even where the bytes are identical. The ledger has no such
    /// constraint, and that is the one saving in this lane the cache structurally
    /// cannot make.
    ///
    /// Nothing asserted it. `paths::project_key` walks to the repository root and
    /// never reads a ref, which is correct by construction and one refactor away
    /// from silently not being true, which is the shape of every guard this
    /// repository has found decorative after the fact.
    ///
    /// Real git rather than a hand-built `.git` directory, because the property is
    /// that a real snapshot change does not move the scope. A fabricated `.git`
    /// would still pass if someone made the key shell out to `git rev-parse`.
    #[test]
    fn a_project_fold_outlives_a_branch_switch_and_a_new_commit() {
        let (store, _d) = temp_store();
        let repo = tempfile::tempdir().expect("tempdir");
        git(repo.path(), &["init", "-q", "-b", "main"]);
        std::fs::write(repo.path().join("app.rs"), "fn main() {}\n").expect("write");
        git(repo.path(), &["add", "app.rs"]);
        git(repo.path(), &["commit", "-qm", "first"]);
        let before = crate::paths::project_key(repo.path());

        let (repeated, payload) = project_repeat_then_fresh();
        Ledger::new(&store, "s1")
            .with_project(&before)
            .project(&repeated);

        git(repo.path(), &["checkout", "-qb", "feature"]);
        std::fs::write(repo.path().join("app.rs"), "fn main() { run() }\n").expect("write");
        git(repo.path(), &["commit", "-qam", "second"]);
        let after = crate::paths::project_key(repo.path());

        assert_eq!(
            after, before,
            "the project scope moved with the branch, so every session after a \
             checkout would open a fresh history"
        );

        let view = Ledger::new(&store, "s2")
            .with_project(&after)
            .project(&payload)
            .expect("the project history has to outlive the snapshot it was recorded on");
        assert!(
            view.contains("not shown here"),
            "a new session on a new commit was not given the project history: {view}"
        );
    }

    /// #567. The run marker said `from an earlier session`, which states where the
    /// lines came from and reads as "you have seen this". It means the opposite:
    /// the project scope only answers for lines the session scope did not, so
    /// those lines were never delivered here.
    ///
    /// A reader acted on it, took a help page missing its `Commands:` block as a
    /// complete one, and concluded the CLI had no uninstall. The run form had no
    /// test of its wording at all: every existing assertion on the phrase is
    /// satisfied by the whole-output marker, so the string could be changed with
    /// the suite green.
    #[test]
    fn a_project_run_marker_says_the_lines_were_not_shown_here() {
        let (store, _d) = temp_store();
        // Repeated lines first, then lines nobody has seen, so the fold is a run
        // inside a payload rather than the whole of it. That is the shape the
        // report hit and the one the whole-output marker never covers.
        let repeated: String = (0..60)
            .map(|i| format!("2026-08-10T00:00:00Z  handler finished request {i} in 12ms\n"))
            .collect();
        let fresh: String = (0..60)
            .map(|i| format!("2026-08-10T00:00:00Z  cache probe {i} missed and refilled\n"))
            .collect();

        Ledger::new(&store, "s1")
            .with_project("/repo")
            .project(&repeated);

        let view = Ledger::new(&store, "s2")
            .with_project("/repo")
            .project(&format!("{repeated}{fresh}"))
            .expect("a project repeat above the floor is projectable");

        assert!(
            view.contains("not shown here"),
            "the marker did not say the lines were never delivered here: {view}"
        );
        assert!(
            !view.contains("already shown"),
            "a project run claimed a sighting this session never had: {view}"
        );
        assert!(
            view.contains(&fresh),
            "this has to be a run inside a payload, not a whole-output fold: {view}"
        );
    }

    /// A marker is not a sighting (#465).
    ///
    /// The session above was handed a pointer, never the bytes behind it, so the
    /// next occurrence in that same session is still a project repeat. Recording
    /// the folded run as shown turned the second marker into `already shown`,
    /// which is false, and dropped the bar from `MIN_LEDGER_RUN_GAIN * 3` to
    /// `MIN_LEDGER_RUN_GAIN`, so the false claim also made the ledger three times
    /// more willing to fold.
    #[test]
    fn a_folded_run_is_not_recorded_as_shown() {
        let (store, _d) = temp_store();
        let repeated = project_repeat();
        Ledger::new(&store, "s1")
            .with_project("/repo")
            .project(&repeated);

        // Each call carries its own fresh tail. The same one twice would put the
        // first call's tail into s2's session scope, and the two runs together
        // would then cover the whole payload, which #705 refuses to fold: the
        // project run would come back verbatim and the test would fail on a
        // fixture artefact rather than on the claim it is about.
        let call = |tag: &str| format!("{repeated}{}", fresh_block(tag));

        // s2 sees it for the first time and is handed a marker, not the bytes.
        let first = Ledger::new(&store, "s2")
            .with_project("/repo")
            .project(&call("cache probe"))
            .expect("a project repeat above the floor is projectable");
        assert!(first.contains("not shown here"), "{first}");

        // Same session, same repeated block. It has still never received it.
        let second = Ledger::new(&store, "s2")
            .with_project("/repo")
            .project(&call("queue drain"))
            .expect("still projectable");

        assert!(
            second.contains("not shown here"),
            "a run this session only ever saw as a marker was reported as shown: {second}"
        );
        assert!(
            !second.contains("already shown"),
            "the ledger claimed a sighting that was a marker: {second}"
        );
    }

    /// Review of #625. A source is a raw command and 46.9% of recorded commands
    /// carry a newline, so interpolating one unfiltered lets the command split
    /// the marker into lines of its own. A marker is a single line, and the count
    /// of markers standing above the first surviving line is derived from that
    /// (#573), so the framing is load-bearing rather than cosmetic.
    #[test]
    fn a_source_cannot_break_out_of_its_marker() {
        let nasty = "cat <<EOF\nsecond line\nthird line\nEOF";
        let marker = Origin::Session
            .marker(
                9,
                &"0".repeat(HANDLE_LEN),
                &SourceOfSighting::Another(nasty.to_string()),
            )
            .0;
        assert_eq!(
            marker.lines().count(),
            1,
            "a multi-line source split the marker: {marker:?}"
        );
        assert!(
            marker.contains("from cat <<EOF") && !marker.contains("second line"),
            "expected the first line only: {marker:?}"
        );

        let tabs = Origin::Session
            .marker(
                9,
                &"0".repeat(HANDLE_LEN),
                &SourceOfSighting::Another("go\ttest\t./...".to_string()),
            )
            .0;
        assert_eq!(tabs.lines().count(), 1, "a tab split the marker: {tabs:?}");
        assert!(
            tabs.contains("from go test ./..."),
            "whitespace was not collapsed: {tabs:?}"
        );
    }

    /// #622. Reading file B had a block elided because file A showed it earlier,
    /// and the marker named neither file. Comparing two files to see whether a
    /// shared block matches is exactly that case, and the dedup answered it by
    /// deleting the evidence.
    ///
    /// Both halves are asserted, because they trade against each other: the
    /// clause has to appear when the source differs, and has to stay away when it
    /// does not. Marker length gates folding, so a clause on every fold would
    /// cost savings on the common case of re-reading one file.
    #[test]
    fn a_fold_says_when_it_is_replacing_another_source() {
        let (store, _d) = temp_store();
        // Shaped like the report: a shared block with unique lines around it. A
        // payload that is *only* the shared block folds whole, which
        // `MIN_WHOLE_OUTPUT_FOLD` refuses under 1 KB, so that fixture tests the
        // floor and never reaches the marker.
        let shared: String = (1..=20)
            .map(|i| format!("    key_{i:02} = \"value_{i:02}\"\n"))
            .collect();
        let file = |name: &str| {
            let uniq: String = (1..=30)
                .map(|i| format!("  uniq_{name}_{i:02} = {i}\n"))
                .collect();
            format!("name = \"{name}\"\n{shared}{uniq}")
        };
        assert!(
            shared.len() > MIN_LEDGER_RUN_GAIN,
            "the shared run must be worth folding, or this tests the gain gate"
        );

        let first = Ledger::new(&store, "s1")
            .from("charlie.tf")
            .project(&file("charlie"));
        assert!(
            first.is_none(),
            "nothing has been shown yet, so there is nothing to fold"
        );

        let cross = Ledger::new(&store, "s1")
            .from("delta.tf")
            .project(&file("delta"))
            .expect("the shared block repeats and is worth a marker");
        assert!(
            cross.contains("from charlie.tf"),
            "a fold drawing on another source did not say so: {cross}"
        );

        // A fresh scope, so this measures the same-source path and not the
        // leftovers of the one above. Sized past `MIN_WHOLE_OUTPUT_FOLD`: a
        // re-read of one file folds the whole payload, which is refused under
        // 1 KB, and that refusal would pass this assertion for the wrong reason.
        let charlie = file("charlie");
        assert!(
            charlie.len() > MIN_WHOLE_OUTPUT_FOLD,
            "fixture must clear the whole-output floor, or the same-source arm proves nothing"
        );
        Ledger::new(&store, "s2")
            .from("charlie.tf")
            .project(&charlie);
        let same = Ledger::new(&store, "s2")
            .from("charlie.tf")
            .project(&charlie)
            .expect("re-reading one file folds its own repeat");
        assert!(
            !same.contains(" from "),
            "re-reading one file paid for a source clause it did not need: {same}"
        );
    }

    /// #779. The clause named a command that cannot have printed the lines: a
    /// bare `cd`, and a `for` loop's header. Both are the first segment of a
    /// multi-line command whose *later* segment did the printing, and the label
    /// was built from the first line of the raw source. A reader who chased the
    /// name found nothing, and one subagent read the mismatch as `Read` handing
    /// back another file's content and re-read the whole tree to be sure.
    ///
    /// Both arms are asserted because they are different answers. One producer
    /// behind a chdir is nameable and gets named; two producers are not, and
    /// stdout carries nothing saying which of them wrote a given line, so the
    /// clause goes away rather than picking one.
    #[test]
    fn a_fold_names_the_command_that_printed_the_lines() {
        let (store, _d) = temp_store();
        let shared: String = (1..=20)
            .map(|i| format!("    key_{i:02} = \"value_{i:02}\"\n"))
            .collect();
        let file = |name: &str| {
            let uniq: String = (1..=30)
                .map(|i| format!("  uniq_{name}_{i:02} = {i}\n"))
                .collect();
            format!("name = \"{name}\"\n{shared}{uniq}")
        };

        Ledger::new(&store, "s1")
            .from("cd /Users/someone/project/repo\ncat charlie.tf")
            .project(&file("charlie"));
        let named = Ledger::new(&store, "s1")
            .from("cat delta.tf")
            .project(&file("delta"))
            .expect("the shared block repeats and is worth a marker");
        assert!(
            named.contains("from cat charlie.tf"),
            "the fold did not name the segment that printed the lines: {named}"
        );
        assert!(
            !named.contains("cd /Users"),
            "the fold credited the bytes to a chdir, which prints nothing: {named}"
        );

        // Two producers, so there is nothing honest to name. A fresh scope, or
        // the rows above answer for these lines first.
        Ledger::new(&store, "s2")
            .from("for f in charlie.tf delta.tf; do echo \"===== $f\"; cat -n \"$f\"; done")
            .project(&file("charlie"));
        let unnamed = Ledger::new(&store, "s2")
            .from("cat delta.tf")
            .project(&file("delta"))
            .expect("the shared block repeats and is worth a marker");
        assert!(
            unnamed.contains("already shown"),
            "dropping the clause dropped the fold with it: {unnamed}"
        );
        assert!(
            !unnamed.contains(" from "),
            "a chain with two producers named one of them: {unnamed}"
        );

        // One producer, inside a loop. `sole_output_command` keeps the keyword
        // that opened the clause because routing has read it that way since the
        // loop rule shipped, and `from do cat "$f"` names a fragment no shell
        // would run.
        Ledger::new(&store, "s3")
            .from("for f in charlie.tf; do cat -n \"$f\"; done")
            .project(&file("charlie"));
        let in_a_loop = Ledger::new(&store, "s3")
            .from("cat delta.tf")
            .project(&file("delta"))
            .expect("the shared block repeats and is worth a marker");
        assert!(
            in_a_loop.contains("from cat -n \"$f\""),
            "the clause kept the keyword that opened the loop body: {in_a_loop}"
        );
    }

    /// The project scope pays a retrieval the session scope does not, so it
    /// carries its own floor. A run that clears the session bar and not the
    /// project one stays verbatim rather than costing the agent a round trip.
    #[test]
    fn holds_a_project_repeat_to_its_own_higher_floor() {
        let (store, _d) = temp_store();
        // Sized to land between the two bars on purpose: it saves more than a
        // session marker plus its gain, and less than a project one. `payload()`
        // clears both, which is what the guard below caught when this test first
        // reached for it.
        let text: String = (0..9)
            .map(|i| format!("2026-08-10T00:00:0{i}Z  handler finished request {i}\n"))
            .collect();
        let session_bar = Origin::Session
            .marker(9, &"0".repeat(HANDLE_LEN), &SourceOfSighting::Unrecorded)
            .0
            .len()
            + Origin::Session.min_gain();
        let project_bar = Origin::Project
            .marker(9, &"0".repeat(HANDLE_LEN), &SourceOfSighting::Unrecorded)
            .0
            .len()
            + Origin::Project.min_gain();
        assert!(
            text.len() > MIN_LEDGER_INPUT && text.len() >= session_bar,
            "fixture must clear the session bar, or it tests the early return"
        );
        assert!(
            text.len() < project_bar,
            "fixture must sit between the two bars, or it tests neither"
        );
        Ledger::new(&store, "s1")
            .with_project("/repo")
            .project(&text);

        // One line neither history has seen, so both folds below are runs rather
        // than whole outputs. Without it the #543 floor decides these cases before
        // either bar is consulted, and the test stops being about the two bars.
        // It also makes the bars above exact: a run marker is what gets rendered
        // here, where a whole-output fold would have rendered the longer wording.
        // Four of them since #601, for the same reason as in the #543 test: one
        // appended line leaves the fold at nine tenths of the payload, which the
        // widened floor now refuses, and the two bars this test exists for would
        // never be reached.
        let probe = format!(
            "{text}{}",
            (0..4)
                .map(|i| format!("a line neither history has ever seen, {i}\n"))
                .collect::<String>()
        );
        assert!(
            text.len() * 4 < probe.len() * 3,
            "the repeat must be under three quarters of the probe, or the #601 floor \
             decides this before either bar is consulted"
        );

        // Same session: over the session floor, so it projects.
        assert!(Ledger::new(&store, "s1").project(&probe).is_some());
        // New session: only the project history has it, and it is too small.
        assert_eq!(
            Ledger::new(&store, "s2")
                .with_project("/repo")
                .project(&probe),
            None
        );
    }

    /// A line in both histories belongs to the session, because that is the
    /// claim that costs the agent nothing. Taking the weaker one would buy a
    /// retrieval for content already in the context window.
    #[test]
    fn prefers_the_session_claim_when_a_line_is_in_both() {
        let (store, _d) = temp_store();
        let text: String = (0..200)
            .map(|i| format!("2026-08-10T00:00:00Z  handler finished request {i} in 12ms\n"))
            .collect();
        let ledger = Ledger::new(&store, "s1").with_project("/repo");
        ledger.project(&text);

        let view = ledger.project(&text).expect("projectable");

        assert!(view.contains("already shown"), "{view}");
        assert!(!view.contains("from an earlier session"), "{view}");
    }

    /// Everything the ledger does not replace comes back byte for byte, line
    /// terminators included. Rebuilding from `lines()` would rewrite a CRLF
    /// payload into LF on the untouched runs, which is a silent edit to content
    /// nobody asked to change and a corrupt diff on Windows.
    #[test]
    fn leaves_untouched_runs_byte_identical_including_crlf() {
        let (store, _d) = temp_store();
        let shared: String = (0..30)
            .map(|i| format!("a shared header line number {i} of this payload\r\n"))
            .collect();
        let ledger = Ledger::new(&store, "s1");
        ledger.project(&shared);

        let mixed = format!(
            "{shared}{}",
            (0..30)
                .map(|i| format!("a fresh line number {i} that has never been shown\r\n"))
                .collect::<String>()
        );
        let view = ledger.project(&mixed).expect("the header is a repeat");

        let fresh_tail: String = (0..30)
            .map(|i| format!("a fresh line number {i} that has never been shown\r\n"))
            .collect();
        assert!(
            view.ends_with(&fresh_tail),
            "the unreplaced tail lost its terminators: {view:?}"
        );
    }

    /// A run under the bounds costs more as a marker than it saves as text.
    #[test]
    fn leaves_a_short_repeated_run_in_place() {
        let (store, _d) = temp_store();
        let shared = "the shared preamble line, over twelve characters";
        let first = format!("{shared}\n{}", payload());
        let ledger = Ledger::new(&store, "s1");
        ledger.project(&first);

        // Only `shared` is a repeat now: one line, far under the run bounds.
        let second = format!(
            "{shared}\n{}",
            (0..60)
                .map(|i| format!("a completely different line number {i} of this second payload"))
                .collect::<Vec<_>>()
                .join("\n")
        );

        assert_eq!(ledger.project(&second), None);
    }

    /// #458, from a real session. A failing `bun` script re-run in the same
    /// session came back with source context and no `TypeError`, because the
    /// error lines had been shown earlier and the fold treated them like any
    /// other repetition. An agent reads that as the failure being gone.
    #[test]
    fn never_folds_a_line_that_states_a_failure() {
        let (store, _d) = temp_store();
        let payload = "19 |   \"x-csrf-token\": csrf,\n                       20 |   Cookie: cookies,\n                       21 | };\n                       22 | \n                       23 | const profiles = await (await fetch(BASE)).json();\n                       24 | const profileId = profiles.profiles[0].id;\n                       TypeError: undefined is not an object (evaluating 'profiles.profiles[0]')\n                             at /tmp/repro237.ts:24:28\n                       Bun v1.3.14 (macOS arm64)\n";

        // Padded past `MIN_WHOLE_OUTPUT_FOLD` since #601. At its original 500-odd
        // bytes the coverage floor now refuses the whole call, which would leave
        // this test asserting that an unfolded payload still contains its own
        // error line, and that is true of any payload. The point here is that the
        // error survives a fold, so the fixture has to be big enough to get one.
        let context: String = (0..12)
            .map(|i| format!("                       {i} |   const value{i} = await load(i);\n"))
            .collect();
        let payload = &format!("{context}{payload}");
        assert!(
            payload.len() > MIN_WHOLE_OUTPUT_FOLD,
            "fixture must clear the coverage floor, or no fold happens at all"
        );

        let ledger = Ledger::new(&store, "s1");
        ledger.project(payload);
        let second = ledger
            .project(payload)
            .expect("a full repeat still folds something");

        assert!(
            second.contains("TypeError: undefined is not an object"),
            "the error was elided on the re-run: {second}"
        );
        assert!(
            second.contains("[OMNI:"),
            "the repeated context should still fold, or this proves nothing: {second}"
        );
    }

    /// The new rule, and the one the old bounds got wrong. Three lines is under
    /// `MIN_LEDGER_RUN_LINES` as it was, but three long lines pay for their
    /// marker several times over, so refusing them was refusing free bytes.
    #[test]
    fn folds_a_short_run_of_long_lines_because_it_pays_for_its_marker() {
        let (store, _d) = temp_store();
        let run: String = (0..3)
            .map(|i| format!("2026-08-11T00:00:0{i}Z  a long structured log line that carries a request id, a latency and a route\n"))
            .collect();
        let ledger = Ledger::new(&store, "s1");
        ledger.project(&format!("{run}{}", payload()));

        let bar = Origin::Session
            .marker(3, &"0".repeat(HANDLE_LEN), &SourceOfSighting::Unrecorded)
            .0
            .len()
            + Origin::Session.min_gain();
        assert!(
            run.len() >= bar,
            "fixture must clear the gain bar ({} vs {bar}), or it tests the old bound",
            run.len()
        );

        let second = format!(
            "{run}{}",
            (0..40)
                .map(|i| format!("a completely different line number {i} of this second payload\n"))
                .collect::<String>()
        );
        let view = ledger
            .project(&second)
            .expect("a run that pays should fold");

        assert!(view.contains("3 lines already shown"), "{view}");
    }

    /// The gain test weighs a marker rendered with a placeholder handle, so it
    /// is only honest while the real handle is the same length. If
    /// `store_rewind` ever widens its slice, the ledger would start folding runs
    /// that do not pay, silently.
    #[test]
    fn renders_a_marker_the_gain_test_can_predict() {
        let (store, _d) = temp_store();
        let handle = store
            .store_rewind_whole("some content worth archiving\n")
            .expect("a healthy store archives");

        assert_eq!(handle.len(), HANDLE_LEN);
        assert_eq!(
            Origin::Session
                .marker(9, &handle, &SourceOfSighting::Unrecorded)
                .0
                .len(),
            Origin::Session
                .marker(9, &"0".repeat(HANDLE_LEN), &SourceOfSighting::Unrecorded)
                .0
                .len()
        );
    }

    /// One fixed-width line, so a fixture's size can be reasoned about against
    /// `MIN_WHOLE_OUTPUT_FOLD` rather than eyeballed. 34 bytes including the
    /// terminator.
    fn row(tag: char, i: usize) -> String {
        format!("{tag} line {i:04} of the fixture payload\n")
    }

    /// #601, as a matrix, because the guard has to hold on one axis and stay out
    /// of the way on the other two. A single fixture proves whichever it happens
    /// to sit on, and the danger here is a guard that quietly stops the folds
    /// this feature exists for.
    ///
    /// Coverage is what the fold takes of the payload; the floor only applies
    /// once that is three quarters or more **and** the run is under
    /// `MIN_WHOLE_OUTPUT_FOLD`.
    /// The hole the review of #601 found in its first draft, kept as its own
    /// test because the matrix above cannot express it: every row there has one
    /// seen block, and this shape has three.
    ///
    /// Three seen blocks of roughly thirty percent each, separated by single new
    /// lines. Weighed per run against the payload, every block passes a four
    /// fifths test on its own and all three fold, so the reader is left with
    /// three markers and two separators, which is the reported case wearing more
    /// markers. Weighed once for the payload, the coverage is what it always was.
    #[test]
    fn counts_coverage_over_the_payload_not_over_one_run() {
        let (store, _d) = temp_store();
        let ledger = Ledger::new(&store, "s1");

        // Eight lines, 272 bytes, so each block clears the per-run gain floor on
        // its own. At six lines it sat under that floor and the test passed
        // whether or not coverage was consulted, which is no test at all.
        let block = |tag: char| (0..8).map(|i| row(tag, i)).collect::<String>();
        let seed = format!("{}{}{}", block('a'), block('b'), block('c'));
        ledger.project(&seed);

        let second = format!(
            "{}{}{}{}{}",
            block('a'),
            row('x', 0),
            block('b'),
            row('x', 1),
            block('c')
        );
        assert!(
            second.len() < MIN_WHOLE_OUTPUT_FOLD,
            "the payload must sit under the floor, or coverage is not consulted"
        );
        let seen = block('a').len() + block('b').len() + block('c').len();
        assert!(
            seen * 4 >= second.len() * 3,
            "the three blocks must clear three quarters together"
        );
        assert!(
            block('a').len() * 4 < second.len() * 3,
            "and no single block may clear it alone, or this tests the old rule"
        );
        assert!(
            block('a').len()
                >= Origin::Session
                    .marker(8, &"0".repeat(HANDLE_LEN), &SourceOfSighting::Unrecorded)
                    .0
                    .len()
                    + Origin::Session.min_gain(),
            "each block must be able to fold on its own, or the guard is not what \
             stops it and this test has no teeth"
        );

        assert_eq!(
            ledger.project(&second),
            None,
            "three sub-threshold blocks folded to markers and two separators"
        );
    }

    /// #643, taken from the row it wrote: `payload_bytes` 695, one project run of
    /// 527 B over 14 lines, `whole_output` 0. Coverage is 75.8%, which clears the
    /// old four-fifths test, so the answer folded and the reader kept four lines
    /// of shell scaffolding: two `echo` headers, a version string and an `ls -l`.
    ///
    /// The numbers are the reported ones rather than round ones on purpose. A
    /// fixture at 50% or at 90% sits on a side of the rule that was already
    /// decided, and this is the band that was not.
    #[test]
    fn refuses_a_fold_that_leaves_only_scaffolding() {
        let (store, _d) = temp_store();
        let ledger = Ledger::new(&store, "s1").with_project("/repo");

        // The fourteen lines a `grep -n` returned, which were the whole question.
        let answer: String = (1..=14)
            .map(|i| format!("{i:02}:export * from \"./module-name-{i:02}\";\n"))
            .collect();
        // What the command printed around them.
        let scaffolding = "=== versi terpasang:\nomni 0.7.6\n\
             lrwxr-xr-x@ 1 user  admin  29 Aug 18 16:33 /opt/local/bin/omni\n\
             === repro 1 (persis yang tadi, urutan sort):\n";
        let payload = format!("{scaffolding}{answer}");

        assert!(
            payload.len() < MIN_WHOLE_OUTPUT_FOLD && payload.len() > MIN_LEDGER_INPUT,
            "the fixture has to sit under the whole-output floor and above the \
             input floor, or it tests an early return: {} bytes",
            payload.len()
        );
        // The fixture has to sit in the gap between the two bars, or it proves
        // nothing: above three quarters so the current rule refuses it, below
        // four fifths so the rule it replaced did not.
        assert!(
            answer.len() * 4 >= payload.len() * 3,
            "the answer must clear three quarters, or something other than this \
             rule is refusing the fold: {} of {}",
            answer.len(),
            payload.len()
        );
        assert!(
            answer.len() * 5 < payload.len() * 4,
            "the answer must NOT clear four fifths, or the old rule already \
             refused this and the fixture proves nothing: {} of {}",
            answer.len(),
            payload.len()
        );
        assert!(
            payload.len() - answer.len() < MIN_LEDGER_INPUT,
            "what survives has to be too small to stand alone, which is the claim"
        );

        // Seen in the project scope, from another session, as a project fold needs.
        Ledger::new(&store, "s0")
            .with_project("/repo")
            .project(&format!(
                "{answer}{}",
                (0..40).map(|i| row('p', i)).collect::<String>()
            ));

        assert_eq!(
            ledger.project(&payload),
            None,
            "the answer folded and left the reader four lines of shell scaffolding"
        );
    }

    #[test]
    fn refuses_a_small_fold_that_would_leave_almost_nothing() {
        // (name, lines reused from the first show, lines new to the second, folds?)
        let cases = [
            // The reported shape: 23 of 26 lines folded, 782 B of an 884 B
            // payload, and the three survivors say nothing about the edit.
            ("small payload, 88% covered", 23, 3, false),
            // Same payload size, a third of it folded: the reader keeps enough
            // to work from, so the trade is the one the ledger is for.
            ("small payload, 38% covered", 10, 16, true),
            // Same coverage as the first row on a payload past the floor. The
            // guard must not reach this: it is the case the feature exists for.
            ("large payload, 92% covered", 55, 5, true),
        ];

        for (name, reused, fresh, should_fold) in cases {
            let (store, _d) = temp_store();
            let ledger = Ledger::new(&store, "s1");

            let first: String = (0..reused).map(|i| row('s', i)).collect();
            let seed = format!(
                "{first}{}",
                (0..40).map(|i| row('p', i)).collect::<String>()
            );
            assert!(
                seed.len() > MIN_LEDGER_INPUT,
                "{name}: seed too small to record"
            );
            ledger.project(&seed);

            let second: String = (0..reused)
                .map(|i| row('s', i))
                .chain((0..fresh).map(|i| row('n', i)))
                .collect();
            let folded_bytes = reused * 34;
            assert_eq!(
                folded_bytes >= MIN_WHOLE_OUTPUT_FOLD,
                should_fold && name.starts_with("large"),
                "{name}: fixture is on the wrong side of the whole-output floor"
            );

            assert_eq!(
                ledger.project(&second).is_some(),
                should_fold,
                "case: {name}"
            );
        }
    }
}
