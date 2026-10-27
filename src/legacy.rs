//! Everything that exists only because veng replaced omni.
//!
//! One rule governs this module: **nothing of the user's is deleted or
//! overwritten.** The old `~/.omni` tree is only ever read. If a veng file is
//! already there, it wins and the old one is left alone, so running a migration
//! twice, or after using veng for a week, cannot lose a row.
//!
//! None of this runs on the hook path. The hook budget is 10 ms and a migration
//! copies a database, so it is an explicit command (`veng migrate`), never
//! something a hook does behind the user's back. The one exception is
//! [`adopt_legacy_env`], which is a loop over the environment at startup.

use anyhow::{Context, Result};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

const OLD_ENV_PREFIX: &str = "OMNI_";
const NEW_ENV_PREFIX: &str = "VENG_";

/// The database file omni wrote.
pub const LEGACY_DB: &str = "omni.db";
/// The database file veng writes.
pub const DB: &str = "veng.db";

/// Config entries that move with the database. Caches, exports and transcripts
/// are rebuilt or short lived and are deliberately not copied.
const CONFIG_FILES: &[&str] = &["config.toml"];
const CONFIG_DIRS: &[&str] = &["filters", "signals"];

/// Which `OMNI_*` variables should be copied to their `VENG_*` name.
///
/// Pure so it can be tested without touching the process environment, which
/// `cargo test` shares between parallel tests. A variable is adopted only when
/// its `VENG_` twin is unset: a user who already moved some settings keeps them.
pub fn env_to_adopt<I>(vars: I, is_set: impl Fn(&str) -> bool) -> Vec<(String, OsString)>
where
    I: IntoIterator<Item = (OsString, OsString)>,
{
    vars.into_iter()
        .filter_map(|(key, value)| {
            let key = key.to_str()?;
            let rest = key.strip_prefix(OLD_ENV_PREFIX)?;
            let new_key = format!("{NEW_ENV_PREFIX}{rest}");
            (!is_set(&new_key)).then_some((new_key, value))
        })
        .collect()
}

/// Let `OMNI_*` settings keep working as `VENG_*`.
///
/// Call first thing in `main`, before any thread exists: changing the
/// environment is only sound while the process is single threaded.
pub fn adopt_legacy_env() {
    for (key, value) in env_to_adopt(std::env::vars_os(), |k| std::env::var_os(k).is_some()) {
        // SAFETY: `main` calls this before it spawns anything.
        unsafe { std::env::set_var(key, value) };
    }
}

/// Where omni kept its tree: `OMNI_HOME` if the user set one, else `~/.omni`.
pub fn legacy_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("OMNI_HOME").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    dirs::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".omni")
}

/// What happened to the database.
#[derive(Debug, PartialEq, Eq)]
pub enum Database {
    /// Copied. `bytes` is the size of the new file.
    Copied { bytes: u64 },
    /// Dry run: would be copied. `bytes` is the old main file, a lower bound
    /// because it does not count what is still in the write-ahead log.
    WouldCopy { bytes: u64 },
    /// There is no omni database to bring over.
    NoLegacyDatabase,
    /// A veng database already exists, and is never overwritten.
    AlreadyPresent,
}

#[derive(Debug)]
pub struct DataReport {
    pub database: Database,
    /// Config files and directories brought over (or, on a dry run, that would be).
    pub config: Vec<String>,
}

/// Bring omni's data across without touching the original.
///
/// `source` is the old tree, `data` and `config` are where veng keeps its state
/// and settings (usually the same directory).
pub fn migrate_data(
    source: &Path,
    data: &Path,
    config: &Path,
    dry_run: bool,
) -> Result<DataReport> {
    let src_db = source.join(LEGACY_DB);
    let dst_db = data.join(DB);

    let database = if !src_db.is_file() {
        Database::NoLegacyDatabase
    } else if dst_db.exists() {
        Database::AlreadyPresent
    } else if dry_run {
        Database::WouldCopy {
            bytes: std::fs::metadata(&src_db).map(|m| m.len()).unwrap_or(0),
        }
    } else {
        std::fs::create_dir_all(data)
            .with_context(|| format!("cannot create {}", data.display()))?;
        if let Err(e) = vacuum_into(&src_db, &dst_db) {
            // Never leave half a database where the next run would call it done.
            let _ = std::fs::remove_file(&dst_db);
            return Err(e);
        }
        let _ = crate::paths::restrict_to_owner(data);
        let _ = crate::paths::restrict_to_owner(&dst_db);
        Database::Copied {
            bytes: std::fs::metadata(&dst_db).map(|m| m.len()).unwrap_or(0),
        }
    };

    let mut moved = Vec::new();
    for name in CONFIG_FILES {
        let from = source.join(name);
        let to = config.join(name);
        if from.is_file() && !to.exists() {
            if !dry_run {
                std::fs::create_dir_all(config)?;
                std::fs::copy(&from, &to)
                    .with_context(|| format!("cannot copy {}", from.display()))?;
            }
            moved.push((*name).to_string());
        }
    }
    for name in CONFIG_DIRS {
        let from = source.join(name);
        let to = config.join(name);
        if from.is_dir() && !to.exists() {
            if !dry_run {
                copy_dir(&from, &to)?;
            }
            moved.push(format!("{name}/"));
        }
    }

    Ok(DataReport {
        database,
        config: moved,
    })
}

