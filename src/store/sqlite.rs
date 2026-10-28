// Safety: String slicing uses ASCII delimiter positions or boundary-checked safe utilities.

use anyhow::{Context, Result};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::paths;
use crate::pipeline::DistillResult;
use crate::pipeline::SessionState;

/// Customizes each pooled connection with required PRAGMA settings.
/// WAL mode is database-level and persists, but synchronous and foreign_keys are per-connection.
///
/// The last three are the in-process cache tier (#393). Asked whether omni should
/// gain Redis-class caching, and the answer is that SQLite already has one and it
/// was left at defaults: `cache_size` was `-2000`, two megabytes, against a 54 MB
/// working set, so roughly 500 pages of 50,226 were cached. `mmap_size` and
/// `temp_store` were unset, so a read-heavy projection paid syscall overhead and
/// spilled temporary results to disk. None of this needs a second daemon.
///
/// **`busy_timeout` is deliberately absent.** rusqlite calls
/// `sqlite3_busy_timeout(db, 5000)` itself in `inner_connection.rs`, so there is
/// nothing to add here and setting it can only lower it. A source grep for the
/// pragma finds nothing in this tree and reads like a missing setting, which is
/// how an earlier attempt talked itself into shortening the wait.
#[derive(Debug)]
struct PragmaCustomizer;

impl r2d2::CustomizeConnection<Connection, rusqlite::Error> for PragmaCustomizer {
    fn on_acquire(&self, conn: &mut Connection) -> Result<(), rusqlite::Error> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA foreign_keys = ON;
             PRAGMA cache_size = -65536;
             PRAGMA mmap_size = 268435456;
             PRAGMA temp_store = MEMORY;",
        )?;
        Ok(())
    }
}

pub struct SqliteBackend {
    pub(crate) pool: Pool<SqliteConnectionManager>,
}

pub struct Store {
    pub backend: SqliteBackend,
}

impl std::ops::Deref for Store {
    type Target = SqliteBackend;

    fn deref(&self) -> &Self::Target {
        &self.backend
    }
}

/// How long a verbatim execution trace is worth keeping.
///
/// Shorter than everything else on purpose. `execution_traces` stores `raw_input`
/// and `distilled_output` in full, so a row is two orders of magnitude heavier
/// than a `distillations` row, and it was in no cleanup at all: 160.1 MB of a
/// 187 MB database, against 6.0 MB for the last seven days. Its only reader is
/// `get_recent_traces`, which asks for the newest N (#165).
pub const TRACE_RETENTION_DAYS: u32 = 7;

impl Store {
    pub fn open() -> Result<Self> {
        Ok(Self {
            backend: SqliteBackend::open()?,
        })
    }

    pub fn open_path(path: &std::path::Path) -> Result<Self> {
        Ok(Self {
            backend: SqliteBackend::open_path(path)?,
        })
    }
}

impl SqliteBackend {
    /// Creates dir ~/.omni/ if none exists, Open/create DB, Run schema migrations
    pub fn open() -> Result<Self> {
        let db_path = if let Ok(custom_path) = std::env::var("OMNI_DB_PATH") {
            std::path::PathBuf::from(custom_path)
        } else {
            paths::database_path()
        };

        Self::open_path(&db_path)
    }

