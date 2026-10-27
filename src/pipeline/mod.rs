pub mod analyzer;
pub mod collapse;
pub mod fidelity;
pub mod format;
pub mod gate;
pub mod producer;
pub mod registry;
pub mod scorer;
pub mod semantic;

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::fmt;

/// Maximum number of recent errors to track in session context
pub const MAX_ACTIVE_ERRORS: usize = 5;

/// Maximum number of recent commands to remember in session
pub const MAX_COMMAND_HISTORY: usize = 20;

/// Maximum number of significant distillations to track per session
pub const MAX_DISTILLATION_HISTORY: usize = 5;

/// Threshold for meaningful compression (e.g., 0.90 means at least 10% savings)
pub const MEANINGFUL_COMPRESSION_THRESHOLD: f64 = 0.90;

/// Default context window size hint (tokens). Configurable via VENG_CONTEXT_WINDOW env.
pub const DEFAULT_CONTEXT_WINDOW_SIZE: u64 = 200_000;

/// Threshold ratio at which pressure becomes Warning (default 0.65)
pub const DEFAULT_PRESSURE_WARNING_THRESHOLD: f64 = 0.65;

/// Threshold ratio at which pressure becomes Critical (default 0.82)
pub const DEFAULT_PRESSURE_CRITICAL_THRESHOLD: f64 = 0.82;

/// Minimum tool calls between repeated pressure warnings
pub const PRESSURE_WARNING_COOLDOWN: u32 = 5;

/// How long after a distillation a repeat of the same command still counts as a
/// re-run caused by it (#109). Wide enough to cover an agent reading the output
/// and reacting, narrow enough that unrelated later work is not blamed on it.
pub const RERUN_WINDOW_SECS: i64 = 300;

/// Minimum rows required on *each* arm before a filter's re-run delta is
/// reported. Below this the delta is noise, and publishing it would be the
/// confident-but-unsupported number VENG exists to stop emitting.
pub const RERUN_MIN_SAMPLES: u64 = 8;

/// Commit timestamp of the #158 fix, `fix(hooks): emit the key Claude Code
/// actually reads (#162)`, 2026-07-22 21:47:55 +0700.
///
/// Before it, the Claude Code post-hook emitted its output under a key the host
/// ignores, so `Keep` rows on that path were recorded as distilled while the
/// agent read the raw bytes. They are controls mislabelled as treatment, and
/// including them does not merely add noise, on the maintainer's store `grep`
/// reads +9.2pp with them excluded and +0.0pp with them included, so dilution
/// alone can hide a real finding. Any before/after comparison must drop them.
pub const POST_HOOK_FIX_TS: i64 = 1_784_731_675;

/// How far the two arms' mean input size may diverge before the comparison is
/// flagged as confounded.
///
/// Distillation only fires on large output, so "distilled" and "passed through"
/// are not always the same population. On the maintainer's store `kubectl`
/// averaged 244,606 B distilled against 115 B raw, a 2,127× skew that made a
/// +48.6pp delta look like evidence when it compared `get -A` dumps against
/// one-line config reads. `grep` (3,196 B vs 3,240 B) and `npm` (11,419 B vs
/// 14,065 B) are matched and survive. 3× separates the two cases cleanly.
pub const RERUN_SIZE_SKEW_LIMIT: f64 = 3.0;

// 1. Segmentation Strategy, how to split tokens
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SegmentationMode {
    Line,      // Default: line by line
    GitHunk,   // Git diff format: split by @@ or diff lines
    TestGroup, // Test runners: group test cases
}

// 2. Collapse Strategy, how to group repetitive lines
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CollapseMode {
    Generic,
    Build,
    Test,
    Infra,
    Log,
}

// 2. Signal tier, how important this segment is
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SignalTier {
    Noise,     // Progress, compiling boring deps, drop
    Context,   // Supporting lines, include if space allows
    Important, // Warning, changed file, biasanya include
    Critical,  // Error, exception, FAILED, selalu include
}

