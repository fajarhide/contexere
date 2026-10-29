//! What keeps an install made under the old name working after the rename (#876).

use std::ffi::OsString;
use std::path::Path;

const NEW_ENV_PREFIX: &str = "CONTEXERE_";
const OLD_ENV_PREFIX: &str = "OMNI_";

/// `CONTEXERE_*` settings to write over their `OMNI_*` twin.
///
/// The code still reads `OMNI_*`, so the new name is folded onto the old one
/// and wins when both are set. Pure so it can be tested without touching the
/// process environment, which parallel tests share.
pub fn env_to_adopt<I>(vars: I) -> Vec<(String, OsString)>
where
    I: IntoIterator<Item = (OsString, OsString)>,
{
    vars.into_iter()
        .filter_map(|(key, value)| {
            let rest = key.to_str()?.strip_prefix(NEW_ENV_PREFIX)?;
            Some((format!("{OLD_ENV_PREFIX}{rest}"), value))
        })
        .collect()
}

/// Call first thing in `main`, before any thread exists: changing the
/// environment is only sound while the process is single threaded.
pub fn adopt_env() {
    for (key, value) in env_to_adopt(std::env::vars_os()) {
        // SAFETY: `main` calls this before it spawns anything.
        unsafe { std::env::set_var(key, value) };
    }
}

/// Whether this process was started under the old name. `omni` is a link to the
/// same binary, and keeps serving `omni_*` tools so a registered host sees what
/// it saw before.
pub fn invoked_as_omni() -> bool {
    std::env::args_os()
        .next()
        .is_some_and(|a| is_omni_name(Path::new(&a)))
}

fn is_omni_name(argv0: &Path) -> bool {
    argv0.file_stem().is_some_and(|s| s == "omni")
}

/// The MCP tool prefix and server key of the name this process runs under.
pub fn mcp_name() -> &'static str {
    if invoked_as_omni() {
        "omni"
    } else {
        "contexere"
    }
}

/// A program name that is this binary under either of its names.
pub fn is_our_program(name: &str) -> bool {
    matches!(name.trim_end_matches(".exe"), "omni" | "contexere")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_the_old_name_by_file_name_only() {
        assert!(is_omni_name("/opt/homebrew/bin/omni".as_ref()));
        assert!(is_omni_name("omni.exe".as_ref()));
        assert!(!is_omni_name("/home/omni/bin/contexere".as_ref()));
        assert!(!is_omni_name("omnibus".as_ref()));
    }

    #[test]
    fn folds_only_the_exact_new_prefix_onto_the_old_name() {
        let vars = vec![
            (
                OsString::from("CONTEXERE_AGENT_ID"),
                OsString::from("cursor"),
            ),
            (OsString::from("OMNI_QUIET"), OsString::from("1")),
            (OsString::from("CONTEXEREX_Y"), OsString::from("no")),
        ];
        assert_eq!(
            env_to_adopt(vars),
            vec![("OMNI_AGENT_ID".to_string(), OsString::from("cursor"))]
        );
    }

    #[test]
    fn our_program_is_either_name_and_nothing_longer() {
        assert!(is_our_program("omni") && is_our_program("contexere.exe"));
        assert!(!is_our_program("omnictl") && !is_our_program("omnibus"));
    }
}
