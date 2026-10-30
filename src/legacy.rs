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

/// A tool's name as this process serves it: `omni` keeps `omni_run`,
/// `contexere` says `contexere_run`. Model-facing text inserts this into the
/// message before any user value, so a file called `omni_build.sh` is never
/// rewritten.
pub fn tool_name(short: &str) -> String {
    format!("{}_{short}", mcp_name())
}

/// The tool prefix a registered server command serves, by its file name: the
/// server names its tools after the name it was started under, which is the
/// name init registered, not the one init itself ran as.
pub fn prefix_of_command(command: &str) -> &'static str {
    if file_name_is_omni(command) {
        "omni"
    } else {
        "contexere"
    }
}

/// Whether a whole command (a path, spaces allowed, quotes optional) names the omni binary.
pub fn file_name_is_omni(command: &str) -> bool {
    let path = command.trim().trim_matches(|c| c == '"' || c == '\'');
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.trim_end_matches(".exe") == "omni"
}

/// Whether a registered post-hook still calls the binary by its old name, so
/// `doctor` can point at `contexere init` (#876 step 5).
pub fn hook_calls_omni(command: &str) -> bool {
    file_name_is_omni(command.trim().trim_end_matches("--post-hook"))
}

/// `OMNI_*` settings with no `CONTEXERE_*` twin, sorted. A twin means the user
/// already moved it; `adopt_env` folded it onto the old name at startup.
pub fn settings_on_old_name<I>(vars: I) -> Vec<String>
where
    I: IntoIterator<Item = (OsString, OsString)>,
{
    let keys: Vec<String> = vars
        .into_iter()
        .filter_map(|(k, _)| k.into_string().ok())
        .collect();
    // Names compare without case, as every setting reader here does (Windows).
    let upper: Vec<String> = keys.iter().map(|k| k.to_ascii_uppercase()).collect();
    let mut old: Vec<String> = keys
        .iter()
        .zip(&upper)
        .filter(|(_, u)| {
            u.strip_prefix(OLD_ENV_PREFIX).is_some_and(|rest| {
                !rest.is_empty() && !upper.contains(&format!("{NEW_ENV_PREFIX}{rest}"))
            })
        })
        .map(|(k, _)| k.clone())
        .collect();
    old.sort();
    old
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
    fn a_command_is_omni_by_its_whole_path() {
        assert!(file_name_is_omni("/Users/Jane Doe/bin/omni"));
        assert!(file_name_is_omni("\"C:\\Users\\x y\\omni.exe\""));
        assert!(!file_name_is_omni("/Users/omni/bin/contexere"));
        assert_eq!(prefix_of_command("/x y/omni"), "omni");
        assert_eq!(prefix_of_command("/x/contexere"), "contexere");
    }

    #[test]
    fn finds_a_hook_on_the_old_name_by_its_binary() {
        assert!(hook_calls_omni("/opt/homebrew/bin/omni --post-hook"));
        assert!(hook_calls_omni("\"/Users/Jane Doe/bin/omni\" --post-hook"));
        assert!(!hook_calls_omni("/opt/homebrew/bin/contexere --post-hook"));
        assert!(!hook_calls_omni("/Users/omni/bin/contexere --post-hook"));
    }

    #[test]
    fn lists_a_setting_on_the_old_name_only_without_its_twin() {
        let vars = vec![
            (OsString::from("OMNI_QUIET"), OsString::from("1")),
            (OsString::from("OMNI_AGENT_ID"), OsString::from("cursor")),
            (
                OsString::from("CONTEXERE_AGENT_ID"),
                OsString::from("cursor"),
            ),
            (OsString::from("OMNIX"), OsString::from("no")),
            // Windows reads environment names without case, and so does the code.
            (OsString::from("omni_passthrough"), OsString::from("1")),
            (OsString::from("omni_db_path"), OsString::from("/x")),
            (OsString::from("Contexere_Db_Path"), OsString::from("/x")),
        ];
        assert_eq!(
            settings_on_old_name(vars),
            vec!["OMNI_QUIET".to_string(), "omni_passthrough".to_string()]
        );
    }

    #[test]
    fn our_program_is_either_name_and_nothing_longer() {
        assert!(is_our_program("omni") && is_our_program("contexere.exe"));
        assert!(!is_our_program("omnictl") && !is_our_program("omnibus"));
    }
}