// Loop Context Mode
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub enum LoopMode {
    #[default]
    Interactive, // Normal session
    OuterLoop, // Berjalan di dalam outer loop (scheduled/automated)
    SubAgent,  // Berjalan sebagai sub-agent (checker, verifier)
    Benchmark, // Running benchmark/eval loop
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LoopContext {
    pub mode: LoopMode,
    pub loop_id: Option<String>,    // UUID dari outer loop runner
    pub iteration: u32,             // Iterasi loop ke-berapa
    pub budget_tokens: Option<u64>, // Token budget per iteration
    pub budget_used: u64,           // Token yang sudah digunakan
    pub goal: Option<String>,       // Goal string dari outer loop
    pub verifier_required: bool,    // Apakah output perlu di-verify oleh checker agent
}

impl LoopContext {
    pub fn update_from_env(&mut self) {
        let mut new_loop_id = None;
        let mut new_goal = None;
        let mut new_budget = self.budget_tokens;

        if let Ok(val) = std::env::var("VENG_LOOP_ID") {
            new_loop_id = Some(val);
        }
        if let Ok(val) = std::env::var("VENG_LOOP_GOAL") {
            new_goal = Some(val);
        }
        if let Ok(val) = std::env::var("VENG_LOOP_BUDGET") {
            new_budget = val.parse().ok().or(self.budget_tokens);
        }

        // Validate security constraints
        if let Err(e) = crate::guard::env::validate_loop_context(
            new_loop_id.as_deref(),
            new_goal.as_deref(),
            new_budget,
        ) {
            eprintln!(
                "[veng:security] Loop context validation failed: {:?}. Ignoring inputs.",
                e
            );
        } else {
            if let Some(id) = new_loop_id {
                self.mode = LoopMode::OuterLoop;
                self.loop_id = Some(id);
            }
            if let Some(goal) = new_goal {
                self.goal = Some(goal);
            }
            self.budget_tokens = new_budget;
        }

        if let Ok(val) = std::env::var("VENG_LOOP_ITERATION")
            && let Ok(new_iter) = val.parse::<u32>()
            && new_iter != self.iteration
        {
            self.iteration = new_iter;
            self.budget_used = 0; // Reset budget per iteration
        }

        if let Ok(val) = std::env::var("VENG_SUBAGENT")
            && val == "1"
        {
            self.mode = LoopMode::SubAgent;
        }
    }
}

// Context window pressure level
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ContextPressure {
    #[default]
    Normal, // < warning threshold
    Warning,  // warning..critical threshold
    Critical, // > critical threshold
}

impl std::fmt::Display for ContextPressure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContextPressure::Normal => write!(f, "Normal"),
            ContextPressure::Warning => write!(f, "Warning"),
            ContextPressure::Critical => write!(f, "Critical"),
        }
    }
}

// 3. Route, path distilasi
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Route {
    Keep,        // score >= 0.7, full distillation
    Soft,        // 0.3-0.69, labeled distillation
    Passthrough, // < 0.3, raw + learn trigger
    Rewind,      // aggressively compressed, stored in RewindStore
    Error,       // engine error, raw preserved
}

impl fmt::Display for Route {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Route::Keep => write!(f, "Keep"),
            Route::Soft => write!(f, "Soft"),
            Route::Passthrough => write!(f, "Passthrough"),
            Route::Rewind => write!(f, "Rewind"),
            Route::Error => write!(f, "Error"),
        }
    }
}

// Implement Display for SignalTier (optional but useful for logging)
impl fmt::Display for SignalTier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

// 4. Output segment
#[derive(Debug, Clone)]
pub struct OutputSegment {
    pub content: String,
    pub tier: SignalTier,
    pub base_score: f32,
    pub context_score: f32, // boost for session context
    pub line_range: (usize, usize),
}

impl OutputSegment {
    pub fn final_score(&self) -> f32 {
        (self.base_score + self.context_score).clamp(0.0, 1.0)
    }

    pub fn mentions(&self, path: &str) -> bool {
        self.content.contains(path)
    }

    pub fn is_diagnostic(&self) -> bool {
        matches!(self.tier, SignalTier::Critical | SignalTier::Important)
    }
}

impl From<semantic::SemanticBlock> for OutputSegment {
    fn from(block: semantic::SemanticBlock) -> Self {
        let tier = match block.class {
            semantic::SemanticClass::Critical => SignalTier::Critical,
            semantic::SemanticClass::Diagnostic => SignalTier::Important,
            semantic::SemanticClass::Context => SignalTier::Context,
            semantic::SemanticClass::Progress => SignalTier::Noise,
            semantic::SemanticClass::Noise => SignalTier::Noise,
            semantic::SemanticClass::Data => SignalTier::Context,
        };

        Self {
            content: block.lines.join("\n"),
            tier,
            base_score: block.score,
            context_score: 0.0,
            line_range: block.line_range,
        }
    }
}

