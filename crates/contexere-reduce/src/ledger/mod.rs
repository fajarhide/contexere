//! The session ledger: what this agent has already been shown.
//!
//! Filters ask what one output can drop; the ledger asks what it repeats from
//! earlier in the session. They are orthogonal: on 7,019 traces, 22.9% of raw
//! bytes were repeats even after every distiller ran. It also reaches file
//! reads, which no filter can shorten without guessing.
//!
//! A run of consecutive lines already emitted becomes one marker with a
//! retrieve handle. Everything else passes through byte for byte.
//!
//! - **Append-only.** Only the output in flight is shortened, never anything
//!   delivered, so the prompt cache prefix stays intact.
//! - **Deterministic.** Same ledger state, same bytes: the handle is a content
//!   address with no timestamp (`renders_identical_bytes_for_identical_state`).
//! - **Nothing is lost.** A run is archived before its marker is written; a
//!   failed archive keeps it verbatim (#388).
//! - **Unknown means untouched.** Callers gate on `pipeline::format::sniff`.

use std::collections::{HashMap, HashSet};

use crate::guard::limits::{
    MIN_LEDGER_INPUT, MIN_LEDGER_RUN_GAIN, MIN_WHOLE_OUTPUT_FOLD, PROJECT_FLOOR_MULT,
};
use crate::pipeline::format::without_ansi_prefix;
use crate::pipeline::{producer, registry};
use crate::store::sqlite::Store;

/// How much of a source name a marker carries: enough for a file name and its
/// directory's tail (#622). Every byte is one a run must beat to fold (#450).
const MAX_SOURCE_IN_MARKER: usize = 40;

/// One line of a source name, safe to put in a marker. A marker must stay one
/// line (#573) and 46.9% of recorded commands contain a newline, so: first line
/// only, control characters dropped, whitespace collapsed, then cut at a char
/// boundary.
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

/// Which history a run was found in, because the two make different claims.
/// `Session`: the agent still holds those bytes. `Project`: they went to another
/// session and this agent never saw them, so the marker says so and the fold
/// has a higher floor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Origin {
    Session,
    Project,
}

impl Origin {
    /// The marker for a replaced run. Every byte is paid on every fold, and a
    /// shorter marker makes smaller runs worth folding (#450).
    fn marker(
        self,
        lines: usize,
        handle: &str,
        source: &SourceOfSighting,
    ) -> (String, &'static str) {
        // Present only when the source differs from the command being answered (#622).
        let from = match source {
            SourceOfSighting::Another(s) => format!(" from {}", source_label(s)),
            _ => String::new(),
        };
        match self {
            // A poll re-run asks whether the value moved, so the marker states the
            // identity rather than "already shown" (#755).
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
            // "Not shown here", not "from an earlier session": the provenance wording read
            // as "you have seen this" and a reader took a truncated help page as complete
            // (#567).
            Self::Project => (
                format!("[OMNI: {lines} lines not shown here{from}, omni retrieve {handle}]"),
                "project_run",
            ),
        }
    }

    /// The marker when the fold covered the whole payload. States the identity, so
    /// a fully elided reply never reads as a command that printed nothing (#519).
    fn whole_output_marker(
        self,
        lines: usize,
        handle: &str,
        source: &SourceOfSighting,
    ) -> (String, &'static str) {
        // No `ThisCommandAgain` arm: this wording already states the identity (#519).
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
            // Nothing of this was delivered here; gated at `MIN_WHOLE_OUTPUT_FOLD` (#567).
            Self::Project => (
                format!(
                    "[OMNI: identical to {lines} lines from an earlier session{from}, none shown here, omni retrieve {handle}]"
                ),
                "project_whole",
            ),
        }
    }

    /// The bytes a fold has to save after its marker. Project scope pays three
    /// times that for the retrieval the reader cannot decline (#448).
    fn min_gain(self) -> usize {
        match self {
            Self::Session => MIN_LEDGER_RUN_GAIN,
            Self::Project => MIN_LEDGER_RUN_GAIN * PROJECT_FLOOR_MULT,
        }
    }
}

/// A handle is always this long, so a marker's length is known before the run is
/// archived. `renders_a_marker_the_gain_test_can_predict` fails if it changes.
const HANDLE_LEN: usize = 16;