    /// Open a Store at a specific path (used by open() and tests)
    pub fn open_path(path: &std::path::Path) -> Result<Self> {
        let parent = path.parent().unwrap_or_else(|| std::path::Path::new(""));
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).context("Failed to create .omni directory")?;
            // Only a directory OMNI resolved for itself. `OMNI_DB_PATH` can name
            // any directory on the machine, and tightening one the user chose
            // would take access away from whoever else is in it (#631).
            if parent == paths::data_home() || parent == paths::config_home() {
                let _ = paths::restrict_to_owner(parent);
            }
        }

        let manager = SqliteConnectionManager::file(path);
        // `build()` blocks until `min_idle` connections exist, and r2d2 defaults
        // `min_idle` to `max_size`. A hook is a run-once process, so it was
        // opening four SQLite connections and running the pragma customizer on
        // each before doing any work. Measured on the release binary, median of
        // 10 opens: **3.19 ms with the default, 1.26 ms with one idle**, against
        // 0.81 ms for a bare `Connection` with the same pragmas. One line
        // recovers 73% of what the pool was costing.
        //
        // The pool itself stays. #174 asked whether a run-once process needs one
        // at all, and the honest answer is that removing it is 62 `self.pool`
        // call sites for the remaining 0.45 ms, while the MCP server shares this
        // type and serves over a single stdio stream where a pool is harmless.
        let pool = Pool::builder()
            .max_size(4)
            .min_idle(Some(1))
            .connection_customizer(Box::new(PragmaCustomizer))
            .build(manager)
            .context("Failed to create SQLite connection pool")?;

        let store = Self { pool };
        store.init_schema()?;

        // After the schema, because WAL mode is what creates the sidecars and
        // they hold committed rows that have not been checkpointed yet. Tighten
        // the database whatever directory it was asked for: the file is OMNI's
        // own, unlike the directory above (#631).
        // The sidecar names append to the whole filename rather than replacing an
        // extension, which `cli::reset` learned when 4.2 MB of `-wal` survived a
        // wipe that reported success (#446).
        let _ = paths::restrict_to_owner(path);
        for sidecar in ["-wal", "-shm"] {
            let mut p = path.to_path_buf().into_os_string();
            p.push(sidecar);
            let _ = paths::restrict_to_owner(&std::path::PathBuf::from(p));
        }

        Ok(store)
    }

    pub fn stats(&self) -> Result<(usize, usize)> {
        let conn = self.pool.get().context("DB pool exhausted")?;
        let sessions: usize = conn
            .query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0))
            .unwrap_or(0);
        let rewinds: usize = conn
            .query_row("SELECT COUNT(*) FROM rewind_store", [], |row| row.get(0))
            .unwrap_or(0);
        Ok((sessions, rewinds))
    }

    /// How many distillations the database holds.
    ///
    /// `stats` counts sessions and rewinds, and `doctor` printed the session
    /// count under the label "records", so a database holding 8,260
    /// distillations announced itself as "24 records" (#118).
    pub fn distillation_count(&self) -> usize {
        self.pool
            .get()
            .ok()
            .and_then(|conn| {
                conn.query_row("SELECT COUNT(*) FROM distillations", [], |row| row.get(0))
                    .ok()
            })
            .unwrap_or(0)
    }

    pub fn latest_activity_timestamps(&self) -> Result<(Option<u64>, Option<u64>)> {
        let conn = self.pool.get().context("DB pool exhausted")?;
        let s_ts: Option<i64> = conn
            .query_row(
                "SELECT last_active FROM sessions ORDER BY last_active DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .ok()
            .flatten();
        // Read from `distillations`, not `rewind_store`. The rewind table only
        // gains a row when a distillation had content worth storing for
        // retrieval, which on a real installation is rare, 0 rows beside 8,260
        // distillations on the reporting machine. `doctor` uses this for its
        // "Last distill" line, so it read `None` forever and printed
        // "never [IDLE]" two seconds after a distilled command (#118). That is
        // the exact line someone reads while checking whether hooks fire, and
        // it was telling them the opposite of the truth.
        let d_ts: Option<i64> = conn
            .query_row(
                "SELECT ts FROM distillations ORDER BY ts DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .ok()
            .flatten();
        Ok((s_ts.map(|v| v as u64), d_ts.map(|v| v as u64)))
    }

    pub fn check_fts5(&self) -> bool {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return false,
        };
        let query =
            "SELECT 1 FROM pragma_compile_options WHERE compile_options LIKE 'ENABLE_FTS5%'";
        conn.query_row(query, [], |row| row.get::<_, i64>(0))
            .is_ok()
    }

    /// Aggregate distillation stats since a given timestamp
    pub fn aggregate_stats(&self, since: i64) -> Result<(u64, u64, u64, u64, i64, u64, u64)> {
        let conn = self.pool.get().context("DB pool exhausted")?;
        // returns (count, total_input, total_output, sum_latency, max_latency, raw_tokens, filtered_tokens)
        let r = conn.query_row(
            &format!(
                "SELECT COALESCE(COUNT(*),0), COALESCE(SUM(input_bytes),0), COALESCE(SUM(output_bytes),0), COALESCE(SUM(latency_ms),0), COALESCE(MAX(latency_ms),0), COALESCE(SUM(raw_tokens),0), COALESCE(SUM(filtered_tokens),0) FROM distillations WHERE ts >= ?1 AND {}",
                applied_only()
            ),
            params![since],
            |row| Ok((
                row.get::<_,u64>(0)?,
                row.get::<_,u64>(1)?,
                row.get::<_,u64>(2)?,
                row.get::<_,u64>(3)?,
                row.get::<_,i64>(4)?,
                row.get::<_,u64>(5)?,
                row.get::<_,u64>(6)?
            )),
        ).unwrap_or((0, 0, 0, 0, 0, 0, 0));
        Ok(r)
    }

    /// Per-filter breakdown: (filter_name, count, avg_reduction_pct)
    #[allow(clippy::type_complexity)]
    pub fn filter_breakdown(&self, since: i64) -> Result<Vec<(String, u64, u64, u64, u64, u64)>> {
        let conn = self.pool.get().context("DB pool exhausted")?;
        let mut stmt = conn.prepare(&format!(
            "SELECT CASE WHEN command != '' THEN command ELSE '[unknown command]' END as grp_name, COUNT(*),
                    COALESCE(SUM(input_bytes), 0), COALESCE(SUM(output_bytes), 0),
                    COALESCE(SUM(raw_tokens), 0), COALESCE(SUM(filtered_tokens), 0)
             FROM distillations WHERE ts >= ?1 AND {} GROUP BY grp_name ORDER BY COUNT(*) DESC",
            applied_only()
        ))?;
        let rows = stmt
            .query_map(params![since], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, u64>(2)?,
                    row.get::<_, u64>(3)?,
                    row.get::<_, u64>(4)?,
                    row.get::<_, u64>(5)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    /// Per-filter re-run rate, distilled arm against raw arm (#109).
    ///
    /// Needs no new instrumentation: `session_id`, `ts`, `command` and `route`
    /// are already on every row, so this answers "which distillers cost the
    /// agent a second run" over history already collected.
    ///
    /// A re-run is the same command, in the same session, within
    /// `RERUN_WINDOW_SECS`. That is the closest thing to an objective statement
    /// that distillation dropped something needed: the agent held the output,
    /// it was not enough, and it paid to fetch the rest.
    ///
    /// Claude Code rows from before `POST_HOOK_FIX_TS` are excluded: on that
    /// path nothing was applied, so their `Keep` rows are controls wearing a
    /// treatment label. Keeping them does not just add noise, it can zero out a
    /// true finding, see the constant. This is not a call on what `omni stats`
    /// does with those rows for *savings* (#163); it is this comparison
    /// refusing rows that were never the experiment it is running.
    ///
    /// ponytail: correlated `EXISTS` per row: O(n²) worst case. Fine at the
    /// ~7k rows this table holds in practice; if it ever gets slow, materialise
    /// the window join into a temp table keyed on `(session_id, cmd)`.
    pub fn rerun_breakdown(&self, since: i64) -> Result<Vec<RerunRow>> {
        let conn = self.pool.get().context("DB pool exhausted")?;
        let mut stmt = conn.prepare(
            "WITH r AS (
                 SELECT session_id, ts, filter_name, route, input_bytes,
                        TRIM(command) AS cmd
                 FROM distillations
                 WHERE ts >= ?1 AND TRIM(command) != ''
                   AND NOT (agent_id = 'claude_code' AND ts < ?4)
             ),
             f AS (
                 SELECT r.filter_name,
                        r.input_bytes,
                        (r.route != 'Passthrough') AS distilled,
                        EXISTS (
                            SELECT 1 FROM r r2
                            WHERE r2.session_id = r.session_id
                              AND r2.cmd = r.cmd
                              AND r2.ts > r.ts
                              AND r2.ts <= r.ts + ?2
                        ) AS rerun
                 FROM r
             )
             SELECT filter_name,
                    SUM(distilled),
                    SUM(1 - distilled),
                    SUM(rerun * distilled),
                    SUM(rerun * (1 - distilled)),
                    CAST(COALESCE(AVG(CASE WHEN distilled = 1 THEN input_bytes END), 0) AS INTEGER),
                    CAST(COALESCE(AVG(CASE WHEN distilled = 0 THEN input_bytes END), 0) AS INTEGER)
             FROM f
             GROUP BY filter_name
             HAVING SUM(distilled) >= ?3 AND SUM(1 - distilled) >= ?3",
        )?;

        let mut rows: Vec<RerunRow> = stmt
            .query_map(
                params![
                    since,
                    crate::pipeline::RERUN_WINDOW_SECS,
                    crate::pipeline::RERUN_MIN_SAMPLES,
                    crate::pipeline::POST_HOOK_FIX_TS
                ],
                |row| {
                    Ok(RerunRow {
                        filter_name: row.get(0)?,
                        distilled: row.get(1)?,
                        raw: row.get(2)?,
                        distilled_reruns: row.get(3)?,
                        raw_reruns: row.get(4)?,
                        distilled_avg_input: row.get(5)?,
                        raw_avg_input: row.get(6)?,
                    })
                },
            )?
            .filter_map(|r| r.ok())
            .collect();

        // Worst offender first, that is the one worth reading a distiller for.
        rows.sort_by(|a, b| {
            b.delta_pp()
                .partial_cmp(&a.delta_pp())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(rows)
    }

    /// Both engines, each against its own base (#665). `omni stats` computed
    /// everything from `distillations` and never read `ledger_folds`, so the
    /// engine that removed the most bytes was absent from the report, and the
    /// one percentage printed was the distiller averaged over every call OMNI
    /// deliberately declined.
    pub fn engine_totals(&self, since: i64) -> Result<EngineTotals> {
        let conn = self.pool.get().context("DB pool exhausted")?;
        let mut t = EngineTotals::default();

        let mut stmt = conn.prepare(&format!(
            "SELECT route, COUNT(*), COALESCE(SUM(input_bytes),0), COALESCE(SUM(output_bytes),0)
             FROM distillations WHERE ts >= ?1 AND {} GROUP BY route",
            applied_only()
        ))?;
        let rows = stmt.query_map(params![since], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u64>(1)?,
                row.get::<_, u64>(2)?,
                row.get::<_, u64>(3)?,
            ))
        })?;
        for (route, calls, input, output) in rows.flatten() {
            // A declined call contributes no saving and no base. Averaging it in
            // is what turned 48% into 9%, and putting its 31 MB in the same
            // column as the savings reads as bytes that went missing.
            if route == "Passthrough" {
                t.declined_calls += calls;
                t.declined_bytes += input;
            } else {
                t.distilled_calls += calls;
                t.distilled_input += input;
                t.distilled_output += output;
            }
        }

        // One fold writes one row per (origin, source agent) pair and repeats
        // the same `payload_bytes` on each, so rows are not folds. Counting rows
        // inflates the population and, below, sums one payload several times:
        // 479 priced rows on the maintainer's machine are 406 folds, and the
        // difference took a real 41% down to a reported 31%. `bytes` is the one
        // column that is genuinely per-row and stays a plain sum.
        //
        // `fold_id` is exact for anything written since the migration that added
        // it. Rows older than that were backfilled from (second, session, agent,
        // scope, payload size), which merges two folds that share all five, so a
        // historical count is a lower bound and a historical ratio a slight
        // overstatement.
        let folds = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(bytes),0), MIN(ts) FROM (
                 SELECT SUM(bytes) AS bytes, MIN(ts) AS ts FROM ledger_folds
                 WHERE ts >= ?1 GROUP BY fold_id)",
            params![since],
            |row| {
                Ok((
                    row.get::<_, u64>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            },
        );
        if let Ok((count, bytes, first)) = folds {
            t.folds = count;
            t.fold_bytes = bytes;
            t.first_fold_ts = first;
        }

        // `payload_bytes` arrived in a migration defaulting to 0, so most rows
        // carry a saving with no base to divide it by. Counting them separately
        // is the difference between a ratio and the two-population error that
        // put a wrong figure in #533's thread.
        let priced = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(bytes),0), COALESCE(SUM(payload),0) FROM (
                 SELECT SUM(bytes) AS bytes, MAX(payload_bytes) AS payload FROM ledger_folds
                 WHERE ts >= ?1 AND payload_bytes > 0 GROUP BY fold_id)",
            params![since],
            |row| {
                Ok((
                    row.get::<_, u64>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, u64>(2)?,
                ))
            },
        );
        if let Ok((count, bytes, payload)) = priced {
            t.priced_folds = count;
            t.priced_fold_bytes = bytes;
            t.priced_payload = payload;
        }

        Ok(t)
    }

    /// Route distribution: (route, count)
    pub fn route_distribution(&self, since: i64) -> Result<Vec<(String, u64)>> {
        let conn = self.pool.get().context("DB pool exhausted")?;
        let mut stmt = conn.prepare(
            "SELECT route, COUNT(*) FROM distillations WHERE ts >= ?1 GROUP BY route ORDER BY COUNT(*) DESC"
        )?;
        let rows = stmt
            .query_map(params![since], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    /// RewindStore metrics: (total_stored, total_retrieved)
    /// Archives whose row was last written since `since`, and how many of those
    /// rows have ever been pulled. `since` of 0 is all time.
    ///
    /// Both halves are looser than they look and the wording is exact on purpose.
    /// `ts` is refreshed when identical content is archived again, so a row can
    /// enter this window carrying a `retrieved` count from before it; and
    /// `retrieved` is a lifetime counter, never a pull inside the window.
    ///
    /// Resetting that counter on re-archive would make the looser reading true and
    /// is not this function's call: #581 pays a verbatim delivery per recorded
    /// pull, so zeroing it here cancels a debt the reader is still owed.
    ///
    /// It took a window because `--detail` prints this pair under a header naming
    /// one, so an all-time count read as part of the window (#667).
    pub fn rewind_metrics(&self, since: i64) -> Result<(u64, u64)> {
        let conn = self.pool.get().context("DB pool exhausted")?;
        let total: u64 = conn
            .query_row(
                "SELECT COUNT(*) FROM rewind_store WHERE ts >= ?1",
                params![since],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let retrieved: u64 = conn
            .query_row(
                "SELECT COUNT(*) FROM rewind_store WHERE ts >= ?1 AND retrieved > 0",
                params![since],
                |row| row.get(0),
            )
            .unwrap_or(0);
        Ok((total, retrieved))
    }

    /// Hot files for session insight
    pub fn hot_files_global(&self, since: i64) -> Result<Vec<(String, u64)>> {
        let conn = self.pool.get().context("DB pool exhausted")?;
        let mut stmt = conn.prepare(
            "SELECT file_path, SUM(access_count) as cnt FROM file_access WHERE last_access >= ?1 GROUP BY file_path ORDER BY cnt DESC LIMIT 5"
        )?;
        let rows = stmt
            .query_map(params![since], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    /// Collapse aggregate: (event_count, total_original_lines, total_collapsed_lines)
    pub fn collapse_aggregate(&self, since: i64) -> Result<(u64, u64, u64)> {
        let conn = self.pool.get().context("DB pool exhausted")?;
        let r = conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(collapse_original),0), COALESCE(SUM(collapse_to),0) FROM distillations WHERE ts >= ?1 AND collapse_original > 0",
            params![since],
            |row| Ok((row.get::<_,u64>(0)?, row.get::<_,u64>(1)?, row.get::<_,u64>(2)?)),
        ).unwrap_or((0, 0, 0));
        Ok(r)
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self.pool.get().context("DB pool exhausted")?;
        conn.execute_batch(
            r#"
            -- 1. Sessions
            CREATE TABLE IF NOT EXISTS sessions (
                id           TEXT PRIMARY KEY,
                started_at   INTEGER NOT NULL,
                last_active  INTEGER NOT NULL,
                task_hint    TEXT DEFAULT '',
                domain_hint  TEXT DEFAULT '',
                state_json   TEXT DEFAULT '{}'
            );

            -- 1b. Passthrough events telemetry
            CREATE TABLE IF NOT EXISTS passthrough_events (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                command      TEXT NOT NULL,
                bytes        INTEGER NOT NULL,
                ts           INTEGER NOT NULL,
                reason       TEXT NOT NULL DEFAULT 'unrecorded'
            );
            CREATE INDEX IF NOT EXISTS idx_pt_ts ON passthrough_events(ts);

            -- 1c. Unhandled tools telemetry
            CREATE TABLE IF NOT EXISTS unhandled_tools (
                tool_name    TEXT PRIMARY KEY,
                count        INTEGER DEFAULT 1,
                last_seen    INTEGER NOT NULL
            );

            -- 2. Distillation tracking
            CREATE TABLE IF NOT EXISTS distillations (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id   TEXT NOT NULL,
                ts           INTEGER NOT NULL,
                filter_name  TEXT NOT NULL,
                input_bytes  INTEGER NOT NULL,
                output_bytes INTEGER NOT NULL,
                route        TEXT NOT NULL,
                score        REAL NOT NULL DEFAULT 0.0,
                context_score REAL NOT NULL DEFAULT 0.0,
                latency_ms   INTEGER NOT NULL,
                rewind_hash  TEXT DEFAULT '',
                command      TEXT DEFAULT '',
                project_path TEXT DEFAULT '',
                agent_id     TEXT DEFAULT 'unknown',
                -- Bytes that reached a model, as against `output_bytes`, which is
                -- only what the distiller returned. -1 means "recorded before the
                -- column existed", never "nothing was delivered" (#212).
                delivered_bytes INTEGER DEFAULT -1
            );
            CREATE INDEX IF NOT EXISTS idx_dist_ts ON distillations(ts);
            CREATE INDEX IF NOT EXISTS idx_dist_session ON distillations(session_id);
            CREATE INDEX IF NOT EXISTS idx_dist_filter ON distillations(filter_name);

            -- Every hook run opens with `find_latest_session`, which is
            -- `ORDER BY last_active DESC LIMIT 1`. Without this the plan is
            -- `SCAN sessions` plus a temp B-tree, so it drags every row's
            -- `state_json` through a sort to return one of them: 24 rows and
            -- 312 KB on the reporting installation. Measured by ablation, that
            -- lookup was 16.8 ms of the hook's 28.8 ms (#569).
            CREATE INDEX IF NOT EXISTS idx_sessions_last_active
                ON sessions(last_active DESC);

            -- 3. File access
            CREATE TABLE IF NOT EXISTS file_access (
                session_id   TEXT NOT NULL,
                file_path    TEXT NOT NULL,
                access_count INTEGER DEFAULT 1,
                last_access  INTEGER NOT NULL,
                PRIMARY KEY (session_id, file_path)
            );

            -- 4. RewindStore
            CREATE TABLE IF NOT EXISTS rewind_store (
                hash         TEXT PRIMARY KEY,
                content      TEXT NOT NULL,
                ts           INTEGER NOT NULL,
                original_len INTEGER NOT NULL,
                retrieved    INTEGER DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_rewind_ts ON rewind_store(ts);

            -- 4b. Session ledger: which lines a scope has already been shown.
            -- Hashes only. The content of a run goes to `rewind_store` and only
            -- when a handle is actually issued, so recording every emitted block
            -- costs 16 bytes a line rather than a second copy of the corpus.
            -- WITHOUT ROWID because every read is a primary-key probe and the
            -- table is nothing but its key.
            -- `agent_id` is recorded and read by nothing on the hook path (#509).
            -- The project scope is the working directory and nothing else, so two
            -- agents in one repo share a history and neither the fold nor the
            -- marker can tell them apart. Keying on the agent would end that, and
            -- would also give up whatever cross-agent reuse is real, which is the
            -- question this column exists to answer before the key changes.
            CREATE TABLE IF NOT EXISTS ledger_lines (
                scope     TEXT NOT NULL,
                line_hash TEXT NOT NULL,
                ts        INTEGER NOT NULL,
                agent_id  TEXT NOT NULL DEFAULT 'unknown',
                -- What command first showed this line, so a fold can say whose
                -- bytes it is replacing (#622). Empty means unrecorded, which
                -- suppresses the marker's source clause rather than guessing.
                source    TEXT NOT NULL DEFAULT '',
                -- Rows written since #911 leave `source` empty and point here.
                source_id INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (scope, line_hash)
            ) WITHOUT ROWID;
            CREATE INDEX IF NOT EXISTS idx_ledger_ts ON ledger_lines(ts);

            -- One row per distinct command (#911). Storing the text on every line
            -- it showed made it 420 of 615 MB in one 30-day store: 539,530 rows,
            -- 18,352 distinct commands, 435 bytes each on average.
            CREATE TABLE IF NOT EXISTS ledger_sources (
                id     INTEGER PRIMARY KEY,
                source TEXT NOT NULL UNIQUE,
                ts     INTEGER NOT NULL
            );

            -- What a fold actually drew on, which nothing recorded before (#533).
            -- `ledger_lines` says a line was seen; it cannot say a marker was ever
            -- issued against it, by which scope, or whose bytes it replaced. So
            -- the value of cross-agent reuse was unrecoverable from the corpus
            -- after the fact, and `PROJECT_FLOOR_MULT` has been pricing that case
            -- since #448 without ever being checked against it.
            --
            -- One row per (origin, source agent) per fold, so the query that
            -- settles the decision is a GROUP BY rather than a replay:
            --   SELECT origin, agent_id = source_agent AS same, SUM(bytes)
            --   FROM ledger_folds GROUP BY 1, 2;
            --
            -- `whole_output` says every line of the payload was folded, so the
            -- agent was handed markers and no content. That is the case
            -- `MIN_WHOLE_OUTPUT_FOLD` refuses below 1 KB (#543), and until this
            -- column nothing recorded it, so the floor could not be checked
            -- against the corpus it was calibrated on:
            --   SELECT * FROM ledger_folds
            --   WHERE whole_output = 1 AND payload_bytes < 1024;
            --
            -- `payload_bytes` is the whole payload, carried on every row of the
            -- call, so the audit is a row predicate. Summing `bytes` would need
            -- a GROUP BY on a per-call key, and `ts` is not one: it is whole
            -- seconds, so two folds by the same agent in the same second merge
            -- and their combined size can clear a floor that neither of them
            -- cleared alone, hiding exactly the violation being looked for.
            CREATE TABLE IF NOT EXISTS ledger_folds (
                id           INTEGER PRIMARY KEY,
                ts           INTEGER NOT NULL,
                scope        TEXT NOT NULL,
                agent_id     TEXT NOT NULL,
                source_agent TEXT NOT NULL,
                origin       TEXT NOT NULL,
                lines        INTEGER NOT NULL,
                bytes        INTEGER NOT NULL,
                whole_output INTEGER NOT NULL DEFAULT 0,
                payload_bytes INTEGER NOT NULL DEFAULT 0,
                -- Which session produced the fold. `scope` above is keyed on the
                -- project when there is one, deliberately, so that the cross-agent
                -- question #533 exists to answer stays askable. That leaves the
                -- table with no way to say "this session", which is the question
                -- `omni_explain_savings` asks (#602).
                session   TEXT NOT NULL DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_ledger_folds_ts ON ledger_folds(ts);

            -- 5. FTS5 for session events
            CREATE VIRTUAL TABLE IF NOT EXISTS session_events USING fts5(
                session_id UNINDEXED,
                event_type UNINDEXED,
                content,
                ts UNINDEXED,
                tokenize = 'porter ascii'
            );

            -- 6. Execution traces
            CREATE TABLE IF NOT EXISTS execution_traces (
                id INTEGER PRIMARY KEY,
                session_id TEXT NOT NULL,
                ts INTEGER NOT NULL,
                command TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                project_path TEXT NOT NULL,
                raw_input TEXT NOT NULL,
                distilled_output TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_traces_ts ON execution_traces(ts);

            -- 7. Session summaries 
            CREATE TABLE IF NOT EXISTS session_summaries (
                session_id   TEXT PRIMARY KEY,
                started_at   INTEGER NOT NULL,
                ended_at     INTEGER NOT NULL,
                agent_id     TEXT DEFAULT 'unknown',
                total_commands INTEGER DEFAULT 0,
                tokens_saved INTEGER DEFAULT 0,
                top_filter   TEXT DEFAULT '',
                exit_reason  TEXT DEFAULT 'unknown',
                project_path TEXT DEFAULT ''
            );

            -- 8. Retrieve events for adaptive threshold 
            CREATE TABLE IF NOT EXISTS retrieve_events (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                command_prefix TEXT NOT NULL,
                hash         TEXT DEFAULT '',
                ts           INTEGER NOT NULL,
                agent_id     TEXT DEFAULT 'unknown'
            );
            CREATE INDEX IF NOT EXISTS idx_retrieve_cmd ON retrieve_events(command_prefix);
            CREATE INDEX IF NOT EXISTS idx_retrieve_ts  ON retrieve_events(ts);

            -- 9. Project knowledge, cross-session semantic memory 
            CREATE TABLE IF NOT EXISTS project_knowledge (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                project_hash TEXT NOT NULL,
                key          TEXT NOT NULL,
                value        TEXT NOT NULL,
                confidence   REAL DEFAULT 0.7,
                hit_count    INTEGER DEFAULT 1,
                last_updated INTEGER NOT NULL,
                UNIQUE(project_hash, key)
            );
            CREATE INDEX IF NOT EXISTS idx_pk_project ON project_knowledge(project_hash);

            -- 10. Multi-agent sessions, shared state across agents 
            CREATE TABLE IF NOT EXISTS agent_sessions (
                agent_id     TEXT NOT NULL,
                session_id   TEXT NOT NULL,
                project_hash TEXT NOT NULL,
                last_active  INTEGER NOT NULL,
                state_json   TEXT DEFAULT '{}',
                PRIMARY KEY (agent_id, project_hash)
            );
            CREATE INDEX IF NOT EXISTS idx_as_project ON agent_sessions(project_hash);

            -- 11b. Pattern memory, cross-session error pattern tracking
            CREATE TABLE IF NOT EXISTS pattern_memory (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                pattern_hash    TEXT NOT NULL UNIQUE,
                pattern_text    TEXT NOT NULL,
                tool_family     TEXT DEFAULT '',
                first_seen      INTEGER NOT NULL,
                last_seen       INTEGER NOT NULL,
                occurrence_count INTEGER DEFAULT 1,
                was_resolved    INTEGER DEFAULT 0,
                resolution_hint TEXT DEFAULT ''
            );
            CREATE INDEX IF NOT EXISTS idx_pm_tool ON pattern_memory(tool_family);
            CREATE INDEX IF NOT EXISTS idx_pm_last ON pattern_memory(last_seen);

            -- 11. One-time data migrations tracker
            CREATE TABLE IF NOT EXISTS schema_migrations (
                id           TEXT PRIMARY KEY,
                applied_at   INTEGER NOT NULL
            );


            -- 14. Loop Memory, cross-session persistent knowledge (L2-02)
            CREATE TABLE IF NOT EXISTS loop_memory (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                loop_goal_hash TEXT NOT NULL,
                key TEXT NOT NULL,
                value TEXT NOT NULL,
                confidence REAL DEFAULT 0.5,
                times_confirmed INTEGER DEFAULT 1,
                first_seen INTEGER NOT NULL,
                last_seen INTEGER NOT NULL,
                ttl_days INTEGER DEFAULT 30,
                UNIQUE(loop_goal_hash, key)
            );
            -- 15a. Retrieval Feedback, adaptive scoring (INT-01)
            CREATE TABLE IF NOT EXISTS retrieval_feedback (
                id           INTEGER PRIMARY KEY AUTOINCREMENT,
                query        TEXT NOT NULL,
                hit_source   TEXT NOT NULL,
                hit_key      TEXT NOT NULL,
                project_hash TEXT NOT NULL,
                command_ctx  TEXT DEFAULT '',
                ts           INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_rf_project ON retrieval_feedback(project_hash, ts);
            CREATE INDEX IF NOT EXISTS idx_rf_cmd ON retrieval_feedback(command_ctx);

            -- 15. Episodic Memory (Engrams)
            CREATE TABLE IF NOT EXISTS engrams (
                id           TEXT PRIMARY KEY,
                session_id   TEXT NOT NULL,
                trigger      TEXT NOT NULL,
                label        TEXT NOT NULL,
                detail       TEXT,
                files        TEXT DEFAULT '[]',
                category     TEXT DEFAULT 'progress',
                tags         TEXT DEFAULT '[]',
                project_hash TEXT NOT NULL,
                ts           INTEGER NOT NULL,
                last_accessed INTEGER NOT NULL,
                hit_count    INTEGER DEFAULT 0,
                FOREIGN KEY (session_id) REFERENCES sessions(id)
            );
            CREATE INDEX IF NOT EXISTS idx_engrams_project ON engrams(project_hash);

            -- 16. Semantic Search Virtual Tables (FTS5)
            CREATE VIRTUAL TABLE IF NOT EXISTS engrams_fts USING fts5(
                label,
                detail,
                files,
                tags,
                content='engrams',
                content_rowid='rowid',
                tokenize='porter unicode61'
            );

            CREATE TRIGGER IF NOT EXISTS engrams_fts_insert
                AFTER INSERT ON engrams BEGIN
                    INSERT INTO engrams_fts(rowid, label, detail, files, tags)
                    VALUES (new.rowid, new.label, new.detail, new.files, new.tags);
                END;

            CREATE TRIGGER IF NOT EXISTS engrams_fts_delete
                AFTER DELETE ON engrams BEGIN
                    INSERT INTO engrams_fts(engrams_fts, rowid, label, detail, files, tags)
                    VALUES ('delete', old.rowid, old.label, old.detail, old.files, old.tags);
                END;

            CREATE TRIGGER IF NOT EXISTS engrams_fts_update
                AFTER UPDATE ON engrams BEGIN
                    INSERT INTO engrams_fts(engrams_fts, rowid, label, detail, files, tags)
                    VALUES ('delete', old.rowid, old.label, old.detail, old.files, old.tags);
                    INSERT INTO engrams_fts(rowid, label, detail, files, tags)
                    VALUES (new.rowid, new.label, new.detail, new.files, new.tags);
                END;

            CREATE VIRTUAL TABLE IF NOT EXISTS knowledge_fts USING fts5(
                key,
                value,
                content='project_knowledge',
                content_rowid='rowid',
                tokenize='porter unicode61'
            );

            CREATE TRIGGER IF NOT EXISTS knowledge_fts_insert
                AFTER INSERT ON project_knowledge BEGIN
                    INSERT INTO knowledge_fts(rowid, key, value)
                    VALUES (new.rowid, new.key, new.value);
                END;

            CREATE TRIGGER IF NOT EXISTS knowledge_fts_delete
                AFTER DELETE ON project_knowledge BEGIN
                    INSERT INTO knowledge_fts(knowledge_fts, rowid, key, value)
                    VALUES ('delete', old.rowid, old.key, old.value);
                END;

            CREATE TRIGGER IF NOT EXISTS knowledge_fts_update
                AFTER UPDATE ON project_knowledge BEGIN
                    INSERT INTO knowledge_fts(knowledge_fts, rowid, key, value)
                    VALUES ('delete', old.rowid, old.key, old.value);
                    INSERT INTO knowledge_fts(rowid, key, value)
                    VALUES (new.rowid, new.key, new.value);
                END;
            "#,
        )?;

        // Safe migration: check for legacy content_type (v0.5.6 migration)
        let has_content_type = {
            let mut stmt = conn.prepare("PRAGMA table_info(distillations)")?;
            let mut rows = stmt.query([])?;
            let mut found = false;
            while let Some(row) = rows.next()? {
                let name: String = row.get(1)?;
                if name == "content_type" {
                    found = true;
                    break;
                }
            }
            found
        };

        if has_content_type {
            // Recreate table to remove legacy NOT NULL content_type column
            conn.execute_batch(
                r#"
                ALTER TABLE distillations RENAME TO distillations_old;
                CREATE TABLE distillations (
                    id           INTEGER PRIMARY KEY AUTOINCREMENT,
                    session_id   TEXT NOT NULL,
                    ts           INTEGER NOT NULL,
                    filter_name  TEXT NOT NULL,
                    input_bytes  INTEGER NOT NULL,
                    output_bytes INTEGER NOT NULL,
                    route        TEXT NOT NULL,
                    score        REAL NOT NULL DEFAULT 0.0,
                    context_score REAL NOT NULL DEFAULT 0.0,
                    latency_ms   INTEGER NOT NULL,
                    rewind_hash  TEXT DEFAULT '',
                    command      TEXT DEFAULT '',
                    project_path TEXT DEFAULT '',
                    agent_id     TEXT DEFAULT 'unknown'
                );
                INSERT INTO distillations 
                (id, session_id, ts, filter_name, input_bytes, output_bytes, route, score, context_score, latency_ms, rewind_hash, command, project_path, agent_id)
                SELECT id, session_id, ts, filter_name, input_bytes, output_bytes, route, score, context_score, latency_ms, rewind_hash, command, '', 'unknown' 
                FROM distillations_old;
                DROP TABLE distillations_old;
                CREATE INDEX idx_dist_ts ON distillations(ts);
                CREATE INDEX idx_dist_session ON distillations(session_id);
                CREATE INDEX idx_dist_filter ON distillations(filter_name);
                "#,
            )?;
        }

        // #441. Rows written before this column carry their reason concatenated
        // onto `command`, so the default says exactly that rather than naming a
        // reason nobody recorded.
        let _ = conn.execute(
            "ALTER TABLE passthrough_events ADD COLUMN reason TEXT NOT NULL DEFAULT 'unrecorded'",
            [],
        );
        // #773. The table could not say which door a decline came through, so it
        // could not say what share of what OMNI declines was declined on behalf
        // of a model. `session` is not that test: `host output cap` fires only
        // for `claude_code`, and 20 of its 39 rows carry no session id.
        //
        // Old rows read `unknown` rather than being guessed at. The door cannot
        // be recovered after the fact, so any query about the split windows
        // itself, the same rule the `reason` column above already needs.
        let _ = conn.execute(
            "ALTER TABLE passthrough_events ADD COLUMN agent_id TEXT NOT NULL DEFAULT 'unknown'",
            [],
        );
        // Rows written before this column predate the flag entirely, so 0 means
        // "not recorded" and not "was a partial fold". Queries about the floor
        // have to bound themselves by ts for that reason.
        let _ = conn.execute(
            "ALTER TABLE ledger_folds ADD COLUMN whole_output INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE ledger_folds ADD COLUMN payload_bytes INTEGER NOT NULL DEFAULT 0",
            [],
        );
        // #602. Rows written before this have no session, so a per-session
        // lookup finds nothing for them, which is honest: the column did not
        // exist and the project-keyed `scope` cannot be narrowed to one session
        // after the fact.
        let _ = conn.execute(
            "ALTER TABLE ledger_folds ADD COLUMN session TEXT NOT NULL DEFAULT ''",
            [],
        );
        // #822. A marker recorded which shape it rendered and never which tool
        // produced the reply, so a report could say `project_run` is retrieved
        // more often than `session_run` and never which surface that lands on.
        // Rows written before this carry an empty string, which is the honest
        // answer rather than a guess from the scope.
        let _ = conn.execute(
            "ALTER TABLE fold_markers ADD COLUMN tool TEXT NOT NULL DEFAULT ''",
            [],
        );
        // #665. One fold writes a row per (origin, source agent) pair, and until
        // this column nothing recorded which rows belonged together. Reporting
        // had to infer it from (second, session, agent, scope, payload size),
        // which merges two folds that land in one second on a payload of the
        // same size.
        //
        // Rows written before the column get that same inference as a backfill,
        // because the information is genuinely gone: the smallest `id` in each
        // inferred group, which is stable and matches what a new fold gets.
        let fresh_fold_id = conn
            .execute(
                "ALTER TABLE ledger_folds ADD COLUMN fold_id INTEGER NOT NULL DEFAULT 0",
                [],
            )
            .is_ok();
        if fresh_fold_id {
            let _ = conn.execute(
                "UPDATE ledger_folds SET fold_id = (
                     SELECT MIN(f.id) FROM ledger_folds f
                     WHERE f.ts = ledger_folds.ts AND f.session = ledger_folds.session
                       AND f.agent_id = ledger_folds.agent_id AND f.scope = ledger_folds.scope
                       AND f.payload_bytes = ledger_folds.payload_bytes)",
                [],
            );
        }
        // #766. `ledger_folds` aggregates a call's folds by (origin, source agent),
        // so it cannot say which marker wording a reader was handed, and it holds
        // no handle, so nothing joins a retrieval back to the fold that caused it.
        // Both questions are per marker, which is a finer grain than that table
        // has, and widening it would change every figure already published from
        // it. A second table at the right grain answers them and disturbs nothing.
        //
        // `kind` is decided by the code that renders the string, never by parsing
        // one back: this repository has been bitten three times by two copies of
        // one string (#452, #454, #456), and #765 removed a tally that tried to
        // recognise markers in text.
        let _ = conn.execute(
            "CREATE TABLE IF NOT EXISTS fold_markers (
                id      INTEGER PRIMARY KEY,
                fold_id INTEGER NOT NULL,
                ts      INTEGER NOT NULL,
                scope   TEXT NOT NULL,
                session TEXT NOT NULL DEFAULT '',
                agent_id TEXT NOT NULL DEFAULT 'unknown',
                tool    TEXT NOT NULL DEFAULT '',
                kind    TEXT NOT NULL,
                handle  TEXT NOT NULL DEFAULT '',
                lines   INTEGER NOT NULL,
                bytes   INTEGER NOT NULL
            )",
            [],
        );
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_fold_markers_handle ON fold_markers(handle)",
            [],
        );
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_fold_markers_ts ON fold_markers(ts)",
            [],
        );
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_ledger_folds_fold ON ledger_folds(fold_id)",
            [],
        );

        // #672. Nothing tied a declined call to the session that made it, so the
        // reason breakdown could only ever be global. A time window is not a
        // substitute: a third of sessions on this machine overlap another.
        // Empty means the host sent no id, which is the honest answer for pipe
        // mode rather than the wall-clock fallback that made #118's slices wrong.
        let _ = conn.execute(
            "ALTER TABLE passthrough_events ADD COLUMN session TEXT NOT NULL DEFAULT ''",
            [],
        );
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_pt_session ON passthrough_events(session)",
            [],
        );

        // #581. Set when a handle is pulled and cleared by the delivery that
        // answers it, so the content a marker sent the reader to fetch is not
        // folded back into that same marker on its way in. Separate from
        // `retrieved`, which is a lifetime counter `rewind_metrics` and #512's
        // adaptive rate both read and must not be reset.
        let _ = conn.execute(
            "ALTER TABLE rewind_store ADD COLUMN owed INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE distillations ADD COLUMN collapse_original INTEGER DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE distillations ADD COLUMN collapse_to INTEGER DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE distillations ADD COLUMN project_path TEXT DEFAULT ''",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE distillations ADD COLUMN agent_id TEXT DEFAULT 'unknown'",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE distillations ADD COLUMN raw_tokens INTEGER DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE distillations ADD COLUMN filtered_tokens INTEGER DEFAULT 0",
            [],
        );
        // #509. `INSERT OR IGNORE` keeps the first writer, so an existing row is
        // the agent that first emitted that line into that scope and the default
        // is honest about rows written before anyone was recorded.
        let _ = conn.execute(
            "ALTER TABLE ledger_lines ADD COLUMN agent_id TEXT NOT NULL DEFAULT 'unknown'",
            [],
        );
        // #622. What command first showed this line. The table keyed lines by
        // (scope, hash) alone, so a fold could replace a block in file B with one
        // shown from file A and the marker had no way to say so. Reading two
        // files to check a shared block matches is exactly that case, and the
        // dedup answered it by deleting the evidence.
        //
        // It is also what makes the frequency measurable: nothing in the corpus
        // could say how often a fold draws on a different source, because the
        // column did not exist. Same reason `ledger_folds` was added by #533.
        // Rows written before this keep the empty default, which reads as
        // "unrecorded" and suppresses the marker clause rather than guessing.
        let _ = conn.execute(
            "ALTER TABLE ledger_lines ADD COLUMN source TEXT NOT NULL DEFAULT ''",
            [],
        );
        // #911. Rows written before it keep their text in `source` and age out
        // with the window; `ledger_seen` reads either.
        let _ = conn.execute(
            "ALTER TABLE ledger_lines ADD COLUMN source_id INTEGER NOT NULL DEFAULT 0",
            [],
        );
        // #212: `output_bytes` is what the distiller returned, which is not the
        // same as what a model read. Rows written before this column exists keep
        // the default of -1, which `omni stats` reads as "unknown" rather than as
        // "nothing was delivered", backfilling them from `output_bytes` would
        // restate the old assumption as if it had been measured.
        let _ = conn.execute(
            "ALTER TABLE distillations ADD COLUMN delivered_bytes INTEGER DEFAULT -1",
            [],
        );

        // One-time data migration:
        // Older builds could attribute Cursor sessions as "vscode" because TERM_PROGRAM
        // detection happened before explicit OMNI_AGENT_ID routing.
        // Keep this migration idempotent and run only once.
        let migration_id = "2026_05_cursor_agent_id_backfill_vscode_to_cursor";
        let already_applied: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM schema_migrations WHERE id = ?1 LIMIT 1",
                params![migration_id],
                |row| row.get(0),
            )
            .optional()?;
        if already_applied.is_none() {
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "UPDATE distillations SET agent_id = 'cursor' WHERE agent_id = 'vscode'",
                [],
            )?;
            tx.execute(
                "UPDATE execution_traces SET agent_id = 'cursor' WHERE agent_id = 'vscode'",
                [],
            )?;
            tx.execute(
                "UPDATE session_summaries SET agent_id = 'cursor' WHERE agent_id = 'vscode'",
                [],
            )?;
            tx.execute(
                "INSERT INTO schema_migrations (id, applied_at) VALUES (?1, ?2)",
                params![migration_id, chrono::Utc::now().timestamp()],
            )?;
            tx.commit()?;
        }

        // Every pattern in an existing database carries a `was_resolved` that was
        // set by any success in its tool family and never taken back, so
        // `omni patterns` reported 20 of 20 RESOLVED while five of them were
        // still firing (#427). The flag now clears on recurrence, but history
        // cannot be re-derived: nothing recorded when a resolution happened. So
        // the label is cleared once and re-earned by the new rule, which is the
        // only reading of these rows that is not a guess.
        let migration_clear_resolved = "2026_08_clear_unearned_pattern_resolutions";
        let resolved_applied: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM schema_migrations WHERE id = ?1",
                params![migration_clear_resolved],
                |r| r.get(0),
            )
            .optional()
            .unwrap_or(None);
        if resolved_applied.is_none() {
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "UPDATE pattern_memory SET was_resolved = 0, resolution_hint = ''",
                [],
            )?;
            tx.execute(
                "INSERT INTO schema_migrations (id, applied_at) VALUES (?1, ?2)",
                params![migration_clear_resolved, chrono::Utc::now().timestamp()],
            )?;
            tx.commit()?;
        }

        // #566. The hook sends no reply on the `Passthrough` route, so the host
        // keeps the bytes it already had, but every byte and token column on the
        // row was computed from the distiller's output. The row therefore booked a
        // saving nobody received. Reconciled against the host's own transcripts
        // that was 16.4% of the bytes booked as saved over the sessions a
        // transcript exists for; across the whole table these rows are 93.0% of
        // them, and they are the difference between the 37.1% `omni stats`
        // published and the 8.3% that is true.
        //
        // Corrected in the column rather than in the queries. Six call sites sum
        // these columns, and a `CASE` in each is one rule in six copies waiting to
        // drift, which this repo has already paid for once. Nothing is lost:
        // `execution_traces` still carries `raw_input` and `distilled_output` for
        // these rows, so what the distiller produced is still readable.
        //
        // A redaction is the one passthrough that does send a reply, and no column
        // records that it happened, so those rows are zeroed here too. That
        // undercounts a rare case by the few bytes `[REDACTED]` saves, which is the
        // safe direction to be wrong in.
        let migration_passthrough = "2026_08_passthrough_rows_book_no_saving";
        let passthrough_applied: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM schema_migrations WHERE id = ?1 LIMIT 1",
                params![migration_passthrough],
                |row| row.get(0),
            )
            .optional()?;
        if passthrough_applied.is_none() {
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "UPDATE distillations \
                 SET output_bytes = input_bytes, \
                     delivered_bytes = input_bytes, \
                     filtered_tokens = raw_tokens \
                 WHERE route = 'Passthrough' AND output_bytes < input_bytes",
                [],
            )?;
            tx.execute(
                "INSERT INTO schema_migrations (id, applied_at) VALUES (?1, ?2)",
                params![migration_passthrough, chrono::Utc::now().timestamp()],
            )?;
            tx.commit()?;
        }

        // #605. #603 fixed `filter_name` going forward and left every row already
        // written, so `omni stats` and every workload query still group 16% of
        // this corpus under an assignment or a binary's full path. 2,892 of
        // 17,402 rows across 398 names on the machine that measured it, and they
        // kept accumulating until 0.7.7 was installed, because merging a fix does
        // not change the binary on disk.
        //
        // Not the #163 shape, which is what makes this cheap. There the rows
        // recorded a saving that never reached an agent, no arithmetic could
        // recover the truth, and the answer was to exclude them by timestamp and
        // delete nothing. Here every column is right except a derived one, the
        // column it derives from is intact on all of them, and `producer_label`
        // is pure. So the recompute is arithmetic rather than a judgement, and it
        // needs no timestamp constant: run over a row written after the fix it
        // returns that row's own label.
        //
        // Ordered after #677 deliberately. Recomputing before that landed would
        // have turned `A=$(ls` into `-1t`, destroying the original label to write
        // something no more correct.
        let migration_labels = "2026_08_recompute_filter_name_from_command";
        let labels_applied: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM schema_migrations WHERE id = ?1 LIMIT 1",
                params![migration_labels],
                |row| row.get(0),
            )
            .optional()?;
        if labels_applied.is_none() {
            let tx = conn.unchecked_transaction()?;
            let stale: Vec<(i64, String)> = {
                let mut stmt = tx.prepare(
                    "SELECT id, command FROM distillations
                     WHERE command IS NOT NULL AND command != ''
                       AND (filter_name LIKE '%=%' OR filter_name LIKE '/%')",
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?;
                rows.flatten().collect()
            };
            for (id, command) in stale {
                let label = crate::pipeline::producer::producer_label(&command);
                tx.execute(
                    "UPDATE distillations SET filter_name = ?1 WHERE id = ?2",
                    params![label, id],
                )?;
            }
            tx.execute(
                "INSERT INTO schema_migrations (id, applied_at) VALUES (?1, ?2)",
                params![migration_labels, chrono::Utc::now().timestamp()],
            )?;
            tx.commit()?;
        }

        // `context_turns` was written on every hooked command, carried an index,
        // and had no `SELECT` anywhere in the tree: 5,532 rows paying write
        // latency and disk for a reader that never existed (#270). The in-memory
        // `SessionState::current_turn` it was built from is still read, by
        // `omni_context_breakdown` and `omni stats`; only the table is gone.
        // `verification_results` had no writer and no reader anywhere in the
        // tree, which is as dead as a table gets (#165).
        let migration_drop_ver = "2026_08_drop_unused_verification_results";
        let ver_applied: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM schema_migrations WHERE id = ?1 LIMIT 1",
                params![migration_drop_ver],
                |row| row.get(0),
            )
            .optional()?;
        if ver_applied.is_none() {
            let tx = conn.unchecked_transaction()?;
            tx.execute("DROP TABLE IF EXISTS verification_results", [])?;
            tx.execute(
                "INSERT INTO schema_migrations (id, applied_at) VALUES (?1, ?2)",
                params![migration_drop_ver, chrono::Utc::now().timestamp()],
            )?;
            tx.commit()?;
        }

        // Unconditional, not migration-gated. Gated, the drop ran once, recorded
        // itself as applied, and then a concurrently installed older binary
        // recreated the table 12 seconds later. The migration never re-runs, so
        // the table survived its own removal permanently (#379). A mixed-version
        // machine is the normal case during an upgrade, and `DROP TABLE IF
        // EXISTS` against a table that is already gone costs a catalogue lookup.
        conn.execute("DROP INDEX IF EXISTS idx_ctx_session", [])?;
        conn.execute("DROP TABLE IF EXISTS context_turns", [])?;

        let migration_id2 = "2026_05_token_backfill";
        let already_applied2: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM schema_migrations WHERE id = ?1 LIMIT 1",
                params![migration_id2],
                |row| row.get(0),
            )
            .optional()?;
        if already_applied2.is_none() {
            let tx = conn.unchecked_transaction()?;
            // Use 3.8 chars/token (~4 bytes/token heuristic) for backfill
            tx.execute(
                "UPDATE distillations SET raw_tokens = input_bytes / 4, filtered_tokens = output_bytes / 4 WHERE raw_tokens = 0",
                [],
            )?;
            tx.execute(
                "INSERT INTO schema_migrations (id, applied_at) VALUES (?1, ?2)",
                params![migration_id2, chrono::Utc::now().timestamp()],
            )?;
            tx.commit()?;
        }

        // One-time data migration: collapse distillations that were recorded
        // twice for one command (#118).
        //
        // On the reporting installation 1,231 of 8,272 rows are second copies -
        // 15% of the table, inflating every count and every top-command tally
        // `omni stats` publishes. They stop at 2026-07-17 and 1,229 of them are
        // `aider`, so whatever produced them is long closed; feeding one payload
        // through the post-hook on this build yields exactly one row. This
        // migration is therefore about the history, not about the write path.
        //
        // The key is every column except `id` and `latency_ms`. Measured on that
        // data, `latency_ms` is the *only* column that ever varies inside a
        // duplicate group, `session_id`, `project_path` and the byte counts
        // never do, which is the fingerprint of the same input distilled twice
        // rather than of two separate commands. Grouping on it as well would
        // leave 947 of the 1,231 in place.
        //
        // A user really could run one command twice inside the same second and
        // get byte-identical input and output. Collapsing that pair costs one
        // under-counted row, and under-counting is the direction this project
        // errs in when it has to choose.
        //
        // Deliberately no UNIQUE index behind this. The write that produced the
        // duplicates no longer happens, and an index over these columns would
        // reject the legitimate case above from then on, trading a closed bug
        // for a permanent silent under-count.
        let migration_id3 = "2026_07_dedupe_double_recorded_distillations";
        let already_applied3: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM schema_migrations WHERE id = ?1 LIMIT 1",
                params![migration_id3],
                |row| row.get(0),
            )
            .optional()?;
        if already_applied3.is_none() {
            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "DELETE FROM distillations WHERE id NOT IN (
                     SELECT MIN(id) FROM distillations
                     GROUP BY session_id, ts, filter_name, input_bytes, output_bytes,
                              route, score, context_score, rewind_hash, command,
                              project_path, agent_id, collapse_original, collapse_to,
                              raw_tokens, filtered_tokens, delivered_bytes
                 )",
                [],
            )?;
            tx.execute(
                "INSERT INTO schema_migrations (id, applied_at) VALUES (?1, ?2)",
                params![migration_id3, chrono::Utc::now().timestamp()],
            )?;
            tx.commit()?;
        }

        Ok(())
    }

    /// Row counts used by the hook-routing integration tests.
    ///
    /// Exposed so a test can assert what a hook wrote without shelling out to
    /// the `sqlite3` binary, which is not guaranteed on every CI runner.
    pub fn session_summary_count(&self) -> usize {
        self.count_rows("session_summaries")
    }

    pub fn session_count(&self) -> usize {
        self.count_rows("sessions")
    }

    fn count_rows(&self, table: &str) -> usize {
        let Ok(conn) = self.pool.get() else {
            return 0;
        };
        let sql = match table {
            "session_summaries" => "SELECT COUNT(*) FROM session_summaries",
            "sessions" => "SELECT COUNT(*) FROM sessions",
            _ => return 0,
        };
        conn.query_row(sql, [], |r| r.get::<_, i64>(0))
            .map(|n| n as usize)
            .unwrap_or(0)
    }

    pub fn record_distillation(
        &self,
        session_id: &str,
        result: &DistillResult,
        command: &str,
        project_path: &str,
        agent_id: &str,
    ) {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return,
        };

        let ts = chrono::Utc::now().timestamp();
        let (col_orig, col_to) = result.collapse_savings.unwrap_or((0, 0));
        let res = conn.execute(
            "INSERT INTO distillations 
             (session_id, ts, filter_name, input_bytes, output_bytes, route, score, context_score, latency_ms, rewind_hash, command, collapse_original, collapse_to, project_path, agent_id, raw_tokens, filtered_tokens, delivered_bytes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                session_id,
                ts,
                result.filter_name,
                result.input_bytes,
                result.output_bytes,
                result.route.to_string(),
                result.score,
                result.context_score,
                result.latency_ms,
                result.rewind_hash.as_deref().unwrap_or(""),
                command,
                col_orig as i64,
                col_to as i64,
                project_path,
                agent_id,
                result.raw_tokens as i64,
                result.filtered_tokens as i64,
                result.delivered_bytes as i64,
            ],
        );

        if let Err(e) = res {
            // Log to stderr for visibility during development/debugging
            eprintln!("[omni:error] failed to record distillation: {}", e);
            if e.to_string().contains("NOT NULL constraint failed") {
                eprintln!(
                    "[omni:error] hint: legacy 'content_type' column may be blocking inserts. OMNI will attempt auto-migration on next startup."
                );
            }
        }
    }

    pub fn record_trace(
        &self,
        session_id: &str,
        command: &str,
        agent_id: &str,
        project_path: &str,
        raw_input: &str,
        distilled_output: &str,
    ) {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return,
        };

        let ts = chrono::Utc::now().timestamp();
        let res = conn.execute(
            "INSERT INTO execution_traces 
             (session_id, ts, command, agent_id, project_path, raw_input, distilled_output)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                session_id,
                ts,
                command,
                agent_id,
                project_path,
                raw_input,
                distilled_output,
            ],
        );

        if let Err(e) = res {
            eprintln!("[omni:error] failed to record trace: {}", e);
        }
    }

    pub fn record_unhandled_tool(&self, tool_name: &str) {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return,
        };
        let now = chrono::Utc::now().timestamp();
        let _ = conn.execute(
            "INSERT INTO unhandled_tools (tool_name, count, last_seen) VALUES (?1, 1, ?2)
             ON CONFLICT(tool_name) DO UPDATE SET count = count + 1, last_seen = excluded.last_seen",
            params![tool_name, now],
        );
    }

    /// Records that a payload was handed back untouched, and why.
    ///
    /// The reason used to be tagged onto the end of `command` because there was
    /// no column for it (#254), which made the largest population in the corpus
    /// readable only by string matching on the thing it was concatenated to.
    /// `reason` is its own column now, so `GROUP BY reason` answers the question
    /// the tagging was invented for (#441): "declined because the payload was
    /// JSON" and "declined because no distiller could parse it" call for
    /// opposite work, and the second is the only direct evidence that the
    /// never-fabricate invariant is doing any.
    pub fn record_passthrough(
        &self,
        command: &str,
        bytes: usize,
        reason: &str,
        session: &str,
        agent_id: &str,
    ) {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return,
        };
        let now = chrono::Utc::now().timestamp();
        let _ = conn.execute(
            "INSERT INTO passthrough_events (command, bytes, ts, reason, session, agent_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![command, bytes as i64, now, reason, session, agent_id],
        );
    }

    /// What each door declined, in bytes, over the last `days`.
    ///
    /// The question `passthrough_events` could not answer until #773: what share
    /// of what OMNI declines was declined on behalf of a model. `session` is not
    /// that test, since a hook payload can arrive without one, so this groups by
    /// the agent the row was recorded for.
    ///
    /// Rows written before the column read `unknown`, which is why the window
    /// matters: a query over all time is mostly a report about a column that did
    /// not exist yet.
    pub fn passthrough_bytes_by_agent(&self, days: i64) -> Vec<(String, i64, i64)> {
        let Ok(conn) = self.pool.get() else {
            return Vec::new();
        };
        let Ok(mut stmt) = conn.prepare(
            "SELECT agent_id, COUNT(*), SUM(bytes)
               FROM passthrough_events
              WHERE ts >= strftime('%s','now') - (?1 * 86400)
              GROUP BY agent_id
              ORDER BY 3 DESC",
        ) else {
            return Vec::new();
        };
        let rows = stmt.query_map(params![days], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2).unwrap_or(0)))
        });
        rows.map(|rs| rs.filter_map(Result::ok).collect())
            .unwrap_or_default()
    }

    /// How many payloads each gate declined, newest window first.
    ///
    /// The point of the column: most calls are passthrough and correctly do
    /// nothing, and until this existed that population was one bucket.
    pub fn passthrough_reasons(&self, since: i64) -> Vec<(String, u64)> {
        self.passthrough_reasons_for(since, None)
    }

    /// One session's declines, or every session's when `session` is `None`.
    ///
    /// `Some("")` returns nothing. An empty id means the host sent none, so it is
    /// shared by every pipe-mode row in the table and cannot identify a session.
    pub fn passthrough_reasons_for(&self, since: i64, session: Option<&str>) -> Vec<(String, u64)> {
        // An empty id is not a session, it is the host declining to name one, so
        // every pipe-mode row from every session carries it. Falling through to
        // the unscoped query here answered "what did this session decline" with
        // every session's declines, which is worse than not answering.
        if matches!(session, Some("")) {
            return Vec::new();
        }
        let Ok(conn) = self.pool.get() else {
            return Vec::new();
        };
        let scoped = session.is_some();
        let sql = if scoped {
            "SELECT reason, COUNT(*) FROM passthrough_events
             WHERE ts >= ?1 AND session = ?2 GROUP BY reason ORDER BY 2 DESC"
        } else {
            "SELECT reason, COUNT(*) FROM passthrough_events
             WHERE ts >= ?1 GROUP BY reason ORDER BY 2 DESC"
        };
        let Ok(mut stmt) = conn.prepare(sql) else {
            return Vec::new();
        };
        let row = |r: &rusqlite::Row<'_>| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64));
        if scoped {
            stmt.query_map(params![since, session.unwrap_or("")], row)
                .map(|rows| rows.flatten().collect())
                .unwrap_or_default()
        } else {
            stmt.query_map(params![since], row)
                .map(|rows| rows.flatten().collect())
                .unwrap_or_default()
        }
    }

    /// Newest first. Exists so the tag above can be asserted; nothing in the
    /// shipped binary reads this table yet.
    pub fn recent_passthroughs(&self, limit: usize) -> Vec<String> {
        let Ok(conn) = self.pool.get() else {
            return Vec::new();
        };
        let Ok(mut stmt) =
            conn.prepare("SELECT command FROM passthrough_events ORDER BY id DESC LIMIT ?1")
        else {
            return Vec::new();
        };
        stmt.query_map(params![limit as i64], |r| r.get::<_, String>(0))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    /// Archives `content` under its own hash and returns the key, or `None` when
    /// the row did not land.
    ///
    /// The key is the content address and nothing else. It used to carry a
    /// nanosecond prefix, `{ts_ns}_{hash}`, which made every key unique and so
    /// made the `INSERT OR IGNORE` beneath it decoration: the same output stored
    /// a fresh copy on every run. That was invisible while the archive never
    /// fired, and became a live disk cost the moment #271 started archiving on
    /// every lossy call (#274).
    ///
    /// A repeat refreshes `ts` rather than being ignored, so content that is
    /// still being produced does not age out of the retention window on the
    /// strength of when it was first seen. `retrieved` is left alone.
    ///
    /// The return used to be a bare `String` handed back on every path,
    /// including a pool-get failure and a swallowed `execute`. The caller then
    /// printed `omni_retrieve("<key>")` for content that is not in the table,
    /// which is the one promise this store exists to keep (#388). Rare while the
    /// archive fired on 139 of 14,962 calls; routine once the ledger records
    /// every emitted block.
    /// `whole_len` is the length of the payload `content` was cut out of, which
    /// is `content.len()` when the handle holds an entire output. It is the only
    /// thing that lets `omni retrieve` tell a reader whether it is holding the
    /// whole answer or one block of it (#627). Before this it was written as
    /// `content.len()` unconditionally, so the column duplicated the content
    /// length and could never answer that.
    pub fn store_rewind(&self, content: &str, whole_len: usize) -> Option<String> {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        // The reserved example handle must never name real content, or the
        // documented "an example never resolves" check would be wrong for
        // whichever payload happened to hash to it (#583).
        let rewind_key = crate::util::text::avoid_example_handle(
            crate::util::text::safe_slice(&hex::encode(hasher.finalize()), 16).to_string(),
        );

        let conn = self.pool.get().ok()?;

        let ts = chrono::Utc::now().timestamp();
        // A caller that understates the whole would make `retrieve` claim
        // completeness for a fragment, which is the defect being fixed, so the
        // floor is the content itself rather than trust.
        let original_len = whole_len.max(content.len()) as i64;

        conn.execute(
            "INSERT INTO rewind_store (hash, content, ts, original_len, retrieved)
             VALUES (?1, ?2, ?3, ?4, 0)
             -- One row per content hash, so the same block can arrive from two
             -- different wholes. Keep the larger: understating completeness
             -- costs a reader one check, overstating it is the bug (#627).
             ON CONFLICT(hash) DO UPDATE SET ts = excluded.ts,
                 original_len = MAX(rewind_store.original_len, excluded.original_len)",
            params![rewind_key, content, ts, original_len],
        )
        .ok()?;

        Some(rewind_key)
    }

    /// Records a handle pulled back into an agent's context, whichever door it
    /// came through.
    ///
    /// `get_retrieve_rate` reads what this writes and raises the route
    /// thresholds when a command family keeps needing its full output. The CLI
    /// was not writing it, and the CLI is the door the mechanism advertises:
    /// the marker prints `omni retrieve <handle>`, a shell command. Measured on
    /// the maintainer's install before the fix, 49 pulls counted on
    /// `rewind_store.retrieved` against 19 rows here (#512).
    ///
    /// Deliberately **not** inside `retrieve_rewind`. `store::query` reads
    /// archived content to answer reports, and counting those would inflate the
    /// rate with reads no agent asked for, which is the same defect in the
    /// other direction.
    pub fn record_rewind_pull(&self, hash: &str) {
        // #581. The debt is booked here and not in `retrieve_rewind`, for the
        // same reason the counter above is: `store::query` reads archived
        // content to answer reports (`query.rs:122` and `:174`), and a report
        // is not a reader asking for its bytes back. Booking it there created
        // debt nobody incurred and spent the next matching delivery paying it.
        // Review on #593 caught that, against a comment two paragraphs up that
        // says exactly this about the other counter.
        //
        // A count rather than a flag, so two readers pulling the same handle
        // before either is answered earn one verbatim delivery each instead of
        // the first clearing it for both.
        if let Ok(conn) = self.pool.get() {
            let _ = conn.execute(
                "UPDATE rewind_store SET owed = owed + 1 WHERE hash = ?1",
                params![hash],
            );
        }
        let cmd = self
            .find_command_for_hash(hash)
            .unwrap_or_else(|| "unknown".to_string());
        let agent_id = std::env::var("OMNI_AGENT_ID")
            .unwrap_or_else(|_| crate::agents::multiagent::detect_agent_id());
        let family = crate::util::command_family::command_family(&cmd);
        self.record_retrieve_event(&family, hash, &agent_id);
    }

    /// Consumes the debt a pull left behind, and says whether there was one.
    ///
    /// #581. A marker tells the reader to run `omni retrieve <handle>`. Whichever
    /// way those bytes come back, through a file it then reads or through the
    /// command's own output, they pass the hook again and hash the same, so the
    /// ledger replaced them with the very marker that sent the reader there. An
    /// agent following the instruction got the instruction back.
    ///
    /// One delivery, not a permanent exemption. Once the reader has the content
    /// the ledger's claim is true again and the next repeat folds normally,
    /// which is why this clears the flag as it reads it. The `UPDATE` does both
    /// in one statement so two hooks racing cannot each be told they are the one
    /// answering the pull.
    pub fn take_owed_delivery(&self, hash: &str) -> bool {
        let Ok(conn) = self.pool.get() else {
            return false;
        };
        conn.execute(
            "UPDATE rewind_store SET owed = owed - 1 WHERE hash = ?1 AND owed > 0",
            params![hash],
        )
        .map(|changed| changed > 0)
        .unwrap_or(false)
    }

    /// Archive a payload that is the whole of what a marker replaced. Test-only:
    /// production callers state the whole explicitly, because the interesting
    /// case is the one where it differs.
    #[cfg(test)]
    pub fn store_rewind_whole(&self, content: &str) -> Option<String> {
        self.store_rewind(content, content.len())
    }

    pub fn retrieve_rewind(&self, hash: &str) -> Option<String> {
        self.retrieve_rewind_sized(hash).map(|(content, _)| content)
    }

    /// The archived block and the size of the payload it was cut out of. Equal
    /// when the handle holds a whole output; `whole_len` is larger when it holds
    /// one block of one, which is what a ledger fold archives (#627).
    pub fn retrieve_rewind_sized(&self, hash: &str) -> Option<(String, usize)> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return None,
        };

        let content: Option<(String, i64)> = conn
            .query_row(
                "SELECT content, original_len FROM rewind_store WHERE hash = ?1",
                params![hash],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .unwrap_or(None);

        if content.is_some() {
            let _ = conn.execute(
                "UPDATE rewind_store SET retrieved = retrieved + 1 WHERE hash = ?1",
                params![hash],
            );
        }

        content.map(|(c, whole)| {
            let whole = usize::try_from(whole).unwrap_or(0).max(c.len());
            (c, whole)
        })
    }

    /// Get recent distillation rows for omni_history MCP tool
    /// Recent distillations, optionally narrowed to one session.
    ///
    /// `None` is the honest answer from the MCP server. `SessionState::new` mints
    /// `{millis}-{pid}-{counter}` every time it is constructed, while the hook
    /// path keys its rows on the **host** session id: 11,281 rows on this machine
    /// carry a host UUID against 892 minted ones. The server filtering on its own
    /// id therefore matched nothing in a Claude Code session, which is most of
    /// what `omni_explain_savings` was reporting when it said "No recent
    /// distillations" (#602). The server cannot see the host id, so it asks by
    /// recency and says so, rather than filtering on a value known to be wrong.
    pub fn get_recent_distillations(
        &self,
        session_id: Option<&str>,
        limit: usize,
    ) -> Vec<DistillationRow> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        let mut stmt = match conn.prepare(
            "SELECT command, input_bytes, output_bytes, route, filter_name
             FROM distillations WHERE (?1 IS NULL OR session_id = ?1)
             ORDER BY ts DESC LIMIT ?2",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        match stmt.query_map(params![session_id, limit as i64], |row| {
            Ok(DistillationRow {
                command: row.get(0)?,
                input_bytes: row.get::<_, i64>(1)? as usize,
                output_bytes: row.get::<_, i64>(2)? as usize,
                route: row.get(3)?,
                filter_name: row.get(4)?,
            })
        }) {
            Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
            Err(_) => vec![],
        }
    }

    /// Recent cross-turn folds for a session, which `distillations` never holds.
    ///
    /// The ledger writes its own table, and `omni_explain_savings` read only
    /// `distillations`, so a route that fired and handed back a live retrieve
    /// handle was reported as "No recent distillations found in current session"
    /// (#602). Counted on the machine that confirmed it: 936 rows in
    /// `ledger_folds`, none of them visible to the tool.
    ///
    /// Keyed on the fold's own `session` column and not on `scope`. `scope` is
    /// deliberately the **project** whenever there is one, so the cross-agent
    /// question #533 exists to answer stays askable, and every one of the 936
    /// rows on the machine that confirmed this bug holds a filesystem path there.
    /// A first draft of this queried `scope` against the session id and returned
    /// nothing in production while its test passed, because the test wrote rows
    /// by hand instead of through `Ledger::project`. Caught in review of #625.
    pub fn get_recent_ledger_folds(
        &self,
        session_id: Option<&str>,
        limit: usize,
    ) -> Vec<LedgerFoldRow> {
        let Ok(conn) = self.pool.get() else {
            return vec![];
        };
        let Ok(mut stmt) = conn.prepare(
            "SELECT origin, lines, bytes, whole_output
             FROM ledger_folds
             WHERE (?1 IS NULL OR session = ?1)
             ORDER BY ts DESC, id DESC LIMIT ?2",
        ) else {
            return vec![];
        };
        match stmt.query_map(params![session_id, limit as i64], |row| {
            Ok(LedgerFoldRow {
                origin: row.get(0)?,
                lines: row.get::<_, i64>(1)? as usize,
                bytes: row.get::<_, i64>(2)? as usize,
                whole_output: row.get::<_, i64>(3)? != 0,
            })
        }) {
            Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
            Err(_) => vec![],
        }
    }

    pub fn delete_session(&self, id: &str) -> Result<()> {
        let conn = self.pool.get().context("DB pool exhausted")?;
        conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn list_recent_sessions(&self, limit: usize) -> Result<Vec<SessionState>> {
        let conn = self.pool.get().context("DB pool exhausted")?;
        let mut stmt =
            conn.prepare("SELECT state_json FROM sessions ORDER BY last_active DESC LIMIT ?1")?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            let json_str: String = row.get(0)?;
            Ok(json_str)
        })?;

        let mut out = Vec::new();
        for r in rows {
            if let Ok(j) = r
                && let Ok(s) = serde_json::from_str::<SessionState>(&j)
            {
                out.push(s);
            }
        }
        Ok(out)
    }

    pub fn upsert_session(&self, state: &SessionState) {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return,
        };

        let state_json = serde_json::to_string(state).unwrap_or_else(|_| "{}".to_string());
        let _ = conn.execute(
            "INSERT OR REPLACE INTO sessions (id, started_at, last_active, task_hint, domain_hint, state_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                state.session_id,
                state.started_at,
                state.last_active,
                state.inferred_task.as_deref().unwrap_or(""),
                state.inferred_domain.as_deref().unwrap_or(""),
                state_json,
            ],
        );
    }

    /// The most recent session **this agent had in this project**.
    ///
    /// `sessions` has no project column and its id is a wall-clock stamp, so the
    /// project comes from `agent_sessions`, which is already keyed
    /// `(agent_id, project_hash)` and already written on every session start and
    /// end. Joining is what lets a session be found for one repository without
    /// giving `sessions` a schema it does not have.
    ///
    /// Returning `None` for a project this agent has never worked in is the
    /// answer, not a miss: the caller then starts fresh, which is what should
    /// happen the first time you open a repository (#482).
    pub fn find_latest_session_for_project(
        &self,
        agent_id: &str,
        project_hash: &str,
    ) -> Option<SessionState> {
        let conn = self.pool.get().ok()?;
        let state_json: Option<String> = conn
            .query_row(
                "SELECT s.state_json
                 FROM sessions s
                 JOIN agent_sessions a ON a.session_id = s.id
                 WHERE a.agent_id = ?1 AND a.project_hash = ?2
                 ORDER BY s.last_active DESC
                 LIMIT 1",
                params![agent_id, project_hash],
                |row| row.get(0),
            )
            .optional()
            .unwrap_or(None);

        state_json.and_then(|json| serde_json::from_str(&json).ok())
    }

    pub fn find_latest_session(&self) -> Option<SessionState> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return None,
        };

        let state_json: Option<String> = conn
            .query_row(
                "SELECT state_json FROM sessions ORDER BY last_active DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .unwrap_or(None);

        if let Some(json) = state_json {
            serde_json::from_str(&json).ok()
        } else {
            None
        }
    }

    pub fn index_event(&self, session_id: &str, event_type: &str, content: &str) {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return,
        };

        let ts = chrono::Utc::now().timestamp();
        let _ = conn.execute(
            "INSERT INTO session_events (session_id, event_type, content, ts) VALUES (?1, ?2, ?3, ?4)",
            params![session_id, event_type, content, ts],
        );
    }

    pub fn search_session_events(
        &self,
        session_id: &str,
        query: &str,
        limit: usize,
    ) -> Vec<String> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return vec![],
        };

        let mut stmt = match conn.prepare(
            "SELECT content FROM session_events 
             WHERE session_id = ?1 AND session_events MATCH ?2 
             ORDER BY rank LIMIT ?3",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };

        let event_iter = match stmt.query_map(params![session_id, query, limit], |row| row.get(0)) {
            Ok(iter) => iter,
            Err(_) => return vec![],
        };

        let mut results = Vec::new();
        for content in event_iter.flatten() {
            results.push(content);
        }
        results
    }

    /// Per-command totals in the window, most-called first.
    ///
    /// `limit` of 0 means every class. The cap is by call count, so any caller
    /// ranking on something else is reading a sample chosen by a different
    /// measure: a class that removed 40 MB in three calls sits below 300 chatty
    /// ones and never reaches the sort. `omni context --tokens` ranks by bytes
    /// and so asks for all of them.
    #[allow(clippy::type_complexity)]
    pub fn get_per_command_stats(
        &self,
        since: i64,
        limit: usize,
    ) -> Result<Vec<(String, u64, u64, u64, u64, u64)>> {
        let conn = self.pool.get().context("DB pool exhausted")?;
        let mut stmt = conn.prepare(&format!(
            "SELECT
                command,
                COUNT(*) as calls,
                COALESCE(SUM(input_bytes), 0) as total_input,
                COALESCE(SUM(output_bytes), 0) as total_output,
                COALESCE(SUM(raw_tokens), 0) as raw_tok,
                COALESCE(SUM(filtered_tokens), 0) as filt_tok
            FROM distillations
            WHERE ts >= ?1 AND command != '' AND command != '[pipe]' AND {}
            GROUP BY command
            ORDER BY calls DESC
            LIMIT ?2",
            applied_only()
        ))?;

        let rows = stmt
            // SQLite reads a negative LIMIT as no limit.
            .query_map(
                params![since, if limit == 0 { -1 } else { limit as i64 }],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, u64>(1)?,
                        row.get::<_, u64>(2)?,
                        row.get::<_, u64>(3)?,
                        row.get::<_, u64>(4)?,
                        row.get::<_, u64>(5)?,
                    ))
                },
            )?
            .filter_map(|r| r.ok())
            .collect();

        Ok(rows)
    }

    /// Agent breakdown: (agent_id, count, input_bytes, output_bytes)
    /// Per-agent totals, with the unapplied rows counted but not added in (#163).
    ///
    /// `calls` / `input_bytes` / `output_bytes` cover only rows that reached an
    /// agent. `unverified` counts the ones excluded, pre-#158 Claude Code rows
    /// whose savings the host discarded, and rows whose result no model read
    /// (#212). They are reported rather than dropped so a shrinking call count
    /// reads as the correction it is, instead of looking like OMNI stopped
    /// working. It is `SUM(NOT (applied))` against the same predicate, so a new
    /// exclusion is surfaced by construction rather than by remembering to.
    pub fn get_agent_breakdown(&self, since: i64) -> Result<Vec<AgentRow>> {
        let conn = self.pool.get().context("DB pool exhausted")?;
        let applied = applied_only();
        let mut stmt = conn.prepare(&format!(
            "SELECT
                COALESCE(agent_id, 'unknown') as agent,
                COALESCE(SUM({applied}), 0) as calls,
                COALESCE(SUM(CASE WHEN {applied} THEN input_bytes ELSE 0 END), 0) as total_input,
                COALESCE(SUM(CASE WHEN {applied} THEN output_bytes ELSE 0 END), 0) as total_output,
                COALESCE(SUM(NOT ({applied})), 0) as unverified
            FROM distillations
            WHERE ts >= ?1
            GROUP BY agent
            ORDER BY calls DESC"
        ))?;

        let rows = stmt
            .query_map(params![since], |row| {
                Ok(AgentRow {
                    agent_id: row.get(0)?,
                    calls: row.get(1)?,
                    input_bytes: row.get(2)?,
                    output_bytes: row.get(3)?,
                    unverified: row.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(rows)
    }

    /// Per-command stats with agent_id: (command, agent_id, count, input_bytes, output_bytes)
    #[allow(clippy::type_complexity)]
    /// Every `(command, agent)` pair in the window, unaggregated.
    ///
    /// There is no row limit, and removing the `LIMIT 200` this carried is the
    /// point. The single caller folds these into shortened command keys before
    /// displaying them, so a cap applied to *raw* commands cut the rows that the
    /// fold would have summed: 1,064 pairs over 1,099 distillations, of which
    /// the cap kept 200. `node -e` is 21 separate one-call rows that add up to a
    /// displayed `21x`, and every one of them sat below the cut, so the row was
    /// shown with no agent at all (#471).
    ///
    /// The bound is the retention window rather than a row count, which is the
    /// honest one here: this is a report command, not a hook.
    pub fn get_per_command_with_agent(
        &self,
        since: i64,
    ) -> Result<Vec<(String, String, u64, u64, u64)>> {
        let conn = self.pool.get().context("DB pool exhausted")?;
        let mut stmt = conn.prepare(&format!(
            "SELECT
                CASE WHEN command != '' THEN command ELSE '[unknown command]' END as command_name,
                COALESCE(agent_id, 'unknown') as agent,
                COUNT(*) as calls,
                COALESCE(SUM(input_bytes), 0) as total_input,
                COALESCE(SUM(output_bytes), 0) as total_output
            FROM distillations
            WHERE ts >= ?1 AND command != '[pipe]' AND {}
            GROUP BY command_name, agent
            ORDER BY calls DESC",
            applied_only()
        ))?;

        let rows = stmt
            .query_map(params![since], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u64>(2)?,
                    row.get::<_, u64>(3)?,
                    row.get::<_, u64>(4)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(rows)
    }

    /// Multi-period stats: Vec of (period_label, count, input_bytes, output_bytes, raw_tokens, filtered_tokens)
    #[allow(clippy::type_complexity)]
    pub fn multi_period_stats(&self) -> Result<Vec<(String, u64, u64, u64, u64, u64)>> {
        let now = chrono::Utc::now().timestamp();
        let midnight = now - (now % 86400);
        let week_ago = now - 7 * 86400;

        let mut periods = Vec::new();
        for (label, since) in [
            ("Today", midnight),
            ("This Week", week_ago),
            ("All Time", 0i64),
        ] {
            let (count, input, output, _, _, raw_tok, filt_tok) = self.aggregate_stats(since)?;
            periods.push((label.to_string(), count, input, output, raw_tok, filt_tok));
        }
        Ok(periods)
    }

    pub fn get_project_stats(&self, since: i64) -> Result<Vec<(String, u64, f64)>> {
        let conn = self.pool.get().context("DB pool exhausted")?;
        let mut stmt = conn.prepare(&format!(
            "SELECT
                project_path,
                COUNT(*) as count,
                CASE 
                    WHEN SUM(input_bytes) = 0 THEN 0.0 
                    ELSE ROUND(100.0 * (1.0 - CAST(SUM(output_bytes) AS REAL) / SUM(input_bytes)), 1)
                END as savings
             FROM distillations
             WHERE ts >= ?1 AND project_path != '' AND {}
             GROUP BY project_path
             ORDER BY count DESC",
            applied_only()
        ))?;

        let rows = stmt
            .query_map(params![since], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(rows)
    }

    // ─── Loop Memory (L2-02) ────────────────────────────────

    /// Set or update a loop memory entry. On conflict, bumps times_confirmed and updates value.
    pub fn loop_memory_set(&self, goal_hash: &str, key: &str, value: &str, confidence: f64) {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return,
        };
        let now = chrono::Utc::now().timestamp();
        let _ = conn.execute(
            "INSERT INTO loop_memory (loop_goal_hash, key, value, confidence, times_confirmed, first_seen, last_seen)
             VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5)
             ON CONFLICT(loop_goal_hash, key) DO UPDATE SET
                value = excluded.value,
                confidence = excluded.confidence,
                times_confirmed = times_confirmed + 1,
                last_seen = excluded.last_seen",
            params![goal_hash, key, value, confidence, now],
        );
    }

    /// Get a single loop memory entry.
    pub fn loop_memory_get(&self, goal_hash: &str, key: &str) -> Option<(String, f64, i64)> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return None,
        };
        conn.query_row(
            "SELECT value, confidence, times_confirmed FROM loop_memory WHERE loop_goal_hash = ?1 AND key = ?2",
            params![goal_hash, key],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?, row.get::<_, i64>(2)?)),
        ).optional().unwrap_or(None)
    }

    /// List all loop memory entries for a given goal hash.
    pub fn loop_memory_list(&self, goal_hash: &str) -> Vec<(String, String, f64, i64)> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        let mut stmt = match conn.prepare(
            "SELECT key, value, confidence, times_confirmed FROM loop_memory WHERE loop_goal_hash = ?1 ORDER BY last_seen DESC",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        match stmt.query_map(params![goal_hash], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, f64>(2)?,
                row.get::<_, i64>(3)?,
            ))
        }) {
            Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
            Err(_) => vec![],
        }
    }

    /// Delete a loop memory entry.
    pub fn loop_memory_forget(&self, goal_hash: &str, key: &str) {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return,
        };
        let _ = conn.execute(
            "DELETE FROM loop_memory WHERE loop_goal_hash = ?1 AND key = ?2",
            params![goal_hash, key],
        );
    }

    /// Which of `hashes` this scope has already been shown, and to whom.
    ///
    /// Asked as a bounded `IN` probe rather than by loading the scope's whole
    /// line set, because each hook is its own process: a session with 100,000
    /// recorded lines would pay to read all of them to ask about 200. Chunked
    /// under SQLite's default 32,766 parameter ceiling with room to spare.
    ///
    /// The agent comes back with the hash because a fold cannot otherwise say
    /// whose bytes it replaced, and that is the whole question #533 has to price:
    /// a project-scoped fold drawing on another agent's lines is the reuse that
    /// keying the scope on `(repo, agent)` would end. `INSERT OR IGNORE` keeps
    /// the first writer, so this names the agent actually shown that line rather
    /// than the last one to repeat it.
    ///
    /// Fails to an empty map. A ledger that cannot read is a ledger that has
    /// seen nothing, which costs a missed reduction and never a false claim.
    pub fn ledger_seen(
        &self,
        scope: &str,
        hashes: &[String],
        not_older_than: i64,
    ) -> std::collections::HashMap<String, SeenLine> {
        let mut found = std::collections::HashMap::new();
        let Ok(conn) = self.pool.get() else {
            return found;
        };
        // Distinct first: a payload that repeats a line inside itself would
        // otherwise ask about it once per occurrence.
        let unique: Vec<&String> = hashes
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();

        for chunk in unique.chunks(500) {
            let placeholders = std::iter::repeat_n("?", chunk.len())
                .collect::<Vec<_>>()
                .join(",");
            let sql = format!(
                "SELECT l.line_hash, l.agent_id, COALESCE(s.source, l.source) \
                   FROM ledger_lines l LEFT JOIN ledger_sources s ON s.id = l.source_id \
                  WHERE l.scope = ? AND l.ts >= ? AND l.line_hash IN ({placeholders})"
            );
            let Ok(mut stmt) = conn.prepare_cached(&sql) else {
                continue;
            };
            let params: Vec<&dyn rusqlite::ToSql> = [
                &scope as &dyn rusqlite::ToSql,
                &not_older_than as &dyn rusqlite::ToSql,
            ]
            .into_iter()
            .chain(chunk.iter().map(|h| *h as &dyn rusqlite::ToSql))
            .collect();
            if let Ok(rows) = stmt.query_map(params.as_slice(), |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    SeenLine {
                        agent: r.get::<_, String>(1)?,
                        source: r.get::<_, String>(2)?,
                    },
                ))
            }) {
                found.extend(rows.flatten());
            }
        }
        found
    }

    /// Records every line of an emitted block against a scope.
    ///
    /// Unconditional by design. The old archive gate demanded more than 40% noise
    /// across more than 20 segments and fired on 139 of 14,962 calls, under 1%,
    /// which is the right bound for "was this worth compressing" and the wrong
    /// one for "might the agent see this again". A block is worth remembering
    /// because it may recur.
    ///
    /// One transaction and one cached statement for the whole batch: this is the
    /// loop where `prepare_cached` actually pays, unlike the one-shot statements
    /// a hook process runs once each.
    pub fn ledger_record(&self, scope: &str, hashes: &[String], agent_id: &str, source: &str) {
        let Ok(mut conn) = self.pool.get() else {
            return;
        };
        let ts = chrono::Utc::now().timestamp();
        let Ok(tx) = conn.transaction() else {
            return;
        };
        // The command is stored once and the lines point at it (#911). Its `ts`
        // moves on every call so the prune keeps what the window still uses.
        let source_id: i64 = if source.is_empty() {
            0
        } else {
            // A failed insert records the lines as unrecorded rather than not at all.
            tx.query_row(
                "INSERT INTO ledger_sources (source, ts) VALUES (?1, ?2)
                 ON CONFLICT(source) DO UPDATE SET ts = excluded.ts
                 RETURNING id",
                params![source, ts],
                |r| r.get(0),
            )
            .unwrap_or(0)
        };
        {
            let Ok(mut stmt) = tx.prepare_cached(
                "INSERT INTO ledger_lines (scope, line_hash, ts, agent_id, source_id)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(scope, line_hash) DO UPDATE SET ts = excluded.ts",
            ) else {
                return;
            };
            // The conflict arm keeps the first writer's `agent_id` and `source`, so
            // they still name the command that actually showed the line rather than
            // the last one to repeat it. That is the property the marker's claim
            // rests on (#622).
            //
            // `ts` is the exception and moves, because the freshness cutoff asks
            // when these bytes last reached an agent, not when they first did
            // (#795). Only lines this call delivered in full are passed here, so a
            // line that was folded away cannot refresh its own sighting.
            for h in hashes {
                let _ = stmt.execute(params![scope, h, ts, agent_id, source_id]);
            }
        }
        let _ = tx.commit();
    }

    /// Retrieve rate per marker shape, over the last `days`.
    ///
    /// Joined on the handle, which is the only thing that ties a retrieval to the
    /// marker that caused it. `retrieve_events` predates `fold_markers`, so a
    /// retrieval of a marker written before #766 finds no row here and is absent
    /// rather than counted against some other shape: window the query and read
    /// `folds` before reading the rate.
    ///
    /// `EXISTS` rather than a join, and the difference is not style (#771 review).
    /// A handle is the SHA of the content, so the same bytes folded twice carry
    /// the same handle, and a plain `LEFT JOIN` then multiplies a marker by every
    /// retrieval that ever matched it: `COUNT(*)` stops counting markers and the
    /// sum stops counting pulls. This asks one question per marker instead, which
    /// is "was this content ever fetched after this marker was shown".
    ///
    /// The bound that remains, stated rather than hidden: two markers of the same
    /// content, in different shapes, are both credited by one retrieval. Content
    /// addressing is what makes a handle worth having, and separating those two
    /// would need a per-marker id the retrieval path does not carry.
    ///
    /// `>=` and not `>`, deliberately. Both tables keep whole seconds, so a tie is
    /// ambiguous either way, and the two readings are not equally likely: a reader
    /// follows a handle within seconds of meeting it (#543 recorded four of four
    /// pulls inside nine seconds), while the case `>` would protect against needs
    /// the same content folded twice with a pull in between, inside one second.
    /// `>` would drop the common case to avoid the rare one. Resolving the tie for
    /// real needs sub-second timestamps on both tables (#771 review).
    pub fn marker_retrieve_rates(&self, since: i64) -> Vec<MarkerRate> {
        let Ok(conn) = self.pool.get() else {
            return Vec::new();
        };
        let Ok(mut stmt) = conn.prepare(
            "SELECT m.kind, m.tool, COUNT(*),
                    SUM(EXISTS (SELECT 1 FROM retrieve_events r
                                 WHERE r.hash = m.handle AND r.ts >= m.ts))
               FROM fold_markers m
              WHERE m.ts >= ?1
              GROUP BY m.kind, m.tool
              ORDER BY 4 DESC",
        ) else {
            return Vec::new();
        };
        let rows = stmt.query_map(params![since], |r| {
            Ok(MarkerRate {
                kind: r.get(0)?,
                tool: r.get(1).unwrap_or_default(),
                folds: r.get(2)?,
                retrieved: r.get(3).unwrap_or(0),
            })
        });
        rows.map(|rs| rs.filter_map(Result::ok).collect())
            .unwrap_or_default()
    }

    /// Records what one call's folds drew on, grouped by origin and source agent.
    ///
    /// Written after the view is decided rather than while it is being built, so
    /// a store that cannot be reached costs a measurement and never a marker.
    /// Same reasoning as `ledger_record` above: the ledger's writes are evidence,
    /// and evidence must not be able to change the output.
    pub fn ledger_record_folds(
        &self,
        scope: &str,
        agent_id: &str,
        session: &str,
        tool: &str,
        folds: &[FoldRecord],
        markers: &[MarkerRecord],
    ) {
        if folds.is_empty() {
            return;
        }
        let Ok(mut conn) = self.pool.get() else {
            return;
        };
        let ts = chrono::Utc::now().timestamp();
        let Ok(tx) = conn.transaction() else {
            return;
        };
        // Every row of this call carries it, so reporting can group by fold
        // instead of inferring one from a timestamp (#665). `id` is the rowid,
        // so `MAX` is a lookup rather than a scan, and the value it picks is the
        // id the first row is about to get.
        let fold_id: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(id), 0) + 1 FROM ledger_folds",
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        {
            let Ok(mut stmt) = tx.prepare_cached(
                "INSERT INTO ledger_folds
                     (ts, scope, agent_id, source_agent, origin, lines, bytes, whole_output,
                      payload_bytes, session, fold_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            ) else {
                return;
            };
            for f in folds {
                let _ = stmt.execute(params![
                    ts,
                    scope,
                    agent_id,
                    f.source_agent,
                    f.origin,
                    f.lines as i64,
                    f.bytes as i64,
                    i64::from(f.whole_output),
                    f.payload_bytes as i64,
                    session,
                    fold_id
                ]);
            }
        }
        {
            let Ok(mut stmt) = tx.prepare_cached(
                "INSERT INTO fold_markers
                     (fold_id, ts, scope, session, agent_id, tool, kind, handle, lines, bytes)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            ) else {
                let _ = tx.commit();
                return;
            };
            for m in markers {
                let _ = stmt.execute(params![
                    fold_id,
                    ts,
                    scope,
                    session,
                    agent_id,
                    tool,
                    m.kind,
                    m.handle,
                    m.lines as i64,
                    m.bytes as i64,
                ]);
            }
        }
        let _ = tx.commit();
    }

    /// Forgets everything a scope was shown, and answers how many lines that was.
    ///
    /// Called at compaction. The ledger's licence to replace a run with a handle
    /// is that the agent is still holding those bytes, and compaction is the
    /// moment inside a session where that stops being true. Forgetting costs a
    /// missed reduction; not forgetting means telling an agent it has content its
    /// context no longer contains, which is the same defect that cancelled the
    /// project-scoped ledger (#401).
    pub fn ledger_forget(&self, session: &str) -> usize {
        let Ok(conn) = self.pool.get() else {
            return 0;
        };
        // A subagent's scope is `<session>/<agent>` (#581), and taking the bare
        // session id left those rows standing: the parent compacts, the scope
        // that survives keeps answering "already shown", and nothing in the
        // reply says the lines went with the summary (#807). `substr` rather
        // than `LIKE`, so an id carrying `_` or `%` cannot widen the delete.
        conn.execute(
            "DELETE FROM ledger_lines
              WHERE scope = ?1 OR substr(scope, 1, length(?1) + 1) = ?1 || '/'",
            params![session],
        )
        .unwrap_or(0)
    }

    /// Pages in the file and how many of them are free, or `None` when the
    /// database cannot be read.
    ///
    /// Measured 2026-08-09 on the maintainer's install: 50,226 pages with 36,264
    /// on the freelist, a 196 MB file holding 54 MB of data. `cleanup_old` and
    /// the trace pruning in #285 delete rows, and SQLite keeps their pages for
    /// reuse rather than returning them, so a database that has been pruned once
    /// stays large forever with `auto_vacuum = 0`.
    pub fn page_stats(&self) -> Option<(i64, i64)> {
        let conn = self.pool.get().ok()?;
        let pages = conn.query_row("PRAGMA page_count", [], |r| r.get(0)).ok()?;
        let free = conn
            .query_row("PRAGMA freelist_count", [], |r| r.get(0))
            .ok()?;
        Some((pages, free))
    }

    /// Rewrites the file without its freelist.
    ///
    /// Operator-triggered, never on the hook path: it rewrites the whole
    /// database, holds a write lock for the duration and needs free disk equal
    /// to the file. `auto_vacuum = INCREMENTAL` was the alternative and is worse
    /// here, because enabling it on an existing database needs a full `VACUUM`
    /// first and then taxes every commit thereafter, to reclaim space this
    /// workload loses in one prune a year.
    pub fn vacuum(&self) -> Result<()> {
        let conn = self.pool.get().context("DB pool exhausted")?;
        conn.execute_batch("VACUUM")?;
        Ok(())
    }

    pub fn cleanup_old(&self, days: u32) {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return,
        };

        let ts_threshold = chrono::Utc::now().timestamp() - (days as i64 * 86400);

        let _ = conn.execute(
            "DELETE FROM sessions WHERE started_at < ?1",
            params![ts_threshold],
        );
        let _ = conn.execute(
            "DELETE FROM distillations WHERE ts < ?1",
            params![ts_threshold],
        );
        let _ = conn.execute(
            "DELETE FROM file_access WHERE last_access < ?1",
            params![ts_threshold],
        );
        let _ = conn.execute(
            "DELETE FROM rewind_store WHERE ts < ?1",
            params![ts_threshold],
        );
        let _ = conn.execute(
            "DELETE FROM session_events WHERE ts < ?1",
            params![ts_threshold],
        );
        // #672. This was the one time-series table nobody pruned, so it kept
        // command strings past the window every other table honours.
        let _ = conn.execute(
            "DELETE FROM passthrough_events WHERE ts < ?1",
            params![ts_threshold],
        );
        // The ledger's eviction policy, defined before the project scope shipped
        // rather than after it, which is what the direction spec asks for.
        //
        // **Time, not size, and one window for both scopes.** A session scope
        // cannot outlive the session, so the ordinary retention window already
        // bounds it. The project scope is the one that could grow without limit,
        // and the honest bound on it is the same window: content nobody has
        // produced in 30 days is content this project has stopped emitting, and a
        // handle for it buys a retrieval of something the agent will not
        // recognise either. A size cap was the alternative and is worse here,
        // because evicting by size evicts the oldest rows of the *busiest*
        // project first, which is exactly where the repeats are.
        //
        // Rows are 16 bytes of hash plus a scope key, so the cost is bounded by
        // distinct lines emitted in the window rather than by bytes shown.
        // Pruned in the same window as the lines it describes. `passthrough_events`
        // shipped with no cleanup at all and grew unbounded, so a new table gets
        // its deletion in the same commit as its schema rather than a follow-up.
        let _ = conn.execute(
            "DELETE FROM ledger_folds WHERE ts < ?1",
            params![ts_threshold],
        );
        let _ = conn.execute(
            "DELETE FROM fold_markers WHERE ts < ?1",
            params![ts_threshold],
        );

        let _ = conn.execute(
            "DELETE FROM ledger_lines WHERE ts < ?1",
            params![ts_threshold],
        );
        // A source a surviving line still points at stays even when its own
        // `ts` is old: a line's `ts` can be refreshed by a later command while
        // it keeps the first one's `source_id`. `NOT IN` scans the lines once.
        let _ = conn.execute(
            "DELETE FROM ledger_sources WHERE ts < ?1
               AND id NOT IN (SELECT source_id FROM ledger_lines)",
            params![ts_threshold],
        );

        // `loop_memory` declared `ttl_days INTEGER DEFAULT 30` and nothing read
        // it, so goal memory was a permanent table wearing a 30 day label (#438).
        // Honoured per row rather than by the caller's window, because the column
        // is per row: a memory written with a longer ttl asked for one.
        //
        // `last_seen` and not `first_seen`, so a fact the loop keeps confirming
        // keeps its place and only what nobody has reconfirmed expires.
        let now = chrono::Utc::now().timestamp();
        let _ = conn.execute(
            "DELETE FROM loop_memory WHERE last_seen < ?1 - (COALESCE(ttl_days, 30) * 86400)",
            params![now],
        );

        // `execution_traces` was in no cleanup at all, which is why it alone grew
        // to 160 MB of a 187 MB database while every other table stayed bounded:
        // it stores `raw_input` and `distilled_output` verbatim, so it is two
        // orders of magnitude heavier per row than anything else here (#165).
        //
        // It gets its own, shorter window. Its only reader is `get_recent_traces`,
        // which asks for the newest N, so a trace older than a week answers no
        // question anyone is posing. Measured on the maintainer's database: 160.1
        // MB in total, 6.0 MB within seven days.
        // Held open by `OMNI_TRACE_RETENTION_DAYS` while a measurement is in
        // flight (#440). Roadmap axis 3 asks that a published figure be
        // reproducible by a stranger with the repo, and it cannot be if the
        // corpus behind it is deleted seven days later. Seven days stays the
        // default: #165 recorded this table at 160 MB of a 187 MB database
        // before the window existed, so the default is load bearing and only the
        // override is new. A value that does not parse is ignored rather than
        // failing the cleanup, because a typo in an env var must not stop the
        // database from being maintained.
        let retention_days = std::env::var("OMNI_TRACE_RETENTION_DAYS")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(TRACE_RETENTION_DAYS);
        let traces_threshold = chrono::Utc::now().timestamp() - (retention_days as i64 * 86400);
        let _ = conn.execute(
            "DELETE FROM execution_traces WHERE ts < ?1",
            params![traces_threshold],
        );
    }

    pub fn get_recent_traces(&self, limit: usize) -> Result<Vec<(String, String, String, String)>> {
        let conn = self.pool.get().context("DB pool exhausted")?;
        let mut stmt = conn.prepare(
            "SELECT session_id, command, raw_input, distilled_output FROM execution_traces ORDER BY ts DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    /// Test that the database is actually writable (catches sandbox restrictions)
    pub fn test_write(&self) -> bool {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return false,
        };
        match conn.execute("CREATE TABLE IF NOT EXISTS _write_test (id INTEGER)", []) {
            Ok(_) => {
                let _ = conn.execute("DROP TABLE IF EXISTS _write_test", []);
                true
            }
            Err(_) => false,
        }
    }

    // ──  Session Summaries ─────────────────────────────────────

    #[allow(clippy::too_many_arguments)]
    pub fn save_session_summary(
        &self,
        session_id: &str,
        started_at: i64,
        agent_id: &str,
        total_commands: u32,
        tokens_saved: u64,
        top_filter: &str,
        exit_reason: &str,
        project_path: &str,
    ) {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return,
        };
        let now = chrono::Utc::now().timestamp();
        let _ = conn.execute(
            "INSERT OR REPLACE INTO session_summaries
             (session_id, started_at, ended_at, agent_id, total_commands,
              tokens_saved, top_filter, exit_reason, project_path)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                session_id,
                started_at,
                now,
                agent_id,
                total_commands as i64,
                tokens_saved as i64,
                top_filter,
                exit_reason,
                project_path
            ],
        );
    }

    /// How long a session lasts before the host ends it, in commands.
    ///
    /// This is the meter #357 promoted and #435 found missing from every
    /// surface: a distillation percentage says how well one payload compressed,
    /// while the thing a user feels is how many turns they get before the
    /// context window forces a compaction. Only sessions the host actually
    /// closed are counted, because an open session's command count is a
    /// half-measured number that would drag the median down every time.
    ///
    /// Returns `(sessions, median_commands, longest, ended_by_compaction)`.
    /// `ended_by_compaction` is the count whose `exit_reason` names compaction,
    /// which is what makes the median mean "before the window ran out" rather
    /// than "before the user went to lunch".
    pub fn session_lifetime(&self, since: i64) -> (u64, u32, u32, u64) {
        let Ok(conn) = self.pool.get() else {
            return (0, 0, 0, 0);
        };
        let mut stmt = match conn.prepare(
            "SELECT total_commands, exit_reason FROM session_summaries
             WHERE ended_at >= ?1 AND total_commands > 0
             ORDER BY total_commands",
        ) {
            Ok(s) => s,
            Err(_) => return (0, 0, 0, 0),
        };
        let rows: Vec<(u32, String)> = match stmt.query_map(params![since], |r| {
            Ok((r.get::<_, i64>(0)? as u32, r.get::<_, String>(1)?))
        }) {
            Ok(rows) => rows.flatten().collect(),
            Err(_) => return (0, 0, 0, 0),
        };
        if rows.is_empty() {
            return (0, 0, 0, 0);
        }
        let compacted = rows
            .iter()
            .filter(|(_, reason)| reason.to_ascii_lowercase().contains("compact"))
            .count() as u64;
        let median = rows[rows.len() / 2].0;
        let longest = rows.last().map(|(c, _)| *c).unwrap_or(0);
        (rows.len() as u64, median, longest, compacted)
    }

    pub fn get_recent_session_summaries(&self, limit: usize) -> Vec<SessionSummaryRow> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        let mut stmt = match conn.prepare(
            "SELECT session_id, started_at, ended_at, agent_id, total_commands,
                    tokens_saved, top_filter, exit_reason, project_path
             FROM session_summaries ORDER BY ended_at DESC LIMIT ?1",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        match stmt.query_map(params![limit as i64], |row| {
            Ok(SessionSummaryRow {
                session_id: row.get(0)?,
                started_at: row.get(1)?,
                ended_at: row.get(2)?,
                agent_id: row.get(3)?,
                total_commands: row.get::<_, i64>(4)? as u32,
                tokens_saved: row.get::<_, i64>(5)? as u64,
                top_filter: row.get(6)?,
                exit_reason: row.get(7)?,
                project_path: row.get(8)?,
            })
        }) {
            Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
            Err(_) => vec![],
        }
    }

    // ──  Retrieve Events (adaptive threshold) ──────────────────

    pub fn record_retrieve_event(&self, command_prefix: &str, hash: &str, agent_id: &str) {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return,
        };
        let now = chrono::Utc::now().timestamp();
        let prefix = crate::util::text::safe_slice(command_prefix, 40);
        let _ = conn.execute(
            "INSERT INTO retrieve_events (command_prefix, hash, ts, agent_id)
             VALUES (?1, ?2, ?3, ?4)",
            params![prefix, hash, now, agent_id],
        );
    }

    /// Records that a memory tool was asked for something.
    ///
    /// Same table as the rewind counter, told apart by the tool name sitting in
    /// `command_prefix`. Until now `retrieve_events` counted one path out of
    /// five: `omni_retrieve` and nothing else, so "was memory read" was an
    /// inference rather than a query. `session_start` injects project knowledge
    /// into every continued session and recorded nothing, which means an
    /// injection that fires every time and one that never fires produced
    /// identical databases (#272).
    ///
    /// Every call is recorded, including the ones that come back empty. The
    /// question this answers is whether anyone asks, and a query that found
    /// nothing is still someone asking. `omni_retrieve` keeps its own rule of
    /// recording only what it found, because its counter feeds the adaptive
    /// compression rate rather than this one.
    pub fn record_memory_read(&self, tool: &str, subject: &str) {
        let agent_id = std::env::var("OMNI_AGENT_ID")
            .unwrap_or_else(|_| crate::agents::multiagent::detect_agent_id());
        self.record_retrieve_event(tool, crate::util::text::safe_slice(subject, 60), &agent_id);
    }

    /// Tokens saved at insertion, and the same saving compounded over the turns
    /// each distilled result was re-sent.
    ///
    /// Returns `(at_insertion, cumulative)`. The second is
    /// `delta × (1 + turns_after × CACHE_READ_RATE)` summed over every row that
    /// actually reduced something, where `turns_after` is how many later
    /// distillations share the row's session. That is the closest a hook can get
    /// to "how many times was this text re-sent", and it is why the figure is an
    /// estimate rather than a measurement (#173).
    ///
    /// `cumulative >= at_insertion` always, and the two are equal for a session
    /// of one call, because a one-shot command earns no multiplier.
    pub fn token_savings_with_reuse(&self, cache_read_rate: f64) -> Result<(u64, u64)> {
        let conn = self.pool.get().context("DB pool exhausted")?;
        conn.query_row(
            "SELECT COALESCE(SUM(delta), 0), COALESCE(SUM(delta * (1.0 + turns_after * ?1)), 0)
             FROM (
               SELECT raw_tokens - filtered_tokens AS delta,
                      COUNT(*) OVER (PARTITION BY session_id)
                        - ROW_NUMBER() OVER (PARTITION BY session_id ORDER BY ts, id)
                        AS turns_after
               FROM distillations
               WHERE raw_tokens > filtered_tokens
             )",
            params![cache_read_rate],
            |r| {
                let at_insertion: f64 = r.get(0)?;
                let cumulative: f64 = r.get(1)?;
                Ok((at_insertion as u64, cumulative as u64))
            },
        )
        .context("summing token savings with reuse")
    }

    /// How many times `tool` handed memory back, all time.
    ///
    /// The point of #272 is that this is a query rather than an inference, so it
    /// belongs beside the insert instead of being re-derived in SQL by whoever
    /// wants the number.
    pub fn count_memory_reads(&self, tool: &str) -> i64 {
        let Ok(conn) = self.pool.get() else {
            return 0;
        };
        conn.query_row(
            "SELECT COUNT(*) FROM retrieve_events WHERE command_prefix = ?1",
            params![tool],
            |r| r.get(0),
        )
        .unwrap_or(0)
    }

    /// Returns retrieve_rate for a command prefix (0.0, 1.0)
    /// High rate = OMNI too aggressive for this command type
    pub fn get_retrieve_rate(&self, command_prefix: &str, window_days: i64) -> f64 {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return 0.0,
        };
        let cutoff = chrono::Utc::now().timestamp() - window_days * 86400;
        let prefix = crate::util::text::safe_slice(command_prefix, 40);
        let retrieves: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM retrieve_events WHERE command_prefix = ?1 AND ts > ?2",
                params![prefix, cutoff],
                |r| r.get(0),
            )
            .unwrap_or(0);
        let distillations: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM distillations WHERE command LIKE ?1 AND ts > ?2",
                params![format!("{}%", prefix), cutoff],
                |r| r.get(0),
            )
            .unwrap_or(1)
            .max(1);
        (retrieves as f64 / distillations as f64).min(1.0)
    }

    pub fn find_command_for_hash(&self, hash: &str) -> Option<String> {
        let conn = self.pool.get().ok()?;
        conn.query_row(
            "SELECT command FROM distillations WHERE rewind_hash = ? LIMIT 1",
            params![hash],
            |r| r.get(0),
        )
        .ok()
    }

    // ──  Project Knowledge (cross-session semantic memory) ─────

    pub fn upsert_project_knowledge(
        &self,
        project_hash: &str,
        key: &str,
        value: &str,
        confidence: f32,
    ) {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return,
        };
        let now = chrono::Utc::now().timestamp();
        let _ = conn.execute(
            "INSERT INTO project_knowledge (project_hash, key, value, confidence, hit_count, last_updated)
             VALUES (?1,?2,?3,?4,1,?5)
             ON CONFLICT(project_hash, key) DO UPDATE SET
               value      = excluded.value,
               confidence = excluded.confidence,
               hit_count  = hit_count + 1,
               last_updated = excluded.last_updated",
            params![project_hash, key, value, confidence as f64, now],
        );
    }

    pub fn get_project_knowledge(&self, project_hash: &str) -> Vec<(String, String, f32)> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        let mut stmt = match conn.prepare(
            "SELECT key, value, confidence FROM project_knowledge
             WHERE project_hash = ?1 AND confidence >= 0.5
             ORDER BY confidence DESC, hit_count DESC LIMIT 50",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        match stmt.query_map(params![project_hash], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, f64>(2)? as f32,
            ))
        }) {
            Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
            Err(_) => vec![],
        }
    }

    // ──  Multi-Agent Sessions ─────────────────────────────────

    /// Sync agent session for cross-agent state sharing
    pub fn sync_agent_session(
        &self,
        agent_id: &str,
        session_id: &str,
        project_hash: &str,
        state_json: &str,
    ) {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return,
        };
        let now = chrono::Utc::now().timestamp();
        let _ = conn.execute(
            "INSERT INTO agent_sessions (agent_id, session_id, project_hash, last_active, state_json)
             VALUES (?1,?2,?3,?4,?5)
             ON CONFLICT(agent_id, project_hash) DO UPDATE SET
               session_id  = excluded.session_id,
               last_active = excluded.last_active,
               state_json  = excluded.state_json",
            params![agent_id, session_id, project_hash, now, state_json],
        );
    }

    /// Get all active agents working on the same project (within last 8h)
    pub fn get_active_agents_for_project(
        &self,
        project_hash: &str,
        exclude_agent: &str,
    ) -> Vec<AgentSessionRow> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        let cutoff = chrono::Utc::now().timestamp() - 8 * 3600;
        let mut stmt = match conn.prepare(
            "SELECT agent_id, session_id, last_active, state_json
             FROM agent_sessions
             WHERE project_hash = ?1 AND agent_id != ?2 AND last_active > ?3
             ORDER BY last_active DESC",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        match stmt.query_map(params![project_hash, exclude_agent, cutoff], |row| {
            Ok(AgentSessionRow {
                agent_id: row.get(0)?,
                session_id: row.get(1)?,
                last_active: row.get(2)?,
                state_json: row.get(3)?,
            })
        }) {
            Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
            Err(_) => vec![],
        }
    }

    // ── Pattern Memory (AI-5) ────────────────────────────────────────

    /// Record or update a recurring error pattern
    pub fn upsert_pattern(&self, pattern_text: &str, tool_family: &str) {
        let hash = Self::pattern_hash(pattern_text);
        let now = chrono::Utc::now().timestamp();
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return,
        };
        let _ = conn.execute(
            "INSERT INTO pattern_memory (pattern_hash, pattern_text, tool_family, first_seen, last_seen, occurrence_count)
             VALUES (?1, ?2, ?3, ?4, ?4, 1)
             ON CONFLICT(pattern_hash) DO UPDATE SET
               last_seen = ?4,
               occurrence_count = occurrence_count + 1,
               -- A pattern that just happened again is not resolved, whatever a
               -- later success said about its tool family (#427). resolve_pattern
               -- marks every unresolved pattern of a family at once, so without
               -- this one green `cargo test` declares five distinct failures
               -- fixed and they stay declared while they keep firing.
               was_resolved = 0,
               resolution_hint = '',
               tool_family = CASE WHEN tool_family = '' THEN ?3 ELSE tool_family END",
            params![hash, crate::util::text::safe_slice(pattern_text, 500), tool_family, now],
        );
    }

    /// Mark a pattern as resolved (command succeeded after failure)
    pub fn resolve_pattern(&self, tool_family: &str, resolution_hint: &str) {
        let now = chrono::Utc::now().timestamp();
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return,
        };
        // Resolve patterns for this tool family that were seen in the last hour
        let _ = conn.execute(
            "UPDATE pattern_memory SET was_resolved = 1, resolution_hint = ?1
             WHERE tool_family = ?2 AND was_resolved = 0 AND last_seen > ?3",
            params![
                crate::util::text::safe_slice(resolution_hint, 500),
                tool_family,
                now - 3600
            ],
        );
    }

    /// Get recurring patterns for a tool family (sorted by occurrence count)
    pub fn get_patterns(&self, tool_family: Option<&str>, limit: usize) -> Vec<PatternMemoryRow> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        let (query, tool_val) = if let Some(tool) = tool_family {
            (
                "SELECT pattern_hash, pattern_text, tool_family, first_seen, last_seen, \
                 occurrence_count, was_resolved, resolution_hint \
                 FROM pattern_memory WHERE tool_family = ?1 \
                 ORDER BY occurrence_count DESC LIMIT ?2",
                tool.to_string(),
            )
        } else {
            (
                "SELECT pattern_hash, pattern_text, tool_family, first_seen, last_seen, \
                 occurrence_count, was_resolved, resolution_hint \
                 FROM pattern_memory WHERE 1=1 \
                 ORDER BY occurrence_count DESC LIMIT ?2",
                String::new(),
            )
        };

        let mut stmt = match conn.prepare(query) {
            Ok(s) => s,
            Err(_) => return vec![],
        };

        if tool_family.is_some() {
            match stmt.query_map(params![tool_val, limit as i64], |row| {
                Ok(PatternMemoryRow {
                    pattern_hash: row.get(0)?,
                    pattern_text: row.get(1)?,
                    tool_family: row.get(2)?,
                    first_seen: row.get(3)?,
                    last_seen: row.get(4)?,
                    occurrence_count: row.get(5)?,
                    was_resolved: row.get::<_, i64>(6)? != 0,
                    resolution_hint: row.get(7)?,
                })
            }) {
                Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
                Err(_) => vec![],
            }
        } else {
            match stmt.query_map(params!["", limit as i64], |row| {
                Ok(PatternMemoryRow {
                    pattern_hash: row.get(0)?,
                    pattern_text: row.get(1)?,
                    tool_family: row.get(2)?,
                    first_seen: row.get(3)?,
                    last_seen: row.get(4)?,
                    occurrence_count: row.get(5)?,
                    was_resolved: row.get::<_, i64>(6)? != 0,
                    resolution_hint: row.get(7)?,
                })
            }) {
                Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
                Err(_) => vec![],
            }
        }
    }

    /// Get top recurring issues across all tools
    pub fn get_top_insights(&self, limit: usize) -> Vec<PatternMemoryRow> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        let mut stmt = match conn.prepare(
            "SELECT pattern_hash, pattern_text, tool_family, first_seen, last_seen, \
             occurrence_count, was_resolved, resolution_hint \
             FROM pattern_memory \
             WHERE occurrence_count >= 2 \
             ORDER BY occurrence_count DESC, last_seen DESC \
             LIMIT ?1",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };

        match stmt.query_map(params![limit as i64], |row| {
            Ok(PatternMemoryRow {
                pattern_hash: row.get(0)?,
                pattern_text: row.get(1)?,
                tool_family: row.get(2)?,
                first_seen: row.get(3)?,
                last_seen: row.get(4)?,
                occurrence_count: row.get(5)?,
                was_resolved: row.get::<_, i64>(6)? != 0,
                resolution_hint: row.get(7)?,
            })
        }) {
            Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
            Err(_) => vec![],
        }
    }

    fn pattern_hash(text: &str) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let normalized = text.trim().to_lowercase();
        let mut hasher = DefaultHasher::new();
        normalized.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
}