// 5. Distillation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistillResult {
    pub output: String,
    pub route: Route,
    pub filter_name: String,
    pub score: f32,
    pub context_score: f32, // for session scorer
    pub input_bytes: usize,
    pub output_bytes: usize,
    pub latency_ms: u64,
    pub rewind_hash: Option<String>, // if content is in RewindStore
    pub segments_kept: usize,
    pub segments_dropped: usize,
    pub collapse_savings: Option<(usize, usize)>, // (original_lines, collapsed_to)
    pub raw_tokens: usize,
    pub filtered_tokens: usize,
    /// Bytes that reach a model's context, as against `output_bytes`, which is
    /// only what the distiller returned.
    ///
    /// The two are not the same thing and the difference is what made the
    /// headline untrue: on Claude Code's large-output path the host discarded
    /// VENG's result and persisted the raw output, while `output_bytes` recorded
    /// a 93% saving for it (#212). It is 0 where no model reads the path at all -
    /// `veng exec` and the shell pipe write to a TTY, and those rows were 73% of
    /// every byte VENG claimed to have saved, in a unit where "tokens" means
    /// nothing.
    ///
    /// **This is derived, not acknowledged.** No host tells a hook whether it
    /// applied the replacement, so this records what VENG handed over. With the
    /// host-cap path detected above, that is the same value on every route this
    /// table books, which is the point: the number now comes from what was
    /// delivered rather than from what a distiller intended.
    pub delivered_bytes: usize,
}

/// What a re-sent tool result is billed at, as a fraction of fresh input.
///
/// A tool result is not read once. It enters the transcript and is re-sent on
/// every later turn of the session, so distilling 20 KB to 2 KB at turn 5 of a
/// 40-turn session saves those bytes on turn 5 and again on every turn after it.
/// VENG counted the first and discarded the rest (#173).
///
/// The multiplier is not full price, and publishing it as though it were would
/// be the bigger-but-not-truer number this tracker exists to fight. With prompt
/// caching the re-sent prefix is billed at the cache-read rate, roughly a tenth
/// of fresh input, so the compounding is real and discounted.
///
/// **This is an assumption, not an observation.** A hook cannot see whether a
/// turn was a cache hit, so anything derived from it is labelled `est.` and
/// prints this rate beside it. An unlabelled assumption here would recreate the
/// `est_cost_usd` figure that 0.6.2 had no data to compute.
pub const CACHE_READ_RATE: f64 = 0.10;

impl DistillResult {
    pub fn savings_pct(&self) -> f64 {
        if self.input_bytes == 0 {
            return 0.0;
        }
        (1.0 - self.output_bytes as f64 / self.input_bytes as f64) * 100.0
    }

    pub fn is_meaningful(&self) -> bool {
        // Return false if there is no significant compression (e.g., < 10%)
        self.output_bytes < (self.input_bytes as f64 * MEANINGFUL_COMPRESSION_THRESHOLD) as usize
    }
}

// 6. Session state (minimal for v0.5.0)
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionState {
    pub session_id: String,
    pub started_at: i64,
    pub last_active: i64,

    // Inferred context
    pub inferred_task: Option<String>,   // "fix auth bug"
    pub inferred_domain: Option<String>, // "authentication"

    // Hot files (path → access count)
    pub hot_files: BTreeMap<String, u32>,

    // Recent errors to boost relevance
    pub active_errors: Vec<String>, // last MAX_ACTIVE_ERRORS error messages

    // Command history
    pub command_count: u32,
    pub last_commands: Vec<String>, // last MAX_COMMAND_HISTORY commands

    // Distillation Telemetry
    pub last_significant_distillations: VecDeque<DistillSummary>, // max MAX_DISTILLATION_HISTORY entries
    pub cumulative_input_bytes: u64,
    pub cumulative_output_bytes: u64,
    pub cumulative_raw_tokens: u64,
    pub cumulative_filtered_tokens: u64,
    pub top_command_info: Option<(String, f32)>, // (command, savings_pct)
    pub toolchain_hints: std::collections::HashMap<String, String>,

    // Context Pressure (v0.5.8-rc3)
    #[serde(default)]
    pub context_window_size_hint: Option<u64>,
    #[serde(default)]
    pub estimated_current_tokens: u64,
    #[serde(default)]
    pub context_pressure: ContextPressure,
    #[serde(default)]
    pub last_pressure_warning_at: Option<u32>,

    // Critical file pinning (v0.5.8-rc3)
    #[serde(default)]
    pub pinned_files: Vec<String>,
    #[serde(default)]
    pub pinned_refresh_count: u32,

    // Context Composition (Phase 1)
    #[serde(default)]
    pub current_turn: crate::analytics::context_composition::ContextTurn,

    // Phase 2: Engram: Automatic Subtask Digest
    #[serde(default)]
    pub engrams: VecDeque<crate::session::engram::Engram>,

    // Phase 2: Rolling Tool Call Log (last 50 calls)
    #[serde(default)]
    pub tool_call_log: VecDeque<crate::session::engram::ToolCallEntry>,

    // Phase 2: Last command_count when pinned files were re-injected
    #[serde(default)]
    pub pinned_reinject_at: u32,

    // Phase 2: Hash of last compact snapshot for delta detection
    #[serde(default)]
    pub last_compact_hash: Option<String>,

    // Loop Context (Phase 3 / L1-01)
    #[serde(default)]
    pub loop_context: LoopContext,

    // L3-02: Predictive Context Pressure Warning
    #[serde(default)]
    pub token_consumption_rate: TokenConsumptionRate,
}