/// What a fold did to the numbers of the lines below it. Only the `Read` path
/// can act on it: the host numbers `file.content` from `startLine`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldShift {
    /// Nothing survives below the fold, so no number moves.
    None,
    /// The survivors are one block with folds above them; add `bump` to the host's
    /// starting number. It counts markers, not folded lines minus one: adjacent runs
    /// of different origin emit one marker each.
    Leading { bump: usize },
    /// Content above and below the fold: no single starting number fits.
    Interior,
}

impl FoldShift {
    /// Whether the survivors form one block. `markers_above` comes from
    /// `substitute`, the only place that knows; three attempts to infer it from the
    /// output were wrong (#573).
    fn of(folded: &HashSet<usize>, total: usize, markers_above: usize) -> Self {
        let survivors: Vec<usize> = (0..total).filter(|i| !folded.contains(i)).collect();
        let (Some(&first), Some(&last)) = (survivors.first(), survivors.last()) else {
            return Self::None;
        };
        if last - first + 1 != survivors.len() {
            return Self::Interior;
        }
        if first == 0 {
            // Survivors start where the payload does, so their numbers are already right.
            return Self::None;
        }
        // A fold below the survivors does not move them.
        Self::Leading {
            bump: first.saturating_sub(markers_above),
        }
    }
}

/// The scope for one reader. A subagent carries the parent's `session_id`, so
/// keying on the session alone told it the parent's lines were already shown
/// (#581). The main agent has no `agent_id` and keeps the bare session.
/// Compaction is not covered: the id stays and the context is gone.
pub fn scope_for(session: &str, agent: Option<&str>) -> String {
    match agent {
        Some(a) if !a.is_empty() => format!("{session}/{a}"),
        _ => session.to_string(),
    }
}

/// The session half of a scope key, the inverse of `scope_for`. Agent ids carry
/// no `/`.
pub fn session_of(scope: &str) -> &str {
    scope.split('/').next().unwrap_or(scope)
}

/// Addresses the ledger for one session, and optionally for its project.
pub struct Ledger<'a> {
    store: &'a Store,
    scope: String,
    project: Option<String>,
    /// The command being answered, so a fold can name a different source (#622).
    /// Empty suppresses the clause.
    source: String,
    /// The tool whose reply this is. Decides whether the host renumbers (#657),
    /// whether the caller's pattern chose the lines (#814), and is recorded per fold
    /// (#822).
    tool: String,
    /// Whether the caller read a window of its source (#796).
    windowed: bool,
    /// Who is being shown these lines. Recorded for measurement only (#509).
    agent: String,
}

/// Where a run was seen. `source` is set only when it differs from the command
/// being answered, the case a reader cannot resolve alone (#622). Part of the
/// grouping key, so lines from two commands split into two runs.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Seen {
    origin: Origin,
    source: SourceOfSighting,
}

/// Which command showed these lines, relative to the one being answered. Not an
/// `Option`: a verbatim re-run needs its own wording (#755).
#[derive(Clone, PartialEq, Eq, Debug)]
enum SourceOfSighting {
    Unrecorded,
    /// A different command, named in the marker (#622).
    Another(String),
    /// This same command, run again with the same lines (#755).
    ThisCommandAgain,
}

/// What one substitution produced.
struct Substitution {
    view: String,
    folded: HashSet<usize>,
    /// Markers above the first surviving line.
    markers_above: usize,
    padded: bool,
    /// One row per marker emitted, for the books (#766).
    emitted: Vec<crate::store::sqlite::MarkerRecord>,
}

/// One stretch of output, and where it was seen before if it was.
struct Run {
    start: usize,
    end: usize,
    seen: Option<Seen>,
}

