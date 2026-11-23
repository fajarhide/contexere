//! What keeps an install made under the old name working after the rename (#876).

use std::ffi::OsString;
use std::path::Path;

const OLD_ENV_PREFIX: &str = "OMNI_";

fn is_omni_name(argv0: &Path) -> bool {
    argv0.file_stem().is_some_and(|s| s == "omni")
}

/// The one line a person who types `omni` sees, on stderr (#876 step 1), naming the
/// release that removes the name (#1066). Stdout is
/// never touched, and with stderr off a terminal (a hook, a host's MCP client) the
/// line is not printed at all, so nothing that parses our output changes.
pub fn old_name_notice(argv0: &Path, stderr_is_terminal: bool) -> Option<&'static str> {
    (stderr_is_terminal && is_omni_name(argv0)).then_some(
        "omni is now contexere. This name stops working in 0.9.0; `contexere doctor` lists what still uses it.",
    )
}

/// The name this process was started under, for `--version` (#958). Goes when the
/// `omni` link does.
pub fn invoked_name() -> &'static str {
    let argv0 = std::env::args_os().next();
    if argv0.is_some_and(|a| is_omni_name(Path::new(&a))) {
        "omni"
    } else {
        "contexere"
    }
}

/// A tool's name as the server serves it, under either binary name since 0.9.0
/// (#1067). Model-facing text inserts this into the message before any user
/// value, so a file called `omni_build.sh` is never rewritten.
pub fn tool_name(short: &str) -> String {
    format!("contexere_{short}")
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
    // Also inside a wrapper's quoted argument, closing quote and all (#1123).
    let rest = command.trim().trim_end_matches(['\'', '"']);
    let rest = rest.trim_end_matches("--post-hook").trim_end();
    // The last word, so `'omni` found through PATH inside a wrapper counts (#1149 review).
    rest.rsplit(' ').next().is_some_and(file_name_is_omni)
}

/// `OMNI_*` settings, sorted. Since 0.9.0 nothing reads them (#1067), so `doctor`
/// names each one. Matched without case, as Windows reads environment names.
pub fn settings_on_old_name<I>(vars: I) -> Vec<String>
where
    I: IntoIterator<Item = (OsString, OsString)>,
{
    let mut old: Vec<String> = vars
        .into_iter()
        .filter_map(|(k, _)| k.into_string().ok())
        .filter(|k| {
            k.to_ascii_uppercase()
                .strip_prefix(OLD_ENV_PREFIX)
                .is_some_and(|rest| !rest.is_empty())
        })
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
    fn tells_a_person_typing_omni_at_a_terminal() {
        let notice = old_name_notice(Path::new("/opt/homebrew/bin/omni"), true);
        assert!(notice.is_some_and(|n| n.contains("contexere doctor")));
        // #1066: the notice names the release that removes the old name.
        assert!(notice.is_some_and(|n| n.contains("stops working in 0.9.0")));
    }

    #[test]
    fn says_nothing_when_stderr_is_not_a_terminal() {
        assert_eq!(
            old_name_notice(Path::new("/opt/homebrew/bin/omni"), false),
            None
        );
    }

    #[test]
    fn says_nothing_under_the_new_name() {
        assert_eq!(
            old_name_notice(Path::new("/opt/homebrew/bin/contexere"), true),
            None
        );
    }

    #[test]
    fn a_command_is_omni_by_its_whole_path() {
        assert!(file_name_is_omni("/Users/Jane Doe/bin/omni"));
        assert!(file_name_is_omni("\"C:\\Users\\x y\\omni.exe\""));
        assert!(!file_name_is_omni("/Users/omni/bin/contexere"));
    }

    #[test]
    fn finds_a_hook_on_the_old_name_by_its_binary() {
        assert!(hook_calls_omni("/opt/homebrew/bin/omni --post-hook"));
        assert!(hook_calls_omni("\"/Users/Jane Doe/bin/omni\" --post-hook"));
        assert!(!hook_calls_omni("/opt/homebrew/bin/contexere --post-hook"));
        assert!(!hook_calls_omni("/Users/omni/bin/contexere --post-hook"));
    }

    /// A wrapped `omni` found through PATH (#1149 review).
    #[test]
    fn finds_a_wrapped_bare_omni() {
        assert!(hook_calls_omni("python3 f.py --after 'omni --post-hook'"));
    }

    /// Inside a wrapper's quoted argument too (#1123).
    #[test]
    fn finds_a_wrapped_hook_on_the_old_name() {
        assert!(hook_calls_omni(
            "python3 f.py --after '/opt/homebrew/bin/omni --post-hook'"
        ));
        assert!(!hook_calls_omni(
            "python3 f.py --after '/opt/homebrew/bin/contexere --post-hook'"
        ));
    }

    /// #1067: a `CONTEXERE_*` twin no longer covers an old setting, since the old
    /// one is ignored either way.
    #[test]
    fn lists_every_setting_on_the_old_name() {
        let vars = vec![
            (OsString::from("OMNI_QUIET"), OsString::from("1")),
            (OsString::from("OMNI_AGENT_ID"), OsString::from("cursor")),
            (
                OsString::from("CONTEXERE_AGENT_ID"),
                OsString::from("cursor"),
            ),
            (OsString::from("OMNIX"), OsString::from("no")),
            (OsString::from("OMNI_"), OsString::from("no")),
            // Windows reads environment names without case.
            (OsString::from("omni_passthrough"), OsString::from("1")),
        ];
        assert_eq!(
            settings_on_old_name(vars),
            vec![
                "OMNI_AGENT_ID".to_string(),
                "OMNI_QUIET".to_string(),
                "omni_passthrough".to_string()
            ]
        );
    }

    #[test]
    fn our_program_is_either_name_and_nothing_longer() {
        assert!(is_our_program("omni") && is_our_program("contexere.exe"));
        assert!(!is_our_program("omnictl") && !is_our_program("omnibus"));
    }
}