// ─── L3-02: Predictive Context Pressure ──────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenConsumptionRate {
    pub samples: VecDeque<(u32, u64)>, // (command_count, tokens_at_time)
    pub avg_tokens_per_command: f64,
}

impl TokenConsumptionRate {
    pub fn update(&mut self, command_count: u32, current_tokens: u64) {
        self.samples.push_back((command_count, current_tokens));
        if self.samples.len() > 10 {
            self.samples.pop_front();
        }

        if self.samples.len() >= 2 {
            let first = self.samples.front().unwrap();
            let last = self.samples.back().unwrap();
            let cmd_diff = last.0.saturating_sub(first.0);
            if cmd_diff > 0 {
                let token_diff = last.1.saturating_sub(first.1);
                self.avg_tokens_per_command = token_diff as f64 / cmd_diff as f64;
            }
        }
    }

    pub fn predicted_full_at_command(&self, current_tokens: u64, window: u64) -> Option<u32> {
        if self.avg_tokens_per_command <= 0.0 {
            return None;
        }
        let remaining = window.saturating_sub(current_tokens) as f64;
        let commands_left = (remaining / self.avg_tokens_per_command) as u32;
        Some(commands_left)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistillSummary {
    pub command: String,
    pub route: Route,
    pub input_bytes: usize,
    pub output_bytes: usize,
    pub timestamp: i64,
}

impl SessionState {
    /// A session id that identifies a session.
    ///
    /// It was `timestamp_millis()` alone, so two sessions created in the same
    /// millisecond were the same session, and `INSERT OR REPLACE INTO sessions`
    /// kept one of them (#490). That is not hypothetical on a machine running
    /// several agents: two `SessionStart` hooks landing together is exactly when
    /// it fires.
    ///
    /// The millisecond stays first because four call sites slice
    /// `session_id[..8]` for display, and it keeps ids roughly sortable by age.
    /// The process id separates concurrent hooks, which are separate processes,
    /// and the counter separates sessions minted inside one process. Both are
    /// stdlib; a uuid crate for this would be a dependency to avoid a format
    /// string.
    ///
    /// This does not make the id meaningful. It is still minted state rather than
    /// an identity the host supplied, which is why the ledger scopes on
    /// `host_session()` instead. The fuller fix is to stop minting where a host
    /// id exists, and it is not this one.
    pub fn new() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static MINTED: AtomicU64 = AtomicU64::new(0);
        let id = format!(
            "{}-{}-{}",
            chrono::Utc::now().timestamp_millis(),
            std::process::id(),
            MINTED.fetch_add(1, Ordering::Relaxed)
        );
        let now = chrono::Utc::now().timestamp();
        let mut state = Self {
            session_id: id,
            started_at: now,
            last_active: now,
            last_significant_distillations: VecDeque::with_capacity(MAX_DISTILLATION_HISTORY),
            ..Default::default()
        };
        state.loop_context.update_from_env();
        state
    }

    pub fn actual_tokens_saved(&self) -> u64 {
        self.cumulative_raw_tokens
            .saturating_sub(self.cumulative_filtered_tokens)
    }

    pub fn estimated_tokens_saved(&self) -> u64 {
        if self.cumulative_input_bytes > self.cumulative_output_bytes {
            crate::util::token_estimate::estimate_tokens(
                (self.cumulative_input_bytes - self.cumulative_output_bytes) as usize,
                crate::util::token_estimate::ContentHint::Mixed,
            ) as u64
        } else {
            0
        }
    }

    pub fn top_command(&self) -> Option<(String, f32)> {
        self.top_command_info.clone()
    }

    // Score boost from session context for a text
    pub fn context_boost(&self, text: &str) -> f32 {
        let mut boost = 0.0f32;
        // Boost if mentioning hot file
        for (path, count) in &self.hot_files {
            if text.contains(path) {
                boost += 0.1_f32 * ((*count as f32 / 10.0_f32).min(0.3_f32));
            }
        }
        // Boost if mentioning active error
        for err in &self.active_errors {
            let err_short = crate::util::text::safe_slice(err, 30);
            if text.contains(err_short) {
                boost += 0.25;
            }
        }
        boost.min(0.4)
    }

    pub fn add_hot_file(&mut self, path: &str) {
        *self.hot_files.entry(path.to_string()).or_insert(0) += 1;
    }

    pub fn add_error(&mut self, error: &str) {
        self.active_errors
            .insert(0, crate::util::text::safe_slice(error, 200).to_string());
        self.active_errors.truncate(MAX_ACTIVE_ERRORS);
    }

    pub fn add_command(&mut self, cmd: &str) {
        self.command_count += 1;
        self.last_commands.insert(0, cmd.to_string());
        self.last_commands.truncate(MAX_COMMAND_HISTORY);
        self.last_active = chrono::Utc::now().timestamp();
    }

    /// Add an engram to the session, capping at MAX_ENGRAMS
    pub fn add_engram(&mut self, engram: crate::session::engram::Engram) {
        self.engrams.push_front(engram);
        if self.engrams.len() > crate::session::engram::MAX_ENGRAMS {
            self.engrams.pop_back();
        }
    }

    /// Add a tool call entry to the rolling log
    pub fn add_tool_call(&mut self, entry: crate::session::engram::ToolCallEntry) {
        self.tool_call_log.push_front(entry);
        if self.tool_call_log.len() > crate::session::engram::MAX_TOOL_CALL_LOG {
            self.tool_call_log.pop_back();
        }
    }

    /// Get the effective context window size (env > field > default)
    pub fn context_window_size(&self) -> u64 {
        std::env::var("VENG_CONTEXT_WINDOW")
            .ok()
            .and_then(|v| v.parse().ok())
            .or(self.context_window_size_hint)
            .unwrap_or(DEFAULT_CONTEXT_WINDOW_SIZE)
    }

    /// Compute pressure thresholds from env or defaults
    fn pressure_thresholds(&self) -> (f64, f64) {
        let mut warn = std::env::var("VENG_PRESSURE_WARN")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_PRESSURE_WARNING_THRESHOLD);
        let crit = std::env::var("VENG_PRESSURE_CRITICAL")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_PRESSURE_CRITICAL_THRESHOLD);

