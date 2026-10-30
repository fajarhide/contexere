//! `omni retrieve <handle>`, the recovery path every marker names. A marker
//! naming an MCP call some hosts cannot make is a handle that does not
//! resolve (#452, #388).

use crate::store::sqlite::Store;
use anyhow::Result;
use colored::*;

const FLAGS: super::Flags = &[];

fn print_help() {
    println!(
        "\n{} {}: Print the content a marker archived",
        "omni".bold().cyan(),
        "retrieve".bold().yellow()
    );
    println!("\n{}", "USAGE:".bold().bright_white());
    println!("  omni {} {}", "retrieve".cyan(), "<handle>".bright_black());
    println!(
        "\nA handle is the 16 characters inside a marker, for example\n  {}\n",
        format!(
            "[OMNI: 50 lines already shown, omni retrieve {}]",
            crate::util::text::EXAMPLE_HANDLE
        )
        .bright_black()
    );
}

/// The stderr frame. Adds the whole-output size when the handle holds one block
/// of it, or a reader takes the block as complete (#627).
fn frame(handle: &str, content: &str, whole_len: usize) -> String {
    let part = if whole_len > content.len() {
        format!(
            " · one block of a {} output",
            super::stats::format_bytes(whole_len as u64)
        )
    } else {
        String::new()
    };
    format!(
        "{} {} · {} lines · {}{}",
        "omni retrieve".bold().cyan(),
        handle.bright_black(),
        content.lines().count(),
        super::stats::format_bytes(content.len() as u64).bright_black(),
        part.bright_black()
    )
}

pub fn run(args: &[String], store: &Store) -> Result<()> {
    if super::has_flag(args, "--help") || super::has_flag(args, "-h") {
        print_help();
        return Ok(());
    }
    super::check_flags("retrieve", args, FLAGS)?;

    // The first non-flag token after the subcommand.
    let handle = args
        .iter()
        .skip_while(|a| *a != "retrieve")
        .skip(1)
        .find(|a| !a.starts_with('-'));

    let Some(handle) = handle else {
        print_help();
        anyhow::bail!("no handle given");
    };

    // Bare, quoted, or still wrapped in the old MCP call.
    let handle = handle
        .trim_start_matches("omni_retrieve(")
        .trim_end_matches(')')
        .trim_matches('"');

    // Before the lookup: the generic miss blames pruning, wrong for an example (#583).
    if handle == crate::util::text::EXAMPLE_HANDLE {
        anyhow::bail!(
            "`{handle}` is the documentation example, not a real handle. Copy the 16 characters from a marker in your own output instead"
        );
    }

    match store.retrieve_rewind_sized(handle) {
        Some((content, whole_len)) => {
            // Counts like the MCP tool (#512).
            store.record_rewind_pull(handle);
            // Frame to stderr, payload to stdout byte for byte: callers paste or parse it (#463).
            eprintln!("{}", frame(handle, &content, whole_len));
            print!("{content}");
            if !content.ends_with('\n') {
                println!();
            }
            Ok(())
        }
        // Name the retention window, or "not found" reads as an archive bug.
        None => anyhow::bail!(
            "no archived content for `{handle}`. Handles live as long as the working tier, 30 days; older ones are pruned"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn says_when_a_handle_holds_one_block_of_a_larger_output() {
        let block = "one\ntwo\nthree\n";

        let whole = frame("abcd", block, block.len());
        assert!(
            !whole.contains("one block of"),
            "a handle holding everything keeps the short frame: {whole}"
        );

        let part = frame("abcd", block, block.len() * 4);
        assert!(
            part.contains("one block of"),
            "a fragment must not be framed as the whole output: {part}"
        );
    }

    #[test]
    fn prints_what_the_marker_archived() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open_path(&dir.path().join("omni.db")).expect("store");
        let handle = store
            .store_rewind_whole("the original forty lines\n")
            .expect("archived");

        assert!(run(&args(&["omni", "retrieve", &handle]), &store).is_ok());
    }

    /// The reserved example handle is refused for the right reason (#583).
    #[test]
    fn the_documentation_example_is_refused_and_not_blamed_on_pruning() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open_path(&dir.path().join("omni.db")).expect("store");

        let err = run(
            &args(&["omni", "retrieve", crate::util::text::EXAMPLE_HANDLE]),
            &store,
        )
        .expect_err("the documentation example must not resolve");

        let msg = err.to_string();
        assert!(
            msg.contains("documentation example"),
            "the reader has to be told it was an example: {msg}"
        );
        assert!(
            !msg.contains("pruned"),
            "an example was never archived, so pruning cannot be the reason: {msg}"
        );
    }

    /// A CLI pull is counted, like an MCP one (#512).
    #[test]
    fn counts_a_pull_the_adaptive_rate_can_read() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("omni.db");
        let store = Store::open_path(&db).expect("store");
        let handle = store
            .store_rewind_whole("archived output\n")
            .expect("archived");

        // No distillation names this hash, so it lands under `unknown`.
        let events = |store: &Store| store.count_memory_reads("unknown");
        assert_eq!(events(&store), 0, "nothing has been pulled yet");

        run(&args(&["omni", "retrieve", &handle]), &store).expect("retrieved");

        assert_eq!(
            events(&store),
            1,
            "the CLI pull left no trace for the adaptive rate to read"
        );
    }

    /// Every shape a marker has printed must work.
    #[test]
    fn accepts_the_handle_in_every_shape_a_marker_has_printed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open_path(&dir.path().join("omni.db")).expect("store");
        let handle = store.store_rewind_whole("content\n").expect("archived");

        for form in [
            handle.clone(),
            format!("\"{handle}\""),
            format!("omni_retrieve(\"{handle}\")"),
        ] {
            assert!(
                run(&args(&["omni", "retrieve", &form]), &store).is_ok(),
                "form {form} was not understood"
            );
        }
    }

    /// Not all-zeros: that is now the reserved example handle (#583).
    #[test]
    fn says_why_a_missing_handle_is_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open_path(&dir.path().join("omni.db")).expect("store");

        let err = run(&args(&["omni", "retrieve", "ffffffffffffffff"]), &store)
            .expect_err("an unknown handle is an error");

        assert!(err.to_string().contains("30 days"), "{err}");
    }
}