// ── v0.5.7 Row Types ─────────────────────────────────────────────────

/// One fold's worth of evidence: whose lines it replaced, and how much.
///
/// Grouped by the caller so a payload folding twenty runs from one agent writes
/// one row rather than twenty. The recipient is on the call, not the row, since
/// it is the same for every fold in a payload.
/// What each engine removed, and the base each removed it from (#665).
///
/// The two ratios come from different populations and may never be added or
/// averaged together. Bytes may be summed; percentages may not.
#[derive(Debug, Default, Clone, Copy)]
pub struct EngineTotals {
    pub distilled_calls: u64,
    pub distilled_input: u64,
    pub distilled_output: u64,
    /// Calls OMNI declined. They save nothing and cost nothing, and they are the
    /// 15,395 that dragged a 48% distiller down to a reported 9%.
    pub declined_calls: u64,
    /// Bytes those declined calls carried. Never a saving and never a loss, so it
    /// stays out of every ratio; it exists so a composition report can say what
    /// OMNI saw without inferring it from the distiller's output, which is what
    /// survived distillation rather than what was handed back (#612).
    pub declined_bytes: u64,
    /// Fold operations, not `ledger_folds` rows. One fold writes a row per
    /// (origin, source agent) pair carrying the same payload size.
    pub folds: u64,
    pub fold_bytes: u64,
    /// Folds recording the payload they came out of, which is the only base the
    /// fold ratio may be taken against. Calls, for the same reason as above.
    pub priced_folds: u64,
    pub priced_fold_bytes: u64,
    pub priced_payload: u64,
    pub first_fold_ts: Option<i64>,
}

