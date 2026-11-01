use anyhow::{Context, Result};
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, OptionalExtension, params};

use crate::paths;
use crate::pipeline::DistillResult;
use crate::pipeline::SessionState;

/// Per-connection pragmas. The cache settings are the in-process cache tier
/// (#393): the default 2 MB cache held about 1% of a 54 MB working set.
/// `busy_timeout` is absent on purpose: rusqlite already sets 5000 ms, and
/// setting it here could only lower it.
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

/// How long a verbatim execution trace is kept. Shorter than the rest: a row
/// holds raw and distilled output in full, and the table was 160 MB of 187 MB
/// before it had any cleanup (#165).
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
    /// Opens the default database, creating its directory and running migrations.
    pub fn open() -> Result<Self> {
        let db_path = if let Ok(custom_path) = std::env::var("OMNI_DB_PATH") {
            std::path::PathBuf::from(custom_path)
        } else {
            paths::database_path()
        };

        Self::open_path(&db_path)
    }

    pub fn open_path(path: &std::path::Path) -> Result<Self> {
        let parent = path.parent().unwrap_or_else(|| std::path::Path::new(""));
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).context("Failed to create .omni directory")?;
            // Only a directory OMNI chose: `OMNI_DB_PATH` may point at a shared one (#631).
            if parent == paths::data_home() || parent == paths::config_home() {
                let _ = paths::restrict_to_owner(parent);
            }
        }

        let manager = SqliteConnectionManager::file(path);
        // One idle connection: r2d2 otherwise opens `max_size` connections before
        // any work, which cost a run-once hook 3.19 ms against 1.26 ms. The pool stays
        // because the MCP server shares this type (#174).
        let pool = Pool::builder()
            .max_size(4)
            .min_idle(Some(1))
            .connection_customizer(Box::new(PragmaCustomizer))
            .build(manager)
            .context("Failed to create SQLite connection pool")?;

        let store = Self { pool };
        store.init_schema()?;

        // After the schema, since WAL creates the sidecars. The file is OMNI's own,
        // so it is tightened wherever it lives (#631). Sidecar names append to the
        // whole filename (#446).
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

    /// How many distillations the database holds; `doctor` once printed the session
    /// count as "records" (#118).
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
        // From `distillations`, not `rewind_store`, which rarely gains rows: `doctor`
        // read "never" seconds after a distilled command (#118).
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

    pub fn aggregate_stats(&self, since: i64) -> Result<(u64, u64, u64, u64, i64, u64, u64)> {
        let conn = self.pool.get().context("DB pool exhausted")?;
        // (count, total_input, total_output, sum_latency, max_latency, raw_tokens, filtered_tokens)
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

    /// Per filter: (name, count, avg_reduction_pct).
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

    /// Per-filter re-run rate, distilled against raw (#109): the same command in
    /// the same session within `RERUN_WINDOW_SECS`, i.e. the agent needed more.
    /// Claude Code rows before `POST_HOOK_FIX_TS` are excluded: nothing was applied
    /// there, so they are controls labelled as treatment.
    ///
    /// ponytail: correlated `EXISTS` per row is O(n²); fine at ~7k rows, else
    /// materialise the window join keyed on `(session_id, cmd)`.
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

        // Worst offender first.
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
            // A declined call has no saving and no base; averaging it in turned 48% into 9%.
            if route == "Passthrough" {
                t.declined_calls += calls;
                t.declined_bytes += input;
            } else {
                t.distilled_calls += calls;
                t.distilled_input += input;
                t.distilled_output += output;
            }
        }

        // Rows are not folds: one fold writes a row per (origin, source agent) with the
        // same `payload_bytes`, so count and sum by `fold_id`. Rows from before that
        // column were backfilled by inference, so old counts are lower bounds.
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

        // Rows with `payload_bytes` 0 have no base; counted apart so the ratio is
        // not a two-population error (#533).
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

    /// Archives last written since `since` (0 is all time), and how many were ever
    /// pulled. `retrieved` is a lifetime counter and `ts` refreshes on re-archive,
    /// so neither is strictly inside the window. Not reset on re-archive: #581
    /// owes one verbatim delivery per pull (#667).
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

    /// (event_count, total_original_lines, total_collapsed_lines)
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
            -- engrams is no longer written or read (#997). Kept so existing rows survive.
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

        // Legacy `content_type` (v0.5.6).
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

        // Older rows carry the reason inside `command` (#441).
        let _ = conn.execute(
            "ALTER TABLE passthrough_events ADD COLUMN reason TEXT NOT NULL DEFAULT 'unrecorded'",
            [],
        );
        // Which door a decline came through (#773). Older rows read `unknown`: it
        // cannot be recovered, so queries on the split must window by ts.
        let _ = conn.execute(
            "ALTER TABLE passthrough_events ADD COLUMN agent_id TEXT NOT NULL DEFAULT 'unknown'",
            [],
        );
        // Older rows read 0 for "not recorded"; window by ts.
        let _ = conn.execute(
            "ALTER TABLE ledger_folds ADD COLUMN whole_output INTEGER NOT NULL DEFAULT 0",
            [],
        );
        let _ = conn.execute(
            "ALTER TABLE ledger_folds ADD COLUMN payload_bytes INTEGER NOT NULL DEFAULT 0",
            [],
        );
        // Older rows have no session, honestly (#602).
        let _ = conn.execute(
            "ALTER TABLE ledger_folds ADD COLUMN session TEXT NOT NULL DEFAULT ''",
            [],
        );
        // Which tool produced the reply (#822). Older rows: empty.
        let _ = conn.execute(
            "ALTER TABLE fold_markers ADD COLUMN tool TEXT NOT NULL DEFAULT ''",
            [],
        );
        // Groups the rows of one fold (#665). Older rows are backfilled by inference
        // on (second, session, agent, scope, payload size): the smallest id per group.
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
        // One row per marker emitted, with its handle, so wording and retrievals can be
        // joined (#766). `kind` comes from the code that renders the string, never
        // from parsing it back.
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

        // Ties a decline to its session (#672). Empty means the host sent no id.
        let _ = conn.execute(
            "ALTER TABLE passthrough_events ADD COLUMN session TEXT NOT NULL DEFAULT ''",
            [],
        );
        let _ = conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_pt_session ON passthrough_events(session)",
            [],
        );

        // Set on pull, cleared by the delivery that answers it, so the fetched
        // content is not folded straight back into its marker (#581). Separate from
        // the lifetime `retrieved` counter.
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
        // The first agent to emit the line into the scope (#509).
        let _ = conn.execute(
            "ALTER TABLE ledger_lines ADD COLUMN agent_id TEXT NOT NULL DEFAULT 'unknown'",
            [],
        );
        // What command first showed this line, so a marker can name a different
        // source (#622). Older rows: empty, which suppresses the clause.
        let _ = conn.execute(
            "ALTER TABLE ledger_lines ADD COLUMN source TEXT NOT NULL DEFAULT ''",
            [],
        );
        // #911: older rows keep their text in `source`; `ledger_seen` reads either.
        let _ = conn.execute(
            "ALTER TABLE ledger_lines ADD COLUMN source_id INTEGER NOT NULL DEFAULT 0",
            [],
        );
        // What the model actually read, distinct from what the distiller returned
        // (#212). Older rows are -1, "unknown", not backfilled.
        let _ = conn.execute(
            "ALTER TABLE distillations ADD COLUMN delivered_bytes INTEGER DEFAULT -1",
            [],
        );

        // One-time, idempotent: Cursor sessions once recorded as `vscode`.
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

        // `was_resolved` was never cleared on recurrence (#427); history cannot be
        // re-derived, so the label is cleared once and re-earned.
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

        // Passthrough rows booked savings nobody received: the host kept its bytes
        // (#566). They were most of the gap between a published 37.1% and a true
        // 8.3%. Zeroed in the column, not in six queries; `execution_traces` still
        // has the distiller output. Redacted passthroughs are zeroed too, undercounting
        // by the few bytes `[REDACTED]` saves.
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

        // Recompute `filter_name` on old rows (#605): #603 fixed new ones only, and
        // 16% of the corpus was grouped under assignments or full paths. Safe because
        // the source column is intact and `producer_label` is pure. Must run after
        // #677, or `A=$(ls` would become `-1t`.
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

        // Dropped tables: `context_turns` had no reader (#270), `verification_results`
        // had neither writer nor reader (#165).
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

        // Unconditional, not migration-gated: an older binary running alongside
        // recreated the table after a one-time drop (#379).
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
            // 3.8 chars per token for the backfill.
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

        // One-time: collapse distillations recorded twice for one command (#118),
        // 15% of the reporting table, all historical. Keyed on every column except
        // `id` and `latency_ms`, the only one that varied inside a duplicate group.
        // No UNIQUE index: it would also reject a real repeat within one second.
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

    /// Row counts for the hook-routing tests, without the `sqlite3` binary.
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

    /// Records that a payload was handed back untouched, and why, in its own
    /// `reason` column (#441): a JSON passthrough and an unparseable one call for
    /// opposite work.
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

    /// What each door declined, in bytes, over the last `days`, grouped by agent
    /// (#773). Older rows read `unknown`, hence the window.
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

    /// How many payloads each gate declined.
    pub fn passthrough_reasons(&self, since: i64) -> Vec<(String, u64)> {
        self.passthrough_reasons_for(since, None)
    }

    /// One session's declines, or every session's for `None`. `Some("")` returns
    /// nothing: an empty id is shared by every pipe-mode row.
    pub fn passthrough_reasons_for(&self, since: i64, session: Option<&str>) -> Vec<(String, u64)> {
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

    /// Newest first; only tests read this table.
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

    /// Archives `content` under its hash and returns the key, or `None` when the
    /// row did not land: a handle for missing content breaks the one promise this
    /// store makes (#388). The key is the content address only (#274). A repeat
    /// refreshes `ts` so live content does not age out; `retrieved` is untouched.
    /// `whole_len` is the payload `content` was cut from, so `omni retrieve` can
    /// say whether it holds the whole answer (#627).
    pub fn store_rewind(&self, content: &str, whole_len: usize) -> Option<String> {
        // The reserved example handle must never name real content (#583).
        let rewind_key =
            crate::util::text::avoid_example_handle(crate::util::text::short_hash(content));

        let conn = self.pool.get().ok()?;

        let ts = chrono::Utc::now().timestamp();
        // The whole is never smaller than the content itself.
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

    /// Records a pull from any door; `get_retrieve_rate` raises route thresholds
    /// from it (#512). Not inside `retrieve_rewind`, which reports also call.
    pub fn record_rewind_pull(&self, hash: &str) {
        // Booked here, not in `retrieve_rewind`, for the same reason (#581, #593
        // review). A count, so two readers pulling one handle each get a delivery.
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

    /// Consumes the debt a pull left behind, and says whether there was one. The
    /// retrieved bytes pass the hook again and would fold back into the marker
    /// that sent the reader there (#581). One delivery, cleared in the same
    /// `UPDATE` so racing hooks cannot both claim it.
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

    /// Test-only: archives a payload that is its own whole.
    #[cfg(test)]
    pub fn store_rewind_whole(&self, content: &str) -> Option<String> {
        self.store_rewind(content, content.len())
    }

    pub fn retrieve_rewind(&self, hash: &str) -> Option<String> {
        self.retrieve_rewind_sized(hash).map(|(content, _)| content)
    }

    /// The archived block and the size of the payload it was cut from (#627).
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

    /// Recent distillations, optionally for one session. The MCP server passes
    /// `None`: its minted id never matches the host's id the hook rows carry
    /// (#602), so it asks by recency instead.
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

    /// Recent cross-turn folds for a session, which `distillations` never holds
    /// (#602). Keyed on the `session` column, not `scope`, which holds the project
    /// path (#625 review).
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

    /// The most recent session this agent had in this project, through
    /// `agent_sessions`, since `sessions` has no project column. `None` for a new
    /// project means start fresh (#482).
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

    /// Per-command totals in the window, most-called first. `limit` 0 means all:
    /// the cap ranks by calls, so a caller ranking by bytes must ask for all.
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

    /// Per-agent totals over rows that reached an agent. `unverified` counts the
    /// rows left out (pre-#158 Claude Code, and results no model read, #212) so a
    /// shrinking count reads as a correction (#163).
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

    /// Every `(command, agent)` pair in the window, unaggregated and unlimited: the
    /// caller folds them into shortened keys, and a raw-row cap dropped rows the
    /// fold needed (#471).
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

    /// Per period: (label, count, input_bytes, output_bytes, raw_tokens, filtered_tokens)
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

    /// Upsert; a conflict bumps `times_confirmed` and updates the value.
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

    /// Which of `hashes` this scope has already been shown, and to whom. A bounded
    /// `IN` probe, chunked under SQLite's parameter limit, rather than loading the
    /// scope. The agent is the first writer, so a fold can say whose bytes it
    /// replaced (#533). Fails to an empty map: a missed fold, never a false claim.
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
        // Distinct first, so a repeated line is asked about once.
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

    /// Records every line of an emitted block against a scope, unconditionally:
    /// a block is worth remembering because it may recur. One transaction and one
    /// cached statement for the batch.
    pub fn ledger_record(&self, scope: &str, hashes: &[String], agent_id: &str, source: &str) {
        let Ok(mut conn) = self.pool.get() else {
            return;
        };
        let ts = chrono::Utc::now().timestamp();
        let Ok(tx) = conn.transaction() else {
            return;
        };
        // The command is stored once (#911); its `ts` moves so the prune keeps it.
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
            // The conflict keeps the first writer's agent and source, which the marker's
            // claim rests on (#622), but refreshes `ts`: freshness asks when the bytes
            // last reached an agent (#795). Only fully delivered lines reach here.
            for h in hashes {
                let _ = stmt.execute(params![scope, h, ts, agent_id, source_id]);
            }
        }
        let _ = tx.commit();
    }

    /// Retrieve rate per marker shape over the last `days`, joined on the handle.
    /// Markers before #766 are absent, so window the query.
    ///
    /// `EXISTS`, not a join: a handle is a content hash, so a join multiplies a
    /// marker by every retrieval of the same content (#771 review). Two markers of
    /// one content in different shapes are both credited by one pull. `>=` because
    /// timestamps are whole seconds and pulls follow markers within seconds.
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

    /// Records what one call's folds drew on, by origin and source agent. Written
    /// after the view is decided, so a failed write costs a measurement, never a
    /// marker.
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
        // One `fold_id` for every row of this call (#665); `MAX(id)` is a rowid lookup.
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

    /// Forgets everything a scope was shown, at compaction, and returns how many
    /// lines. After compaction the agent no longer holds those bytes (#401).
    pub fn ledger_forget(&self, session: &str) -> usize {
        let Ok(conn) = self.pool.get() else {
            return 0;
        };
        // Subagent scopes `<session>/<agent>` go too (#807). `substr`, not `LIKE`, so
        // `_` or `%` in an id cannot widen the delete.
        conn.execute(
            "DELETE FROM ledger_lines
              WHERE scope = ?1 OR substr(scope, 1, length(?1) + 1) = ?1 || '/'",
            params![session],
        )
        .unwrap_or(0)
    }

    /// Pages in the file and how many are free, or `None`. Pruned pages stay on the
    /// freelist with `auto_vacuum = 0`: once 36,264 of 50,226 pages.
    pub fn page_stats(&self) -> Option<(i64, i64)> {
        let conn = self.pool.get().ok()?;
        let pages = conn.query_row("PRAGMA page_count", [], |r| r.get(0)).ok()?;
        let free = conn
            .query_row("PRAGMA freelist_count", [], |r| r.get(0))
            .ok()?;
        Some((pages, free))
    }

    /// Rewrites the file without its freelist. Operator-triggered only: it locks
    /// and needs free disk equal to the file. `auto_vacuum = INCREMENTAL` would tax
    /// every commit to reclaim one prune a year.
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
        // Was never pruned (#672).
        let _ = conn.execute(
            "DELETE FROM passthrough_events WHERE ts < ?1",
            params![ts_threshold],
        );
        // Ledger eviction is by time, one window for both scopes: a project that has
        // not emitted a line in 30 days has stopped emitting it. A size cap would
        // evict the busiest project first, where the repeats are. New tables get
        // their prune in the same commit as their schema.
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
        // A source a surviving line still points at stays, however old its own `ts`.
        let _ = conn.execute(
            "DELETE FROM ledger_sources WHERE ts < ?1
               AND id NOT IN (SELECT source_id FROM ledger_lines)",
            params![ts_threshold],
        );

        // `ttl_days` per row, by `last_seen`, so reconfirmed facts stay (#438).
        let now = chrono::Utc::now().timestamp();
        let _ = conn.execute(
            "DELETE FROM loop_memory WHERE last_seen < ?1 - (COALESCE(ttl_days, 30) * 86400)",
            params![now],
        );

        // Traces get a shorter window: two orders of magnitude heavier per row, and
        // only the newest are read (#165). `OMNI_TRACE_RETENTION_DAYS` holds it open
        // during a measurement (#440); an unparseable value is ignored.
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

    /// Whether the database is writable, e.g. under a sandbox.
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

    /// How long a session lasts before the host ends it, in commands, over closed
    /// sessions only (#435). Returns `(sessions, median_commands, longest,
    /// ended_by_compaction)`.
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

    /// Records that a memory tool was asked, empty answers included: the question
    /// is whether anyone asks (#272). `omni_retrieve` keeps its own rule, since its
    /// counter feeds the adaptive rate.
    pub fn record_memory_read(&self, tool: &str, subject: &str) {
        let agent_id = std::env::var("OMNI_AGENT_ID")
            .unwrap_or_else(|_| crate::agents::multiagent::detect_agent_id());
        self.record_retrieve_event(tool, crate::util::text::safe_slice(subject, 60), &agent_id);
    }

    /// Tokens saved at insertion, and compounded over later turns:
    /// `delta × (1 + turns_after × CACHE_READ_RATE)`. An estimate (#173);
    /// `cumulative >= at_insertion` always.
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

    /// How many times `tool` handed memory back, all time (#272).
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

    /// Retrieve rate for a command prefix, 0.0 to 1.0; high means OMNI is too aggressive.
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

    /// Agents active on this project within the last 8 hours.
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

    /// Resolved when a command in the family succeeds after a failure.
    pub fn resolve_pattern(&self, tool_family: &str, resolution_hint: &str) {
        let now = chrono::Utc::now().timestamp();
        let conn = match self.pool.get() {
            Ok(c) => c,
            Err(_) => return,
        };
        // Only patterns seen in the last hour.
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

    /// Recurring patterns for a tool family, most frequent first.
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

/// What each engine removed, and the base each removed it from (#665). The
/// two ratios come from different populations: sum bytes, never percentages.
#[derive(Debug, Default, Clone, Copy)]
pub struct EngineTotals {
    pub distilled_calls: u64,
    pub distilled_input: u64,
    pub distilled_output: u64,
    /// Calls OMNI declined; averaged in, they turned 48% into 9%.
    pub declined_calls: u64,
    /// Bytes the declined calls carried, kept out of every ratio (#612).
    pub declined_bytes: u64,
    /// Fold operations, not `ledger_folds` rows.
    pub folds: u64,
    pub fold_bytes: u64,
    /// Folds that record their payload size, the only valid base for the fold ratio.
    pub priced_folds: u64,
    pub priced_fold_bytes: u64,
    pub priced_payload: u64,
    pub first_fold_ts: Option<i64>,
}

impl EngineTotals {
    pub fn distilled_saved(&self) -> u64 {
        self.distilled_input.saturating_sub(self.distilled_output)
    }

    /// Bytes neither engine let through, absolute because percentages may not be summed.
    pub fn total_saved(&self) -> u64 {
        self.distilled_saved() + self.fold_bytes
    }

    pub fn distilled_pct(&self) -> Option<f64> {
        (self.distilled_input > 0)
            .then(|| 100.0 * self.distilled_saved() as f64 / self.distilled_input as f64)
    }

    /// `None` when no fold records a payload size: no base, no ratio.
    pub fn fold_pct(&self) -> Option<f64> {
        (self.priced_payload > 0)
            .then(|| 100.0 * self.priced_fold_bytes as f64 / self.priced_payload as f64)
    }
}

#[derive(Debug, Clone)]
/// How often one marker shape sent a reader back for the bytes (#766). A high
/// rate means the shape did not answer the question it interrupted.
pub struct MarkerRate {
    pub kind: String,
    /// The tool whose reply the marker replaced; empty before #822.
    pub tool: String,
    pub folds: i64,
    pub retrieved: i64,
}

/// One marker a reader was handed, finer than `ledger_folds`. `kind` comes from
/// the rendering arm (#766); `handle` joins later retrievals back to it.
pub struct MarkerRecord {
    pub kind: &'static str,
    pub handle: String,
    pub lines: usize,
    pub bytes: usize,
}

/// One fold's evidence: whose lines it replaced and how much, grouped by the
/// caller so twenty runs from one agent write one row.
pub struct FoldRecord {
    /// The agent shown these lines first.
    pub source_agent: String,
    /// `session` or `project`, a string because SQL reads it far more than Rust.
    pub origin: &'static str,
    pub lines: usize,
    pub bytes: usize,
    /// Every line was folded. Per call, repeated on each row of it.
    pub whole_output: bool,
    /// The whole payload, repeated on each row so the floor audit is a row
    /// predicate; whole-second `ts` cannot key a call.
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

/// What the ledger remembers about a line: `source` is the command that showed
/// it first, empty before #622, which suppresses the marker clause.
#[derive(Debug, Clone)]
pub struct SeenLine {
    pub agent: String,
    pub source: String,
}

/// One cross-turn fold for `omni_explain_savings` (#602). No command: the
/// folded bytes may have been shown by a different one.
#[derive(Debug)]
pub struct LedgerFoldRow {
    pub origin: String,
    pub lines: usize,
    pub bytes: usize,
    pub whole_output: bool,
}

/// One filter's re-run comparison (#109). `Passthrough` rows are the control:
/// a distilled arm re-run more often than raw removed something needed.
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

    /// Points by which distilling raised the re-run rate; positive cost a second run.
    pub fn delta_pp(&self) -> f64 {
        self.distilled_pct() - self.raw_pct()
    }

    /// Whether the arms differ too much in size to compare (`RERUN_SIZE_SKEW_LIMIT`):
    /// then the delta measures input size, not lost signal.
    pub fn is_confounded(&self) -> bool {
        let (lo, hi) = if self.distilled_avg_input <= self.raw_avg_input {
            (self.distilled_avg_input, self.raw_avg_input)
        } else {
            (self.raw_avg_input, self.distilled_avg_input)
        };
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
    pub calls: u64,
    pub input_bytes: u64,
    pub output_bytes: u64,
    /// Calls left out above: pre-#158 Claude Code rows never applied.
    pub unverified: u64,
}

/// SQL restricting savings sums to rows whose distillation reached an agent
/// (#163). Pre-`POST_HOOK_FIX_TS` Claude Code rows were dropped by the host
/// (#158), and terminal rows were read by a human, not billed (#212: 73.4% of
/// all claimed savings). Gates the sums, not the rows, which stay true history.
/// `delivered_bytes` -1 means "not yet recorded". Interpolated because the
/// timestamp is a compile-time constant.
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

impl SqliteBackend {
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

        // Score descending.
        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(limit);

        results
    }
}

impl SqliteBackend {
    /// Logged by `omni_recall` for adaptive scoring.
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

    /// One knowledge value by exact key, e.g. `__omni_goal__`.
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

    /// Commands recalled at least `threshold` times in 7 days, for `omni_adaptive_insights`.
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

    /// Knowledge entries never recalled.
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

#[derive(Debug, Clone)]
pub struct KnowledgeHit {
    pub key: String,
    pub value: String,
    pub confidence: f32,
    pub project_hash: String,
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

    /// One stored command per call; pre-#911 rows answer with their own text.
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

    /// Loosened by hand then reopened: a strict-umask runner would pass with no
    /// code, and existing installs need tightening on the next run (#631).
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

    /// Driven through `Ledger::project`: a hand-written fixture with an invented
    /// scope passed against a fix that returned nothing in production (#602).
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

        // As the MCP server asks: it cannot see the host session id.
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

    /// The override holds traces open; the default stays at #165's window.
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

    /// A recurring pattern loses its resolved flag (#427).
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

    /// `ttl_days` is honoured (#438).
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

    /// The headline `omni stats` leads with.
    #[test]
    fn reports_the_median_session_and_how_many_hit_a_compaction() {
        let (store, _d) = get_temp_store();
        for (id, commands, reason) in [
            ("s1", 10u32, "clear"),
            ("s2", 50, "auto_compact"),
            ("s3", 200, "logout"),
            // Never closed, so excluded from the median.
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

    /// Terminal rows are not token savings (#212).
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

        store.record_distillation("s1", &row(100), "cat a.txt", "", "claude_code");
        store.record_distillation("s1", &row(0), "cat b.txt", "", "terminal");

        let (count, input, output, ..) = store.aggregate_stats(0).expect("stats");

        assert_eq!(count, 1, "only the delivered call counts");
        assert_eq!(input, 1_000);
        assert_eq!(output, 100);
    }

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

    /// Rates group by tool as well as shape (#822).
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

    /// Declined calls are out of the base and folds are read (#665); either half
    /// alone passes with the other broken.
    #[test]
    fn the_declined_calls_stay_out_of_the_base_and_the_folds_are_counted() {
        let (store, _dir) = get_temp_store();

        let mut distilled = any_distillation();
        distilled.input_bytes = 1_000;
        distilled.output_bytes = 200;
        distilled.delivered_bytes = 200;
        store.record_distillation("s1", &distilled, "cargo build", "", "claude_code");

        // Nine large declined calls: averaged in, 80% reads as 8%.
        let mut declined = any_distillation();
        declined.route = crate::pipeline::Route::Passthrough;
        declined.input_bytes = 10_000;
        declined.output_bytes = 10_000;
        declined.delivered_bytes = 10_000;
        for _ in 0..9 {
            store.record_distillation("s1", &declined, "kubectl get pods", "", "claude_code");
        }

        // One fold, two rows from two agents, same payload size: sum per fold, not per row.
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

        // A second fold in the same second that repeats an (origin, agent) pair.
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

        // A third fold that only `fold_id` separates from the first two.
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

        // One second for every row, so the collision does not depend on timing.
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
        // 1,000 removed from 6,000; per-row sums would give 2,000 or 8,000.
        assert_eq!(t.fold_pct().map(|p| (p * 10.0).round()), Some(167.0));

        // Bytes add up; the two percentages may not.
        assert_eq!(t.total_saved(), 1_850);
    }

    /// "Last distill" reads `distillations`, not the rarely written `rewind_store` (#118).
    #[test]
    fn reports_the_last_distillation_not_the_last_rewind() {
        let (store, _dir) = get_temp_store();
        store.record_distillation("s1", &any_distillation(), "git status", "", "claude_code");

        let (_sessions, last_distill) = store
            .latest_activity_timestamps()
            .expect("timestamps readable");

        assert!(
            last_distill.is_some(),
            "a recorded distillation must set the last-distill reading, \
             even with an empty rewind_store"
        );
    }

    /// `doctor`'s record count is distillations, not sessions (#118).
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

    /// Passthrough rows book no saving (#566). The `Keep` row is the control, so an
    /// `UPDATE` without a `WHERE` fails.
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

    /// Inserts rows directly and re-arms the dedupe migration, so two rows land in
    /// one second without depending on the clock.
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

    /// Duplicates differing only in `latency_ms` collapse to one (#118).
    #[test]
    fn collapses_a_distillation_recorded_twice() {
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

        drop(Store::open_path(&db).expect("second open runs the migration"));

        assert_eq!(row_count(&db), 1, "the second copy must be collapsed");
    }

    /// Rows that differ in anything else are two events and both stay.
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

    /// Recorded once, so later rows are never re-collapsed.
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
        store.record_distillation("sess_123", &res, "npm start", "", "claude_code");
    }

    /// No row cap: the caller's fold needs every row (#471).
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
        // 250 one-call commands, all tied at 1, defeat a cap ordered by calls.
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

    /// A one-shot command earns no multiplier (#173).
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

    /// 60×1.2 + 60×1.1 + 60×1.0 at a 10% cache-read rate, never 60×3.
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

    /// A long session must not lend its turn count to a short one.
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

    /// Traces are pruned by their own shorter window (#165).
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

        // The general window is far longer, so anything pruned here was the trace rule.
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

    /// Opening an old store drops `context_turns` (#270).
    #[test]
    fn drops_the_write_only_context_turns_table() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("omni.db");

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

    /// The drop runs even with its migration row present (#379).
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

    /// Both halves of the windowed pair are pinned (#667).
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

    /// Identical content is archived once (#274).
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

    /// Report reads do not book a delivery debt (#593 review).
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

    /// Two readers of one handle each get a delivery (#593 review).
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

    /// No key for a row that was never written (#388).
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

        // A content address only (#274).
        assert_eq!(hash.len(), 16);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));

        let retrieved = store.retrieve_rewind(&hash);
        assert_eq!(retrieved, Some(content.to_string()));

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

    /// A returned key resolves to its content.
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

        // Porter stemming: 'run' matches 'running'.
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

    /// Size-skewed arms are flagged: their delta measures size, not lost signal.
    #[test]
    fn flags_arms_of_wildly_different_size_as_confounded() {
        assert!(row(239, 140, 244_606, 115).is_confounded());
    }

    #[test]
    fn treats_similarly_sized_arms_as_comparable() {
        assert!(!row(103, 255, 3_196, 3_240).is_confounded());
    }

    #[test]
    fn treats_a_skew_at_the_limit_as_comparable() {
        // Exactly 3×: the limit is exclusive.
        assert!(!row(10, 10, 3_000, 1_000).is_confounded());
        assert!(row(10, 10, 3_001, 1_000).is_confounded());
    }

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

    /// Below the sample floor no delta is published.
    #[test]
    fn excludes_a_filter_below_the_minimum_sample_size() {
        let (store, _dir) = get_temp_store();
        seed_rerun_fixture(&store, "npm");
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

    const BEFORE_FIX: i64 = crate::pipeline::POST_HOOK_FIX_TS - 1;
    const AFTER_FIX: i64 = crate::pipeline::POST_HOOK_FIX_TS + 1;

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

    /// The #158 cutoff applies only to Claude Code.
    #[test]
    fn counts_other_agents_savings_regardless_of_when_they_were_recorded() {
        let (store, _dir) = get_temp_store();
        insert_as(&store, "s1", BEFORE_FIX, "git", "git log", "Keep", "aider");

        let (count, ..) = store.aggregate_stats(0).unwrap();

        assert_eq!(count, 1);
    }

    /// Terminal rows are not token savings (#212).
    #[test]
    fn excludes_terminal_rows_from_the_token_headline() {
        let (store, _dir) = get_temp_store();
        insert_as(
            &store, "s1", BEFORE_FIX, "git", "git diff", "Keep", "terminal",
        );

        let (count, ..) = store.aggregate_stats(0).unwrap();

        assert_eq!(count, 0, "TTY bytes are not tokens anyone was billed for");
    }

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

    /// The rows survive; only their byte sums are excluded.
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

    /// Declines group by the agent they were for (#773).
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

    /// `passthrough_events` is pruned like the rest (#672).
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

    /// Declines can be scoped to one session (#672).
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
        // Pipe mode: no id.
        store.record_passthrough("sort -u", 400, "below guardrail", "", "claude_code");

        assert_eq!(
            store.passthrough_reasons_for(0, Some("mine")),
            vec![("structured:json".to_string(), 1)]
        );
        let theirs = store.passthrough_reasons_for(0, Some("theirs"));
        assert_eq!(theirs.len(), 2, "{theirs:?}");
        assert_eq!(theirs.iter().map(|(_, n)| n).sum::<u64>(), 2);

        // An empty id must not return other sessions' declines.
        assert!(
            store.passthrough_reasons_for(0, Some("")).is_empty(),
            "an empty session id answered with the whole table"
        );
        assert_eq!(store.passthrough_reasons_for(0, None).len(), 2);
    }

    /// The label recompute is idempotent: run twice, like the #379 hazard (#605).
    #[test]
    fn the_recompute_fixes_a_stale_label_and_does_not_run_twice() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("omni.db");
        drop(Store::open_path(&db).expect("first open builds the schema"));

        let seed = [
            ("A=$(kubectl", "A=$(kubectl get pods -n web)"),
            ("/bin/ls", "/bin/ls -l /tmp"),
            ("S=/tmp/x", "S=/tmp/x cargo build --release"),
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

        // Second open: recorded, and a fixed point anyway.
        assert_eq!(labels(&db), after, "a second open changed the labels again");
    }

    /// A row without a command is left as it is.
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