        // Reduce warning threshold if running in OuterLoop mode
        if self.loop_context.mode == LoopMode::OuterLoop
            && (warn - DEFAULT_PRESSURE_WARNING_THRESHOLD).abs() < f64::EPSILON
        {
            warn = 0.50;
        }

        (warn, crit)
    }

    /// Recalculate context pressure from current estimated tokens
    pub fn recalculate_pressure(&mut self) {
        let window = self.context_window_size();
        let ratio = self.estimated_current_tokens as f64 / window as f64;
        let (warn_threshold, crit_threshold) = self.pressure_thresholds();

        self.context_pressure = if ratio >= crit_threshold {
            ContextPressure::Critical
        } else if ratio >= warn_threshold {
            ContextPressure::Warning
        } else {
            ContextPressure::Normal
        };
    }

    /// Generate a pressure warning message, if warranted
    pub fn pressure_warning(&self) -> Option<String> {
        let window = self.context_window_size();
        let pct = if window > 0 {
            (self.estimated_current_tokens as f64 / window as f64 * 100.0) as u32
        } else {
            0
        };
        let est_k = self.estimated_current_tokens / 1000;
        let win_k = window / 1000;

        match self.context_pressure {
            ContextPressure::Normal => None,
            ContextPressure::Warning => Some(format!(
                "⚠️ VENG: Context ~{pct}% full (~{est_k}k/{win_k}k tokens). Consider compacting after completing current subtask."
            )),
            ContextPressure::Critical => Some(format!(
                "🚨 VENG: Context ~{pct}% full (~{est_k}k/{win_k}k tokens). COMPACT NOW before continuing."
            )),
        }
    }

    /// Check if a pressure warning should be emitted (respects cooldown)
    pub fn should_emit_pressure_warning(&self) -> bool {
        if self.context_pressure == ContextPressure::Normal {
            return false;
        }
        match self.last_pressure_warning_at {
            None => true,
            Some(last) => {
                let gap = self.command_count.saturating_sub(last);
                // Always warn on first Critical, otherwise respect cooldown
                (self.context_pressure == ContextPressure::Critical
                    || self.context_pressure == ContextPressure::Warning)
                    && gap >= PRESSURE_WARNING_COOLDOWN
            }
        }
    }
}