impl EngineTotals {
    pub fn distilled_saved(&self) -> u64 {
        self.distilled_input.saturating_sub(self.distilled_output)
    }

    /// Bytes neither engine let through. Absolute, because summing the two
    /// percentages is the error this whole type exists to prevent.
    pub fn total_saved(&self) -> u64 {
        self.distilled_saved() + self.fold_bytes
    }

    pub fn distilled_pct(&self) -> Option<f64> {
        (self.distilled_input > 0)
            .then(|| 100.0 * self.distilled_saved() as f64 / self.distilled_input as f64)
    }

    /// `None` when no fold in the window records a payload size, because a
    /// ratio over a base that does not exist is a guess wearing a percent sign.
    pub fn fold_pct(&self) -> Option<f64> {
        (self.priced_payload > 0)
            .then(|| 100.0 * self.priced_fold_bytes as f64 / self.priced_payload as f64)
    }
}

#[derive(Debug, Clone)]
/// How often one marker shape sent a reader back for the bytes.
///
/// The whole point of recording the kind (#766). A shape with a high retrieve
/// rate is one that did not answer the question it interrupted, which is a work
/// queue rather than an opinion about wording.
pub struct MarkerRate {
    pub kind: String,
    /// The tool whose reply the marker replaced, empty for a row written before
    /// the column existed (#822).
    pub tool: String,
    pub folds: i64,
    pub retrieved: i64,
}

