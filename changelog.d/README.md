# Changelog fragments

One file per entry. **Write your changelog entry here, not in `CHANGELOG.md`.**

```
changelog.d/<issue-or-slug>.<section>.md
```

`<section>` is one of `added`, `changed`, `fixed`, `removed`. So a fix for #544
is `changelog.d/544.fixed.md`, and a branch closing two issues writes two files.
Use the issue number when there is one. Work that shipped without a ticket takes
a short slug instead, never a number that belongs to something else.

**Anything here that is not `README.md` and is not hidden counts as an entry**,
whatever it is named. Get the section wrong and `scripts/changelog_cut.sh` refuses
to cut a release until you rename it. That is deliberate: the rule is one sentence
so the shell script and `count_fragments` in `crates/contexere-reduce/src/util/changelog.rs` cannot drift
apart, and an entry cannot go missing by being spelled unusually. Do not leave
scratch files in this directory.

The file holds the bullet and nothing else. No `##` heading, no `### Fixed`
heading, no blank line at the top. `scripts/changelog_cut.sh` supplies the
headings at release time and groups every fragment under the right one.

```markdown
- **`omni_run` could hang until the host gave up (#544)**: stdout was drained to
  EOF and only then stderr, so a child that fills the stderr pipe buffer before
  closing stdout blocks forever. `wait_with_output` drains both.
```

## Why the directory exists

Because two branches editing `## [Unreleased]` in `CHANGELOG.md` always
conflict, and this repo's own history is the evidence: five branches opened in
one sitting on 2026-08-02 produced four identical resolutions and four full CI
re-runs. Parallel agent sessions made it worse, because they cannot batch
themselves into one branch the way one person can.

Two branches never write the same path here, so there is nothing for git to
conflict on. That is the whole trick.

## Depth is unchanged

The entries in `CHANGELOG.md` are unusually detailed on purpose: each one states
the measured evidence, the wrong number that was published, and the mechanism. A
fragment is held to exactly that bar. A one-line entry is a regression whichever
file it lives in.

## What counts them

`build.rs` adds the fragments here to the bullets still under `## [Unreleased]`,
and `omni doctor` prints `[N UNRELEASED] … cut a tag` from the total. A tree with
uncut work says so; a properly cut release says nothing. Leaving your entry out
of both places makes the released binary lie about itself (#137).