#[cfg(test)]
mod session_id_tests {
    use super::SessionState;

    /// Two sessions are two sessions (#490).
    ///
    /// The id was a millisecond clock reading, so a pair minted in the same
    /// millisecond collided and `INSERT OR REPLACE INTO sessions` kept one. Found
    /// while writing #482's regression test, which creates two states back to
    /// back and could not tell them apart.
    #[test]
    fn mints_a_distinct_id_every_time() {
        let ids: Vec<String> = (0..1000).map(|_| SessionState::new().session_id).collect();
        let unique: std::collections::HashSet<&String> = ids.iter().collect();
        assert_eq!(
            unique.len(),
            ids.len(),
            "ids collided: {} distinct out of {}",
            unique.len(),
            ids.len()
        );
    }

    /// Four call sites render `session_id[..8]`, and byte slicing panics off a
    /// char boundary, so the id has to stay ASCII and long enough to survive it.
    #[test]
    fn stays_sliceable_for_display() {
        let id = SessionState::new().session_id;
        assert!(id.len() >= 12, "too short to slice: {id}");
        assert!(id.is_ascii(), "not ascii, [..8] could panic: {id}");
        // The assertion above is the boundary proof clippy wants, and slicing is
        // the point of the test: it reproduces exactly what the four display call
        // sites do.
        #[allow(clippy::string_slice, reason = "asserted ascii on the line above")]
        let _ = &id[..8];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_display_formatting_correct() {
        assert_eq!(format!("{}", Route::Keep), "Keep");
        assert_eq!(format!("{}", Route::Soft), "Soft");
        assert_eq!(format!("{}", Route::Passthrough), "Passthrough");
        assert_eq!(format!("{}", Route::Rewind), "Rewind");
        assert_eq!(format!("{}", Route::Error), "Error");
    }

    #[test]
    fn test_distill_result_savings_pct_calculation() {
        let res = DistillResult {
            output: String::new(),
            route: Route::Keep,
            filter_name: String::new(),
            score: 0.0,
            context_score: 0.0,
            input_bytes: 100,
            output_bytes: 25,
            latency_ms: 0,
            rewind_hash: None,
            segments_kept: 0,
            segments_dropped: 0,
            collapse_savings: None,
            raw_tokens: 0,
            filtered_tokens: 0,
            delivered_bytes: 25,
        };
        assert_eq!(res.savings_pct(), 75.0);

        let res_zero = DistillResult {
            input_bytes: 0,
            output_bytes: 0,
            ..res
        };
        assert_eq!(res_zero.savings_pct(), 0.0);
    }

    #[test]
    fn test_distill_result_is_meaningful_threshold() {
        let mut res = DistillResult {
            output: String::new(),
            route: Route::Keep,
            filter_name: String::new(),
            score: 0.0,
            context_score: 0.0,
            input_bytes: 100,
            output_bytes: 89, // > 10% savings (89 < 90)
            latency_ms: 0,
            rewind_hash: None,
            segments_kept: 0,
            segments_dropped: 0,
            collapse_savings: None,
            raw_tokens: 0,
            filtered_tokens: 0,
            delivered_bytes: 89,
        };
        assert!(res.is_meaningful());

        res.output_bytes = 90; // Exactly 10% savings (90 < 90 is false)
        assert!(!res.is_meaningful());

        res.output_bytes = 95; // < 10% savings
        assert!(!res.is_meaningful());
    }

    #[test]
    fn context_boosts_with_hot_files() {
        let mut state = SessionState::new();
        state.add_hot_file("src/main.rs");
        // base count is 1 => boost = 0.1 * min(1/10, 0.3) = 0.01

        let text = "Error in src/main.rs at line 10";
        assert!((state.context_boost(text) - 0.01).abs() < f32::EPSILON);

        for _ in 0..19 {
            state.add_hot_file("src/main.rs");
        }
        // count is 20 => boost = 0.1 * min(20/10, 0.3) = 0.03
        // Float precision might cause issues here, so we check with a small delta.
        assert!((state.context_boost(text) - 0.03).abs() < f32::EPSILON);
    }

    #[test]
    fn context_boosts_with_active_errors() {
        let mut state = SessionState::new();
        state.add_error("expected identifier, found keyword `fn`");

        let text1 = "compiler output: expected identifier, found keyword `fn`";
        assert_eq!(state.context_boost(text1), 0.25);

        // Multiple matches are not additive for errors individually within the method loop unless there are multiple different errors matched.
        let text2 = "something else";
        assert_eq!(state.context_boost(text2), 0.0);
    }

    #[test]
    fn output_segment_final_score_is_clamped() {
        let seg1 = OutputSegment {
            content: "test".to_string(),
            tier: SignalTier::Noise,
            base_score: 0.8,
            context_score: 0.5,
            line_range: (0, 1),
        };
        assert_eq!(seg1.final_score(), 1.0);

        let seg2 = OutputSegment {
            content: "test".to_string(),
            tier: SignalTier::Noise,
            base_score: -0.5,
            context_score: 0.1,
            line_range: (0, 1),
        };
        assert_eq!(seg2.final_score(), 0.0);

        let seg3 = OutputSegment {
            content: "test".to_string(),
            tier: SignalTier::Noise,
            base_score: 0.4,
            context_score: 0.2,
            line_range: (0, 1),
        };
        // Use an epsilon check due to potential binary representation artifacts of f32 addition
        assert!((seg3.final_score() - 0.6).abs() < f32::EPSILON);
    }

    #[test]
    fn pressure_is_normal_when_tokens_low() {
        let mut state = SessionState::new();
        state.estimated_current_tokens = 50_000; // 25% of 200k default
        state.recalculate_pressure();
        assert_eq!(state.context_pressure, ContextPressure::Normal);
        assert!(state.pressure_warning().is_none());
    }

    #[test]
    fn pressure_transitions_to_warning() {
        let mut state = SessionState::new();
        state.estimated_current_tokens = 140_000; // 70% of 200k → Warning
        state.recalculate_pressure();
        assert_eq!(state.context_pressure, ContextPressure::Warning);
        assert!(state.pressure_warning().is_some());
        assert!(state.pressure_warning().unwrap().contains("⚠️"));
    }

    #[test]
    fn pressure_transitions_to_critical() {
        let mut state = SessionState::new();
        state.estimated_current_tokens = 180_000; // 90% of 200k → Critical
        state.recalculate_pressure();
        assert_eq!(state.context_pressure, ContextPressure::Critical);
        assert!(state.pressure_warning().unwrap().contains("🚨"));
    }

    #[test]
    fn pressure_respects_custom_window_size() {
        let mut state = SessionState::new();
        state.context_window_size_hint = Some(100_000);
        state.estimated_current_tokens = 70_000; // 70% of 100k → Warning
        state.recalculate_pressure();
        assert_eq!(state.context_pressure, ContextPressure::Warning);
    }

    #[test]
    fn pressure_warning_respects_cooldown() {
        let mut state = SessionState::new();
        state.estimated_current_tokens = 140_000;
        state.recalculate_pressure();

        // First warning at command 0 → should emit
        assert!(state.should_emit_pressure_warning());
        state.last_pressure_warning_at = Some(0);

        // Command 3 → within cooldown → should not emit
        state.command_count = 3;
        assert!(!state.should_emit_pressure_warning());

        // Command 5 → at cooldown boundary → should emit
        state.command_count = 5;
        assert!(state.should_emit_pressure_warning());
    }

    #[test]
    fn pressure_display_formatting() {
        assert_eq!(format!("{}", ContextPressure::Normal), "Normal");
        assert_eq!(format!("{}", ContextPressure::Warning), "Warning");
        assert_eq!(format!("{}", ContextPressure::Critical), "Critical");
    }
}
