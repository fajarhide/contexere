# Releasing

```sh
make ci                          # fmt + clippy + test + security + binary-check
make bump VERSION=x.y.z
make release VERSION=x.y.z
make release-sha VERSION=x.y.z   # after the tag has actually built
```

## The order, and why it is not negotiable

**Cut the changelog first, then bump.**

`bump_version.sh` does not touch `CHANGELOG.md`, and `build.rs` counts what is still
uncut in the tree it compiles: the bullets under `## [Unreleased]` plus the fragments in
`changelog.d/`. Tag without folding them and the released binary tells every user
`[N UNRELEASED] … cut a tag`. It accuses itself.

So:

```sh
make changelog-cut VERSION=x.y.z   # folds changelog.d/ into ## [x.y.z] - <date>
git commit -am "docs(changelog): cut x.y.z"
make bump VERSION=x.y.z
```

A correctly cut build prints `omni vx.y.z [AHEAD/RC]` with no `UNRELEASED` line. Verify
that before pushing the tag.

**The half of that line to trust afterwards is the missing `UNRELEASED`, not the
label.** `guard::update::get_status` caches the newest known release in
`~/.omni/update_cache.json` for 14400 seconds, so for four hours after a tag a machine
that ran `contexere doctor` beforehand still holds the previous version and reports `Ahead`
whatever it is running. Observed on 0.7.5, where the freshly installed release printed
`[AHEAD/RC]`. `build.rs` computes the unreleased count from the tree with no cache, so
that half is always current; delete the cache file if you want the label to mean
something.

Day to day, the entry goes in `changelog.d/<issue>.<section>.md` as the work merges, not
into `CHANGELOG.md` and not at tag time. One file per entry means two branches never
write the same path, which is what stopped every parallel branch conflicting on
`## [Unreleased]`. The format is Keep a Changelog and SemVer, and the entries here are
unusually detailed on purpose: each states the measured evidence, the wrong number that
was published, and the mechanism. A one-line entry is a regression in that file's
quality.

The first cut needed one manual tidy and it is done. 0.7.5 folded three fragments beside
seven bullets written into `## [Unreleased]` before the convention existed, and arrived
with two `### Changed` and two `### Fixed` under one version heading. Merging those four
into two was the only hand edit. **Check a cut by word count rather than by eye**: 2,252
words across the old section plus the fragments, 2,252 in the folded section. A
reordering that drops a bullet body looks correct in a heading-level diff.

## CI green does not mean the release will build

The 0.6.2 tag produced **no binaries at all**. `release.yml` asked for `stable` per
cross target while `rust-toolchain.toml` pinned a version, so every cross-compile died
with `can't find crate for core` before compiling a line. `ci.yml` stayed green
throughout, because it only builds host-native.

The fix is that cross targets belong in `rust-toolchain.toml`, and its `targets` list
has to stay in sync with the release matrix.

**After tagging, watch the release workflow actually produce artifacts** before running
`make release-sha` or announcing anything.

## Things that look like failures and are not

`omni-release.sh` ends in an interactive `read -p`, so an automated run has to pipe
`echo y |` into it.

It pushes `main` and the tag together. `main` is branch-protected, and a maintainer
token bypasses it: the push prints *"Changes must be made through a pull request"* and
succeeds anyway with `rc=0`. That line is not an error.

## The Homebrew step

`update_homebrew_sha.sh` pushes to **two** repositories: the tap, and `omni.rb` back
to `main`. Check the tap clone is clean and synced with its remote first, or the run
aborts partway and leaves the formula half updated.

Afterwards, verify the formula's SHAs against the release's published `SHA256SUMS`
rather than trusting the script's own success line, then confirm:

```sh
brew info fajarhide/tap/omni      # expect: x.y.z → stable <new>
```

## Before merging anything into a release

CI green is not review-clean. Read the review comments, automated and human, validate
each one against the code rather than assuming the reviewer is right or wrong, and fix
or reply. A green pipeline says the tests passed. It says nothing about a correctness
bug a reviewer flagged.

## Branch shape

One branch per batch, not per issue. N parallel branches cost N full CI runs of about
eleven minutes each, serialised. Batch a lane into one branch, one commit per issue, one
pull request with several `Closes #N` lines.

Split only when a reviewer would genuinely need them apart, or when one is risky
enough to be reverted alone.

That conflict used to be `CHANGELOG.md`, every time. It is gone: entries are files in
`changelog.d/` now, and two branches never write the same path. What remains is the CI
cost, which is why batching still pays.

`Closes #N` must be in the pull request body **before** the merge. GitHub evaluates
the keyword at merge time only; adding it afterwards does nothing, silently.
