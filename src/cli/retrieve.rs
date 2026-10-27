//! `veng retrieve <handle>`, the recovery path every marker names.
//!
//! Filed as #452 from a real session: the markers promised `veng_retrieve("…")`,
//! which is an MCP tool, and on a host where that tool is not surfaced the agent
//! was left with a hole in the file it had just read and no way to fill it. The
//! MCP tool works where MCP works; this exists so the promise does not depend on
//! that.
//!
//! It is also the reason the marker text changed. A marker naming a tool call
//! that only some hosts can make is a handle that does not resolve, and #388
//! settled that a handle which does not resolve is the one defect this mechanism
//! cannot have.

use crate::store::sqlite::Store;
use anyhow::Result;
use colored::*;

const FLAGS: super::Flags = &[];

fn print_help() {
    println!(
        "\n{} {}: Print the content a marker archived",
        "veng".bold().cyan(),
        "retrieve".bold().yellow()
    );
    println!("\n{}", "USAGE:".bold().bright_white());
    println!("  veng {} {}", "retrieve".cyan(), "<handle>".bright_black());
    println!(
        "\nA handle is the 16 characters inside a marker, for example\n  {}\n",
        format!(
            "[VENG: 50 lines already shown, veng retrieve {}]",
            crate::util::text::EXAMPLE_HANDLE
        )
        .bright_black()
    );
}

/// The stderr frame, built where a test can read it.
///
/// `N lines · N B` reads as a measurement of the whole command's output, and for
/// a ledger fold it measures one block of it: a reader took ten of fourteen
/// entries as a complete listing and nearly designed a dependency layering
/// against it (#627). The extra clause is said only when the two differ, so a
/// handle that does hold an entire output keeps the short frame.
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
        "veng retrieve".bold().cyan(),
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

    // `args` is the whole argv, so the handle is the first token after the
    // subcommand that is not a flag.
    let handle = args
        .iter()
        .skip_while(|a| *a != "retrieve")
        .skip(1)
        .find(|a| !a.starts_with('-'));

    let Some(handle) = handle else {
        print_help();
        anyhow::bail!("no handle given");
    };

    // Accept what a marker prints and what a model might paste back: bare, in
    // quotes, or still wrapped in the MCP call it used to name.
    let handle = handle
        .trim_start_matches("veng_retrieve(")
        .trim_end_matches(')')
        .trim_matches('"');

    // Said before the lookup, because the generic miss below blames pruning and
    // that would be a cause this code cannot know. A reader who pasted the
    // manual's example deserves to be told it was an example (#583).
    if handle == crate::util::text::EXAMPLE_HANDLE {
        anyhow::bail!(
            "`{handle}` is the documentation example, not a real handle. Copy the 16 characters from a marker in your own output instead"
        );
    }

    match store.retrieve_rewind_sized(handle) {
        Some((content, whole_len)) => {
            // The same door the marker tells the agent to use, so it counts the
            // same as the MCP tool (#512).
            store.record_rewind_pull(handle);
            // The frame goes to stderr and the payload to stdout, byte for byte.
            // #463 asked for this surface to be framed like the others, and a
            // header on stdout would have done it by editing archived bytes that
            // a caller is about to paste or parse. Same rule the pipeline holds
            // itself to: what a later step reads is not ours to decorate.
            eprintln!("{}", frame(handle, &content, whole_len));
            print!("{content}");
            if !content.ends_with('\n') {
                println!();
            }
            Ok(())
        }
        // Naming the retention window matters more than the failure does: a
        // handle older than the working tier is gone on purpose, and "not found"
        // alone reads as a bug in the archive.
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

    /// #627. `veng retrieve` returned ten of the fourteen lines a command
    /// produced, under `10 lines · 227 B`. Nothing was lost, the other four were
    /// on screen and the handle holds what the fold replaced, but the frame
    /// reads as a measurement of the whole and the reader took the short listing
    /// as complete.
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
        let store = Store::open_path(&dir.path().join("veng.db")).expect("store");
        let handle = store
            .store_rewind_whole("the original forty lines\n")
            .expect("archived");

        assert!(run(&args(&["veng", "retrieve", &handle]), &store).is_ok());
    }

    /// #583. Four of the seven example handles in our own source and manual
    /// still resolved on the maintainer's machine, so a checker asking "does
    /// this handle resolve" counted our documentation as evidence of folding.
    /// The reserved one has to be refused, and refused for the right reason:
    /// the generic miss blames the 30 day prune, which for an example is a
    /// cause this code cannot know.
    #[test]
    fn the_documentation_example_is_refused_and_not_blamed_on_pruning() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open_path(&dir.path().join("veng.db")).expect("store");

        let err = run(
            &args(&["veng", "retrieve", crate::util::text::EXAMPLE_HANDLE]),
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

    /// #512. `get_retrieve_rate` backs off the route thresholds for a command
    /// family whose full output keeps being needed, and it reads
    /// `retrieve_events`. Only the MCP tool wrote there, while the marker sends
    /// the agent to this subcommand, so the counter saw a minority of pulls.
    ///
    /// Asserted at this level and not on `record_rewind_pull` directly: the
    /// defect was never in the recording, it was a door that walked past it.
    #[test]
    fn counts_a_pull_the_adaptive_rate_can_read() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("veng.db");
        let store = Store::open_path(&db).expect("store");
        let handle = store
            .store_rewind_whole("archived output\n")
            .expect("archived");

        // No distillation row names this hash, so the family resolves to
        // `unknown`, which is the key the row lands under.
        let events = |store: &Store| store.count_memory_reads("unknown");
        assert_eq!(events(&store), 0, "nothing has been pulled yet");

        run(&args(&["veng", "retrieve", &handle]), &store).expect("retrieved");

        assert_eq!(
            events(&store),
            1,
            "the CLI pull left no trace for the adaptive rate to read"
        );
    }

    /// A model handing back the marker verbatim is the likeliest way this is
    /// called, so the shapes a marker has ever printed all have to work.
    #[test]
    fn accepts_the_handle_in_every_shape_a_marker_has_printed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open_path(&dir.path().join("veng.db")).expect("store");
        let handle = store.store_rewind_whole("content\n").expect("archived");

        for form in [
            handle.clone(),
            format!("\"{handle}\""),
            format!("veng_retrieve(\"{handle}\")"),
        ] {
            assert!(
                run(&args(&["veng", "retrieve", &form]), &store).is_ok(),
                "form {form} was not understood"
            );
        }
    }

    /// The fixture moved off all-zeros in #583. That value is now the reserved
    /// documentation handle and is refused earlier with a different reason, so
    /// leaving it here would have quietly stopped testing the prune message.
    #[test]
    fn says_why_a_missing_handle_is_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = Store::open_path(&dir.path().join("veng.db")).expect("store");

        let err = run(&args(&["veng", "retrieve", "ffffffffffffffff"]), &store)
            .expect_err("an unknown handle is an error");

        assert!(err.to_string().contains("30 days"), "{err}");
    }
}