/// One marker a reader was actually handed, at the grain `ledger_folds` cannot
/// reach: that table groups a call's folds by (origin, source agent), and a
/// marker is per run.
///
/// `kind` comes from the arm that rendered the string, so a wording change moves
/// the label with it (#766). `handle` is what joins a later `retrieve_events`
/// row back to the marker that sent the reader looking, which is the measurement
/// this exists for: a marker shape with a high retrieve rate is one that did not
/// answer the question it interrupted.
pub struct MarkerRecord {
    pub kind: &'static str,
    pub handle: String,
    pub lines: usize,
    pub bytes: usize,
}

pub struct FoldRecord {
    /// The agent that was shown these lines first, from `ledger_lines.agent_id`.
    pub source_agent: String,
    /// `session` or `project`. Kept as a string because it is read by SQL far
    /// more often than by Rust, and a query should not have to know an enum's
    /// discriminants.
    pub origin: &'static str,
    pub lines: usize,
    pub bytes: usize,
    /// Every line of the payload was folded, so the agent holds markers and no
    /// content. A property of the call, not of this row: each row of one call
    /// carries the same value, because the table already aggregates by
    /// (origin, source agent) and a per-run flag has nowhere to live here.
    pub whole_output: bool,
    /// The whole payload this fold came out of, repeated on every row of the
    /// call so the floor audit is a row predicate rather than a GROUP BY. `ts`
    /// is whole seconds and so cannot key a call: two folds by one agent inside
    /// one second would merge, and their combined size can clear a floor
    /// neither cleared alone.
    pub payload_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct PatternMemoryRow {
    pub pattern_hash: String,
    pub pattern_text: String,
    pub tool_family: String,
    pub first_seen: i64,
    pub last_seen: i64,
    pub occurrence_count: i64,
    pub was_resolved: bool,
    pub resolution_hint: String,
}

#[derive(Debug, Clone)]
pub struct DistillationRow {
    pub command: String,
    pub input_bytes: usize,
    pub output_bytes: usize,
    pub route: String,
    pub filter_name: String,
}

/// One cross-turn fold, as `omni_explain_savings` needs to report it (#602).
///
/// No command column, because a fold is not attributed to one: the ledger keys
/// lines by scope and hash, so the bytes it replaced may have been shown by a
/// different command than the one being answered. Saying which would need a
/// source per line, which the table does not carry.
/// What the ledger remembers about a line it has shown before.
///
/// `source` is the command that showed it first, and is empty for rows written
/// before #622 added the column. Empty means unrecorded, which suppresses the
/// marker's source clause rather than guessing at one.
#[derive(Debug, Clone)]
pub struct SeenLine {
    pub agent: String,
    pub source: String,
}

#[derive(Debug)]
pub struct LedgerFoldRow {
    pub origin: String,
    pub lines: usize,
    pub bytes: usize,
    pub whole_output: bool,
}

/// One filter's re-run comparison: distilled output against raw output (#109).
///
/// `Passthrough` rows are the control arm, the agent read the original bytes -
/// so the two arms differ only in whether OMNI changed what was read. A filter
/// whose distilled arm is re-run more often than its raw arm removed something
/// the agent needed, whatever its reduction percentage claims.
#[derive(Debug, Clone)]
pub struct RerunRow {
    pub filter_name: String,
    pub distilled: u64,
    pub raw: u64,
    pub distilled_reruns: u64,
    pub raw_reruns: u64,
    pub distilled_avg_input: u64,
    pub raw_avg_input: u64,
}

impl RerunRow {
    pub fn distilled_pct(&self) -> f64 {
        pct(self.distilled_reruns, self.distilled)
    }

    pub fn raw_pct(&self) -> f64 {
        pct(self.raw_reruns, self.raw)
    }

    /// Percentage points by which distilling this filter's output raised the
    /// re-run rate. Positive means distillation cost the agent a second run.
    pub fn delta_pp(&self) -> f64 {
        self.distilled_pct() - self.raw_pct()
    }

    /// Whether the two arms are too differently sized to compare.
    ///
    /// See `RERUN_SIZE_SKEW_LIMIT`: distillation only fires on large output, so
    /// a filter can land its big invocations in one arm and its small ones in
    /// the other. When that happens the delta measures input size, not lost
    /// signal, and must not be reported as if it measured lost signal.
    pub fn is_confounded(&self) -> bool {
        let (lo, hi) = if self.distilled_avg_input <= self.raw_avg_input {
            (self.distilled_avg_input, self.raw_avg_input)
        } else {
            (self.raw_avg_input, self.distilled_avg_input)
        };
        // A zero-byte arm cannot be size-matched against a non-zero one.
        if lo == 0 {
            return hi > 0;
        }
        hi as f64 / lo as f64 > crate::pipeline::RERUN_SIZE_SKEW_LIMIT
    }
}

/// One agent's totals, with the rows excluded from them counted separately.
#[derive(Debug, Clone)]
pub struct AgentRow {
    pub agent_id: String,
    /// Calls whose distillation reached the agent.
    pub calls: u64,
    pub input_bytes: u64,
    pub output_bytes: u64,
    /// Calls excluded from the three fields above: pre-#158 Claude Code rows
    /// whose savings were recorded but never applied.
    pub unverified: u64,
}

/// SQL restricting a savings sum to rows whose distillation actually reached an
/// agent (#163).
///
/// A `claude_code` row written before `POST_HOOK_FIX_TS` records a distillation
/// that was computed, scored, routed and stored, and then dropped by the host,
/// because the hook emitted `updatedResponse`, a key Claude Code ignores (#158).
/// The agent read the raw bytes. Those rows are indistinguishable from the
/// `omni exec` and pipe rows where the same numbers are true.
///
/// The rest of such a row is still good evidence, latency, command, project and
/// file access all really happened. Only the byte and token columns are fiction,
/// so this gates **sums over those columns**, not the row. Deleting the rows
/// would destroy true history to remove a false column, which is what the
/// never-drop invariant argues against.
///
/// The second clause is #212, and it is the larger of the two. `omni exec` and
/// the shell pipe write to a TTY: a human reads the result, no context holds it,
/// nothing is billed. Compressing it may still help readability, but counting it
/// as *tokens saved* and folding it into one headline makes the headline
/// meaningless, those rows were **73.4% of every byte OMNI claimed to have
/// saved all-time**, and stripping them took the figure from 66.3% to the 29.3%
/// that Claude Code actually saw. Rows written since `delivered_bytes` exists say
/// so themselves; older `terminal` rows are excluded by name, because the column
/// is `-1` there and `-1` means "recorded before this was known", never "nothing
/// was delivered".
///
/// Interpolated rather than bound because `POST_HOOK_FIX_TS` is a compile-time
/// `i64` constant, never user input.
pub(crate) fn applied_only() -> String {
    format!(
        "NOT (agent_id = 'claude_code' AND ts < {}) \
         AND delivered_bytes != 0 \
         AND NOT (delivered_bytes = -1 AND agent_id = 'terminal')",
        crate::pipeline::POST_HOOK_FIX_TS
    )
}

fn pct(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        100.0 * part as f64 / whole as f64
    }
}

