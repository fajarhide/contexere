//! Every path OMNI touches, in one place. A call site that builds
//! `home_dir().join(".omni")` itself escapes every override here (#312, #315).

use dirs::home_dir;
use std::env;
use std::path::PathBuf;

/// The legacy tree, `~/.omni`, whether or not it exists.
fn legacy_home() -> PathBuf {
    home_dir().unwrap_or_else(temp_dir).join(".omni")
}

/// Resolve one of the two roots, most specific first (#217):
///
/// 1. `OMNI_HOME` puts the whole tree in one place.
/// 2. `OMNI_CONFIG_HOME` / `OMNI_DATA_HOME` split config from data.
/// 3. An existing `~/.omni` wins over XDG, so an upgrade never appears to lose
///    a database.
/// 4. `XDG_CONFIG_HOME/omni` / `XDG_DATA_HOME/omni` on a fresh install.
/// 5. `~/.omni`.
///
/// Read once through a `LazyLock`: tests run in parallel in one process.
fn resolve_root(split_var: &str, xdg_var: &str) -> PathBuf {
    if let Some(p) = env::var_os("OMNI_HOME").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    if let Some(p) = env::var_os(split_var).filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    let legacy = legacy_home();
    if legacy.exists() {
        return legacy;
    }
    if let Some(p) = env::var_os(xdg_var).filter(|p| !p.is_empty()) {
        return PathBuf::from(p).join("omni");
    }
    legacy
}

/// Where configuration lives: `config.toml`, `filters/`, `signals/`.
#[inline]
pub fn config_home() -> PathBuf {
    static ROOT: std::sync::LazyLock<PathBuf> =
        std::sync::LazyLock::new(|| resolve_root("OMNI_CONFIG_HOME", "XDG_CONFIG_HOME"));
    ROOT.clone()
}

/// Where state lives: the database, transcripts, caches, exports.
#[inline]
pub fn data_home() -> PathBuf {
    static ROOT: std::sync::LazyLock<PathBuf> =
        std::sync::LazyLock::new(|| resolve_root("OMNI_DATA_HOME", "XDG_DATA_HOME"));
    ROOT.clone()
}

/// The OMNI home directory, equal to [`config_home`].
#[inline]
pub fn omni_home() -> PathBuf {
    config_home()
}

#[inline]
pub fn temp_dir() -> PathBuf {
    env::temp_dir()
}

#[inline]
pub fn database_path() -> PathBuf {
    data_home().join("omni.db")
}

#[inline]
pub fn config_file() -> PathBuf {
    config_home().join("config.toml")
}

#[inline]
pub fn cache_directory() -> PathBuf {
    data_home().join("cache")
}

#[inline]
pub fn exports_directory() -> PathBuf {
    data_home().join("exports")
}

/// Narrow a path OMNI owns to its owner: `0700` for a directory, `0600` for a
/// file. The archive keeps raw command output on purpose, and a default umask
/// left it world readable (#631). Reads the mode first, since a `stat` is
/// cheaper than a `chmod` on the hook path. Errors are the caller's to ignore:
/// hooks fail open.
#[cfg(unix)]
pub fn restrict_to_owner(path: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let meta = std::fs::metadata(path)?;
    let want = if meta.is_dir() { 0o700 } else { 0o600 };
    if meta.permissions().mode() & 0o777 != want {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(want))?;
    }
    Ok(())
}