/// One folded line, so the view keeps the payload's line count and every
/// survivor its own number; that is what makes an interior fold legal (#664).
/// `⋮` because editors already read it as elided content.
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

    pub fn with_project(mut self, project: impl Into<String>) -> Self {
        self.project = Some(project.into());
        self
    }

    pub fn from(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }

    /// `Read` is renumbered by the host, so a split view is never built (#657).
    /// `Grep` lines were chosen by the caller's pattern (#814).
    pub fn tool(mut self, name: &str) -> Self {
        self.tool = name.to_string();
        self
    }

    fn renumbers(&self) -> bool {
        self.tool == "Read"
    }

    fn filters_its_reply(&self) -> bool {
        self.tool == "Grep"
    }

    pub fn windowed(mut self, yes: bool) -> Self {
        self.windowed = yes;
        self
    }

    /// The agent these lines go to, resolved by the caller: the environment can
    /// say `claude_code` for a Codex payload.
    pub fn by(mut self, agent: impl Into<String>) -> Self {
        self.agent = agent.into();
        self
    }

    /// The view of `text` this session has earned, or `None` to leave the bytes
    /// alone. Lines are recorded either way: worth remembering because they may
    /// repeat, not because they compressed today.
    pub fn project(&self, text: &str) -> Option<String> {
        self.project_reporting_shift(text).map(|(view, _)| view)
    }

    /// The view, and what the fold did to the numbering below it (#557). Read
    /// from the folded indices, since a file may contain marker-like lines.
    pub fn project_reporting_shift(&self, text: &str) -> Option<(String, FoldShift)> {
        let shift = std::cell::Cell::new(FoldShift::None);
        // The gain gate wraps the projection; `MIN_LEDGER_INPUT` is stricter, so it decides.
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

        // `split_inclusive`, not `lines()`, so CRLF payloads come back byte-exact.
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        let hashes: Vec<String> = lines.iter().map(|l| line_key(l)).collect();

        // Session first: a line in both is one the reader holds. No age bound, since
        // the reader holds those bytes however old.
        let in_session = self.store.ledger_seen(&self.scope, &hashes, 0);
        // The project scope is age-bounded: a day-old sighting is a stale bet (#795).
        let fresh_enough =
            chrono::Utc::now().timestamp() - crate::guard::limits::PROJECT_SIGHTING_MAX_AGE_SECS;
        // A Read is the agent asking for a file it does not hold. A project fold
        // there hands a new subagent `⋮` for the lines it asked for, because a
        // sibling read them (#906). Bash keeps the project scope.
        let in_project = match &self.project {
            Some(p) if self.tool != "Read" => self.store.ledger_seen(p, &hashes, fresh_enough),
            _ => HashMap::new(),
        };
        // Never folded: a failure line, because a repeated error means the bug is
        // still there (#458); and a diff's changed lines, which are a new fact even
        // if shown before (#788), read past `--color` and `@@@`. Marking them unseen
        // splits the run so the rest still folds. Keyed by hash for the 10 ms budget.
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

        // Decided once, so the grouping key and the marker agree (#622).
        let differing = |seen: &crate::store::sqlite::SeenLine| {
            if seen.source.is_empty() {
                SourceOfSighting::Unrecorded
            } else if seen.source == self.source {
                SourceOfSighting::ThisCommandAgain
            } else {
                // Name only the command that wrote stdout: a quarter of clauses credited a
                // leading `cd` (#779). `None` for two producers, rather than picking one.
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

        // Project against earlier commands only, so the result is deterministic; a
        // repeat inside this payload is already in front of the agent.
        let projected = self
            .substitute(&lines, &hashes, &origin_of)
            .filter(|s| s.view.len() < text.len());

        // Decided before recording: a renumbering host drops a split view, and the
        // books must not carry a fold the agent never received (#657). A padded view
        // keeps every survivor's number (#664).
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

        // Record what the agent was handed, not what it was given (#465): a folded
        // run recorded here would later read as "already shown" and lower the bar.
        let delivered: Vec<String> = match &projected {
            Some(Substitution { folded, .. }) => hashes
                .iter()
                .enumerate()
                .filter(|(i, _)| !folded.contains(i))
                .map(|(_, h)| h.clone())
                .collect(),
            None => hashes.clone(),
        };
        // Tally what the folds drew on before the write below makes every line look
        // like this session's (#533).
        if let Some(Substitution {
            folded,
            emitted: markers,
            ..
        }) = &projected
        {
            // Every line folded: the agent holds markers only. Read per call rather than
            // per run, which is what this column asks.
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
                // Cannot happen via `ledger_record`; `unknown` beats understating the total.
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
            // Keyed on the project when there is one, so agents sharing a repo compare.
            let scope = self.project.as_deref().unwrap_or(&self.scope);
            // So the session travels separately.
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
        // Also recorded in the project scope, so later sessions can draw on it.
        if let Some(p) = &self.project {
            self.store
                .ledger_record(p, &delivered, &self.agent, &self.source);
        }

        projected.map(|s| (s.view, shifts))
    }

    /// The marker one run would be replaced by, rendered rather than estimated so
    /// the gain test weighs the string that is sent (#450). A run covering every
    /// line gets the longer whole-output wording (#519).
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

    /// The view, and the indices of the lines it replaced, so the caller records
    /// only what it delivered (#465) without re-deciding which runs folded.
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

        // Coverage is asked once for the whole payload (#601): per-run checks let
        // three 30% runs fold 90% of a reply. This also covers the whole-output floor.
        // Three quarters since #643: a reply whose 78.7% fold hid the entire answer
        // cleared four fifths, while the fixtures that must fold sit at 72.1% and
        // 63.9%. It cost 3,768 bytes over five days of the corpus.
        let seen_bytes: usize = runs
            .iter()
            .filter(|r| r.seen.is_some())
            .map(|r| lines[r.start..r.end].iter().map(|l| l.len()).sum::<usize>())
            .sum();
        if payload_bytes < MIN_WHOLE_OUTPUT_FOLD && seen_bytes * 4 >= payload_bytes * 3 {
            return None;
        }

        // Planned before emitting, so the survivors' shape is known. Only the store
        // can still turn a fold back into a verbatim run, which is the safe direction.
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

        // The project scope may fold part of a reply, never all of it: the reader
        // would hold one marker and nothing to check it against (#705). Unplanning
        // only the project runs keeps any honest session fold. Cost: 1.51% of folded
        // bytes.
        //
        // Also off when the command rationed its own output (`tail -5`): those lines
        // are the answer, not background, which the payload share cannot see in a
        // compound reply (#735). Narrower than `registry::passes_through_verbatim`,
        // because `cat` and `grep` are where the project scope earns its keep.
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

        // `grep` output was chosen by the caller's pattern, so a partial fold reads as
        // fewer matches (#814): whole or nothing. Only for folds drawing on another
        // command; a same-command re-run still folds, since there the identity is the
        // answer (#755).
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

        // A renumbering host cannot take survivors in two blocks, so each fold is
        // padded back to its own line count and every survivor keeps its number
        // (#664). The plan asks with zero markers; the real bump is counted below.
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
        // Markers above the first survivor, counted here because only here knows (#573).
        let mut markers_above = 0usize;
        // What was handed over, one row per marker, for which wording a reader met (#766).
        let mut emitted: Vec<crate::store::sqlite::MarkerRecord> = Vec::new();
        let mut still_above = true;
        let mut padded_any = false;
        for (i, run) in runs.iter().enumerate() {
            let body = lines[run.start..run.end].concat();
            // A run folds when it saves more than its rendered marker plus padding costs
            // (#450); padding is part of the price. The whole-output wording is longer
            // (#519), so it is decided before this test weighs it. Below
            // `MIN_WHOLE_OUTPUT_FOLD` a whole-output fold loses, since the agent must
            // retrieve to see anything (#543).
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
                    // Count the terminator the emit adds back, or a run folds one byte short (#664 review).
                    let marker = self
                        .marker_for(run, seen, lines.len(), &"0".repeat(HANDLE_LEN))
                        .0
                        .len()
                        + usize::from(body.ends_with('\n'));
                    body.len() >= marker + padding + seen.origin.min_gain()
                });

            // A handle only for content that is provably archived (#388). A run whose
            // handle was just pulled stays verbatim once, since that delivery answers the
            // pull (#581). The payload size goes with it, so `omni retrieve` never presents
            // one block as the whole reply (#627).
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
                    // The marker needs a newline only when the replaced text ended a line.
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

/// Consecutive lines that agree about where they were seen. Grouped by
/// `Option<Origin>` so a session run and a project run never share a marker.
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

/// Whether the command rationed its output to lines it named: `head`/`tail`
/// with a line count (a bare `head` included) or a `sed` line address. Not
/// `tail -f` or `head -c`.
///
/// Only in command position, so `grep -rn head src/` does not count (#738);
/// per segment, with newlines as separators (#750); `sed` ranges count, since
/// `sed -n 60,200p` names lines as `tail -5` does (#741). `awk` is left out:
/// `NR` also appears in `{print NR}`. Matched on the basename.
fn rations_its_output(command: &str) -> bool {
    command
        .split(['\n', ';', '|', '&'])
        .any(segment_rations_its_output)
}

/// The same question for one command segment. Under a wrapper (`docker exec`,
/// `ssh`) the name counts anywhere, erring toward keeping the named lines
/// (#751).
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
fn filters_its_own_output(command: &str) -> bool {
    command.split(['\n', ';', '|', '&']).any(|segment| {
        let tokens: Vec<&str> = segment.split_whitespace().collect();
        tokens.iter().enumerate().any(|(i, tok)| {
            opens_a_command(&tokens, i)
                && matches!(tok.rsplit('/').next().unwrap_or(tok), "grep" | "rg" | "ag")
        })
    })
}

/// Whether the token at `i` is the first word of its segment's command, past
/// `sudo`, `env` and `VAR=value` assignments.
fn opens_a_command(tokens: &[&str], i: usize) -> bool {
    tokens[..i].iter().all(|t| introduces_a_command(t))
}

/// A word that precedes a command without being the command.
fn introduces_a_command(token: &str) -> bool {
    matches!(
        token.rsplit('/').next().unwrap_or(token),
        // `command` runs the next word as a program, like `env` (#814 review).
        "sudo" | "env" | "time" | "nohup" | "command"
    ) || token.split_once('=').is_some_and(|(name, _)| {
        !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    })
}

/// A `sed` address naming line numbers: `5p`, `60,200p`. Pattern addresses and
/// substitutions filter, which is a different claim.
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

/// A line's identity in the ledger: trimmed, so `sed -n` and `cat` of a line
/// agree, and hashed to keep the index small.
pub fn line_key(line: &str) -> String {
    crate::util::text::short_hash(line.trim())
}

#[cfg(test)]
mod tests {
    /// The main agent's scope must stay the bare session id byte for byte, or an
    /// upgrade orphans the rows the previous binary wrote (#581).
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

    /// Sized against the constants, so the fixture reaches the logic, not an early return.
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

    /// A re-run of the same command states the identity (#755); a different
    /// command still names itself (#622).
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

        // Names the stage that wrote the bytes, not the loop header (#779).
        assert!(
            elsewhere.contains("already shown") && elsewhere.contains("from grep ^is_read_only"),
            "a different command still has to name the source it is standing in \
             for (#622): {elsewhere}"
        );
    }

    /// The recorded kind comes from the arm that renders the wording, so the pair
    /// cannot drift (#766).
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

        // An earlier draft passed with the handle column blank.
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

        // A handle pulled twice must not multiply its own marker through the join (#771).
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

    /// The handle carries the size of the payload it came from (#627).
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

    /// The marker's count must be the payload's own line count (#519).
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

    /// A partial fold keeps the run wording.
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

    /// The whole-output flag must separate the two shapes, or it answers nothing
    /// (#543).
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

        // One unseen line: the head folds, the tail stays.
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
        // Same second, so size must be readable per row.
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

    /// 341 bytes: above `MIN_LEDGER_INPUT` and the session gain, below
    /// `MIN_WHOLE_OUTPUT_FOLD`.
    fn under_the_whole_output_floor() -> String {
        (0..6)
            .map(|i| format!("2026-08-10T00:00:{i:02}Z  handler finished request {i} in 12ms"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// A repeated block and a fresh one, the only shape a project fold may take
    /// (#705). The repeated half clears the project scope's higher bar.
    fn project_repeat_then_fresh() -> (String, String) {
        let repeated = project_repeat();
        let payload = format!("{repeated}{}", fresh_block("cache probe"));
        (repeated, payload)
    }

    /// Primes a first session; six times the session floor.
    fn project_repeat() -> String {
        (0..200)
            .map(|i| format!("2026-08-10T00:00:00Z  handler finished request {i} in 12ms\n"))
            .collect()
    }

    /// Runs git in `dir` with every inherited `GIT_*` removed: `GIT_DIR` alone
    /// points the test at another repository (PR #754 review).
    fn git(dir: &std::path::Path, args: &[&str]) {
        let mut cmd = std::process::Command::new("git");
        cmd.current_dir(dir).args(args);
        for (name, _) in std::env::vars_os() {
            // Uppercased: Windows env names are case-insensitive to git.
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

    fn fresh_block(tag: &str) -> String {
        (0..20)
            .map(|i| format!("2026-08-10T00:00:00Z  {tag} {i} missed and refilled\n"))
            .collect()
    }

    /// Under 1 KB a whole-output fold stays verbatim (#543); the partial arm
    /// checks small payloads can still fold.
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

        // Four new lines keep this under the three-quarter coverage floor (#601).
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

    /// A handle must resolve to the exact bytes it replaced.
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

    /// A nondeterministic view breaks the upstream prompt cache.
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

    /// Prompt caching is prefix-matched, so the same history must replay to the
    /// same bytes (#610). Asserts a fold happened first, since three passthroughs
    /// would also compare equal.
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

    /// One session's history is never another's.
    #[test]
    fn never_claims_another_sessions_history() {
        let (store, _d) = temp_store();
        let text = payload();
        Ledger::new(&store, "s1").project(&text);

        assert_eq!(Ledger::new(&store, "s2").project(&text), None);
    }

    /// `INSERT OR IGNORE` keeps the first agent shown a line, so "a different agent
    /// was shown this" stays answerable (#509).
    #[test]
    fn records_which_agent_was_shown_each_line() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("omni.db");
        let store = Store::open_path(&db).expect("store");
        // Below the project fold bar, so the second agent is shown every line and
        // writes; a folding fixture passes with `INSERT OR REPLACE` too.
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

    /// Records whose bytes a cross-agent fold replaced, the row the shared-scope
    /// decision needs (#533).
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

    /// A project fold never takes the whole reply, for a subagent or a second
    /// session alike (#705). The partial arm is asserted too: that is where the
    /// project scope earns its bytes.
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

        // With fresh lines around it, the same block must still fold.
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

    /// A Read never folds from the project scope (#906); Bash still does, so this
    /// pins the refusal and not the scope going quiet.
    #[test]
    fn a_read_never_folds_from_the_project_scope() {
        let (store, _d) = temp_store();
        let repeated = project_repeat();
        let payload = format!("{repeated}{}", fresh_block("cache probe"));
        Ledger::new(&store, scope_for("s1", Some("sibling")))
            .with_project("/repo")
            .tool("Read")
            .project(&repeated);

        let read = Ledger::new(&store, scope_for("s1", Some("fresh")))
            .with_project("/repo")
            .tool("Read")
            .project(&payload);
        assert_eq!(
            read, None,
            "a fresh reader's Read was folded from the project scope"
        );

        let bash = Ledger::new(&store, "s2")
            .with_project("/repo")
            .tool("Bash")
            .project(&format!("{repeated}{}", fresh_block("bash probe")))
            .expect("Bash still folds from the project scope");
        assert!(bash.contains("not shown here"), "{bash}");
    }

    /// The project marker must not claim the lines were shown; the fixture carries
    /// fresh lines since #705.
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

    /// A rationed command (`tail -5`) is never project-folded, even when the payload
    /// share stays low in a compound reply (#735). Unbudgeted commands must still
    /// fold. Each arm has its own fresh block: sharing one hands the refusal to the
    /// #705 guard and the test passes with this guard deleted.
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

        // Only the command differs.
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

    /// A stale project sighting is refused (#795). One store, so age is the only
    /// variable.
    #[test]
    fn a_project_fold_needs_a_fresh_sighting() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("omni.db");
        let store = Store::open_path(&db).expect("store");
        let repeated = project_repeat();

        // A scope per arm with its own writer and fresh block; sharing either makes
        // another guard decide and the test passes with the bound removed.
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

    /// A repeat sighting refreshes the timestamp, so a line shown again now is
    /// fresh (PR #825 review). Only `ts` moves: attribution stays (#622).
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

        // Stale: refused, delivered verbatim, and that delivery is the repeat sighting.
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

    /// A `grep` reply is never partly folded against other output (#814); an
    /// identical re-run still folds (#755).
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

        // `command grep` is the same filter.
        assert!(filters_its_own_output("command grep -rn handler src/"));
        assert!(filters_its_own_output("sudo rg --hidden pattern ."));
        assert!(!filters_its_own_output("cat notes.md"));

        // The reported shape: another command printed the lines first.
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

        // Same command, same output: the identity is the answer (#755).
        Ledger::new(&store, "s2").from(grep).project(&shown);
        let again = Ledger::new(&store, "s2")
            .from(grep)
            .project(&format!("{}{shown}", fresh_block("new match")))
            .expect("a re-run of the same command still folds");
        assert!(again.contains("identical to an earlier run"), "{again}");
    }

    /// A `Read` window lives in `offset`/`limit`, not the path, so only the flag
    /// separates these arms (#796).
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

    /// A diff's changed lines are never folded; without a hunk header they are
    /// (#788).
    #[test]
    fn a_diff_keeps_its_changed_lines() {
        let (store, _d) = temp_store();
        let line = |i: usize| format!("    let value_{i} = compute({i});");
        // Plain, coloured and combined.
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

        let added: String = (0..30).map(|i| format!("+{}\n", line(i))).collect();
        Ledger::new(&store, "flat").project(&added);
        let folded = Ledger::new(&store, "flat")
            .project(&format!("{added}{}", fresh_block("flat")))
            .expect("the same lines without a hunk header fold");
        assert!(folded.contains("already shown"), "{folded}");
    }

    /// A line budget counts; a byte budget, a follow, or a word merely containing
    /// a name does not.
    #[test]
    fn rations_its_output_reads_a_line_budget_and_nothing_else() {
        for cmd in [
            "tail -5 notes.md",
            "tail -n 5 notes.md",
            "cargo test | tail -30",
            "head -20 src/main.rs",
            "head notes.md",
            "/usr/bin/tail -1 notes.md",
            "cat notes.md; echo ---; tail -5 handlers.log",
            "cat a.md && head -3 b.md",
            // #741, unquoted and quoted.
            "sed -n 60,200p app/run-panel.tsx",
            "sed -n '285,340p' src/lib.rs",
            "sed -n 5p notes.md",
            // #750: a newline separates commands.
            "cd /path/to/repo\nsed -n '58,95p' src/app.tsx",
            "cd /path/to/repo\ntail -20 /tmp/dev.log",
            "cat a.md;tail -5 b.md",
            // #751: words that introduce a command.
            "sudo tail -20 /var/log/app.log",
            "env OMNI_PASSTHROUGH=1 sed -n '335,360p' src/hooks/post_tool.rs",
            // #751: inside a wrapper the name counts anywhere.
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
            // #738: somebody else's argument.
            "grep tail notes.md",
            "grep -rn head src/",
            "ls | grep tail",
            "cat tail",
            "sed -n '/failed/p' build.log",
            "sed 's/a/b/' notes.md",
            // `awk` is uncovered on purpose.
            "awk 'NR>=60 && NR<=200' src/lib.rs",
            // #751: a later command's `5p` is not this one's address.
            "sed -n '/failed/p' build.log; echo 5p",
            "cd /path/to/repo\ngrep -rn head src/",
        ] {
            assert!(!rations_its_output(cmd), "should not ration: {cmd}");
        }
    }

    /// The project key must not move with the branch or commit: that is the saving
    /// the host's prompt cache cannot make (#739). Uses real git, so a key that
    /// shelled out to `git rev-parse` would fail.
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

    /// The project run marker must say the lines were not shown here (#567).
    #[test]
    fn a_project_run_marker_says_the_lines_were_not_shown_here() {
        let (store, _d) = temp_store();
        // Repeated then unseen, so the fold is a run, not the whole output.
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

    /// A marker is not a sighting (#465): the second occurrence in a session that
    /// was only handed a marker is still a project repeat.
    #[test]
    fn a_folded_run_is_not_recorded_as_shown() {
        let (store, _d) = temp_store();
        let repeated = project_repeat();
        Ledger::new(&store, "s1")
            .with_project("/repo")
            .project(&repeated);

        // Own fresh tail per call, or #705 decides instead of the claim under test.
        let call = |tag: &str| format!("{repeated}{}", fresh_block(tag));

        let first = Ledger::new(&store, "s2")
            .with_project("/repo")
            .project(&call("cache probe"))
            .expect("a project repeat above the floor is projectable");
        assert!(first.contains("not shown here"), "{first}");

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

    /// A multi-line source must not split the marker into several lines (#625,
    /// #573).
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

    /// The clause appears when the source differs and is absent when it does not
    /// (#622): it costs bytes on every same-file re-read.
    #[test]
    fn a_fold_says_when_it_is_replacing_another_source() {
        let (store, _d) = temp_store();
        // A shared block among unique lines; alone it would fold whole and hit the floor.
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

        // A fresh scope, sized past `MIN_WHOLE_OUTPUT_FOLD`.
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

    /// The clause names the segment that printed, never a leading `cd` or loop
    /// header; with two producers it names none (#779).
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

        // Two producers: nothing honest to name. A fresh scope.
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

        // One producer inside a loop keeps its `do` keyword.
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

    /// A run between the session and project bars folds only in the session scope.
    #[test]
    fn holds_a_project_repeat_to_its_own_higher_floor() {
        let (store, _d) = temp_store();
        // Sized between the two bars; `payload()` clears both.
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

        // Four unseen lines keep these as runs, not whole outputs, so the #543 and
        // #601 floors stay out of it.
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

        assert!(Ledger::new(&store, "s1").project(&probe).is_some());
        assert_eq!(
            Ledger::new(&store, "s2")
                .with_project("/repo")
                .project(&probe),
            None
        );
    }

    /// A line in both histories belongs to the session, the free claim.
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

    /// Untouched lines come back byte for byte, CRLF included.
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

    #[test]
    fn leaves_a_short_repeated_run_in_place() {
        let (store, _d) = temp_store();
        let shared = "the shared preamble line, over twelve characters";
        let first = format!("{shared}\n{}", payload());
        let ledger = Ledger::new(&store, "s1");
        ledger.project(&first);

        let second = format!(
            "{shared}\n{}",
            (0..60)
                .map(|i| format!("a completely different line number {i} of this second payload"))
                .collect::<Vec<_>>()
                .join("\n")
        );

        assert_eq!(ledger.project(&second), None);
    }

    /// A repeated failure line survives a fold (#458).
    #[test]
    fn never_folds_a_line_that_states_a_failure() {
        let (store, _d) = temp_store();
        let payload = "19 |   \"x-csrf-token\": csrf,\n                       20 |   Cookie: cookies,\n                       21 | };\n                       22 | \n                       23 | const profiles = await (await fetch(BASE)).json();\n                       24 | const profileId = profiles.profiles[0].id;\n                       TypeError: undefined is not an object (evaluating 'profiles.profiles[0]')\n                             at /tmp/repro237.ts:24:28\n                       Bun v1.3.14 (macOS arm64)\n";

        // Big enough to fold at all since #601.
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

    /// Three long lines pay for their marker; the old line-count bound refused them.
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

    /// The gain test assumes the real handle is `HANDLE_LEN` long.
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

    /// One 34 byte line, so fixture sizes can be reasoned about.
    fn row(tag: char, i: usize) -> String {
        format!("{tag} line {i:04} of the fixture payload\n")
    }

    /// Three seen blocks of about 30% each fold 90% of a reply if coverage is
    /// weighed per run; it is weighed per payload (#601 review).
    #[test]
    fn counts_coverage_over_the_payload_not_over_one_run() {
        let (store, _d) = temp_store();
        let ledger = Ledger::new(&store, "s1");

        // Eight lines, so each block clears the per-run gain alone.
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

    /// The reported row: 75.8% coverage, under 1 KB, leaving only shell
    /// scaffolding (#643). Real numbers, because this band is the one in question.
    #[test]
    fn refuses_a_fold_that_leaves_only_scaffolding() {
        let (store, _d) = temp_store();
        let ledger = Ledger::new(&store, "s1").with_project("/repo");

        let answer: String = (1..=14)
            .map(|i| format!("{i:02}:export * from \"./module-name-{i:02}\";\n"))
            .collect();
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
        // Between three quarters and four fifths, or it proves nothing.
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

    /// The coverage floor applies only at three quarters or more and under
    /// `MIN_WHOLE_OUTPUT_FOLD`; the other rows must still fold (#601).
    #[test]
    fn refuses_a_small_fold_that_would_leave_almost_nothing() {
        // (name, lines reused from the first show, lines new to the second, folds?)
        let cases = [
            // The reported shape: 23 of 26 lines folded.
            ("small payload, 88% covered", 23, 3, false),
            // A third folded: enough is left to work from.
            ("small payload, 38% covered", 10, 16, true),
            // Same coverage past the floor: the case the feature is for.
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