#[derive(Debug, Clone)]
pub struct SessionSummaryRow {
    pub session_id: String,
    pub started_at: i64,
    pub ended_at: i64,
    pub agent_id: String,
    pub total_commands: u32,
    pub tokens_saved: u64,
    pub top_filter: String,
    pub exit_reason: String,
    pub project_path: String,
}

#[derive(Debug, Clone)]
pub struct AgentSessionRow {
    pub agent_id: String,
    pub session_id: String,
    pub last_active: i64,
    pub state_json: String,
}

// ── Engram Persistence & FTS5 Search ────────────────────
impl SqliteBackend {
    pub fn persist_engram(
        &self,
        session_id: &str,
        engram: &crate::session::engram::Engram,
        category: &str,
        project_hash: &str,
    ) -> Result<()> {
        let conn = self.pool.get().context("DB pool exhausted")?;
        let id = format!("{}-{}", engram.trigger, engram.timestamp);
        let files_json = serde_json::to_string(&engram.files).unwrap_or_else(|_| "[]".into());
        let now = chrono::Utc::now().timestamp();
        conn.execute(
            "INSERT OR REPLACE INTO engrams
             (id, session_id, trigger, label, detail, files, category, tags, project_hash, ts, last_accessed, hit_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '[]', ?8, ?9, ?10, 0)",
            rusqlite::params![
                id, session_id, engram.trigger.to_string(), engram.label,
                engram.detail, files_json, category, project_hash,
                engram.timestamp, now
            ],
        )?;
        Ok(())
    }

    pub fn search_knowledge(
        &self,
        query: &str,
        project_hash: Option<&str>,
        limit: usize,
    ) -> Vec<KnowledgeHit> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        let mut out: Vec<KnowledgeHit> = vec![];
        if let Some(ph) = project_hash {
            let sql = "SELECT pk.key, pk.value, pk.confidence, pk.project_hash
                 FROM knowledge_fts kf
                 JOIN project_knowledge pk ON pk.rowid = kf.rowid
                 WHERE knowledge_fts MATCH ?1 AND pk.project_hash = ?2
                 ORDER BY rank LIMIT ?3";
            if let Ok(mut stmt) = conn.prepare(sql)
                && let Ok(mapped) =
                    stmt.query_map(rusqlite::params![query, ph, limit as i64], |row| {
                        Ok(KnowledgeHit {
                            key: row.get(0)?,
                            value: row.get(1)?,
                            confidence: row.get(2)?,
                            project_hash: row.get(3)?,
                        })
                    })
            {
                out = mapped.filter_map(Result::ok).collect::<Vec<KnowledgeHit>>();
            }
        } else {
            let sql = "SELECT pk.key, pk.value, pk.confidence, pk.project_hash
                 FROM knowledge_fts kf
                 JOIN project_knowledge pk ON pk.rowid = kf.rowid
                 WHERE knowledge_fts MATCH ?1
                 ORDER BY rank LIMIT ?2";
            if let Ok(mut stmt) = conn.prepare(sql)
                && let Ok(mapped) = stmt.query_map(rusqlite::params![query, limit as i64], |row| {
                    Ok(KnowledgeHit {
                        key: row.get(0)?,
                        value: row.get(1)?,
                        confidence: row.get(2)?,
                        project_hash: row.get(3)?,
                    })
                })
            {
                out = mapped.filter_map(Result::ok).collect::<Vec<KnowledgeHit>>();
            }
        }
        out
    }

    pub fn search_engrams(
        &self,
        query: &str,
        project_hash: Option<&str>,
        limit: usize,
    ) -> Vec<EngramHit> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        let mut out: Vec<EngramHit> = vec![];
        if let Some(ph) = project_hash {
            let sql = "SELECT e.label, e.detail, e.category, e.files, e.ts, ef.rank
                 FROM engrams_fts ef
                 JOIN engrams e ON e.rowid = ef.rowid
                 WHERE engrams_fts MATCH ?1 AND e.project_hash = ?2
                 ORDER BY rank LIMIT ?3";
            if let Ok(mut stmt) = conn.prepare(sql)
                && let Ok(mapped) =
                    stmt.query_map(rusqlite::params![query, ph, limit as i64], |row| {
                        let files_json: String = row.get(3)?;
                        Ok(EngramHit {
                            label: row.get(0)?,
                            detail: row.get(1)?,
                            category: row.get(2)?,
                            files: serde_json::from_str(&files_json).unwrap_or_default(),
                            ts: row.get(4)?,
                            rank: row.get::<_, f64>(5).unwrap_or(0.0).abs(),
                        })
                    })
            {
                out = mapped.filter_map(Result::ok).collect::<Vec<EngramHit>>();
            }
        } else {
            let sql = "SELECT e.label, e.detail, e.category, e.files, e.ts, ef.rank
                 FROM engrams_fts ef
                 JOIN engrams e ON e.rowid = ef.rowid
                 WHERE engrams_fts MATCH ?1
                 ORDER BY rank LIMIT ?2";
            if let Ok(mut stmt) = conn.prepare(sql)
                && let Ok(mapped) = stmt.query_map(rusqlite::params![query, limit as i64], |row| {
                    let files_json: String = row.get(3)?;
                    Ok(EngramHit {
                        label: row.get(0)?,
                        detail: row.get(1)?,
                        category: row.get(2)?,
                        files: serde_json::from_str(&files_json).unwrap_or_default(),
                        ts: row.get(4)?,
                        rank: row.get::<_, f64>(5).unwrap_or(0.0).abs(),
                    })
                })
            {
                out = mapped.filter_map(Result::ok).collect::<Vec<EngramHit>>();
            }
        }
        out
    }

    pub fn unified_recall(
        &self,
        query: &str,
        project_hash: Option<&str>,
        limit: usize,
    ) -> Vec<RecallHit> {
        let mut results = vec![];

        let knowledge = self.search_knowledge(query, project_hash, limit);
        for k in knowledge {
            results.push(RecallHit {
                key: format!("[Knowledge] {}", k.key),
                value: k.value,
                source: "knowledge".to_string(),
                score: k.confidence as f64 * 10.0, // Scale confidence to match roughly with BM25 rank magnitude
            });
        }

        let engrams = self.search_engrams(query, project_hash, limit);
        for e in engrams {
            results.push(RecallHit {
                key: format!("[Engram: {}] {}", e.category, e.label),
                value: e.detail.unwrap_or_default(),
                source: "engram".to_string(),
                score: e.rank,
            });
        }

        // Sort by score descending (larger is better for our abs(rank) and scaled confidence)
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit);

        results
    }
}

// ── Adaptive Scoring & Knowledge Helpers (Sprint 3) ──────
impl SqliteBackend {
    /// Log every recall invocation for adaptive scoring feedback loop.
    /// Called by omni_recall MCP tool after returning results.
    pub fn log_retrieval(
        &self,
        query: &str,
        hit_source: &str,
        hit_key: &str,
        project_hash: &str,
        command_ctx: &str,
    ) {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return,
        };
        let _ = conn.execute(
            "INSERT INTO retrieval_feedback (query, hit_source, hit_key, project_hash, command_ctx, ts)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                query,
                hit_source,
                hit_key,
                project_hash,
                command_ctx,
                chrono::Utc::now().timestamp()
            ],
        );
    }

    /// Retrieve a single knowledge value by exact key.
    /// Used for `__omni_goal__` pinning and similar reserved keys (BIZ-03).
    pub fn get_knowledge(&self, project_hash: &str, key: &str) -> Option<String> {
        let conn = self.pool.get().ok()?;
        conn.query_row(
            "SELECT value FROM project_knowledge WHERE project_hash = ?1 AND key = ?2 LIMIT 1",
            rusqlite::params![project_hash, key],
            |row| row.get(0),
        )
        .optional()
        .ok()
        .flatten()
    }

    /// Count engrams for a project (for stats display, BIZ-02).
    pub fn count_engrams(&self, project_hash: &str) -> usize {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return 0,
        };
        conn.query_row(
            "SELECT COUNT(*) FROM engrams WHERE project_hash = ?1",
            rusqlite::params![project_hash],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0) as usize
    }

    /// Count knowledge entries for a project (for stats display, BIZ-02).
    pub fn count_knowledge_entries(&self, project_hash: &str) -> usize {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return 0,
        };
        conn.query_row(
            "SELECT COUNT(*) FROM project_knowledge WHERE project_hash = ?1",
            rusqlite::params![project_hash],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0) as usize
    }

    /// Count recall calls within the last 24 hours for a project (BIZ-02).
    pub fn count_recalls_today(&self, project_hash: &str) -> usize {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return 0,
        };
        let since = chrono::Utc::now().timestamp() - 86400;
        conn.query_row(
            "SELECT COUNT(*) FROM retrieval_feedback WHERE project_hash = ?1 AND ts > ?2",
            rusqlite::params![project_hash, since],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0) as usize
    }

    /// Analyze retrieval patterns to surface adaptive insights (INT-01).
    /// Returns a Vec of (command_ctx, recall_count) for commands recalled >= threshold times
    /// within the last 7 days. This is the raw signal for `omni_adaptive_insights`.
    pub fn get_frequent_recall_commands(
        &self,
        project_hash: &str,
        min_count: u32,
        days: u32,
    ) -> Vec<(String, u64)> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        let since = chrono::Utc::now().timestamp() - (days as i64 * 86400);
        let mut stmt = match conn.prepare(
            "SELECT command_ctx, COUNT(*) as cnt
             FROM retrieval_feedback
             WHERE project_hash = ?1 AND command_ctx != '' AND ts > ?2
             GROUP BY command_ctx
             HAVING cnt >= ?3
             ORDER BY cnt DESC
             LIMIT 5",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(
            rusqlite::params![project_hash, since, min_count as i64],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64)),
        )
        .map(|iter| iter.filter_map(Result::ok).collect())
        .unwrap_or_default()
    }

    /// Surface knowledge entries that have never been recalled (Underused signal, INT-01).
    pub fn get_unreferenced_knowledge(&self, project_hash: &str) -> Vec<String> {
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return vec![],
        };
        let mut stmt = match conn.prepare(
            "SELECT pk.key FROM project_knowledge pk
             LEFT JOIN retrieval_feedback rf ON rf.hit_key = pk.key AND rf.project_hash = pk.project_hash
             WHERE pk.project_hash = ?1 AND pk.key NOT LIKE '__omni_%' AND rf.id IS NULL
             LIMIT 10",
        ) {
            Ok(s) => s,
            Err(_) => return vec![],
        };
        stmt.query_map(rusqlite::params![project_hash], |row| row.get(0))
            .map(|iter| iter.filter_map(Result::ok).collect())
            .unwrap_or_default()
    }
}

// ── Public Data Types ────────────────────────────────────

#[derive(Debug, Clone)]
pub struct KnowledgeHit {
    pub key: String,
    pub value: String,
    pub confidence: f32,
    pub project_hash: String,
}

#[derive(Debug, Clone)]
pub struct EngramHit {
    pub label: String,
    pub detail: Option<String>,
    pub category: String,
    pub files: Vec<String>,
    pub ts: i64,
    pub rank: f64,
}

#[derive(Debug, Clone)]
pub struct RecallHit {
    pub key: String,
    pub value: String,
    pub source: String,
    pub score: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn get_temp_store() -> (Store, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("omni.db");
        (Store::open_path(&db_path).unwrap(), dir)
    }

    /// #911. A command is stored once however many lines it showed, and a row
    /// written before the lookup table still answers with its own text.
    #[test]
    fn a_source_is_stored_once_and_old_rows_still_name_theirs() {
        let (store, _dir) = get_temp_store();
        let hashes: Vec<String> = (0..50).map(|i| format!("h{i}")).collect();
        store.ledger_record("/repo", &hashes, "a", "cat src/main.rs");
        store.ledger_record("/repo", &["h99".to_string()], "a", "cat src/main.rs");

        let conn = store.pool.get().unwrap();
        let count = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(count("SELECT COUNT(*) FROM ledger_sources"), 1);
        assert_eq!(
            count("SELECT COUNT(*) FROM ledger_lines WHERE source <> ''"),
            0
        );

        conn.execute(
            "INSERT INTO ledger_lines (scope, line_hash, ts, agent_id, source)
             VALUES ('/repo', 'old', strftime('%s','now'), 'a', 'git diff')",
            [],
        )
        .unwrap();
        let ask: Vec<String> = ["h0", "h99", "old"].map(String::from).to_vec();
        let seen = store.ledger_seen("/repo", &ask, 0);
        assert_eq!(seen["h0"].source, "cat src/main.rs");
        assert_eq!(seen["h99"].source, "cat src/main.rs");
        assert_eq!(seen["old"].source, "git diff");
    }

    /// #911. The prune drops a source no line points at, and keeps an old one a
    /// refreshed line still names.
    #[test]
    fn the_prune_keeps_a_source_a_surviving_line_still_names() {
        let (store, _dir) = get_temp_store();
        store.ledger_record("/repo", &["kept".to_string()], "a", "cat kept.rs");
        store.ledger_record("/repo", &["gone".to_string()], "a", "cat gone.rs");
        let conn = store.pool.get().unwrap();
        let aged = 40 * 86_400;
        conn.execute("UPDATE ledger_sources SET ts = ts - ?1", [aged])
            .unwrap();
        conn.execute(
            "UPDATE ledger_lines SET ts = ts - ?1 WHERE line_hash = 'gone'",
            [aged],
        )
        .unwrap();
        drop(conn);

        store.cleanup_old(30);

        let conn = store.pool.get().unwrap();
        let mut stmt = conn
            .prepare("SELECT source FROM ledger_sources ORDER BY source")
            .unwrap();
        let left: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(left, ["cat kept.rs"]);
    }

    /// #631. The database holds raw command output, and it inherited the umask:
    /// 0644 on the machine that reported it, for 147 MB of it.
    ///
    /// Loosened by hand and reopened rather than tested straight after creation,
    /// for two reasons. A runner with a strict umask would give 0600 anyway and
    /// the assertion would pass against no code at all, and an existing install
    /// already has the bytes on disk, so tightening on the next run is the case
    /// that matters.
    #[cfg(unix)]
    #[test]
    fn a_reopen_tightens_a_database_left_world_readable() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let db_path = dir.path().join("omni.db");
        drop(Store::open_path(&db_path).unwrap());