/// A consistent copy of a live SQLite database, including whatever is still in
/// its write-ahead log. A plain file copy of `omni.db` while omni is running can
/// miss recent rows or copy a torn page; `VACUUM INTO` cannot.
fn vacuum_into(src: &Path, dst: &Path) -> Result<()> {
    let conn =
        rusqlite::Connection::open_with_flags(src, rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE)
            .with_context(|| format!("cannot open {}", src.display()))?;
    conn.execute("VACUUM INTO ?1", [dst.to_string_lossy().as_ref()])
        .with_context(|| format!("cannot copy {} to {}", src.display(), dst.display()))?;
    Ok(())
}

/// Recursive copy that never follows a symlink out of the tree.
fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed_legacy(dir: &Path) {
        let conn = rusqlite::Connection::open(dir.join(LEGACY_DB)).expect("open");
        conn.execute_batch(
            "CREATE TABLE notes (id INTEGER PRIMARY KEY, body TEXT);
             INSERT INTO notes (body) VALUES ('kept across the rename');",
        )
        .expect("seed");
    }

    fn read_note(db: &Path) -> String {
        let conn = rusqlite::Connection::open(db).expect("open copy");
        conn.query_row("SELECT body FROM notes", [], |r| r.get(0))
            .expect("row")
    }

    #[test]
    fn adopts_only_variables_whose_new_name_is_unset() {
        let vars = vec![
            (
                OsString::from("OMNI_AGENT_ID"),
                OsString::from("claude_code"),
            ),
            (OsString::from("OMNI_QUIET"), OsString::from("1")),
            (OsString::from("PATH"), OsString::from("/bin")),
            (OsString::from("OMNIBUS_X"), OsString::from("no")),
        ];
        let adopted = env_to_adopt(vars, |k| k == "VENG_QUIET");

        assert_eq!(
            adopted,
            vec![("VENG_AGENT_ID".to_string(), OsString::from("claude_code"))],
            "VENG_QUIET is already set so it must win, and only the exact OMNI_ prefix counts"
        );
    }

    #[test]
    fn copies_a_database_and_leaves_the_original_untouched() {
        let src = tempfile::tempdir().expect("src");
        let dst = tempfile::tempdir().expect("dst");
        seed_legacy(src.path());
        let before = std::fs::read(src.path().join(LEGACY_DB)).expect("read");

        let report = migrate_data(src.path(), dst.path(), dst.path(), false).expect("migrate");

        assert!(
            matches!(report.database, Database::Copied { .. }),
            "{report:?}"
        );
        assert_eq!(read_note(&dst.path().join(DB)), "kept across the rename");
        assert_eq!(
            std::fs::read(src.path().join(LEGACY_DB)).expect("read again"),
            before,
            "the old database must be byte for byte what it was"
        );
    }

    #[test]
    fn a_dry_run_writes_nothing() {
        let src = tempfile::tempdir().expect("src");
        let dst = tempfile::tempdir().expect("dst");
        seed_legacy(src.path());
        std::fs::write(src.path().join("config.toml"), "x = 1").expect("config");

        let report = migrate_data(src.path(), dst.path(), dst.path(), true).expect("dry run");

        assert!(
            matches!(report.database, Database::WouldCopy { .. }),
            "{report:?}"
        );
        assert_eq!(report.config, vec!["config.toml".to_string()]);
        assert!(
            !dst.path().join(DB).exists(),
            "a dry run created the database"
        );
        assert!(
            !dst.path().join("config.toml").exists(),
            "a dry run copied config"
        );
    }

    #[test]
    fn never_overwrites_a_veng_database_that_already_exists() {
        let src = tempfile::tempdir().expect("src");
        let dst = tempfile::tempdir().expect("dst");
        seed_legacy(src.path());
        std::fs::write(dst.path().join(DB), b"mine").expect("existing");

        let report = migrate_data(src.path(), dst.path(), dst.path(), false).expect("migrate");

        assert_eq!(report.database, Database::AlreadyPresent);
        assert_eq!(std::fs::read(dst.path().join(DB)).expect("read"), b"mine");
    }

    #[test]
    fn says_so_when_there_is_nothing_to_bring_over() {
        let src = tempfile::tempdir().expect("src");
        let dst = tempfile::tempdir().expect("dst");

        let report = migrate_data(src.path(), dst.path(), dst.path(), false).expect("migrate");

        assert_eq!(report.database, Database::NoLegacyDatabase);
        assert!(report.config.is_empty());
    }

    #[test]
    fn brings_filters_across_but_keeps_ones_veng_already_has() {
        let src = tempfile::tempdir().expect("src");
        let dst = tempfile::tempdir().expect("dst");
        std::fs::create_dir_all(src.path().join("filters")).expect("mkdir");
        std::fs::write(src.path().join("filters/a.toml"), "old").expect("filter");
        std::fs::create_dir_all(src.path().join("signals")).expect("mkdir");
        std::fs::create_dir_all(dst.path().join("signals")).expect("mkdir");
        std::fs::write(dst.path().join("signals/keep.toml"), "mine").expect("signal");

        let report = migrate_data(src.path(), dst.path(), dst.path(), false).expect("migrate");

        assert_eq!(report.config, vec!["filters/".to_string()]);
        assert_eq!(
            std::fs::read_to_string(dst.path().join("filters/a.toml")).expect("copied"),
            "old"
        );
        assert!(dst.path().join("signals/keep.toml").exists());
    }
}
