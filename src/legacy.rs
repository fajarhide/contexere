//! Settings that outlive the rename. `OMNI_*` keeps working as `VENG_*`, read
//! and never rewritten, so an install set up for omni needs no edit (#876). The
//! old `~/.omni` tree is read in place by `paths`, never copied.

use std::ffi::OsString;

const OLD_ENV_PREFIX: &str = "OMNI_";
const NEW_ENV_PREFIX: &str = "VENG_";

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

/// Whether this process was started under the old name. Through the transition
/// `omni` is the same binary as `veng` (a link the installers lay down), and it
/// keeps omni's MCP tool names so an existing registration sees what it saw (#876).
pub fn invoked_as_omni() -> bool {
    std::env::args_os()
        .next()
        .is_some_and(|a| is_omni_name(std::path::Path::new(&a)))
}

fn is_omni_name(argv0: &std::path::Path) -> bool {
    argv0.file_stem().is_some_and(|s| s == "omni")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_the_old_name_by_file_name_only() {
        assert!(is_omni_name("/opt/homebrew/bin/omni".as_ref()));
        assert!(is_omni_name("omni.exe".as_ref()));
        assert!(!is_omni_name("/home/omni/bin/veng".as_ref()));
        assert!(!is_omni_name("omnibus".as_ref()));
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
}