        std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o666))
            .expect("loosen the database");

        drop(Store::open_path(&db_path).unwrap());

        let mode = std::fs::metadata(&db_path)
            .expect("stat")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "recorded output must not stay world readable");
    }

    /// #602. `omni_explain_savings` reported "No recent distillations" for a
    /// session whose only compression was cross-turn folding, because folds go to
    /// `ledger_folds` and the tool read `distillations`.
    ///
    /// Driven through `Ledger::project`, the real write path, rather than by
    /// inserting rows by hand. The first version of this test did insert them,
    /// invented a session-shaped `scope`, and passed against a fix that returned
    /// nothing in production: `scope` holds the **project** whenever there is one,
    /// by design, and all 936 rows on the machine that confirmed the bug carry a
    /// filesystem path there. A fixture that writes its own rows cannot catch a
    /// disagreement between the writer and the reader, which is the only thing
    /// that was wrong.
    #[test]
    fn folds_are_found_by_the_session_that_made_them() {
        let (store, _dir) = get_temp_store();
        let text: String = (1..=40)
            .map(|i| format!("    key_{i:02} = \"value_{i:02}\" padding padding padding\n"))
            .collect();

        let fold_once = |scope: &str| {
            crate::ledger::Ledger::new(&store, scope)
                .with_project("/repo")
                .by("claude_code")
                .project(&text)
        };
        assert!(
            fold_once("sess-a/claude_code").is_none(),
            "nothing shown yet"
        );
        assert!(
            fold_once("sess-a/claude_code").is_some(),
            "a repeat above the floor must fold, or this tests a floor and not the lookup"
        );

        let mine = store.get_recent_ledger_folds(Some("sess-a"), 10);
        assert!(
            !mine.is_empty(),
            "the session that folded cannot see its own folds"
        );
        assert!(
            store.get_recent_ledger_folds(Some("sess-b"), 10).is_empty(),
            "a session with no folds borrowed another's"
        );

        // The shape the MCP server actually asks in. It cannot see the host
        // session id, and the id it does hold is minted by `SessionState::new`,
        // so filtering on that returns nothing and reads as "nothing happened".
        assert!(
            !store.get_recent_ledger_folds(None, 10).is_empty(),
            "an unfiltered lookup must see the folds, or the MCP tool is blind again"
        );
        let minted = format!("{}-{}-0", 1_787_000_000_000i64, std::process::id());
        assert!(
            store.get_recent_ledger_folds(Some(&minted), 10).is_empty(),
            "a minted id matched a host-keyed row, so this fixture proves nothing"
        );
    }

    /// #441. The largest population in the corpus was one undifferentiated
    /// bucket, readable only by string matching on the command it had been
    /// concatenated to.
    #[test]
    fn counts_passthroughs_by_the_gate_that_declined_them() {
        let (store, _d) = get_temp_store();
        store.record_passthrough(
            "kubectl get pods -o json",
            900,
            "structured json",
            "s1",
            "claude_code",
        );
        store.record_passthrough("gh api repos", 700, "structured json", "s1", "claude_code");
        store.record_passthrough("cargo build", 500, "below guardrail", "s2", "claude_code");

        let by_reason = store.passthrough_reasons(0);

        assert_eq!(
            by_reason,
            vec![
                ("structured json".to_string(), 2),
                ("below guardrail".to_string(), 1)
            ]
        );
    }

    /// #440. The knob exists so a corpus can outlive the window while a figure
    /// is being measured; the default has to stay where #165 put it.
    #[test]
    fn holds_traces_open_when_the_override_asks_and_ignores_a_bad_one() {
        let read = |raw: Option<&str>| -> u32 {
            raw.and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(TRACE_RETENTION_DAYS)
        };

        assert_eq!(read(None), 7, "the default is load bearing");
        assert_eq!(read(Some("90")), 90);
        assert_eq!(
            read(Some("not-a-number")),
            7,
            "a typo must not stop cleanup"
        );
    }

    /// #427: every one of 20 entries read RESOLVED, including patterns that had
    /// fired 27 times, because a success resolves a whole tool family and
    /// nothing ever took the flag back.
    #[test]
    fn a_pattern_that_recurs_stops_being_resolved() {
        let (store, _d) = get_temp_store();
        store.upsert_pattern("test module::perf::latency ... FAILED", "cargo test");
        store.resolve_pattern("cargo test", "cargo test");

        let resolved_now = store
            .get_patterns(None, 10)
            .into_iter()
            .find(|p| p.pattern_text.contains("latency"))
            .expect("recorded");
        assert!(resolved_now.was_resolved, "a success does resolve it");

        store.upsert_pattern("test module::perf::latency ... FAILED", "cargo test");

        let after_recurrence = store
            .get_patterns(None, 10)
            .into_iter()
            .find(|p| p.pattern_text.contains("latency"))
            .expect("recorded");
        assert!(
            !after_recurrence.was_resolved,
            "it fired again, so calling it resolved is a false claim"
        );
        assert_eq!(after_recurrence.occurrence_count, 2);
    }

    /// #438: the column declared a contract for four releases and nothing read
    /// it, so the schema said 30 days and the table was permanent.
    #[test]
    fn expires_loop_memory_that_nothing_has_reconfirmed() {
        let (store, _d) = get_temp_store();
        let conn = store.pool.get().expect("conn");
        let now = chrono::Utc::now().timestamp();
        let long_ago = now - 40 * 86400;
        conn.execute(
            "INSERT INTO loop_memory (loop_goal_hash, key, value, first_seen, last_seen, ttl_days)
             VALUES ('g', 'stale', 'v', ?1, ?1, 30),
                    ('g', 'fresh', 'v', ?1, ?2, 30),
                    ('g', 'patient', 'v', ?1, ?1, 365)",
            params![long_ago, now],
        )
        .expect("seed");
        drop(conn);

        store.cleanup_old(90);

        let conn = store.pool.get().expect("conn");
        let mut stmt = conn
            .prepare("SELECT key FROM loop_memory ORDER BY key")
            .expect("prepare");
        let keys: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .expect("query")
            .flatten()
            .collect();

        assert_eq!(
            keys,
            vec!["fresh".to_string(), "patient".to_string()],
            "only the row past its own ttl goes, and a longer ttl is honoured"
        );
    }

    /// The headline `omni stats` leads with, so a wrong median is a wrong
    /// product claim rather than a wrong debug line.
    #[test]
    fn reports_the_median_session_and_how_many_hit_a_compaction() {
        let (store, _d) = get_temp_store();
        for (id, commands, reason) in [
            ("s1", 10u32, "clear"),
            ("s2", 50, "auto_compact"),
            ("s3", 200, "logout"),
            // Never closed, so it has nothing to say about how long a session
            // lasts, and counting it would drag every median down.
            ("s4", 0, "clear"),
        ] {
            store.save_session_summary(id, 0, "claude_code", commands, 0, "", reason, "/p");
        }

        let (sessions, median, longest, compacted) = store.session_lifetime(0);

        assert_eq!(
            sessions, 3,
            "a session with no commands is not a data point"
        );
        assert_eq!(median, 50);
        assert_eq!(longest, 200);
        assert_eq!(compacted, 1);
    }

    #[test]
    fn open_creates_database_and_schema() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("omni.db");

        let store = Store::open_path(&db_path).unwrap();

        let conn = store.pool.get().unwrap();
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .unwrap();
        let tables: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        assert!(tables.contains(&"sessions".to_string()));
        assert!(tables.contains(&"distillations".to_string()));
        assert!(tables.contains(&"file_access".to_string()));
        assert!(tables.contains(&"rewind_store".to_string()));
        assert!(tables.contains(&"session_events".to_string())); // Because of fts5, session_events and its shadow tables exist
        assert!(tables.contains(&"execution_traces".to_string()));
    }

    /// #212: `terminal` rows were 73.4% of every byte OMNI claimed to have
    /// saved. That path writes to a TTY, a human reads it, no context holds it,
    /// nothing is billed, so folding it into a *token* headline made the
    /// headline describe nothing. Stripping it took the all-time figure from
    /// 66.3% to the 29.3% Claude Code actually saw.
    #[test]
    fn excludes_bytes_no_model_read_from_the_savings_sum() {
        let (store, _dir) = get_temp_store();

        let row = |delivered: usize| DistillResult {
            output: String::new(),
            route: crate::pipeline::Route::Keep,
            filter_name: "cat".to_string(),
            score: 0.0,
            context_score: 0.0,
            input_bytes: 1_000,
            output_bytes: 100,
            latency_ms: 1,
            rewind_hash: None,
            segments_kept: 0,
            segments_dropped: 0,
            collapse_savings: None,
            raw_tokens: 250,
            filtered_tokens: 25,
            delivered_bytes: delivered,
        };

        // One call a model read, one written to a terminal.
        store.record_distillation("s1", &row(100), "cat a.txt", "", "claude_code");
        store.record_distillation("s1", &row(0), "cat b.txt", "", "terminal");

        let (count, input, output, ..) = store.aggregate_stats(0).expect("stats");

        assert_eq!(count, 1, "only the delivered call counts");
        assert_eq!(input, 1_000);
        assert_eq!(output, 100);
    }

    /// A row for these two tests; the values are irrelevant, only that it lands.
    fn any_distillation() -> DistillResult {
        DistillResult {
            output: String::new(),
            route: crate::pipeline::Route::Keep,
            filter_name: "git".to_string(),
            score: 0.0,
            context_score: 0.0,
            input_bytes: 1_000,
            output_bytes: 100,
            latency_ms: 1,
            rewind_hash: None,
            segments_kept: 0,
            segments_dropped: 0,
            collapse_savings: None,
            raw_tokens: 250,
            filtered_tokens: 25,
            delivered_bytes: 100,
        }
    }

    /// #822. A marker recorded its shape and never the tool behind the reply,
    /// so a rate could say `project_run` is retrieved more often than
    /// `session_run` and never which surface that lands on. Grouped by both now.
    #[test]
    fn a_marker_records_the_tool_that_produced_the_reply() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open_path(&dir.path().join("omni.db")).expect("store");
        let fold = || FoldRecord {
            source_agent: "claude_code".to_string(),
            origin: "project",
            lines: 12,
            bytes: 300,
            whole_output: false,
            payload_bytes: 1_200,
        };
        let marker = |handle: &str| MarkerRecord {
            kind: "project_run",
            handle: handle.to_string(),
            lines: 12,
            bytes: 300,
        };

        store.ledger_record_folds(
            "/proj",
            "claude_code",
            "s1",
            "Read",
            &[fold()],
            &[marker("aaaa")],
        );
        store.ledger_record_folds(
            "/proj",
            "claude_code",
            "s1",
            "Bash",
            &[fold()],
            &[marker("bbbb")],
        );
        store.record_retrieve_event("omni retrieve", "aaaa", "claude_code");

        let rates = store.marker_retrieve_rates(0);
        let read = rates
            .iter()
            .find(|r| r.tool == "Read")
            .expect("the Read marker is its own row");
        let bash = rates
            .iter()
            .find(|r| r.tool == "Bash")
            .expect("the Bash marker is its own row");
        assert_eq!((read.folds, read.retrieved), (1, 1), "{rates:?}");
        assert_eq!((bash.folds, bash.retrieved), (1, 0), "{rates:?}");
        assert_eq!(read.kind, "project_run");
    }

    /// #665. The one percentage `omni stats` printed was the distiller averaged
    /// over every call OMNI deliberately declined, and `ledger_folds` was not
    /// read at all. On the maintainer's machine that reported 4.5% for a week in
    /// which the distiller took 48% off what it was given and the ledger removed
    /// 1.6 times as many bytes again.
    ///
    /// Both halves are here because either one alone passes with the other
    /// broken: a base that includes the declined calls still reads folds, and a
    /// reader that ignores the ledger still gets the distiller ratio right.
    #[test]
    fn the_declined_calls_stay_out_of_the_base_and_the_folds_are_counted() {
        let (store, _dir) = get_temp_store();

        let mut distilled = any_distillation();
        distilled.input_bytes = 1_000;
        distilled.output_bytes = 200;
        distilled.delivered_bytes = 200;
        store.record_distillation("s1", &distilled, "cargo build", "", "claude_code");

        // Nine declined calls, each larger than the distilled one. Averaged in
        // they drag 80% down to 8%, which is the shape of the defect.
        let mut declined = any_distillation();
        declined.route = crate::pipeline::Route::Passthrough;
        declined.input_bytes = 10_000;
        declined.output_bytes = 10_000;
        declined.delivered_bytes = 10_000;
        for _ in 0..9 {
            store.record_distillation("s1", &declined, "kubectl get pods", "", "claude_code");
        }

        // One fold, two rows: the lines came from two agents, and every row of a
        // fold repeats the same payload size. Summing `payload_bytes` over rows
        // therefore divides 500 saved bytes by 4,000 instead of 2,000.
        store.ledger_record_folds(
            "/proj",
            "claude_code",
            "s1",
            "Bash",
            &[
                FoldRecord {
                    source_agent: "claude_code".to_string(),
                    origin: "project",
                    lines: 40,
                    bytes: 400,
                    whole_output: false,
                    payload_bytes: 2_000,
                },
                FoldRecord {
                    source_agent: "codex".to_string(),
                    origin: "project",
                    lines: 10,
                    bytes: 100,
                    whole_output: false,
                    payload_bytes: 2_000,
                },
            ],
            &[],
        );

        // A second fold, same second, same session, same payload size, and it
        // repeats a (origin, source agent) pair the first one already used. The
        // table has no fold id, so nothing but that repeat distinguishes the two.
        store.ledger_record_folds(
            "/proj",
            "claude_code",
            "s1",
            "Bash",
            &[FoldRecord {
                source_agent: "claude_code".to_string(),
                origin: "project",
                lines: 40,
                bytes: 400,
                whole_output: false,
                payload_bytes: 2_000,
            }],
            &[],
        );

        // A third fold sharing the second, the session, the agent, the scope and
        // the payload size, and overlapping the first two in no (origin, source
        // agent) pair at all. Nothing outside `fold_id` separates it from them.
        store.ledger_record_folds(
            "/proj",
            "claude_code",
            "s1",
            "Bash",
            &[FoldRecord {
                source_agent: "codex".to_string(),
                origin: "session",
                lines: 10,
                bytes: 100,
                whole_output: false,
                payload_bytes: 2_000,
            }],
            &[],
        );

        // A fourth fold with no payload size: summable, never divisible.
        store.ledger_record_folds(
            "/proj",
            "claude_code",
            "s1",
            "Bash",
            &[FoldRecord {
                source_agent: "claude_code".to_string(),
                origin: "session",
                lines: 5,
                bytes: 50,
                whole_output: false,
                payload_bytes: 0,
            }],
            &[],
        );

        // Force every row into one second. The recorder stamps wall clock, so a
        // test cannot otherwise decide whether two folds land together, and the
        // collision these three are here to prove would appear or not by timing.
        store
            .pool
            .get()
            .unwrap()
            .execute("UPDATE ledger_folds SET ts = 1000", [])
            .unwrap();

        let t = store.engine_totals(0).unwrap();

        assert_eq!(t.distilled_calls, 1);
        assert_eq!(t.declined_calls, 9);
        assert_eq!(t.distilled_input, 1_000, "a declined call is not a base");
        assert_eq!(t.distilled_pct().map(|p| p.round()), Some(80.0));

        assert_eq!(t.folds, 4, "four folds across five rows");
        assert_eq!(
            t.fold_bytes, 1_050,
            "bytes are per row and all of them count"
        );
        assert_eq!(t.priced_folds, 3, "three of them record a base");
        assert_eq!(
            t.priced_payload, 6_000,
            "one payload per fold, not one per row"
        );
        // 1,000 removed from 6,000. Two rows deep it would be 1,000 from 2,000,
        // and one row per payload would make it 1,000 from 8,000.
        assert_eq!(t.fold_pct().map(|p| (p * 10.0).round()), Some(167.0));

        // 800 from the distiller, 1,050 from the ledger. Bytes may be summed;
        // the two percentages above may not.
        assert_eq!(t.total_saved(), 1_850);
    }

    /// #118: the "Last distill" reading came from `rewind_store`, which only
    /// gains a row when a distillation had content worth storing for later
    /// retrieval. On a real installation that table was empty beside 8,260
    /// distillations, so `doctor` printed "never [IDLE]" seconds after
    /// distilling, the wrong answer to the one question it is asked.
    #[test]
    fn reports_the_last_distillation_not_the_last_rewind() {
        // Arrange
        let (store, _dir) = get_temp_store();
        store.record_distillation("s1", &any_distillation(), "git status", "", "claude_code");

        // Act
        let (_sessions, last_distill) = store
            .latest_activity_timestamps()
            .expect("timestamps readable");

        // Assert
        assert!(
            last_distill.is_some(),
            "a recorded distillation must set the last-distill reading, \
             even with an empty rewind_store"
        );
    }

    /// #118: `doctor` printed the session count under the label "records", so a
    /// database holding thousands of distillations announced a two-digit number.
    #[test]
    fn counts_distillations_apart_from_sessions() {
        let (store, _dir) = get_temp_store();
        for cmd in ["git status", "git diff", "git log"] {
            store.record_distillation("s1", &any_distillation(), cmd, "", "claude_code");
        }

        let (sessions, _rewinds) = store.stats().expect("stats");

        assert_eq!(store.distillation_count(), 3);
        assert_ne!(
            store.distillation_count(),
            sessions,
            "the two numbers must not be interchangeable"
        );
    }

    /// #566. A `Passthrough` row books a saving the model never received: the
    /// hook sends no reply on that route, so the host keeps its own bytes, while
    /// every byte and token column was computed from the distiller's output.
    /// Across the reporting installation these rows carried 93.0% of every byte
    /// booked as saved, and correcting them moves `omni stats` all-time from a
    /// published 37.1% to 8.3%.
    ///
    /// A `Keep` row in the same table is the control: it really was delivered, so
    /// the migration must not touch it. Without that half the test would pass
    /// against an `UPDATE` with no `WHERE` clause.
    #[test]
    fn passthrough_rows_stop_booking_a_saving() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("omni.db");
        drop(Store::open_path(&db).expect("first open builds the schema"));

        {
            let conn = rusqlite::Connection::open(&db).expect("open seed db");
            conn.execute(
                "DELETE FROM schema_migrations WHERE id = '2026_08_passthrough_rows_book_no_saving'",
                [],
            )
            .expect("re-arm migration");
            for (route, out, delivered, filtered) in
                [("Passthrough", 346, 346, 90), ("Keep", 200, 200, 50)]
            {
                conn.execute(
                    "INSERT INTO distillations
                       (session_id, ts, filter_name, input_bytes, output_bytes, route,
                        latency_ms, command, delivered_bytes, raw_tokens, filtered_tokens)
                     VALUES ('s1', 1700000000, 'grep', 424, ?1, ?2, 1, 'grep -rn x', ?3, 110, ?4)",
                    params![out, route, delivered, filtered],
                )
                .expect("seed row");
            }
        }

        drop(Store::open_path(&db).expect("second open runs the migration"));

        let conn = rusqlite::Connection::open(&db).expect("open assert db");
        let read = |route: &str| -> (i64, i64, i64) {
            conn.query_row(
                "SELECT output_bytes, delivered_bytes, filtered_tokens
                 FROM distillations WHERE route = ?1",
                params![route],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("row")
        };

        assert_eq!(
            read("Passthrough"),
            (424, 424, 110),
            "a passthrough row still books bytes the hook never sent"
        );
        assert_eq!(
            read("Keep"),
            (200, 200, 50),
            "the migration rewrote a row that really was delivered"
        );
    }

    /// Puts rows straight into an existing database and re-arms the dedupe
    /// migration, so the next `open_path` runs it over them. Going through
    /// `record_distillation` would stamp `ts` from the clock, and two calls
    /// landing in the same second is not something a test should depend on.
    fn seed_rows_and_rearm_migration(db: &std::path::Path, rows: &[(&str, i64, i64, i64)]) {
        let conn = rusqlite::Connection::open(db).expect("open seed db");
        conn.execute(
            "DELETE FROM schema_migrations WHERE id = '2026_07_dedupe_double_recorded_distillations'",
            [],
        )
        .expect("re-arm migration");
        for (command, ts, output_bytes, latency_ms) in rows {
            conn.execute(
                "INSERT INTO distillations
                   (session_id, ts, filter_name, input_bytes, output_bytes, route, latency_ms, command)
                 VALUES ('s1', ?1, 'git', 1000, ?2, 'Keep', ?3, ?4)",
                params![ts, output_bytes, latency_ms, command],
            )
            .expect("seed row");
        }
    }

    fn row_count(db: &std::path::Path) -> i64 {
        rusqlite::Connection::open(db)
            .expect("open count db")
            .query_row("SELECT COUNT(*) FROM distillations", [], |r| r.get(0))
            .expect("count")
    }

    /// #118: 1,231 of 8,272 rows on the reporting installation are a second
    /// copy of one command, inflating every count `omni stats` publishes by 15%.
    /// Inside a duplicate group `latency_ms` is the only column that ever
    /// differs, which is what distinguishes one input distilled twice from two
    /// separate commands.
    #[test]
    fn collapses_a_distillation_recorded_twice() {
        // Arrange
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("omni.db");
        drop(Store::open_path(&db).expect("first open builds the schema"));
        seed_rows_and_rearm_migration(
            &db,
            &[
                ("git status", 1_700_000_000, 100, 3),
                ("git status", 1_700_000_000, 100, 5),
            ],
        );

        // Act
        drop(Store::open_path(&db).expect("second open runs the migration"));

        // Assert
        assert_eq!(row_count(&db), 1, "the second copy must be collapsed");
    }

    /// The counter-case. Two rows that differ in something a reader can act on
    /// are two events, and deleting one would under-report a real command.
    #[test]
    fn keeps_rows_that_differ_in_more_than_latency() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("omni.db");
        drop(Store::open_path(&db).expect("first open"));
        seed_rows_and_rearm_migration(
            &db,
            &[
                ("git status", 1_700_000_000, 100, 3),
                ("git status", 1_700_000_000, 250, 3), // different output_bytes
                ("git diff", 1_700_000_000, 100, 3),   // different command
                ("git status", 1_700_000_042, 100, 3), // different second
            ],
        );

        drop(Store::open_path(&db).expect("second open"));

        assert_eq!(row_count(&db), 4, "distinguishable rows must survive");
    }

    /// The migration is recorded, so a later open must not touch rows written
    /// after it ran. Without the marker every open would re-collapse history.
    #[test]
    fn does_not_run_the_dedupe_a_second_time() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("omni.db");
        drop(Store::open_path(&db).expect("first open"));
        seed_rows_and_rearm_migration(
            &db,
            &[
                ("git status", 1_700_000_000, 100, 3),
                ("git status", 1_700_000_000, 100, 5),
            ],
        );
        drop(Store::open_path(&db).expect("second open runs the migration"));

        // A genuine repeat arriving later, which the migration must now ignore.
        let conn = rusqlite::Connection::open(&db).expect("open");
        conn.execute(
            "INSERT INTO distillations
               (session_id, ts, filter_name, input_bytes, output_bytes, route, latency_ms, command)
             VALUES ('s1', 1700000000, 'git', 1000, 100, 'Keep', 9, 'git status')",
            [],
        )
        .expect("late row");
        drop(conn);

        drop(Store::open_path(&db).expect("third open"));

        assert_eq!(
            row_count(&db),
            2,
            "the migration ran again and ate a new row"
        );
    }

    #[test]
    fn record_distillation_does_not_panic() {
        let (store, _dir) = get_temp_store();
        let res = DistillResult {
            output: "hello".to_string(),
            route: crate::pipeline::Route::Keep,
            filter_name: "test_filter".to_string(),
            score: 0.8,
            context_score: 0.1,
            input_bytes: 100,
            output_bytes: 10,
            latency_ms: 12,
            rewind_hash: None,
            segments_kept: 1,
            segments_dropped: 0,
            collapse_savings: None,
            raw_tokens: 20,
            filtered_tokens: 5,
            delivered_bytes: 10,
        };
        // Should not panic
        store.record_distillation("sess_123", &res, "npm start", "", "claude_code");
    }

    /// The `LIMIT 200` this carried was applied to raw commands, but the caller
    /// folds them into shortened keys afterwards, so the cap silently cut rows
    /// the fold would have summed. `node -e` was 21 one-call rows adding up to a
    /// displayed `21x`, all of them below the cut, and the command was rendered
    /// with an agent nobody had resolved (#471).
    #[test]
    fn returns_every_command_not_the_first_two_hundred() {
        let (store, _dir) = get_temp_store();
        let res = DistillResult {
            output: "x".to_string(),
            route: crate::pipeline::Route::Keep,
            filter_name: "f".to_string(),
            score: 0.0,
            context_score: 0.0,
            input_bytes: 100,
            output_bytes: 10,
            latency_ms: 1,
            rewind_hash: None,
            segments_kept: 1,
            segments_dropped: 0,
            collapse_savings: None,
            raw_tokens: 20,
            filtered_tokens: 5,
            delivered_bytes: 10,
        };
        // 250 distinct one-call commands, which is the shape that defeats a cap
        // ordered by call count: every row ties at 1.
        for i in 0..250 {
            store.record_distillation(
                "s",
                &res,
                &format!("node -e script{i}.js"),
                "",
                "claude_code",
            );
        }

        let rows = store.get_per_command_with_agent(0).expect("query");

        assert_eq!(
            rows.len(),
            250,
            "every (command, agent) pair has to survive; a row limit here drops \
             the ones the caller was about to sum"
        );
    }

    fn saving(raw: usize, filtered: usize) -> DistillResult {
        DistillResult {
            output: String::new(),
            route: crate::pipeline::Route::Keep,
            filter_name: "f".to_string(),
            score: 0.0,
            context_score: 0.0,
            input_bytes: raw,
            output_bytes: filtered,
            latency_ms: 0,
            rewind_hash: None,
            segments_kept: 0,
            segments_dropped: 0,
            collapse_savings: None,
            raw_tokens: raw,
            filtered_tokens: filtered,
            delivered_bytes: filtered,
        }
    }

    /// #173. A one-shot command earns no multiplier: nothing is re-sent after
    /// it, so the compounded figure must equal the at-insertion one. Anything
    /// else is a free multiplier, which is the bigger-but-not-truer number this
    /// tracker exists to fight.
    #[test]
    fn gives_a_single_call_session_no_reuse_credit() {
        let (store, _dir) = get_temp_store();
        store.record_distillation("solo", &saving(100, 40), "cargo test", "", "claude_code");

        let (at_insertion, cumulative) = store
            .token_savings_with_reuse(crate::pipeline::CACHE_READ_RATE)
            .expect("query");

        assert_eq!(at_insertion, 60);
        assert_eq!(
            cumulative, 60,
            "a call with no later turns compounds nothing"
        );
    }

    /// The first result of a three-call session is re-sent twice, the second
    /// once, the third never. At a 10% cache-read rate that is
    /// 60×1.2 + 60×1.1 + 60×1.0, and never 60×3.
    #[test]
    fn discounts_reuse_instead_of_multiplying_it() {
        let (store, _dir) = get_temp_store();
        for _ in 0..3 {
            store.record_distillation("multi", &saving(100, 40), "cargo test", "", "claude_code");
        }

        let (at_insertion, cumulative) = store.token_savings_with_reuse(0.10).expect("query");

        assert_eq!(at_insertion, 180);
        assert_eq!(
            cumulative, 198,
            "60*1.2 + 60*1.1 + 60*1.0; a full-price multiplier would be 360"
        );
        assert!(cumulative >= at_insertion);
    }

    /// Sessions are independent: a long one must not lend its turn count to a
    /// short one, which is what a global row ordering would do.
    #[test]
    fn counts_reuse_within_a_session_only() {
        let (store, _dir) = get_temp_store();
        store.record_distillation("a", &saving(100, 40), "cargo test", "", "claude_code");
        for _ in 0..3 {
            store.record_distillation("b", &saving(100, 40), "cargo test", "", "claude_code");
        }

        let (_, cumulative) = store.token_savings_with_reuse(0.10).expect("query");

        assert_eq!(
            cumulative, 258,
            "session a contributes 60, session b contributes 198"
        );
    }

    /// #165. `execution_traces` stores `raw_input` and `distilled_output`
    /// verbatim and was in no cleanup at all, which is why it alone reached 160.1
    /// MB of a 187 MB database while every other table stayed bounded. Its only
    /// reader asks for the newest N, so a trace older than the window answers no
    /// question anyone is posing.
    #[test]
    fn prunes_execution_traces_on_their_own_shorter_window() {
        let (store, _dir) = get_temp_store();
        let now = chrono::Utc::now().timestamp();
        let conn = store.pool.get().expect("conn");
        for (id, age_days) in [("fresh", 1i64), ("stale", 30)] {
            conn.execute(
                "INSERT INTO execution_traces (session_id, command, agent_id, project_path, raw_input, distilled_output, ts)
                 VALUES (?1, 'cargo test', 'claude_code', '.', 'raw', 'out', ?2)",
                params![id, now - age_days * 86400],
            )
            .expect("seed");
        }
        drop(conn);

        // The general retention is far longer than the trace window, so anything
        // pruned here was pruned by the trace rule and not by the shared one.
        store.cleanup_old(365);

        let conn = store.pool.get().expect("conn");
        let remaining: Vec<String> = conn
            .prepare("SELECT session_id FROM execution_traces")
            .expect("prepare")
            .query_map([], |r| r.get(0))
            .expect("query")
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(
            remaining,
            vec!["fresh".to_string()],
            "a trace older than {} days must go even when the shared retention keeps it",
            TRACE_RETENTION_DAYS
        );
    }

    /// `verification_results` had no writer and no reader anywhere in the tree.
    #[test]
    fn drops_the_table_with_neither_a_writer_nor_a_reader() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("omni.db");
        {
            let conn = rusqlite::Connection::open(&db).expect("open");
            conn.execute_batch("CREATE TABLE verification_results (id INTEGER PRIMARY KEY);")
                .expect("seed");
        }

        let _store = Store::open_path(&db).expect("store");

        let left: i64 = rusqlite::Connection::open(&db)
            .expect("reopen")
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'verification_results'",
                [],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(left, 0);
    }

    /// #270. `context_turns` was written on every hooked command, carried an
    /// index, and had no `SELECT` anywhere in the tree: 5,532 rows paying write
    /// latency and disk for a reader that never existed. Opening a store that
    /// still has it must remove it, or the rows keep their index and nothing
    /// reclaims the space.
    #[test]
    fn drops_the_write_only_context_turns_table() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("omni.db");

        // A database from before the drop.
        {
            let conn = rusqlite::Connection::open(&db).expect("open");
            conn.execute_batch(
                "CREATE TABLE context_turns (id INTEGER PRIMARY KEY, session_id TEXT NOT NULL);
                 CREATE INDEX idx_ctx_session ON context_turns(session_id);
                 INSERT INTO context_turns (session_id) VALUES ('old');",
            )
            .expect("seed");
        }

        let _store = Store::open_path(&db).expect("store");

        let conn = rusqlite::Connection::open(&db).expect("reopen");
        let tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'context_turns'",
                [],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(tables, 0, "the table and its rows must be gone");

        let indexes: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'idx_ctx_session'",
                [],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(indexes, 0, "its index costs write latency too");
    }

    /// #379. The drop was gated on a `schema_migrations` row, so once it had run
    /// the table could be recreated by a concurrently installed older binary and
    /// never be removed again. Seeding both the marker row and the table is that
    /// machine, and the drop has to run anyway.
    #[test]
    fn drops_context_turns_even_when_its_migration_is_already_recorded() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("omni.db");

        {
            let conn = rusqlite::Connection::open(&db).expect("open");
            conn.execute_batch(
                "CREATE TABLE schema_migrations (id TEXT PRIMARY KEY, applied_at INTEGER);
                 INSERT INTO schema_migrations (id, applied_at)
                   VALUES ('2026_08_drop_write_only_context_turns', 0);
                 CREATE TABLE context_turns (id INTEGER PRIMARY KEY, session_id TEXT NOT NULL);
                 INSERT INTO context_turns (session_id) VALUES ('resurrected');",
            )
            .expect("seed");
        }

        let _store = Store::open_path(&db).expect("store");

        let conn = rusqlite::Connection::open(&db).expect("reopen");
        let tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'context_turns'",
                [],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(tables, 0, "a recorded migration must not protect the table");
    }

    /// #667. `--detail` prints this pair under a header naming a window, so an
    /// all-time count read as part of that window. The window is the archive's own
    /// timestamp: `retrieved` is a lifetime counter on the row, and re-archiving
    /// refreshes `ts`, so the pair means "archives last written in this window, and
    /// how many of those rows have ever been pulled". Both halves are pinned here
    /// because the wording is what went wrong first.
    #[test]
    fn rewind_metrics_counts_only_the_window() {
        let (store, _dir) = get_temp_store();
        let old = store
            .store_rewind_whole("archived ten days ago")
            .expect("archived");
        store
            .store_rewind_whole("archived just now")
            .expect("archived");

        {
            let conn = store.pool.get().expect("conn");
            conn.execute(
                "UPDATE rewind_store SET ts = ts - 864000 WHERE hash = ?1",
                params![old],
            )
            .expect("backdate");
        }

        let week = chrono::Utc::now().timestamp() - 7 * 86_400;
        assert_eq!(
            store.rewind_metrics(week).expect("metrics").0,
            1,
            "an archive older than the window is not part of it"
        );
        assert_eq!(
            store.rewind_metrics(0).expect("metrics").0,
            2,
            "all time still counts both"
        );

        store
            .store_rewind_whole("archived ten days ago")
            .expect("archived");
        assert_eq!(
            store.rewind_metrics(week).expect("metrics").0,
            2,
            "re-archiving refreshes ts, so the row is in the window it was written in"
        );
    }

    /// #274. The key carried a nanosecond prefix, so every key was unique and
    /// the `INSERT OR IGNORE` under it could never fire: the same output was
    /// archived again on every run. Harmless while the archive never fired at
    /// all, a live disk cost from #271 onward.
    #[test]
    fn archives_identical_content_once() {
        let (store, _dir) = get_temp_store();
        let content = "the same 200 lines of output";

        let first = store.store_rewind_whole(content).expect("archived");
        let second = store.store_rewind_whole(content).expect("archived");

        assert_eq!(first, second, "the key is the content, so it cannot differ");
        assert_eq!(
            store.rewind_metrics(0).expect("metrics").0,
            1,
            "identical content must occupy one row"
        );
        assert_eq!(store.retrieve_rewind(&first), Some(content.to_string()));
    }

    /// #593 review, the first of two. `store::query` calls `retrieve_rewind` to
    /// answer reports, so booking the debt there charged a delivery to a read no
    /// agent made, and the next real fold paid it. The comment on the counter
    /// beside it already said this about `retrieve_rewind`; the flag went in on
    /// the wrong side of it anyway.
    #[test]
    fn a_report_read_does_not_owe_a_delivery() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open_path(&dir.path().join("omni.db")).expect("store");
        let handle = store
            .store_rewind_whole("archived body\n")
            .expect("archived");

        store.retrieve_rewind(&handle).expect("content");

        assert!(
            !store.take_owed_delivery(&handle),
            "a read that no agent asked for created a delivery debt"
        );
    }

    /// #593 review, the second. A flag would let the first delivery clear the
    /// debt of two readers, so the second one is folded back into the marker it
    /// was following. A count pays each of them once.
    #[test]
    fn two_pulls_earn_two_verbatim_deliveries() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open_path(&dir.path().join("omni.db")).expect("store");
        let handle = store
            .store_rewind_whole("archived body\n")
            .expect("archived");

        store.record_rewind_pull(&handle);
        store.record_rewind_pull(&handle);

        assert!(store.take_owed_delivery(&handle), "first pull unanswered");
        assert!(store.take_owed_delivery(&handle), "second pull unanswered");
        assert!(
            !store.take_owed_delivery(&handle),
            "the debt outlived the pulls that created it"
        );
    }

    /// #388. The key used to come back on every path, including a swallowed
    /// insert, so the caller printed `omni_retrieve("<key>")` for a row that was
    /// never written. The archive's whole promise is that the handle resolves.
    #[test]
    fn reports_an_archive_write_that_did_not_land() {
        let (store, _dir) = get_temp_store();
        store
            .pool
            .get()
            .expect("conn")
            .execute("DROP TABLE rewind_store", [])
            .expect("drop");

        assert_eq!(
            store.store_rewind_whole("content nobody can retrieve"),
            None
        );
    }

    /// Different content still gets its own row, so the deduplication is by
    /// identity rather than by collapsing everything into one key.
    #[test]
    fn keeps_distinct_content_apart() {
        let (store, _dir) = get_temp_store();

        let a = store
            .store_rewind_whole("output of the first command")
            .expect("archived");
        let b = store
            .store_rewind_whole("output of the second command")
            .expect("archived");

        assert_ne!(a, b);
        assert_eq!(store.rewind_metrics(0).expect("metrics").0, 2);
        assert_eq!(
            store.retrieve_rewind(&b),
            Some("output of the second command".to_string())
        );
    }

    #[test]
    fn rewinds_and_retrieves_content() {
        let (store, _dir) = get_temp_store();
        let content = "this is some compressed content";
        let hash = store.store_rewind_whole(content).expect("archived");

        // A content address and nothing else. The nanosecond prefix this used to
        // carry is what made `INSERT OR IGNORE` decoration (#274).
        assert_eq!(hash.len(), 16);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));

        let retrieved = store.retrieve_rewind(&hash);
        assert_eq!(retrieved, Some(content.to_string()));

        // Retrieved counts updated
        let conn = store.pool.get().unwrap();
        let count: i32 = conn
            .query_row(
                "SELECT retrieved FROM rewind_store WHERE hash = ?1",
                params![hash],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    /// Was `duplicate_rewind_hashes_are_unique`, which asserted `hash1 != hash2`
    /// for identical content. That was the defect written down as a requirement:
    /// the keys differed only because of a nanosecond prefix, and the prefix is
    /// what stopped the archive from ever deduplicating (#274). The half of it
    /// that stated a real guarantee, that a returned key resolves to its content,
    /// is kept here and in `archives_identical_content_once`.
    #[test]
    fn every_returned_key_resolves_to_its_content() {
        let (store, _dir) = get_temp_store();
        let content = "duplicate me";

        let hash1 = store.store_rewind_whole(content).expect("archived");
        let hash2 = store.store_rewind_whole(content).expect("archived");

        assert_eq!(store.retrieve_rewind(&hash1), Some(content.to_string()));
        assert_eq!(store.retrieve_rewind(&hash2), Some(content.to_string()));
    }

    #[test]
    fn indexes_and_searches_session_events() {
        let (store, _dir) = get_temp_store();
        store.index_event("sess_1", "command", "git status is running fast");
        store.index_event("sess_1", "command", "npm install");
        store.index_event("sess_2", "command", "git status is running"); // diff session

        let res = store.search_session_events("sess_1", "running", 10);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0], "git status is running fast");
    }

    #[test]
    fn fts5_stems_search_terms() {
        let (store, _dir) = get_temp_store();
        store.index_event("sess_2", "log", "The server is running now");

        // Porter stemming makes 'run' match 'running'
        let res = store.search_session_events("sess_2", "run", 10);
        assert_eq!(res.len(), 1);
        assert_eq!(res[0], "The server is running now");
    }

    #[test]
    fn cleanup_removes_stale_entries() {
        let (store, _dir) = get_temp_store();
        let old_ts = chrono::Utc::now().timestamp() - (5 * 86400); // 5 days ago

        let conn = store.pool.get().unwrap();
        conn.execute("INSERT INTO distillations (session_id, ts, filter_name, input_bytes, output_bytes, route, latency_ms) VALUES ('sess_1', ?1, 'f', 1, 1, 'K', 1)", [old_ts]).unwrap();
        drop(conn);

        store.cleanup_old(2); // keep last 2 days

        let conn = store.pool.get().unwrap();
        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM distillations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    // ─── Re-run metric (#109) ───────────────────────────

    fn row(distilled: u64, raw: u64, d_avg: u64, r_avg: u64) -> RerunRow {
        RerunRow {
            filter_name: "f".into(),
            distilled,
            raw,
            distilled_reruns: 0,
            raw_reruns: 0,
            distilled_avg_input: d_avg,
            raw_avg_input: r_avg,
        }
    }

    /// The guard that stopped `kubectl`'s +48.6pp from being published as a
    /// finding: 244,606 B of `get -A` dumps against 115 B of config reads is
    /// not one population, so the delta measures size, not lost signal.
    #[test]
    fn flags_arms_of_wildly_different_size_as_confounded() {
        assert!(row(239, 140, 244_606, 115).is_confounded());
    }

    /// `grep`'s arms are matched (3,196 B vs 3,240 B), so its delta is real.
    #[test]
    fn treats_similarly_sized_arms_as_comparable() {
        assert!(!row(103, 255, 3_196, 3_240).is_confounded());
    }

    #[test]
    fn treats_a_skew_at_the_limit_as_comparable() {
        // Exactly 3×, the limit is exclusive, so this still compares.
        assert!(!row(10, 10, 3_000, 1_000).is_confounded());
        assert!(row(10, 10, 3_001, 1_000).is_confounded());
    }

    /// An empty arm cannot be size-matched, and must not divide by zero.
    #[test]
    fn treats_an_empty_arm_as_confounded_without_panicking() {
        assert!(row(10, 10, 0, 4_000).is_confounded());
        assert!(row(10, 10, 4_000, 0).is_confounded());
        assert!(!row(0, 0, 0, 0).is_confounded());
    }

    #[test]
    fn computes_delta_as_distilled_minus_raw_percentage_points() {
        let r = RerunRow {
            filter_name: "npm".into(),
            distilled: 100,
            raw: 100,
            distilled_reruns: 48,
            raw_reruns: 8,
            distilled_avg_input: 1_000,
            raw_avg_input: 1_000,
        };
        assert_eq!(r.delta_pp(), 40.0);
    }

    #[test]
    fn reports_zero_rate_for_an_arm_with_no_rows() {
        assert_eq!(row(0, 5, 100, 100).distilled_pct(), 0.0);
    }

    fn insert(store: &Store, session: &str, ts: i64, filter: &str, cmd: &str, route: &str) {
        insert_as(store, session, ts, filter, cmd, route, "aider");
    }

    fn insert_as(
        store: &Store,
        session: &str,
        ts: i64,
        filter: &str,
        cmd: &str,
        route: &str,
        agent: &str,
    ) {
        let conn = store.pool.get().unwrap();
        conn.execute(
            "INSERT INTO distillations
             (session_id, ts, filter_name, input_bytes, output_bytes, route,
              score, context_score, latency_ms, command, agent_id)
             VALUES (?1, ?2, ?3, 1000, 500, ?4, 0, 0, 0, ?5, ?6)",
            params![session, ts, filter, route, cmd, agent],
        )
        .unwrap();
    }

    /// Four commands run twice under distillation (so the first of each pair is
    /// a re-run) against eight distinct commands passed through raw.
    fn seed_rerun_fixture(store: &Store, filter: &str) {
        for i in 0..4 {
            let cmd = format!("{filter} task{i}");
            insert(store, "s1", 1000, filter, &cmd, "Keep");
            insert(store, "s1", 1100, filter, &cmd, "Keep");
        }
        for i in 0..8 {
            insert(
                store,
                "s1",
                1000,
                filter,
                &format!("{filter} raw{i}"),
                "Passthrough",
            );
        }
    }

    #[test]
    fn counts_a_repeat_of_the_same_command_within_the_window_as_a_rerun() {
        let (store, _dir) = get_temp_store();
        seed_rerun_fixture(&store, "npm");

        let rows = store.rerun_breakdown(0).unwrap();

        let npm = rows.iter().find(|r| r.filter_name == "npm").unwrap();
        assert_eq!(npm.distilled, 8);
        assert_eq!(npm.raw, 8);
        // Only the first of each pair has a later twin.
        assert_eq!(npm.distilled_reruns, 4);
        assert_eq!(npm.raw_reruns, 0);
        assert_eq!(npm.delta_pp(), 50.0);
    }

    #[test]
    fn ignores_a_repeat_that_falls_outside_the_window() {
        let (store, _dir) = get_temp_store();
        for i in 0..4 {
            let cmd = format!("npm task{i}");
            insert(&store, "s1", 1000, "npm", &cmd, "Keep");
            // One second past RERUN_WINDOW_SECS.
            insert(
                &store,
                "s1",
                1000 + crate::pipeline::RERUN_WINDOW_SECS + 1,
                "npm",
                &cmd,
                "Keep",
            );
        }
        for i in 0..8 {
            insert(
                &store,
                "s1",
                1000,
                "npm",
                &format!("npm raw{i}"),
                "Passthrough",
            );
        }

        let rows = store.rerun_breakdown(0).unwrap();

        assert_eq!(
            rows.iter()
                .find(|r| r.filter_name == "npm")
                .unwrap()
                .distilled_reruns,
            0
        );
    }

    #[test]
    fn does_not_count_a_repeat_from_a_different_session() {
        let (store, _dir) = get_temp_store();
        for i in 0..8 {
            let cmd = format!("npm task{i}");
            insert(&store, "s1", 1000, "npm", &cmd, "Keep");
            insert(&store, "s2", 1100, "npm", &cmd, "Keep");
            insert(
                &store,
                "s1",
                1000,
                "npm",
                &format!("npm raw{i}"),
                "Passthrough",
            );
        }

        let rows = store.rerun_breakdown(0).unwrap();

        assert_eq!(
            rows.iter()
                .find(|r| r.filter_name == "npm")
                .unwrap()
                .distilled_reruns,
            0
        );
    }

    /// Below the sample floor a delta is noise, and publishing it would be the
    /// confident-but-unsupported number this metric exists to catch.
    #[test]
    fn excludes_a_filter_below_the_minimum_sample_size() {
        let (store, _dir) = get_temp_store();
        seed_rerun_fixture(&store, "npm");
        // One short on the distilled arm.
        for i in 0..(crate::pipeline::RERUN_MIN_SAMPLES - 1) {
            insert(&store, "s1", 1000, "rare", &format!("rare a{i}"), "Keep");
        }
        for i in 0..crate::pipeline::RERUN_MIN_SAMPLES {
            insert(
                &store,
                "s1",
                1000,
                "rare",
                &format!("rare b{i}"),
                "Passthrough",
            );
        }

        let rows = store.rerun_breakdown(0).unwrap();

        assert!(rows.iter().any(|r| r.filter_name == "npm"));
        assert!(!rows.iter().any(|r| r.filter_name == "rare"));
    }

    /// Pre-#158 Claude Code `Keep` rows are controls wearing a treatment label;
    /// counting them can zero out a real finding.
    #[test]
    fn excludes_claude_code_rows_recorded_before_the_post_hook_fix() {
        let (store, _dir) = get_temp_store();
        let before = crate::pipeline::POST_HOOK_FIX_TS - 1;
        for i in 0..16 {
            insert_as(
                &store,
                "s1",
                before,
                "ghost",
                &format!("ghost {i}"),
                "Keep",
                "claude_code",
            );
            insert_as(
                &store,
                "s1",
                before,
                "ghost",
                &format!("ghost r{i}"),
                "Passthrough",
                "claude_code",
            );
        }

        let rows = store.rerun_breakdown(0).unwrap();

        assert!(!rows.iter().any(|r| r.filter_name == "ghost"));
    }

    #[test]
    fn keeps_claude_code_rows_recorded_after_the_post_hook_fix() {
        let (store, _dir) = get_temp_store();
        let after = crate::pipeline::POST_HOOK_FIX_TS + 1;
        for i in 0..8 {
            insert_as(
                &store,
                "s1",
                after,
                "live",
                &format!("live {i}"),
                "Keep",
                "claude_code",
            );
            insert_as(
                &store,
                "s1",
                after,
                "live",
                &format!("live r{i}"),
                "Passthrough",
                "claude_code",
            );
        }

        let rows = store.rerun_breakdown(0).unwrap();

        assert!(rows.iter().any(|r| r.filter_name == "live"));
    }

    #[test]
    fn returns_no_rows_when_nothing_has_been_recorded() {
        let (store, _dir) = get_temp_store();
        assert!(store.rerun_breakdown(0).unwrap().is_empty());
    }

    #[test]
    fn ranks_the_worst_offender_first() {
        let (store, _dir) = get_temp_store();
        seed_rerun_fixture(&store, "npm");
        // `quiet` is distilled but never re-run: delta 0 against npm's +50.
        for i in 0..8 {
            insert(&store, "s1", 1000, "quiet", &format!("quiet a{i}"), "Keep");
            insert(
                &store,
                "s1",
                1000,
                "quiet",
                &format!("quiet b{i}"),
                "Passthrough",
            );
        }

        let rows = store.rerun_breakdown(0).unwrap();

        assert_eq!(rows.first().unwrap().filter_name, "npm");
    }

    // ─── Unapplied savings (#163) ───────────────────────

    const BEFORE_FIX: i64 = crate::pipeline::POST_HOOK_FIX_TS - 1;
    const AFTER_FIX: i64 = crate::pipeline::POST_HOOK_FIX_TS + 1;

    /// A `claude_code` row written before #158 landed recorded a saving the host
    /// threw away. Summing it overstates what OMNI actually did.
    #[test]
    fn excludes_unapplied_claude_code_savings_from_the_totals() {
        let (store, _dir) = get_temp_store();
        insert_as(
            &store,
            "s1",
            BEFORE_FIX,
            "git",
            "git log",
            "Keep",
            "claude_code",
        );

        let (count, input, output, ..) = store.aggregate_stats(0).unwrap();

        assert_eq!((count, input, output), (0, 0, 0));
    }

    #[test]
    fn counts_claude_code_savings_recorded_after_the_fix() {
        let (store, _dir) = get_temp_store();
        insert_as(
            &store,
            "s1",
            AFTER_FIX,
            "git",
            "git log",
            "Keep",
            "claude_code",
        );

        let (count, input, output, ..) = store.aggregate_stats(0).unwrap();

        assert_eq!((count, input, output), (1, 1000, 500));
    }

    /// The #158 cutoff is about a hook whose output the host ignored, so it must
    /// not touch an agent that never used that hook, at any timestamp.
    #[test]
    fn counts_other_agents_savings_regardless_of_when_they_were_recorded() {
        let (store, _dir) = get_temp_store();
        insert_as(&store, "s1", BEFORE_FIX, "git", "git log", "Keep", "aider");

        let (count, ..) = store.aggregate_stats(0).unwrap();

        assert_eq!(count, 1);
    }

    /// `terminal` used to be counted here on the grounds that `omni exec` and the
    /// pipe "wrote stdout directly and were always genuine". The compression is
    /// genuine and it is still not a *token* saving: that output goes to a TTY,
    /// no context holds it and nothing is billed. Those rows were 73.4% of every
    /// byte OMNI claimed all-time, which is what made the headline describe
    /// nothing (#212). Split from the timestamp case above because the two
    /// exclusions have nothing to do with each other.
    #[test]
    fn excludes_terminal_rows_from_the_token_headline() {
        let (store, _dir) = get_temp_store();
        insert_as(
            &store, "s1", BEFORE_FIX, "git", "git diff", "Keep", "terminal",
        );

        let (count, ..) = store.aggregate_stats(0).unwrap();

        assert_eq!(count, 0, "TTY bytes are not tokens anyone was billed for");
    }

    /// Excluded rows are reported, not silently missing: a call count that
    /// shrinks without explanation reads as OMNI having stopped working.
    #[test]
    fn reports_unapplied_rows_beside_the_totals_rather_than_dropping_them() {
        let (store, _dir) = get_temp_store();
        for i in 0..3 {
            insert_as(
                &store,
                "s1",
                BEFORE_FIX,
                "git",
                &format!("old {i}"),
                "Keep",
                "claude_code",
            );
        }
        insert_as(&store, "s1", AFTER_FIX, "git", "new", "Keep", "claude_code");

        let rows = store.get_agent_breakdown(0).unwrap();

        let cc = rows.iter().find(|r| r.agent_id == "claude_code").unwrap();
        assert_eq!(cc.calls, 1, "only the applied row counts");
        assert_eq!(cc.unverified, 3, "the excluded rows are still named");
        assert_eq!(cc.input_bytes, 1000, "excluded bytes stay out of the sum");
    }

    /// The rows survive, only their byte columns are disowned. Deleting them
    /// would destroy true latency and command history to remove a false sum.
    #[test]
    fn keeps_the_unapplied_rows_in_the_table() {
        let (store, _dir) = get_temp_store();
        insert_as(
            &store,
            "s1",
            BEFORE_FIX,
            "git",
            "git log",
            "Keep",
            "claude_code",
        );

        let _ = store.aggregate_stats(0).unwrap();

        let conn = store.pool.get().unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM distillations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn reports_no_unapplied_rows_when_there_are_none() {
        let (store, _dir) = get_temp_store();
        insert_as(&store, "s1", AFTER_FIX, "git", "git log", "Keep", "aider");

        let rows = store.get_agent_breakdown(0).unwrap();

        assert!(rows.iter().all(|r| r.unverified == 0));
    }

    #[test]
    fn aggregates_an_empty_table_without_panicking() {
        let (store, _dir) = get_temp_store();
        assert_eq!(store.aggregate_stats(0).unwrap().0, 0);
        assert!(store.get_agent_breakdown(0).unwrap().is_empty());
    }

    /// #773. The table could say what OMNI declined and not who it declined for,
    /// and `session` is not that test: a hook payload can arrive without a
    /// session id, so `host output cap`, a branch that only fires for
    /// `claude_code`, had 20 of its 39 rows carrying none.
    ///
    /// That gap made #608 unsizeable. Its case rests on 12.33 MB of structured
    /// payload having no downstream reader, and the share of that which ever
    /// entered a model's context is the only part the ledger could take.
    #[test]
    fn a_decline_records_which_agent_it_was_declined_for() {
        let (store, _dir) = get_temp_store();
        store.record_passthrough(
            "kubectl get pods -o json",
            900,
            "structured:json",
            "",
            "claude_code",
        );
        store.record_passthrough("jq . big.json", 700, "structured:json", "", "terminal");
        store.record_passthrough("cat notes.md", 100, "below guardrail", "s1", "claude_code");

        let by_agent: std::collections::HashMap<String, (i64, i64)> = store
            .passthrough_bytes_by_agent(1)
            .into_iter()
            .map(|(agent, calls, bytes)| (agent, (calls, bytes)))
            .collect();

        assert_eq!(
            by_agent.get("claude_code").copied(),
            Some((2, 1_000)),
            "the hook door has to be countable on its own: {by_agent:?}"
        );
        assert_eq!(
            by_agent.get("terminal").copied(),
            Some((1, 700)),
            "the pipe door has to be countable on its own: {by_agent:?}"
        );
        assert!(
            by_agent.values().all(|(_, bytes)| *bytes > 0),
            "an empty session must not be read as a door: {by_agent:?}"
        );
    }

    /// #672. Every other time-series table is pruned by `cleanup_old` and this
    /// one was not, so it kept command strings past the window the rest honour.
    /// A user who shortens retention got it everywhere except here.
    #[test]
    fn the_retention_window_reaches_the_passthrough_log() {
        let (store, _dir) = get_temp_store();
        store.record_passthrough(
            "kubectl get pods",
            900,
            "structured:json",
            "s1",
            "claude_code",
        );
        store
            .pool
            .get()
            .unwrap()
            .execute(
                "UPDATE passthrough_events SET ts = strftime('%s','now') - 60 * 86400",
                [],
            )
            .unwrap();
        store.record_passthrough("cargo build", 500, "below guardrail", "s1", "claude_code");

        store.cleanup_old(30);

        let left = store.passthrough_reasons(0);
        assert_eq!(
            left,
            vec![("below guardrail".to_string(), 1)],
            "the 60 day old decline outlived a 30 day window"
        );
    }

    /// #672. Nothing tied a decline to the session that made it, so the reason
    /// breakdown could only be global. A time window is not a substitute: a
    /// third of the sessions on the machine this was measured on overlap another.
    #[test]
    fn a_decline_is_attributed_to_the_session_that_made_it() {
        let (store, _dir) = get_temp_store();
        store.record_passthrough(
            "kubectl get pods",
            900,
            "structured:json",
            "mine",
            "claude_code",
        );
        store.record_passthrough(
            "terraform plan",
            800,
            "structured:json",
            "theirs",
            "claude_code",
        );
        store.record_passthrough(
            "cargo build",
            500,
            "below guardrail",
            "theirs",
            "claude_code",
        );
        // Pipe mode, where the host sends no id at all.
        store.record_passthrough("sort -u", 400, "below guardrail", "", "claude_code");

        assert_eq!(
            store.passthrough_reasons_for(0, Some("mine")),
            vec![("structured:json".to_string(), 1)]
        );
        let theirs = store.passthrough_reasons_for(0, Some("theirs"));
        assert_eq!(theirs.len(), 2, "{theirs:?}");
        assert_eq!(theirs.iter().map(|(_, n)| n).sum::<u64>(), 2);

        // An empty id is not a session. Asking for one must not hand back every
        // *other* session's declines, which is what falling through to the
        // unscoped query did, and what the first version of this assertion
        // locked in by expecting it.
        assert!(
            store.passthrough_reasons_for(0, Some("")).is_empty(),
            "an empty session id answered with the whole table"
        );
        assert_eq!(store.passthrough_reasons_for(0, None).len(), 2);
    }

    /// #605. #603 fixed the label going forward and left 16% of the recorded
    /// corpus filed under an assignment or a binary's full path, which every
    /// workload query groups on equally.
    ///
    /// Run twice on purpose. #379 is the precedent for a migration undone by live
    /// code, and a recompute that is not idempotent is the same hazard wearing a
    /// different hat: the predicate that selects a stale row must not match the
    /// row the migration just wrote.
    #[test]
    fn the_recompute_fixes_a_stale_label_and_does_not_run_twice() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("omni.db");
        drop(Store::open_path(&db).expect("first open builds the schema"));

        let seed = [
            ("A=$(kubectl", "A=$(kubectl get pods -n web)"),
            ("/bin/ls", "/bin/ls -l /tmp"),
            ("S=/tmp/x", "S=/tmp/x cargo build --release"),
            // Already correct, and it must come out unchanged.
            ("cargo", "cargo build --release"),
        ];
        {
            let conn = rusqlite::Connection::open(&db).expect("open seed db");
            conn.execute(
                "DELETE FROM schema_migrations
                 WHERE id = '2026_08_recompute_filter_name_from_command'",
                [],
            )
            .expect("re-arm migration");
            for (i, (name, command)) in seed.iter().enumerate() {
                conn.execute(
                    "INSERT INTO distillations
                        (id, session_id, ts, filter_name, input_bytes, output_bytes,
                         route, latency_ms, command)
                     VALUES (?1, 's1', 1, ?2, 100, 50, 'Keep', 1, ?3)",
                    params![i as i64 + 1, name, command],
                )
                .expect("seed row");
            }
        }

        let labels = |db: &std::path::Path| -> Vec<String> {
            let store = Store::open_path(db).expect("reopen runs the migration");
            let conn = store.pool.get().unwrap();
            let mut stmt = conn
                .prepare("SELECT filter_name FROM distillations ORDER BY id")
                .unwrap();
            stmt.query_map([], |r| r.get::<_, String>(0))
                .unwrap()
                .flatten()
                .collect()
        };

        let after = labels(&db);
        assert_eq!(
            after,
            vec![
                "kubectl".to_string(),
                "ls".to_string(),
                "cargo".to_string(),
                "cargo".to_string()
            ],
            "a stale label survived the recompute"
        );

        // Second open. The migration is recorded, so nothing runs, and even if it
        // did the recompute is its own fixed point.
        assert_eq!(labels(&db), after, "a second open changed the labels again");
    }

    /// The recompute reads `command` and nothing else, so a row that lost its
    /// command has no arithmetic to recover and is left exactly as it is rather
    /// than relabelled from a guess.
    #[test]
    fn a_row_with_no_command_keeps_its_stale_label() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("omni.db");
        drop(Store::open_path(&db).expect("first open builds the schema"));
        {
            let conn = rusqlite::Connection::open(&db).expect("open seed db");
            conn.execute(
                "DELETE FROM schema_migrations
                 WHERE id = '2026_08_recompute_filter_name_from_command'",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO distillations
                    (id, session_id, ts, filter_name, input_bytes, output_bytes,
                     route, latency_ms, command)
                 VALUES (1, 's1', 1, 'A=$(kubectl', 100, 50, 'Keep', 1, '')",
                [],
            )
            .unwrap();
        }

        let store = Store::open_path(&db).expect("reopen runs the migration");
        let conn = store.pool.get().unwrap();
        let name: String = conn
            .query_row("SELECT filter_name FROM distillations", [], |r| r.get(0))
            .unwrap();
        assert_eq!(name, "A=$(kubectl");
    }
}
