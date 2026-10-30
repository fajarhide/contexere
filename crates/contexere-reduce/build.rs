use std::process::Command;

fn main() {
    // Re-run if .git/HEAD changes
    println!("cargo:rerun-if-changed=../../.git/HEAD");

    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let build_date = Command::new("date")
        .args(["+%Y-%m-%d %H:%M:%S"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    println!("cargo:rustc-env=OMNI_GIT_HASH={}", git_hash);
    println!("cargo:rustc-env=OMNI_BUILD_DATE={}", build_date);

    // #137: how many changelog entries this build carries that no release does.
    //
    // The GitHub-release check in `guard::update` answers "is there a newer
    // release than mine". It cannot answer "are there fixes that have not been
    // released at all", because in that state the newest release *is* the
    // running version — which is how six correctness fixes sat merged and
    // unshipped in #127 while `omni doctor` printed `[LATEST]`.
    //
    // A build-time count is the honest source: `## [Unreleased]` describes the
    // tree this binary was compiled from. A properly cut release has none, so a
    // released binary says nothing.
    println!("cargo:rerun-if-changed=../../CHANGELOG.md");
    println!("cargo:rerun-if-changed=../../changelog.d");
    println!("cargo:rerun-if-changed=src/util/changelog.rs");
    // The sum of both places. New work writes a fragment under `changelog.d/`
    // so parallel branches stop conflicting on `## [Unreleased]`, but entries
    // already written into that section still count until the next tag folds
    // them in. Counting one and not the other would let a tag be cut on work
    // the binary then denies having.
    let unreleased = std::fs::read_to_string("../../CHANGELOG.md")
        .map(|s| count_unreleased_entries(&s))
        .unwrap_or(0)
        + count_fragments(std::path::Path::new("../../changelog.d"));
    println!("cargo:rustc-env=OMNI_UNRELEASED_ENTRIES={}", unreleased);
}

// One implementation, used by the build script and compiled into the library,
// so the number `omni doctor` prints and the number under test cannot diverge.
include!("src/util/changelog.rs");