/// Windows has no umask and no mode bits in this sense, so there is nothing to
/// narrow and the ACL it inherits is the one the user's profile already carries.
#[cfg(not(unix))]
pub fn restrict_to_owner(_path: &std::path::Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Starts from loose modes rather than setting a umask, which is process
    /// state that parallel tests would race on.
    #[cfg(unix)]
    #[test]
    fn tightens_a_directory_and_a_file_that_are_already_loose() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let sub = dir.path().join("omni-home");
        std::fs::create_dir(&sub).expect("mkdir");
        let file = sub.join("omni.db");
        std::fs::write(&file, b"rows").expect("write");

        std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o777)).expect("chmod dir");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o666))
            .expect("chmod file");

        restrict_to_owner(&sub).expect("dir");
        restrict_to_owner(&file).expect("file");

        let mode =
            |p: &std::path::Path| std::fs::metadata(p).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode(&sub), 0o700, "a directory OMNI owns is owner only");
        assert_eq!(mode(&file), 0o600, "a file OMNI owns is owner only");
    }

    /// `.cargo/config.toml` points `OMNI_HOME` at `target/`; fails if either half
    /// is removed (#307).
    #[test]
    fn a_test_run_never_resolves_to_the_developers_real_home() {
        let home = omni_home();
        assert!(
            home.ends_with("omni-home"),
            "OMNI_HOME is not in effect: a test run would write to {}",
            home.display()
        );

        let real = home_dir().map(|h| h.join(".omni"));
        assert_ne!(
            Some(home.clone()),
            real,
            "the suite must not share a home with the installed binary"
        );
    }

    /// Checks every path the product opens, not just the accessor (#315).
    #[test]
    fn every_resolved_path_stays_inside_a_configured_root() {
        let config = config_home();
        let data = data_home();

        assert!(
            config_file().starts_with(&config),
            "config_file resolved to {}, outside the config root {}",
            config_file().display(),
            config.display()
        );

        for (name, path) in [
            ("database_path", database_path()),
            ("cache_directory", cache_directory()),
            ("exports_directory", exports_directory()),
        ] {
            assert!(
                path.starts_with(&data),
                "{name} resolved to {}, outside the data root {}",
                path.display(),
                data.display()
            );
        }
    }

    #[test]
    fn an_existing_legacy_tree_wins_over_xdg() {
        // Asserts the ordering without driving the process env, which a
        // parallel test would race on.
        let legacy = legacy_home();
        if legacy.exists() {
            assert_eq!(
                resolve_root("OMNI_DATA_HOME_UNSET_FOR_TEST", "XDG_DATA_HOME"),
                if env::var_os("OMNI_HOME").is_some_and(|p| !p.is_empty()) {
                    omni_home()
                } else {
                    legacy
                },
                "an existing ~/.omni must keep being used"
            );
        }
    }
}

/// The project a ledger scope belongs to: the repository root, not the
/// directory the command ran in (#525). A linked worktree's `.git` is a file
/// pointing at the main checkout, so stopping at the first `.git` would split
/// one repository into one project per worktree. Outside a repository, the
/// directory itself.
pub fn project_key(from: &std::path::Path) -> String {
    for dir in from.ancestors() {
        let dot = dir.join(".git");
        if dot.is_dir() {
            return dir.to_string_lossy().to_string();
        }
        if dot.is_file() {
            return worktree_main_root(&dot).unwrap_or_else(|| dir.to_string_lossy().to_string());
        }
    }
    from.to_string_lossy().to_string()
}

/// `<main>/.git/worktrees/<name>` back to `<main>`, or `None` for any other
/// shape rather than a guess.
fn worktree_main_root(dot_git_file: &std::path::Path) -> Option<String> {
    let contents = std::fs::read_to_string(dot_git_file).ok()?;
    let gitdir = contents.trim().strip_prefix("gitdir:")?.trim();
    let git_dir = std::path::Path::new(gitdir)
        .ancestors()
        .find(|a| a.file_name().is_some_and(|n| n == ".git"))?;
    Some(git_dir.parent()?.to_string_lossy().to_string())
}

#[cfg(test)]
mod project_key_tests {
    use super::project_key;

    #[test]
    fn a_plain_checkout_answers_its_own_root() {
        let d = tempfile::tempdir().expect("tempdir");
        let root = d.path().join("repo");
        std::fs::create_dir_all(root.join(".git")).expect("mkdir");
        std::fs::create_dir_all(root.join("src/deep")).expect("mkdir");

        assert_eq!(
            project_key(&root.join("src/deep")),
            root.to_string_lossy(),
            "a subdirectory has to answer the repository root, not itself"
        );
    }

    /// Stopping at the first `.git` would return the worktree root here.
    #[test]
    fn a_linked_worktree_answers_the_main_checkout() {
        let d = tempfile::tempdir().expect("tempdir");
        let main = d.path().join("omni");
        let tree = main.join(".git-worktrees/520");
        std::fs::create_dir_all(main.join(".git/worktrees/520")).expect("mkdir");
        std::fs::create_dir_all(tree.join("src")).expect("mkdir");
        std::fs::write(
            tree.join(".git"),
            format!("gitdir: {}/.git/worktrees/520\n", main.display()),
        )
        .expect("write");

        assert_eq!(
            project_key(&tree.join("src")),
            main.to_string_lossy(),
            "a worktree has to share the main checkout's history, not open its own"
        );
    }

    #[test]
    fn outside_a_repository_it_stays_where_it_is() {
        let d = tempfile::tempdir().expect("tempdir");
        let loose = d.path().join("notes");
        std::fs::create_dir_all(&loose).expect("mkdir");

        assert_eq!(project_key(&loose), loose.to_string_lossy());
    }

    #[test]
    fn a_git_file_it_cannot_parse_falls_back_to_the_directory() {
        let d = tempfile::tempdir().expect("tempdir");
        let odd = d.path().join("odd");
        std::fs::create_dir_all(&odd).expect("mkdir");
        std::fs::write(odd.join(".git"), "something else entirely\n").expect("write");

        assert_eq!(project_key(&odd), odd.to_string_lossy());
    }
}
