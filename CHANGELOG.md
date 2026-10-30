# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).


## [Unreleased]

## [0.8.2] - 2026-10-30

### Added
- **`contexere` on its own says what it did (#769)**: run with no arguments in a terminal, it prints one line from the same rows as `contexere stats`, such as `last 7 days: 255.9 KB never reached your model, 154 folds, 9,067 calls left alone`, then points at `stats` and `--help`. On Claude Code the hooks print no footer, so this is the one place a person sees the result without picking a view. Piped input works as before, and `contexere --help` still prints the help.
- **Contexere installs from crates.io (#876)**: `cargo install contexere` builds the same binary the release ships, from two crates, `contexere` and the library it depends on, `contexere-reduce`. Packaging failed before, because the library embedded the Hermes plugin from `plugins/hermes/`, outside the crate, and `cargo publish` only packages the crate's own files. The library now embeds its own copy, and `ships_the_same_plugin_users_install` fails when that copy and `plugins/hermes/__init__.py` differ. `cargo publish --workspace --dry-run` packages and verifies both: the library as 102 files, 431.2 KiB compressed, the binary as 11 files, 31.3 KiB. The manifest also drops to the five keywords and the category slugs crates.io accepts.

### Changed
- **The Homebrew formula is now `contexere` (#876)**: install with `brew install fajarhide/tap/contexere`. It still installs `omni` beside `contexere`, so hooks keep working. **If you installed as `omni`, run `brew trust --tap fajarhide/tap && brew upgrade fajarhide/tap/omni` once**: without it Homebrew skips the renamed formula, and `brew upgrade` stays on the old version with no message.
- **`doctor` lists what still uses the old name (#876)**: a host whose post-hook still calls `omni` gets a `Name:` line pointing at `contexere init`, and any `OMNI_*` setting with no `CONTEXERE_*` twin is listed under `Settings:`. Both keep working; the lines are the checklist for moving across before the old names are removed.
- **The repository is a Cargo workspace (#948)**: the reduction pipeline moved as-is into `crates/contexere-reduce`, and the `contexere` binary is now a thin crate that depends on it, so later pieces can live beside it instead of growing one crate. Nothing changes for a user: the same 1,197 tests pass, and each unit test now runs once instead of twice, because the binary no longer compiled its own copy of every module.

### Fixed
- **Token figures were counted in GPT's vocabulary (#584)**: the bytes-per-token divisors were calibrated against `cl100k_base`, so every token figure was 40 to 60% lower than what Claude bills. Measured with `count_tokens` on `claude-opus-5-5` over tracked files of public repos, they are now Code 2.5, Prose 2.5, Json 1.9 (its p10, so a budget guard never under-counts), BuildLog 2.3 and Mixed 2.2. The `Read` distill gate moved to 2,560 tokens so a code file is still distilled from 6,400 bytes, as before; JSON, TOML and YAML now reach the distiller from 4.9 KB instead of 5.6 KB, and `.log` from 5.9 KB instead of 7.6 KB. Markdown, text and other files pass through whole either way. The pre-compact summary cap, also in tokens, now cuts at about a third fewer bytes (Json 2.8 to 1.9). Percentages do not move, since the divisor cancels in a ratio.
- **A zsh `(eval):N:` error and a pod in `CrashLoopBackOff`, `ImagePullBackOff` or `ErrImagePull` tier Critical (#818)**: they tiered as context, so a distiller could drop them. In the first labelled sample (950 of 23,264 blocks, one labeller, reweighted by tier) this moves 289 blocks to Critical and takes the estimated miss rate for Critical plus Important from 60.0% to 14.1%. Treat those figures as a first estimate: blocks are not deduplicated, so one repeated zsh line accounts for 12 of the 17 misses.
- **A path to a secret was hidden as if it were the secret (#950)**: `KEY=/etc/app/backup_ed25519` and `TOKEN_FILE=/run/secrets/api_token` came back as `[REDACTED]`, which removed the one fact a runbook or script review needs. A path is now delivered only when the key agrees it names one: `*_FILE`, `*_PATH` and `*_DIR` keys take an absolute or `~/` path of two segments or more with path characters only, the bare `KEY` pattern only an all-lowercase one, and every other key (`API_TOKEN`, `SECRET`) none, because base64 can open with `/`. Replayed over 56,504 recorded lines holding `=`: at most 15 stopped being redacted, every one a `*_FILE` or `KEY` path, and none became newly redacted.
- **`doctor` no longer calls a correct MCP install missing (#966)**: three checks searched the config for `"omni"` while `init` writes `contexere`, so Gemini CLI, Cline and the six MCP-only hosts read `not configured` right after `init`. A server someone else named `omni` no longer passes, and one of ours on the old name gets a `Name:` line, for Claude Code, Cursor and Codex too (#876).
- **`make binary-check` finds the binary under `CARGO_TARGET_DIR` (#969)**: the size check and smoke test read a hardcoded `target/ci`, so with the variable set the size check printed "passed" over a missing file and the smoke test blamed a build that had succeeded. A missing binary now fails with its path.
- **`init` no longer erases a host config it cannot parse (#972)**: a file with a comment or trailing comma was read as empty and written back, losing every other setting and server. It is now left byte for byte, and `init` names the path. An absent or empty file still starts from `{}`.
- **A list of field names under a bare `KEY` is delivered (#974)**: `KEY = ("trace", "seq")` read as `KEY = [REDACTED]`, hiding the only definition of the name. Elements must be plain lowercase identifiers, so a mixed-case value and anything under a strong key such as `API_KEY` stay redacted. Replayed over 72,948 recorded assignment lines: 226 redacted before, 225 after, 0 newly redacted, and the one line now delivered is that list.
- **A method call with keyword or symbol arguments was hidden as a secret (#980)**: a credential-named key holding `Client.new.chat(query: query, history: history)` or `fetch_key(:billing)` came back as `[REDACTED]`, cutting a line of source someone was reading to edit. #783 recognised a call only when its arguments held no colon, comma or space. A call, or a chain of them such as `Client.new().get_token()`, is now a reference whatever its arguments hold, unless a quote sits anywhere in it, a space separates two calls, or an argument is a long word with a digit or mixed case, which is how a literal written bare or as a Ruby symbol looks. Replayed over 44,323 recorded lines holding `=`: 3 stopped being redacted, all three a `Client.new.chat(query: ..., history: ...)` call, and none became newly redacted.

## [0.8.1] - 2026-10-29

### Changed
- The Homebrew caveats, description and homepage, the share card, the dashboard and two manual diagrams now say Contexere; `fajarhide/tap/contexere` installs the same formula as `fajarhide/tap/omni`, and CONTRIBUTING.md plus a new AGENTS.md set out which old names stay (#960).

### Fixed
- `contexere --version` prints `contexere`, and `omni --version` through the old link still prints `omni` (#958).

## [0.8.0] - 2026-10-29

### Changed
- Homebrew and `install.sh` install `contexere` and keep `omni` as a link to it, and release archives keep their `omni-*` names with both binaries inside, so `brew upgrade` and every registered hook keep working across the rename.
- **The binary is now `contexere`, and nothing an existing install relies on breaks (#876)**: `omni` stays the same program, installed as a link to it, and serves its MCP tools as `omni_*`, while `contexere` serves them as `contexere_*`. `CONTEXERE_*` settings are read first and `OMNI_*` as the fallback, `~/.omni` and `omni.db` are read in place until a `~/.contexere` exists and nothing is copied or moved, and markers keep the `[OMNI` prefix and the `omni retrieve` hint. `contexere init` replaces the hook or MCP entry an omni install wrote on every host instead of adding a second one. `scripts/upgrade_smoke.sh` drives the old release's hook command against the new binary and back, and runs in CI.
- `omni doctor` now runs the post-hook each host has registered, with a canary credential, and reports whether it fires and redacts, instead of only whether the entry exists. A registered hook that cannot run is reported as `does not fire`.
- **The brand is now Contexere in the README, the manual and CLI copy (#956)**: titles, banners and help text say Contexere and docs use `contexere ...`; the `omni` command, `[OMNI` markers, `OMNI_*` settings and `~/.omni` are unchanged.

### Fixed
- A `*_URL` value with no `user:password@`, query or fragment, such as `postgres://localhost:5432/appdb`, is no longer redacted. A URL that carries credentials still is.
- A Python `key=lambda` argument is no longer redacted: a lambda under a `*key` name is code, not a credential.
- The openclaw plugin pins `@hono/node-server` 2.1.3, past the static path bypass in 2.1.1 (a development dependency).

## [0.7.13] - 2026-10-28

### Changed
- **The ledger stores each command once instead of on every line it showed (#911).** On a 30-day store that took `ledger_lines` from 420 MB to 50 MB and the whole `omni.db` from 615 MB to 262 MB, measured on a rebuilt copy. Rows written before the upgrade keep their text until they age out of the 30-day window, so the saving arrives as that window turns over; a `VACUUM` after that returns the freed pages to the disk.
- **Pinned Rust toolchain raised from 1.97.0 to 1.99.1 (#922)**: `rust-toolchain.toml`, both CI jobs and the release build now use the current stable. `make ci` passes on 1.99.1 with no new clippy lints and no code changes. Building from source needs rustup to fetch 1.99.1 once.
- **Eight fewer crates in the build (#926)**: `tracing-subscriber` and `hex` are gone, taking `matchers`, `nu-ansi-term`, `sharded-slab`, `thread_local`, `tracing-log` and `valuable` with them. Logging was only reachable through an undocumented `RUST_LOG`, and every hex string OMNI writes now comes from one `short_hash` helper that produces the same 16 characters as before, so ledger keys, rewind handles and project hashes already in `omni.db` still match.

### Fixed
- **A worktree-isolated Claude Code agent can run git again (#905).** Claude Code's worktree guard refuses every `omni exec ... git`, so the pre-hook now leaves git unwrapped inside a linked worktree.
- **A Read never folds from the project scope (#906).** Two subagents launched together each got `⋮` for the lines they were sent to read, because a sibling had read the file first. A Read still folds what the same reader was already shown, and Bash output still folds from the project scope. The trade is about 12% of ledger fold bytes, measured over 7 days.
- **A credential assigned mid-line after `export` or a shell operator, as a crontab entry or a `sh -c` one-liner carries it, is now redacted (#909).** Replayed over 10,981 recorded traces it hid 24 more values, all after `export` and all under credential names, and changed no line the old rules already handled.

## [0.7.12] - 2026-10-22

### Added
- **`omni stats --view bill` prices what the host billed, by model, project and session
  (#887).** Each request is priced from the model that served it at a dated API list
  price, with cache writes priced by TTL, fast mode at twice the rate and Haiku 5.5's
  long-prompt tier. A model missing from the table is shown as `unpriced` with its
  tokens, never as $0. `--by project|model|session` picks the table. Checked against an
  independent Python pass over one week of this machine's transcripts: $1078.32 against
  $1078.51, the gap being the live session's own transcript growing between the two
  runs. Spend counts subagent requests, which the token tables still leave out.

### Fixed
- **A credential inside a structured payload is hidden on the hook path (#866).** The
  format gate kept YAML and other structured output byte for byte and returned before the
  redactor, so a token in a `run:` block reached the model as printed. The value is now
  replaced where it stands, every other line comes back unchanged, and the notice goes to
  the hook's context message instead of into the document. `omni exec` and pipe mode
  still pass structured output through verbatim. A failed command's output is not
  covered yet.
- **A failing `bun test` keeps the lines that say where it failed (#875).** `bun` has no
  summariser, so its output reached the JS fallback, which kept only the failure and the
  tally. The `at file:10:38` line, the Expected/Received header and the code frame were
  cut under `[Partial signal]`, and a lone `^` survived without the line it points at.
  The fallback now hands a failing run back whole when it cannot parse it; a green run
  still gets the tally. Over one machine's recorded traces it had cut 5 failing runs for
  about 1.9 KB.
- **The release gate tests the build, not the maintainer's history (#877).**
  `tests/smoke_test.sh` assumed an empty store, and `omni-release.sh` ran it against the
  live `~/.omni`, where fold markers have been recorded since 0.7.10, so the 0.7.11
  release stopped at 75 of 76. The smoke test now brings its own temporary home unless
  the caller set `OMNI_HOME`.
- **Query rows from `sqlite3`, `psql` and `mysql` reach the reader as printed (#886).** A
  result was cut to its first line, labelled `Query result columns`, and three sampled
  rows, so with `-separator` and no `-header` a data row was presented as the schema and
  29 rows came back as 4. Its own count and the fold marker disagreed, 28 against 26.
  The collapse fallback stays off these clients too, since it folds identical rows and a
  one-column `select status` is exactly that.
- **rustls is 0.23.46 (#888).** 0.23.45 carries four advisories published after 0.7.11
  (RUSTSEC-2026-0346 to 0349), among them a ServerHello downgrade and `Debug` output that
  exposes TLS secrets. It reaches OMNI through `ureq`, which the update check and cloud
  calls use. Nothing else in the lockfile moved.
- **A successful query is no longer reported as a database error (#891).** The database
  summariser picked its error arm whenever the output contained `error:` anywhere, so 30
  rows of which two held `error: timeout` came back as `DB Error (2 found)` and the other
  28 were gone. The distiller is deleted: over one machine's store it had rewritten 6 of
  42 `psql` and `sqlite3` calls for about 11 KB, and a client's own error is a line or two
  that the summary made longer. `psql`, `mysql`, `sqlite3`, `pg_dump` and `redis-cli`
  output now passes through.
- **A comparison is no longer redacted as an assignment (#893).** A line opening with a
  sensitive-looking name and `==`, `===`, `=>` or `=~` was split at the first `=`, so a
  wrapped condition `firstToken === "py" ||` came back as `firstToken =[REDACTED]`, its
  operand and `||` gone. Only a spaced `=` counts as an operator, so `printenv`'s
  `PASSWORD=~r4nd0m` is still hidden. Replayed over 21,521 recorded lines holding an
  `=`: six stopped being redacted, all of them comparisons, and none started.
- **rustls-webpki is 0.103.16 and webpki-roots 1.0.10 (#898).** The older versions could
  accept distrusted intermediates during certificate verification (RUSTSEC-2026-0382 and
  0381). They reach OMNI through `ureq`. Nothing else in the lockfile moved.

## [0.7.11] - 2026-10-14

### Added
- **`scripts/ab_cache.py` measures whether OMNI costs prompt-cache hits, and the README now
  says what it found (#610).** Each pair is the same prompt run twice through `claude -p`,
  once with the hook live and once with `OMNI_PASSTHROUGH=1`, order alternating, and the
  report reads the host's transcripts and never OMNI's tables. On omni 0.7.10, 20 pairs,
  one six-command workload, one machine: tool result bytes -39.1%, cache-write tokens
  -18.1%, billed cost -11.0%, and output tokens +92.5%, whose cause the run does not
  establish. The last figure goes against OMNI and is printed beside the others in
  the README and the manual.
- **`omni stats --view bill` reads the host's side of the books (#859).** Every other view
  reports what OMNI did, from OMNI's own tables, so none could say what a session cost or
  where its context went. This one reads Claude Code's transcripts: billed tokens by class,
  context bytes by source, and tool result bytes by tool, for the `--since` window. A
  request is counted once by `requestId` because the host repeats `usage` on every record
  of a request, and the blocks on a repeated record are still read. Subagent records are
  left out. It prints no dollar figure. Checked against an independent count over two
  transcripts: 14 requests, 765,189 cache-read tokens and 125,955 B of tool results in
  both. `CLAUDE_CONFIG_DIR` is honoured, as the host honours it.

### Fixed
- **The benchmarks page states the release it is on, and says why `search` reads zero
  (#842).** The page regenerates its table from the artifact and hand writes the sentence
  above it, so the two drifted a release apart: the table said 0.7.10 while the prose still
  said 1.4% from the filters and 3.0% with the ledger, plus three more figures from the
  previous run. A test now holds the whole
  sentence to the artifact, scoped to the current run's section because the page is an
  archive and carries retired figures of the same shape, and the harness records the call
  outcome and token figures so all five are checkable rather than the two that happened to
  be recorded. The head to head prose below the table was a release behind in the same way. The `search` class reading
  0.0% captured is explained where it is printed: #815 made a `grep` reply fold whole or not
  at all, a whole fold needs the identical reply to have been printed before, and this
  corpus holds no repeat of a `grep` command inside one session across 1,069 traces. The
  refusal costs 4.6 KB over 810 calls.
- **Redaction ends an unquoted shell value at whitespace, and looks at every assignment on
  the line (#851).** Everything after the first `=` was the value, so
  `APP_TOKEN=x ./bin/app serve &` was delivered as `APP_TOKEN=[REDACTED]` with the command
  gone and nothing saying so. The same rule read only the first assignment, so
  `APP_ROOT=/tmp/x APP_TOKEN=x ./bin/app` was delivered whole, token included. A line is
  now redacted word by word, and counted per value, when it proves it is a shell line: it
  opens with two or more `NAME=value` words, or with one and ends in a shell operator or a
  continuation (`&`, `&&`, `;`, `|`, `\`). Anything else keeps the old rule on purpose.
  `API_TOKEN=abc ./run` and `PASSWORD=correct /horse` are the same shape, so that command
  is still lost rather than half a passphrase delivered, and a quote or an escaped space
  anywhere on the line keeps the old rule too. On such a line a credential behind an
  ordinary assignment is hidden from its `=` to the end of the line, so falling back
  never delivers one. `env` output is never split: its distiller reads one assignment per
  line whatever the value holds. A bare `auth` holding one to three digits behind another
  assignment is a status and is delivered, as in `front=200 auth=302`; any other
  credential name keeps its value hidden however short it is. Replayed over 20,626 recorded
  lines holding an `=`: 54 redacted before and after, 12 changed, every one of them a
  command kept after its token.
- **`az ssh vm -- <cmd>` output is the remote command's, and is delivered as such (#858).**
  `wraps_another_command` knew `ssh`, `kubectl exec` and `az aks command invoke`, and did
  not know `az ssh`, so the remote command's output was judged against az's grammar. A
  23 line `du` listing came back as the two ssh client warnings and `[OMNI: 21 lines
  omitted]`, which is the answer cut and the noise kept. `az ssh vm` and `az ssh arc` now
  route like `ssh`; `az ssh config` does not, since it prints az's own output. Over the
  last 30 days here that is 7 of 15,887 calls and 5,180 B no longer removed.
- **A provider token with no name beside it is redacted (#862).** The redactor keyed on
  the name left of `=`, so a credential printed on its own was delivered: sourcing a file
  that holds a bare key puts the key in the shell's `command not found` line. A value
  carrying an issuer's prefix at the length the issuer writes it is now hidden wherever it
  appears: `sk-ant-`, `ghp_` and its siblings, `github_pat_`, `AKIA`, `xoxb-` and its
  siblings. The length floor is the rule. Over 3,860 recorded traces the prefixes matched
  25 times and 23 were fixture-sized strings such as `sk-ant-abc123`; with the floor the
  rule touches two lines, one real key and one test fixture. The whole redactor, this rule
  included, takes 11 µs per payload on a release build over that corpus. Not covered: the
  output of a command OMNI can see failed. Where the host reports the failure, the output
  is handed back untouched (#120), so a token it prints is still delivered, and on Claude
  Code a failed `. ~/.keyfile` is exactly that case (#866). OpenCode and Continue report no
  failure, so their output is rewritten either way.

## [0.7.10] - 2026-09-23

### Added
- **The ledger records which marker shape it rendered (#766)**: `ledger_folds` groups a
  call's folds by origin and source agent and carries no handle, so it could say how much
  was folded and never which of the five marker wordings a reader met, nor whether anyone
  went back for the bytes. Three wording changes shipped on reasoning alone for that
  reason. `fold_markers` records the shape and the handle at the grain a marker lives at,
  the shape comes from the arm that renders the string rather than from parsing one back,
  and `marker_retrieve_rates` joins the handle to a later retrieval. A shape with a high
  retrieve rate is one that did not answer the question it interrupted.
- **`omni stats --view folds` prints what a fold claimed and what came back (#816).** A
  fold says the reader does not need these bytes; a retrieve on that marker's handle says
  it did. The pair has been computed since #771 and read by nothing outside tests, so the
  ledger's guards were tuned from bug reports rather than from its own rate. The view
  lists folds, retrieves and the rate per marker shape, with no verdict colour and no
  threshold, because no bar has been measured yet. `--view calibration` is the same view.
  A store written before markers were recorded says so instead of printing a confident
  zero.
- **The signal classifiers can be scored against a labelled corpus (#818).**
  `cargo run --example sample_blocks` takes a stratified sample of `bench-corpus` and
  writes one row per block with the tier `semantic::classify_block` gave it, which is the
  classifier every scoring path uses. `scripts/score_labels.py` turns a labelled copy of
  that sample into precision, recall and a miss rate for the Critical tier, and exits
  non-zero when the miss rate regresses against a pinned baseline, so the bench can gate
  on it. Labelling itself stays a separate step: 27.2% of the corpus carries an employer,
  client or infrastructure name and 4.2% an assignment to a credential-shaped key, so
  nothing in it may be handed to an outside labeller as it stands. The sampler runs the
  shipping redactor over every block and holds back anything matching the operator's own
  patterns, read from a file rather than carried in this repository.
  The sample is drawn by a hash of each trace's key rather than in corpus order, so it
  spreads across a class instead of grading the oldest traces in it, and a trace whose
  blocks are all held back does not spend the class's quota.
- **A fold records which tool produced the reply (#822).** `fold_markers` kept the marker
  shape and never the surface, so a rate could say `project_run` is retrieved more often
  than `session_run` and never say whether that lands on `Read`, `Grep` or a shell
  command. `omni stats --view folds` groups by both now. Rows written before the column
  read `unrecorded` rather than being attributed by guesswork. The ledger takes the tool
  name once instead of the two booleans the hook derived from it, `renumbered` (#657) and
  `searched` (#814), which is one fact travelling once.
- **A second benchmark corpus, and one anyone can rebuild (#838).**
  `scripts/build-swebench-corpus.py` turns SWE-bench Verified instances into a corpus in
  the existing schema: it clones each repository at its pinned base commit and replays a
  fixed exploration script, with no model, no API key and no container, so a reader can
  rebuild it byte for byte and check the number rather than trust it. `scripts/bench.sh`
  takes `CORPUS_DIR` and names its artifact after the corpus, so the two series never
  overwrite each other. On 40 instances across 10 repositories the same build reads 25.2%
  against 0.9% on the local corpus; the difference is the workload, which is 85% source
  file reads here against a shell-heavy mix there. The commands are a fixed script rather
  than an agent's choices, and whether OMNI costs task success is a separate question that
  needs a real agent and a budget.

### Fixed
- **`omni init --hook` registered an MCP server it said it would not (#757)**: the flag has
  read "Only install hooks" since it shipped and installed both halves anyway, and then
  `omni doctor` called the resulting state a broken install and `omni doctor --fix` undid
  it. Three ways of overruling a choice the CLI offered in the first place. `--hook` and
  `--mcp` now install the half they name, naming the host or both halves still gets both,
  and an absent MCP registration on Claude Code is reported as what it is rather than as a
  fault: the hooks are what shortens output there, and the server is a convenience whose
  tool definitions sit in the prefix of every request.
- **A decline now records who it was declined for (#773)**: `passthrough_events` carried the
  command, the byte count and the reason, and no agent, so OMNI could say what it handed
  back and never what share of it was handed back to a model rather than to a shell. The
  obvious proxy is wrong: a hook payload can arrive with no session id, and `host output
  cap`, a branch that only fires for Claude Code, has 20 of its 39 rows carrying none. The
  column makes the split a fact, and `passthrough_bytes_by_agent` reports it. Rows written
  before it read `unknown` rather than being guessed at.
- **A folded reply carried a second accounting of itself (#775)**: on a re-run of
  `git log --oneline -40` the distiller cut a single byte, the ledger folded the reply to a
  few lines, and a banner then announced `1 bytes omitted` on top of fold markers that
  already accounted for all 40 lines. Every figure was true and the arithmetic a reader can
  do was not. The banner is weighed against the distiller's own output now, since that is
  the stage its numbers describe, and when it cannot pay for itself the reply keeps the
  ledger's fold instead of being thrown away: the first cut of this fix turned a 3,257 byte
  reply folded to 325 into a passthrough. Singulars too, so no more `1 bytes`.
- **A folded reply was labelled a partial signal (#775)**: `[Partial signal]` says the
  pipeline recognised some of the output and not all of it, which is a claim about the
  distiller, and it was being read off a route computed after the ledger folds. So a
  payload the distiller never touched could reach that route on the strength of a fold and
  carry the banner over an answer nothing was lost from: 16 of 40 lines folded with a
  handle, 24 verbatim, and the reply calling itself partial. The banner now reads the
  distiller's own ratio. The pipe door was checked and never had this: its ledger runs
  after the banner, so its ratio was already the distiller's.
- **A cut too small to announce no longer disappears under a fold (#778)**: when the
  distiller's cut was smaller than the marker that would name it, the marker was not
  printed and the reply was handed back whole, so nothing was lost. Since #777 the
  ledger's fold is kept in that situation, and the fold markers answer for the lines
  they replaced while nothing answered for the bytes the distiller took. Under 80 bytes,
  and one byte in the case that found it, but unannounced and unreachable is the one
  thing this pipeline is not allowed to do. The floor is now tested before the ledger
  runs and the ledger is handed what the command produced, so the fold is still kept and
  every stage still answers for its own bytes. Measured first: the arm has not been
  reached once in 745 recorded fold calls, so this is a guarantee being closed rather
  than a saving being recovered.
- **A fold names the command that printed the lines, not the one in front of it (#779)**: the
  `from` clause was built from the first line of the recorded source, and a recorded source
  is the whole command string. 28.7% of sourced lines here were recorded against a
  multi-line command and 86.6% of those open with `cd <path>`, so a quarter of every `from`
  clause credited the bytes to a chdir, which writes no stdout at all; a `for` header
  claimed 18 lines a `cat` inside the loop had printed. One subagent read the mismatch as
  the `Read` tool returning another file's content and re-read the whole tree before it
  would trust it. The clause now resolves the source through the same predicate that
  decides which segment a distiller may claim, so a single producer behind a chdir is named
  correctly and a chain with two producers carries no clause: stdout is one stream and
  nothing in it says which of them wrote a given line.
- **A value that names a credential is no longer destroyed like one.**
  `redact_sensitive_assignments` checked the key and never looked at the value, so any
  line assigning to a variable called `token`, `secret`, `password` or `api_key` arrived
  with its right-hand side replaced, even when that side was plainly code. Reading a
  Python file with a loop variable named `token`, the line `token = token.strip()` was
  delivered as `token = [REDACTED]`, and the marker gave no hint the cut was a false
  positive: six of seven lines in the reported repro lost their value and only one was a
  credential. An unquoted call and the null literals now pass through for every pattern,
  the way a `$VAR` expansion already did, because neither holds credential material
  whatever the key is named. Under those strong patterns nothing else changed, so a bare
  identifier (`hunter2` has that shape), anything carrying a quote
  (`decrypt("ghp_real")`) and any dotted value are still cut, the last because a JWT is
  alphanumeric runs joined by dots and nothing separates it from `cfg.api_key`. The weak
  `KEY` pattern keeps the wider exceptions it already had from #486 and #530.
- **A diff keeps its changed lines when the same text was shown earlier (#788).** The
  already-shown fold matched a `+` or `-` line against identical text printed before, for
  example by a `git show` of the commit that first added it, and folded it. Reviewing a
  staged diff then delivered the unchanged context and none of the additions or
  deletions, with nothing saying a changed line was gone. In a payload carrying a `@@ `
  hunk header, `+`, `-` and `@@` lines are now never folded, the exemption failure lines
  already had.
- **`kubectl logs` passes through (#789).** The kubectl summariser kept up to ten
  severity-matched lines, so an 18 line application log came back as five copies of one
  ERROR, and the 13 INFO lines walking a connection pool from `1/5` to `5/5`, which were
  the cause, went behind a handle. A log stream is the answer the caller asked for.
  Measured before choosing: of 155 recorded `kubectl logs` calls over 30 days, 148
  already passed through and the other seven saved about 22 KB.
- **A project-scope fold needs a sighting from the last six hours (#795).** The project
  scope replaces lines another session was shown, and the marker says so. What makes that
  a fair bet is that the other session is alive, or was minutes ago. A day-old sighting is
  a different claim: in the report a `grep` came back with 9 of its 11 matches behind a
  handle, credited to a session from the previous day and a directory that no longer
  existed. Six hours is measured rather than guessed: across 734 project folds in 30 days
  of the maintainer's store, the freshest line in the scope was 0 seconds old at the
  median, 259 at p99 and 1,178 at the worst, so the bound refuses nothing that was
  recorded. The session scope keeps no bound, because there the reader is holding the
  bytes however long ago they arrived.
  A line delivered in full again refreshes its sighting, so the bound asks when the bytes
  last reached an agent rather than when they first did.
- **A `Read` with `offset` or `limit` is no longer folded against project history
  (#796).** The ledger's line-budget guard reads the command, and for a `Read` the command
  is the bare path, so a window picked to see one model's fields came back with those
  fields replaced by a handle, and the host then refused the same `Read` again as
  unchanged. The Claude Code normalizer now reads the window, and a windowed `Read` gets
  the guard `tail -5` and `sed -n 60,200p` already had: session-scope folds still apply,
  project-scope ones do not.
- **`npx` is routed by the program it launches (#797).** Every `npx` command went to the
  JS/TS distiller, so `npx tsx report.mts` printing seven result lines came back as its
  last line, `console errors: none`, which reads the same for a finished run and one that
  stopped halfway. A JS tool or test runner behind `npx` keeps its distiller, and anything
  else passes through.
- **Base64 is treated as structured, so neither the distillers nor the ledger touch it
  (#798).** A reprinted base64 payload was replaced by an `already shown` marker, which
  broke the obvious repair for a damaged paste, and the first print had passed through
  only because `tr` sits on the passthrough list. The format sniffer now recognises one
  line of the base64 alphabet, or lines wrapped at a fixed width of 60 or more, from
  256 bytes up and mixing at least two of upper case, lower case and digits. Lists of
  names or short hashes, and a long run of one letter, do not match.
- **`readfile` no longer summarises a `Read` window as the whole file (#799).** A 340 line
  window of a Rust file cleared the distiller's token gate and came back as
  `Imports: None in the full file` with no lines of content, while the file's `use` lines
  sat above the window. A `Read` that names `offset` or `limit` now skips the distiller.
- **A redacted secret is no longer handed back raw by the collapse fallback (#800).**
  Redaction ran on every payload, but its output is barely shorter than the input, so for
  any command outside the verbatim list the post-hook replaced it with a collapse of the
  raw output, which carried no `[REDACTED]`, and the guardrail then returned the original
  bytes. `printenv`, `set`, `export -p` and `echo` delivered `DB_PASSWORD=` in full while
  `cat .env` and `env` were redacted. A distilled output that redacted something now skips
  the fallback. A payload under 50 bytes skipped redaction altogether, so a bare
  `DB_PASSWORD=hunter2` was delivered on every command; the size floor now stands aside
  when there is something to redact.
- **A `git diff` keeps its changed lines whatever flags precede the subcommand (#805).**
  `resolve_profile` read the word after `git` as the verb, so `git -C repo diff` was not a
  diff: it segmented by line, and the diff distiller then had no hunk header inside a
  segment to walk. On a 3.7 KB diff it kept the two `@@` lines and dropped all 83 changed
  ones. The verb is now read positionally, past the global flags and the values they take.
  The `omni exec` path resolved its profile from the first token as well, which made
  `cd repo && git diff` a `cd`; it now uses the chain resolver the Bash hook has used
  since #339.
- **Compaction forgets a session's subagent scopes too (#807).** The ledger's licence is
  that the reader is still holding the bytes a handle replaces, and compaction is where
  that stops being true, so `PreCompact` clears the shown set. It cleared the bare
  session id only, while a subagent's scope is `<session>/<agent>`, so those rows kept
  answering `already shown` for a context the summary had taken away. The reported
  main-agent case did not reproduce on 0.7.9 or on `main`: across the nine compacted
  sessions in this machine's store, holding 19,853 ledger lines, none is older than its
  session's last compaction.
- **Output the host truncated is no longer booked as delivered (#808).** Claude Code cuts
  an over-cap Bash reply one byte short of the 30,000 byte cap: four of four truncated
  payloads in this machine's trace store arrive with stdout at exactly 29,999 bytes, the
  last a 620 line file that stops at line 590. The gate tested `>= 30,000`, so it never met
  one, and the ledger recorded all 1,186 lines as shown. A later `tail` of the same file
  then folded 90 lines the session had never received, which is #716 returning through an
  off-by-one.
- **A `grep` reply folds whole or not at all (#814).** The pattern already picked every
  line, so folding some of them answers a different question rather than the same one
  shorter: 9 of 11 matches replaced by a handle came back reading as a file with two
  matches. `grep`, `rg` and `ag` now get the guard `head`, `tail` and a `sed` range
  already had. A re-run that prints the identical result still folds as a whole, which is
  the case where the identity is the answer. Split out of #795, whose other half, whether
  a project-scope fold may draw on another session at all, is still open.
- **A Rust panic is a failure whatever printed it (#819).** `panicked at` was known to the
  `cargo` and `rustc` arm of the tier check only, so the same panic arriving through a
  shell script, a make target or any test wrapper scored as ordinary context. On a 408
  line payload the reader kept 96 `Compiling` rows and lost the one line naming the test
  and the source location, under a marker that only said how many lines went. The generic
  list carried the Go shape, `panic:`, and not the Rust one.
- **`omni_retrieve` takes the word the marker prints (#826).** Every marker ends in
  `omni retrieve <handle>` and so does the CLI's usage line, but the MCP tool's field was
  named `hash`, so an agent that copied what it was told to run got `missing field hash`
  back and the content stayed behind the handle. The field is `handle` now, with `hash`
  kept as an alias so a caller written against the old name keeps working.
- **A unified diff is delivered as it was printed (#832).** `git apply` and `patch` parse a
  diff the way `jq` parses JSON, so it now sits on the same gate rather than being guarded
  one stage at a time. Three things were rewriting it at once on 0.7.9: the distiller
  replaced the `diff --git`, `index`, `--- a/…` and `+++ b/…` header with a bare `b/…`, the
  ledger folded the context lines away because an earlier `cat` of the same file had shown
  them, and the `@@ -5,7 +5,7 @@` header was left claiming seven lines on each side of a
  hunk that showed two. `git apply --check` answered `patch fragment without header`, and a
  reader taking the header at face value believed six lines of context had been inspected
  and found unchanged. Guarding only the changed lines had already been tried and does not
  cover this: #788 and #805 report the mirror of it. `git diff --stat` and `git show --stat`
  carry neither marker and are still distilled.
- **A `gh` listing is delivered whole (#833).** `gh issue list --limit 20` returned twenty
  rows and the reader got ten, under `... [10 more items, use --limit to see more]`, which
  names a flag the command already carried: raising it returns more rows from `gh` and the
  reply is still ten. #235 narrowed this summariser to the `list` subcommands on the
  grounds that `--limit` was the tell, and #833 is that the advice is wrong on the one
  surface it was left. Every row of a listing is an item the caller asked to see, which is
  the rule `find` (#198), `ls` and `ps` (#200), `docker ps` (#233) and `kubectl` (#301)
  already have. #314 closed this class for `aws s3 ls` and `gcloud` at one occurrence each
  and said a family earns the guard once it shows real volume: 219 `gh` commands in 7 days
  on the reporting machine, 37 of them list shaped, against 14 `kubectl`. The summariser
  had no other caller and is deleted rather than guarded.
- **`rmcp` moves to 2.1 (#836).** GHSA-9g45-5xwm-f3wc, custom HTTP headers leaking to
  cross-origin redirect targets, is fixed in 2.1.0. OMNI speaks MCP over stdio and builds
  `rmcp` with `transport-io` only, so the advisory's HTTP client path is not one this binary
  runs; the bump closes the alert that printed on every push rather than a reachable
  vulnerability.
- **The figures writer picks its artifact by version, and the gate ignores a foreign one
  (#840).** `sorted(["0.7.9.json", "0.7.10.json"])[-1]` is `0.7.9.json`, so from 0.7.10
  onward the writer that regenerates the README's table would have taken the previous
  release's numbers, and the gate that checks the table would have passed on them. The CI
  gate itself already ordered by parsed version; what it lacked was a rule against the
  second artifact family #838 adds, which sorted below the local one by the accident of
  failing to parse rather than by anything stated. An artifact whose name is not purely a
  version is now not a candidate for the main table.

## [0.7.9] - 2026-09-04

### Changed
- **The head to head is published again, and it does not favour us (#712)**: the
  comparison against rtk, lean-ctx, caveman and headroom was withdrawn in #710 because
  the corpus behind it had been deleted, and the re-run that was supposed to replace it
  had two silent arms. #711 fixed one. The other was calling `caveman tools compress`,
  and nothing on the measuring machine answers to a bare `caveman`: pointed at
  `caveman-shrink`, `tools compress` is not a subcommand it knows, so it falls through
  to shrink mode and hands the payload back with `"ratio":0`. An arm reporting zero on
  every trace reads as an arm that lost. `caveman-engine compress` is the entry point
  that works. All four arms now run on every `make bench`, resolved from the machine
  rather than from four environment variables somebody has to remember, and the rows
  land in `docs/benchmarks/<version>.json` and in the generated table in the README and
  on the benchmarks page, so the comparison is a build output that cannot drift from
  its corpus again. A named arm that produces no row now aborts the artifact instead of
  being published as a zero. On the frozen corpus OMNI is last of the four rows that
  carry a ledger: headroom's cross-turn dedup takes 5.8% where ours takes 4.9% over
  identical filters and identical blocks, and our filter tier is the weakest of the
  three at 1.4% against 2.1%. `OMNI_BENCH_ARMS=off` skips the competitor arms, which
  take the run from about 40 seconds to about 20 minutes.

### Fixed
- **The project scope replaced whole replies, leaving the reader nothing (#705)**:
  a freshly dispatched subagent's first command came back as
  `[OMNI: identical to 40 lines from an earlier session, none shown here]` and no
  content at all, and it was never a subagent property: a second session on the same
  project got the identical empty reply with the identical handle. A reviewer
  dispatched onto a pull request had to pipe files through `base64` to read the code
  it was sent to review. The session scope may still fold a whole reply, because that
  reader is holding the bytes. The project scope may not: it answers for lines the
  reader has never seen, so it now folds only part of a payload and always leaves real
  lines to weigh the marker against. Not the reader's own set, which the report
  proposed and the corpus falsifies: all four recorded whole-output project folds that
  name a session happened in a session already holding 261 to 1,369 lines of its own,
  the 1,319-byte case in the report included. Priced over this store the class is 10
  folds and 32,104 bytes, 1.51% of every byte folded, against the 678,585 bytes the
  partial arm keeps.
- **A third of all traffic was filed under `cd` (#706)**: command classes were keyed
  on the first two words of the recorded string, and half of what an agent runs is a
  multi-line block whose first line changes directory. So `omni stats --view
  commands`, `omni context --tokens` and the per-agent map all answered "where did
  your bytes go" with the name of a directory change, 7,222 calls deep on this
  machine's corpus, and the answer was true and unusable. The key is now the segment
  that actually wrote the output, the same predicate the pipeline routes on, so the
  block is filed under the program that produced the bytes. A leading loop keyword
  goes the same way, since `do` names no program either, and the program is reduced
  to its file name so `/opt/homebrew/bin/python3` and `python3` are one row. The
  local absolute path that used to head the table goes with the `cd`: 6 of 2,051
  classes still carry one, against the single heaviest class before.
- **The headroom arm of the replay benchmark had never run (#711)**: pointing
  `OMNI_BENCH_HEADROOM` at the `headroom` command handed
  `spec_from_file_location` a console script, so the driver could not import
  anything and the harness printed "headroom arm did not run" instead of a
  number. It printed that every time, and headroom is the one competitor that
  ships live cross-turn dedup, so the arm the comparison exists for was the arm
  that was always absent. The command now resolves to the module file through
  its own interpreter, which works whichever python it was installed under, and
  a named arm that produces no number fails the run rather than narrating its
  own absence. First measurement on the recorded corpus, 9,469 traces: their
  dedup 6.7% against our ledger 6.3% on identical blocks.
- **The ledger claimed lines the host never delivered (#716)**: Claude Code caps
  Bash output at 30,000 bytes, writes the full text to a file, and shows the model
  a 2 KB preview instead. The gate that declines those payloads read
  `tool_response.stdout`, but `normalize` takes a Claude Code payload's text from
  `content`, `file.content` or `stdout`, and the host sends plenty of over-cap
  results in the first two. Those runs were recorded as delivered, so a later
  `tail` of the same file came back as `[OMNI: 90 lines already shown]` for lines
  that had never been in context: a 620-line file was unreachable by `tail`, by
  `sed`, and by any ordinary command, with nothing in either marker to say so. The
  gate now measures whichever field `normalize` actually read, in its own
  precedence, still on one field so a large stdout beside a large stderr is not
  mistaken for a truncated result.
- **A declined payload's bytes never reached the context breakdown (#725)**: six reasons
  hand a payload straight back, and every one of them returned before the accounting, so
  `omni context --tokens` reported those turns lighter than they were. Measured on this
  machine over 23 days: 569 calls and 1.37 MB missing, of which the host-cap branch the
  report named was 19 calls. The count now happens at the shared exit. The one branch
  deliberately left uncounted is that same host cap: Claude Code writes the raw output to
  a file and keeps a preview, 2,233 characters for a 36.1 KB reply on record, so booking
  the 30,000 the hook was handed would overstate the turn rather than correct it.
- **A redacted empty default came back as broken syntax (#726)**: reading a
  `.tsx` component to plan an edit to it, `apiKey = "",` was delivered as
  `apiKey = "[REDACTED]"`, with no comma and no credential anywhere in the file.
  Two defects on one line. The value reaches the redactor with its quotes and
  its trailing punctuation attached, so an empty string never read as empty and
  every key called `apiKey`, `token` or `password` had its default rewritten
  whatever it held. The replacement then consumed everything after the opening
  quote, which is the punctuation loss #486 fixed for HCL and did not generalise
  past a value with nothing after it. An agent that copies the delivered line
  into an edit writes a syntax error, and nothing in the output says the
  punctuation moved. The literal between the quotes now decides whether there is
  a secret to hide, single quotes keep their delimiter the way double quotes
  already did, and trailing punctuation comes back with the line. Anything else
  after the closing quote could be a second literal and is still redacted.
- **A competitor arm that changed nothing was published as a measured loss (#733)**:
  every arm in the head-to-head hands back the raw payload when its binary fails,
  `rtk_out` from three separate paths, so a tool that cannot spawn or cannot parse its
  arguments does not go silent the way #711's headroom arm did. It emits a full table of
  unchanged bytes and lands as a tidy 0.0% row, which is what `caveman tools compress`
  did for four releases, and #730's presence check cannot see it because the row is
  there. A named arm whose filter-only row saved nothing now aborts the artifact instead
  of publishing the zero. headroom is not probed that way on purpose: its arm already
  panics when it produces no number, and a headroom that deduped nothing would read as
  the filters-only figure rather than as zero. What the machine could not find is
  recorded in `arms_absent`, so a three-arm table can no longer be read as a four-arm
  comparison, and `OMNI_BENCH_ARMS=off` works again after #732 made an empty `arms` read
  as a failed measurement.
- **A `tail -5` came back as a handle and none of its five lines (#735)**:
  the coverage floor added by #543 and tightened by #643 asks what share of the
  *payload* a fold took, and a compound reply hides the answer. The reported call was
  `cat notes.md; echo ---; tail -5 index.md`, where every line of the `tail` was folded
  through the project scope and the run still measured 45%, because the `cat` in front
  of it paid for the other 55%. The floor never looked, #705's whole-output guard never
  fired because the `cat` survived, and the reader got a header announcing content
  followed by a marker instead of it, then spent an `omni retrieve` to get back 965
  bytes the fold had saved 910 of. Nothing at that stage can find a command boundary
  inside a joined payload, so the ledger now asks the command rather than the bytes: a
  `head` or `tail` counting lines has already named the answer, so its retrieval is not
  a risk the project scope is taking, it is the outcome. Only the project scope is
  refused, since the session scope's reader is holding the bytes and its handle costs
  nothing. `tail -f` and `head -c` do not count, because neither names a set of lines.
  Deliberately narrower than `registry::passes_through_verbatim`, which reaches the same
  conclusion one stage earlier for the collapse fallback: its list carries `cat` and
  `grep`, and those are where the project scope earns most of what it earns. Measured
  over the 7 days in `ledger_folds`, 569 folds and 564,995 bytes: dropping the size
  precondition on the existing floor instead, which is what the report first proposed,
  would give up 170,518 bytes, 30.2% of every byte folded, and still miss this case.
- **The ledger was told which command it was answering on one door of three (#736)**:
  `Ledger::from` names the command a reply belongs to, and only `fold_cross_turn`
  passed it. `process_payload`, which is where a Bash reply actually meets the ledger,
  and `pipe::distill`, which is where an `omni exec` reply does, both left it empty,
  though each had the command in scope. An empty source never equals a recorded one, so
  #622's differing-source clause could not fire on either: a marker there either named a
  source it should not have or, on a first sighting, said nothing while the ledger knew
  better. It also made #735's guard inert on the only path that had reported the defect,
  which is how the same fix could pass its unit test and change nothing in the shipped
  binary. Driving the built binary through `--post-hook`, a repeat now reads
  `200 lines not shown here from cat handlers.log` where it read `200 lines not shown
  here`. Same shape as #452, #454 and #456: one pipeline, more than one entrance, and a
  fix applied to whichever entrance the report came through.
- **`grep -rn head src/` turned off the project scope (#738)**: the line-budget predicate
  added in #737 matched `head` or `tail` anywhere in the command string, so any search
  whose *pattern* was one of those words, or any pipeline ending `| grep tail`, read as a
  reader rationing its own output. It failed safe, since the reader kept its content, but
  it gave up folds for nothing on two shapes this repository runs constantly. The name now
  counts only in command position: the first word of the reply, or the first word after a
  shell separator. A separator glued to the following token (`cat a.md;tail -5 b`) is not
  recognised and that shape folds as before, which is a deliberate limit rather than an
  oversight: agents write the spaced form, and a tokeniser is a larger thing than this
  predicate deserves. Raised by review on the pull request that shipped the defect and
  merged ahead of the fix, which is the second time in three pull requests.
- **Nothing asserted that the ledger folds across a branch switch (#739)**: the host's
  prompt cache is pinned to the git snapshot it started on, so a checkout or a new commit
  starts the next session cold even where the bytes are identical. The ledger's project
  scope is keyed on the repository root and never reads a ref, which is the one saving in
  this lane the cache structurally cannot make, and it was true only by construction. A
  test now primes a project scope, switches branch and lands a commit under it with real
  git, and asserts a new session still gets the fold.
- **A `sed` range of a source file came back as a marker on its first appearance (#741)**:
  `sed -n 60,200p` of a React component, in a context cleared moments earlier, returned 28
  lines of the `steps.map` render branch as `[OMNI: 28 lines not shown here]`, and the next
  step was editing that render path to add a case. An edit written against a marker is a
  wrong edit, which is a worse outcome than the wasted retrieval #735 described. The ledger
  behaved as designed: the lines were seen in an earlier session of the same project and
  the marker used the project-scope wording that does not claim this context saw them. The
  design was one command family too narrow. #737 taught the ledger to step aside when a
  command names its own line budget and covered only `head` and `tail`, and
  `sed -n 60,200p` names its lines exactly as `tail -5` does. `sed` addresses that name
  line numbers now count, quoted or bare, single line or range. A pattern address
  (`/failed/p`) and a substitution do not: those filter, which is a different claim from
  naming the lines wanted. `awk` stays uncovered on purpose, because its line selection is
  an expression rather than a flag and the obvious guess matches `{print NR}`, which prints
  line numbers rather than selecting them.
- **The ledger folded the lines a multi-line command asked for (#750, #751)**: reading
  the line budget in command position (#742) read that position off the previous token,
  and `split_whitespace` has already eaten the newline, so `cd repo` followed by
  `sed -n '58,95p' src/app.tsx` on the next line was one long command and the guard was
  off. Replayed over the 4,193 commands recorded here, the narrowing gave up the guard on
  168 of them and 136 of those to a newline. The question is now asked per command rather
  than per reply, so a newline separates, a `sed` address cannot be read out of a later
  command, `sudo` and `env VAR=1` step aside, and a wrapper such as
  `docker exec app tail -5 app.log` keeps the lines it named.
- **A re-run was told its lines were shown, not that they were unchanged (#755)**: a poll
  run a second time asks whether the value moved, and when it has not, that is the answer.
  The run marker said `N lines already shown`, which puts the bytes behind a handle
  without saying they are this command's own output and identical to last time, so a
  reply whose surviving half was an unrelated table read as a complete result. The source
  clause already knew: #622 recorded a source only when it differed from the command being
  answered, so the case that most needs naming collapsed into the same absence as
  "nothing recorded". A same-command run now says `N lines identical to an earlier run`,
  and a different command still names itself.
- **The benchmark measured a ledger with its guards switched off (#760)**: `make bench`
  replayed the frozen corpus through a ledger that was never told which command produced
  the payload, so every rule that reads the command was inert in the one place this
  project measures itself: the `from` clause, the line budget that leaves a `tail -5`
  alone, and the wording a re-run gets. Found by measuring 0.7.8 against `main`, where
  every class came back byte-identical because three of the four changes in that range
  could not reach the harness. With it fixed, on the same corpus and the same commit, the
  ledger arm reads **3.0% where it published 4.9%**, and file reads read 1.5% where they
  published 4.3%. Only the arms that use our ledger move; the filters and the three
  competitor engines are unchanged. Artifacts now carry
  `harness.ledger_knows_the_command`, so a measurement taken before this is visibly not
  comparable with one taken after.

## [0.7.8] - 2026-08-26

### Added
**`make bench` measures a corpus that does not move.** Every benchmark figure OMNI
had published was computed from `execution_traces`, and `TRACE_RETENTION_DAYS` is
7. Two releases therefore reported numbers derived from two different corpora, and
the delta between them mixed a code change with a corpus change with no way to
tell which. That is why the headline drifted release to release and nobody could
explain it.

The corpus is frozen once and replayed from a file. `docs/benchmarks/<version>.json`
lands in the repo, so a release-over-release delta is finally attributable to the
code.

The payloads stay local. They are real command output, so `bench-corpus/` is
excluded from the repo and only the measurement plus a composition summary is
committed. That buys a figure that is stable and attributable; it does not buy
independent reproducibility, and the artifact says so by carrying the corpus hash
rather than pretending anyone else can re-run it.

Two things the first run found, both now checks rather than intentions. The replay
printed the database path while reading a frozen file, which is a provenance line
that lies. And a command class is the basename of whatever ran, so a locally named
script published its own name: the artifact names only classes above 0.5% of the
corpus and refuses to write at all if a name carries a redacted word.

### Changed
**`omni context --tokens` answers "where did my tokens go" from data OMNI already
records.** #612 came from someone saying "I thought it was just me, because of
polluted context". That is a guess they cannot check, and OMNI is the one thing in
the stack already holding the answer for the part it can see.

```
 Where the tokens went · last 30 days

   tool output OMNI saw       38.9 MB   18,449 calls
     distilled                 3.5 MB   49% of what it was given   1,146 calls
     folded                    1.8 MB   52% of what it folded      925 folds
     handed back untouched    31.6 MB   by design                  17,303 calls

   why it was handed back
     below guardrail           15,932 calls
     structured:yaml              731 calls
     ...
```

One line per engine, each percentage against its own base, because the ledger folds
payloads whose bytes are not inside the distiller's input: they may be summed and may
never share a denominator.

**What it deliberately does not report.** #612 asks first for the share of context
that is tool output against everything else. OMNI cannot answer that: it records what
passed through its hook and never sees the prompts, the system block, the tool
definitions or the assistant's own text. A denominator built from what OMNI happens
to hold would read as "share of your context" while meaning "share of the part OMNI
touched". The report says so in its closing line instead, and a test asserts that
line survives.

Read only, no new writes, and it is a CLI subcommand rather than an MCP tool so it
costs nothing to a session that does not run it, per #609. `engine_totals`,
`passthrough_reasons` and `filter_breakdown` already existed; the only store change
is `declined_bytes`, because the first draft printed what survived distillation as
what was handed back.
**One gate, two named questions.** `format::sniff` was asked the same thing at five
call sites, and each decided for itself what a `Some` meant:

```
src/hooks/pipe.rs:130        passthrough on structured
src/hooks/pipe.rs:355        ledger gate, pipe path
src/hooks/post_tool.rs:317   the all-or-nothing decline
src/hooks/post_tool.rs:425   ledger gate, fold_cross_turn
src/hooks/post_tool.rs:793   ledger gate, Bash path
```

Three of those five were the same ledger decision written out three times, twice in
one file. #452, #454 and #456 each fixed one copy of a duplicated predicate in this
codebase and left the other standing.

`refuses_lossy_stages` and `refuses_the_ledger` both answer `sniff(..).is_some()`
today, so nothing changes: the suite passes without a snapshot moving. The point is
that they are two questions rather than one. Folding a run of already-seen lines
leaves a marker and a recoverable handle rather than rewriting content, which is why
#608 proposes letting the ledger in where nothing downstream parses the payload
while lossy stages stay refused. That decision now lands as a one-line edit to one
function instead of three copies, one of which would be missed.

A test forbids the gate shape returning to the hooks. Calling `sniff` for its
`Structured` value is untouched and still happens, because the passthrough reason is
built from it.
**The benchmark reports what share of the available repetition OMNI captures, not
only what share of bytes it removed.** #704 stopped a figure moving when the code
changed. It did nothing about a figure moving when the *workload* changed, and that
turned out to be the larger source of the drift.

Replaying the frozen corpus against the table in the README, four of six classes
reproduce and two do not. One of the two is the hero line: file reads read 89.6%
saved on the corpus that number was taken from, and 4.5% here. Not a regression.
That corpus averaged 12.4 KB per file read against 2.1 KB here, the same large
files read again and again, so both numbers are true of their own week.

That is the problem with the metric rather than with either number. "89.6% off file
reads" reads as a property of OMNI and is a property of one week's work.

Two columns per class now, and the second is the one that holds still:

```
class              calls        input    filters   + ledger  available  captured
file read           1056      1891398       0.0%       4.5%      17.7%     25.3%
git                  899       858631       5.1%       8.8%      18.4%     20.8%
search               810       765443       3.4%       4.2%       6.5%     13.7%
aggregate           9478      8486830       1.4%       5.1%      15.6%     24.1%
```

`available` is the share of bytes whose line had already been delivered in that
scope, which is the ceiling: the ledger cannot fold what was never repeated.
`captured` is what it took of that ceiling. Savings swing about twentyfold across
these two corpora; capture stays between 10.5% and 25.3%.

Computed inside the replay with the replay's own class definition, on purpose. A
first estimate using a separate script put the ceiling at 6.3% and capture at 71%,
against 17.7% and 25.3% measured properly, because the two classifiers disagreed
about which traces were file reads. Two numbers from two classifiers is the
mistake, and the fix is one classifier rather than a footnote.

A ledger that claims more bytes than were repeated has a broken denominator, so
that is asserted rather than printed.
**`omni stats` says something now, and `omni stats --view detail` says it in one
voice.** Every number is the same number; this is layout and wording.

The default view was three engine lines and three footnotes, so the caveats
outweighed the content. It carries the two things a reader asks next: where the
bytes were, and what the hook cost in latency. Both were already queried for the
detail view and were only shown if you asked for a 58 line report.

What detail got wrong, and none of it was taste:

- **`By Command` was ranked by percentage.** Nine of its ten rows were commands that
  ran once, and the row carrying the most bytes was last. It is ranked by bytes and
  headed `where the bytes were`, which is the same defect and the same fix as #702
  in `omni context --tokens`.
- **Two columns shared the header `Saved`**, one holding a percentage and one bytes.
  They are `off` and `bytes`, and the byte column is sized from its rows, because a
  `-238.5 KB` overflowed a fixed six and pushed the bar right on that row alone.
- **The two views disagreed about vocabulary.** Detail said `Signal Ratio: 9.2%
  reduction`; default said `distilled 49%`. Same database, same window, different
  bases, and neither said which. Detail now reads `of what it saw 9.2% over every
  call, declined included`, and it prints the three per-engine lines from the same
  helper the default view uses, so a reworded label moves in both places or in
  neither. #665 fixed this inside one view and left the other alone.

  Worth recording how that was caught. The smoke check written for it searched only
  the default view while its name claimed both, and correcting it proved the detail
  view carried none of the three engine words. The claim was in this file before the
  code was.
- **The route list mixed two levels**, so `below guardrail 16,522` sat beside
  `Passthrough 94%` and read as a fifth route. The reasons are drawn as what they
  are, a breakdown of one row.
- **A three-line caveat sat inside a table.** The caveats qualify the whole report,
  so they are at the foot, once.
- Three capitalisation styles in one block, and `Session lifetime` was a heading, a
  row and a caveat for a fact that fits beside `latency`.

**It is 61 lines, not the "under 45" the issue asked for.** That target was written
before counting, and reaching it means deleting figures the issue also said to keep.
One candidate looked genuinely redundant, the `All Time` row against the header, and
it is not: on `--since week` the header reads 9,329 and `All Time` 19,131. They
coincide only while the database is younger than the window.

Checks in the smoke test, because `cargo test` never runs the binary's output: every
section heading survives a rewording, and both views still use the same word for the
same thing.

### Fixed
**`omni_run` folds across turns now, on the one tier where it is the only way the
model reads less.** The mechanism was never missing. `omni_run` distils through
`pipe::run_inner`, which has carried the ledger stage since #416, and that stage is
gated on a host session id. Only `omni exec --session` ever set one, so on an MCP
host the gate stood shut and nothing folded.

The id is minted per process and is deliberately not `SessionState::session_id`.
That one is a wall-clock stamp on globally persisted state, and `init_globals` hands
an MCP process whichever session was last written by anything, so scoping by it
would tell one agent it had already been shown output that went to another (#118).
An `omni --mcp` process serves one stdio client for its lifetime, so the process is
the session: it begins when the client connects and ends when it disconnects, which
is exactly the boundary a fold may reason across.

The fold test is Unix only, like its neighbour: its fixture needs a shell with `seq`
and `awk`, and on Windows it produced 175 bytes of error text twice, which is two
equal lengths and a red assertion for a reason unrelated to the ledger.

Two tests, because one cannot cover it. The first runs the same command twice
through the real entry point and asserts the second comes back shorter, which is the
only way to tell a working ledger from an absent one. The second reads `run`'s own
body, because `run` is an async loop over stdio that no unit test can drive, and the
fold test opens the gate itself so it passes with the wiring deleted. That gap was
the defect in miniature: the mechanism worked and nothing invoked it.
**The tier table stopped saying MCP-only hosts distil nothing.** #686 put `omni_run`
on that tier, so "Memory, recall and session state. No shell distillation, and no
claim of it." became false in the README and in the manual's agents page, both
languages.

The row now says what is actually true there: memory, recall and session state plus
`omni_run`, and the host's own tool output is never rewritten, which is why
`omni_run` is the only path by which the model reads less.

The sentence in `troubleshooting.md` was already precise enough to survive, because
it says a Handoff-first or MCP-only host cannot rewrite its *built-in shell tool's*
output. That is still exactly right.
**Rows in `omni context --tokens` line up again, and one call is one call.** The
class key can reach 18 columns and the report padded the name to 16, so any name
that overflowed pushed the bytes, the percentage and the count right on that row
alone. Two of five rows were wrong on the machine that found it.

```
     cd /Users/name/...   93.7 KB      4% removed  3,396 calls
     sed -n             30.4 KB      6% removed  215 calls
     journalctl -u      21.2 KB     75% removed  1 calls
```

The name is truncated to the width the report actually pads to, which is what
`omni stats` has always done with the same key. Counts read "1 call" now: six
lines of this report end in a count and a noun, and a short window holding one of
something is the ordinary case rather than the edge.
**The hero line promised that every number replays on your own history, and the one
number that did not was in it.** "89.6% off file reads across the corpus" was
measured on a trace corpus `TRACE_RETENTION_DAYS` deleted. Replayed against the
corpus frozen by #704, four of the six classes in that table reproduce and two do
not, and file reads read 4.5% where the copy said 89.6%.

Not a regression. That corpus averaged 12.4 KB per file read against 2.1 KB here,
so both numbers are true of their own week. The defect is in the metric: a corpus
aggregate reads as a property of OMNI and is a property of one week's work.

What the copy says now, on every surface and in all six translations:

- the mechanism claim stays, because it is the strong one. 97.2% off a file read
  twice reproduces on any machine, on demand
- the corpus figure names its corpus and its hash, and sits beside the capture rate,
  which is the share of the available repetition the ledger actually took. Savings
  move twentyfold between the two corpora; capture moves from 25.3% to about the
  same place
- the per-class table is generated from `docs/benchmarks/<version>.json` inside
  marked region comments, so it cannot drift from the measurement again
- the head-to-head is withdrawn rather than restated. Of four arms, `caveman` is not
  installed and `headroom` crashes (#711), and headroom is the only competitor
  shipping the same cross-turn dedup. Of the two that ran, rtk's filters plus our
  ledger took 5.8% against our 5.1%: on this corpus we are not the top arm, and the
  benchmarks page says so (#712)

The benchmarks page keeps every retired figure on purpose. It is an archive that
labels each run with its corpus, and deleting a published number to make a check
green is worse than labelling it. It is held to a different rule: it must name the
current corpus.

**Three tests, because prose is what rotted.** No claim surface may quote a retired
figure, in either decimal convention, and that second half is the point:
`README-id.md` and `-vi.md` write `89,6%`, so a scan for `89.6` called them clean
while they still carried it. That is #541 again, and it happened while writing this
check. 52 findings across 9 files at the start, 0 now.
**Every `omni stats` view now draws the same frame.** Five views drew three
different ones. The default and detail views ruled off above and below their
title; `--view context` still said "OMNI Signal Report: Context"; `--view
projects` and `--view rerun` drew no rule above at all, and `projects` folded the
period label into the middle of a sentence, "OMNI Project Analytics, last 30 days
Breakdown".

```
 OMNI · last 30 days
 OMNI · detail · last 30 days
 OMNI · projects · last 30 days
 OMNI · rerun · last 30 days
 OMNI · context
```

One `print_header` in `cli/mod.rs` prints all of them, so a sixth view cannot
quietly draw a sixth frame. `context` passes no scope because it reads the live
session rather than a window, and its labels now use the same lowercase
vocabulary as the detail view.

**The two blocks in the default view line up.** They sized their columns
independently, so the byte column of "where the bytes were" sat two characters
off the engine block directly above it. Both go through one `aligned_rows` call
now, and the test asserts the shared right edge rather than any one width.

```
    folded                2.0 MB   54% of what it folded       1,033 folds
    distilled             3.7 MB   49% of what it distilled    1,223 calls
    left alone                 0   by design                  18,847 calls

    where the bytes were
    npm run               1.8 MB    30% off                      153 calls
    cat .superpower...  238.5 KB    76% off                       25 calls
    cd /Users/fajar...  182.3 KB     4% off                    6,115 calls
```

**`--limit` reaches every table view.** It is documented as "Rows in a table view
(default 10, 0 for all)" and only `--view detail` read it, so `omni stats --view
projects --limit 2` printed 161 rows, the same as passing nothing. `projects` and
`rerun` read it now, through the one `row_limit` that also resolves `--limit 0`
and the older `--all-commands`, and a cut table says how many rows it hid.
**`omni stats --help` stopped offering two names for one view.** `--view commands`
and `--view detail` produced byte-identical output. `renderer` maps both to
`"detail"` and nothing downstream reads which was named, so the seven values the
help listed were six views and a synonym.

A comment in `run_detail` said otherwise: "`--view commands` keeps the ratio
ordering, because there the ratio is the subject." #714 moved that table to byte
ordering and its review moved `--all-commands` to match, on the grounds that one
table gets one ordering. The clause was what got left behind.

`commands` still works, undocumented, the way `-d` does. A test now walks the
values the help advertises and fails if two of them reach the same renderer.
**`--share` and `--card` honour `--since`, and say which window they are showing.**
Both accepted the flag and threw it away: `run` called them without `args` at all,
and `share_figures` hardcoded the window twice, reading `All Time` out of
`multi_period_stats` and asking `get_top_commands` for `since = 0`. So
`omni stats --since week --share` printed the all-time card, byte for byte. Same
shape as #670, which was the JSON report ignoring the four window flags.

```
OMNI saved me 3.9M bytes (9.1%) across 20,129 commands, all time.
OMNI saved me 421K bytes (5.4%) across 8,312 commands, last 7 days.
OMNI saved me 7K bytes (2.5%) across 294 commands, today.
```

No window named still means all time, because that is what a share card is for,
and `scope` cannot tell "no flag" from an explicit `--since month`. `named_scope`
answers that question and nothing else.

The line now names its window either way. A percentage whose population the
reader cannot see is what #665 was filed about, and this is the surface built for
posting in public.

The card's stat line and the width it is fitted to were two copies of one format
string. They are one string now, and the test drives the fitted size down by
lengthening the window, so a measurement that stops reading what it prints fails.

## [0.7.7] - 2026-08-24

### Changed
**OMNI's MCP surface is 23.1% smaller, with every tool still advertised.** The
tool definitions sit in the prefix of every request for the life of a session, so
each byte is re-read once per request rather than once. `schemars` was stamping
every parameter struct with two keys a caller never uses: `$schema`, a 56 byte URL
naming the JSON Schema draft, and `title`, which is the Rust struct name
(`OmniRunParams`). Over the nine tools that is 686 B of 2,972 B, now 2,286 B.

Nothing was removed to get it. No tool dropped, no description shortened, no
parameter changed, and `omni_retrieve` still parses its argument from the trimmed
schema. `title` was also leaking an internal type name onto the wire, which is not
something a caller should be reading.

Two tests hold it: one asserts neither key is advertised on either policy arm, the
other ratchets the total byte size so a schema growing back fails rather than
being noticed later.

This is the cheap half of #609. The expensive half stands: 37 of 260 sessions in
the measured corpus ever call an omni tool, so 86% still carry a surface they never
use, and `omni_context` has never been called once.
**`omni_context` is now `omni context`, and the MCP surface every request carries
is 8 tools instead of 9.** `mcp::policy::FULL` admits a tool on one stated rule,
"called at least once in the corpus". Re-measured over 253 recorded sessions,
`omni_context` was the only advertised tool with **zero** calls ever, and the three
places OMNI recommended it appear zero times in 10,578 traces, so this is not an
agent ignoring advice.

It cost 189 B in the prefix of every request on a Full-tier host. Measured through
the real server over stdio: **2,296 B to 2,098 B**.

Nothing is removed. `omni context <file>` prints the same report, costs nothing per
request, and an agent that wants it still has `omni_run("omni context <file>")`.
The two Guard strings that used to recommend the tool now name the command.

The surface gate is ratcheted from 2,500 to 2,200, just above what measures today,
because a bound with room in it does not notice a regression: the old 3,000 gate
stayed green through the whole 7,912 B era.

This still does not settle what #609 was filed for. 86% of sessions carry a surface
they never call, and that is a question about advertising 2,098 B rather than about
any one tool.

Review found two more while checking this. `omni context ./src/x.rs` and `omni
context src/x.rs` were two different keys: `normalize_path_string` only swapped
backslashes, so the graph answered "imports: none detected" for one spelling and a
hot-file lookup keyed on its result said "no" for a file the session had touched.
That was true for as long as the MCP tool existed and is fixed at the normaliser,
where every caller routes through. And the Guard string that suggests the command
now quotes the path, since a filename with a space in it was being pasted into
something a reader would run.
**A Full-tier host is now told about two tools instead of eight, and the surface it
carries in every request drops from 2,098 B to 467 B.** The rule that picked the set
was "called at least once in the corpus", and one call is not a price. Measured over
256 recorded sessions with `mcp_surface.py`:

| tool | bytes | calls | sessions |
| --- | ---: | ---: | ---: |
| `omni_retrieve` | 207 | 109 | 24 |
| `omni_explain_savings` | 260 | 29 | 27 |
| the other six | 1,631 | 11 | 7 |

The six cost 77.7% of the surface for 7.4% of the calls, in the prefix of every
request of every session, while 214 of the 256 sessions called nothing at all.

The cut is only on the tier whose shell OMNI already hooks, which is why it is
affordable there: `omni exec` runs what `omni_run` ran, `omni remember` writes what
`omni_remember` wrote, `omni stats --view context` prints what
`omni_context_breakdown` printed, and `omni stats --view detail` carries
`omni_history`'s rows with repeats folded into a count. `OMNI_MCP_TOOLS=all` puts the
tools themselves back. `omni_recall` and
`omni_find_noise` have no CLI equivalent and are behind the override alone. On a
Handoff-first host nothing changes: MCP is the only door OMNI has there, so all eight
stay advertised.

An unregistered host now takes the Handoff-first list rather than the Full one. The
guarantee that being wrong about a host costs a flag and not a capability did not
change; the list holding it did, because `FULL` is now priced for a host with a
hooked shell and an unrecognised id is exactly the case where nobody knows.

This is what #609 was filed to decide. What it does not do is take the surface to
zero for the 84% of sessions that never call a tool; that needs an opt-in default,
which would cost the other 16% their tools with no signal.
**Two guards for the property that makes OMNI cache-safe.** Prompt caching is
prefix-matched, so a tool that rewrites anything already in the conversation
invalidates every token after the edit. OMNI's answer is structural: the only
thing it can rewrite is the newest tool result.

The post-tool hook may emit exactly three keys and all three name the call it was
handed. A fourth is somebody having found a way to reach into the prefix, and the
test now fails the build saying so.

And an identical ledger sequence replayed against identical history has to produce
identical bytes. The ledger is the only stateful stage, so the same command is
rewritten differently depending on what earlier commands showed, and a prefix that
cannot be reproduced cannot be cached. The fixture asserts it reached the fold
before comparing, because three identical passthroughs would satisfy the
comparison while testing nothing.

No claim is published yet. The measurement that would support one is not settled;
see #610.
**The `string_slice` deny is live in every file again.** `src/lib.rs` has denied it
for a long time, and six files switched it off for their whole contents with a
module-level allow. That is where all 23 of #619's panicking cuts lived, including
one that crashed `infer_domain` outright, so the guard existed and was disabled in
exactly the places that needed it.

The 38 remaining slices are safe, and now say why one function at a time: each
index comes from `find` or `rfind` on an ASCII needle, with any offset added to it
being that needle's own byte length, or from a `char_indices` walk. A blanket
allow covers whatever gets written into a file next; a targeted one covers the
code someone actually looked at.

One was not safe. `parse_query` tested `to_lowercase().starts_with("context for ")`
and then sliced the **original** string at a hard-coded 12. Lowercasing can change
a string's byte length, so the two are not the same offset and the cut could land
inside a character. It now asks `get(..12)`, which answers `None` on a
non-boundary instead of panicking, so it needs no exemption at all.

`src/util/text.rs` keeps its file-level allow, which is the one place it is
honest: every function in it verifies a boundary before indexing, and that is the
file's whole job.
**A fold in the middle of a `Read` keeps the file's line count, so it may fold at all.**
The editor numbers whatever it is handed, counting from the line the read started at, so a
view with fewer lines than the payload puts every surviving line on a number it is not at
(#557). #658 worked around that by folding only down to the first line that survives, which
left everything below the first change on the table. A marker followed by one `⋮` per line
it replaced removes the constraint instead: the survivors keep their own numbers and no
starting number moves.

Measured over 20 markdown files read twice with no edit between the reads, 615 KB:

| | #658 | now |
| --- | --- | --- |
| corpus | 27.3% | **82.3%** |
| `CHANGELOG.md`, 434 KB | 4.4% | 76.5% |
| `CONTRIBUTING.md` | 8.3% | 84.9% |
| `docs/website/src/develop/index.md` | 7.8% | 84.0% |
| files that already folded whole | 95.4% to 99.6% | unchanged |

Every one of the 20 delivers a fold now; four of them delivered nothing before #658 and
little after it.

`⋮` rather than a blank or a tilde, because editors already read it as elided content, so a
reader who skips the marker above does not take it for a line of the file. It costs 4 bytes
against the ~50 a line of source runs to, and the run has to beat that filler as well as its
marker before it folds: a seen line arriving alone between new ones is left verbatim, where
before it could buy a marker it never paid for.

The padding is only used where the survivors would otherwise be split. A fold with nothing
under it, or one whose survivors are already contiguous, keeps its `startLine` bump and
emits no filler.
**`omni stats` reads both engines now, each percentage against its own base.**

The report computed everything from `distillations` and never read `ledger_folds`,
so the engine that removed the most bytes was absent. On the maintainer's machine,
over one week:

```
distiller   309,429 B removed   the whole of what the report showed
ledger      501,385 B removed   not mentioned on any line
```

The ledger took out 1.6 times what the distiller did and the report said 4.5%,
because that 4.5% was the distiller's saving divided by 15,718 calls OMNI had
deliberately declined. The default view now leads with the bytes and gives each
engine its own row:

```
    5.1 MB  never reached your model

    folded      1.7 MB   41% of what it folded         906 folds
    distilled   3.4 MB   48% of what it distilled    1,056 calls
    left alone       0   by design                  15,718 calls
```

The two percentages come from different populations and are never added: bytes are
summed, ratios are not. `left alone` reads `0` rather than the 31 MB that passed
through, because those bytes are neither a win nor a loss.

The fold figures count fold operations rather than `ledger_folds` rows: one fold
writes a row per (origin, source agent) pair and repeats the payload size on each,
so summing rows divided by the same payload several times and reported 31% where
the real figure is 41%.

`ledger_folds` gains a `fold_id` for this, because nothing in the table said which
rows belonged together and every way of inferring it merges some pair of folds.
Rows written before the column are backfilled from (second, session, agent, scope,
payload size), which is the best the surviving data supports, so a count over
history is a lower bound and its ratio a slight overstatement. Anything written
since is exact. It is taken over the folds that record a payload at
all, and says how many that was. `payload_bytes` arrived in a migration defaulting
to zero, so the rest carry a saving with no base to divide it by; where none do,
no percentage is printed at all.

`--json` gains an `engines` object naming the two separately, so nothing downstream
can average them.

Session lifetime, the per-period table, top commands and the agent split moved to
`--detail`, which also names why each call was declined out of `passthrough_events`.
That column has been recorded since #533 and nothing printed it, which left a 94%
passthrough share looking like an accusation instead of an explanation.
**`omni stats` has five flags instead of twelve.** Four of them selected one thing, the
window, under six names, and passing two took whichever branch was tested first. That
collapses into `--since hour|today|week|month|all`, and the view flags into
`--view summary|detail|commands|projects|context|rerun|share`, with `--limit` replacing
`--all-commands`.

`--json` and `--card` are output formats rather than views now. `--card` outranks
everything, since naming it can only mean writing the file, and `--json` outranks the view,
since there is one machine-readable report and it is not per view. Reading them as views is
how `--view detail --card` came to write no image at all. The decision is a table over
(view, json, card) with the argument wiring tested separately, because the mistakes there
are arguments that never arrive rather than rows that are wrong.

Every earlier spelling still resolves, absent from `--help` and from the manual: `--detail`,
`--today`, `--day`, `-d`, `--week`, `-w`, `--month`, `-m`, `--hour`, `-H`, `--all-commands`,
`--share`, `--project`, `--context` and `--rerun`. Nothing in a script breaks and no
deprecation notice is printed, because the rename is ours and not the caller's. `-d` reads
as detail and means day, which is why it keeps working and is never shown again.

A window flag on its own still selects the detail view, as it did through the old chain's
last branch, so `omni stats --week` prints what it always printed. `--since week` names a
window and nothing else, which is the reading the new flag is for.

A flag that takes a value no longer eats the flag behind it, and the command is refused
rather than run on a default. `omni stats --since --view detail` used to read `--view` as
the window, fall back to thirty days, and then let `--view` be found again by its own
resolver, so a malformed invocation succeeded and reported something else. It exits 1 with
the flag named. Treating the value as merely absent was the first attempt and was half a
fix: the report still ran.

The accepted names live in one list with a count of how many the help page shows, rather
than in a visible list and a hidden one. A second copy of a flag name is a copy that drifts,
and #452, #454 and #456 were each one half of a pair being fixed.
**The benchmark page publishes two runs, and says why they disagree by a factor of
fifteen.** Every figure on it came from one replay of one week, 2026-08-11 to 08-14, and
`execution_traces` prunes at seven days, so that corpus is gone and nobody can re-derive
those numbers, us included. Re-running the same harness on 2026-08-23 over the following
week reads **1.0% from the filters and 4.6% with the ledger**, against 32.6% and 69.6%.

Neither figure is wrong and the gap is the useful part: that week was 80.6% duplicate bytes
from a fortnight of building OMNI, and this one is shell plumbing, 4,598 of 8,934 calls
beginning with `cd` and a median repeated run of 10 bytes. Both runs are on the page with
their corpus lines, the older one labelled as unreproducible rather than deleted.

Two things the page now says that it did not. The harness replays `execution_traces`, which
holds shell commands only, so **no figure on it covers the ledger's file-read path**,
including the count-preserving fold from #664. And the method snapshots the database before
replaying, since hooks write to it while the replay reads.

The README and both index pages keep the figures they published and gain one line saying
those traces have since been pruned, so that window cannot be replayed. The percentages
there do not move: the later run is lower because its week barely repeats itself, not
because anything got worse, and swapping one week's corpus for another's on a marketing
surface would say something the measurement does not.
**`omni doctor`'s agent list is one shape in three columns, and the offer to see the
passing checks is printed once instead of once per host.** Fourteen hosts rendered as
twenty lines in two different shapes: a clean host was `Claude Code: Full, 5 checks
[OK]` and a host with anything to say became a block, so name, tier and count started
at a different column on every row. `omni doctor --detail to see them` appeared three
times for one fact.

```
  Claude Code      Full           5 checks [OK]
  Cline            MCP-only       1 check  [OK]
    Distill:        MCP tier: no per-tool hook on this host
  Pi Agent         MCP-only
    Config:         not configured
```

The name now comes from `AgentIntegration::name()` rather than the report's own
heading, which carries colour escapes that no padding can measure. Two hosts printed
one name here and another in `init` and `reset`; they agree now, so the list says
`Cline` and `Pi Agent`.

A host with no passing check shows no count rather than `0 checks [OK]`, which reads
as a failure when it means "nothing configured to check".

The tier legend was four lines because one sentence was hand-wrapped onto a
continuation indented to nothing above it. It is three aligned rows now, and the
sentences moved to `Tier::label()`, which existed with no caller at all while `doctor`
kept its own copy.

Nothing is hidden that was not hidden before, no row that is not `[OK]` stops
printing in either mode, and `--json` is untouched.
**The one tier that cannot hook a shell now gets the tool that runs one.** An
MCP-only host gave OMNI no hook, so nothing was ever distilled there, and
`omni_run` was withheld with this reason:

```rust
/// No shell distillation exists on this tier, so the tools that report on it
/// would describe something that never happens.
```

That is right about `omni_explain_savings` and the other three reporters. It was
circular about `omni_run`, which does not report distillation, it performs it: the
tier had no distillation partly because the only tool that could produce any was
held back on the grounds that the tier had no distillation.

The file already made the argument one tier up, for Handoff-first, and every word
of it applies: MCP is the only door OMNI has there, and an unadvertised tool is not
one shell command away, it is unreachable. The difference between the two tiers is
an installed rule nudging the agent toward `omni_run`, which is a nudge and not a
capability.

Only `omni_run`. It costs 417 B in the prefix of every request; the four reporting
tools are 803 B more, and this file's own rule is that a tool added without a
measurement has no price behind it. They become arguable once there are recorded
runs on this tier to report on.

The tier's own description moves with it. `Tier::label()` said "no shell distill",
which #684 had just started printing at install time, so two changes in this release
would have contradicted each other. A test now checks the label against the tool set
rather than trusting it: a tier offering `omni_run` may not claim it does not distil.
The manual's tool table moves too, in both languages.

Cross-turn folding still does not reach these hosts, because the ledger runs in the
hooks and `omni_run` never builds one. That half of #686 stays open: it needs a
session identity MCP does not carry, and inventing one is how #118 happened.
**The README and manual stopped leading with a number that talks a reader out of
the tool.** Four user-facing places opened with "97.3% of calls saved nothing at
all" and one with 96.1%. Every word was true and it was the wrong sentence to put
first: it averages the payloads OMNI deliberately declines, the small ones, the
structured ones, the enumerations whose rows are the answer, into the calls that
win, and reports the result as performance. A reader takes it as "this tool does
nothing", which the table two lines below disproves.

This project polices overstating and had no rule against understating, so the copy
had drifted into apologetics: "we publish that because it tells you what the rest
are worth", "it does nothing most of the time and a great deal occasionally".
Understating is a different way of being wrong about your own numbers.

The replacement leads with the class where the bytes are, using figures already
verified with their corpus and window rather than any new arithmetic: file
re-reads are the largest class and the ledger takes 89.6% off them, a file read
twice comes back 97.2% smaller, and the restraint is stated as the guarantee it is,
nothing deleted, nothing invented, no call comes back larger. The aggregate stays,
as the floor of what the tool does rather than the headline.

Every unflattering figure, 96.1% included, stays on `develop/benchmarks.md`, which
is where an auditor looks. The mechanism sentences in `how-it-decides` and
`pipeline` that say a block is recorded even when the projection saved nothing are
untouched: those describe behaviour, not performance.

Seven READMEs and both manual trees, English and Indonesian.
**The README published a corpus the manual replaced two releases ago.** `README.md`
and its six translations carried 6,656 traces replayed on 0.7.3 while
`docs/website/src/develop/benchmarks.md` carried 5,984 replayed on 0.7.5, so the
repository disagreed with itself on every figure the two shared: 2.7% against 32.6%
from the filters, 14.9% against 69.6% with the ledger, and 35.9% against 98.9% on the
`docker build` fixture.

Two of those were reversed claims rather than stale numbers. The head-to-head conceded
the filter row to rtk, 6.2% against our 2.7%, and on this corpus ours read 32.6% while
the row we actually lose is lean-ctx at 49.4%. And "file re-reads: 0.0% from the
filters" was the argument for the ledger; that class reads 39.2% from the filters now,
so the argument is made from the gap that remains.

Every README also carries the manual's own caveat next to the headline, because a
number travels and the sentence qualifying it does not: 286 groups of byte-identical
payloads are 80.6% of these bytes, and the same harness over a week of ordinary work
reads 14.9%. The six translations additionally lose a marker-accounting paragraph the
English README dropped in #325 and they kept, which cited a `git diff` figure the table
had already stopped printing.

### Fixed
**The release script builds the binary it smoke-tests.** `scripts/omni-release.sh`
ran `tests/smoke_test.sh ./target/release/omni` and never built that file, so the
last gate before a tag validated whatever binary happened to be on disk. Cutting
0.7.6 stopped at `Results: 44/46 passed, 2 failed` against a `target/release/omni`
two days old, minutes after `make binary-check` passed 46/46 against a freshly
built `target/ci/omni`. The two gates disagreed because they test different files
and only one of them built what it tested.

Failing loudly was the lucky direction. The same staleness passes silently
whenever the old binary still satisfies the checks, and it does today: the
two-day-old 0.7.6 binary in this tree passes 46 of 46. A gate that can go green on
a build nobody made is not a gate. `smoke_test.sh` also falls back to
`./target/debug/omni` when the release path is missing, so a machine that has
never built release validated a debug binary and said nothing about it; building
first removes that fallback's ability to fire during a release.
**The `Read` skeleton stopped advertising a recovery route that cannot work.** It
ended `Re-read with offset/limit for the full file.`, and a re-read is a fresh
`Read` of the same file, so it reaches the same distiller and returns the same
skeleton. Following the instruction verbatim on a 303 line file returned the
identical summary a second time, and a third identical request folded to a single
ledger marker with no content at all.

The route that does work was already one line below it, and every `omni retrieve`
handle in the report resolved. So the note now counts what was dropped and names
nothing, because the handle is minted after that string is built and cannot be
reached from it.

Its number also read as a contradiction next to that marker and was not one. It
counts file lines the skeleton does not render, while the marker counts reply
lines cut from what was about to be sent, so the two differ legitimately on the
same read. Both stay, each now saying which denominator it used. The note is 41
bytes shorter as a result, which is worth almost nothing at 8 distilled reads a
week and is not the reason for the change.
**A small fold that would leave the reader nothing to read is refused.**
Resolving a merge conflict and re-reading the same window folded 23 of 26 lines
and delivered three that had nothing to do with the edit, under a marker reading
`23 lines already shown`. An agent verifying a deletion reads that as nothing
changed. The fold was right about every line it counted: a deletion and a
reorder produce no new line, so there was nothing for it to emit, and absence is
the one thing a set keyed on line hashes cannot represent.

`MIN_WHOLE_OUTPUT_FOLD` already refuses this trade when a fold covers the payload
exactly, on the argument that a handle and nothing else is a round trip the agent
has no say in. Three surviving lines out of twenty-six is that same situation, so
the floor now covers any call whose folds take four fifths or more of the payload
between them. Coverage is asked once for the payload, not once per run: review
caught the first draft weighing each run against the whole reply, which let three
seen blocks of thirty percent, separated by a new line each, fold ninety percent
of a small payload and leave the reader three markers and two separators. The
wording does not move: a partial fold still says `N lines already shown`, because
it still is one.

Priced before the fraction was chosen, and the first price was wrong in the way
this project keeps warning about. `ledger_folds.payload_bytes` only exists since
#560, so 41 of the recorded calls carry it and the rest read zero. Dividing the
refused bytes by the whole table's total gave 0.12%, a figure whose numerator and
denominator came from different populations. Against the 41 calls that can
actually answer, the guard refuses 3 of them and 1,748 bytes of 54,400, or 3.2%
of what the ledger folded in that window. The all-time figure cannot be computed
and is not quoted.

It still cannot reach a large payload, where a 90% fold is the case the feature
exists for and the floor is met many times over.

Three cheaper designs were measured and dropped. Reporting `N removed` in the
marker needs the previous payload, which no table stores. Ordering or gap
detection over `ledger_lines` cannot tell a deletion from `grep` printing three
matching lines out of a file, which is most of what the ledger folds. Keying the
previous run by command needs a per-hook lookup and a column, against a 10 ms
budget, to catch what a floor already covers.
**`omni_explain_savings` can see cross-turn folds.** It read the `distillations`
table; the ledger writes to `ledger_folds`, and nothing had ever read that one. So
a session whose compression was entirely folding answered "No recent distillations
found in current session" while its own markers carried live retrieve handles. On
the machine that confirmed it: 936 folds recorded, none visible to the tool.

That was the worst case to be blind to. This is the tool the docs point at for
judging whether a route paid for itself, and folding is exactly where a marker plus
a 16 character handle can cost more than the lines it replaced. It also made a
defect report harder to write, because route and byte counts had to be guessed
rather than quoted.

The report now carries both, and only says nothing happened when both are empty.

The filter that never matched came first. `SessionState` mints its own id every
time it is constructed, `{millis}-{pid}-{counter}`, while the hook keys its rows on
the id the **host** supplied: 11,281 rows here carry a host UUID against 892 minted
ones. The MCP server cannot see the host id, so filtering on the one it holds
matched nothing, and that was true of the distillations half too, before any of
this touched it. Both tools now ask by recency and say so.

Folds record which session made them, because the table could not say. Its `scope`
column holds the **project** whenever there is one, deliberately, so the
cross-agent question it was built for stays askable, and every one of those 936
rows carries a filesystem path there. Asking it for a session id returns nothing,
which is what the first attempt at this fix did.

Folds are reported in lines and bytes with their scope, and without a command: the
ledger keys lines by scope and hash, so the bytes a fold replaced may have been
shown by a different command than the one being answered, and naming the current
one would be a guess dressed as attribution.
**A recorded row is named after its program again, and this time in both of the
places that write the name.** #339 taught `sole_output_command` to strip a
`VAR=value` prefix and closed. Routing was fixed and `distillations.filter_name`
was not, because neither writer of that column went through there: `hooks::pipe`
took the first token of the raw command and had never received the fix at all,
and `hooks::post_tool` reached it through `.unwrap_or(clean_command)`, which
hands the raw chain straight back for any command with two producers.

Measured on 0.7.6 on a database whose oldest row postdates #339: 1,525 of 11,335
rows were filed under an assignment and 291 under a binary's full path, 16.0% of
the corpus, against the 1,079 rows #339 measured before it was closed. So the
rate had not moved. Every aggregate keyed on that column was wrong by that much,
and those aggregates are what a distiller gets sized from here.

One `producer_label` both doors call, with `resolve_profile`'s private copy of
the basename logic folded in beside it. Writing the test as a matrix of command
shapes rather than one case immediately found a second defect in the new
fallback, which was naming a chain after its `cd` instead of after the first
segment that produces output.
**Rows written before the label fix are corrected rather than waited out.** #603
fixed `filter_name` going forward and left everything already recorded, so 2,892
of 17,402 rows on the machine that measured it, 16.6% across 398 distinct names,
were still filed under an assignment or a binary's full path. They kept
accumulating until this release was *installed*, because merging a fix does not
change the binary on disk.

A migration recomputes them from the command each row still carries.
`producer_label` is a pure function and every stale row kept its command, so this
is arithmetic rather than a judgement about which rows are real, which is what
separates it from #163: there the truth was unrecoverable and the answer was to
exclude by timestamp and delete nothing. It needs no timestamp constant either,
because recomputing a row written after the fix returns that row's own label.

A row that lost its command is left exactly as it is. The migration runs once,
records itself in `schema_migrations`, and is its own fixed point if it runs again.
**The `git diff` summary is measured from the diff, and a decorative hunk no
longer disappears under it.** The `git diff: N files changed, X+, Y-` line counted
the `+` and `-` lines that survived the scorer rather than the ones in the payload
it replaces, so it reported a diffstat for the filtered output while reading as a
diffstat for the commit.

One tier turns that into a false claim on its own. `is_blank_or_decorative` tiers
a block whose lines carry only `-`, `=`, `*` or `_` as Noise, and a hunk removing
a rule of `=====` characters is exactly that once each line carries its `-`
marker. The whole segment was skipped before the emit and before the count, so a
diff adding one line and removing five came back as
`git diff: 1 files changed, 0+, 0-` with no hunk under it. A reader concludes the
commit changed nothing.

The counts now come from the payload, and position decides what counts rather
than the leading character: `+` and `-` lines inside a hunk, nothing outside one.
That was the other half of the same defect. `git show --format=%B -p` prints the
commit body unindented, so a body line opening with `-` read as a removal and a
60+/60- diff came out as `61+, 62-`. Position also settles a case a prefix test
gets wrong in the opposite direction: a removed line whose own text starts with
`--` renders as `--- …`, so excluding `---` dropped a real removal from the
output while the summary still counted it. Shown and counted now follow one rule.

A tier may only drop a segment that carries no hunk. The per-line filter below it
already decided what a hunk contributes.

Worth saying because the report that found this described a different mechanism:
its own `git show … | sed … | head -40` had cut the removal hunk before OMNI saw
it, and the `1+, 0-` in that transcript was true about every byte OMNI received.
The defect above was found while failing to reproduce that one.
**Cuts at a byte index no longer panic on non-ASCII text.** `String::truncate` and
a `&s[..n]` slice both panic when the index lands inside a multi-byte character,
and twenty-three cuts took a fixed or length-clamped index on text OMNI does not
control: error messages, recorded commands, session payloads, task and goal
strings. Two sat in the MCP server, where the panic takes down a tool call, and
`CONTRIBUTING.md` requires a hook never to crash its host.

They now go through the `util::text` helpers that were already in the tree and
already doing this correctly, so an accented path or a CJK identifier costs a
character off a preview rather than the process.

One of them was reachable rather than theoretical. `infer_domain` walks byte
indices to find the common prefix of two directories and sliced at every one of
them, so two hot files under a directory whose name carries any multi-byte
character panicked outright.

`src/lib.rs` has denied `clippy::string_slice` all along, and thirteen files
switched it off wholesale with a module-level allow, which is where every one of
these lived. Six of those files no longer need the exemption. `infer_domain` keeps
a function-scoped one that says which three slices are proven safe and why,
because a blanket allow is what hid the wrong one.
**A fold says when it is replacing bytes from a different command.** The ledger
keyed shown lines by scope and hash alone, so reading file B could have a block
elided because file A showed an identical one earlier, and the marker named
neither file nor line range. Comparing two files to check a shared block matches
is exactly that case, and the dedup answered it by deleting the evidence.

Lines now record the command that first showed them, and a marker carries
`from <source>` when the fold draws on a different one:
`[OMNI: 22 lines already shown from charlie.tf, omni retrieve …]`.

The clause is absent when the source is the same, which is the common case of
re-reading one file, and that is deliberate rather than an optimisation. Marker
length gates folding: the gate weighs the rendered string, so every byte added
here is a byte a run must beat before folding is worth it, and trimming this
marker by 22 bytes was worth 0.3 points of aggregate savings on its own (#450). A
clause on every fold would have paid that back the wrong way. The decision lives
in the run's grouping key, so a stretch whose lines came from two commands now
splits into two runs rather than picking one of them to name.

A source is a raw command, and 46.9% of recorded commands carry a newline, so the
marker takes the first line only with control characters dropped and whitespace
collapsed. A marker is a single line and the count of markers above the first
surviving line is derived from that, so a command able to split it would corrupt
both the framing and that accounting.

Rows written before this keep an empty source, which reads as unrecorded and
suppresses the clause rather than guessing. The column is also what makes the
frequency measurable: nothing in the corpus could say how often a fold draws on a
different source, because there was nowhere to record it.
**`omni retrieve` framed one block of an output as the whole output.** A reader
followed a `[OMNI: 10 lines already shown, omni retrieve …]` marker and got ten
lines under `10 lines · 227 B`, for a command that had produced fourteen. Nothing
was lost: the other four were on screen, and a ledger fold archives the run it
replaced rather than the reply. The frame is what was wrong, and it reads as a
measurement of the whole, so a short file listing looked complete and the reader
nearly designed a dependency layering off it.

`rewind_store.original_len` was written as `content.len()` on every insert, so the
one column that could have answered "is this all of it" duplicated the content
length instead. It now carries the size of the payload the block was cut from, and
`omni retrieve` says `· one block of a 797 B output` when the two differ. A handle
that does hold an entire output keeps the short frame, so nothing is added to the
common case.

Rows written before this ship with the old value and read as whole; they age out
with the 30 day retention window. The MCP `omni_retrieve` tool is deliberately
unchanged: its result is a single string the agent may paste or parse, and the CLI
keeps its frame on stderr for the same reason.
**The Hermes plugin was a Python syntax error and had never loaded.** It lived in
a Rust raw string that opened with five quote characters, so every `__init__.py`
`omni init --hermes` ever wrote failed to parse at line 32. The install printed
`✓ Installed Hermes plugin` and the host has produced zero rows since 2026-08-11.
Nothing caught it because nothing could: a Rust string literal is not Python to
any tool in this repo.

Even with the quoting fixed the three handlers would not have run. Hermes calls a
hook as `cb(**kwargs)` and passes thirteen keys, so `def on_post_tool_call(tool_name,
params, result)` raises TypeError on every call and `invoke_hook` swallows it into
a debug log.

The plugin is now a real file at `plugins/hermes/__init__.py`, compiled and
exercised by a `checks (plugins/hermes)` CI job, and it registers the two hooks
whose return value Hermes actually uses: `transform_terminal_output` and
`transform_tool_result`. The second fires for **every** tool, which makes the
Read, Grep and WebFetch distillers reachable for the first time; they have been
written since #172 and never executed, because Claude Code's PostToolUse matcher
is Bash-only.

Hermes also gets its own payload format rather than borrowing one. `OMNI_AGENT_ID`
can only override a Claude-Code-shaped payload, so reusing Codex's shape would
have filed every Hermes row under `codex_cli`, and Claude Code's shape has nowhere
to put an exit code, which is what #120 needs to refuse a failed command. Hermes'
tool names are translated to the arms that exist: `terminal` used to fall into the
unhandled-tool bucket, so even a loading plugin would have distilled nothing.

Measured end to end through the installed plugin, driven with the kwargs Hermes
really passes: 790 B to 283 B, two `hermes` rows in `distillations` and one ledger
fold, on a host whose lifetime total was zero. A failing command is left alone.

**OpenClaw's plugin was never loaded either, and the reason was one line of
config.** OpenClaw does not scan `~/.openclaw/plugins/`. A directory is loaded
only when `plugins.load.paths` names it, and `omni init --openclaw` copied files
in and printed a tick without touching the config, so `openclaw plugins list`
never showed the plugin at all. The install writes that key now, preserving
everything else in a file that holds the user's channels and credentials, and it
is idempotent across upgrades. `omni doctor` checks the key rather than the files
and warns when the plugin is on disk but unreachable.

Behind that sat two more defects, each enough on its own. `api.registerHook` and
`api.on` are different buses: the first files a handler in the internal hook
registry and wraps it in an `async` function, while `runToolResultPersist` reads
`registry.typedHooks`, which only `api.on` writes, and discards anything that
returns a Promise. Registering the right hook name on the wrong bus loads clean,
reports no error and rewrites nothing. And the plugin's only tool shelled
`omni exec -- <cmd>`, the argument form that fails with `No such file or
directory`.

The tool names came from a real turn rather than from the bundle, after the
bundle's names turned out to be wrong: it has a tool literally called `bash` and
the name arriving at the hook is `exec`. Mapping the bundle names left every
shell result unrecognised, which distils through the generic path and files no
`distillations` row.

`tool_result_persist` rewrites what OpenClaw persists to its session transcript,
not the payload of the turn that produced the output. So the first read of a
command is raw and every replay after it is distilled, which is where a tool
result is re-read a median of 129 times.

Measured against a real OpenClaw 2026.7.1-2 with an OpenAI-compatible stub in
place of a model, over three turns of one session: 29,442 B on the first turn,
7,374 B with an OMNI marker on the second and third, and one `openclaw` row in
`distillations` on a host whose lifetime total was zero. `plugins/openclaw` also
gets a `typecheck (plugins/openclaw)` CI job, which is what caught a TypeBox
object where the host wants a JSON Schema, a tool cast to a factory, and an errno
string typed as a number.
**`~/.omni` and the database were left at whatever the umask gave them.** Nothing
in the tree ever called `set_permissions`, so a default umask of 022 produced a
0755 directory holding a 0644 database: on the machine that found it, 147 MB of
recorded command output readable by every other local account. Redaction does not
cover this and is not meant to, since the archive keeps the original on purpose so
`omni retrieve` can return it.

The directory is now 0700 and the database, including its `-wal` and `-shm`
sidecars, is 0600. Set explicitly rather than left to the umask, and applied on
every open, so an existing install is tightened on the next run instead of keeping
the modes it already has.

A directory named by `OMNI_DB_PATH` is left alone, because it can be any directory
on the machine and narrowing one the user chose would take access away from
whoever else is in it. The database file itself is tightened wherever it lives.
Windows has no umask or mode bits in this sense, so that path is unchanged.
**Every workflow action was pinned to a mutable ref, so a compromise upstream
would have been published under our name.** `dtolnay/rust-toolchain@master` is a
branch, and a tag like `actions/checkout@v7` can be repointed at will. That is the
`tj-actions/changed-files` shape, and `release.yml` is what builds the binaries
Homebrew then distributes. All 22 `uses:` lines across the workflows are full
commit SHAs now, with the version in a trailing comment so Dependabot still opens
the bump PRs.

Pinning `dtolnay/rust-toolchain` is not a pure rename: that action chooses the
toolchain from its own ref, and at the pinned commit `toolchain` is a **required
input**, so a SHA with no input fails the build outright. Every call names
`1.97.0` now, which is what `rust-toolchain.toml` pins anyway. `release.yml` used
to ask for `stable` and then have rustup switch to the pinned channel underneath
it, which is the mismatch that produced no binaries at all for v0.6.2 (#179).

**Releases carry build provenance.** `SHA256SUMS` proves a file has not changed
since publication and says nothing about who published it, which is the question
that matters for a binary in the hook path of every command. The release job
attests its archives, and `SECURITY.md` documents the one-line
`gh attestation verify` a user runs. Releases from 0.7.7 onward are attested.

**`SECURITY.md` told reporters to open a public issue**, listed as "Recommended",
and claimed 0.5.x was the supported line while 0.7.6 was out. A reader taking it
at its word would publish a working exploit against a tool sitting in front of
every shell command, with no embargo and no release to upgrade to. Private
reporting is first and the public-issue route is gone; the table names 0.7.x.
**A failing `terraform validate` was rendered as a clean plan.** The terraform
summariser counted a resource for any line containing `will be created`, and
terraform's own error prose contains it: "so if this file will be created by a
resource in this configuration". A validate that exited 1 came back as
`terraform: +1 ~0 -0 resources` with `+ configuration` under it, the `configuration`
being the first word of the next wrapped line. That is the shape of a clean plan,
nothing in it says the command failed, and the reporter read it as "validate
passed, one resource to add" and moved on.

Plan lines are now matched only on terraform's resource-address comment
(`  # aws_instance.web will be created`), and the `Plan:` and `Apply complete!`
summaries only at the start of a line. A real plan distills exactly as before, and
the fixture proves it: `+2 ~1 -1` with all four addresses.

The guard that should have caught this is #120, which refuses to distill a command
that exited non-zero. It was blind here because the reported command ended in
`| tail -4`, so the exit code the host reports belongs to `tail`. A summariser
cannot rely on the exit code reaching it, and has to be anchored to syntax the
output it claims to read actually uses.
**A command that wrote a newline to stderr was announced as having written to
stderr.** The reader got a `[stderr]` header with nothing under it, and on a
failing command that header is the first thing they go looking at. The guard was
`!stderr.is_empty()`, which `"\n"` and `"   "` both pass.

Both payload shapes built that section and both had the same wrong predicate, so
they are one function now. #452, #454 and #456 were each one copy of a pair being
fixed while the other kept the bug, and this is the same pair.

Trimming decides whether there is content, never what it is: a stream with
something in it is still appended byte for byte, so indentation a compiler emitted
survives.
**`omni reset` had no `--openclaw` flag**, so the OpenClaw integration could only
be removed by uninstalling every other one with `--all`. Checking the registry
against the flag table while fixing it found `--vscode` missing too, which the
report had not noticed: it compared only the first-party integrations and missed
the MCP hosts.

Both flags exist now, in the table, the interactive menu and the argument parse.

The real fault is that the host list was written out four times in one file, as a
boolean, a term of a fourteen-way condition, a menu arm and a push. It is one
table now, derived from the flag table itself, and `omni reset` is 13 lines
shorter for it.

A test walks `all_integrations()` and fails on any id its own flag does not
select. It drives the real routing rather than comparing flag names, after review
pointed out that a name-only check passes while a flag selects nothing, which is
#143's guard answering a question nobody asked all over again.

Review also caught that `check_flags` accepts `--flag=value` and validates the
name alone, so `omni reset --openclaw=1` was accepted, selected nothing and fell
into the interactive menu with the plugin still installed. `cli::flag_name` is the
one place that rule lives now and `reset` routes through it. Twelve other modules
still compare the whole argument; that is #646.
**A 527 byte `grep` came back with no content lines at all.** Fourteen lines were
folded into one project-scope handle and the four lines left standing were shell
scaffolding: two `echo` headers, a version string and an `ls -l`. The command had
been run to check where a new export had landed in a sorted list, so the sort
order was the whole answer, and the reply read as "no matches" rather than
"matches withheld".

The sub-1 KB coverage bar was four fifths. This reply was 78.7% covered, which
cleared it, and the payload was not folded whole so the #567 whole-output floor
did not apply either. Both guards were built for this exact case and neither
reached it.

The bar is three quarters now. The number is not derived from anything: the
fixtures that must still fold sit at 72.1% and 63.9%, this report must not at
78.7%, and three quarters is the only simple fraction in the gap. Priced over the
recorded corpus it refuses 9 more of 108 sub-1 KB calls and gives up 3,768 bytes
across five days, against a 13.3% retrieve rate on blocks that size.

Reproduced from the row the report wrote (`payload_bytes` 695, one project run of
527 B over 14 lines, `whole_output` 0) rather than from the prose, after a
synthetic repro at 100% coverage came back clean on 0.7.6 and on `main` alike.
**`--flag=value` was accepted and then ignored, in every command.** `check_flags`
validates that form on the flag name alone, and all sixty call sites compared the
whole argument, so the flag passed validation and the command behaved as if it
were absent.

For a boolean flag that means the wrong mode: `omni version --json=1` printed the
human banner, and `omni init --openclaw=1` exited 0 having installed nothing.

For a flag that takes a value it is worse, because the command succeeds and
answers about something else:

```
$ omni dashboard --port=18877
  OMNI dashboard http://127.0.0.1:7717     # before
  OMNI dashboard http://127.0.0.1:18877    # after
```

`omni patterns --tool=bash` filtered on nothing the same way.

Two helpers now hold the rule, `has_flag` and `flag_value`, and every command uses
them. `flag_value` reads both `--port 8080` and `--port=8080`, and splits on the
first `=` only, so a value containing one survives.

A test scans `src/cli/` and fails on any argument compared to a flag directly.
Sixty sites across twelve commands is what it took to notice, and each was correct
on its own and wrong against `check_flags`, so the guard is that nobody writes the
raw form again. It found one leftover while being written.

Review found two the first pass missed, both now covered. `stats` kept its own
matcher for the windows that grew alternative spellings (`--day` / `--today`), and
because it compared against a parameter rather than a flag literal the source scan
could not see it, so `--hour=1` still selected the default overview. That copy is
deleted and the shared `has_any` replaces it.

And routing the positional word `help` through `has_flag` widened it: `help=x`
started triggering the help page. `help` is now matched exactly and only as a
subcommand's first argument, which also fixes something older than this change,
`omni query how do i get help` printed the help page instead of answering.
**The distiller dropped the line a bundler marked as the one that threw, and kept
the unmarked source around it.** In a compiler frame the leading `>` and the `^`
under it are the entire answer, and they carried no failure signal at all, so they
tiered as ordinary context while lines 49, 51 and 53 survived. The reader was left
with five lines of source, none of which is the one that failed, and the
`12 lines omitted` label gave no hint that the marked line was among them.

Two shapes are Critical now, kept narrow because a leading `>` is also a markdown
quote and a shell redirection, and tiering those Critical would stop a document
being distilled at all:

* `> 50 |  throw ...`, which needs the `|` gutter after the marker to tell it
  apart from quoted prose.
* `|      ^^^^`, a row of nothing but gutter, carets, tildes and space.

`> Build error occurred` is prose with no colon for the `error:` rule to catch, so
`build error` joins the generic markers where it belongs.

Same shape as #425 one layer up, where a runner's verdict glyph tiered as context
and the distiller kept three passing tests while dropping the failing one.

The report's second finding, `omni_explain_savings` answering "No recent
distillations found in current session", is already fixed on `main` by #625 and
only waiting on a release: v0.7.6 still carries the old wording.
**A blanket `*.py` ignore had kept an announced file out of every release.** The
0.5.2 changelog says `scripts/seed_marketing.py` was added; `.gitignore` ignores
every Python file outside `plugins/`, so it has never been in the repository and
no user has ever had it. That changelog line is corrected in place rather than
quietly dropped.

`scripts/*.py` is exempt now, because that directory is tooling a reader is told
to run. The same rule was about to swallow a reproduction script the README
pointed at, which would have shipped an instruction nobody could follow.

The seeder itself is archived rather than committed. It opens `~/.omni/omni.db`,
runs `DELETE FROM distillations`, and refills it with randomised rows under a
comment reading "make it look real". That database is where every published figure
here is measured from, so the script fabricates the evidence this project exists to
keep honest. It also names a `content_type` column that no longer exists, which is
the only reason it has not already done the damage.

A test walks README, `CONTRIBUTING.md` and both manuals, and fails on any
`scripts/...` path that is not in the repository. It also refuses to pass by not
looking: a manual directory that moved, or a document it cannot read, fails rather
than being skipped. `CHANGELOG.md` is exempt: history legitimately names files
that were removed later, which is a different thing from a live instruction naming
a file that never existed.
**A log's failure lines were tiered as ordinary context and dropped, while a
hundred routine lines were kept.** `is_critical` matched `error:` and a line
starting `error `, which is how a tool writes a one-line message and not how a log
writes anything. A timestamped line, a bracketed level and logfmt were all missed,
so `GenericDistiller` selected the routine lines first and discarded the failures
with the tail.

Driven through the hook against a build of `main`, on 300 routine lines followed
by two `ERROR` and one `FATAL`:

```
                            before              after
zzunknowncommand --flag     5993 B, 0 of 3      5991 B, 3 of 3
sort -u app.log             1824 B, 0 of 3       246 B, 3 of 3
LOG_LEVEL=debug ./run.sh    5993 B, 0 of 3      5991 B, 3 of 3
```

`sort` gets seven times smaller *and* keeps the answer, because the lines that
matter are now the ones selected.

The rule reads position, not presence. Two earlier drafts matched the bare word
anywhere and both were caught by their own tests: `Compiling error-chain v0.12.4`
is cargo naming a crate, and `const keys = [...new Set(parsed.error.issues)]` is
source code. A log puts its level in the first few tokens, so only those are read.
`no errors detected` is excluded, but only when the negated word is the matched
one: `ERROR: no error handler registered` is a failure whose message happens to
contain the phrase, and an exclusion that read the whole line threw it away. A
count of exactly zero is the same statement in digits, so `0 errors found` is
excluded too, which is the rule `mentions` already carries for `failed` (#210).

Third instance of one shape, after #425 (a runner's verdict glyph) and #650 (a
compiler frame's own marker): the predicate that decides what is worth keeping did
not recognise how the tool actually says it failed.

Measured for regression over 150 recorded payloads through the real hook path:
identical bytes delivered before and after, so nothing that was being distilled
changed.

`LOG_LEVEL=debug ./run.sh` is in the table because an env-var prefix defeats
profile routing (#339), and #605 measures that shape at 16.5% of recorded rows.
**A `Read` fold the pipeline refuses is no longer recorded as a saving.** The ledger
wrote its fold into `ledger_folds` while building the view, and the `Read` path decides
afterwards whether it can use that view at all: survivors sitting in two blocks cannot
be renumbered from one `startLine`, so the view is dropped and the host keeps its own
bytes (#557). The row stayed. `omni_explain_savings` reads that table, so the tool that
answers "did this route pay for itself" was quoting folds that were reverted.

Measured on `97a8da2` by reading 17 markdown files twice with no edit between the reads:
13 delivered a fold, 4 delivered nothing and recorded one anyway. `CHANGELOG.md`
recorded 78.4% of 434,639 bytes, `CONTRIBUTING.md` 95.9% of 15,340, and the agent
received both in full.

The ledger is now told when the caller's host renumbers what it is handed, so the
refusal happens before the view is built and the books never see it. `ledger_record` is
untouched and now reads the situation correctly on this path as a side effect: with no
view to deliver, every line counts as delivered, which is what the agent received.
Before, a refused view recorded only the survivors, so those lines stayed foldable for a
later read that had already been shown them.

Why those four payloads refuse the fold in the first place is a separate defect (#658);
this change is only about the accounting.
**A `Read` whose survivors split now folds its head instead of nothing.** The fold was
all or nothing: survivors sitting in two blocks cannot be renumbered from one `startLine`,
so the whole view was dropped and the agent paid for every byte again (#557). Two things
split the survivors far more often than an edit does. A second change anywhere is one, and
a line stating a failure is the other, since it is marked unseen so it never folds (#458)
and that splits the run around it. Six such lines are scattered through this repo's own
`CONTRIBUTING.md`, which is why an unchanged re-read of it saved nothing at all.

Folding down to the first line that survives leaves everything below it contiguous, so the
existing `startLine` bump still describes all of it and no number moves. Runs are now
planned before any of them is emitted, which is what makes the shape of the survivors
knowable in advance; only the store can still turn a planned fold back into a verbatim run,
and that direction is the safe one.

Priced over 18 markdown files read twice with no edit between the reads, 611 KB:

| | before | after |
| --- | --- | --- |
| corpus | 23.9% | 27.3% |
| `CHANGELOG.md`, 434 KB | 0.0% | 4.4% |
| `CONTRIBUTING.md` | 0.0% | 8.3% |
| `docs/website/src/develop/index.md` | 0.0% | 7.8% |
| files that already folded | 95.4% to 99.6% | unchanged |

The ceiling is structural rather than a tuning choice: while the line count has to survive,
only a fold at an edge can keep the numbering true, so the head is all there is to take. A
fold that preserves the line count prices at 82.8% on the same corpus and was refused for
now, because it inserts lines the file does not contain.
**`omni stats --json` and the MCP session surface called an estimate an actual
count, on every row ever written.** `measurement_method` exists to tell a reader
how far to trust the number beside it, and the branch deciding it tested
`raw_tokens > 0`. That column has exactly one writer:

```rust
// src/hooks/post_tool.rs
// Reporting columns, so the calibrated central estimate.
let raw_tokens = estimate_tokens(content.len(), ContentHint::Mixed);
```

So the branch was never false. 16,405 of 16,405 recorded rows reported `actual`
for bytes divided by 3.6, a constant calibrated against `cl100k_base`, which is
GPT's vocabulary rather than Claude's. A field whose whole job is to qualify the
figure was answering the question wrongly, which is worse than not answering it.

Both surfaces say `estimated` now, and a test scans them and fails on the literal.

`format_exact_tokens` is `format_compact`. Neither half of the old name was true:
it rounds to `12K`, and since #589 its callers pass bytes as often as tokens,
because the share card's unit is chosen by the caller.

This is the half of #584 that needed no calibration run and no API key. The other
half, a divisor measured against Claude's tokenizer, is still open there.
**`omni stats --json` honours the window it is given.** It accepted `--hour`, `--day`,
`--week` and `--month` and ignored all four, so a scoped request answered about the whole
database. The window now reaches the engines, the commands, the agents, the archive counts
and the latency. `periods` keeps its three standard windows, because scoping it would leave
two of its rows describing a window they are not named after.

`--since` replaces those four flags (#667), so the fix and the flag they came from ship
together. The report assembly takes a window as an argument now, which is what makes it
testable: a window in the future has to come back empty, and a hardcoded zero returns the
rows.

`rewind_metrics` took a window in the same change, since `--detail` prints its archive pair
under a header naming one. The pair means "archives whose row was last written in this
window, and how many of those rows have ever been pulled": `ts` is refreshed when identical
content is archived again, and `retrieved` is a lifetime counter. Resetting that counter
would make the looser reading true and is deliberately not done, because #581 pays a
verbatim delivery per recorded pull.
**The passthrough log outlived every retention window.** `cleanup_old` prunes
`sessions`, `distillations`, `file_access`, `rewind_store`, `session_events`,
`ledger_folds`, `ledger_lines`, `loop_memory` and `execution_traces`.
`passthrough_events` was not on that list and never had been, so the one table
holding a command string for every declined call kept it forever while everything
else rolled off. A user who shortens retention got it everywhere except there.

It also had no session key, so a declined call could not be attributed to the
session that made it and the reason breakdown could only ever be global. A time
window is not a substitute: a third of the sessions on the machine this was
measured on overlap another. `passthrough_reasons_for(since, session)` narrows it
now, and asking for an empty session id returns nothing rather than everything:
empty means the host sent no id, so it is shared by every pipe-mode row in the table
and cannot identify a session. The reader that uses this is #612.
**`omni stats --all-commands --project` gives the project breakdown again.** The old flag
chain tested `--all-commands` last, after `--project`, so a script passing both got the
project view. #667 grouped it with `--detail` while collapsing that chain, which reversed
the precedence and handed those callers the detail report instead.

Fourth finding on that refactor and the third of the same shape: collapsing an ordered chain
into a resolver drops the order, and the order was the contract. Each of the legacy branches
carries an assertion for both orders now, the way the output-format table already did.
**A producer label could be a fragment of your own command line.** `filter_name`
is what `omni stats` groups on, and it is derived by stripping leading `VAR=value`
words off the command. The strip cut on whitespace, and both a command
substitution and a quoted value are wider than one word:

```
A=$(kubectl get pods -n web) && echo done   ->  "get"        want kubectl
A=$(ls -1t /tmp/x.gz | head -1)             ->  "-1t"        want ls
C="--context abc"; kubectl get pods         ->  "abc"        want kubectl
```

`-1t` is not a program, and the third line is worse than untidy: a column sized
for `cargo` and `kubectl` was absorbing whatever word happened to sit inside a
quoted flag. `is_silent` had the same whitespace split, which is why that segment
was picked as the producer at all, so both now read one shared quote-aware word
splitter rather than two copies of the same wrong one.

An assignment whose value opens `$(` names the program inside the substitution,
but only when nothing follows it. `TAG=$(git rev-parse HEAD) docker build .` runs
docker, and a substitution's output is captured into the variable rather than
written to stdout, so as soon as a real command comes after, that command is what
produced the payload.

When every word is an assignment, the producer is the first one that actually ran
something: `A=$(kubectl get pods) B=2` is `kubectl`, and keeping the last one
instead returned an empty label. An empty substitution has not run anything, so
`A=$( ) B=$(ls)` is `ls`. Nor does a quoted one: `$(` inside single quotes is text,
and 886 of the 2,123 recorded commands containing `$(` also contain a quote. The
body ends at the matching paren, skipping parens that are inside quotes, and the
scan indexes by byte rather than by character count.

The scanner balances `$( )` and honours backslash escapes, so a nested substitution
and `FOO="a\"b c" kubectl` both stay one word.
**`omni init` says what the host lets OMNI do, instead of a green tick that means
different things.** Configuring an MCP-only host printed the same success as a Full
one:

```
🤖 Antigravity IDE Setup
  ✓ Configured MCP Server in Antigravity IDE settings
```

Antigravity exposes no hook, so nothing is ever distilled and no row is ever
recorded there. On the machine that hit it the config had been in place twelve days
and every table held zero rows under that host. The user's conclusion was that
`omni stats` was broken.

The tier is now printed on every successful install, on every tier rather than only
the disappointing one:

```
  ✓ Configured MCP Server in Antigravity IDE settings
  ℹ MCP-only: memory and session state, no shell distill

  ℹ Full: model-facing distill active
```

`AgentIntegration::tier()` has carried this since #351 and `doctor` has printed it
correctly all along. Installation is simply where the expectation is formed, and it
was the one surface that never asked.
**OMNI told three hosts it could not do the thing it was already doing.** OpenClaw,
Hermes and Pi all hand every tool result to `omni --post-hook`, which is the path that
applies the ledger, so the model reads distilled bytes on all three. None of them
overrode the tier, so `doctor` printed "memory and session state, no shell distill"
under their names and the MCP surface was cut to the memory tools, withholding
`omni_explain_savings` from the hosts that had finally acquired something to explain.

The default is deliberate and stays: a new integration has to claim distillation rather
than inherit the claim, after three of them printed `[OK] installed` and recorded
nothing (#351). This is the same root failing the other way, so the guard is not "check
that OpenClaw declares Full", which is the same hand-maintenance one layer down. A test
derives the floor from the plugin sources in the tree: anything that spawns
`--post-hook` is asking OMNI to replace what the model reads, so the integration that
installs it cannot be the tier whose whole meaning is that nothing is replaced.

OpenClaw's Full is narrower than Claude Code's and the docs now say so.
`tool_result_persist` rewrites the result OpenClaw persists rather than the one in
flight, so the saving lands on every later turn that re-reads the transcript while the
turn that ran the command still sees the raw bytes.

The Hermes guide's "expect 25 tools" went with it. That was the whole surface, not the
advertised one, and the advertised set has been priced since #609.
**The Pi plugin threw away every distillation it asked for.** It spawned
`omni --post-hook`, waited for the answer, and then read
`hookSpecificOutput.updatedResponse`, a key OMNI stopped emitting in 0.6.5 (#158).
`updated` was always `undefined`, so the handler always returned `undefined` and Pi
received raw output on every single tool result while the hook ran and did the work.

Probed rather than reasoned about. A Pi-shaped payload of 29,099 bytes comes back as

```
updatedToolOutput: { result: <7,365 chars>, status: "success" }
```

so 74.7% was being removed and discarded, every time, for the whole life of the
plugin. The `OmniHookOutput` type declared the wrong key too, which is why the
TypeScript check on that plugin passed: the contract was wrong in the place that
would otherwise have caught it.

The Rust side has asserted the key it writes since #158. Nothing asserted that
anyone reads it, and that is the boundary #158 was about. A test scans every plugin
that spawns `--post-hook` and fails unless it names `updatedToolOutput` and does not
name `updatedResponse`. Comments are stripped first: the doc comment on this fix
names both keys, and without stripping, prose about the bug satisfied the guard.

## [0.7.6] - 2026-08-17

### Added
- **A `Read` with repeats at both ends folds both of them, and the middle keeps
  its line numbers.** Folds above and below the surviving lines leave those lines
  as one block, still the same distance into the file and into the view, so one
  `startLine` closes that gap whatever follows them. The previous release refused
  this because nothing reported how many marker lines stood above the block, and
  every way of inferring it from the output was wrong: one marker per fold is
  wrong when adjacent runs of different origin emit one each, searching the view
  for the surviving text matches inside a marker when the text is something a
  marker also says, and recognising markers by their prefix is defeated by a file
  whose own lines start with it. `substitute` reports the count now, which is the
  only place that knows.

  Sized before it was built, over 4,552 `Read` results in the local transcripts:
  folds at both ends are 129 of the 1,868 reads that repeat something and carry
  257 KB, against 3.29 MB reachable before, so roughly 7.8% more folded bytes.
  End to end on that shape, same binary either side: **0.0% saved before, 33.8%
  after**, with `startLine` moving from 100 to 124 exactly as the twenty-five
  folded head lines require. Reads whose survivors sit in two separate blocks,
  1,020 of the 1,868, stay refused and always will: one starting number cannot
  describe two offsets. (#573)
- **A `Read` that repeats the head of a file folds again, and the host's line
  numbers stay true.** #557 stopped a fold renumbering the lines under it by
  refusing the fold. That is correct, and it costs the most common shape there
  is: re-reading a file at a later offset repeats its head, so the run the ledger
  can fold is exactly the run with content below it. The host renders
  `file.content` with `cat -n` numbering counted from `startLine`, so moving
  `startLine` by the number of marker lines now standing above the survivors puts
  every one of them back where the file has it. Verified on live host transcripts
  before being relied on rather than assumed, which is the #158 lesson: a `Read`
  requested at offset 215 comes back with `215` on its first line.

  The rule is exact rather than clever. The survivors have to be one block
  running to the end of the payload, which makes the lines above them a
  subtraction and means nothing has to be searched for. Everything else refuses,
  including shapes that are correctable in principle, because the alternatives
  are searching the view for the survivors' text, which can match inside a
  marker, or a marker count nothing reports. A refused fold costs bytes; a wrong
  number costs an edit in the wrong place. The shape comes from the ledger's own
  folded indices, so a file whose lines begin with `[OMNI:` cannot change the
  answer.

  Measured on four overlapping windows of a markdown file under the host's output
  cap: **0.0% saved before, 4.7% after**, one fold where there had been none.
  Source files are unaffected, since the `readfile` distiller acts on those first
  (46.6% either way).
- **A fold now records whether it covered the whole output**: `MIN_WHOLE_OUTPUT_FOLD` refuses a whole-output fold under 1 KB because the agent is left holding a handle and no content, and #543 calibrated that floor on four such folds. Nothing recorded which folds were whole-output, so the floor could not be checked against its own corpus after the fact: `distillations` carries `collapse_original = 0` on exactly the four rows the decision cites, and `ledger_folds` had no flag at all. Verifying the claim on a live store meant guessing from the delivered-bytes ratio, which misreads an aggressive `Keep` as a whole-output fold.

  `ledger_folds.whole_output` is set when every line of the payload folded, which is the call-level reading of the same condition the floor tests per run. The two differ only when adjacent runs of different origins tile the payload, and the question the column answers is whether the agent kept any content at all, so the call-level reading is the right one for a table that already aggregates by (origin, source agent) per call.

  ```sql
  SELECT * FROM ledger_folds WHERE whole_output = 1 AND payload_bytes < 1024;
  ```

  Zero rows is the floor holding. `payload_bytes` rides along on every row of the call for that query to be a row predicate: summing `bytes` would need a GROUP BY on a per-call key, and `ts` is whole seconds, so two folds by one agent inside one second merge and their combined size can clear a floor that neither cleared alone. That is the audit silently hiding the one thing it exists to find, so the size is recorded per row instead.

  Rows written before the columns carry 0 meaning "not recorded" rather than "was partial", so a query has to bound itself by `ts`. The migration was run against a copy of a real 89 MB store and left its 55 existing rows intact.

### Changed
- **OMNI advertised 25 MCP tools and 16 of them had never been called.** The
  definitions sit in the prefix of every request of every session, so they are
  re-read on each one rather than paid for once. Measured across 229 Claude Code
  transcripts: nine tools account for all 109 calls, `omni_retrieve` and
  `omni_explain_savings` alone for 84% of them, and the sixteen that were never
  called are **4,940 bytes**. That is the same size as the **4,942 bytes** OMNI
  removes from tool output in the median of the 35 sessions that pushed more
  than 50 KB through the hook, and it is carried from position
  0 where it is re-read on every request instead of from the middle of the
  session, so advertising them cost about twice what the distillers earned. A
  host is now told about the tools its tier can use: nine on Full and
  Handoff-first, four on MCP-only, and `omni_run` is guaranteed present on
  Handoff-first because there it is the only path by which the model reads less.
  `OMNI_MCP_TOOLS=all` restores every tool, and `omni doctor` says which is in
  force. The evidence is one machine and Claude Code only, which is why the
  override exists and why doctor names it. (#577)
- **The landing page leads with what OMNI buys and the use-case page covers eleven
  situations instead of six.** No figure changed. The three largest measured ones, 97.2%
  off a repeated file read, 94% off `git log -15` and 92.9% off a `cargo test` run, were
  each buried inside a page and are now on the first screen beside the 4,940 bytes the
  lean MCP surface takes off every request. The aggregate and the 97.3% of calls that save
  nothing keep their own section, because a tool that claims to help everywhere is one
  nobody can predict. Five situations are new: reading one file in several passes,
  dispatching a subagent, following a marker back, a context that gets compacted, and the
  tool list every request carries. Both languages.
- **`Your first hour` teaches the two checks that make the rest worth trusting.** It now
  opens with what the hour buys, checking OMNI rather than trusting it, and adds the two
  commands that do it: pulling content back through a handle, and telling a real marker
  apart from one that is only text. Both were verified against a built binary before being
  written down, exit 1 with `the documentation example` for the reserved handle and exit 0
  with the content for a real one. The `omni stats` section also says its absolute figures
  are bytes now and why they used to be something else. Both languages.
- **`Seeing what it saved` says what its numbers are counted in, and names a breaking
  change.** Every absolute figure is bytes now, and the page explains why they used to be
  something else and why the percentages never moved with them. It also documents that
  `omni stats --json` renamed `commands[].tokens_saved` to `bytes_saved`: the field held
  bytes under the old name for one release, and a machine-readable surface asserting the
  wrong unit is the defect rather than a cosmetic slip.
- **`What OMNI is` covers the second thing OMNI edits: itself.** Tool definitions sit in
  the prefix of every request, so a prefix byte is carried from the first request while a
  removed output byte was inserted in the middle. Sixteen of twenty-five advertised tools
  had never been called across 229 sessions, and they weighed 4,940 bytes against the
  4,942 the distillers remove from output in a busy session. Both languages throughout.
- **`The ledger` names the three readers its premise fails for.** The page already rested
  on one sentence, that the agent is still holding these bytes, and every rule is a
  defence of the moment that stops being true. There are exactly three readers it fails
  for and this release answered all three: a subagent carrying the parent's session id, a
  context the host compacted, and a reader following a handle back. The pattern is stated
  as the thing worth keeping, since it is what makes the next surprise diagnosable. Both
  languages.
- **The mark's ink is the terminal blue, not amber**: the site's terminal block stopped
  reading in amber when it moved onto the design system's terminal ramp, which left the
  logo as the only orange left in the product. The three gradient stops are the amber
  ones converted rather than re-picked: the middle one is `--wl-terminal-blue` exactly,
  and the outer two keep the old gradient's lightness offsets and chroma ratio at that
  hue, so the modelling in the artwork survives the swap. All three are in gamut. The
  path is untouched, and `media/logo.png` is re-rendered from the SVG at the 1254px it
  already was.
The README and all six translations now say plainly that every percentage on the
measurements page is bytes per command and not a bill. Billed input tokens track
roughly turns times prefix size, so a shortened payload only pays when it also
removes a turn, and the end to end saving on whole sessions is larger on average
while not being guaranteed on any single one.

### Fixed
- **The manual described 0.7.4 after 0.7.5 shipped (#555)**: three behaviour changes
  landed in the release and reached no page a reader sees. The ledger page gave the two
  fold bars, 150 bytes for a session-origin run and three times that for a project-origin
  one, and stopped there, so the floor #543 added was invisible: a fold covering the whole
  output needs 1024 bytes of input or the run stays verbatim. That is the bar a reader
  meets first, because a short command whose entire output repeats is the common case. The
  264-byte input floor was undocumented too, and between them the two explain most of the
  reports that OMNI did nothing. Both are now on the ledger page in English and
  Indonesian, with the measurement that set them.

  **The three floors are now guarded rather than written down.** #541 was a count in
  prose that nothing read, and putting two more numbers on a page would have rebuilt it,
  so `every_documented_count_matches_the_code` reads `MIN_LEDGER_RUN_GAIN`,
  `MIN_LEDGER_INPUT` and `MIN_WHOLE_OUTPUT_FOLD` out of `guard/limits.rs` and checks every
  statement of them in both books, importing the constants rather than parsing them back
  out of the source. Each claim carries enough of its sentence to belong to one floor and
  nothing else, and a claim that matches no document at all now fails too, because a
  pattern reworded past its sentence is a check that quietly stopped checking, which is
  what #541 was.

  **Coverage is counted per book, not once across the tree.** One shared counter would let
  the English page keep a claim alive while the Indonesian one stopped stating it, which
  is #541's reach problem again, one dimension out: the count is right, the guard is
  green, and one language is no longer being checked. Every wrong number and every
  single-language reword was proved to turn the test red, in both directions, and so was
  dropping the tool count from both Indonesian pages that carry it, which nothing caught
  before this change.

  `ledger_folds` was missing from the database table list, which is the one table #533
  added and the only one that records why a marker was issued.

  **The release page told the reader to verify a tag by a label that lies for four
  hours.** `guard::update::get_status` caches the newest known release for 14400 seconds,
  so a machine that ran `omni doctor` before the tag reports `[AHEAD/RC]` whatever it is
  running, which is what 0.7.5 did on a fresh install. The reliable half is the absence of
  the `UNRELEASED` line, computed by `build.rs` from the tree with no cache. The same page
  forecast a manual tidy on the first fragment cut; that happened on 0.7.5, two heading
  pairs merged by hand, and the page now says to verify a cut by word count rather than by
  eye.

  Two samples named old versions: the build transcript on both manual front pages said
  `Compiling omni v0.7.4`, and the plugin skill's `omni doctor` sample said `v0.7.3`.
- **A ledger fold renumbered a `Read` result, so every line number below the
  marker was wrong.** The reply goes back as `file.content` and the host renders
  it with `cat -n` numbering counted from `startLine`, so it numbers whatever
  lines OMNI returns. Replacing a run in the middle with a one-line marker
  removes lines the count was walking over: in the report, real line 130 came
  back labelled 101 and real line 199, the last in the file, came back as 170,
  a shift of exactly the fold size minus the marker's own line. Nothing in the
  payload said so. Those numbers are a contract rather than prose, since the
  agent writes `file:line` into issues and commit messages and decides what to
  edit from them, so this is the familiar shape: nothing truncated, no bytes
  lost, and a confident statement the code cannot support. A fold with nothing
  under it moves no number, so the whole-output fold and any fold reaching the
  end of the payload still happen. `Grep` carries its positions inside the text
  where a fold cannot move them, and `Bash` output is not numbered, so only
  `Read` is gated. (#557)
- **A bare `pass=` was redacted as a credential, so a test loop's result was
  destroyed.** `echo "pass=$P/10"` after ten runs came back as
  `pass=[REDACTED]`, and nothing in the output said the token had been a count
  rather than a secret, which leaves re-running the loop or reporting a number
  nobody saw. A bare `pass` now delivers a count, `3` or `3/10` and nothing
  else. The `KEY` value rule from #486 is deliberately not reused: it accepts any
  short lowercase word, which is right for `key` and wrong for `pass`, where
  `hunter2` is what a password looks like. So `pass=hello` and `pass=hunter2`
  stay redacted, which is narrower than the report asked for and follows the rule
  this file already states, that hiding a value which did not need hiding is
  recoverable and printing a secret is not. `DB_PASS` is named after what it
  opens and keeps its strength unconditionally; `password=`, `PGPASSWORD=` and
  `db_pass=` are unchanged. Each of those has a row in the test. (#559)
- **`passwd=` had never been redacted at all.** Found while fixing the above
  rather than reported: the pattern set matches on whole segments, so `PASSWD` is
  neither equal to `PASS` nor a word ending in it, and `PASSWORD` is a different
  word again. `passwd=hunter2` was delivered in full by every release that has
  shipped this matcher. Added to the set. (#559)
- **`kubectl get pods` keeps every row again.** For one release a 10 row pod table
  arrived as three lines with seven pod names deleted, at a reported 73.5% saving.
  The distiller was not the thing that changed: `src/distillers/cloud.rs` is
  byte-identical to 0.7.3. `signals/tools/kubectl.toml` had been shadowing it since
  #110, #510 retired the TOML layer, and the summariser underneath ran for the first
  time. It is deleted rather than guarded, because the same arm already hands back
  every other `kubectl get` listing for exactly this reason: a count of pods cannot be
  turned back into a pod name. Repeated tables are still cheap, the ledger folds a
  listing the agent has already seen, and it needs the rows intact to do it. The guard
  now sits at the hook boundary rather than at the distiller, because collapse runs
  between them and can eat the rows on its own (#562).
- **`omni stats` booked savings on calls where the hook sent nothing, and the
  all-time figure was 4.5x too high.** `process_payload` returns `None` when the
  route is a passthrough and nothing was redacted, so the host keeps the bytes it
  already had, but every byte and token column on the recorded row was computed
  from the distiller's output before that decision was made. A distiller that cut
  more than the guardrail's tenth and less than the soft threshold therefore
  booked a saving the model never received. Found by reconciling `distillations`
  against the host's own transcripts under `~/.claude/projects`, where the
  `tool_result` bytes are post-hook and are what the model was actually given: over
  the 45 sessions holding a transcript, 67 of the 389 rows that booked a saving
  were this shape, 33,751 bytes of 205,873, and one was verified end to end before
  anything changed (raw 7,744 B, booked 6,356 B, transcript entry 7,744 B with no
  marker). `applied_only()` could not separate them, because it gates on
  `delivered_bytes` and `delivered_bytes` was copied from the same string: it
  equalled `output_bytes` on all 6,145 `claude_code` rows. Existing rows are
  corrected once by migration rather than by a `CASE` in each of the six queries
  that sum those columns, and `execution_traces` still holds what the distiller
  produced. On the reporting installation these rows carried 93.0% of every byte
  ever booked as saved: all time went from **37.1% to 8.3%** over an unchanged
  6,386 commands, which matches the 4% to 15% per command measured independently
  from `execution_traces`. (#566)
- **A project fold said `from an earlier session`, which reads as a claim the
  reader had already seen the content.** It means the opposite: the project scope
  only answers for lines the session scope did not, so those lines were never
  delivered here. Acting on it, a reader took a `claude plugin --help` page whose
  `Commands:` block had been elided as a complete one and concluded the CLI had no
  uninstall subcommand. The run form now says `N lines not shown here`, leading
  with the only thing the reader has to act on and dropping the provenance, which
  also makes it nine bytes shorter, 72 to 63. Marker length gates folding, so the
  honest wording is the cheaper one here rather than a trade against it. The
  whole-output form keeps the provenance and adds `none shown here`, because there
  the agent holds nothing at all. The manual carries the new wording in both
  languages, and `bench_replay` counts project markers on `shown here`, which both
  forms contain and neither session form does. (#567)
- **The guard that was supposed to catch a marker the manual does not document
  could not fail.** It compared each template only up to its first placeholder, so
  it checked `[OMNI: ` and `[OMNI: identical to ` and nothing else, and any page
  holding any marker satisfied it. The wording after the count, which is the whole
  of what a reader looks up, was never read. Proved by putting a retired marker
  back into the page and watching the suite stay green. A second version, checking
  each literal fragment independently, let one template borrow another's text: the
  whole-output session example contains `lines already shown, omni retrieve`, so
  deleting the ordinary session marker still left it green. It now matches each
  template whole, with `{lines}` typed as a number and `{handle}` as a hex digest,
  which is what stops the borrowing. Both experiments turn it red. (#567)
- **Every hook run sorted the whole `sessions` table to read one row.**
  `find_latest_session` is `ORDER BY last_active DESC LIMIT 1` and there was no
  index on that column, so the plan was `SCAN sessions` plus a temp B-tree: it
  dragged every row's `state_json` through a sort to return one of them, 24 rows
  and 312 KB on the reporting installation. With the index the plan is
  `SCAN sessions USING INDEX idx_sessions_last_active` and there is no sort. Worth
  2% of the hook's wall time measured by A/B on the release binary, which is
  smaller than the query plan suggests and is reported as measured rather than as
  the plan implies. (#569)
- **The skill still failed both skills.sh audits after #558, because a scanner
  reads a URL and not the sentence disowning it.** #558 replaced the
  `curl | bash` install with a release archive checked against `SHA256SUMS`, and
  left one line saying where the pipe installer lives and why it is not the
  instruction. Gen Agent Trust Hub called that URL "the installation process" and
  returned Fail at HIGH; Snyk returned Warn at MEDIUM W012, quoting a
  fetch-and-execute command that was no longer anywhere in the file. Both re-ran
  on the corrected version, at 12:16Z and 12:15Z on 2026-08-15, so this was the
  wording and not a stale copy. The three lines are gone. A reader loses nothing:
  the sentence documented a route it told them not to take, and `README.md` still
  carries the pipe for anyone who wants it. (#578)
- **Following a marker returned the marker.** A fold prints `omni retrieve <handle>`, and
  the bytes that came back passed through the hook, hashed the same, and were folded into
  the very marker that had sent the reader to fetch them. An agent that did what the
  marker said got the instruction again, and `OMNI_PASSTHROUGH=1` on the retrieve did not
  help because the fold lands on the later read rather than on that command. A pull now
  marks the archived row owed, and the delivery answering it is handed over verbatim. It
  costs one delivery and not an exemption: the next repeat folds again, which matters
  because 15.05% of the archive on this installation has been pulled at least once. (#581)
- **A subagent was told the parent's bytes were already shown.** Claude Code hands a
  subagent the parent's `session_id`, so keying the ledger on the session alone answered
  a subagent's first read with the parent's history: a 200 line payload came back as
  `identical to the 200 lines already shown` to a context that had received none of it.
  The host does distinguish them, through a top-level `agent_id` that is unset for the
  main agent, and OMNI was not reading it. The scope is now the reader rather than the
  session. The fold is kept and only the claim changes: a subagent falls through to the
  project scope, whose wording since #575 says `none shown here`, so it still gets a
  marker and a handle. The main agent has no `agent_id`, so its scope is unchanged and no
  history is orphaned by the upgrade. Not fixed, and recorded on the issue rather than
  left implied: after compaction the same premise fails with no signal in the payload to
  detect it. (#581)
- **A marker OMNI emitted and a marker printed as an example were byte-identical, so
  nothing could verify OMNI had run.** The examples in the source and the manual were
  not invented, they were harvested from real sessions: four of the seven distinct
  handles still resolved on the maintainer's machine, including the canonical one that
  appears in 99 places. That defeated both ways of asking whether anything was folded,
  since grepping for the marker shape counted our own documentation and resolving the
  handle counted it too. Measuring a 0.7.5 A/B it scored an `OMNI_PASSTHROUGH=1` arm as
  having folded eight times and briefly declared a valid control run invalid. Every
  example now uses one reserved handle, `0000000000000000`; `store_rewind` can never
  mint it, and `omni retrieve` refuses it by name instead of blaming the 30 day prune,
  which for an example is a cause the code cannot know. `omni retrieve` already exited 1
  for an unknown handle, so the check existed and our own strings were what broke it.
  (#583)
- **The hooks page told you to feed `omni --post-hook` a payload and never said what one
  looks like.** Getting the shape wrong exits 0, prints nothing, and reads as `0.0%
  saved`, so there is no error to notice and a distiller cutting 96% can be written off as
  not firing. Both shapes are documented now, in both languages: `Bash` carries the output
  at `tool_response.content`, `Read` wraps it at `tool_response.file.content` with the
  `startLine` a fold has to move. Both `Read` forms are real and reach different stages,
  the bare one the ledger and the wrapped one the `readfile` distiller, which is the pair
  that cost an hour on 2026-08-17. A test asserts both still parse, so the page cannot
  quietly stop being true. (#586)
- **The context breakdown counted a quarter of a file's size and called it tokens.** It
  accumulated `size_bytes / 4` from file metadata, a rougher estimator than the 3.6 the
  rest of the report used and still not a Claude token count, so the block had to be
  labelled a rough estimate to be honest. It accumulates the sizes now, which are counted,
  and the label goes with the estimator. The largest-file-read line and the share card's
  `unit` moved with it: the card used to say `tokens` or `bytes` depending on whether the
  rows happened to carry a token column, so two installs could publish the same saving
  under different words. No figure `omni stats` prints is derived from another vendor's
  tokenizer now. (#589)
- **`omni stats` reported in a unit OMNI cannot defend.** The per-period summary read
  `10K to 10K tokens`, and the per-command and per-filter annotations read `-N tokens`,
  all of them a byte count divided by 3.6, a constant calibrated against `cl100k_base`.
  Every one of those now reports the bytes `distillations` counts exactly, and the
  percentages beside them are unchanged because the divisor cancels in a ratio. The
  context breakdown is the one block that could not move: it accumulates `size_bytes / 4`
  from file metadata, so there is no measured total behind it, and it now says so and
  prints `~` rather than pretending to a count. #592 fixed the same defect in the hook
  banner; this is the report surface. (#589)
- **`omni stats --json` renamed `tokens_saved` to `bytes_saved`.** The field briefly held
  bytes under the old name, which is a machine-readable surface asserting the wrong unit.
  A consumer reading that key has to update; the alternative was leaving the name lying.
  (#589)
- **The savings banner reported a unit OMNI cannot defend.** `-500tok this call` was a
  byte count divided by 3.6, a constant calibrated against `cl100k_base`, which is GPT's
  encoding. It now reports the bytes it counted. The percentage beside it is unchanged and
  always was sound: the divisor cancels in a ratio, so `(raw/k - filtered/k) / (raw/k)` is
  `(raw - filtered) / raw` for any `k`. The banner's own bars moved with the figure they
  gate, 500 and 1000 becoming 1800 and 3600 at the same 3.6, so it does not fire 3.6 times more
  often, which leaving them would have caused. The translation is close rather than
  exact: the old bar subtracted two independently rounded estimates and disagrees with
  the new one over savings of 1,797 to 1,799 B, always in the direction of the old bar
  firing and the new one staying silent. `omni stats` also printed `Tokens Reduced`
  directly under `Data Distilled`, the same quantity in exact units, and that line is
  deleted rather than converted. This makes #212's guard stricter: the banner now prints
  the `distillations` row's own two columns subtracted, with no estimator between the
  banner and the record it has to agree with. `omni stats` still labels other figures in
  the same derived unit and this does not touch them. (#589)

### Removed
- **The benchmark image is gone from the six translated READMEs**: `performance.png` was
  hotlinked by `id`, `ja`, `ko`, `vi`, `zh` and `ar`, and by nothing else. `README.md`
  never carried it, so those six readers were the only ones seeing it, and for weeks what
  they saw was 58.9% across 1,810 executions while the English table beside them said
  14.9% across 6,656. The card was regenerated from the current corpus first, then
  dropped, so nothing was removed to hide a number. Every README keeps the table the
  image duplicated.

## [0.7.5] - 2026-08-14

### Added
- **The manual reads in Indonesian (#539)**: #513 shipped a language switcher whose every entry left the manual for a README on GitHub, because the manual itself was English only. It advertised seven languages and delivered one page in each. 27 pages plus a `SUMMARY` are now translated (`index`, `concepts`, `use`, `integrations`, `reference`), served at `/docs/id/`, and the `ID` entry in the switcher points at the manual instead of the README.

  `develop/` is deliberately not translated. Those seven pages change fastest and are read by people about to send a patch, so an Indonesian copy of them would be wrong more often than it was useful; one Indonesian page says that and links the seven originals.

  **No second `book.toml` and no new dependency.** mdBook reads configuration from the environment, so `docs/website/build.sh` renders the same book twice with a different `src`, `language` and `build-dir`, then copies the second into `book/id` so the site still copies one tree. The diagrams are copied into `src-id` at build time rather than forked: their labels are English either way, and a copy made at build time cannot fall behind the original.

  **A stale translation is worse than none, so two things carry the drift.** Every Indonesian page gets a banner naming itself a translation and linking its English original, injected in `theme/siteframe.js` rather than pasted into 28 files that would then fall out of step with each other. And `scripts/check_translations.sh` fails CI when an English page changes without its counterpart being touched, skipping pages that have no counterpart so `develop/` does not go permanently red. `tests/docs_match_the_code.rs` now scans both books: the prose is translated and the commands are not, so an Indonesian shell fence can name a deleted subcommand exactly as easily as an English one.

- **The manual header carries the mark (#540)**: `theme/siteframe.js` turned `.menu-title` into a link home whose content was the plain string `OMNI`, while the README, the site and the favicon all carried the logo. The mark is read from the `<link rel=icon>` mdBook already emits rather than from a hardcoded filename, because mdBook 0.5 fingerprints theme assets and the hash changes whenever the mark does.

- **A fold now records what it drew on (#533, first step)**: `ledger_lines` says a line was *seen*. It cannot say a marker was ever issued against it, under which scope, or whose bytes it replaced, so the value of cross-agent reuse was unrecoverable from the corpus after the fact. `PROJECT_FLOOR_MULT = 3` prices exactly that case and was calibrated in #448 on cross-session repetition within a single agent, which is to say it has never been checked against the thing it charges for.

  `ledger_folds` takes one row per (origin, source agent) per call, so the query that settles the open decision is a `GROUP BY` rather than a replay. `ledger_seen` returns the agent alongside the hash to make the source answerable at all, and `INSERT OR IGNORE` on the line table means that names the agent actually shown the line rather than the last one to repeat it.

  **The decision the rows are for is deliberately not taken here.** Whether the project scope stays shared or becomes `(repo, agent)` is a choice between two unmeasured options until the corpus has run, and choosing early is what this project keeps refusing to do. Deletion ships in the same commit as the schema, pruned in the same window as the lines it describes, because `passthrough_events` shipped with no cleanup and grew unbounded.

### Changed
- **Changelog entries moved to `changelog.d/` fragments**: every branch used to edit `## [Unreleased]` in `CHANGELOG.md`, so N parallel branches cost N-1 identical "keep both sides" resolutions and N full CI re-runs. The repo has its own evidence: five branches opened in one sitting on 2026-08-02 produced four of them, and the workaround written down at the time, batch a lane into one branch, stops being available the moment two agent sessions work in parallel, which is now routine here.

  One file per entry, `changelog.d/<issue>.<section>.md`, holding the bullet and no heading. Two branches never write the same path, so git has nothing to conflict on. `scripts/changelog_cut.sh` folds them into a version section at release time and deletes them, which also automates the manual "move the entries under `## [x.y.z]`" step that #137 exists because someone forgot.

  **Additive on purpose.** `build.rs` counts the fragments *plus* the bullets still under `## [Unreleased]`, so `omni doctor` stays true while both shapes coexist and no in-flight branch had to be rebased to land this. The first cut after it may need one manual tidy where a carried bullet lands beside a heading the fragments just wrote; the script says so when it happens.

- **The skill's install line names `fajarhide/skills` (#547)**: one skill answered to two URLs. `npx skills add fajarhide/omni` is what the README and both manuals printed, `npx skills add fajarhide/skills --skill omni` is what the directory page prints. Both pull the same file, since `fajarhide/skills/skills/omni/SKILL.md` is synced daily from `plugins/claude-code/skills/omni/SKILL.md`, so the two could only ever disagree on which URL a reader learned.

  The one now documented is the one with a page behind it. `https://www.skills.sh/fajarhide/omni` answers 200 and renders a 404 body with no `<title>`; `https://www.skills.sh/fajarhide/skills/omni` is a real listing, and the four files now link it. `fajarhide/omni` still installs and is simply no longer advertised. The short form was run into an empty directory before it was published: `rc=0`, `Installed 1 skill`.

  **Six of the seven READMEs documented neither install path.** `ja`, `zh`, `ko`, `vi` and `ar` carried no plugin and no skills block, and `id` carried the skills block without the plugin one, so the block English readers have had since #546 reached nobody else. All six now carry both, in their own language.

### Fixed
- **The shipped skill told agents to install Claude Code's hooks whatever host they were in (#545)**: `plugins/claude-code/skills/omni/SKILL.md` is the skill `npx skills add` installs, and it instructed the agent to run `omni init --claude` unconditionally, while describing OMNI as "a Claude Code hook" that writes "into `~/.claude/settings.json`". Followed inside Cursor or Codex, that configures a host nobody asked about. It now says to run bare `omni init`, which configures the host it is running inside and prints which one it picked, and it states the rule in as many words: do not name a host you have not established you are running in. The prose generalised with it, from "a Claude Code hook" to "a hook the agent host calls".

  **The binary did not change, the instructions did.** This shipped in the same commit as #545's documentation change and was described by neither the issue nor the pull request, which is why it took a changelog audit to find. The install URL that commit added is the one #547 superseded the same day, so both entries touch the same file for different reasons.

- **A changelog fragment with the wrong extension vanished silently**: `changelog.d/544.fixed.txt` was counted by nothing and folded by nothing. `count_fragments` required `.md` and so did the glob in `scripts/changelog_cut.sh`, so both sides agreed to ignore it: `omni doctor` reported no outstanding entry, the release cut printed `No fragments in changelog.d/` and exited 0, and the entry never reached the release notes. That is the outcome the directory was built to prevent, one dimension further out than the mistyped section the guard already caught.

  Fixed by removing the dimension rather than by patching the instance. **Anything in `changelog.d/` that is not `README.md` and is not hidden is an entry**, on both sides, so there is no extension for the two predicates to disagree about. A mistyped *section* still cannot be placed and still refuses the cut, loudly, before `CHANGELOG.md` is touched. Hidden files stay excluded because macOS writes `.DS_Store` into any directory it opens and counting it would have `omni doctor` report unreleased work that does not exist.

  `tests/changelog_cut.rs` drives the real script in a temporary directory: the fold, the two refusals, the post-release state where only `README.md` remains, the dotfile, and a re-run against a version already cut. Five deliberate breaks were each proved to turn it red, including reverting each predicate separately, and one arm that first appeared green turned out to be a patch that had not applied rather than a weak test.

  A follow-on from review on the same change: a fragment named with a space or a newline made the fold loop word-split and `cat` open a path that did not exist. That already failed safely, non-zero with `CHANGELOG.md` untouched, but it failed as `cat: changelog.d/545: No such file or directory`, naming neither the file nor the reason. Names are now checked against `[A-Za-z0-9._-]` before anything is read, and the refusal says which name and why. A space is the realistic version of this; the newline is what review raised.

- **`omni_run` could hang until the host gave up (#544)**: reported on Windows in Cursor, where some Git commands returned `MCP error -32001: Request timed out` after 120 seconds while the same command took 397 ms in PowerShell. Three separate defects in one spawn, none of them Windows-only.

  **stdout was drained to EOF and only then stderr.** A child that fills the stderr pipe buffer before closing stdout blocks forever, and the reader blocks with it. Reproduced on macOS with 200 KB of stderr; the buffer is 64 KB there and around 4 KB on Windows, which is why it was reported there first. `wait_with_output` drains both at once, which also deletes the hand-rolled read. `omni exec` never had this because it inherits stderr rather than piping it.

  **The child inherited the server's stdin, which is the JSON-RPC pipe to the host.** One byte read from it breaks the framing for the rest of the session, so every later call fails no matter what it asks for. The child gets `Stdio::null()`.

  **Nothing bounded the child, and the blocking call ran on a runtime worker.** One hung command took the thread that was serving everything else, which is the shape of a report where three calls succeed and everything after them times out. There is a 60 s deadline now, `OMNI_RUN_TIMEOUT_SECS` raises it for a build that legitimately outlasts it, and the work moved to `spawn_blocking`. A timeout returns a sentence naming the command that stalled rather than the host's opaque error code.

  The reporter's `git --version` case is not explained by any of these: it writes about 25 bytes to stdout and nothing to stderr. That half of the report is still open.

- **A whole-output fold under 1 KB now stays verbatim (#543)**: every floor in the ledger asked one question, whether a run outgrows the marker replacing it. That is the right question for a partial fold, where the agent keeps context beside the handle and can decline to spend it. It is the wrong question when the fold covers the entire output, because then the agent holds a handle and nothing else, so needing any part of the payload costs a retrieval round trip it has no say in.

  Every whole-output fold this machine recorded after 0.7.4 went in was under 1 KB, and **four of the four were retrieved within nine seconds**, against a 0.85% retrieve rate across all 5,178 distillations in the same store. Those folds saved 2,680 bytes, then spent 319 bytes of marker plus four extra tool calls handing the same 2,999 bytes back. `MIN_WHOLE_OUTPUT_FOLD` is 1024 because that is the top of the measured range, not a knee: nothing above it was observed either way, so the floor covers what is known to lose and leaves the rest folding. n=4, one machine, window bounded by the 2026-08-11 store reset.

  **The report that opened #543 was wrong and the issue records the correction.** It described three runs of one command returning zero content lines non-deterministically. `execution_traces` still held all three: they were three different commands, the middle one's quoted output was the last two lines of a ten line reply, and the third came to 255 B, under `MIN_LEDGER_INPUT` (264), so the ledger never read it. There was no non-determinism, and the same 264 B floor is why the reporter's synthetic repro never fired. What is real reproduces in two runs of one command, not three.

  `holds_a_project_repeat_to_its_own_higher_floor` moved to a partial fold in the same change. Its fixture sat between the session and project bars and covered the whole payload, so the new floor decided it before either bar was consulted and the test stopped being about the two bars.

- **The MCP tool count read 26 in six places and the guard could not see one of them (#541)**: `tools/list` answers 25 on 0.7.4. `CONTRIBUTING.md` said `26 tools`, and both books repeated it in the one paragraph whose whole point is to count rather than trust a written number.

  `every_documented_count_matches_the_code` was already checking counts and passed on all six, for two reasons that are about reach and not about the number. `doc_files()` walks the two books and `README.md`, so `CONTRIBUTING.md` was the only prose file with a count in it that nothing read. And the claim pattern matches the English noun, so every count in the Indonesian manual went unchecked, including the two that were right.

  The self-contradicting sentences now name the command instead of a number, in both languages, so there is nothing left there to drift. The counts a reader wants up front stay, and all of them are inside the guard now: `CONTRIBUTING.md` is scanned, and the pattern reads `perkakas` as well as `tools`.

## [0.7.4] - 2026-08-13

A release about claims that were not true: a documented escape hatch that did
nothing on the host it mattered on, a manual that named commands which exit 1,
a distiller cutting prose it never read, and a project scope that split its own
history every time someone opened a worktree.

### Added
- **The manual has a share card (#527, partly)**: every one of its 35 pages carried 0 `og:*` tags, so a link to the ledger page or the benchmark method shared as a bare URL. `theme/head.hbs` now emits 11 tags per page, with `og:title` taken from the page's own chapter title rather than the book title repeated 35 times, and `og:image` pointing at the `media/og.png` that already existed. The shared description was also the positioning from before the manual was rewritten to lead with the ledger; it now matches what the README says.

  **`og:url` and `<link rel=canonical>` are deliberately still missing.** Both need the absolute URL a reader lands on, and mdBook offers only `{{ path }}`, the *source* path (`concepts/the-ledger.md`). The site serves these extensionless and redirects `.html` to bare, so a tag built from `{{ path }}` would name a URL that redirects, and a canonical pointing at a redirect splits the signal rather than joining it. Handlebars here has no string helpers, so doing it right means rewriting the built HTML after mdBook runs. Left out rather than shipped wrong, and the reason is written into `head.hbs` next to the tags that are there. A per-page description needs the same post-build step or first-paragraph extraction, and the sitemap half belongs to the repo that builds the site.

- **The manual is checked against the code it describes (#520)**: `tests/docs_match_the_code.rs` asserts that every `omni <cmd>` written inside a shell fence is a subcommand `main.rs` dispatches, and that counts stated in prose match what they count. This class had been fixed by hand four times: #180 named two deleted subcommands, #185 two removed flags, #390 four whole documents, and the #521 sweep found `omni history` and `omni insight` in shell blocks that exit 1, plus a tool count reading 26 against 25. Every round was found by a person happening to open one page.

  **The subcommand list is read from `main.rs` rather than copied**, so the check cannot become the second list that drifts from the first, which is the defect it exists to catch. A structural change that defeats the scan empties the set and a sanity assertion fails loudly instead of the suite passing vacuously. Probing the real binary would be the more honest level and is unsafe here: `reset` drops the database, `dashboard` binds a port and blocks, `update` reaches the network.

  **Fence-scoped on purpose.** `integrations/loops.md` names `omni handoff` in prose precisely to say it is not a subcommand. Proven by reintroducing both defects #521 fixed: the gate reports `use/memory.md:83 omni insight` and `reference/mcp-tools.md:3 says 26 MCP tools, the code has 25`. Prose describing the pipeline in the wrong order, the fourth kind #521 found, is not checkable here and is not claimed to be.

- **A Claude Code plugin, so onboarding is two lines typed inside the session (#529)**: `/plugin marketplace add fajarhide/omni` then `/plugin install omni@omni`. It ships a skill, not the binary: an agent with the MCP tools wired still has nothing telling it *when* to call `omni_retrieve`, and the install instructions were written for a human at a terminal. The skill carries the non-interactive install, `omni doctor` and what healthy looks like, the five markers and what each one means, and the rule that output carrying a marker must not be written back to a file. Measured on install: 1 skill, about 68 tokens always-on and 740 on invoke.

  **The plugin lives at `plugins/claude-code/`, not at the repository root.** The issue asked for `source: "./"`, and a local install of that copied the whole checkout, `target/` included, into the plugin cache: **81 GB**. Pointed at the subdirectory it is 36 KB. `plugins/openclaw` and `plugins/pi` already put host integrations there, and `bump_version.sh` now bumps the new manifest alongside openclaw's, because a `version` in `plugin.json` pins the plugin and a release that forgot it would leave every installed copy behind.

- **The ledger records which agent it showed each line to (#509)**: `ledger_lines` was keyed `(scope, line_hash)` with no agent anywhere, and the project scope is the working directory string alone. Two agents in one repo therefore write into one history and read from it, so a fold can hand agent B a `from an earlier session` marker for bytes only agent A ever received, and nothing in the data could say how often that happens.

  **Recorded, not keyed on, because the measurement decides that.** `PROJECT_FLOOR_MULT = 3` was calibrated in #448 on cross-session repetition within one agent, and keying the scope on `(project, agent_id)` would end the cross-agent case along with whatever reuse in it is real. On the maintainer's corpus 1 of 28 project paths carries two agent ids and the second is `terminal`, so the effect is latent rather than live, which is the argument for a column now and a decision later. `INSERT OR IGNORE` keeps the first writer, so an existing row names the agent that was actually shown that line rather than the last one to repeat it. The id is the caller's resolved value, never `detect_agent_id()` at the write: a Codex payload arriving while `CLAUDECODE` is set answers `claude_code` to the naive check, which is the exact distinction the column exists to make.

### Changed
- **Four diagrams, drawn from the code rather than from the docs**: where OMNI sits, in the README; the pipeline, on `The pipeline, stage by stage`; the ledger's fold decision, on `The ledger`; and one tool call's two hooks, on `Hooks`. Sources live in `docs/diagrams/` and export to `docs/website/src/media/`, so the site and the README read one file rather than two copies that drift. They are drawn in the manual's own tokens instead of a diagram-tool default.

  **Drawing the pipeline found the page describing it backwards.** `Collapse` was documented as running before `Distill` and feeding it markers. That was true until #116 and wrong for the two releases since: both hooks score and distill the **raw** bytes, and the collapsed form is used only when the distilled one misses `beats_guardrail`. The prose and the stage summary are corrected. `kubectl_table_distills_from_raw_not_collapse_markers` was already pinning the real behaviour, so the code was right and only the description was wrong.

  `media/architecture.svg` is deleted. Nothing referenced it, and it carried the drop shadows and mono-for-everything the new set avoids.

- **Any process in the group could be credited as the command that produced stdin (#516)**: `detect_sibling_command`'s first pass returned the first non-shell line of `ps -o command= -g <pgid>`, with no test that the process had anything to do with the payload. Two failures came out of it, and the second is the one with the row count.

  **Output stopped being deterministic.** Benchmarked through a Node harness whose process group holds an `esbuild --service=... --ping` daemon, a 4,421 byte application log was attributed to esbuild and folded to 129 bytes on one run, then passed through unchanged on the next: same binary, same stdin, same machine, minutes apart. `AGENTS.md` requires deterministic hook output and `ledger` opens by calling determinism a rule it must not break; this sat above both.

  **Its own shell exclusion never fired.** The guard tested `starts_with("zsh ")` and Claude Code invokes `/bin/zsh -c`, so **282 of 289 pipe-path rows in the maintainer's database recorded `/bin/zsh` as the producing command**, dragging the whole shell-snapshot wrapper into `filter_name` and `command`. Re-running the same benchmark on the fix moves all 123 rows from the esbuild path to `[pipe]`.

  The pass is gone. What remains parses an actual `cmd | omni` pipeline out of the group's command lines, which is evidence rather than proximity, and now lives in a pure `pipeline_in` that a test can drive.

- **The ledger page now says how it remembers (#511)**: it explained what the ledger is worth and which claim each marker makes, and never store, retrieve or forget. Added the premise the whole design follows from, a flow of one command through the stage, the two tables and why hashes and bytes live apart, retrieval as an exact lookup with no ranking or candidate set, and the three ways it forgets: compaction drops the session scope, 30 days prunes both, and nothing is evicted by size. Also what two agents in one repo share, which is the project scope keyed on the directory alone (#509), and the corresponding note on `Reading the markers` and `Memory across sessions`.

  The page carried a warning about #465 as a live defect. It has been closed since 0.7.3, so the text now describes the fix, that only delivered lines are recorded, as part of the flow.

- **The README now demos the half we win on (#503)**: the existing pair shows `git log` compression, which is filter work, and filters are 2.7% on our own corpus against rtk's 6.2%. A new pair shows the ledger instead: the same file read twice in one session, where the second read comes back as one marker and a retrieval handle. Measured on 0.7.3 through the Homebrew binary, `cat src/guard/limits.rs` twice is 7.6 KB then 214 B, a 97.2% reduction on the second call.

  **Figures re-measured on 0.7.3 across the manual and the translations.** Same 6,656-trace corpus, so the only variable is the build. The aggregate is unchanged at 14.9% and three cells moved: other 6.8 to 6.9, file read 25.2 to 25.0, `git` 22.3 to 22.1. The tape carries `unset OMNI_PASSTHROUGH`, because `vhs` inherits the recorder's environment and a stray passthrough records a second read that never folds, which is the claim backwards.

### Removed
- **The TOML filter layer is gone (#505, finishing #449)**: `signals/` (48 files), `src/pipeline/toml_filter.rs` (1,364 lines), the `rust-embed` dependency, `omni learn`, the `omni_learn` MCP tool, `omni doctor --test-filter / --benchmark / --coverage / --validate`, and the stream-mode path that only TOML filters could select.

  **Re-measured on 0.7.3 over the same 6,656-trace corpus with `load_all_filters` stubbed to empty:** the aggregate is 2.7% / 14.9% with the layer and 2.7% / 14.9% without it. Ledger-arm bytes move 5,506,126 to 5,508,144, so the whole layer was worth **2,018 bytes over 6,656 commands, 0.031% of the corpus**, and infra scored better without it (4.4% / 8.2% becomes 4.7% / 8.5%). Measured by removal on the release binary, 40 interleaved runs each, it cost **5 to 7 ms per hook** against a 10 ms budget, with p90 going 21.4 ms to 10.5 ms.

  **Reproduced on this branch**, 40 interleaved runs of the release binary against a 121-line `kubectl get pods -A` payload: median 16.5 ms to 11.1 ms, p90 18.0 ms to 12.3 ms, and both binaries emit a byte-identical 122 B, so the Rust distiller does the same job unaided. The three removals together take the release binary from 9,925,456 to 9,173,952 bytes.

  **What replaces it: nothing.** A tool that needs handling gets a Rust distiller, which is snapshot-tested and cannot be shadowed by whichever regex matched first, the failure #110 filed twice. `omni_find_noise` still reports recurring patterns, now as a finding with its sample line rather than as a TOML snippet to paste. `src/session/correction.rs` went with it: it was reachable only from `omni learn`.

- **clap, and the second dispatch table it needed (#506)**: `OmniArgs` and `OmniCommand` were 141 lines of derive, and 15 of 17 arms matched `Some(OmniCommand::X { .. })`, discarded every field clap had filled in, and handed the raw `env::args()` to a module that parsed it again. 18 files under `src/cli/` do that second parse. What clap actually decided was seven hidden boolean flags, which is now a 15-line scan.

  **Collapsing the two dispatch tables found a real gap.** `remember` and `goal` existed only in the clap arm and were absent from the `external_subcommand` fallback, so anything clap failed to classify as a subcommand reached `unknown` and exited 1. `omni init --curser` had the same shape in reverse: the fallback's `init` arm was `let _ = run_init(&args)`, swallowing the error #151 was filed for while the clap arm handled it correctly. Both are fixed by there being one table.

  **`omni remember` was the one subcommand that used clap's result**, so it hand-parses now through the same `check_flags` every other subcommand uses. Its `--project-scoped` was `default_value_t = true` on a plain bool, meaning the flag could not change anything and global scope was unreachable despite the help text calling it the default; `--global` is the half that was missing.

- **Three dependencies that were paying for nothing (#507)**: `is-terminal` is `std::io::IsTerminal`, stable since 1.70 against a pinned 1.97, and `src/hooks/pipe.rs` was the only importer while every call site in it already wrote `std::io::stdout().is_terminal()`. `tempfile` was listed under both `[dependencies]` and `[dev-dependencies]` while every use of it under `src/` sits inside `#[cfg(test)]`. `benches/pipeline.rs` and its `[[bench]]` target were 50 lines behind `criterion` that neither CI nor the `Makefile` ever invoked, for a method the project measured and rejected: the shipped latency figure came from A/B on the release binary after a unit-test timer overstated the same number by roughly 2x.

  `cargo tree --edges normal` goes 220 to 218 unique crates, and the full tree 265 to 237. No runtime behaviour changes.

### Fixed
- **`omni init` exited 1 with no tty, so the one documented setup line was the one an agent could not run (#528)**: the no-flag path opens a `dialoguer::Select`, which can only fail without a terminal, and the failure read `IO error: not a terminal` and named no remedy. It now configures the host that ran the command, read from the environment the same way the exec and pipe paths already read it, and says which host it picked and how to pick another. A host it cannot name, a plain shell included, gets an error listing `omni init --claude`, `--all` and `--help`.

  **Falling back to `--all` was the issue's first suggestion and was not taken.** `--all` is not detection: it writes into fourteen hosts and drops a `.vscode/mcp.json` in the working directory, which is a large silent side effect to trigger on stdin happening to be a pipe. Installing into a host nobody asked for is the worse of the two failures.

- **`key={item}` and `PASS=$((PASS + 1))` were delivered as `[REDACTED]` (#530, #532)**: #486 gave the value a say in the redaction decision and taught it exactly one shape, a lowercase identifier, so two whole families of line were still destroyed by a key name. React requires a `key` prop on every list item and a formatter puts it on its own line as soon as the element has a few attributes, which makes `key={item}` the most common `key=` line in a `.tsx` file. The redacted form is not valid TSX, so an agent that reads a component to edit it and writes it back produces a syntax error. A brace expression is now delivered as written, scoped to the weak `KEY` pattern so a JSON-valued `GOOGLE_CREDENTIALS` is still cut.

  **The second one was in this repository's own suite.** `tests/smoke_test.sh` read back `PASS=[REDACTED]` for its pass counter, because `PASS`, `TOKEN` and `AUTH` are strong patterns whose value deliberately gets no say. A shell expansion names a value instead of holding one, so unlike the identifier rule this one is safe for every pattern: `$((...))`, `${...}`, `$(...)` and `$NAME` survive whatever the key is called, while `PASSWORD="$3cr3t!"` and `PASSWORD=$3cr3t!` are literals that happen to start with the character and stay redacted.

- **`OMNI_PASSTHROUGH=1 <cmd>` did nothing on the Claude Code hook path (#534)**: the manual documents that prefix as "raw output, every time" in four places, including `OMNI_PASSTHROUGH=1 kubectl get pods -o yaml`, which is exactly the case where someone needs exact bytes. It held for `omni exec` and the pipe, where the variable is read by the same process, and not on the host the promise matters on: a `PostToolUse` hook is a process the host spawned, so it inherits the host's environment and never sees an assignment typed in front of a command. The escape hatch was inert and nothing said so.

  **The command string does reach the hook**, so the assignment is honoured by reading it there. Only a leading assignment counts, the position a shell would apply it in, so `echo OMNI_PASSTHROUGH=1` mentions the name without setting anything and is still distilled. Proven at the hook boundary rather than on the predicate, because the predicate was always right about the text it was given: `the_prefix_the_manual_documents_reaches_the_hook` feeds `process_payload` the same output twice and asserts the prefixed call returns nothing where the bare one rewrites.

  **The first version of that test passed its own control by accident**, on a fixture that was never rewritten at all, which would have made the whole assertion vacuous. It now primes the ledger first so the unprefixed call is certain to be folded.

- **A worktree was a different project, so its ledger history was too (#525, first step)**: the project scope was `current_dir()` and nothing else, so the same repository reached by another path opened a separate history. Measured before the fix: 31 project scopes on the maintainer's database, one repository split across **six** of them, with 2,759 lines (15.6% of that project's history) stranded in scopes nothing would consult again. Four of the six were created in a single day of ordinary work, because a git worktree is a different directory. `paths::project_key` resolves the repository root and both ledger doors use it.

  **A linked worktree is why this is not a walk up to `.git`.** There, `.git` is a *file* holding `gitdir: <main>/.git/worktrees/<name>`, so stopping at the first `.git` returns the worktree root and splits the history exactly as before while looking like a fix. The gitdir is read and resolved back to the main checkout, and an unreadable or unfamiliar `.git` file falls back to the directory rather than to a guess. Proven by replacing the resolution with the naive walk, which fails `a_linked_worktree_answers_the_main_checkout`.

  **`distillations.project_path` still records the working directory**, deliberately: changing it would silently restate what every existing row means. The remaining steps in #525, what the agent column is for and the marker wording that cannot say who, are unaffected by this and still open.

- **Two ledger markers the manual never mentioned (#526)**: #522 added a second pair of forms, for a fold that covers the whole reply, and `use/markers.md` kept listing two of the four the code can print. A reader met `[OMNI: identical to …]` in their own output with nowhere to look it up. Both forms are documented now, with the distinction that decides which appears: `identical to` is a reply repeated in full, the plain wording is a gap inside one.

  **The gate added in #520 could not see it**, which is the more useful half. It checks commands named in shell fences and counts stated in prose; a marker is neither. `every_marker_the_ledger_can_print_is_documented` now matches every `[OMNI: …]` template in `src/ledger/mod.rs` against that page, on the stable half before the first `{` since the rest is a runtime count and a handle. It is the cheapest of the three checks because the templates are string literals in one file. Proven by deleting the two new forms from the page: the test names them back.

- **A 333-line markdown spec arrived as 18 lines (#523)**: the `readfile` distiller's last arm was `distill_unknown_file`, 15 lines of head and 5 of tail, which parses nothing. Every extension without a structural distiller landed there, so a design document read before implementing from it came back as its title and its closing caveats with the 309 lines that said anything dropped. It is the rule this project holds every other distiller to, broken in place: a distiller that could not parse its input returns the input, and slicing by position is a confident answer about content nothing understood. In a log the first and last lines carry the run and its verdict, which is why `.log` keeps a summariser; in prose they carry neither. Unparsed content now passes through whole.

  **Fixing it exposed a bigger one, in the opposite direction.** The three non-Bash arms chained `fold_cross_turn` onto the distiller's `Option`, so a payload the distiller declined never reached the ledger either. Cross-turn folding is the largest single source of savings and file reads are the class it earns most on, so the first version of this fix silently disabled it for every prose read: `folds_a_repeated_read_across_turns` going red is what caught it. Read, Grep and WebFetch now share one `reply_through_ledger`, and the ledger runs on what the agent is about to receive whether or not a distiller understood it. It returns `None` when neither stage changed anything, so the host is never handed an identical copy and a saving that did not happen.

  **Not measured, because this path records nothing.** `distillations` holds zero readfile rows, so neither the cut being removed nor the folding being gained could be priced from the corpus. That gap is worth its own issue.

- **A re-run of an identical command came back as two markers and zero content lines (#519)**: the ledger folded the whole payload, then the rewind marker measured "what survived" *after* that fold, counted the ledger's own marker as surviving content, and reported the same loss again under a second handle. A 43 line re-run delivered `43 lines already shown` followed by `42 lines omitted`, two ids, and an arithmetic that reconciles with nothing. Same defect as #301, one stage further up: the counts now describe the distiller, taken before the ledger runs, and the ledger accounts for itself.

  **A fold covering the whole output now says so.** `N lines already shown` is a claim about a run and reads as one, so a fully elided reply was indistinguishable from a command that printed nothing. It renders `identical to the N lines already shown` instead, which is the fact a re-run was asking for. The affordability test weighs the longer string, because the first draft weighed the short one and emitted the long one.

  **The issue's primary suggestion was measured and rejected.** "Do not apply already-shown dedup to a command issued again in the same session" would take out 861 of 3,983 calls carrying **88.4% of input bytes and 96.2% of all savings** on the maintainer's corpus, to fix a pathology that affects 20 of those 861. The issue sanctions the alternative taken here: one marker, one id, counts that match the payload. Replayed on one corpus, before and after are identical at 35.3% bytes and 39.1% tokens.

- **A rewind pull counted only when it came through MCP (#512)**: `get_retrieve_rate` raises the route thresholds for a command family whose full output keeps being needed, and it reads `retrieve_events`. Only the MCP tool wrote there, while the marker prints `omni retrieve <handle>`, a shell command, so the mechanism advertised the door it did not count. Measured before the fix on the maintainer's install: 49 pulls on `rewind_store.retrieved` against 19 rows in `retrieve_events`. Both doors now call one `record_rewind_pull`, kept out of `retrieve_rewind` on purpose, because `store::query` reads archived content to answer reports and counting those would inflate the same rate from the other side.

  **The rows that did land were keyed on a family nothing could ever match.** `command_family` returned the first whitespace token for anything unrecognised, so `OMNI_DB_PATH=/tmp/<uuid>/d.db cargo test` filed under the path itself, a per-invocation string, and 6 of the 19 existing rows are that shape. Leading `KEY=value` assignments are now skipped, with the key constrained to an environment name so `--replace=x` and paths carrying an `=` are left alone.

- **The 50 KB safety truncation kept the head and cut the answer (#508)**: on a 1,400 line synthetic log the cut returned 817 routine `status=200` lines and removed the single `RuntimeError: FATAL: connection pool exhausted` line, which was the last in the stream, under a footer reporting 42% saved. On the stdout of a failing command the answer is at the end: the stack trace, the exit reason, the last error before the process died. The cut is now a middle elision that reserves a fifth of the budget for the tail, so both ends survive.

  **The elided middle is archived and the marker names the handle**, which was the half of #219 that never landed. Content over `MAX_REWIND_BYTES` is not offered to the store, and the marker then says nothing rather than promising a retrieval that would fail.

## [0.7.3] - 2026-08-12

Three fixes about mistaken identity. A tool name, a session and a command's output
were each taken for something they were not, and in every case the substitution was
invisible from the outside: the tool name scored a *higher* saving on the wrong path,
two sessions merged without an error, and a probe whose answer had been deleted read
exactly like a probe that found nothing.

### Fixed
- **A snake_case tool name never reached its own distiller (#488)**: `distil_tool_reply` matches `Bash`, `Read`, `Grep` and `WebFetch` exactly. The Pi and OpenAI-shaped branches normalised the name before that check and the ClaudeCode branch, the shape a third-party integrator is most likely to send, passed it through, so `bash` fell to the generic arm and reached neither the per-tool distiller nor the ledger.

  **The failure looked like success, which is why it lasted.** The generic arm still shortens, and on a real shell corpus `bash` reported 59.9% against `Bash`'s 37.6%: the wrong path scored better because it was keeping the first thirty lines and discarding the rest, where the right one folds only what the agent has already been shown.

- **Two sessions minted in the same millisecond were one session (#490)**: the id was `timestamp_millis()` alone, and `INSERT OR REPLACE INTO sessions` kept one of the pair. On a machine running several agents, two `SessionStart` hooks landing together is exactly the case. The id is now `{millis}-{pid}-{counter}`, all stdlib. The millisecond stays first because four call sites slice `session_id[..8]` for display.

  This does not make the id meaningful. It is still minted state rather than an identity the host supplied, which is why the ledger scopes on `host_session()` instead.

- **`kubectl run` container stdout was judged as kubectl output, so every payload line was dropped (#497)**: `wraps_another_command` recognised `exec` and not `run`, so `kubectl run --rm -i -- <cmd>` was routed to the kubectl grammar. An eight-line probe came back as `pod "omni-repro" deleted` plus a marker: the one line kubectl prints itself survived and the eight the command was run for did not. `run` now counts as a wrapper for `kubectl`, `docker` and `podman`, the same as `exec`.

  The failure is silent in the direction that matters, because a probe returning nothing is a plausible result and nothing in the delivered text separates it from an answer that was thrown away. **The two doors disagreed**: the same payload through `hooks::post_tool` was never rewritten, so only `hooks::pipe` destroyed it, which is why a post-hook probe reported no defect. The corpus cannot arbitrate the widening to `docker` and `podman`: it holds two `podman run` and no `kubectl run` in 6,656 commands.

## [0.7.2] - 2026-08-12

The release that stopped four published numbers from being wrong, and paid 0.06% to
stop the ledger claiming a sighting it never had.

### Fixed
- **A folded run was recorded as shown, so the next fold lied about it (#465)**: a run replaced by a marker never reached the context, but every line of it was written into the session scope anyway. The next occurrence in that session therefore read as `Origin::Session` and said `already shown` about bytes the session had only ever seen as a pointer. The wording is the smaller half: `Origin::min_gain` charges `MIN_LEDGER_RUN_GAIN * 3` for a project fold, because the agent has no say in the retrieval it may have to pay for content it has never seen, and `MIN_LEDGER_RUN_GAIN` for a session fold, which is free. Mislabelling the second sighting dropped the bar from 450 bytes to 150, so **the false claim made the ledger three times more willing to fold, on a premise that had just stopped being true.** `substitute` now returns the indices it replaced and `project_inner` records the complement.

  **Telling the truth costs 0.06%, measured.** Over the 6,656-trace corpus the ledger arm moves from 5,502,733 to 5,506,627 bytes, with 814 session and 95 project markers where it was 817 and 90: folds that were falsely session-origin are now correctly project-origin and some no longer clear the higher bar. The aggregate still rounds to 14.9%.

- **The benchmark reported a floor the code does not use (#472)**: `bench_replay` printed `floor_mult=6` while `Origin::min_gain` applied `MIN_LEDGER_RUN_GAIN * 3`. The 6 was correct when #448 chose it as a starting point and stayed behind when the replay moved the arm. Not a wrong measurement, a wrong label on a real one, on the line anyone tuning the floor reads to confirm which arm they just ran. The multiplier is now `guard::limits::PROJECT_FLOOR_MULT`, used by the ledger and printed by the harness. `OMNI_PROJECT_FLOOR_MULT` is gone with it: nothing in `src/` ever read it, only the harness's own `println`, so setting it relabelled the output without moving the arm.

- **The published 15.4% did not reproduce on the shipped code (#473)**: re-replayed over the same 6,656-trace corpus, the aggregate is **14.9%**, and every class carrying ledger folds moved with it (file reads 26.3 to 25.2, git 22.9 to 22.3, search 13.5 to 13.3, other 7.1 to 6.8, build and test 78.3 to 78.0). The cause is #458 in this same release: the ledger no longer folds a line that states a failure, so a repeated `TypeError` survives a re-run instead of becoming a pointer. **Refusing to fold the error channel costs half a point of ratio**, which is the trade this project says it makes whenever correctness and compression conflict. What rules out a different corpus: the rtk arm reproduced its published figure to the byte. Two errors of a different kind went with it: the README named the window as 2026-08-03 to 08-10 while the corpus covers 08-04 to 08-11, so a reader taking the window at face value could not have reproduced anything, and the derived "12.7 points on top of our filters, 11.9 on theirs" was left behind by an earlier recomputation. It is 12.2 and 11.4.

- **Every id selector in the manual's theme was dead (#477)**: mdBook 0.5 prefixes its ids with `mdbook-`, and the theme targeted the old names, so `#sidebar`, `#menu-bar`, `#searchbar`, `#searchresults`, `#theme-list` and the three theme entries matched nothing: 8 of 8. The palette survived because `.light` and `.navy` are class selectors whose variables mdBook's own stylesheet consumes, which is exactly why it went unnoticed. It also meant the rule hiding the three themes this design does not cover never fired, so Coal, Rust and Ayu were live in the picker rendering in mdBook's colours. Separately the manual linked home from nowhere: 37 pages, zero links back to the site serving them, so `Docs` in the site navbar was a one-way door. `theme/siteframe.js` makes the wordmark the way home and adds Site, Releases, Journal and Discord, reading its depth from mdBook's own `path_to_root` rather than forking `index.hbs`.

- **CI never compiled `plugins/pi` (#464)**: `make ci` is all Rust, and nothing ran `tsc` or `npm` against the extension Pi actually loads, so a TypeScript **major** bump merged on a green matrix. #461 took typescript from 5.9.3 to 7.0.2 and passed all three checks; the bump was fine, and the matrix could not have said either way. Same shape as #423, an `undici` advisory in the same package that `cargo audit` structurally could not see. The command lives in `package.json` as `scripts.typecheck` so CI and a contributor run the same thing, and it is `npm ci` so a bump is checked against the lockfile the PR proposes.

- **`omni stats --detail` named an agent it had never resolved (#471)**: on a database whose every row was `claude_code`, 9 of the 10 rows in By Command were labelled `Terminal`, and the Agent Distribution section three lines below reported `Claude Code 100.0%`. One command contradicted itself on one screen. The cause was two shortenings a single column apart: rows were grouped for display with `shorten_command(cmd, 18)` and the agent map was keyed with `shorten_command(cmd, 19)`, so every command whose two-token prefix ran past 18 cut at a different place on each side and the lookup missed. `.unwrap_or("Terminal")` then reported the miss as a fact. Both sides now read one `CMD_KEY_WIDTH`, and a miss says `Unknown`, which the file's own comment at `agent_display_name` already argued for: "a human ran this in a shell" and "OMNI could not tell" are different facts and only one is actionable.

  A second cause was underneath. `get_per_command_with_agent` carried `LIMIT 200`, applied to **raw** commands, while the caller folds those into shortened keys afterwards. On the measured database that cap kept 200 of 1,064 pairs, and `node -e` was 21 separate one-call rows that add up to the displayed `21x`, every one below the cut. The limit is gone: the bound is the retention window, which is the honest one for a report command. Verified against a 1,099-row database, where all ten rows now read `Claude Code` and match `SELECT agent_id, COUNT(*) FROM distillations GROUP BY agent_id`.

- **A re-run of a failing script came back with no error in it (#458)**: the ledger folded a `TypeError` and its stack frame because both lines had been shown earlier in the session, so the second run of a still-broken `bun` script reached the model as source context and nothing else. An agent re-running a command to check a fix reads that as the failure being gone. It is the fabricated-success mode, reached through cross-command dedup rather than through a distiller mislabelling its own surface, which is why it can hit **any** command whose error text recurs. The reporter could not reproduce it synthetically and said so rather than guessing; the real payload was still in `execution_traces` and reproduced it exactly.

  **The heuristic is sound for informational lines and wrong for the error channel.** Repetition is the signal there: the same `TypeError` appearing again means the bug is still there, and that is the line worth spending tokens on. The ledger now marks any line `semantic::carries_failure` recognises as unfoldable, so it survives verbatim and the run splits around it, leaving the repeated frames either side still foldable. That predicate is the scorer's own, exposed rather than copied, because two copies of one rule drift and only one of them gets reported (0.7.1 shipped three fixes of exactly that shape).

  **The second layer is what makes this the last time.** `pipeline::fidelity::preserves_failures` is a post-condition: every invariant in this pipeline constrains what a stage is *given*, and none asked what came *out*. If the input stated a failure and the reply no longer does, both entry points hand the bytes back. It catches stages that do not exist yet, and the distiller class #333 and #354 were filed under.

  Measured on the reporter's exact payload: on 0.7.1 the second run contains no `TypeError`; with the fix it keeps the error, the frame and the version line, and folds only the repeated source context. **The cost is 0.5 points of aggregate, 15.4% to 14.9%**, and markers rise from 772 to 817 because runs now split around error lines. That is the trade, stated rather than buried: half a point to never elide the answer.


### Added
- **A second competitor arm in the benchmark (#468, partial)**: the head to head had one competitor, so it answered "which of these two" rather than "how does this compare". lean-ctx is drivable through `compress diff - --shell "<cmd>" --json`, which takes the payload on stdin and is told the producing command, the same courtesy the rtk arm extends. Over 6,656 traces: omni 14.9% with the ledger, rtk 6.2%, lean-ctx 6.1%. **rtk and lean-ctx land a tenth of a point apart from opposite shapes**, rtk averaging 461 bytes off each of 872 commands and lean-ctx 2,950 off each of 134, which the aggregate hides entirely. Two absences are printed rather than left to be inferred: no `lean-ctx + our ledger` row, because its preview reports byte counts and never emits the compressed text, and no headroom arm, because its equivalent is a whole-conversation deduplicator that belongs against the ledger rather than the filters. The arm is off unless `OMNI_BENCH_LEANCTX` names the binary.

### Changed
- **Every framed surface draws one width (#463)**: there were five. 41 columns in `doctor`, `learn`, `session` and `init`, 49 in `stats`, 62 in `patterns` and `query`, 66 and 40 in `diff`, and one 70 in `learn`. None matched the content it framed, so `omni stats --detail` drew a 49-column rule around a 90-column table and `omni doctor` a 41-column rule over a 206-column line. `cli::WIDTH` is now the single number and `cli::print_rule` the single caller, so the next surface cannot pick its own.

  The table rules are built from the column widths they sit under rather than typed as literals, which is what the Agent Distribution separator needed: it carried five groups under a four-column header, 56 columns under a 43-column one, because the leading `──` had been copied from the By Command table above it. The detail table itself is narrower, 69 columns against 90, with the bar column padded so every row reaches the rule instead of ending ragged.

  Three surfaces changed shape rather than width. `omni doctor` prints the tier legend as one line per tier, since as a single 187-character sentence it started at column 19 and wrapped mid-word, and it no longer appends a `Distill tier:` row that the clean-host one-liner already states inline, so the list has one shape. `omni diff` loses its two inner 40-column rules, which sat at a third width under labels that already separate the blocks. `omni engram` bounds its bullets, one of which reached 160 columns.

  `omni retrieve` is framed on **stderr**, and stdout still carries the archived bytes untouched. #463 asked for a header, a line count and a footer on this surface; putting them on stdout would have delivered that by editing bytes a caller is about to paste or parse, which is the rule the pipeline holds itself to.

  The web dashboard prints `216K` where it printed `216444`, from `format_exact_tokens`, which was already public two files away, and gains Top Commands and Agent Distribution from the same `filter_breakdown` and `get_agent_breakdown` calls that feed the CLI, so its footer's claim to show "the same figures as `omni stats`" is true rather than aspirational.

- **Relicensed from MIT to Apache 2.0 (#467)**: `LICENSE` is now the verbatim Apache License 2.0, with a `NOTICE` file carrying the copyright, since section 4(d) only has effect when one exists. `Cargo.toml`, the README badge and licence line, and all six translations move together, because a repository stating two licences at once is worse than either. Apache 2.0 adds an explicit patent grant from contributors and requires a redistributor to state what they changed. It does **not** restrict copying: it is permissive in the same way MIT is, and a fork may still close its source. That is recorded here so nobody later relies on a protection it does not provide. Releases up to and including 0.7.2 were published under MIT and that grant is irrevocable for the versions it covered.

- **Contributor documentation is one file instead of three (#466)**: `CLAUDE.md` (387 lines) and `AGENTS.md` (45) were addressed to AI assistants while `CONTRIBUTING.md` was 51 lines of pointers, so a human contributor read the thin one and never reached the standards CI enforces. All three are now `CONTRIBUTING.md`, and the repository ships `README.md`, `CONTRIBUTING.md`, `CHANGELOG.md` and `SECURITY.md`.

  Four claims were wrong and are gone with the files that carried them: `docs/HERMES_OMNI_INTEGRATION.md` told readers to run `omni doctor --fix` to trust a project `.omni/filters/` tier that #447 and #449 deleted, `docs/ROADMAP.md` offered "a TOML filter for an internal tool" as a contribution path that no longer exists on disk, and both it and `docs/ARCHITECTURE.md` linked to a `DEVELOPMENT.md` that never existed. The "27 OMNI tools" figure in the Hermes guide was wrong too, by one: that count came from grepping `"omni_[a-z_]+"` in `src/mcp/server.rs`, and `omni_auto_noise` at line 1036 is a filter name passed to `generate_toml`, not a tool. The server's own `tools/list` answers **26**, and `tools/call` on the extra name returns `-32602 tool not found`.

  `README.md` and the six translations linked to `docs/ARCHITECTURE.md` and `docs/BENCHMARKS.md` in 21 places, and those links were what backed the published savings figures. The per-class table, the rtk head to head including the row we lose, and the reproduce command now live in `README.md` itself, so no public claim is left without a way to check it. Seventeen code comments citing `CLAUDE.md`, `AGENTS.md` or `ROADMAP.md` as the authority for a rule were repointed. The `AGENTS.md` and `CLAUDE.md` entries in `guard::config::pinned_files` and `hermes_default_config` are untouched: they name files in a *user's* project, not this one.

## [0.7.1] - 2026-08-11

Two installer defects, both found by setting 0.7.0 up on a real machine rather than by a
gate, and both invisible to `omni doctor` until now.

### Fixed
- **`omni retrieve` re-entered the pipeline it exists to escape (#456)**: the marker tells the agent to run `omni retrieve <handle>`, and that command's own stdout went back through the Bash post-hook like any other output. The recovered content came back as a **fresh marker with a fresh handle**, so following the instruction produced another instruction and the original was unreachable past the first hop. Filed the day after #452 made the command exist, and worse than #452 was: there the escape hatch was unreachable, here it loops. The post-hook now declines any command that exists to hand archived bytes back, `omni retrieve` and `omni diff`, and records the passthrough with its own reason. Matched on the token after the program rather than by substring, so `grep retrieve` and a file called `retrieve.md` keep being distilled. Reproduced through `omni --post-hook` before the fix and after, with `cat` on the same payload as the control: the retrieval now emits nothing and the host keeps the bytes, while `cat` still folds.

  **The same loop existed behind a second door, and it was not reported.** Probing the sibling path after fixing the reported one found `omni exec omni retrieve <handle>` doing exactly the same thing through `hooks::pipe`, which had no such guard: `[OMNI: 19 lines omitted, ...] [OMNI Active] 15.6% reduction`. It also still printed the **old** `omni_retrieve("...")` marker, the MCP-only form #452 was filed about, because that fix changed `post_tool`'s copy of the string and `pipe` carried its own. Both are corrected, and the guard is one function shared by the two paths rather than two copies that can drift apart the way the marker did.
- **`omni init` registered OMNI twice when the binary moved (#454)**: `ensure_hook` compared the command it was about to write against the ones already there byte for byte, absolute path included, so reinstalling from a different build matched nothing and appended. A machine set up twice ran **two OMNI processes per hooked call**: the pre-hook rewriting a command the first pass had already rewritten, the post-hook distilling output the first pass had already distilled, and a second row recorded for it. That is #379's double counting arriving through the installer. Identity is now the binary name plus the flag, compared as a whole token because `--pre-hook` ends with `-hook` and a substring test would collapse the two into one. A match rewrites the command to the current path, which also makes an upgrade that moves the binary a no-op rather than a duplication. **Gemini CLI carried its own copy of the same wrong test** and nobody had reported it, so `is_our_hook` now lives in `agents/mod.rs` and both hosts have a test that installs from two paths and asserts one hook per event. Cursor and Codex were already correct: both purge their own entries before writing. Verified end to end as well as in unit tests, two binaries against one temp `HOME`, eight events, one hook each.
- **`omni doctor` reported `[OK]` for an install that ran OMNI twice (#454)**: it asked whether a hook was present and never how many. It counts now and warns per event, naming how many times that event would run OMNI. The installer no longer creates the state; this is for the machines already in it.
- **`omni init --all` was documented as Claude-only and configures fourteen hosts (#455)**: the help listed it under `CLAUDE SPECIFIC FLAGS` as "Perform full Claude setup (hooks + MCP)" while `init.rs` hands it the full agent list. One of those writes lands in the working directory, because VS Code's config path is `.vscode/mcp.json` relative to the cwd, so running it inside a repository writes into that repository. The behaviour is kept, since it is what someone typing `--all` wants, and the help now says both things: the group is named for what it holds, and the flag names the file it drops where you are standing.

### Documentation
- **The recovery path is named as the CLI everywhere**, not as the MCP tool. `README.md`, `docs/ARCHITECTURE.md` and the Hermes integration guide had the agent calling `omni_retrieve`, which is reachable only where MCP is wired; that is what #452 was. All three now name `omni retrieve <handle>` first and the MCP tool as the alternative.
- **`omni retrieve` and `omni dashboard` are documented** in the README and all six translations, which shipped in 0.7.0 without an entry anywhere a user reads.
- **`CLAUDE.md` stopped telling contributors to create `~/.omni/signals/my_signal.toml`**, a path 0.7.0 no longer reads, and its Security section stopped describing the trust gate that was deleted with the tier it guarded.
- The Hermes guide's learn section says what `omni learn` output is for now that there is nowhere on disk to put it: a finding, and an issue.


## [0.7.0] - 2026-08-11

The long-term support release. Three things define it: the ledger stops guessing
whether a fold is worth making and measures it, the surfaces stop reporting numbers
the project had already retired, and a config tier that could let a repository edit
what a visitor's agent sees is deleted rather than repaired.

### Added
- **`omni retrieve <handle>`, because a marker promised a tool half the hosts cannot call (#452)**: reported from a real session. A 50 line span of prose came back as `[OMNI: 50 lines already shown, omni_retrieve("cd900c16a4a94eb2")]`, and `omni_retrieve` is an MCP tool; on a host where it is not surfaced the agent was left with a hole in the file it had just read and no way to fill it. That is precisely the defect #388 said this mechanism cannot have. Every marker now names the CLI instead, which exists everywhere: the ledger's two forms, the rewind marker, and the search distiller's guard line. It accepts the handle bare, quoted, or still wrapped in the old `omni_retrieve(...)` call, because a model handing the marker back verbatim is the likeliest way it is called, and an unknown handle says why by naming the 30 day working tier rather than reading as a broken archive. The MCP tool still works where MCP works; the promise no longer depends on it. Side effect worth stating: the marker is 62 bytes rather than 65, so the gain rule folds slightly more.
- **`omni dashboard`, on loopback, over the numbers `omni stats` prints (#451)**: the terminal report is a snapshot and the meters #357 promoted are trends, which is the one shape a table in a terminal cannot show. `std::net::TcpListener`, one thread, one page, **no dependency added and no JavaScript**. It binds `127.0.0.1` and answers `GET` only, because the database holds command output and none of it should leave the machine. Every panel reads a `Store` method the CLI already reads, so the two surfaces cannot disagree; that constraint is the point rather than a shortcut, since a dashboard computing its own numbers would be a second source of truth and this release exists partly to fix one of those. An empty database renders "not measurable yet" rather than a zero that reads as a measurement.
- **`passthrough_events.reason` (#441)**: six guards in `process_payload` return `None` and a distiller returning `None` from the trait boundary is a seventh, and all seven landed in one bucket with the reason concatenated onto the command because there was nowhere else to put it (#254). `GROUP BY reason` answers the question that tagging was invented for. "Declined because the payload was JSON" and "declined because no distiller could parse it" call for opposite work, and the second is the only direct evidence the never-fabricate invariant is doing any.

### Changed
- **OMNI reads no filter file from disk (#449)**, and this is the breaking change in the release. After #447 took the project tier, `~/.omni/signals/` goes too: every signal is compiled into the binary, so the set that runs is the set the inline tests cover and no file on any machine changes what an agent is shown. The measurement is what licenses it: disabling **every** embedded signal moves the filter column by 804 bytes over 6,656 recorded commands, so the whole layer is a rounding error and the external tiers were a share of that. What they cost is not: a second dispatch path, a cache fingerprint over files that may not exist, a load-time difference between a developer's machine and CI, and a class of bug where a distiller is well tested and unreachable. Two writers had to become honest with it, since `omni learn --apply` and the `omni_learn` MCP tool both wrote a file and reported success; with nothing reading that file, reporting success would be the fabrication class this project exists to stop shipping, so both now say there is nowhere to apply to. `docs/FILTERS.md` is rewritten for contributors rather than users: a signal that lands in the repository ships in the binary for everyone.
- **Six MCP-only host adapters become a table (#443)**: 889 lines across `roo_code`, `opencode`, `copilot`, `antigravity`, `zed` and `vscode` that differed in three fields, where the config lives, which JSON key holds the server map, and what to call the host. `install`, `uninstall` and `doctor_check` were the same code six times, so a seventh host meant a seventh copy and the cheap path was the wrong one. **802 lines deleted, 346 added**, and adding an MCP host is now a row in `mcp_host::HOSTS`. Zed keeps `context_servers` and VS Code keeps `servers`, asserted by a test along with the property that no two hosts resolve to the same file. Deliberately outside the table: `cline` purges hook names Cline never emits and has a test for it, `openclaw` copies a plugin directory, and the Full tier hosts genuinely differ.
- **Three stages out of `process_payload`, 701 lines to 524 (#442)**: every host reaches the pipeline through this function and the entry conditions could not be read without scrolling past the pipeline itself. `declined()` holds the six gates that decide whether a payload may be touched at all, `trim_enormous()` is the 2 MB head and tail cut and is now a `Cow` so an ordinary payload is not copied to be handed back unchanged, and `distil_tool_reply()` holds the non-Bash arms that return before the Bash rewind block and therefore each owe the archive guarantee themselves. Behaviour is unchanged and the suite is the proof: 1,687 tests pass either side of the move. The remaining 524 lines are the Bash pipeline itself, one sequence over one set of locals, and splitting it would mean threading a context struct through six stages to make a line count smaller.

### Changed
- **The ledger folds a run when it pays for its marker, not when it clears two proxies (#450)**: `MIN_LEDGER_RUN_LINES` and `MIN_LEDGER_RUN_BYTES` asked whether a run was big and never whether it was worth replacing, so a 3-line 150 byte run was refused while nothing anywhere compared a run against the size of the marker that would replace it. Both are gone, replaced by `MIN_LEDGER_RUN_GAIN`: fold only when the run saves 150 bytes **after** paying for its own marker, weighed against the marker rendered rather than estimated, which is possible because a handle is always 16 characters. The marker is shorter too, 87 bytes to 65, and the two compound because a cheaper marker makes smaller runs worth folding. The split that decided this is worth recording: of the repetition the ledger declined, the 4-line bound held 266,005 bytes over 11,406 runs, which is **23 bytes per run against an 87 byte marker**, so that bound was protecting the output rather than costing anything. Measured over 6,656 `claude_code` traces from 2026-08-04 to 08-11: aggregate **13.9% to 15.4%**, the ledger claiming 817,155 bytes over 849 folds against 720,518 over 678. The 100 byte bar was left on the table deliberately: it is worth another 0.6 points and costs 240 more substitutions, which is the same choppiness trade the sweep behind the old constants declined. `MIN_LEDGER_INPUT` drops from 400 to 264 and stops being a guess, because 264 is the smallest payload that can hold a run that pays for itself.
- **`omni stats` leads with session lifetime, and the distillation figure says what it is (#435)**: #357 retired blended distill % as the product number and `ROADMAP.md` calls it a diagnostic for one host's pipeline, not a product claim. `src/cli/stats.rs` never followed, opening with `reduction_pct` while no mode reported anything about how long a session lasts. The headline is now median commands per closed session, the longest, and how many ended at a compaction, read from `session_summaries`, which the `SessionEnd` hook has been writing all along. Sessions with zero commands are excluded, since an unclosed session's count is half-measured and would drag every median down, and when nothing has been recorded the block says it is not measurable yet rather than printing a zero that reads as a result. The period rows keep their place under a heading that names them. The number was never wrong; it was presented as something the project had stopped treating it as.
- **`omni doctor` is 51 lines instead of 101, and every line left carries information (#426)**: 34 of the hundred said `[OK]` and the tier legend arrived in full on every host, which on a ten-integration machine meant the same sentence ten times. The legend prints once and each host names its tier; a host with nothing to report collapses to `Claude Code: Full, 5 checks [OK]`; a host with anything to say keeps its heading, **every row that is not `[OK]`**, and a count of the rest. `--detail` prints everything. The condensing happens at the capture sink #353 added for `--json`, so it is one function rather than an edit to twenty `doctor_check` implementations, and a row that is not `[OK]` is never hidden in either mode, which is what makes the default safe to read.

### Security
- **Project-local signals are deleted rather than re-gated (#447)**: `guard::trust::is_trusted` hashed `<project>/omni_config.json` and the answer decided whether to load `<project>/.omni/signals/*.toml`. **The hash covered a different file from the one it gated**, so a signal added or edited after a project was trusted loaded with the trust record untouched. A TOML filter carries `strip_lines_matching`, which is enough to hide a failing test or an error line from an agent, so a repository could quietly edit what its visitors were shown. The obvious repair is to hash the directory it actually gates; the deletion is better and is what shipped. The tier is worth nothing measurable, since disabling **every** embedded signal moves the filter column by 804 bytes over 6,656 commands and a project tier is a subset of that, and what it costs is a supply chain the tool does not need. Gone with it: `guard/trust.rs`, the `omni_trust` MCP tool, two dead path helpers, the project row in doctor's filter report and the project half of the filter cache fingerprint. 354 lines deleted against 43 added. The integration test is inverted rather than removed: a signal placed inside a checkout, with `omni_config.json` marking it trusted, must not load.

### Fixed
- **`omni patterns` reported 20 of 20 entries RESOLVED, five of which had fired 27 times (#427)**: `resolve_pattern` marks every unresolved pattern of a tool family at once, so a single green `cargo test` declared five distinct failures fixed, and nothing ever took the label back when they recurred. `upsert_pattern` now clears `was_resolved` and its hint, so **resolved means "has not happened since"**, which is the only reading of that word an agent can act on. Existing databases carry the unearned label and nothing recorded when a resolution happened, so a migration clears it once and lets the new rule re-earn it rather than guessing which of them were true. On the maintainer's database the same command now reads `ACTIVE` for the five recurring entries. The fix-hint half of the issue was closed by #431.
- **Every documented flag on `omni reset` was rejected before the command ran (#444)**: `print_help` advertised thirteen and `main.rs` declared `Reset` as a bare clap variant, so `omni reset --all` failed with "unexpected argument" and `--help` printed clap's one line stub instead of the module's help. It carries the same `trailing_var_arg` shape as `Doctor` beside it now. Passing argv through re-opens the hole #151 closed elsewhere, an unrecognised flag falling into the interactive menu and exiting 0, so the flag list is a `FLAGS` const that `print_help` renders and `check_flags` validates: what the command documents and what it accepts are one list.
- **Hermes uninstall reported success while leaving the install fully configured (#445)**: it removed the plugin **directory** and returned `Ok`, while `doctor_check` twenty lines below has always known there are two installation forms. An install living only in `~/.hermes/config.yaml` was untouched, nothing printed, and `perform_reset` said "All resets completed". It now also removes the OMNI entry under `mcp_servers` and the `[agents.hermes]` section from omni's own config, and says "Hermes was not installed" when there was nothing to do. The removal takes **our entry and not the mapping**, because `mcp_servers` is shared with every other server the user has registered, and the key itself goes only when ours was the last entry under it. Compression stays and the output says so: the installer only enables it when it was off, and after the fact nothing distinguishes that from a user who always had it.
- **`project_hash` had four implementations and one disagreed (#437)**: `agents::multiagent` trimmed the trailing slash; `mcp::server`, `cli::goal` and `hooks::session_end` did not. A project reached as `/foo/bar/` on one path and `/foo/bar` on another therefore had two addresses, and its knowledge, engrams and patterns split across both while `omni_recall` found half. The trimming version is the one implementation and the other three call it, because a trailing slash names the same directory. Paths without one hash identically either way, so no existing row moves.
- **`loop_memory.ttl_days` was declared and never enforced (#438)**: the column has said 30 days since it was added while no `SELECT` filtered on it, no `DELETE` used it, and `cleanup_old` never named the table, so goal memory was a fourth permanent table wearing a retention label. Two local analysis documents and the reporter all read the schema and concluded otherwise. `cleanup_old` now expires per row against the row's own `ttl_days`, keyed on `last_seen`, so a fact the loop keeps confirming keeps its place and only what nobody has reconfirmed expires.
- **The database wipe left `omni.db-wal` and `omni.db-shm` behind (#446)**: SQLite runs in WAL mode, so the database is three files and removing one leaves the other two holding its content. 4.2 MB of `-wal` survived a wipe that reported success, and a stale `-wal` beside a fresh database is the one way this can corrupt rather than merely mislead.
- **Two of the three roadmap axes described a state `main` had already left (#436)**: axis 1 said `require_parsed` was voluntary and called by 3 of 12 distillers, when the invariant moved into the trait in #250 and holds for all 12 by construction; axis 2 said the `PostToolUse` matcher was `Bash` only, when it has read `Bash|Read|Grep|WebFetch` since #246. A set of local analysis documents built a restructuring proposal on the axis 1 text, which is what a stale direction document costs. Both are marked closed **with what is still open under them**, since "closed" alone reads as "stop looking here". Axis 3 is rewritten around what actually remains, that a figure cannot outlive a corpus which prunes at seven days.
- **Every em and en dash is out of the tree**: 52 source files and the tracked docs, in printed strings and in comments. One snapshot moved with it, because a distiller really did emit one in its summary line. The rule is the repo's own and had drifted; a grep is what keeps it.

### Added
- **`OMNI_TRACE_RETENTION_DAYS` holds the verbatim window open for a measurement (#440)**: roadmap axis 3 asks that a published figure be reproducible by a stranger with the repo, and it cannot be when the corpus behind it is deleted seven days later. Seven days stays the default, since #165 recorded `execution_traces` at 160 MB of a 187 MB database before the window existed, so only the override is new. A value that does not parse is ignored rather than failing the cleanup, because a typo in an environment variable must not stop the database being maintained.
- **A Memory Model section in the README and all six translations (#439)**: three tiers the schema has always implemented, written down for the first time. Permanent for conclusions, 30 days for working state, 7 days for verbatim bytes. It states the one thing a handle cannot promise, that `omni_retrieve` for content archived more than 30 days ago will not resolve. The honest answer to "will OMNI still know my project after a month away" is yes for the conclusions and no for the raw bytes, and that distinction was invisible.
- **The replay attributes every repeated byte the ledger declines to the gate that declined it (#450)**: `GapSeen` keys each line with `ledger::line_key` in payload order, so its notion of already-shown is byte-identical to the ledger's and the totals subtract cleanly. It also prices the marker as the variable it is, dumps run sizes via `OMNI_BENCH_RUNS_OUT`, and adds a project-scope switch (#448) so the two ledger scopes can be measured apart for the first time: turning the project scope off costs 0.8 points, which is the first evidence that it earns its place. `format::sniff` was probed directly to prove that its zero is the corpus and not a broken predicate.


### Fixed
- **Project-local signals had never loaded (#433)**: `.omni/signals/` is documented in `docs/FILTERS.md` as the **highest** priority tier and has a section of its own, and the code path that would load it could not return true. `guard::trust::is_trusted` takes the project **directory** and appends `omni_config.json` itself, which is what its own unit tests assert by calling it with `dir.path()`. All three production callers handed it the **file**, so it looked for `<cwd>/omni_config.json/omni_config.json`, failed the existence check before ever reading the trust registry, and returned false however the project had been trusted. `omni doctor` used the same broken path, so it agreed with the defect instead of exposing it. Every unit test on `is_trusted` passed throughout, because they call it the way it was meant to be called; the regression test therefore drives `load_all_filters` from inside a real trusted project, which is the only level the defect is visible at. It also takes a mutex around `set_current_dir`, since that is process-wide and cargo runs tests in parallel. One wrinkle worth recording: on macOS `tempdir()` yields `/var/...` while `current_dir()` resolves to `/private/var/...`, and the trust registry is keyed on the string, so a project trusted through a symlinked path is not recognised by the loader.

### Added
- **A filter for biome and ultracite, which had none and were claimed by one that did nothing for them (#425 follow-up)**: `eslint.toml` matched `biome` and saved **0.0%** on it, because eslint prints one line per problem and biome prints a block per problem with a code frame, a rule banner and `ℹ` explanation paragraphs. `signals/tools/biome.toml` keeps the three things an agent can act on, the `path:line:col rule/name` header, the `✖`/`⚠` message and the final tally, and drops the frames and the documentation. Measured on a 1,520 byte three-error run: **58.4%**, with every location, rule and message intact. Strip-based and with no `on_empty` on purpose: a `strip_lines_matching` pattern that fails to match removes nothing, so a wrong claim costs a passthrough rather than an answer, where a keep list would delete every line biome adds in a future release. `eslint.toml` stops claiming `biome` so there is one owner.

### Fixed
- **biome output was reported as `tsc: 3 errors in 0 files`, with every location deleted (#430)**: `is_tsc_output` treated any line containing `Found ` and ` error` as tsc's, and biome closes with `Found 3 errors.`, as does ruff. So a 1,520 byte lint run with three errors in three files came back as 76 bytes naming the wrong tool, claiming zero files, and carrying no path an agent could open: a 95% "saving" that deleted the answer. tsc's own summary is `Found 3 errors in 2 files.`, and the ` in N files` half is exactly what the arm did not require; it does now. A second guard sits behind it: a summary that counted errors but located **zero files** is self-contradictory on its face and is what a misrouted payload produces, so that state fails open instead of publishing the contradiction. Found while writing the biome filter, not by a gate.

### Fixed
- **Two CLI surfaces that answered with the input they were given (#428, #427)**: `omni stats -d` selected `--today`, while `-w` and `-m` selected `--week` and `--month`, so `-d` was the one member of the window family whose long form did not share its letter, sitting beside a `--detail` that does. `--day` is an alias now and `--today` still works; `has()` takes a slice because a `(long, short)` pair could not carry a third name, and `check_flags` splits the `FLAGS` spec on commas so the validator needed no change. And `omni patterns` printed `Fix hint: <the command it came from>` on every row: a real 20-entry run carried 7 distinct hints and each one was a command name already on the line above. The line is dropped when the hint equals the tool family rather than reworded, because handing the input back under the word "hint" is the defect this project exists to file. Both issues shrank when the code was read rather than the output watched, and both corrections are on the issues: the stats window was disclosed in the header the whole time, so no number ever changed silently, and the patterns status column has both `RESOLVED` and `ACTIVE` branches, so a run of twenty resolved rows is this machine's data and not a field stuck on one value. Entry grouping in `patterns` is untouched and #427 stays open for it.
- **`bun test` lost the name of the failing test, and promoted a passing one (#425)**: found while assessing omni against a TypeScript/Bun monorepo, not by a gate, and the suite had no fixture for a `✓`/`✗` runner at all. On a 672 byte run with one failure the distilled reply opened `error: expected 404 to be 200` with no indication of which test produced it, while three passing tests survived. The decision is the scorer's, not the distiller's: `✗ router > returns 404 for an unknown channel` tiered **Context** and `✓ queue > retries a failed job` tiered **Critical**. Two causes one line apart. `✗` was not a failure marker to `semantic::is_critical`, so bun and every reporter that marks results with glyphs carried no failure signal at all; and a passing test whose *name* contains "failed" read as a verdict, which is #210 arriving through a second door, because that fix protected tallies and a test name is prose. A line that has already announced a pass is no longer read as announcing a failure, glyph verdicts are recognised, and `fail` counts alongside `failed` since reporters disagree about the tense. Fixing the tier exposed a second loss that the accident had been hiding: bun's `7 pass` / `1 fail` summary was surviving only on a positional boost from the wrongly-Critical line, so foreign tallies are now recognised on their own. The existing `a_green_tap_run_keeps_its_verdict` then caught a regression in the same change, since TAP writes `# fail 0` with the count on the other side of the word; the zero is read from both sides now. Each of the three new guards was broken separately and each went red only for its own reason.

### Security
- **Five dependabot alerts, all `undici`, all closed by one devDependency bump (#423)**: one high and four moderate, every one of them fixed in `undici@8.9.0`. It arrived transitively at 8.5.0 through `@earendil-works/pi-coding-agent`, which `plugins/pi/package.json` pinned at `^0.83.0`; there is no newer 0.83.x, so the caret could never resolve to a fix. `^0.84.0` depends on `undici@8.9.0` and closes all five at once. Scope stated so the severity is not read wrong: this is a devDependency of the Pi plugin, absent from the Rust binary, the published crate and the Homebrew formula, so installing omni never exposed anyone. `make ci` runs `cargo audit`, which is Rust only, so no gate was about to break either. The reason to fix it is that the alerts are real for anyone developing the plugin, and `npm audit` now reports zero. `tsc --noEmit` passes against 0.84.1, so the API the extension uses is unchanged.

### Fixed
- **`docs/BENCHMARKS.md` published two aggregates from two replays without saying so (#420)**: the headline and the per-class table read 15.7% over 7,095 traces; the head-to-head read 16.1% over 7,198. Both were true and they were not the same measurement, the first predating the project ledger and the second including it, and nothing in the file distinguished them. A reader takes that as one number quoted inconsistently, which is the reading this file exists to prevent, three lines under its own rule that every figure states the window it covers. Fixed by re-replaying once on current `main` rather than by labelling the two runs, so **one run stands behind every figure**: 7,221 traces, 2026-08-03 08:45 to 08-10 12:51 UTC, 5.2% from the filters and 16.1% with the ledger, 5.0% fewer tokens at 3.562 bytes per token, 25.0% raw repetition against 22.6% post-filter, rtk `pipe` at 8.1% on the same bytes, and **0 calls larger** now that #398 is fixed. The file says the stricter rule out loud, because the weaker one is what let this through: one run, or each figure names its own.

### Fixed
- **Stream mode rewrote every CRLF payload as CR (#411)**: the reader breaks a line on a bare `\r` deliberately, so a progress bar redraw is its own line rather than being buffered into the whole download. That turned a Windows line ending into two reads: the first ended `...\r` and the second was a lone `\n`, which trims to empty and is dropped by the writer's non-empty check. Every `\r\n` in a stream-mode payload therefore came out as `\r`. It peeks now rather than reads, using `BufRead::fill_buf`, so a `\r` immediately followed by `\n` stays one line ending and a `\r` that really is a redraw leaves the next byte for the next line. Filed separately from #398 rather than folded into it, because that one was a terminator the writer invented and this one is a terminator the reader split, and because no CRLF payload appears in the 7,198 trace corpus at all: the cost here is zero on this installation and real on a Windows host. Two tests, one per property, and each fails only for its own reason: removing the peek reddens the CRLF one and leaves the redraw one green.

### Added
- **The head-to-head P4 asked for, on one corpus, with the losing half published (#417)**: `OMNI_BENCH_RTK=/path/to/rtk` adds a third arm to the replay; without it nothing changes and CI never needs a competitor installed. Same 7,198 traces, same bytes in. omni's filters alone take 5.2%, **rtk `pipe` takes 8.2%**, and omni with the ledger takes 16.1%. So rtk's filters are better than ours by 3 points on the same input, and the ledger is what puts omni ahead: it removes bytes because the agent has already been shown them, not because a pattern calls them noise. The comparison is tilted toward rtk on purpose, since it is handed the exact filter name for each command where its own hook would have to infer it, and anything it has no filter for counts as a passthrough rather than a miss. P4 set the bar at "if omni does not win, the claim does not ship"; it wins, and the half it loses is in `docs/BENCHMARKS.md` beside the half it wins.
- **`omni exec` runs the ledger, which it never did (#416)**: the issue asked to make passthrough non-terminal on the theory that nothing runs after a distiller declines. The check it demanded first found the premise wrong, `distill_with_command` is `route(...).unwrap_or_else(|| input.to_string())` so `None` already means "carry on with the input", and found something else instead: **the two hooks were running different chains.** `post_tool` has had a ledger stage since #397 and `pipe` never had one, so every command the pre-hook rewrote into `omni exec` got the filters and nothing else. The blocker was identity: the ledger may only key on the host's session id, never `SessionState::session_id`, which is a wall-clock stamp that covered 16 project paths in one value (#118), and the pre-hook was not forwarding one. It does now, by the same mechanism that carries `--agent` since #360, and `strip_exec_wrapper` drops both flags rather than the first, because stripping one leaves the other on the front of the command the registry reads, which is #375 with a different flag.

### Added
- **`src/pipeline/producer.rs` and `src/pipeline/gate.rs`, the module split section 5.4 asked for (#395)**: shell-line decomposition moves out of `registry.rs`, which keeps distiller and profile selection and loses command parsing. Nothing in `producer` knows what a distiller is, and it is unit-testable without a database. **The behaviour is deliberately unchanged and the replay proves it**: the filter column reads 5.2% before and after, which is the differential this move had to survive. P2 also proposed a *new* rule, decompose and take the last segment, and that is deliberately not implemented: `owning_tail` carries the measurement over 4,958 recorded pipelines where routing by the last stage regardless hands 69.1% of them to `head`, `tail` or `grep`, all verbatim passthroughs. The genuinely new component is the gain gate, which answers the two questions that are true regardless of how good a distiller is, an input too small to be worth touching and a result that is not actually smaller. It wraps the projection rather than being re-derived inside it, and it is what makes a routing change safe to ship against a measurement predicting little gain: the worst case is no change.
- **The ledger reaches the project's earlier sessions, with a marker that does not claim the agent saw them (#396)**: an earlier draft cancelled this phase because a handle for another session's content is a lie. That was right about the wording and wrong about the remedy. `[OMNI: N lines shown in an earlier session of this project, not in this one, omni_retrieve("...")]` is an accurate statement, and the trade then stops being free and becomes the same bet `store_rewind` already makes everywhere else: a handle plus a possible retrieval against the block itself. So the project scope carries its own floor, six times the session one, because the bytes saved have to cover a retrieval the agent has no choice about. A line in both histories is claimed by the session, since that is the claim that costs nothing. Measured: aggregate 15.7% to **16.2%**, `git` 19.1% to 22.1%, file reads 24.6% to 24.8%, and no trace worse. Eviction is defined before shipping rather than after, per the spec: time and not size, one window for both scopes, because evicting by size evicts the busiest project's oldest rows first and that is exactly where the repeats are.

### Added
- **`omni stats --card` writes the share summary as an image, sized for the places images get posted (#400)**: `--share` prints text, and text does not upload to Instagram, TikTok or a WhatsApp story. The card carries the same figures, from the same computation, into `~/.omni/cards/` as SVG at 1:1, 9:16 and 16:9, then converts each to PNG if the machine already has `rsvg-convert`, ImageMagick or Inkscape. No converter is installed and none is suggested: the SVG is written either way and the conversion command is printed. SVG rather than PNG because the two in-binary routes are both worse here. A real renderer (`resvg` plus a font crate) is roughly 2 MB against the binary-size gate in `make ci`, paid by every user for a command almost nobody runs, and a hand-rolled PNG encoder needs a hand-written bitmap font, which is a lot of code and looks it. Three sizes rather than one because a 16:9 card posted to a story is letterboxed into illegibility and cropping a square to 9:16 loses the number the card exists to show. Both renderers read one `ShareFigures`, so the text card and the image card cannot quote different numbers for the same installation. Four defects were found by rendering it rather than by reading it: `"#` closes a `r#"` raw string, so every hex colour ended the literal; ImageMagick honoured neither the CSS font keyword nor `letter-spacing`, rendering a serif italic with 200px gaps in the wordmark; a fixed column offset put `99.8%` on top of `kubectl kustomize`, so the numbers are anchored to the right edge instead; and the fitted body size was rounded up, which put the longest line four pixels past the margin it had just been computed to fit inside. The last one is a test rather than a fix note, because it went red first. One line the command prints and a doc would not: the card names your top commands, so read it before you post it.
- **The session ledger: a run of lines the agent has already been shown comes back as a handle (#394)**: every filter in `src/distillers/` answers "given this output, what can be dropped", one command at a time. The ledger answers "given everything shown in this session, what is this output repeating", and the corpus says the two are orthogonal. Replayed over 7,065 model-facing traces, **25.1% of raw bytes are lines already seen and 22.6% still are after every distiller has run**, so filtering removes 5.2% of bytes and barely dents repetition. It also reaches the class nothing else can: two thirds of model-facing bytes are the agent reading files, where filtering saves about 1.5% and cannot do better without guessing which parts of a file the agent meant. A run of lines it has already been shown can be handed back without guessing anything. Consecutive lines that were all emitted earlier in the same session become one marker naming the count and an `omni_retrieve` handle; everything else passes through byte for byte. Three gates keep it narrow. `format::sniff` first, the same one collapse sits behind, because replacing lines inside a JSON document corrupts what the next step parses. The scope is the **host's** session id and never the local fallback: `SessionState::session_id` is a wall-clock stamp that covered 16 project paths and 3,739 commands in one id (#118), and scoping by it would let the ledger tell a session it had already been shown output that went to a different one, which is a false claim rather than a missed saving. And a run stays verbatim unless it clears both 8 lines and 240 bytes, because a marker replacing less than that costs more than it saves. Storage is hashes: `ledger_lines` holds 16 bytes a line, and the content of a run reaches `rewind_store` only when a handle is actually issued, so recording every emitted block does not mean a second copy of the corpus. Recording is unconditional and substitution is not, which is the widened gate the plan asked for: a block is worth remembering because it may recur, not because it compressed today. Determinism is a test rather than an intention (`renders_identical_bytes_for_identical_state`), because the `{ts_ns}_{hash}` handle that made identical input emit different bytes shipped once and was fixed with nothing behind it.
- **`omni doctor` reports dead space in the database, and `--fix` reclaims it (#393)**: deleting rows does not shrink a SQLite file. The pages go on a freelist for reuse and with `auto_vacuum = 0` are never handed back, so the trace pruning in #285 removed 160 MB and reclaimed nothing: 50,226 pages with 36,264 free, a 196 MB file holding 54 MB. Doctor prints the share always and vacuums only when asked, because `VACUUM` rewrites the whole file and needs free disk equal to it. `auto_vacuum = INCREMENTAL` was the alternative and is worse here, since enabling it on an existing database needs a full vacuum first and then taxes every commit to reclaim space this workload loses about once a year.
- **`omni stats` names the fidelity alarm instead of only acting on it (#394)**: an expansion request is an agent saying the view it was given was not enough, so a rising share of archived blocks being fetched back means distillation is cutting too much. That number was already being acted on silently, since `post_tool` raises the route thresholds once a command family passes 25%. It is now printed when it crosses the same line.
- **`tests/bench_replay.rs` reports repetition and real tokens, not only bytes (#392)**: every phase of the direction plan is gated on numbers the harness could not produce. It now accounts line-level repetition (exact match, lines of 12 characters or more, split by same-session and earlier-session-same-project), counts tokens with `cl100k_base`, and breaks the corpus down by command class and by shell shape so the ledger and addressing gates can be read off one run. The tokenizer is a **dev dependency only**: #174 removed exact counting from the hook after measuring it at 34.3 ms against a 10 ms budget, and a benchmark has no such budget.

### Changed
- **The README leads with what OMNI does instead of a percentage, in all seven languages (#399 follow-up)**: the hero sold a savings figure and the badge strip repeated it, which is the pitch two 60k-star projects in this category already make, and one of them measurably filters better than we do. What OMNI has that a filter cannot is the ledger, and the front page never said so. The opening is now the problem the agent actually has, that it reads every line the terminal prints and then reads most of them again next turn, followed by a `What it does` section that did not exist: a reader previously had to reach line 115 to learn what the tool does. Removed: the badge strip, and the 18-line `How to read a savings number, including ours` section, which argues about metrics and belongs in `docs/BENCHMARKS.md` rather than on the front page. Collapsed: two promise paragraphs that restated the guarantee table directly beneath them. English drops from 238 lines and 1,695 words to 209 and 1,269. **No aggregate figure appears in any of the seven files now**, deliberately: `docs/BENCHMARKS.md` currently publishes 15.7% and 16.1% from two different replays (#420), and the per-class numbers that remain do not disagree with each other, so the README stays true whichever way that resolves. The six translations carry the same substance but keep their own section order, which had already drifted from the English file before this change.
- **`make ci` runs in half the time, and the release binary it gates is unchanged (#405)**: a warm round measured 335s, of which 209s was `cargo build --release` recompiling one crate. Every dependency was already cached; `codegen-units = 1` simply gives codegen a single core out of eight. Priced back to back on the same machine, that setting costs **355s against 89s at 16**, and buys 9,941,888 bytes against 11,466,880. So the gate now builds a `[profile.ci]` that inherits release and differs only in `codegen-units`, which leaves the shipped artifact byte-identical: `release.yml` runs `cargo build --release --target <triple>` itself and never calls `make`. The size check stays at 15 MB and is therefore stricter on this proxy than on the real binary, which is the direction a proxy may err. `make test` also stops running the suite twice. `cargo insta test` is `cargo test` with insta's assertion mode changed, so following `cargo test --all` with it re-executed all 1,604 tests to re-derive what the first run had decided, and the baseline log's 3,208 is that doubling rather than a larger suite. `--check` is the stricter mode, refusing to write a `.snap.new`. Warm total is **171s**. Two hypotheses died on the way: the duplicate run was worth 11s of the CI job rather than half of it, and a clean worktree already ran the old pipeline in 335s, so the 15 to 20 minutes reported locally is a 76 GB `target/` with 568,642 files in `debug/deps`, which `ls` alone takes 25.6s to walk. That is a `cargo clean`, not a code change, and it is why this entry does not claim to have fixed it.
- **The README and its six translations stop quoting a corpus that no longer exists (#399)**: `docs/BENCHMARKS.md` moved to the replayed window in #396 and the front page did not, so the first thing a reader saw was 43.3% over 9,965 traces while the document it linked to said seven-day pruning had deleted the corpus behind those numbers. Three different problems were in there and they needed three different answers. "0 of 9,965 calls ever grew the output" was a true fact with no window attached, and this corpus has 2 of 7,095, so it is stated with its window and linked to #398 rather than quietly dropped. 43.3% and 9,965 become 15.7% over 7,095 traces covering 2026-08-03 to 08-10 UTC, split into the 5.2% the filters take and the rest the ledger adds. And `git` 89%, `cargo` 91%, `kubectl` 77% are gone from the hero line and the badge strip in all seven files: they were single-fixture measurements presented as corpus aggregates, and they were stale even as fixture measurements, because `kubectl get pods` reports 0% now and losing that 9.3% was the fix. What replaces them is build and test at 87.9% and file re-reads at 24.6%, both read off the per-class table, because a class aggregate and a single fixture are not the same measurement and the front page now says which one it is quoting. The "90.0% of calls saved nothing" answer becomes 97.1%, which is the less flattering number and the one this corpus produced.
- **Every published aggregate is now a figure the harness can regenerate, and it states the week it covers (#396)**: `execution_traces` is pruned to seven days, so a corpus is gone a week after it is measured and the 9,965-trace run behind the 43.3% headline cannot be re-derived by anyone, us included. `docs/BENCHMARKS.md` leads with that fact and carries the replay for 2026-08-03 to 08-10 UTC: 7,095 traces, all `claude_code`, 5.2% from the filters and **15.7% with the ledger**, 5.0% fewer tokens by `cl100k_base` at 3.545 bytes per token, and 25.4% of raw bytes being lines already shown against 22.9% after the distillers. Its per-command table is replaced by a per-class one with a filters column and a ledger column, because that is the comparison the numbers are actually making: build and test output is 87.9% and 292 KB, file reads are 1.54 MB and the filters take **0.0%** of them. The two aggregate claims that could not survive this go with it. `Cargo.toml` said "58.9% fewer tokens on a real command mix, measured" and the repo description said "cuts terminal noise 89%"; neither carried the window and the population a savings figure is only true inside, and a crates.io description cannot carry them, so the ratio is gone from both rather than replaced with a fresher one that would rot the same way. The README and its six translations still quote the old corpus and are filed as #399, because seven files in six languages is a translation job and half of one is worse than none.
- **The measurement corrected three claims the plan was built on (#392)**: the plan says terminal output runs 2.5 to 3 characters per token and that byte figures therefore understate volume by 30 to 60%. Measured on the live corpus it is **3.541 bytes per token**, against the 3.6 `util::token_estimate` ships, so the estimator is off by 1.7%. Token savings do land a little below byte savings (5.0% against 5.2%) and the byte-sink and token-sink rankings do disagree, which is what the accounting was worth adding for. Second, the published 43.3% all-time figure **cannot be re-derived at all**: `TRACE_RETENTION_DAYS` is 7, so the corpus behind it was deleted by #165's pruning, and every figure from here on states the window it covers. Third, the harness's own population label compared row counts, so a corpus with no `terminal` rows printed "every trace, terminal included, which is not the figure to publish"; it asks the filter now.
- **SQLite is tuned for a read-heavy ledger before the ledger multiplies reads (#393)**: `PragmaCustomizer` set `journal_mode`, `synchronous` and `foreign_keys` and nothing else, leaving `cache_size` at the 2 MB default against a 54 MB working set, roughly 500 pages cached of 50,226, with no `mmap_size` and no `temp_store`. Those three are the in-process cache tier, and they are the answer to whether omni should gain a Redis: it already has one and it was switched off. `busy_timeout` is deliberately **not** among them, because rusqlite sets it to 5000 ms itself and setting it here can only lower it, which an earlier attempt talked itself into doing after a source grep found the pragma missing from this tree.

### Fixed
- **The secret redactor matched its patterns as substrings, so `passed=`, `authors=` and `monkeys=` lost their values on every command's output (#408)**: `is_sensitive_key` asked `upper.contains(p)` against a list holding `PASS`, `KEY`, `AUTH` and `CRED`, and those are inside a great many ordinary words. `redact_sensitive_assignments` runs at the top of the distill path and is deliberately not gated on the command (#344), so the reach was every command, not `env`. Worse than a cosmetic false positive on two counts: that function returns its body with no marker, unlike `distill_env_output` which prefixes `[OMNI: N sensitive value(s) redacted]`, so nothing told the reader OMNI had done it; and a second `cat` of the same file came back redacted too, because the re-read went through the same hook. Found by the defect eating its own evidence, when a `make ci` timing line reading `passed=3208` came back as `passed=[REDACTED]` while `failed=0` survived, leaving the output plausible and the measurement gone. Matching is now per underscore-delimited segment, which is the unit a SCREAMING_SNAKE environment name actually carries: vendor prefixes such as `API_` anchor to the first segment, multi-word patterns such as `DATABASE_URL` match a consecutive run, and the rest match a segment outright or its tail, so `MYPASSWORD` and `APIKEY` are still caught while `PASSED` is not. `CRED` is replaced by `CREDS`, `CREDENTIAL` and `CREDENTIALS`, because a stem only works under substring matching and would otherwise have missed `GOOGLE_CREDENTIALS` while still eating `credits`. `distill_env_output` held a second copy of the substring test and now calls the one predicate, since fixing only the first would have left `env` output redacting `passed=` after every other command stopped. `BYPASS` ends with `PASS` and is a known false positive left in place on purpose: a value hidden needlessly is recoverable through `omni_retrieve`, a secret printed is not.
- **The stream writer invented a line ending, so a passthrough came back larger than it went in (#398)**: `stream_distill` chose between `\r` and `\n` and always wrote one. Two things followed. A last line with no terminator got one appended, so 2 of 7,032 replayed traces came back **exactly one byte larger**, both `podman run`, against a published claim that not one call in the corpus ever grows. The line's own terminator is written back verbatim now, which is empty when the input had none. A sibling defect in the same two lines, CRLF being rewritten as CR because the reader deliberately splits on the carriage return, is filed as #411 rather than folded in: it needs the reader to peek past a `\r`, which is not a one-line change, and no CRLF payload appears in the corpus at all. This could not be diagnosed before #406: a stripped line came back as the `on_empty` sentence and swamped a one byte delta, which is why the first attempt at this was written, failed for the wrong reason, and was reverted rather than shipped on a guess. The two tests were written against unfixed code and watched fail first.
- **`on_empty` fired per line in stream mode, so every stripped line came back as a success sentence (#406)**: `TomlFilter::apply` ends in a zero-state, returning the filter's `on_empty` string when the filtered result is empty. That is right for a whole payload, which is what the batch path hands it. `stream_distill` hands it **one line at a time**, so the answer to "this payload filtered down to nothing" was given once per stripped line, and the writer's non-empty check then printed each one. `signals/tools/docker.toml` strips 16 noise patterns including `^Copying blob `, so a `podman build` producing ten of those lines emitted ten copies of `docker: image operation completed successfully`. Both directions of wrong at once: it grew the output it exists to shrink, and it asserted a successful image operation once per line it had recognised nothing in, which is the #224 shape under a different filter's name. `npm.toml` and `bun.toml` are stream-mode with an `on_empty` too. The zero-state is now scoped rather than deleted: `apply` keeps it for a payload, `apply_line` is what the stream loop calls, and a test pins each half. Found while trying to reproduce #398, which is the same code path and looks like the small visible edge of this, since `podman run --rm alpine true` prints almost nothing and so grew by one byte instead of by a paragraph.
- **The ledger kept claiming content the agent had lost to compaction (#401)**: the session ledger replaces a run of lines with a handle on the grounds that the agent **already holds those bytes**, so the handle costs nothing unless it wants to re-read them. Compaction deletes that premise inside a session: afterwards the context no longer holds the earlier content, and every marker from then on says "already shown this session" about lines the agent cannot see. This is the same argument that cancelled the project-scoped ledger on #396, applied one scope down, and it was less obvious there because a new session starting empty is visible while a compaction boundary inside one is not. Nothing was lost, since the handle still resolves, so this is not a #388-class broken promise; what broke is the trade, because after a compaction the agent pays a retrieval to buy back content that would have arrived in full. `PreCompact` now drops the scope's rows, which is the conservative direction: the ledger forgets and re-sends in full, costing a missed reduction rather than a wrong claim. The forgetting runs before the summary work, which can return early on an unchanged delta and must not gate it. Found by an independent review of #397 rather than by a test, and the gap is worth naming: nothing in the suite covered the ledger across a compaction boundary until now. Removing the call fails `forgets_what_the_agent_lost_to_compaction`.
- **`store_rewind` returned a key even when the write failed, so the marker promised `omni_retrieve()` for content that was never archived (#388)**: the function handed back the content address on every path, including a pool-get failure and a swallowed `execute`, and the caller in `hooks/post_tool.rs` printed a handle for it while the sibling `None` arm two lines below already printed an honest "not archived". It returns `Option<String>` now, and both hook paths fall through to that wording. Rare while the archive fired on 139 of 14,962 calls; routine once the ledger records every emitted block, which is why it lands with it. A handle that does not resolve is the one defect this store cannot have.
- **`context_turns` survived its own drop migration (#379)**: the drop was gated on a `schema_migrations` row, so it ran once, recorded itself as applied, and then a concurrently installed older binary recreated the table 12 seconds later with the migration never able to run again. A mixed-version machine is the normal case during an upgrade. The drop is unconditional at startup now, which costs a catalogue lookup against a table that is already gone.

## [0.6.15] - 2026-08-09

### Added
- **`omni init --cursor` installs the rule that makes `omni_run` get used, and `omni doctor` fails without it (#352)**: Cursor cannot let a hook replace built-in tool output, so `omni_run` is the only path by which a shell command is distilled there. That tool existed and the agent still had to choose it, which is the difference between a lever being available and being used. Printing a hint was the cheap version and it does not work: the user has to notice it, copy it and create a file. Running init inside a repository now writes `.cursor/rules/omni.mdc` (two lines, `alwaysApply: true`), uninstall removes it again, and outside a repository it prints the rule rather than writing into a home directory uninvited. Doctor treats a missing rule as a failure with the reason spelled out, `the agent will keep using the built-in shell`, because hooks green plus no rule is the same false green this tracker keeps finding. The rule is charged to the Rules context bucket #352 tracks, so a test caps it at three lines: a rule that grows makes the bucket it exists to help worse.
- **Codex CLI and Gemini CLI reach model-facing distillation (#351)**: the parity path turned out to be the *pre* hook, not the post hook. Codex documents `hookSpecificOutput.updatedInput.command` on `PreToolUse` and states that `PostToolUse` cannot replace output at all, so wrapping the command in `omni exec` is what changes the bytes the model reads. OMNI already emitted exactly that reply, so Codex needed no adapter, only the config bug above; a contract test now pins the shape. Gemini reads replacement arguments from `hookSpecificOutput.tool_input` under a `BeforeTool` event, so the reply shape is chosen from the incoming payload's `hook_event_name` rather than an install-time flag that could drift out of step with the config that set it. An earlier plan to gate this on Codex's `[features]` flag was dropped after asking the binary: `codex features list` reports `hooks stable true`, so no flag is needed.
- **`omni_run`, the distillation path for hosts whose hooks cannot replace built-in tool output (#351, #352)**: on Cursor every route is closed, `updated_mcp_tool_output` is documented "For MCP tools only", `afterShellExecution` defines no output fields, and `beforeShellExecution` is allow/deny/ask with no command rewrite. So a command can only be distilled there if OMNI is the tool that ran it. `omni_run` executes a command and returns its distilled output as the MCP tool result, reusing `omni exec`'s pipeline: the same filters, the same format gate, the same redaction, and the same #122 rule that a command exiting non-zero passes through verbatim rather than being summarised into a clean-looking failure. This is not a new capability for the agent, which can already run shell commands, and it stays under the host's own MCP approval flow.
- **`omni doctor` prints a distill tier for every host (#351)**: `model-facing distill active` or `hooks/MCP only, built-in tool output not rewritten`. The tier is a trait method defaulting to MCP-only, so a new integration has to claim distillation rather than inherit the claim, which is what let three of them ship the claim without the behaviour. The README and its six translations now state the same thing in one sentence instead of listing host names under one verb.

### Changed
- **`omni session --inject` now carries the state a new chat cannot re-derive (#352)**: on Cursor this block is the only recovery path, because the host does not let a hook rewrite built-in tool output at all. At 200 bytes it held the task line and one error truncated mid-sentence, so a fresh chat still had to rediscover which files were in play and what was broken; it bought its cheapness by not working. The budget is 1000 bytes, chosen against a measurement rather than taste: the session it was tuned on had run 667 commands producing 539,795 bytes of tool output, so rebuilding that state by replay costs it again and this block is 0.2% of it. It now carries task and domain, the last three distinct commands, five hot files with access counts, and two distinct unresolved errors. Two defects showed up only in the real output and not in a fixture: one recorded heredoc filled the entire budget and the truncation then removed the hot files and errors, so each item is capped separately; and a leading `cd` plus a run of `VAR=value` assignments is identical across entries and ate that per-item budget before the program appeared, so three different commands rendered as three identical lines.
- **The host-tier matrix is product law, in the README, all six translations and `omni doctor` (#357)**: three tiers rather than two. **Full** (Claude Code, Codex CLI, Gemini CLI, Aider) is where the host applies OMNI's rewrite. **Handoff-first** (Cursor, Windsurf) is where it cannot, but `omni_run` still distils whatever is routed through it and `omni init --cursor` installs the rule that makes the agent reach for it. **MCP-only** is memory and session state with no shell distillation and no claim of one. Cursor moved out of MCP-only because the model demonstrably reads less there now, which is the distinction the tier exists to make.
- **`docs/ROADMAP.md` names the context-window KPI as primary and demotes distill % to a scoped diagnostic (#357)**: Conversation growth, turns before compaction and task-recovery cost are what OMNI is bought to move. Distill % is secondary, always scoped by `agent_id`, always model-facing. The reasoning is recorded rather than asserted: 81% of recorded calls are Passthrough and correctly do nothing, so a blended figure describes the command mix more than the product; `terminal` rows are TTY bytes no model reads; prompt-cache reads bill about a tenth of fresh input; and on a flat-rate plan compression does not reduce a bill at all. Any public headline number must now cite its `agent_id`, its corpus and a reproduce command, which is what #212 and #324 are made of. Three new non-goals are dated and reasoned: chasing a higher % by making distillers more aggressive, claiming shell distillation on a host that does not apply the rewrite, and intercepting a host's shell by denying it and returning output as a hook message.

### Removed
- **Five documents in `docs/` that described behaviour the code no longer has (#390)**: `docs/` held 11 top-level files, of which only `ARCHITECTURE.md` and `BENCHMARKS.md` were reachable from the README and its six translations, and `CONTRIBUTING.md` linked a `docs/FILTERS.md` that did not exist. `TESTING.md` advertised "135+ tests" against a suite of 3,152, a binary under 5MB against a 9.4MB release binary and a 15MB CI gate, latency under 5ms against the 34.3ms that measuring by removal actually produces, and "100% context safe", which is the kind of absolute this project files issues about. `SESSION.md` documented `PreCompact` injecting its summary into compaction metadata, which #371 established never happened. `COMMAND_AUDIT.md` was a dated snapshot whose central table said `ls -la` loses every filename and `env` comes out 50% larger, both fixed by #198 and #200, and its reproduce section was an empty code block. `MIGRATION.md` walked through upgrading 0.4.x to 0.5.0 off the Node and Zig build, with download URLs pinned to v0.5.0. `DEVELOPMENT.md` claimed 147 tests and named `src/pipeline/classifier.rs`, a module `MIGRATION.md` recorded as deleted two files away; `CONTRIBUTING.md` and `CLAUDE.md` already carry its content, correctly. The one section of the 1,275 line `HOW_TO_USE.md` that nothing else covered, filter authoring, is now `docs/FILTERS.md`, which is what `CONTRIBUTING.md` was already pointing at. Nothing measured was dropped. A document that states a behaviour the binary does not have is the same defect as a distiller reporting a saving it did not make, and it is worse for having no test that can fail.
- **`docs/autonomous-loops/`, five shipped examples that nothing tested (#390)**: 393 lines of runnable loop templates reachable only from `docs/LOOP_ENGINEERING.md`, which nothing links either. They had already failed once in the way that matters: when `omni handoff` was removed as a subcommand in #164, `shell-loop.sh` guarded its checkpoint with `omni handoff --json … || echo "CONTINUE"` and kept looping against a default it never computed, while `mastra-integration.ts` swallowed the same failure twice behind `catch { return true }` (#180). The comments were repaired then; the mechanism that broke them was not, because no test runs either file and no subcommand removal has a reason to grep them. An example that reports a healthy loop from state it never read is the defect this project exists to stop, so the examples go and the interface stays: `OMNI_LOOP_ID`, `OMNI_LOOP_GOAL`, `OMNI_LOOP_BUDGET` and `OMNI_LOOP_ITERATION` are read at `src/pipeline/mod.rs:123-154` and remain documented in `docs/LOOP_ENGINEERING.md`, which now explains why the templates are gone rather than pointing at them.

### Fixed
- **`PreCompact` built a 6000-token snapshot and returned it to a field the event has no slot for (#371)**: the reply carried `hookSpecificOutput.systemPromptAddition`, and Claude Code, the only host OMNI registers this hook with (`agents/claude.rs:485`), documents no content field on `PreCompact` at all. Renaming to `additionalContext` would not have helped, which is what separates this from #364 and #158: there was no correct field to move to, so the snapshot was discarded whatever it was called. Unlike those two the host does not reject the reply either, so nothing appeared in the transcript and the loss left no trace. The fix is deletion rather than a new hook, because the replacement path was already running: this handler writes the snapshot to `session_events` and upserts the session *before* compaction, and Claude Code fires `SessionStart` afterwards, where `session_start::process_payload` rebuilds its block from that row and returns it as `additionalContext`, the field that host does read. So the summary still reaches a model after a compaction, from the hook that can inject, and the dead reply is gone. Delta detection now skips the write rather than emitting a shorter dead string. Every test on the reply was rewritten to read the recorded snapshot back out of the store: asserting on the return value is what let this survive, since it proved the struct serialised, which was never in doubt, and said nothing about the host reading it. Restoring the reply fails `emits_no_reply_because_the_event_accepts_no_content` and removing the delta guard fails `delta_detection_skips_unchanged_compact`, both checked by breaking them. The dispatcher's routing test is deliberately not one of those: it now asserts the recorded snapshot, so it stays green either way, which is the point of separating routing from the reply.: every site discarded the install result and printed success unconditionally, 19 of them across 11 integrations, and none checked the outcome. The cost is not the label: in fix mode that line *replaces* the warning the non-fix branch would have pushed, so a failure had nowhere to surface. Against an unwritable Codex home with no `hooks.json` written, `doctor --json --fix` answered `healthy: true`, `hooks installed`, and zero warnings; it now prints `[FAILED] …: Permission denied (os error 13)` and raises a warning. Same shape as #367 fixed hours earlier, which is why the repair is one shared `report_fix` helper rather than 19 corrected copies free to drift apart again. Removing its failure branch fails the regression test; the sibling test stops the reporter swallowing successes.
- **Every hook Cursor registers was inert (#384)**: `omni init --cursor` writes `beforeShellExecution`, `postToolUse`, `postToolUseFailure` and `stop`, and the dispatcher matches event names exactly and case-sensitively, so the last three fell to the wildcard and did nothing while `omni doctor` reported them installed. On Cursor that meant no session summary, no error recorded for a failed command, and nothing feeding the `active_errors` list that relevance scoring and `session --inject` read, on the one host where `--inject` is the only recovery path. The defect had two layers: the dispatcher routes on the event name and the handler then re-checks the raw name from the payload, so mapping only the dispatcher left the path dead, which is what the first attempt did. One mapping is applied at both layers now. `stop` maps to `SessionEnd` deliberately: it fires per agent turn rather than per session, and `save_session_summary` is an `INSERT OR REPLACE` keyed on the session, so repeated firing refreshes the row instead of accumulating one per turn, which makes the original intent recorded in `cursor.rs` correct and only the name mapping missing. Asserted against the binary because a unit test on the mapping passes while either layer is still broken; each of the three guards was removed separately and the matching test failed each time. The row counts come from the library rather than the `sqlite3` binary, which is not guaranteed on every CI runner.
- **#212's terminal exclusion never reached `SessionState`, so session-level savings still counted TTY bytes (#382)**: `store/sqlite.rs` excludes `agent_id='terminal'` rows from the token headline and has a test for it, but `session/tracker.rs` had no agent filter and added every distillation to `cumulative_input_bytes` / `cumulative_output_bytes`, which is what `estimated_tokens_saved()` divides and what the `SessionEnd` summary and `omni_context` report. The same quantity was therefore reported two ways depending on the path, and the path with a passing test was the correct one, which is worse than being consistently wrong because the test reassures whoever checks. The savings counters now accumulate only for work a model received, using the test `delivered_bytes` already applies here: a TTY on the other end means a human is reading and nothing is billed. Commands, hot files and errors are still tracked for terminal usage, because they feed relevance scoring and `session --inject` regardless of who read the output. The decision is a pure function because `track_command` works on a spawned thread a test cannot observe without racing it.
- **A rewritten command was recorded twice (#380)**: the pre-hook turns `cargo test` into `omni exec cargo test`, that child distills the output and inserts a row, and the host's PostToolUse then fires with the already-distilled summary so the post-hook distilled it again and inserted a second. Measured through the binary: 1 row after `omni exec` (17329 bytes in, Rewind), 2 rows after the post-hook (116 bytes in, Passthrough). Agent Distribution counted double for every rewritten command, and the second row saves nothing because its input is already a summary, so it also dragged the aggregate savings percentage down. That is the second metric-integrity defect today after #369, and the figures are the whole argument for running OMNI. The post-hook now recognises the bracketed `[OMNI: …]` marker every distilled block carries, which is the only signal available once the text reaches it as a plain string. Verified both ways: a rewritten command records one row, a genuine tool output is still distilled and still records one. The first two unit tests passed with the guard removed because they exercised only the marker predicate; the integration test drives `omni --post-hook` and fails when the guard is taken away.
- **`omni init --hermes` disabled every Hermes plugin the user had (#377)**: the MCP block was spliced into `~/.hermes/config.yaml` directly after the `plugins:` line, and `mcp_servers:` is itself a top-level key, so that ended the `plugins` mapping and every plugin entry underneath became a child of `mcp_servers`. Loaded with a real YAML parser, a config with two enabled plugins came back as `plugins: None` and `mcp_servers: [omni, my-linter, my-formatter]`. This was not OMNI failing to work but OMNI's installer breaking a config it does not own, silently. The block is appended at the end now: top-level keys are order-independent, so appending is both correct and the only placement that cannot capture someone else's entries, and there was never a reason to place it after `plugins:`. Extracted as `append_top_level_block` so it is testable without writing to a real Hermes home; restoring the splice fails the regression test.
- **The `omni exec` wrapper was never stripped, so every rewritten command was routed as a path (#375)**: `rewrite_logic` writes `current_exe()`, an absolute path, while `strip_exec_wrapper` matched the literal prefix `"omni exec "`. The two have never agreed, so a rewritten command reached the registry and the TOML filters still wrapped: a filter keyed on `^git\b` saw a path instead of the program and routing fell through to the generic path for exactly the commands OMNI had chosen to wrap. The `--agent` flag from #360 rode along for the same reason, which is why the fix in #372 did not deliver what its comment claimed. It matches the executable's file name now, which is what stays constant across a dev build, a Homebrew prefix, a relocated binary and `omni.exe`. Every fixture had used the bare word `"omni exec …"`, a shape the writer never produces, so the tests passed on input that never arrives; they use the full-path form now and reverting to the literal prefix fails both.
- **Three hook handlers had never run on Claude Code (#373)**: `session_end`, `post_tool_failure` and `pre_compact` each declared their input with camelCase `serde` renames and no alias, while the host sends snake_case, so `from_str` failed and `.ok()?` returned `None` before any work happened. 66 recorded `SessionEnd` and 182 recorded `PostToolUseFailure` payloads are all snake_case; the only camelCase occurrences anywhere were written by a test run. The cost: no session summary, no FTS row and no agent-session sync for any session ever ended, and no error recorded for any failed command, so the `active_errors` list that feeds relevance scoring and `session --inject` has always been empty on this host. Each handler had a second layer behind the casing. `session_end` compared the host's `session_id` against `state.session_id`, a UUID against OMNI's internal millisecond timestamp, so it dropped every payload that did parse, and it protected nothing because the summary is written under OMNI's own id; the field is gone rather than silenced. `post_tool_failure` read the message from `tool_response`, which the payload does not contain, so with only the casing fixed it would have recorded the literal `unknown error` for every failure; it falls back to the top-level `error` the host actually sends. Every fixture was written in the camelCase shape the producer never emits, so the tests passed on input that never arrives and would still pass with the snake_case path deleted; they use the real shape now, and reverting the contract fails `session_end_saves_summary`.
- **Eight defects found by an independent review of the day's merges, six of them shipped earlier the same day**: `omni session --inject` listed the three *oldest* commands because `add_command` inserts at index 0 and the iterator was reversed, so a resuming chat was told what the session opened with instead of what it had just been doing (#352). Its truncation cut to `MAX - 30` then appended a 35 byte marker, so a truncated block came out at 1005 against a 1000 budget. `resolve_agent_id` guarded on `from_env != "unknown"`, a value `detect_agent_id` never returns (its fallback is `terminal`), so a Codex session started from a plain shell was filed under `terminal`, the bucket #212 showed inflates the savings headline: the row vanished from Agent Distribution *and* corrupted the figure it landed in. `omni doctor --json` collected `warnings` and dropped them, so a Codex install whose hooks await review (#367) reported `healthy: true` with no way for a program to learn the hooks were being skipped. `distill_grep` sorted on count alone, leaving equal counts to `HashMap` iteration order, which changed both the listing and the quoted files for identical input now that the tail is printed. Both readers of a rewritten command stripped only `omni exec `, so `--agent gemini git status` reached the registry with the flag attached and `^git\b` filters stopped matching. `hooks_awaiting_review` compared a raw path against a TOML key whose backslashes are escaped, so on Windows doctor reported all three hooks awaiting review even when approved. `omni exec --agent <id>` with no command executed `--agent` as the program.
- **`omni init --codex` hooked every command twice after the binary moved (#369)**: dedupe matched the exact command string, so repointing OMNI (a dev build to a stable copy, a changed Homebrew prefix, a relocated binary) left the previous entry beside the new one and both fired. Each `distillations` row was then written twice, which inflates that host's counts and savings in Agent Distribution, so the defect corrupts the measurement #351 asks for rather than merely duplicating a line. Installing now removes every prior OMNI entry for the event, in either shape and at any path, using the same `is_omni_hook_command` predicate uninstall already relies on; other tools' entries in the same file are left alone. Same family as #364: the install path recognised only the exact thing it wrote last time, so anything that changed underneath it accumulated instead of migrating. Found by repointing a real install, not by a test.
- **`omni doctor` reported Codex hooks `[OK]` when its own upgrade had just disabled them (#367)**: the Trust check asked whether a `hooks.state` record exists, and Codex also re-checks the entry's hash. `omni init --codex` has to rewrite the entries to migrate the dead shape from #364, which invalidates every approval that was working, and doctor then found the stale records and said nothing. Reproduced on a live install. The hash cannot be verified from the config, so doctor no longer implies it: a missing record is still a `[WARNING]`, and a present one now reads `recorded; Codex re-checks the hash, so re-approve after any upgrade`. `omni init --codex` says outright that the rewrite voided earlier approvals, because an upgrade that silently disables a working integration is worse than a fresh install that was never approved.
- **Codex ran none of OMNI's hooks, because the installed entry shape is one it accepts and never executes (#364)**: `install_omni_hooks` wrote `{"command": "<exe> --pre-hook"}`; Codex runs a matcher group whose handlers live in a nested `hooks` array. Same `CODEX_HOME`, same `--dangerously-bypass-hook-trust`, same probe script: the flat entry ran 0 times and the nested one ran once. This corrects the conclusion recorded on #359, where the shape probes wrote to `~/.codex` while `CODEX_HOME` pointed elsewhere and were therefore testing a file Codex never opened; trust records are real but they are the second gate, not the first. Upgrading replaces the dead entry rather than adding a working one beside it, and uninstall now recognises the command one level down as well, so it cannot leave the orphan that bricked `config.toml` in #351. Verified against a real `codex exec` on a fresh session and a fresh database: `SessionStart`, `PreToolUse` and `PostToolUse` all complete, and the row reads `codex_cli`.
- **The SessionStart summary reached no model, on any host (#364)**: the reply carried `systemPromptAddition`. Claude Code documents `additionalContext`, and Codex's `SessionStart` reply schema is `additionalProperties: false` with `additionalContext` as its only content field, so it discarded the whole reply and logged `SessionStart Failed`. This is #158's defect one layer up. `watchPaths` is the same problem in a second field: valid on Claude Code, fatal on Codex, and only visible on a **fresh** session because a continued one emits none, which is why it looked intermittent. It is now gated on Claude Code's `prompt_id`, which Codex does not send; the failure direction is deliberate, since guessing wrong costs one watch list on Claude and guessing the other way costs the entire reply on Codex. `BeforeAgentStart` keeps `systemPromptAddition`: it is OMNI's own event, read by the Pi extension, and renaming it would have broken a working host to fix two others. An existing test caught exactly that when the shared struct was renamed.
- **Codex could not be told apart from Claude Code by environment (#364)**: it exports nothing identifying to a hook, so a Codex session started from a Claude Code shell inherits `CLAUDECODE=1` and filed its distillations under `claude_code`. The payload is the only reliable signal: Codex sends `turn_id` on `PreToolUse` and Claude Code does not. It is absent on `SessionStart`, which is why the `watchPaths` gate uses a different field.
- **A repo-wide `Grep` named 10 of 68 files and gave a bare count for the rest (#362)**: a grep asks *where* something occurs, so replying with the ten densest files and `... [58 more files omitted]` answers a different question, and the file the caller wanted was as likely to be in the tail as the head. Every matching file is now named with its match count; match lines are still quoted only for the ten densest, which is what keeps the reduction worth having. Measured through the built binary on this repo: 235 matches across 68 files, 18630 bytes raw, 5467 bytes distilled, 71% smaller with all 68 files named, against 82% smaller with 10 named before. Eleven points of reduction for an answer that is still correct. On a sparse search the report is no longer 20% smaller than its input, so `distill_grep` returns `None` and the raw output passes through, which is the correct punt rather than a regression.
- **`omni doctor --json` printed 58 lines of report before the document, so piping it failed (#353)**: `omni doctor --json | jq .` died on the first character. Every integration's `doctor_check` wrote its human-readable report straight to stdout, and the JSON started at line 59. The report now goes through a sink the caller redirects, and the JSON path captures and drops it; findings still reach the caller through `warnings` and the return value. Threading a `&mut dyn Write` through `doctor_check` was the obvious fix and is worse than it looks: several integrations report from inside a closure (`let fmt_hook = |name, present| ...`), which would borrow the writer for the rest of the function. The sink is thread-local rather than global so capturing cannot silence a report on another thread, and it is restored through `catch_unwind` because `--fix` calls into the installers and a sink left installed would silence every later report in the process. `--fix` needed the installers routed the same way: fixing only the doctor path left `omni doctor --json --fix` still unparseable, because `doctor_check` calls `install()`. Asserted against the built binary, not the function, since the defect was in what reached stdout.
- **Gemini's hooks work, but every row it produced was filed under another agent (#360)**: verified end to end against the real Gemini CLI with only the model stubbed. `BeforeTool` and `AfterTool` both fire, the payload carries `run_shell_command` and its `command`, and Gemini applies OMNI's rewrite (proved by substituting an unmistakable command and finding it in the transcript, alongside Gemini's own `[System] Tool input parameters were modified by a hook before execution`). The rewrite then spawned `omni exec`, and that child inherits no evidence of who spawned it, so it guessed from the environment: `claude_code` when a Claude session was the outer shell, `terminal` once that was stripped, never `gemini`. The pre-hook now names the host in the command it writes, `omni exec --agent gemini …`, and the flag outranks `OMNI_AGENT_ID` because it describes this command rather than whichever shell is outermost. A flag rather than an `OMNI_AGENT_ID=` prefix: the prefix is shell dependent and the registry would have to strip it back off. The precedence is a pure function so no test writes the process-global or the environment, which is the cross-test hazard already paid for twice in this tracker. Confirmed by re-running the same harness: the row now reads `gemini`. This affects every Full-tier host that works by command rewrite, not only Gemini.
- **`omni init --codex` wrote to `~/.codex` even when Codex was using a different home (#359)**: Codex resolves its config through `$CODEX_HOME` and only falls back to `~/.codex`. A launcher that exports it therefore got an install into a directory the running Codex never opens, while `omni doctor` printed a hardcoded `~/.codex/config.toml` and called it `[OK]`, so the diagnostic confirmed the file OMNI had written rather than the file in use. The path is resolved once now and every message prints the real one. The decision is a separate pure function from the environment read, because a test that sets `CODEX_HOME` is exactly the ambient-state hazard that the `detect_agent_id` split above was made to avoid; an exported-but-empty value is not a location and still falls back.
- **Codex ran none of OMNI's hooks, because it runs only hooks it has been told to trust (#359)**: `omni init --codex` wrote a valid `hooks.json` and every entry was ignored, silently. Codex 0.144.6 records trust per entry under `[hooks.state."<hooks.json>:<event>:<group>:<index>"]` with the entry's own hash, skips anything it has no record for, and prints nothing about it in `codex exec`; the interactive TUI surfaces them as "Hooks need review". Proved by deleting one trust record from a hook that was working: it stopped firing, and came back when the record was restored. That also explains why entries already in the file ran while every entry OMNI wrote did not, which had looked like a config-shape problem through eight different shapes. `omni init --codex` now says what remains to be done, and `omni doctor` fails while any OMNI hook is awaiting review, with the reason spelled out. OMNI deliberately does not write the trust records itself: the hash exists so a human approves each command before Codex runs it, and a tool that grants itself that approval removes the only thing between "can write a config file" and "can execute anything". Auto-approving was investigated and is not available to an installer either way: the `trusted_hash` preimage is not derivable from the config alone, and Codex rejects `bypass_hook_trust` from `config.toml` while honouring `--dangerously-bypass-hook-trust` on the command line, so the bypass stays a per-invocation choice by the person running it. Both messages name that flag for unattended runs.
- **Codex distillations were filed under `claude_code`, so Codex could never appear in Agent Distribution (#351)**: Codex CLI sends Claude Code's exact `tool_name` + `tool_input` + `tool_response` document, and the agent id was derived from the payload shape alone. Every Codex row therefore landed in the `claude_code` bucket however well its hooks worked, which is why `omni stats` showed 3 rows for a host that was running. The environment is the only thing that distinguishes the two, so where the format is ambiguous it now decides. A format that identifies itself keeps its own answer, so a stale env var cannot relabel Aider's pipe as Codex, and both directions are asserted. The first version of this fix made `detect_agent_id` read the environment directly and three existing tests caught it: that value also drives the Claude Code host-cap branch in `post_tool`, so a behaviour decision would have depended on ambient state and a concurrently running test could flip it. The label and the contract are separate functions now.
- **`podman run` was summarised as `docker logs`, and the fix line was the one it deleted (#354)**: with the machine stopped podman prints the remediation on line one and the symptom on line two. Both went to `distill_docker_logs`, which kept the socket path, dropped the line that says how to fix it, and labelled the result `docker logs: 1 errors/warnings found` for a command that was neither docker nor logs. The cause is that `is_docker_logs` already existed in that file and the dispatch never called it: every docker or podman payload that was not a `ps` table or a build log fell through to the log summariser. It is the gate now, and a payload the tool cannot identify is handed back whole, which is the rule #105 and #112 arrived at. A real timestamped stream still reaches the summariser, and both directions are asserted so the gate cannot quietly become "never summarise".
- **Three agent integrations registered hooks their host never emits, and doctor called all three installed (#351)**: Cursor's `afterFileEdit` was already known (#340); the same shape turned up twice more. Cline was given `PreToolUse` / `PostToolUse` / `PreCompact`, which are Claude Code's event names, while Cline's lifecycle hooks are `TaskStart`, `UserPromptSubmit`, `TaskComplete` and `TaskCancel` and none of them fires per tool call. Gemini's hooks were registered with a matcher of `"Bash"`, which is Claude's tool name; Gemini matches on its own and its shell tool is `run_shell_command`. Both printed `[OK] installed` and recorded zero rows. Cline is now MCP tier and its installer purges the three dead entries on upgrade; Gemini matches the real tool name and is Full tier.
- **`omni init --codex` could brick Codex CLI, and could not repair it afterwards (#351)**: uninstall removed `[mcp_servers.omni]` by skipping lines until the next `[`, so a sub-table such as `[mcp_servers.omni.tools.omni_session]` outlived its own parent. Codex then reads a server with no transport and refuses to start at all, `Error: config.toml:45:14: invalid transport`, which turns a working editor into a dead one by running uninstall. Install could not undo it either: its guard was `contains("[mcp_servers.omni]")`, which an orphan does not satisfy. Verified against Codex 0.144.6 in a copied `CODEX_HOME`: orphan alone and `codex features list` dies, parent restored and it runs, which also confirms OMNI's `type = "stdio"` schema was correct all along. Uninstall now removes the whole subtree and install repairs a missing transport.

## [0.6.14] - 2026-08-07

### Changed
- **The README says what each host can actually do, instead of listing five names under one verb (#349)**: it claimed OMNI "works with Claude Code, Cursor, Windsurf, Codex and Roo out of the box", and for the product's core function that is not true on Cursor. Established while closing #340: Cursor's only output-rewriting field is `updated_mcp_tool_output`, documented as "For MCP tools only", and `afterShellExecution` defines no output fields at all, so a hook cannot replace the output of a Shell, Read or Write tool there. `additional_context` is available and adds tokens, which is the opposite of the job. Driven through the 0.6.13 release binary with both documented Cursor payload shapes: zero bytes returned, zero rows recorded. `omni init --cursor` still installs and still does real work (the pre-hook on `beforeShellExecution`, the MCP server, shared session state); what cannot happen is distillation of command output. A reader was previously left to discover that by measuring. All six translations carry the corrected sentence, and `normalize_cursor` now records why teaching it Cursor's real payload shapes would change nothing.

### Fixed
- **A command that named a parseable output format was still being cut (#346)**: `az acr repository show-tags … --orderby time_desc --top 25 -o tsv` delivered 10 of 25 rows. `format::sniff` protects the shapes it can recognise and a single-column TSV has no delimiter to recognise, so nothing stopped the cloud distiller head-sampling it. Head-plus-marker is the worst possible cut for a time-ordered listing: the head is what the caller already knew and the tail is the history they ran the command for. The reported case turned on rows 18 to 20, where a bare-SHA tag lands before its prefixed sibling. An explicit `-o`/`--output` naming `tsv`, `json`, `yaml`, `name`, `jsonpath`, `go-template` or `custom-columns` now passes the payload through, because naming one of those is a statement that something downstream parses it. The *value* is the tell and not the flag, so `-o wide` and `-o table` render for a human and stay compressible. Measured over the recorded corpus: 48 traces carry the flag, 3 were being visibly shortened, and the 1.4 KB that buys back was loss rather than saving.
- **Gate 6 never ran on the commands that actually print secrets (#344)**: the env redaction lived inside the env distiller, so it only fired when the registry decided the command was an env command. A bare `env` is `passes_through_verbatim` and `printenv` is not recognised at all, so both delivered their values exactly as printed. **Measured over 5,733 recorded traces, and the measurement is the whole argument**: 25 carry an env-shaped payload, 17 of those hold a credential that reached the model unredacted (`DB_POSTGRESDB_PASSWORD` eight times, plus an API key, an `SSH_PRIVATE_KEY` and an access token), and **not one of the 25 came from a command named `env`**. They arrive from `cd ... && ...`, `kubectl exec`, `sed`, `printf` and `export`. A gate keyed on the command could only ever have covered the case nobody runs. The redaction now sits on the payload, at the same dispatch choke point as the JSON guard, and returns the input minus its secrets rather than continuing to a summariser that might drop the line instead. Known and accepted cost: `SENSITIVE_PATTERNS` matches on substrings, so a public key and a public URL variable are redacted too, 2 of the 17 in the corpus. Over-redaction leaves the key name visible, the line in place and the value in the rewind archive; under-redaction puts a password in the transcript, so the asymmetry decides it.
- **The env distiller dropped every value and kept only the secret it had redacted (#342)**: it emitted per-prefix counts and no values at all, so a nine-variable `kubectl exec … env` came back as `env: 9 vars | REDACTED: 1 sensitive` over `DB(8) APP(1)` plus the password line. The DB host, port, database, user, schema, SSL flag, driver and app host were gone, and the one value surfaced by name was the only one that must never be printed. `DB(8)` answers "which DB vars are set" while the question anyone runs the command for is "set to *what*". `env` output is an enumeration, the shape `passes_through_verbatim` already protects for `ls` and `ps`, and the only reason to touch it is the secret in it, so it redacts rather than summarises: every line survives, sensitive values become `[REDACTED]`, a one-line header appears only when something was hidden, and a payload with no `KEY=VALUE` line is returned untouched. **Two guards in the hook had to go with it, and without them the fix would have been a security regression rather than a fix.** Redacting makes the output *longer* than the input, because `[REDACTED]` is wider than the secret it replaces, so the guardrail restore handed the raw bytes back under "nothing worth a deletion" and put the password on screen; and a distillation that saves nothing is dropped entirely before it reaches the agent, which left the host's own plaintext in place. Both now make an exception for a redaction. The first version of the security test could not fail, because a four-line fixture sat under the five-line floor in `is_env_output` and never reached the redactor at all; it is nine lines now and goes red when either guard is removed. Probing that also turned up #344, filed separately: a bare `env` is `passes_through_verbatim`, so Gate 6 does not run on it and only the piped form redacts, by accident.
- **A JSON line is handed back whole, because the payload-level format gate is all-or-nothing (#334)**: two JSON API responses printed beside a single `pod "vmq" deleted` notice are not NDJSON, so `format::sniff` returned `None`, format-safety switched off for the whole payload, and the cloud distiller delivered **17 bytes of a 460 byte answer** with both result sets gone. The report blamed the command head; that was wrong, `sniff` has always been content-based, and the correction is on the issue. The guard now sits ahead of distiller dispatch rather than in the scorer, because a distiller re-parses raw input and never sees a tier. **Measured before choosing the shape rather than to justify it afterwards, and the measurement killed the first design**: relaxing `sniff_ndjson` to a majority-of-bytes rule, which is what this set out to be, would newly protect 7 of 5,514 recorded traces, and the unanimous rule it replaces fires on exactly none of them. The line-level guard covers the 116 traces holding a JSON line and costs **0 KB** of the 371 KB the corpus saves, because none of those was being usefully compressed anyway. Both figures are in the source comment, because the first is the one that would have justified the wrong build. An earlier revision also tiered JSON lines in `semantic.rs`; it changed nothing once the dispatch guard existed and was reverted rather than left as a second guard for one job.
- **An already-aggregated histogram was routed to the pod-table distiller and lost its spikes (#338)**: `kubectl … logs … | awk … | sort | uniq -c` is a histogram by the time OMNI sees it, but `uniq` was not in `RESHAPING_TAILS`, so the payload was still claimed by the `kubectl` head. Of 40 rows, 10 were delivered, and the two traffic spikes the query existed to find were both in the 30 that went. `uniq -c` prepends a count column and so emits a grammar its input did not have, which is the same test `wc` and `column` already pass. Bare `uniq` only dedupes and by that reasoning belongs with `sort`, but splitting the two costs an argument check for no measured gain: of 4,335 recorded pipelines, 55 end in `uniq -c` and none in a bare `uniq`.
- **A lossless regroup was reported as data loss (#335)**: `distill_grep_output` folds a repeated `path:` prefix into a header, so 11 matches come back as 15 lines holding all 11 and every line number. The byte ratio lands in `Soft`, so the output carried `[Partial signal - omni learn recommended]` and `274 bytes omitted`, both telling the reader a complete answer was incomplete. A distiller that emits more lines than it consumed restructured rather than cut, and that condition now suppresses both markers in each hook path. The test is `>` and not `>=` on purpose: an in-place shortening leaves the count equal and *is* real loss, so it still reports its byte figure.
- **A vitest suite that never loaded was reported as four test failures (#333)**: when a transform error stops the suite loading, vitest runs zero tests and says so, `Tests  no tests`. `failed_tests` parsed as 0 and the count fell through to the length of the `❯` list, which in that payload is the transform's stack frames, so the delivered line read `vitest: ✓ 0/0 | ✗ 4 [node.js:4090, …]` for a run where nothing executed. The `[TSCONFIG_ERROR] Failed to load tsconfig …` line, the only actionable one in the payload, was deleted. A failure counter is the number a reader trusts without checking, and inventing four of them sends the reader to debug imaginary assertions. A distiller that cannot parse its input returns the input.
- **Cursor's post-hook was registered on `afterFileEdit`, so no command output ever reached the distiller (#340)**: `install_omni_hooks` wired `--post-hook` to a file-edit event. A file write carries no stdout, so the half of the integration that produces the savings was fed nothing for its entire life, while the pre-hook on `beforeShellExecution` was correct, which is what made the wiring look plausible. Cursor's actual event names, read off a `~/.cursor/hooks.json` another tool had already populated rather than guessed: `afterAgentResponse`, `beforeMCPExecution`, `beforeShellExecution`, `beforeSubmitPrompt`, `postToolUse`, `postToolUseFailure`, `preToolUse`, `stop`. `postToolUse` is the analogue of Claude Code's `PostToolUse` and `afterFileEdit` does not appear at all. `postToolUseFailure` and `stop` are now wired too, matching what `claude.rs` registers for `PostToolUseFailure` and `SessionEnd`; Cursor has no `PreCompact`, so nothing maps to it. **`omni doctor` reported the broken wiring as installed**: it grepped the whole file for `--post-hook` and printed two fixed labels, so the check passed whatever event the command sat under, which is the same defect as #158, a hook contract verified against our own field name rather than against the host. It asserts the event per hook now and names the missing one. Install also purges OMNI's own entries before re-adding, so the dead registration is removed on upgrade instead of left firing, and a reinstall after the binary moved leaves one command rather than two. The evidence is the code and Cursor's own event list, not the telemetry: `distillations` holds zero `cursor` rows against 9,857 for `claude_code`, but OMNI had never been installed into Cursor on that machine either, so the zero proves it never ran rather than proving the event was wrong. Installing the fix on a real Cursor writes the four events correctly and leaves another tool's eight hooks untouched, which is verified; that a `postToolUse` payload then reaches and satisfies `--post-hook` is not, so #340 stays open until a Cursor user sees a distillation row appear.
- **An environment-variable prefix defeated profile routing, so `FOO=bar cargo test` was not recognised as a test run (#339)**: `is_assignment` already existed, but it only decided whether a whole chain *segment* was silent, and a single-segment command never reaches that branch. Every caller that reads a command head therefore read the assignment as the program: `OMNI_DB_PATH=/tmp/d.db kubectl get pods` resolved to `Generic` where the bare command resolves to `Infra`, `FOO=bar cargo test` lost the test profile entirely, and `sole_output_command` handed back the string with the prefix still attached, which no TOML filter keyed on `^kubectl\b` can match either. One shared helper strips leading assignments and the four places that read a head use it, rather than each re-deriving the rule. Two smaller defects found in the same probe went with it: `post_tool.rs` scored the payload with `resolve_profile_for_chain` and then re-read the profile with `resolve_profile` twenty lines later, so on `cd /tmp/x && kubectl get pods` the segment counts and the `queue_for_learn` decision were built from a profile that never ran; and `filter_name` recorded the raw first token, writing the assignment and its value into the column, 1,079 rows and 264 distinct values in one database. Measured before choosing the shape rather than to justify it afterwards: env-prefixed commands are 1,082 of 9,812 recorded locally and save 14.9% against 22.9% for the rest, worth about 112 KB across the corpus. Small, and a one-line strip rather than a parser is what that buys. The report also claimed compound `cd X && cmd` was misrouted; it is not, #264 already handles that, and the claim came from reading the broken `filter_name` column, which is the third defect demonstrating why it mattered.


## [0.6.13] - 2026-08-04

### Fixed
- **The import graph counted directories as modules, so `imported_by` was wrong wherever it is read (#258)**: `resolve_candidate` accepted any candidate path that `exists()`, and a brace-grouped `use crate::pipeline::{CollapseMode, SegmentationMode}` leaves the extractor holding `crate::pipeline::`, whose candidate is the directory `src/pipeline`. Directories exist, so they won the lookup and became graph nodes. Measured on this repo: `src` held 113 dependents and `src/pipeline` 30, which is 143 of the graph's edges pointing at two directories, while `src/pipeline/mod.rs`, the file all thirty of those imports actually name, had none. `omni_context` reports that number today. One word fixes it, `is_file()` for `exists()`, because `mod.rs` and `index.*` were already later in the candidate list: brace-grouped imports now land on the module file, and `use crate::{a, b}`, which names no single file, resolves to nothing instead of to `src`.
- **Dependabot was watching the one manifest with no dependencies in it (#300)**: `.github/dependabot.yml` pointed its npm ecosystem at `/`, where `package.json` is `private: true` and declares no dependencies at all, so that entry has never had anything to open a PR for. The two directories that do have dependencies, `plugins/pi` (a devDependency on `@earendil-works/pi-coding-agent`) and `plugins/openclaw` (a runtime dependency on `@sinclair/typebox`, pinned exactly and three patches behind), were watched by nothing. `npm audit` is not part of `make ci` either, which is why #138, #139 and #300 are the same transitive pin found three times by a human running `npm audit` on a hunch. Both directories are watched now. The advisory itself is still open upstream and still cannot be closed from here, which was re-verified rather than assumed: `@earendil-works/pi-coding-agent@0.83.0` is the newest release, its `npm-shrinkwrap.json` pins `brace-expansion@5.0.7` and `undici@8.5.0`, and a consumer `overrides` block naming `^5.0.9` and `^8.8.1` reinstalled both at the shrinkwrapped versions. What did move since the issue was filed is the count: three advisories now, not one, because a second brace-expansion advisory raised the floor again and five undici advisories arrived. None of it reaches anyone: `npm audit --omit=dev` on `plugins/pi` is 0 of 0, and the published `@fajarhide/omni-pi` ships `index.ts` and `README.md` with no dependencies. Three type imports in `plugins/pi/index.ts` that no line referenced went with it.
- **A `grep` at the end of a pipeline now owns its output, so a matched line cannot be scored away as noise (#326)**: `kubectl … logs … | grep -iE 'mcp|slack|error|warn|ready'` delivered one line of fifteen. Routing reads a pipeline as belonging to its first stage unless the last one reshapes the payload, and `grep` does not reshape anything, so `kubectl` kept the buffer and `distill_kubectl_generic` kept `is_critical` lines only. The line it kept was the `ERROR`; the fourteen it dropped included the `WARNING` the pattern named by name, `3/3 MCP servers connected` and `Bolt app is running!`. What arrived said the pod had failed to start, and the evidence that it had started was behind the retrieval marker. #316 established the rule that a grep pattern **is** the caller's filter and fixed it inside `system_ops`, where the payload arrives without its command, so it could only ever recognise a `grep -n` by shape and never covered a piped one. The rule sits at the routing boundary now: a `grep`/`rg`/`ag` tail claims the payload, and the grep path is the only distiller allowed to touch it. It hoists repeated paths when that shrinks the output and hands the input back when it does not, so neither outcome can lose a match. The same names joined `passes_through_verbatim`, because routing alone leaves the collapse fallback free to eat what the pattern selected: with those three names removed, 121 matched lines came back as `120 INFO entries (collapsed from 120 lines)` over a `119 lines omitted` marker. Measured over 3,392 recorded traces before choosing the rule: 429 pipelines put a `grep` after a pipe, `kubectl … | grep` averaged 76.8% of its bytes kept with individual rows at 5% (1,793 → 95 on a `get pods -A`), and of 214 bare `grep`/`rg` traces the lossless hoist fires on 46 for 13.4 KB while 28 more came back carrying a `[Partial signal]` marker over lines the pattern had matched. The reductions this gives up are the ones produced by deleting matched lines.
- **The benchmark behind the published headline still counted terminal bytes, so the README overstated savings by 36 points (#324)**: `tests/bench_replay.rs` read `SELECT command, raw_input FROM execution_traces` with no agent filter. On the reporting installation that corpus is 68% `terminal` by bytes, 888 traces carrying 86.0 MB of TTY output no model ever receives, against 13.9 MB from `claude_code` and 25.7 MB from `aider`. #212 established exactly this and fixed it in `omni stats`, which now prints "Terminal output is excluded" on its own line; the harness that produces the number the README quotes never got the same fix. Replaying the same pipeline over the same database, one run each: **everything, 126.0 MB to 26.3 MB, 79.1% net**; **model-facing only, 40.1 MB to 22.7 MB, 43.3% net**. The harness replays the model-facing population by default now, prints which population it used, and `OMNI_BENCH_ALL=1` asks for the wider one. Both are worth having, because the gap between them is the mistake, and hiding one of the two is how it comes back.

### Changed
- **The import graph will not be wired into the scorer, and the measurement is why (#258)**: the proposal was to feed `imported_by.len()` into `scorer::score_segments` so a file forty others import survives a cut that a leaf file does not. Sampled against 2,764 recorded traces before building any of it, which is what the issue's own "Not measured" section asked for. Only **127** of them name a file this repo's graph knows, **52** name two or more, and **26**, under 1%, name two files whose dependent counts differ, which is the entire population a dependents signal could re-rank. Building the persistence and mtime-invalidation layer the issue calls "the actual work" would buy a tie-break on fewer than one call in a hundred, against a `build_graph` that takes **542 ms** on this repo where `AGENTS.md` gives the whole hook 10 ms. The sampling also found the graph was wrong, which is the fix above and the reason those figures are the corrected ones; before it, the same probe read 196, 95 and 47 because two directory nodes were absorbing the edges. Reopen it if the corpus changes shape, with the same query.
- **The command-to-distiller decision moved into `pipeline/registry.rs`, where `CLAUDE.md` says it belongs (#194)**: `distillers::route` was twenty `if matches!(base, …)` blocks, the standing violation of the SSOT guardrail, and #323 had already closed the half of it with a demonstrated cost. What kept the rest open was that a `&str -> distiller` map cannot express it: `cargo` routes by subcommand, `git` and `gh` decline on a flag, and the JVM and Dart arms split on whether the command line mentions a test. So the registry answers with a `Distillation` enum that has a `Passthrough` variant, and `route` is one `match`. The ten command-reading predicates went with it, so `passes_through_verbatim`, `wraps_another_command`, `lists_containers`, `lists_kubectl_names`, `git_enumerates_files`, `is_vcs_list_command` and `cargo_subcommand` now live beside the mapping that uses them; the two hooks call `registry::passes_through_verbatim` instead. Order is preserved exactly, because order is the behaviour here: `mvn` and `gradle` appear in both the JVM and build-tool arms, `kubectl … exec` has to reach `wraps_another_command` before the cloud arm claims it, and the grep arm has to answer before `passes_through_verbatim`. Verified by differential rather than by reading: every one of **5,227 commands** (5,125 distinct ones recorded in `~/.omni/omni.db`, plus 102 hand-picked for the arms the corpus does not cover, from `cargo tree` to `az aks command invoke` to `git show --stat`) was run through eleven probes shaped for eleven distiller families, before and after, and the 57,497 outputs are byte-identical. The harness was then shown to have teeth by routing `psql` to the generic distiller, which moved 16 commands. One duplicated rationale block was deleted rather than moved: the routing arm carried its own copy of the enumeration reasoning that `passes_through_verbatim`'s doc already states, and one line of that copy had already gone stale on #326. Test count is unchanged at 614, with four moved to sit beside the functions they cover.
- **README cut from 370 lines to 219, and `docs/PERFOMANCE.md` deleted**: ten `##` sections meant half the page was never read while still standing between a visitor and the install command. `How it works`, `Architecture` and the Memory OS section moved to `docs/ARCHITECTURE.md`; the corpus, per-command table, fixtures and latency tables moved to `docs/BENCHMARKS.md`, which also absorbs the method block `tests/bench_replay.rs` cites. `Why This Matters` and `Integrations` were deleted outright, because the hero already says which agents are supported and the intro already says what the problem is. `docs/PERFOMANCE.md` went with them: it published 58.9% over an 1,810-trace corpus that #324 superseded, and a second performance document with a different corpus is how a stale number survives a correction. Nothing measured was dropped, and every figure that moved to `docs/` is still reachable in one click from Benchmarks. All six translations follow the same shape.
- **New section: how to read a savings number, including ours**: five questions with our answers, which is the only comparison worth making. A feature-by-feature table against named competitors was drafted and thrown away after checking their READMEs: reversibility is not a differentiator (headroom caches originals for retrieval, lean-ctx documents five recovery paths), passthrough is not either (lean-ctx ships 270 passthrough rules), and rtk already publishes its own caveat that cutting 90% of bash output is not cutting a bill by 90%. Three of the five rows would have been false. What no tool in the category publishes is the share of calls where it saved nothing, so that is the row we lead with, and it names nobody.
- **README leads with what OMNI does well instead of with its worst aggregate**: not one figure changed, and none was rounded. The old hero opened on a category description and buried the per-command results under a 43.3% whole-mix average that includes thousands of commands OMNI deliberately does nothing to. The new one opens on `git` 89%, `cargo` 91%, `kubectl` 77%, which is where the saving actually is, and the 43.3% still appears in Benchmarks with the same corpus and the same caveat. "90.0% of calls saved nothing" now reads "OMNI acts on 1 command in 10, and adds zero bytes to the other 9", which is the same fact stated as the design decision it is. "Latency is a real cost, not zero" leads with the measured 21 ms and keeps the 205 MB row underneath it. Two guarantees were promoted out of a table into prose, because RewindStore recovery and the `Option<String>` return that makes fabrication unrepresentable are the two things no competing compressor offers, and they were the smallest text on the page. The line about publishing the share of calls where we did nothing stays, sharpened: a tool claiming 90% on every command is telling you it summarised something you needed. All six translations follow, and three stale figures went with them: the `git diff` 45% and `docker build` 37% that predate the retrieval marker, a `kubectl get pods` row still promising a `35 pods | 30 running, 5 error` summary the pipeline stopped producing, and Arabic's 82/308/276 ms latency block.
- **Every published number re-measured on 0.6.12, and most of them moved down**: the README carried 58.9% net over an 1,810-trace corpus with 63.6% of calls saving nothing. Measured now over 9,965 model-facing traces: **43.3% net, 90.0% saving nothing, and not one call in 9,965 that made the output larger**. The drop is not a regression, it is this release's correctness work showing up in the accounting: `Build: ok` no longer answers for commands that never built (#250), `make`, `kubectl` listings, single-file `grep -n`, green `npm test` and reshaping pipeline tails all pass through now (#129, #301, #316, #310, #277), and each of those passthroughs used to be counted as a saving. The fixture table moved for a different reason and by a constant: every row is ~77 bytes larger because #271 attaches a retrieval marker whenever anything is dropped, so `git diff` reads 25.2% delivered against the 44.6% earlier releases quoted for the distiller's output alone. The marker is what makes the cut reversible, so the table counts it and says so. `kubectl get pods` went from 9.3% to 0%, which was the fix. Latency moved the other way: a 496 B `git status` is **21.1 ms** on a fresh database and **60.7 ms** on a 205 MB one, against the 82 ms and 308 ms the README promised, and a 16.5 KB `cargo test` is 24.5 ms rather than 276 ms, after #283, #296 and #174. All six translations carry the same figures, and their hero lines said "tokens" for a benchmark that measures bytes; they say bytes now.


## [0.6.12] - 2026-08-03

### Changed
- **The two lists that had to agree are one list (#194, in part)**: #277 added seven names to `registry::reshaped_by` and to `distillers::passes_through_verbatim`, in two files, and the only thing keeping them in step was a comment telling the next person to keep them in step. Getting the first without the second is not a style problem: on the first run of #277 it sent `gh api … --jq '.content' | base64 -d` to the generic distiller, and that decode produces a source file an enumeration cut would shred (#235). `RESHAPING_TAILS` is the list now and both sides read it. A test walking the list against the other side was written and then deleted, because once both sides read the same constant it asserts that every member of a list is in that list and cannot fail; the constant is the mechanism, which is the whole point of removing the comment that used to be. The rest of #194, moving nineteen `matches!` arms of command-to-distiller mapping out of `distillers/mod.rs`, stays open: it is roughly 600 lines of mechanical move with no user-visible change, and the part with a demonstrated cost is this one.

### Added
- **`omni stats --share`, a copy-pasteable summary of your own measured savings (#135)**: reuses `multi_period_stats`, the same aggregation the default report reads, so the figure printed here is the figure `omni stats` shows and cannot drift from it. Two decisions make it a real number rather than a growth claim, and both were arguments this project has already had with itself. It prints the **net** all-time percentage and never a per-command peak, because `kubectl kustomize` reports 99.8% on the reporting installation and quoting that would be the cherry-pick the changelog spends its length arguing against. And it prints one decimal place, because `{:.0}` renders that 99.8% as `100%`, which is a figure the tool never measured. The card also states that terminal output is excluded, which is #212's fix and the difference between the 64.5% a user reads today and a headline that once counted 86 MB of TTY bytes no model ever received. The no-data path prints one sentence and exits 0; `tests/smoke_test.sh` drives both branches against the shipped binary, because a growth surface that panics on a fresh install is worse than not having one.


### Fixed
- **Every hooked `Read` rebuilt the whole import graph, 48 ms, to sometimes append one advisory line (#320)**: `hooks/post_tool.rs` called `graph::indexer::build_graph(&cwd)` unconditionally on the Read arm and passed the resulting count into `distill_readfile_with_context`. That count has exactly one use, the dependents guard at `readfile.rs:50`, and the guard sits behind two gates that reject most payloads: `estimated_tokens >= MIN_DISTILL_TOKENS` (2,000) and a distillation that actually shrank the file by a fifth. So a `Read` of any small file walked the repository and threw the answer away a few lines later. Measured on the release binary against this repository, 125 files indexed: **116.6 ms cold, 48.0 ms median warm**, against an `AGENTS.md` budget of 10 ms for the whole hook and a Bash post-hook that measures 18.9 ms end to end. `MAX_FILES` is 5,000 and there is no cache, so a larger project pays more. The count is passed as `impl FnOnce() -> usize` now and consulted at the guard, so the walk happens only when the line it feeds is about to be printed. The test asserts the closure is **not called** for a file below the threshold, because "it compiles with a closure" was equally true of the version that still walked every time. This became reachable rather than being missed: #258 recorded the Read arm as unreachable on 2026-07-29, and #172 widened the `PostToolUse` matcher to `Bash`, `Read`, `Grep` and `WebFetch` in 0.6.9 after it. #258 itself stays open and still describes something else, feeding `imported_by` into the scorer. The now-unreachable `graph failed` fallback goes with it, and `distill_readfile` becomes the test-only helper it had already turned into.


## [0.6.11] - 2026-08-03

### Fixed
- **A pipeline that ends in a stage rewriting its payload is routed to that stage (#277)**: the exception #269 added covered `jq` and `yq` only, so `kubectl get pods | cut -d' ' -f1` was still handed to the kubectl distiller, which describes a table the command no longer emits. The measurement this issue asked for now exists, over **4,958 recorded pipelines**, and it rejects the design the issue proposed. Classifying every stage and routing to the last transformer is worse at both ends: routing by the last stage regardless hands **69.1%** of pipelines to `head`, `tail` or `grep`, all verbatim, and stops distilling two thirds of every pipeline anyone runs; while only **7.5%** end in a reshaping stage at all, and the residual's dominant first stages are `cd` (119), `echo` (32) and `for` (23), which have no distiller to claim the payload anyway. Genuinely at risk, a real distiller upstream of a real reshaper, is about **1.3%**. So the shape stays a list of names and what changed is that it is the measured list: `cut`, `tr`, `awk`, `base64`, `wc`, `column` and `xargs` join `jq` and `yq`, each because it provably emits something that is not its input's grammar. `sed` and `sort` are deliberately excluded: they leave the shape intact and are 334 of the recorded tails between them, so treating them as reshapers would stop distilling a pod table because somebody sorted it. The same names go on `passes_through_verbatim`, because naming a stage as the owner only helps if that stage is then handled; the existing `gh api … | base64 -d` test caught that immediately, since the decode produces a source file an enumeration cut would shred (#235).


### Fixed
- **A `grep -n` over one file lost 7 of its 12 matches, keeping prose and dropping config (#316)**: grep only prefixes the filename when it was given more than one file, so a single-file `grep -n` prints `<lineno>:<text>` and nothing else. `is_grep_output` required the part before the colon to look like a path, so that shape was not recognised as grep output at all and fell through to the generic system-ops fallback, which keeps Critical and Important segments and drops the rest. On a 12-match grep over a Grafana alerting manifest the agent received 5 lines: four of runbook prose and one expression, while `expr: absent(probe_success{tier="carrier"} == 1)` and the alert `summary` template were cut. An agent reading that concludes the file has one alert expression when it has two, and that no rule uses `absent()` when one does. A grep pattern **is** the caller's filter, so every line in the result was asked for by construction, and scoring those lines by noise is a second filter that cannot know what the first was looking for. The reproduction used the reporter's own archived bytes from `rewind_store` rather than the excerpt in the report, because the report's own quoting had elided the ends of four lines and a shorter fixture routes differently. Widening the detector is the safe direction here, which is why it was chosen over a new arm: `distill_grep_output` hoists losslessly and hands back the input whenever hoisting does not shrink it, so a payload misrouted *into* it cannot lose a line. The reporter also noted the marker said 6 omissions for 7 dropped lines; that half was already fixed by #306 and reads 7 on this build.


### Fixed
- **`OMNI_HOME` did not reach the code that reads filters, so `cargo test` still loaded the developer's live `~/.omni` (#315, #217)**: #308 added the override and verified it by measuring *writes*. `toml_filter::resolve_user_signal_dir` derived `~/.omni` itself, so it was outside the override entirely, and it runs on every hooked command. That left the other half of #304 in place: filters from a developer's home still joined the `find()` race that decides which signal claims a command, so the suite's own filter set depended on whose machine it ran on. Every path in the tree resolves through `paths` now, including nine call sites that each rebuilt `dirs::home_dir().join(".omni")` (`cli/reset.rs`, `cli/doctor.rs`, `cli/diff.rs`, `hooks/session_end.rs`, `hooks/pipe.rs`, `guard/update.rs`, `store/transcript.rs`, `agents/hermes.rs`, `cli/learn.rs`). Verified on the loaded set rather than on the resolved path, because the path was right once while the set was still wrong: after a full `cargo test --all`, the hook loads **45 filters, all of them shipped, none learned**. The last writer into the shared home went too, an MCP test that called `omni_learn(apply: true)`; the write path is now tested against a path the test owns, which `apply_to_config` already accepted.
- **The XDG Base Directory spec is honoured for a fresh install (#217)**: `config.toml`, `filters/`, `signals/` and `trusted.json` resolve `OMNI_HOME` → `OMNI_CONFIG_HOME` → an existing `~/.omni` → `$XDG_CONFIG_HOME/omni` → `~/.omni`; the database, transcripts, caches and exports take the same ladder through `OMNI_DATA_HOME` and `$XDG_DATA_HOME`. **An existing `~/.omni` deliberately wins over XDG**, which is the migration decision: an install that already has a tree keeps using it, so upgrading cannot appear to lose a database. Nothing moves on its own. With no environment set every root is still the same `~/.omni`, so this changes nothing for anyone who has not asked for it.
- **A scoring feature that was read on every line and written nowhere (#164)**: `SessionState::scoring_modifier` was consulted in two `scorer.rs` branches, one of them per line, and assigned in no production path, so `GoalScoringModifier` was permanently `None` and both branches were dead weight in the hot path. That also settles the question #164 left open: `goal` looked load-bearing because the scorer read something only `omni goal` could set, and the scorer could not read it at all. `omni goal` keeps earning its place through the SessionStart injection, where `build_knowledge_context` emits the stored `__omni_goal__` at confidence 1.0. Six tests went with the feature: five macro-generated copies asserting the field survived a serde round-trip, and one whose own comment said it "initialize[d] it manually to simulate L3" before asserting `is_some()` on the value it had just assigned.

### Changed
- **The connection pool opened four SQLite connections before a run-once hook did any work (#174)**: r2d2 defaults `min_idle` to `max_size`, and `build()` blocks until that many connections exist, each through the pragma customizer. Measured on the release binary, median of 10 opens: **3.19 ms to 1.26 ms** with `min_idle(Some(1))`, against 0.81 ms for a bare `Connection` carrying the same pragmas. One line recovers 73% of the cost. The pool itself stays, which is the recorded answer to the last open item on #174: removing it is 62 `self.pool` call sites for the remaining 0.45 ms, and the MCP server shares the type while serving over a single stdio stream, where a pool costs nothing and the ticket's own caveat asked for it to be checked before cutting.


## [0.6.10] - 2026-08-03

### Removed
- **Five direct dependencies that nothing in the tree references (#174)**: `indicatif` was already dead when that audit was written, and the `Cargo.toml` comment beside it described progress bars for heavy operations that were never built. The other four went dead since, because #251 and #298 deleted their callers: `console` (4 files at the time of the audit), `lru` (1), `unicode-segmentation` (1) and `glob` now all grep to zero across `src/`, `tests/` and `benches/`. Dropping them takes `Cargo.lock` from 299 packages to 290, since `console` and `indicatif` between them carried `number_prefix`, `portable-atomic`, `web-time` and a second major of `foldhash`, and `lru` carried `allocator-api2`. This is not a binary size crusade: the cost of a dead dependency is that it is a supply chain entry and a standing upgrade obligation that buys nothing back. What the same audit listed and this change keeps is kept on purpose, each with one real caller: `dialoguer` for the interactive `omni init` prompt, `is-terminal` for the pipe's TTY check, `strsim` for unknown-flag suggestions and correction matching, `unicode-width` for column alignment. The two subsystem questions on that ticket are not deletions and stay open: `tiktoken-rs` is the `cl100k_base` half of #283, a product call about which model's vocabulary a reported token count is counted against, and the `r2d2` pool needs a measurement of the MCP server, its one long lived caller, before the single shot hook path is moved to a plain connection.

### Changed
- **64% of every hooked command was loading a GPT vocabulary to fill a reporting column (#283, #174)**: `util::token_estimate` carried two estimators for one quantity, an exact `cl100k_base` count and a bytes-per-token heuristic, and callers picked one. `stats.rs:153-167` picked both: `tokens_saved` came from the BPE numbers when the row had them and from the heuristic when it did not, so one column in one table was computed two different ways depending on which path filled the row. The exact one is gone. Measured by removal on the release binary, same payload, 10 runs each, median wall clock of the post-hook: **53.8 ms to 18.9 ms**. That is **34.3 ms per command** for a number nothing acts on, against an `AGENTS.md` budget of 10 ms for the whole hook. The test suite's own lib target went from 18.5 s to 2.8 s for the same reason. What the exact count bought was 4.9%: calibrated against `cl100k_base` over 4,000 real traces from this installation, 37.9 MB, the aggregate is **3.614 bytes per token**, median 3.61, p10 2.94, p90 4.13, and `ContentHint::Mixed` was 3.8, so it now reads 3.6 because that is what was measured. It was also the wrong vocabulary: `cl100k_base` is GPT-3.5/4's encoding, not Claude's, so it answered a question nobody asked with four decimal places, which is the failure mode this tracker is named after. The budget guard in `pre_compact` deliberately does not use the central estimate: it uses `ContentHint::Json` at 2.8, the conservative end of the measured range, because under-counting tokens is the direction that overshoots a cap. `tiktoken-rs` goes with it, taking `Cargo.lock` from 290 packages to 283 and the release binary from 11.74 MB to 9.91 MB.

### Security
- **protobufjs DoS cleared in `plugins/pi` (#139)**: `@earendil-works/pi-coding-agent` 0.81.0 shipped an `npm-shrinkwrap.json` pinning protobufjs in the vulnerable 7.5.0 to 7.6.4 range (GHSA-j3f2-48v5-ccww), and a dependency's shrinkwrap beats a consumer `overrides`, so the only route was an upstream release. 0.83.0 is that release. `npm --prefix plugins/pi audit` no longer reports it. This is a dev-only transitive of the `pi` plugin's tooling and never reached the Rust binary. The same audit now reports a high-severity brace-expansion advisory (GHSA-mh99-v99m-4gvg, fixed in 5.0.8) that 0.83.0's shrinkwrap still pins at 5.0.7, which is the identical mechanism arriving under a new advisory number; `npm audit fix` advertises a fix for it and then cannot apply one, for the same shrinkwrap reason. Filed separately rather than left implied.

### Fixed
- **A background thread printed OMNI's console text into a piped payload (#312)**: `session::learn::queue_for_learn` spawns a thread, and once the queue passed 50 lines that thread called `cli::learn::run_learn`, which is a user-facing printer. In pipe mode stdout **is** the payload channel, so its output landed inside the data. Observed on `make ci (ubuntu)`: a 396 B git diff was delivered as **545 B**, the diff intact and then 149 bytes of `ℹ No learning data available yet. / OMNI collects samples of repetitive noise as you use it. / Run this again after you've processed more output.` appended to it. Anything downstream that parses that output gets a syntax error it cannot attribute. It was green on macOS on every run, cold home and warm, because the process normally exits before the thread is scheduled, which is the worst property this class of bug can have: a test can pass everywhere and a user can still hit it. Queueing stays and the unattended apply is gone; `omni learn --apply` still does it on request. The filters that apply was writing behind the user's back are the ones #307 measured accumulating to 1.8 MB, and #266 had already narrowed the suggestions they are built from. The same function also resolved its own directory with `dirs::home_dir().join(".omni")` rather than asking `paths::omni_home()`, so `OMNI_HOME` did not cover the queue and the writer and the reader disagreed about which home they meant, which is what made the notice fire at all.
- **A green `npm test` came back as its own echo line, with the verdict deleted (#310)**: `npm test` routes to `JsTsDistiller`, TAP is none of the five shapes it detects, and it landed in `distill_fallback`, which on a payload where nothing scored Critical or Important sampled the first line of each of the first ten segments. Measured on a green `node --test` run through the release post-hook: **804 B and 44 lines came back as 122 B and three**, the first of them npm's own echo of the script name. `1..6`, `# tests 6`, `# pass 6` and `# fail 0` were all gone, so the reply answered "npm test ran" to a question that was "did it pass", and cost a round trip to find out. Not hypothetical: 41 recorded traces carry the `… and N more` marker and the npm ones are this shape, one of them 50 lines and 970 B in against 2 lines and 45 B out. The asymmetry is the dangerous half. The same payload with one failing subtest passes through untouched, because failure vocabulary tiers those segments Critical, so a green run lost its result while a red one kept it and `… and 42 more` looked identical either way. The branch's own comment said it sampled "rather than return nothing at all", which was a reasonable sentence before #250 and is not one now: the head of a payload the distiller could not parse is not a summary of it, and for a test runner the verdict is always in the tail. It returns `None`, which the trait now expresses and the dispatch boundary already handles. Checked rather than assumed on the way out: a 300 line `npm install` deprecation wall, 21,680 B, was already passing through on `main` before this change, so the sample was not what had been compressing it and declining costs nothing there.
- **The test suite wrote auto-learned filters into the developer's live `~/.omni` (#307)**: `paths::omni_home()` had no override, so every path derived from it resolved to the real one during `cargo test`. Measured before the fix: `cargo test --lib` left 1 learned filter behind, `cargo test --all` left 11, and one `make ci` added **31,458 bytes** to `~/.omni/filters/learned.toml`, which had reached **1.8 MB** of filters nobody had asked for. `load_all_filters` reads that directory on every hook and #296 measured filter loading as the largest single cost on that path, so running the tests was quietly making the installed binary slower for the person running them. Worse than the cost, those filters then joined the `find()` race that decides which signal claims a command, which made the suite's own result depend on test ordering: on `make ci (ubuntu)` seven of them matched `black --check .` ahead of the shipped `sys_black` and stripped nothing, so the hook fell through and `still_applies_a_batch_filter_when_a_signal_survives` failed twice and then passed (#304). `OMNI_HOME` is read now, once through a `LazyLock` rather than per call, because a value re-read under a parallel test is the process-state hazard this repo has already paid for; `.cargo/config.toml` points it at `target/omni-home` for anything cargo launches, and the Makefile passes the same path to `tests/smoke_test.sh`, which drives the binary directly and so never sees cargo's environment. Verified by measurement rather than by inspection: a full `cargo test --all` and a full smoke run each now add **0 bytes** to the real file. The override is also what #217 needs for XDG paths, and exporting `OMNI_HOME` yourself still wins, because the config entry deliberately does not set `force`.
- **A kubectl listing lost its whole tail, and the marker undercounted what went (#301)**: `kubectl config get-contexts -o name` prints one identifier per row and nothing else, so there is no status column to summarise. `distill_kubectl_generic` keeps only rows matching its critical vocabulary or `configured|created|unchanged|deleted`, and a context name matches neither, so a 20 row list arrived as 10. Because the estate sorts alphabetically the cut landed on the tail, and every `k8s-*` production cluster was the half that went: the delivered list looks complete and contains no production context at all, which is worse than a random ten. It joins `ls`, `find`, `docker ps` and the rest on the verbatim list, covering `-o name` on any subcommand plus `api-resources`, `api-versions` and the three `config get-*` listings. `kubectl get pods` keeps its distiller, because a pod table has a `STATUS` column with real noise and a `READY` + `RESTARTS` fingerprint that says so. The second defect in the same report is separate and general: the marker read `9 lines omitted` for 10 dropped rows. `rewind_marker` was handed a `final_out` that already carried OMNI's own `[Partial signal - omni learn recommended]` banner, so each banner line counted as content that survived and the loss was understated by exactly that many, on every command that reaches the banner path rather than only this one. It is now given the distiller's output sliced back to where OMNI's commentary begins. The reporter had already checked the important half: `omni_retrieve` on the hash returned all 20 rows byte-identical, so #271's guarantee held while the number describing it did not.
- **`make` is a composite runner, and one distiller was speaking for all of it (#129)**: a target runs whatever its recipe says, usually several programs, and their output arrives concatenated with no delimiter between them. `make check` is tsc, then eslint, then pytest. Routing `make` to `BuildDistiller` gave the whole buffer to a distiller that can only describe one of those, and on a run where each gate found something, 431 B came back as 111 B headed `Build: 1 errors, 0 warnings`: the eslint finding deleted outright, and a headline that counted neither the warning nor the failed test it printed two lines below. #106 covered the `npm run` shape of this by detecting npm's `> a && b` echo; `make` prints no such line, which is why that fix filed this one instead of guessing at the format. The answer turned out not to need a format at all. `make` joins `python`, `ruby` and `az aks command invoke` on the verbatim list under the rule this repo already applies: something that runs an arbitrary program has no grammar of its own, so the honest answer is the input. `gcc`, `clang`, `cmake` and `cargo` still route to the distiller when invoked directly, so a compile still compresses and only the wrapper passes through. Measured before choosing rather than to justify it afterwards: 0 of 9,832 recorded commands start with `make `, so this trades a compression that has never fired on this installation for a false claim that does. The clean half was already fixed one entry below, since a `make` run with nothing to report no longer answers `Build: ok`; this closes the half where a gate actually fails, which is the half that matters.
- **The zero-state guard was voluntary, so the tenth false claim was still shipping: `pip list` came back as `Build: ok` (#250)**: `Distiller::distill` returns `Option<String>` now, `None` means "I could not read this input", and `distill_with_command` hands the raw bytes back. The rule used to live in `require_parsed`, a helper each of twelve distiller files had to remember to call. Three called it, and all nine false claims recorded in this changelog over eight releases landed in a file that did not, which is the correlation that made this a class rather than a run of unrelated bugs. A sweep of all ten distillers, each fed output its own tool really produces but carrying none of the signal that distiller summarises, found exactly one fabricator left. `BuildDistiller` answered `Build: ok` whenever it counted zero errors and zero warnings, which is every non-build invocation of the sixteen tools routed to it: `pip list` 307 B to 9 B with every installed package gone, `go env` 297 B to 9 B, `make help` 420 B to 9 B, `dotnet --list-sdks` 152 B to 9 B. Confirmed on the far side of the hook contract rather than in a unit test, because that is the level that can be wrong: feeding `--post-hook` a real Claude Code Bash payload returned `updatedToolOutput.stdout` beginning `Build: ok` for `pip list`. A retrieval marker was attached, so this was a marked total loss and not a silent one, but the first line is still a verdict about a build that never ran, and `beats_guardrail` cannot catch it because 9 bytes out of 307 clears the threshold comfortably: the worse the fabrication, the better the reported saving. `Build: ok` is now gated on the output carrying a line only a build prints, and that list holds build *activity* rather than success banners, because a tool whose clean run is a single line (`ruff`, `mypy`, `black`) has nothing worth compressing and failing open on it costs nothing. After the fix the four commands above are handed back byte for byte with the hook applying nothing at all, while `cargo build` still distils 188 B to 85 B. `require_parsed` stays for the helpers several layers below the trait, now documented as an implementation detail rather than the guarantee. Two existing tests asserted the fail-open as `output == input` and assert `None` instead, which is the stronger claim, since the old form also passed for a distiller that happened to reconstruct its input.
- **246 lines of surface that had to be read, compiled and maintained without ever being called (#251)**: found by a repo-wide audit, each item verified to have zero callers outside its own file and tests. `OmniStr` was 68 lines behind a file-wide `#![allow(dead_code)]`, kept compiling by a re-export nothing consumed. `StorageBackend` was a 42-line trait with no implementations that had also drifted from the type it was meant to abstract, its `record_distillation` taking a different argument list than `Store::record_distillation` actually takes: a zero-implementation abstraction out of sync with the concrete type is a comment pretending to be code. `paths::worktree` was 56 lines whose own doc comment said it was infrastructure for a feature that would consume it later, and later never came. `format_tokens` and `format_insights` were called only by their own tests, which is a test proving a function still compiles and nothing else, so both went with them. Nothing here was a correctness bug and the diff is pure deletion, 246 lines removed and none added. `verification_results`, the fifth item on that ticket, was already dropped as part of #165.
- **The package shipped the one claim its own README argues against (#252)**: `Cargo.toml` sold OMNI as cutting token consumption "by up to 90%", which is the string crates.io renders, `cargo search` prints and most aggregators scrape. `README.md:135` says in as many words that a tool claiming to save 90% of every command is telling you it summarised output you needed. #131 replaced the inflated hero claims in the README and the package metadata was not carried along, so the one line most people see undercut the thing that makes the rest credible. It carries the measured figure now, the same 58.9% the README leads with, at 89 characters so it survives the truncation in `cargo search` output. Honest measurement is the differentiator here, which makes this a correctness problem in the packaging rather than a cosmetic one.
- **Every hooked command compiled 249 line-filter regexes so that at most one filter's set could be used (#283, in part)**: `match_command` is what decides whether a signal applies; `strip_lines_matching` and `keep_lines_matching` are read only by the filter that matched. Both loaders compiled all of them up front, on a path each hooked command pays because every hook is its own process. They are stored raw now and compiled on first use. Measured on the release binary, 20 iterations of the uncached loader, best of run: **19.78 ms to 6.40 ms**, mean 20.55 ms to 7.78 ms. The first attempt at this measured as no change at all, which turned out to be the change not reaching the path under test: `load_embedded_filters` builds through `create_filter_from_config`, and that builder was still validating, which means compiling. Deferring the cost must not defer the diagnosis, so `omni doctor` now force-compiles via `validate_line_patterns` and reports a malformed pattern the loader no longer sees. Verified by driving the shipped binary against a signal file containing `([unclosed`, not only in a unit test: doctor prints `invalid line pattern in signal 'deliberately_broken'`, and removing that call makes it silent. The `cl100k_base` half of #283 is untouched and remains a product call, so the issue stays open.
- **`omni_find_noise` offered one command's content as a permanent strip rule (#266)**: it concatenated every recent trace into one buffer before counting, so "four times inside one command" and "once in each of four commands" were the same number. A `gh issue list` row (`^\d+ CLOSED TestDistiller`, an OMNI issue title seen once) and a truncated Terraform variable description came back at confidence 0.85, framed as TOML to paste into `~/.omni/signals/user.toml`. That is a worse surface than a runtime distiller getting one command wrong: a bad suggestion outlives the session and applies to every future one. Detection now runs per trace and a pattern must have been produced by at least two distinct commands, because recurring noise is by definition what more than one command emits. Two classes are excluded by construction rather than by frequency, since no count makes them safe: OMNI's own channel markers, where stripping `[stderr]` hides that a command wrote to the error channel at all and leaves the reader no signal anything was hidden, and structural keys, where `spec:`, a lone brace, a markdown fence and a YAML document separator are the shape of the document rather than decoration, so a filter removing them corrupts the manifest instead of tidying it. The guard sits in `detect_patterns`, which `omni_learn` shares, so a single-command suggestion is covered by the same rule.
- **A declined composite buffer was handed back byte for byte, so 25 of 45 composite runs saved exactly nothing (#291)**: #106 taught the JS/TS dispatcher to decline a composite so that every gate's verdict survives instead of the first detector claiming the whole buffer, and that part was right and stays. What it did not do is compress what it hands back. `npm run verify` measured 23,249 B in and 23,249 B out on three consecutive runs. The generic collapse the #106 comment expects to fold this cannot: the noise in a composite buffer is not repetition but a bundler size table where every row carries a distinct path and a distinct size, so similarity grouping has nothing to group. On the measured trace those rows are 176 of 269 lines and 85.9% of the bytes. Folding only them, driven through the release binary rather than a fixture, takes 23,250 B to 3,436 B, an 85.2% saving, with the eslint findings and their line numbers, the secret-scan verdict, `all 11 checks passed`, `✓ built in` and every `> script` echo still present. The fingerprint is `kB │ gzip:`, a box-drawing character beside a specific label: a bare `kB` would match a test line, and this is the sibling-format rule the project already applies to durations (#242). The fold asserts nothing and summarises nothing, which is the point of declining, and where there is no table it returns the input unchanged. The `jsts_npm_composite` snapshot recorded the verbatim handback, which is to say it recorded the cost, and is updated. Filed with a wrong aggregate first: the original -0.9% came from a `LIKE` that caught six `omni exec npm i` rows rather than composites, and the corrected figure of 0.0% on 25 of 45 runs supports the ticket better than the wrong one did.
- **The eslint distiller reported how many problems there were and never where (#108)**: it parsed every problem's path, collected them into a `HashSet`, and then emitted `files_affected.len()` and nothing else. The reporter's run of `npx eslint src/lib/i18n.tsx` named one file and three line numbers and came back as `eslint: 0 errors, 3 warnings in 1 files`, an 86.2% reduction of a payload whose entire value was those three locations. An agent reading it has to re-run eslint to find out where, so the reduction was token-negative: it cost a round trip to save 577 bytes. Locations are grouped under their file now and capped at 20, errors ordered before warnings so the blocking problems survive the cap, and a marker names how many were dropped. The parse reuses `is_eslint_finding_line`, the same predicate that decides this is eslint output at all, so the shape that dispatches and the shape that is parsed cannot drift; it is stricter than "contains a colon" on purpose, which keeps the stanza formatter's bare `7:14` and the inline formatter's `path:7:14` in separate branches so no problem is counted twice. Confirmed still live against the release binary through the post-hook before and after, not only in a unit test: `signals/tools/eslint.toml` matches the command but strips nothing from a run that has findings, so it misses `beats_guardrail` and the Rust distiller is what actually answers. The `jsts_eslint` snapshot recorded the old summary, which is to say it recorded the defect, and is updated.
- **Two of the four passthrough gates recorded the bare command, so opposite findings became the same row (#254)**: `passthrough_events` has no reason column, so each gate encodes its reason as a `[tag]` on the end of the command string, and only the format sniff and the host output cap were doing it. After #253 the two untagged gates sat on opposite sides of the pipeline. One means a shipped signal file stripped its entire input and has no `fallback_message`, which names a filter to write a zero-state for; the other means a distiller produced something too weak to keep, which names a distiller to improve. Read as one bucket, a run of `black` rows looks like the same weak-distiller story as a run of `date` rows and the filters needing a `fallback_message` never surface. Nothing is misled by this, it degrades the table this project queries by hand to decide what to build next. `recent_passthroughs` is added for the regression test and nothing in the shipped binary reads the table yet. Writing the test turned up a separate defect, filed as #288: `format::sniff` calls any payload CSV when every line carries the same non-zero comma count, so the comma-free fixture this test needs is comma-free on purpose.
- **Six subcommands still swallowed a flag they did not know and reported success (#151)**: the critical half of that ticket was already closed, `omni stats --detial` now exits 1 and suggests `--detail`, but an audit of all fourteen real subcommands found `diff`, `engram`, `goal`, `patterns`, `update` and `version` still accepting anything and running their default path at `rc=0`. `goal` was the worst of them: its catch-all arm treats any argument as goal text, so `omni goal --nonsense` silently stored `--nonsense` as the project's north star. All six route through the existing `cli::check_flags` now, each with the flag list its own help prints, so the two cannot drift. `goal` checks only the leading token, because free text is the feature there and a goal may legitimately contain a flag. The guard lives in `tests/smoke_test.sh`, which drives the shipped binary and which `cargo test` never runs: it sweeps every subcommand and fails if any of them exits 0 on `--zzz-not-a-real-flag`.
- **`az aks command invoke` was routed by `az`, so it lost everything after the first section (#234)**: it does not produce `az` output. It runs an arbitrary shell command inside a cluster and hands back that program's stdout, so a four-section run came back as nine rows of the first section plus `[Partial signal]`, with the TLS probe that was the point of the call discarded, after a 40 second round trip against a private cluster. `kubectl exec`, `docker exec` and `ssh host '<cmd>'` are the same shape, and #112 was already `kubectl exec` output summarised as `docker logs: 9 lines, no errors detected`. Wrappers join `python`, `python3` and `ruby` on the verbatim list for the same reason they are there (#190): something that runs an arbitrary program has no grammar of its own, so the honest answer is the input. The alternative, parsing the inner command and routing on that, was measured before being rejected: 59 of 8,032 recorded commands are wrappers, 0.7%, so the parser and its failure modes would be bought for less than one call in a hundred.
- **`docker ps` deleted every running container and kept the dead ones (#233)**: the summariser prints `docker: 5 containers | 3 running, 2 exited` and then names only the exited ones, by construction. On the repo's own fixture that is 862 bytes down to 75, a reported 91.3% saving, with every running container's name, image, port mapping and uptime gone and no marker, because nothing thought anything was lost. An agent runs `docker ps` to find a running container's port far more often than to find a dead one, so the distiller kept the rarer half. Container listings are enumerations now, the same rule that covers `ls`, `find`, `ps`, `df`, `du`, `stat` and `wc`: every row is a distinct datum and there is no noise to drop. `ps` is the closest relative and a busy machine's process table dwarfs any container list. `docker build` and `docker logs` are logs with real noise and keep their summarisers. The `-a` case on a host with hundreds of dead containers stays bounded by `MAX_OUTPUT_BYTES`, which cuts with a marker that states what it removed. The `cloud_docker_ps` snapshot recorded the old summary, which is to say it recorded the defect, and is updated to the full table.
- **`execution_traces` was in no cleanup at all, so it alone reached 160.1 MB of a 187 MB database (#165)**: it stores `raw_input` and `distilled_output` verbatim, making a row two orders of magnitude heavier than anything else, and `cleanup_old` covered `sessions`, `distillations`, `file_access`, `rewind_store` and `session_events` but not it. It gets its own shorter window of seven days rather than the shared thirty, because its only reader is `get_recent_traces`, which asks for the newest N: a trace older than a week answers no question anyone is posing. Measured on the maintainer's database, 160.1 MB in total against 6.0 MB inside seven days. It does not shrink the file: nothing in the tree runs `VACUUM`, and SQLite frees deleted pages for reuse rather than returning them, so the live data falls and the database stops growing without bound while `omni.db` stays at its high-water mark until someone runs `VACUUM` by hand. Doing it automatically was rejected rather than overlooked, because `cleanup_old` runs on a background thread at hook startup and a multi-second exclusive lock on a 187 MB file taken while hooks are firing is a hang, not an optimisation. `verification_results` is dropped, having neither a writer nor a reader anywhere in the tree. The rest of the list in #165 is out of date in both directions and was re-checked against the code rather than trusted: `retrieve_events` is alive now that #272 writes it, `loop_memory` and `session_summaries` always had both ends, `unhandled_tools` is write-only but becomes useful exactly now that #172 lets other tools reach the hook, and `file_access` has two readers and no writer, which is a decision rather than a deletion.
- **`context_turns` was written on every hooked command and read by nothing (#270)**: 5,532 rows and an index, paying write latency on the hook's own budget and disk on a 187 MB database, for a `SELECT` that exists nowhere in the tree. The table and its index are dropped, and an existing database drops them on open so the space is actually reclaimed. The in-memory `SessionState::current_turn` it was built from stays, because that one is read, by `omni stats` and `omni_context_breakdown`. With the last writer gone the pre-hook no longer opens the database at all, which takes a SQLite connection off that path entirely.
- **The RewindStore archived the same output again on every run (#274)**: the key was `{ts_ns}_{hash}`, so every key was unique and the `INSERT OR IGNORE` beneath it could never fire. The key is the content address now, and a repeat refreshes the retention timestamp instead of being ignored, so content still being produced does not age out on the strength of when it was first seen. Invisible while the archive never fired at all; a live disk cost from #271 onward, when archiving started happening on every lossy call. The test that asserted two identical payloads get different keys was the defect written down as a requirement and is replaced: what it really guaranteed, that a returned key resolves to its content, is kept.
- **A TOML filter that shortened output in the pipe archived nothing and marked nothing**: `hooks::pipe` returned `rewind_hash: None` unconditionally on the TOML branch, so the guarantee #271 closed for the distiller path was still open one branch over, and every `omni exec` of a command a filter claims dropped its lines with no marker and nothing to retrieve. The archive decision is shared between both branches now, applied to the bytes about to be emitted. `hooks::post_tool` never had this gap, because its rewind block already sat downstream of the same join.
- **A test set `OMNI_PASSTHROUGH` in the real process environment, so any test distilling at the same moment silently took the passthrough branch**: cargo runs tests in parallel and `hooks::pipe::run_inner` reads that variable on entry, so the mutation reached every concurrent reader. It surfaced as `archives_the_raw_output_it_shortens` failing on CI while passing on every local run, `left: 0, right: 1`, an archive that never happened because distillation never happened. The comment above it asserted the suite was single-threaded, which is the shape of bug this project keeps finding elsewhere: an assumption stated as a fact. The value predicate is split out and tested directly, so nothing mutates process state to check a truth table.
- **A second regex was compiled per payload, in the TOML filter path (#283)**: `TomlFilter::apply` built the ANSI escape pattern inside the call, so every filter with `strip_ansi` paid a compile for every payload it touched. Same defect as the one above, one module over, found by sweeping the tree for `Regex::new` outside a static rather than by waiting for it to be reported. `pipeline::format` was already doing it correctly.
- **The hook spent 52 ms classifying an 80-line payload and 0.1 ms distilling it, because a regex was compiled once per line (#283)**: `is_context` built its path pattern with `Regex::new` on every call and `classify_block` calls it for every line, so a single 64-character line cost 660 microseconds. `AGENTS.md` budgets the whole hook at 10 ms. Hoisting it into a `LazyLock`, which `CLAUDE.md` already names as the standard and which `semantic.rs` used nowhere, takes `classify_block` from 660.6 microseconds to 800 nanoseconds and `score_segments` on 80 lines from 52.40 ms to 98.7 microseconds, measured warm on a release build. That is the scoring stage, which is most of what a hooked command pays for. A regression guard runs a thousand classifications under a 200 ms bound: two orders of magnitude above the fixed cost and three times under the broken one, so it cannot flake the way the collapse throughput gate does (#245) while still failing the moment the compile moves back inside the call.

## [0.6.9] - 2026-08-02

### Fixed
- **`npm run build` was delivered as `prettier --write: 2 reformatted, 0 unchanged` over four bare timestamps (#242)**: no prettier ran, no file was rewritten, and both facts the agent needed, whether the build passed and what it produced, were gone. `is_prettier_write_line` asked whether *any* token on a line ended in `ms`, so every build tool that prints a duration looked like prettier: `astro build` emits `16:55:35 [types] Generated 25ms` and the whole log was claimed. A fingerprint has to be something no sibling format also prints, which is the rule #105, #112 and #114 each arrived at from a different direction, and a duration is the opposite of it. The detector now matches prettier's actual line, `<path> <n>ms` with nothing after it but its own ` (unchanged)`, where the path has an extension and is not a clock or a bracketed tag. Reproduced against the release binary before and after, with `npm` shadowed on `PATH` so the command name is the real one.
- **CI took 23 minutes of wall clock for 13 minutes of work (#179)**: the macOS and Windows matrix declared `needs: ci`, so it waited for the ubuntu job before starting, although the two share nothing; the matrix runs the test suite on other platforms while ubuntu runs lint, audit and the smoke checks. It runs in parallel now, which costs more runner minutes on a red build and returns the answer in about half the time. Two smaller wastes went with it: `cargo-insta` and `cargo-audit` were compiled from source on every run instead of installed as the prebuilt binaries they publish, and the workflow asked for `stable` while `rust-toolchain.toml` pins 1.97.0, so rustup fetched a second toolchain it never used. That last one is the same class of drift that made the 0.6.2 tag produce no binaries at all.
- **Cumulative, cache-discounted savings are computed and tested, and deliberately not published yet (#173)**: `Store::token_savings_with_reuse` sums `delta x (1 + turns_after x cache_read_rate)` over every distillation that reduced something, where `turns_after` counts the later distillations in the same session. A one-call session earns no multiplier, sessions do not lend each other turns, and a full-price multiplier would be 360 where the discounted figure is 198 on the same three calls. The number is not printed, because on the existing database it reports 17.0M at insertion against 469.3M with re-use, a 27.6x multiplier that comes from #118 item 1 rather than from re-use: until #259 every distillation was filed under a wall-clock id, so one "session" spans 3,739 commands across 16 project paths and its first row is credited 374 turns. The arithmetic is right and the input is not, and publishing a bigger number that is less true is the defect this project keeps closing. It goes in once enough history exists under real host session ids. The cache-read rate it assumes is 10%, named in `pipeline::CACHE_READ_RATE` with the reason it can only ever be an assumption: a hook cannot observe whether a turn was a cache hit.
- **The five findings in #118 are closed, and the last one was closed by verification rather than by code (#118)**: the session id under #259, the duplicate rows under #261, both `doctor` readings under #260, and the marker a passthrough used to add under #262. The fifth, rows labelled `Passthrough` that recorded a large reduction, no longer reproduces: the example in the report is 34,519 bytes down to 14,105, a ratio of 0.59, which under the current thresholds is `Soft` rather than `Passthrough`, and the label moved when #110 stopped a TOML filter short-circuiting the route it set. A guard for the remaining band was written and then removed: no fixture could be built that reached it, so its test passed with and without the change, and a check that cannot fail proves nothing.
- **`PostToolUse` was registered for `Bash` only, so the `Read`, `Grep` and `WebFetch` distillers had never executed on Claude Code (#172)**: written, gated and snapshot-tested since the Rust rewrite, and unreachable by registration. The blind spot concealed itself, because `record_unhandled_tool` sits downstream of the matcher that excluded everything it was built to detect: 8,968 distillations and 0 unhandled tools recorded, with every `filter_name` in the table a shell command rather than a tool. The matcher is now `Bash|Read|Grep|WebFetch`, and an existing settings file is migrated rather than left behind, which `ensure_hook` would have done silently by returning as soon as it recognised the command. What this changes was measured before it was taken and it is not free: `src/pipeline/collapse.rs`, 878 lines, comes back as 20 of them, an import list, three signatures and a marker, because `readfile.rs`'s only floor is `MIN_DISTILL_TOKENS = 2000`. **Corrected after publication (#284), visibly rather than by quiet edit**: this entry first said that floor is one "nearly every real source file clears", which is the exact generalisation the 0.6.8 entry below had already measured and withdrawn. Over 1,770 real `Read` calls, 7.6% clear it, and `.rs` only 3.1%, 9 of 290. The number that matters for the decision is the other one in that measurement: those 7.6% of calls carry 44% of all `Read` bytes, so the floor targets the heavy reads rather than the typical one. The conclusion is unchanged, and `collapse.rs` at 878 lines carries it alone: for the calls it does touch, a source file is replaced by a signature index. That trade was the maintainer's call with those numbers in hand. It ships after #246 stopped the `readfile` path reporting a document as a clean log, and after #273 made these arms archive and mark what they cut, so nothing they drop is unrecoverable.
- **The `Read`, `Grep`, `WebFetch`, `MultiEdit` and unknown-tool arms dropped bytes with no marker and nothing to retrieve (#273)**: every one of them returns before the Bash pipeline reaches the rewind block, so the guarantee #271 closed was still open one match arm away. `MultiEdit` and the unknown-tool fallback are the plainest case, keeping 30 lines of the payload and labelling the result with the line count of the input, so `[OMNI MultiEdit: 412 lines]` sat above 30 of them. The rule now lives in one function that the Bash path and all five arms share, because a second copy of it drifts: archive the raw content when the reply lost bytes, say so either way, and decline the rewrite when the marker would cost more than the cut saved. None of these arms runs on Claude Code yet (#172), which is why this lands before that matcher is widened rather than after.
- **Memory reads were uncounted, so "nobody reads it" and "it is read constantly and never recorded" produced the same database (#272)**: `retrieve_events` had exactly one writer, the `omni_retrieve` handler, and 0 rows against 8,968 distillations. `omni_recall`, `omni_knowledge`, `omni_insight` and `omni_query` recorded nothing, and neither did the largest memory read in the product: `session_start` injects project knowledge into every continued session without anyone asking for it. Those two readings of an empty table point at opposite fixes, and no query could tell them apart. Each of the five now writes a row naming itself, so the question is answerable per tool and per `agent_id` instead of inferred. `Store::count_memory_reads` is the reader, beside the insert rather than re-derived in SQL by whoever wants the number. Every call is counted including the ones that come back empty, because the question is whether anyone asks and a query that found nothing is still someone asking; `omni_retrieve` keeps its own rule of counting only what it found, since its counter feeds the adaptive compression rate rather than this one.
- **Three elision reports close with #267, verified on the release build rather than inferred from the diff (#263, #265, #268)**: `cat`, `grep`, `rg`, `tail`, `head` and `curl` all resolve to `CollapseMode::Log`, which was one of the four modes still falling through to the skeleton. Re-run against this build, a 51-line Deployment manifest comes back byte-identical with its `securityContext`, `capabilities.drop`, `envFrom.secretRef` and `SESSION_COOKIE_SECURE` intact, 28 of 28 distinct context names are delivered, and a `grep -B2 -A1` keeps its 68-character separator. Checked above the marker floor as well, so this is not #271's small-payload passthrough hiding it: at 5,304 bytes with 80 distinct rows, everything arrives and no omission marker is emitted.
- **Skeleton grouping survived #241 in the four content modes, so nine distinct paths still came back as one marker (#267)**: #241 removed it from `CollapseMode::Generic` alone, and `Test`, `Build`, `Infra` and `Log` kept falling through to `normalize_structural`, which rewrites every digit run to `#`. `cat`, `grep`, `rg`, `tail`, `head`, `curl` and any command string containing `.log` resolve to `Log`; `docker`, `podman`, `kubectl`, `helm`, `terraform`, `aws`, `gcloud` and `az` resolve to `Infra`. Reported by @keefar against 0.6.8 and reproduced: nine Portainer stack paths differing only in an ordinal arrived as `[9 similar lines collapsed] (pattern: "/var/lib/docker/volumes/portainer_data/_data/compose/#/v#")`, forty container rows as one more, and the footer reported 93% compression over 49 rows of data with nothing left to answer with. A count that identifies nothing leaves re-running with distillation bypassed as the only recovery, which is the token-negative outcome collapse exists to avoid. The four modes now group on the whole line like `Generic`, so their named special cases still fold what they were written for (`Compiling serde v1.0.217`, `test foo::bar ... ok`, `---> 49f356fa4eb1`, `info: …`) and everything else keeps its identifiers. Identical lines are still repetition and still collapse. `normalize_structural`, its 2,048-entry LRU cache and `is_git_hash_line` had no callers left and are deleted rather than kept warm for a rule the project has now decided against twice.
- **A prose `.txt` file was delivered as `Log: 0 errors, 0 warnings`, with every line gone and nothing saying so (#246)**: `distill_log_file` counts lines containing `error`, `fatal`, `panic` or `warn` and prints the count as a finding. A notes file has none of those words, so a 19 KB document came back as 103 bytes stating that a log was clean which was never a log. That is `require_parsed`'s exact prohibition, a summary byte-identical to a real clean run emitted with nothing positively parsed, and the same shape as #190's `Build: ok` for a python script and #216's `DB: ok` for a schema dump. Three changes, each measured against the corpus in the issue: every distinct file a `Read` call actually fetched on this machine that clears the 2,000 token floor, 133 files and 2.1 MB deduplicated by content hash, of which 28 dropped their content silently. `.txt` no longer routes to a log summariser at all, because it is not a log format and sending it to one is the fingerprint problem from #105 and #112; it gets head and tail with a count, the same as `.md`. `distill_log_file` declines through `require_parsed` when it recognised neither an error nor a warning, since a file with no error lines is not a clean log, it is a file this function cannot read. And both `distill_log_file` and `distill_config_file` call `omitted_note` like their neighbours: #176 gave every language path a count of what it removed and stopped there, which is why 13 of the 13 measured `.yaml` reads came back as a key list with no way to tell a container spec had ever been below the fold. None of this has ever reached an agent, because `PostToolUse` is registered for `Bash` only (#172); it would have gone live on 22% of the calls that matcher would touch.
- **A pipeline was routed by its first stage, so `kubectl get pod -o json | jq -r '...'` was handed to the cloud distiller, which kept one of four lines (#269)**: the three it dropped were the pod phase, the node and the zone, the fields the command was run to check; the one it kept was the timestamp. Nothing chose that survivor for being signal. Filed against `jq` missing from the passthrough allowlist, and that is not what deleted the lines: probed through `post_tool::process_payload`, `jq -r '...' pod.json` on its own is declined and always was, `printf` of the same 90 bytes is declined, and only the piped form comes back as `created: 2026-08-02T03:43:16Z` plus a `[Partial signal]` marker. Most filters select rows out of a shape they leave intact, so `kubectl get pods | head -20` is still a pod table and still routes to `kubectl`; `jq` and `yq` rewrite the payload into something of their own, so the output is theirs and routes to them. The general rule was measured before being rejected: of 5,143 distinct recorded commands, 1,035 are pipelines, and routing all of them by the last stage would hand 871 to `head`, `tail` or `sed` and stop distilling them entirely, so that question is filed as #277 rather than guessed at here. `jq` and `yq` also join `passes_through_verbatim`, which is the reporter's own recommendation and the right one for the format-safe contract, so the collapse fallback cannot reach their output either. Writing the test for that half found a third defect: both hooks asked `passes_through_verbatim` about the whole command string, so `kubectl get pods -o json | jq -r '...'` read as `kubectl`, the gate said no, and collapse rewrote 60 rows a later step had to parse. It reads the resolved command now, which closes the same hole for `cd x && cat file`, where the string reads as `cd` and a file read was collapsed the way #235 describes.
- **A command chain was routed by its first executable, so everything the later commands printed was replaced by a summary of the first one (#264)**: `git status && echo '=== tree ===' && find . -type f` came back as `git: on branch main | staged:0 mod:0 untracked:0`. The 40 paths the command was run for were gone with no marker, no count and no rewind hash, and the ratio read as a 99% win on the bytes that held the answer. `git status` is the worst case only because its distiller emits a fixed one-liner whatever the input, leaving no residue to notice; the same routing claimed `git log -12 && git status --short && ls` and dropped the `ls`. Splitting stdout back onto the chain is not possible, because it is one stream with nothing marking which program wrote which line, so the rule is the honest one: `registry::sole_output_command` finds the single command that wrote this output, and when several did, the distillers, the TOML filters and the collapse fallback all stand down and the output is handed back untouched. A leading `cd`, `export` or `source` prints nothing, so `cd /project && cargo test` is still distilled; the builtin list is deliberately short, because leaving a producer out of it costs a passthrough while letting one in costs the answer. It also knows shell control flow, which CI is what caught: `i=0; while [ $i -lt 60 ]; do echo x; i=$((i+1)); done` is one program whose stdout comes from `echo`, and reading its clauses as separate producers turned every loop and every `for f in *.yaml` one-liner into a passthrough. Measured on 5,143 distinct recorded commands: 2,928 still route to a distiller, and quote-aware splitting is what keeps 205 of them, since an operator inside `awk '/^kind:/{f=1} f && /^---/{f=0}'` is not a chain. The savings given up are real and small: chain commands, approximated with a `LIKE` filter over 2,295 recorded rows, had 6.25 MB of input booked down to 4.54 MB, and 1,535 of those rows were already passthrough.
- **The RewindStore had never archived anything, so "everything cut is archived" was false for every distillation OMNI has ever run (#271)**: the gate deciding whether to keep the raw output asked the scorer for a noise ratio and required more than 40% noise across more than 20 segments. No real payload has that shape. Across 8,968 distillations on the maintainer's database, 0 carried a `rewind_hash` and `rewind_store` held 0 rows, while `README.md` sells "everything cut is archived in a local SQLite RewindStore" and `omni stats` printed an archive count read from the empty table. TTL was ruled out before the fix rather than assumed: `cleanup_old` deletes from `distillations` and `rewind_store` under one 30-day threshold, so a hash written inside the retained window would still be sitting in the column. It was also the wrong question. A re-scored noise ratio describes what the scorer thought of the input, not what the agent is about to lose, and the two disagree on every path where a TOML filter or a distiller produced the output. Both hook paths now archive on the only question that decides it: did this reply lose bytes. The bound is stated instead of implied, because archiving everything is not affordable: inputs above `MAX_REWIND_BYTES` (64 KB) are not archived and the marker says so with the size, since the same 30 days of history is 13.3 MB of raw content under that cap and 83.1 MB without one, 53 outliers up to 5 MB carrying six times the bytes of the 3,604 rows below it. The archive runs after the route is settled, and that placement is the difference between recording a loss and inventing one: an earlier draft archived first, and measured on the release binary `git log --oneline -80` archived on 15 runs out of 15 and then handed the raw output back anyway, so every one of those writes stored content the agent had never lost. A cut whose marker would cost more than the cut saved falls back to passthrough instead of charging the agent tokens for fewer facts, which is visible on small payloads: a 208 byte `git diff` that used to arrive as a 110 byte summary with 50 lines silently gone is now handed over untouched, because paying roughly 75 marker bytes to cut 98 is not worth altering the host's output. #268 asked for that floor; this does not close it, because the lines should not have been dropped in the first place. Latency is unchanged, A/B on the release binary over 15 runs of the same command: 85.5 ms average on this branch against 86.5 ms on 0.6.8.
- **`omni doctor` reported "Last distill: never" while distilling, and called a database of 8,260 distillations "24 records" (#118)**: both readings named one table and queried another. The last-distill timestamp came from `rewind_store`, which only gains a row when a distillation had content worth keeping for retrieval, 0 rows on the reporting machine, so the query returned `None` forever and the line printed `never [IDLE]` two seconds after a distilled command. That is the line someone reads while checking whether hooks fire at all, and it was answering the opposite of the truth. The "records" count printed `COUNT(*) FROM sessions`. Both now read what they describe, and the database line names both numbers instead of leaving one unlabelled.
- **15% of the distillation history was a second copy of one command, inflating every count `omni stats` publishes (#118)**: 1,231 of 8,272 rows on the reporting installation are duplicates. Dated before anything was changed: the newest one is 2026-07-17, the day 0.6.2 was cut, twelve days of rows since contain none, 1,229 of the 1,231 are `aider`, and one payload through the post-hook on this build produces exactly one row, so the write that made them is long closed and this is a history problem. A one-time migration collapses them, keyed on every column except `id` and `latency_ms`, because latency is the only column that ever varies inside a duplicate group while `session_id`, `project_path` and the byte counts never do; that is the signature of one input distilled twice rather than two commands, and grouping on latency as well would leave 947 of them in place. No `UNIQUE` index was added on purpose: the duplicate write no longer happens, and such an index would from then on reject a user who genuinely runs one command twice inside a second with identical output, trading a closed bug for a permanent silent under-count.
- **A passthrough replaced the tool's output with the same bytes plus a marker, so doing nothing cost tokens (#118)**: when a distillation saved under a tenth, the hook handed the host back the original output prefixed with `[OMNI: Passthrough …]`. Measured on the reporting database, that marker added **33,762 tokens** across all such calls, 604 of them in a single day, at a modal 10 tokens each, spent to announce that OMNI had changed nothing. The hook now emits nothing on a passthrough, so the host keeps its own bytes at zero marker cost, which is already what the format-sniff, host-output-cap and TOML zero-state gates in the same function do. The row is still recorded, at its honest 0%. Removing the marker turned five existing tests red, and every one of them had been asserting `is_some()` on a payload OMNI never reduced: the banner made the hook return something for any input over the 50-byte floor, so those assertions could not fail. They now assert either the decline or real reduction. `post_tool_hook_still_distills_text_build_log` is the clearest case, it asserted a 317-byte fixture of error blocks, which `BuildDistiller` keeps by design, "must still be distilled"; it now checks what its file is actually about, that a text build log is not claimed by the format gate.
- **Every distillation was filed under a wall-clock id instead of the host's session, so one "session" spanned 16 projects and 3,739 commands (#118)**: `SessionState::session_id` is minted from `timestamp_millis()` in `SessionState::new`, and the persisted state is global rather than per-host-session, so rows grouped by "whenever OMNI last started". Measured on the reporter's live DB: 17 OMNI sessions against 63 real ones in `~/.claude/projects`, one id covering 16 project paths, and a PostToolUse banner reporting `-132386tok session` for a project that contributed 11 rows. The host had been sending its own `session_id` on every payload the whole time, `hooks::session_start` parses the same key and spends it on one log line before discarding it, which is how this stayed invisible. `normalize` now reads the top-level `session_id` (or `sessionId`) once for every agent, treating a blank value as absent, and `post_tool` files distillations and traces under it, falling back to the local id so pipe mode and hosts that send none are still recorded. Existing rows are not rewritten; they keep the old grouping and remain wrong. This is item 1 of the five in #118, and the prerequisite for #173, a cumulative saving multiplies by turns-since-insertion within a session, which means nothing while one session spans 16 projects.
- **A TOML filter could replace non-empty tool output with an empty string when its line rules removed every row (#224)**: `TomlFilter::apply` only handled an empty filtered result when the signal declared a `fallback_message`. Without one it returned `""`, so the post-hook wrote empty `stdout` over the host's real bytes and recorded the deletion as 100% compression. The mechanism is current but the reported trigger is deliberately synthetic: 200 `black --check` file rows reproduce it, while real Black output also prints a summary that survives the filter. Batch filtering now carries an explicit passthrough outcome: the post-hook declines its rewrite so the host keeps the original result, and the pipe path emits the input byte-for-byte; a configured fallback still wins. The distinction from `apply` is intentional because stream-mode filters call it once per line, where empty means "drop this noise line" rather than "drop the command result." Hook-level review caught that merely returning the input from `apply` was not enough: it failed the TOML size guard and fell through to `BuildDistiller`, changing the 200 rows into the fabricated `Build: ok`. Regression tests cover both hook paths, the all-stripped and configured-fallback outcomes, a surviving-signal counter-case, and unchanged stream suppression.

## [0.6.8] - 2026-07-29

### Fixed
- **A compound search whose first `grep` found nothing was delivered as `grep: 20 matches in 10 files` (#247)**: `distill_grep_output` counted matches over the whole payload and printed one header at position 0, so the number described output it had never seen. Two greps in one Bash call, the first matching nothing, came back with that header standing exactly where the first grep's result belonged, above a `---sep---` and the second grep's twenty hits, which were also printed in full below it. A search that found **nothing** read as one that found twenty hits across ten files, with no marker to detect it by. The reporter acted on it: the question was whether a Cloudflare API token had ever been written to disk in a project, the fabricated count said yes in three files, and the true answer was that no such token exists anywhere in the repo. This is the class an agent cannot catch from the inside, a fabricated *non-empty* result for an empty command. Non-match lines (a shell `echo`, grep's own warnings, a blank separator) were already kept verbatim, so the section boundary was **visible in the output** and simply not consulted when the header was placed; the same invariant #227 established for `collapse`, no summary may span a surviving line that separates the rows it counts, had not been applied here. Fixed by closing the run of match lines at every non-match line and writing that run's count directly above its own rows. A single `grep` is one run, so it still gets exactly one count at the top and no existing snapshot moved. **The zero-state arm went with it**: `matches == 0` used to return the string `grep: no matches`, discarding the body and asserting an outcome for input that was never recognised. It is near-unreachable, `is_grep_output` routes here only once three lines already parse as `path:content`, but a latent false claim is not worth the two lines it saved, so it fails open and returns the input. Verified at the hook boundary on the reported shape rather than in the function: the built binary's `--post-hook` now leads with `---sep---` and puts `grep: 20 matches in 10 files` under it, beside the rows it counts. **The regression test was toothless on the first attempt and is worth recording as such**: deleting the new flush call moved the header to the *bottom*, which satisfied a "no count before the separator" assertion while still being wrong, so the test now asserts what the agent reads *first* and was confirmed red against the original implementation pasted back verbatim. Its counter-test, a single grep still counts once, at the top, stayed green in both directions. Split out of a comment left on the closed #220, where it had no queue to sit in. Measured on Homebrew rust 1.97.1 while CI pins 1.97.0 and no rustup is on PATH, so clippy here does not predict CI: `cargo fmt --check`, `cargo clippy --all-targets -D warnings` and the full suite (18 targets, all `ok`, 0 failed) are green on 1.97.1.
- **A 15-second wall-clock assertion reddened a PR that could not have caused it (#245)**: `test_streaming_distill_1k_line_output` timed `score_segments` over 1,000 lines and failed the build above 15 s. That is not a measurement of the code, it is a measurement of a shared, unpinned GitHub runner in a debug build with the rest of the matrix competing for the host. It fired on PR #241, whose diff touched `CHANGELOG.md` and `src/pipeline/collapse.rs` and nothing else; `score_segments` lives in `pipeline/scorer.rs`, which has no reference to `collapse` at all, so the failing path was provably untouched. Green on `ubuntu-latest` in the same run, green locally, green on re-run of the same commit with no code change. A red check that cannot be attributed to the diff is worse than a missing one: it trains everyone to merge past red, which is how a real failure gets waved through. The assertion is deleted rather than relaxed, because the test's actual intent is the assertion beside it, `critical_count >= 10`, "compression is massive but didn't drop errors", which is deterministic and is the thing worth guarding. Latency already has a home in `performance_benchmarks.rs`, and `AGENTS.md`'s sub-10 ms hook budget is not what a 15-second gate measures. `bench_collapse_1000_lines` had already solved the same problem the other way, relaxing its target under `#[cfg(debug_assertions)]` with a comment naming slow CI runners as the reason; this test was simply left behind. No new test: deleting an assertion needs no assertion to prove it, and the surviving one runs on every CI job. Measured on Homebrew rust 1.97.1 while CI pins 1.97.0 and no rustup is on PATH, so clippy here does not predict CI: `cargo fmt --check`, `cargo clippy --all-targets -D warnings` and `critical_scenarios` (15 passed, 0 failed) are green on 1.97.1.
- **The pre-hook rewrote the caller's whole pipeline, so `| tail` read OMNI's markers and `> log` received a truncated file (#157, #207)**: `rewrite_logic` tested the leading utility with `starts_with` and then wrapped the **entire** command string, redirects and pipes included. `bash tidy.sh 2>&1 | tail -3` ran as `omni exec bash tidy.sh 2>&1 | tail -3`, which puts distillation upstream of a stage the caller wrote deliberately: `tail` no longer saw the script's last three lines, it saw the last three lines of OMNI's summary, and the run's only verdict (`dipindah: 947 | dilewati: 0`) was among the 850 lines collapse had already folded away. The reporter's agent read the markers as the result and carried a number over from an earlier dry run to answer with. The redirect form is the same defect with a file on the end: `npm run build > build.log 2>&1` wrote **12 lines and an `[OMNI Active] ⏺ 85.7% reduction` banner** into a file the shell was told to write, dropping the 25-line `Route (app)` table that is the reason anyone keeps a build log, and `cargo tree > tree.log` appended the banner even at 0.0% reduction, giving anything that later parses the file a foreign trailing line. That makes the documented escape hatch, redirect and inspect, return *less* than the terminal did, which is the worst possible property for an escape hatch. #170 closed the cargo half at the routing layer and left this one, because the mechanism is not the distiller: it is one format string, and it fires for every entry on the allow list. The fix is to decline the rewrite when the command carries its own downstream stages. Detection is a quote-aware scan for `|`, `>`, `<`, `;` and `&` outside single quotes, double quotes and backticks, so `git commit -m "fix: a | b"` still rewrites, a blanket `contains('|')` would have given up coverage for punctuation inside a commit message. **This costs no coverage on the path the agent reads**: `PostToolUse` still distills whatever the pipeline finally produces, so the only bytes that stop being distilled are the intermediate ones the caller's own stage was written to consume, and the file on the end of a redirect. Verified at the hook boundary rather than in the function, since the claim is about what the host is told to run: the built binary's `--pre-hook` returns no rewrite for `bash tidy.sh 2>&1 | tail -3` and `npm run build > build.log 2>&1`, and still returns `omni exec …` for `git status` and for the quoted-pipe commit. Four regression tests cover the pipe, the redirect, the chain operators and the quoted-operator case; the pipe test in `hooks::pre_tool` was an existing assertion that the pipeline **was** wrapped, so it was inverted rather than added, and it was confirmed red against the restored rewrite with the fix stashed. **Two things this does not fix and they stay with their tickets**: collapse still keeps a head and no tail, so a summary line that follows repetitive rows is still droppable when the output does reach a distiller (#157's residual), and the reduction percentage still counts deleted rows as a saving (#109, #212). Measured on Homebrew rust 1.97.1 while CI pins 1.97.0 and no rustup is on PATH, so clippy here does not predict CI: `cargo fmt --check`, `cargo clippy --all-targets -D warnings` and the full suite (18 targets, all `ok`, 0 failed) are green on 1.97.1.
- **A markdown document read with `cat` was delivered as `tree: N entries`, and a `sed` range of source lost 37 of 56 lines (#236, #235)**: two arms sent arbitrary file content to summarisers that have no grammar for it. `cat`, `head`, `tail`, `sed` and `awk` were routed to `SystemOpsDistiller`, in the same `matches!` as `apt`, `tar` and `brew`, where `is_tree_output` tested `any` line for a box-drawing character. **One** such line classified the whole payload, so `cat docs/DEVELOPMENT.md`, 127 lines of prose whose only tree is a code block illustrating the repo layout, came back as `tree: 127 entries` plus one folded line, and `head -80 docs/HOW_TO_USE.md` came back as a single line. Box-drawing characters are how every project documents a directory layout; 25 markdown files in this repository contain them, `CLAUDE.md` among them. The second half of that predicate was looser still, matching any line containing both `directories` and `files`, and it fired **live during this fix** on a `grep`+`sed` of `system_ops.rs` itself: 3 KB of source came back as `tree: // Look for summary line "N directories, M files"` under a claimed 99% compression, because the comment above `distill_tree_output` names both words. The reported harm is what makes this the widest of the family: the file was a skill document being read for instructions, the dropped range held the verification commands the reader was about to skip, and prose is the worst case for a shape-based fallback because nothing in the surviving text looks incomplete. The other arm is `VcsDistiller`: `gh` and `glab` route to it on the wrapper name, and it cuts **any** output over 10 lines to the first 10 with no grammar check at all, stripping blank lines on the way. That is what actually ate #235's reproduction, the pipeline reads `gh api … | base64 -d | sed -n '285,340p'`, so the base executable is `gh`, not `sed`, and it appended `... [37 more items, use --limit to see more]`, naming a flag that neither `gh api` nor `sed` has, so the suggested recovery could not be run. The same cut took a Homebrew cask to 10 lines in #226. Fixed on both arms by the rule the codebase already had: the five readers join `passes_through_verbatim`, the single predicate that keeps a command out of the distiller *and* out of the hooks' collapse fallback (#214), so a passthrough cannot be undone one stage later; and the `gh` arm now requires a `list` subcommand, since `--limit` belongs to those and nothing else. `is_tree_output` is tightened rather than left behind the routing change, because `grep` and `rg` still reach it: real `tree` closes with `N directories, M files` on its own line, which a document quoting a tree does not print, and `tree --noreport` falls back to requiring the connectors be the shape of the output rather than a passage inside it. The now-dead `should_passthrough_config_output` special case for `cat` of a `.toml`/`.json`/`.env` is deleted rather than left as a second list, `cat` passes through unconditionally now, and two lists that must agree is how #214 recurred. Verified through the built binary with an isolated DB: `cat docs/DEVELOPMENT.md` returns 127 of 127 lines, `cat CLAUDE.md` 382 of 382, and a 56-line source range 56 of 56. Four regression tests cover the document, the source range through both the bare `sed` form and the `gh api` pipeline, and the tree fingerprint against real `tree`, `tree --noreport`, a document quoting a layout, and a source comment naming both words; each was confirmed red against the restored routing and the restored `any` fingerprint. **The counter-test is the one that matters**: `gh pr list` over 25 rows must still be summarised, and it stayed green in both directions, so the gate is not a blanket passthrough wearing a fix's name. `tests/smoke_test.sh` is 31/31 on the release binary. **What this does not cover**: the `.rb`-path collapse that produces invalid Ruby is the other half of #226 and stays with it, and a wrapper being routed by the wrapper rather than by what ran inside it is #234. Measured on Homebrew rust 1.97.1 while CI pins 1.97.0 and no rustup is on PATH, so clippy here does not predict CI: `cargo fmt --check`, `cargo clippy --all-targets -D warnings` and the full suite (18 targets, all `ok`, 0 failed) are green on 1.97.1.
- **`Passthrough` was printed over bytes that had already lost lines, and the gap marker under it carried no count (#229)**: the last stage of the post-hook compares the distilled length against the raw one, and when the saving came in under a tenth it prefixed `[OMNI: Passthrough (low compression)]`, onto `final_out`, the *distilled* string. Neither branch of that block returned `content`, so `Passthrough` named a route that was not taken. The word is what makes this a false claim rather than noise: it reads as "OMNI changed nothing", so an agent that sees it does not re-run the command. Measured on 0.6.7 through the live Claude Code post-hook on a 69-line, 2,972-byte payload: the four data rows of a markdown table were gone and the header **and separator survived**, so the table read as present and empty, while a second table of the same shape further down was kept whole. The reproduction is now a unit test, and with the fix stashed it prints the defect verbatim, `| Workload | Before | After | Savings |`, the separator row, then `... [8 lines omitted]` where eight data rows stood, under the banner. The fix is the one the report proposed: take the route the banner names. Below a tenth saved there is nothing here worth a deletion, so `final_out` becomes `content` and `Route::Passthrough` is recorded rather than inferred. The second half is the marker `src/distillers/generic.rs` emitted for a gap between selected segments, a bare `... [omitted]` with no count, one dropped line and forty read identically, so the caller cannot judge whether re-running is worth it. It now states the line count, which is what every other cut in this codebase already does (`[N similar lines collapsed]`, `... [N more rows]`, `and N more` from #188, `[OMNI: output truncated, N of M lines kept]` from #219, `[OMNI: N lines omitted by max_lines]` from #111). **Deliberately not changed**: the neighbouring `... [{N} more lines]` counts segments while calling them lines, which is a smaller inaccuracy in the same file and belongs with whichever ticket measures it rather than being folded in here on a guess. Two regression tests, the hook-level one asserting the bytes under the banner rather than the banner's spelling, checking that the label is worded correctly is exactly what let this through, and a distiller-level one on the count; both confirmed red against the restored behaviour. `tests/smoke_test.sh` is 31/31 on the release binary. Measured on Homebrew rust 1.97.1 while CI pins 1.97.0 and no rustup is on PATH, so clippy here does not predict CI: `cargo fmt --check`, `cargo clippy --all-targets -D warnings` and the full suite (18 targets, all `ok`, 0 failed) are green on 1.97.1.
- **`git show --stat` dropped every file row, because one recognised `--oneline` subject disarmed the fail-open guard (#231)**: `git show` carries no `diff --git` and no `On branch`, so `GitDistiller` fell to `distill_log`. There the `--oneline` subject matched `RE_GIT_LOG_HASH` and was kept whole, while the stat rows are neither commits nor metadata and fell past every arm to the rule that drops body lines before the next commit, correct for `git log`, where those lines really are droppable prose, and wrong for `git show`, where the same position holds the payload. The distiller cannot tell the two apart because it never sees the subcommand. Then `if result.is_empty() { input }` did not fire: `result` was non-empty because one line **had** been recognised. **This is #228 in another distiller**, a partial recognition taking a guard whose condition is "recognised nothing" and disarming it, and it is worth naming as a shape rather than as two bugs, since #228 was `passed == 0` and this is `result.is_empty()`. Measured on `9cd7a80` through `omni exec`: `git show --stat --oneline HEAD` returned the subject and `[Partial signal - omni learn recommended]`, both stat lines gone, the marker carrying no count and no rewind hash and not distinguishing "list shortened" from "everything after line 1 discarded". The original report measured the larger case: 399 B / 6 lines to 80 B, five of six lines gone, **booked as a 79.9% saving**, on a commit holding four files of which three were PNGs, the check was whether `git add -A` had swept those screenshots in, and the answer came back naming no files, so the commit read as single-file. It was not. Fixed by routing on the flag rather than on the command or the content: `--stat`, `--numstat`, `--shortstat`, `--name-only` and `--name-status` exist only to make git emit a file list, which makes the flag reliable in a way the subcommand is not, `git show` *without* one prints a diff and stays legitimately distillable. The predicate is added to `passes_through_verbatim` as well as to the routing arm, and that second half is the part that would have been missed: a passthrough returns its input, never beats `beats_guardrail`, and is read by both hooks as a distiller that punted, so the collapse fallback would have folded the file rows up instead, every row of a `--stat` shares a shape. That is exactly #214, and the doc comment on that predicate says so. Verified through the built binary rather than the unit test, since the claim is about the bytes a caller receives: `git show --stat --oneline HEAD` now returns all four lines byte-for-byte. Two tests, one over `git show --stat`, `git log --stat`, `git show --name-only` and `git diff --numstat`, confirmed red against the restored routing where it reproduced the defect verbatim, subject kept, all five rows gone, and **a counter-test that a plain `git log -2` still drops `Author:`/`Date:` and keeps every subject**, green in both directions, so this is not a blanket passthrough for `git` wearing a fix's name. **Deliberately not changed**: the `result.is_empty()` guard itself. Strengthening it into "did I recognise most of this" would put #199's deliberate body-line drop and #107's subject-keeping at risk with no failing case to check against, and the flag is what the reported defect turns on. Measured on Homebrew rust 1.97.1 while CI pins 1.97.0 and no rustup is on PATH, so clippy here does not predict CI: `cargo fmt --check`, `cargo clippy --all-targets -D warnings` and the full suite (18 targets, all `ok`, 0 failed) are green on 1.97.1.
- **Generic collapse folded rows that shared a shape but not a value, so fourteen issue numbers went behind one marker (#232), and the Ruby-cask case turned out to be a release gap (#226)**: `normalize_structural` rewrites every digit run to `#` and lowercases the rest, which is a *skeleton*, safe where something already established that the varying token is noise (a crate version behind `Compiling`, a test name in `test foo::bar ... ok`, a layer hash after `--->`), and unsafe in `CollapseMode::Generic`, which is the fallback for commands **no distiller claimed**, where nothing established anything. Fourteen rows of `Now\t#224` … `Now\t#237` normalised to one pattern and all fourteen were deleted behind `[14 similar lines collapsed] (pattern: "now\t##")`. The marker lowercases `Now` and renders each three-digit number as a single `#`, so it does not even preserve the width of what it replaced; a second call in the same session lost which three issues carried a label. The count survives and every identifier does not, which leaves re-running with distillation bypassed as the only recovery, the token-negative outcome collapse exists to avoid. `Generic` now groups on the whole trimmed line. Identical lines are repetition under any reading and still collapse; the four specific modes keep their skeleton grouping, where the domain knowledge that licences it actually exists. **The ticket asked which of two causes was live and the answer is measured, not assumed**: a probe against `main` (which carries #227) reproduces the marker exactly, so this is the collapse fallback and not the release gap. **The same probe settles #226 the other way, and against the expectation this change was written under.** Its four Ruby `end` lines all survive on `main`: the blank line between cask blocks is not collapsible, so #227's per-section grouping already puts each `end` alone in its own segment, below `MIN_GROUP_SIZE`. The reporter saw it on a released 0.6.7 that predates #227, so the observation was correct and the cause was the release gap. The `.rb`-path half of that report, `gh api …/Casks/bubo.rb | base64 -d` cut to 10 lines with the blank lines stripped, was never collapse at all: it is `VcsDistiller` claiming any `gh` output over 10 lines, fixed in the #235/#236 entry above. Both halves are locked by regression tests here rather than left to the release. **A worked example of why this is worth fixing, from writing the fix**: the probe's own output was read back through the Bash hook, and what appeared on screen was `[4 similar lines collapsed] (pattern: "end")`: OMNI collapsing the evidence into the very shape under investigation, which read as "#226 still reproduces" until the log file was opened directly. **One existing test was rewritten rather than left green.** `keeps_groups_inside_their_own_section` (#220) used rows carrying distinct timestamps, which only grouped because `Generic` normalised the digits away; under this change its `groups` are empty, `all(|g| g.count == 3)` holds vacuously and the test asserts nothing while still passing. Its rows are now literally repeated, and the per-row assertions are exact counts rather than `contains`, so the same invariant is exercised instead of quietly retired. Four tests in total: the #232 case, the cask, the counter-case that forty identical lines must still collapse, and the reworked #220. Only the #232 one goes red against the restored skeleton, the cask and the counter-case are green in both directions, which is the evidence for the release-gap finding and for this not being "stop collapsing" wearing a fix's name. `savings_assertions.rs`, `bench_replay.rs` and the `cargo test`/`cargo build` fixture collapses are unaffected, since those run in `Test` and `Build` mode. Measured on Homebrew rust 1.97.1 while CI pins 1.97.0 and no rustup is on PATH, so clippy here does not predict CI: `cargo fmt --check`, `cargo clippy --all-targets -D warnings` and the full suite (18 targets, all `ok`, 0 failed) are green on 1.97.1.
- **Savings were counted for bytes no model ever read, and the banner and the DB disagreed about the same call (#212)**: four defects in the accounting layer, which is the layer the README sells. **(1) The host's own cap.** Claude Code truncates a Bash result at 30,000 bytes before the hook sees it, and above roughly the same size it writes the **raw** output to a file, previews the **raw** first 2 KB, and discards whatever the hook returns. OMNI distilled the capped fragment anyway, measured its saving against a number the host had already chosen, and booked it: the reported row is `input_bytes = 30000` exactly, `output_bytes = 2129`, printed to the agent as `[OMNI: -6194tok this call | … | 93% compression]` for a distillation that appears nowhere in the transcript. 43 rows sit at exactly that size from 2026-07-08 onward. A payload arriving at the cap now returns before any work is done and is recorded as the passthrough it is. **The guard reads `tool_response.stdout`, not the normalised content, and that distinction was a defect caught by self-review before merge**: `normalize` folds a non-empty stderr into `content`, so a 25 KB stdout beside a 6 KB stderr would have cleared the cap on a result the host never truncated, and the guard would have declined to distil perfectly ordinary output, a loss of compression that announces itself to nobody. Its regression test asserts the uncapped-but-summing case and was confirmed red against the wrong reading. **(2) TTY bytes in a token headline.** `agent_id='terminal'` was **73.4% of every byte OMNI claimed to have saved all-time**, `omni exec` and the shell pipe, where output goes to a terminal, no context holds it and nothing is billed. Compressing it may help a human read it; counting it as *tokens saved* and folding it into one figure made the figure describe nothing. Stripping it takes the all-time headline from 66.3% to the 29.3% Claude Code actually saw, which over 19 days is roughly 807K real tokens, a real saving, and about a twentieth of what was published. The exclusion goes into `applied_only()`, the single predicate every savings sum already consults for the #158 rows, so `unverified` picks the new category up by construction (`SUM(NOT (applied))`) rather than by anyone remembering to add it, and `omni stats` names it rather than letting the count shrink unexplained. **(3) Two live estimators.** The banner ran a bytes-per-token heuristic over the byte delta while the `distillations` row ran a real tokenizer over each string; for the same call the banner said 6,194 tokens and the row said 16,983 - 1,209 = 15,774. The banner is the copy that enters the agent's context, so it now reports `raw_tokens - filtered_tokens` from the result it is describing. `detect_content_hint` was the heuristic's only remaining caller and is deleted rather than left as a second way to answer the same question. **(4) A `delivered_bytes` column**, so the headline is computed from what reached a model instead of from what a distiller returned. **Its limit is recorded rather than papered over**: no host acknowledges to a hook whether it applied the replacement, so this is what OMNI *handed over*, not an acknowledgement. On the exec and pipe paths the distinction that can be measured is `stdout().is_terminal()`, a property of the process, not a guess about the caller, which is what separates a human reading a result from a context holding one. Old rows default to `-1`, read as "recorded before this was known"; backfilling them from `output_bytes` would restate the old assumption as though it had been measured. **One existing test asserted the belief this ticket refutes** and was rewritten rather than left green: `counts_other_agents_savings_regardless_of_when_they_were_recorded` counted a `terminal` row on the grounds that exec and pipe "wrote stdout directly and were always genuine". The compression is genuine and it is still not a token saving, so the timestamp case (which is about #158's ignored hook key and has nothing to do with this) keeps `aider`, and `terminal` gets its own test naming the real reason. Five tests: the cap, the counter-case at one byte under it with the command and content held identical so size is the only variable, the banner/row agreement, and the store-level exclusion. **What is not fixed here and stays open**: #173's re-send amplification pushes the other way and the two need netting against each other before any headline is republished, and #118's duplicate rows still inflate the largest entries. No new benchmark figure is published in this release, because the netting has not been done. `tests/smoke_test.sh` is 31/31 on the release binary. Measured on Homebrew rust 1.97.1 while CI pins 1.97.0 and no rustup is on PATH, so clippy here does not predict CI: `cargo fmt --check`, `cargo clippy --all-targets -D warnings` and the full suite (18 targets, all `ok`, 0 failed) are green on 1.97.1.
- **Three reasons the `Read` distiller could never have run, and the measurement that says what enabling it would cost (#172)**: `PostToolUse` is registered with the matcher `"Bash"`, so the `Read`/`Grep`/`WebFetch` arms in `hooks::post_tool` have been written, gated and tested since the Rust rewrite and have never executed on Claude Code. The gap concealed itself: `record_unhandled_tool` is the telemetry built to surface exactly this, and it sits downstream of the registration that excludes everything it detects, so `unhandled_tools` held **0 rows beside 6,682 distillations** while every `filter_name` in the table was a shell command. Widening the matcher is one line, and this release does not take it, because driving the built binary's `--post-hook` with a real payload first turned up three defects that would each have made the flip fail *silently*, and one measurement that makes it the maintainer's call rather than an obvious win. **(1)** `normalize_claude_code` extracts content from `tool_response.content` or `tool_response.stdout`. Claude Code's `Read` result, captured from a live transcript, not assumed, carries its text at `file.content`, beside `numLines`/`startLine`/`totalLines`. Neither arm matched, so a `Read` payload normalised to `None` and the hook emitted **nothing**: widening the matcher alone would have shipped silence, no output and no error, and looked like a feature that worked. **(2)** `ClaudeToolInput` reads `command` and `path`; the `Read` tool names its argument `file_path`, so the distiller would have seen `"unknown"` and been unable to pick a language for the per-language distillation that is the whole point. **(3)** `shape_for_host` had no `Read` shape, so the reply would have been the `{status, result}` object Claude Code's schema rejected in #187, the bug that cost two releases and printed savings for output the agent never received. All three are fixed here, with the `Read` reply echoing the host's own object and correcting `numLines` to the delivered line count while leaving `totalLines` and `startLine` alone: those describe the file and the request, not the payload, and swapping the content while leaving the count is the fabricated number this project exists to stop emitting. **The measurement is why the matcher stays as it is.** Through the built binary, `src/pipeline/collapse.rs`, 878 lines, comes back as **20**: an import list, three signatures, a risk-marker heading and `... [867 of 877 lines omitted …]`. Every function body and every test goes. The marker is honest and the loss is still total for anyone about to edit the file, and `readfile.rs`'s only floor is `MIN_DISTILL_TOKENS = 2000`. **Correction, measured after this entry was written and left here rather than quietly edited away**: the floor is narrower than this entry originally claimed. Over 1,770 real `Read` calls taken from session transcripts, **7.6%** clear it, `.rs` just 3.1%, 9 of 290, so `collapse.rs` at 878 lines is an outlier, not the typical read, and the original wording ("which nearly every real source file clears") was an unsupported generalisation from one example. The floor is also well placed: those 7.6% of calls carry **44% of all `Read` bytes**. What the measurement does not change is the conclusion, for the calls it *does* touch, a source file is replaced by a signature index, and that is still a decision rather than an obvious win. That is the ticket's own decision (1), "a distilled source file the agent then edits is a correctness hazard, not a token win", and it now has a number attached instead of a worry. `Grep` is untouched for a narrower reason: no `Grep` result shape has been captured from a live host, and guessing a second schema is precisely how the first one was got wrong. Verified at the hook boundary rather than in the function, since every one of these defects is about what crosses it: the same `Read` payload that produced zero bytes before now returns the host's shape with `numLines` 12 against `totalLines` 400 and no `{status, result}`. One regression test asserts the swapped content, the corrected count, the preserved `totalLines`, and the absence of the #187 shape. `tests/smoke_test.sh` is 31/31 on the release binary. Measured on Homebrew rust 1.97.1 while CI pins 1.97.0 and no rustup is on PATH, so clippy here does not predict CI: `cargo fmt --check`, `cargo clippy --all-targets -D warnings` and the full suite (18 targets, all `ok`, 0 failed) are green on 1.97.1.
- **A `cargo test` run that exited 101 with one failing test was reported as `Tests: 1 passed, 0 failed` (#228)**: `TestDistiller`'s pass counter tested `seg.content.contains("ok")` over a whole segment. That substring sits inside `token`, `broken`, `lookup` and `cookie`, so a `wc -l` line naming a log file under `token-efficient/` counted as a passing test. The count is what made it dangerous rather than merely wrong: the #195 fail-open guard one branch down is `summary.is_none() && passed == 0`, so a single phantom pass takes output that carries **no runner tally at all** and formats it into one. Measured through the live Claude Code post-hook in this repo, whose own path contains `token`: real stdout was the two lines `rc=101` and `      24 /…/token-efficient-omni/…/x.log`; what reached the agent was `Tests: 1 passed, 0 failed`; the redirected log held `test result: FAILED. 0 passed; 1 failed`. **The control is what identifies the mechanism**: the same failing run with its log written to `/tmp/zz/x.log`, where `ok` appears nowhere in stdout, passed through verbatim. The only variable was the substring. Highest-severity class, the reader acts on it and is wrong, and the third instance of the same shape in this file after #210 (`contains("failed")` matching `0 failed`) and #195 (a success string on zero parse). Fixed by using the predicate this file already had: `is_passing_test_line` was written for #210 and reads cargo's grammar (`test <name> ... ok`, `... ignored`), so the counter now applies it per trimmed line instead of matching a substring across the segment. `contains("PASS")` and `contains('✓')` are **deliberately left alone**: neither is reproduced, and anchoring them means guessing at jest, vitest and nextest line formats with no failing case to check against, a second unverified change bought with the first. Routing was verified rather than assumed, at `distill_with_command` for both `cargo test` and `env cargo test --test probe`: both now return the input byte-for-byte. Both forms reach the Rust distiller, the first by fall-through, `signals/tools/cargo.toml` matches `^(cargo|rustc)` and wins first per #110, but its strip rules match nothing in two lines of shell output, so it fails `beats_guardrail`, and the second because `env …` does not match the filter at all, which is the form the live reproduction used. The regression test drives the hook's own order (`score_segments(raw)` → `distill`) and was confirmed red against the restored substring, reproducing `Tests: 1 passed, 0 failed` verbatim while the other seven tests in the module stayed green in both directions, including `falls_back_to_counting_when_runner_printed_no_summary`, so real `... ok` lines are still counted. **One verification could not be completed and is recorded rather than claimed**: driving the built binary's `--post-hook` with an isolated `OMNI_DB_PATH` returns empty stdout with `ERROR r2d2: database is locked` when run from inside a session whose own hooks hold the DB, so the end-to-end hook check was made at `distill_with_command` instead of at the hook boundary. Measured on Homebrew rust 1.97.1 while CI pins 1.97.0 and no rustup is on PATH, so clippy here does not predict CI: `cargo fmt --check`, `cargo clippy --all-targets -D warnings` and the full suite (18 targets, all `ok`, 0 failed) are green on 1.97.1.
- **A section's rows were counted into the previous section's marker and deleted from their own (#220)**: `collapse_inner` grouped lines by normalized pattern into a single `BTreeMap` spanning the whole output, emitted one marker at the group's first index, and marked every member collapsed. Nothing in that plan knew about the literal lines standing between the members, so a group reached across them: the marker landed in the first section carrying the *global* count, and the later sections lost their rows entirely. The count is what makes this worse than the neighbouring collapse tickets, #111 and #219 dropped rows with no count at all, where here a number is printed, looks per-section, and is wrong. Measured on the released 0.6.7 through `omni exec` with an isolated DB, on the reporter's shell repro (two sections, 3 alice + 3 bob then 1 alice + 1 bob): section 1 came back as `[4 similar lines collapsed]` twice and section 2 as `[Partial signal - omni learn recommended]`, a fourth row invented under each name in section 1, both real rows gone from section 2. The reporter hit it reviewing PR comments one command per PR, where the fabricated fourth comment reads as an unaddressed review finding; a merge decision was one step from resting on it. **Strict contiguity is the obvious fix and it is wrong**: docker build output cycles `Step … / ---> Using cache / ---> <hash>`, so no two consecutive lines share a pattern and a contiguous-run rule collapses none of it, `collapses_infra_cache_lines` went red on exactly that. The invariant that actually holds is narrower: no *surviving* line may lie between the first and last row of a group. Rows folded into some other marker are not boundaries, so interleaved patterns still collapse, while a header that will be printed does split the group. Whether a line survives depends on the grouping it is part of, so the split is iterated to a fixpoint, a line whose pattern falls below `MIN_GROUP_SIZE` inside its own segment becomes a boundary and re-splits the segments around it. It settles in two rounds on the repro and one on `cargo`/`docker` output. Counts are now correct by construction rather than by a second check, since a group cannot contain rows it did not stand over. Verified through the built binary rather than the unit test, since the claim is about the bytes a caller receives: the same repro now prints `[3 similar lines collapsed]` under each name in section 1 and both of section 2's rows verbatim. The regression test asserts per-section counts, the survival of section 2's rows, and the survival of the header; it was confirmed red against the restored global grouping, where it reproduced the count of 4. `collapses_cargo_test_500_fixture`, `collapses_cargo_build_fixture`, `collapses_infra_cache_lines` and `collapses_generic_repetition` all stay green, so repetitive output still collapses at the same rate. Measured on Homebrew rust 1.97.1 while CI pins 1.97.0, so clippy here does not predict CI: `cargo fmt`, `cargo clippy --all-targets -D warnings` and the full suite (18 targets, all `ok`, 0 failed) are green on 1.97.1.

## [0.6.7] - 2026-07-27

### Fixed
- **A TOML filter's `max_lines` cut the tail with nothing to say so (#111)**: `TomlFilter::apply` enforced the cap with a bare `lines.truncate(max)`, so the caller received a short, well-formed, *incomplete* output and had no way to tell it from a complete one. Same never-drop violation as #219, one layer down, and the last of that family: #219 closed the 50 KB safety cut, this closes the per-filter cap. Measured on the post-hook path, the one that reaches a Claude Code agent, with an isolated DB: `ruff check .` over 200 findings came back as **50 rows ending mid-list at `src/mod_49.py`**, with 150 findings gone and no marker; it now ends `[OMNI: 150 lines omitted by max_lines=50]`. **The issue's stated impact was stale and is worth correcting rather than repeating**: it named `kubectl get pods -A` as the live case on the grounds that `signals/tools/kubectl.toml` shadows the Rust distiller per #110, but #110 was closed by #124 in 0.6.6, and a 250-pod table driven through both the exec path and the post-hook now reaches `CloudDistiller` and returns `k8s: 250 pods | 250 running, 0 pending, 0 error`, never touching `max_lines`. The defect is real and reachable; `ruff` is the demonstrated path, not `kubectl`. Twenty-five shipped filters carry `max_lines`, and **none of their inline tests has an input that exceeds its own cap**, so no existing expectation changed, `omni learn --verify` reports all loaded filters passing before and after. Two regression tests cover the marker and the untouched-under-cap case; the first was confirmed red against the restored bare `truncate` while the second stayed green both ways, so the marker is a report of a real loss rather than decoration on every filtered call. The rewind hash is deliberately still absent here, as in #219, that is the larger half of this ticket's original ask and stays with it. **Filed while verifying this on the post-hook path**: an all-stripped payload is delivered to the host as an empty `stdout` rather than declined, and 15 of 45 shipped filters have no `fallback_message` behind them (#224, mechanism traced, no realistic trigger demonstrated). Measured on Homebrew rust 1.97.1 while CI pins 1.97.0, so clippy here does not predict CI: `cargo fmt`, `cargo clippy --all-targets -D warnings`, the full suite (18 targets, all `ok`, 0 failed), `omni learn --verify` and `tests/smoke_test.sh` are green on 1.97.1.
- **`ps aux` lost 416 of 556 rows to the 50 KB safety cut, and the marker never said so (#219)**: the last stage of both hook paths truncates at `MAX_OUTPUT_BYTES` and appended a bare `[OMNI: output truncated]`. It stated *that* bytes went and never how many, and it handed back no rewind hash, so a two-line cut and a four-hundred-line one were indistinguishable to anything reading the result. This is the residual recorded when #200 closed and not filed until now. Measured on a build of `main`: `ps aux` at 556 rows / 133,033 B came back as 140 lines / 50,274 B with the marker naming nothing, under a footer reporting **62.2% saved**, a figure that is 416 deleted process rows, not noise removed. `ps` is on the #200 passthrough arm, so the distiller correctly returned every row and the *next* stage deleted three quarters of them; same shape as #214, through truncation rather than collapse. Every other cut in this codebase already states a count (`… and N more` from #188, `... [N more rows]`, `[N similar lines collapsed]`, `+N more files`), which is what made this one the outlier. The two copies of the block, `hooks/pipe.rs` and `hooks/post_tool.rs`, the latter spelling the cap `50_000` inline rather than reading the constant, so raising it would have raised half the product, are now one call to `util::text::truncate_with_marker`, and `MAX_OUTPUT_BYTES` moved to `guard::limits` beside the other thresholds (CLAUDE.md SSOT). The helper trims back to the last newline before appending, so the count is exact rather than approximate and no caller is handed half a row; output with no line break at all has no lines to count and reports bytes instead of claiming `1 of 1 lines kept` for a fragment. Verified through the built binary: the same command now ends `[OMNI: output truncated, 192 of 535 lines kept]`. **Two things are deliberately not fixed here and stay with their own tickets**: the cut still offers no rewind hash, which is the larger half of #111, and the reported reduction still counts deleted rows as a saving, which is the accounting defect in #212 and #109, this change makes the loss legible, not smaller. Four regression tests cover the count, the line-boundary trim, the no-newline byte form, and the untouched case; the first three were each confirmed red against the restored bare marker, while the fourth stayed green in both directions so the guard is not simply "always append something". One of them was initially toothless, cutting at byte 55 of 9-byte rows left the fragment `r`, which a `starts_with("row ")` filter silently discarded, so the cut point moved to 58, where the fragment still reads as a row and the assertion can see it. Local clippy caught a `filter(..).next_back()` that `rfind` says better. Measured on Homebrew rust 1.97.1 while CI pins 1.97.0, so clippy here does not predict CI: `cargo fmt`, `cargo clippy --all-targets -D warnings`, the full suite (18 targets, all `ok`, 0 failed) and `tests/smoke_test.sh` are green on 1.97.1.
- **A `python3` script's 40 data rows were collapsed into one marker and booked as 95.7% saved, undoing #190 one stage later (#214)**: #190 routed `python`, `python3` and `ruby` to verbatim passthrough because a bare interpreter runs an arbitrary program whose stdout is the answer. A passthrough returns its input, so it can never beat `guard::limits::beats_guardrail`, and the hooks read that as a distiller which punted: `hooks/pipe.rs` and `hooks/post_tool.rs` then fell back to `collapse` for its line savings. `is_enumeration_command` was the guard written for exactly this trap when `ls`/`find`/`ps` hit it in #200, and the interpreters were added to the passthrough arm five issues later without it. Measured on the released 0.6.6 and on a build of `main`, identically: a script printing `row 0: value=0 …` through `row 39: value=273 …` (2,133 B, 40 distinct rows) came back as `[40 similar lines collapsed] (pattern: "row #: value=# status=active region=ap-southeast-#")` at 92 B, reported as 95.7% saved. The `#` is where every number was, and `value=` is what the script was written to produce. The narrow fix would have been a second list of interpreters beside the first, so instead the two lists became one: `is_enumeration_command` is now `passes_through_verbatim`, holding both the enumeration commands and the interpreters, and it is the single thing both the routing arm in `distill_with_command` and the collapse exemption in either hook consult. The separate interpreter arm is deleted rather than left as a duplicate `matches!`, since a passthrough arm added without the matching guard entry *is* this bug, and two lists are how it recurs. Nothing else claims `python`/`ruby` between the arm's old position and the predicate's, so the routing is unchanged; #190's own test still guards the boundary, including that the substring `test` inside a `-c` argument or a path segment (`ruby /proj/contest/x.rb`) must not reach `TestDistiller`. Verified through the built binary rather than a unit test, because the distiller was already correct and a test over it passes in both directions: the same 40-row script now returns 2,133 B at 0.0% reduction with `value=273` intact. The regression test drives `omni exec` with an isolated DB and asserts all 40 rows, the absence of any collapse marker, and the survival of the last row's distinct value; it was confirmed red against pre-fix `main`, where it reproduced the marker verbatim, while its opposite-direction sibling, 60 lines of repetitive build progress, which must still collapse, stayed green in both runs. Measured on Homebrew rust 1.97.1 while CI pins 1.97.0, so clippy here does not predict CI: `cargo fmt`, `cargo clippy --all-targets -D warnings` and the full suite (18 targets, all `ok`, 0 failed) are green on 1.97.1.
- **`sqlite3 db ".schema"` came back as `DB: ok (3 lines output)`, with the schema deleted (#216)**: `distill_db_generic` collected only Critical-tier segments and, finding none, returned `format!("DB: ok ({} lines output)", segments.len())` while its `_input` parameter went unread. The underscore is the whole defect: the payload was never looked at, so every successful non-tabular result was answered with a health verdict for a question nobody asked. `ok` appears nowhere in the input, it is invented in the success direction, and 100% of the bytes are gone. Same class as #190 (`Build: ok` for `python3 -c`) and #195 (`Tests: 0 passed` for `go test` over packages with no test files), one distiller over: `DatabaseDistiller` routes to `distill_db_error` on an error keyword and `distill_query_result` on `rows)`/`row)`/a table fingerprint, so the generic arm is where `.schema`, `.tables`, `.indexes`, `PRAGMA` output and every other non-tabular result land. Fixed the way #195 was, by failing open: the arm returns its input verbatim when it parsed nothing, which leaves `DB: ok` alive only where it would be true, on a command that genuinely printed nothing. Verified through the built binary rather than the unit test, since the claim is about the bytes a caller receives: a three-statement schema dump (136 B) now round-trips byte for byte at a reported 0.0% reduction where before it came back as 23 B of verdict, and a 40-table dump (3,351 B) round-trips too, so the passthrough is not re-cut on this path by the hooks' collapse fallback. The regression test is a snapshot over `tests/fixtures/sql_create.txt`, a fixture that already existed with nothing wired to it, routed as `sqlite3 app.db .schema`; it was confirmed red against the restored success string, where it produced `DB: ok (15 lines output)`. Two neighbouring defects were measured and left in the tracker rather than folded in: the tabular arm keeps a header, three sample rows and an honest `... [N more rows]` marker, which is lost signal with a truthful count and a lesser problem, and a passthrough that fails `beats_guardrail` can still fall back to collapse in `hooks/pipe.rs` for any command outside `is_enumeration_command`, which is #214. Measured on Homebrew rust 1.97.1 while CI pins 1.97.0, so clippy here does not predict CI: `cargo fmt`, `cargo clippy --all-targets -D warnings` and the full suite (17 targets, all `ok`, 0 failed) are green on 1.97.1.
- **A fully green `cargo test` run was reported as having failures (#210)**: `semantic::is_critical` classified a line as Critical on `lower_text.contains("failed")`, and `TestDistiller`'s detail loop collected failure lines on the same substring. cargo's *passing* tally reads `test result: ok. 479 passed; 0 failed; …`, so on a green run every tally matched both. `runner_summary` skipped only the one line *equal to* the chosen headline, which meant that in a workspace, where cargo prints one tally per target, the other 16 were filed as failure details, ten were printed verbatim and the rest were labelled `... 6 more failures`. Measured in this repo on a run that was `479 passed; 0 failed` × 17, exit 0: the output ended `... 7 more failures`. Highest-severity class, since the reader acts on it and is wrong; it is **not** #195 (closed), which fires when nothing parses, where this fires when everything does. The same substring also matched a test *named* after failure, `test guard::preserves_failed_lines ... ok` was Critical and filed as a failure. Fixed at both layers, since the distiller alone would have left every other distiller reading green output as Critical: `mentions_failure` now requires the word to be a verdict rather than incidental, rejecting it when preceded by an identifier character (it names something) or by a count of exactly zero (the runner is reporting none); `is_failure_detail` excludes tally lines and `... ok`/`... ignored` per-test lines by the runner's own grammar; and the segment loop requires some line in the segment to read as a failure, because `is_critical` works on a whole block and a chunk holding one real failure beside nine green tallies is Critical as a unit. **The fix's own failure mode was closed in the same change**: excluding tallies from the details empties them, so a green run fell to the `failed == 0` branch, which returned only the first tally, reporting a 17-target run as though it were the first target's numbers, a second false claim traded for the first. That branch now quotes every tally when there is more than one. Verified end to end through the built binary rather than the unit test, since the defect was only visible in composition: `omni exec cargo test` on a green workspace returns all 17 tallies with the word "failure" absent, where before it returned ten of them under a `... 6 more failures` label. The three regression tests were each confirmed red against the restored substring match, and the existing failing-run test (`reports_runner_totals_not_a_recount_of_collapsed_lines`, the #116 regression over `cargo_test_500.txt`) still passes, so real failures are untouched. Two smaller defects in the same block are recorded rather than folded in: the `!failure_details.contains(…)` dedup drops duplicate lines with no marker, and the `... N more failures` cut hands back no rewind hash. Measured on Homebrew rust 1.97.1 (CI pins 1.97.0), so clippy here does not predict CI.
- **`omni stats` columns drifted out of alignment as the counters grew (#209)**: the overview padded its numeric columns to widths written into the format string, and the real values had already outgrown them. `{:>3}` held the command count while `This Week` printed `1,226`; `{:>4}` held the input tokens while `All Time` printed `21.2M`; `({:>2}x)` held the per-command call count against `143x` and the per-agent count against `3296x`. Once a value overflows its field, `println!` prints it in full and every column to its right shifts by the overflow, so `commands`, `tokens`, `saved` and `(-N tokens)` each started at a different position on every row. Widening the constants only moves the break, because `format_number` gains a separator every three digits and `format_exact_tokens` widens again at each of `999` → `1000K` → `100.2M`: any constant is wrong for some future row, and a stats table that degrades as the numbers grow is the one table guaranteed to degrade. The three blocks now measure the widest formatted value they are about to print and pad to that, via a shared `max_width`. Verified under a pty rather than a redirect, since `colored` changes behaviour when stdout is not a terminal: before, `Today`'s `commands` sat two columns left of `This Week`'s; after, all three rows and all eight `Top Commands` rows and all three `Agent Distribution` rows line up. The regression test asserts the byte offset of `commands`, `tokens` and `saved` is identical across rows built from a 3-digit count beside a 5-digit one and a K-scale total beside an M-scale one, and it was confirmed to go red against the restored hardcoded widths. The `--detail` view was already correct and is untouched: it prints a header row and sized its columns to match. Measured on Homebrew rust 1.97.1 (CI pins 1.97.0), so clippy here does not predict CI: `cargo fmt`, `cargo clippy --all-targets -D warnings`, the full suite (17 targets, all `ok`, 0 failed) and `tests/smoke_test.sh` are green.

## [0.6.6] - 2026-07-26

### Fixed
- **Verbose `git log` dropped most of the commits it was given (#199)**: `git log -12` came back with **2 of 12 commits**, the output cut mid-line, with no marker. The cause was the #110 shadow trap: `signals/tools/git_log.toml` matched `^git log` and won ahead of the Rust distiller, and its line-strip rules cannot tell a commit subject from a body line, both are 4-space indented, so on a verbose multi-commit log it stripped only `Author:`/`Date:`, kept every body, blew past its `max_lines = 20`, and truncated the older commits away. The filter is removed; verbose `git log` now falls through to the Rust `distill_log`, which keeps **one compact `hash subject` line per commit**, the `--oneline` view, so every commit and its subject survive while the body and the `Author`/`Date`/`Merge` metadata go. Measured through the built binary: `git log -12` now returns 12 of 12 commits (345 lines → 11), and `git log --oneline -8` still returns all 8. `--oneline` was already handled by the Rust distiller (the filter's strip rules matched nothing there, per #107), so removing the filter changes nothing for it. `distill_log` also now **fails open** when it recognises no commit at all, the fixture harness feeds arbitrary text through it as `git status`, and the old last-resort "first 5 lines" was replaced with returning the input verbatim (#143), so non-log input is never reduced to an empty or lossy guess. Regression test asserts a three-commit log with bodies keeps all three subjects and drops the bodies; the removed filter's integration tests were deleted with it. Verified on Homebrew rust 1.94.0 (CI pins 1.97.0): fmt, clippy `--all-targets -D warnings`, full suite (1,150 passed, 0 failed).
- **Enumeration commands lost the items that were the answer; now they pass through verbatim (#198, #200)**: a command audit (`docs/COMMAND_AUDIT.md`) measured OMNI on 35 agent-common commands and found a clean split, it wins on `verdict + noise` output (`cargo test` 93% keeping every failure, `terraform plan` 86% keeping every resource) and *destroys* output that is `a list of data`, where every line is a distinct datum and there is no noise to drop. `find` returned 30 of 98 paths under a `[Partial signal]` marker with no count (#198), `ls -la` replaced every filename with `ls: 16 items | 13 files`, `wc` dropped per-file counts, and `env`/`df` came out *larger* than they went in. `ls`, `tree`, `find`, `ps`, `df`, `du`, `stat`, `wc` and bare `env` now route to verbatim passthrough in `distill_with_command`, and, because a passthrough returns the input and so never beats the size guardrail, the hooks' collapse fallback (`hooks/pipe.rs`, `hooks/post_tool.rs`) is now skipped for them via a shared `is_enumeration_command`, so they are not re-compressed into `[N similar lines collapsed]` after the distiller hands them back. `grep`/`rg` are deliberately *not* included: their distiller hoists the repeated path losslessly and keeps every match. Verified through the built binary: `find src -type f` (98 paths), `ls -la`, `wc`, `env` and `df -h` now round-trip byte-for-byte where before they dropped or grew. Two residuals are recorded rather than hidden: a very large enumeration (`ps aux`, ~148 KB) is still cut by the separate `MAX_OUTPUT_BYTES` output cap with `[OMNI: output truncated]` and no count, that cap is a #111-class defect of its own, not specific to these commands, and verbose multi-commit `git log` is truncated in the pipe path (#199), a distinct git-distiller issue. The three snapshot tests that asserted the old lossy `ls`/`find`/`env` distillation were replaced with explicit passthrough assertions. Measured on Homebrew rust 1.94.0 (CI pins 1.97.0): fmt, clippy `--all-targets -D warnings`, full suite (1,150 passed, 0 failed) green.
- **TOML filter `priority` was parsed and never read, so which filter won was accidental (#119, contributed by @adity982 in #134)**: `FilterConfig` deserialized a `priority` field that nothing ever ordered by, and filters were collected into a `HashMap` whose iteration order is randomized per run, so when two filters matched the same command, the one that "won" was not the higher-priority one, it was whichever the hash happened to yield first, and it could differ between runs. `load_all_filters` now sorts by descending `priority` (`Reverse`) with a stable sort, and the document's filter map is a `BTreeMap` so the pre-sort load order is deterministic (alphabetical) rather than hash-random; equal priorities therefore break ties consistently instead of arbitrarily, making the documented knob actually decide selection. Review of the PR caught that the ordering test had no teeth, its filter names sorted lexicographically into the same order `BTreeMap` already yields, so the sort was a no-op on that input and a broken sort would have passed, so it was renamed to force a load order that is *not* already priority-sorted, and a neutered sort was confirmed to fail it (the parse-only second assertion, which tested `load_from_file` rather than the sort, was dropped). Verified on Homebrew rust 1.94.0 (CI pins 1.97.0).
- **`TestDistiller` fabricated `Tests: 0 passed, 0 failed` for output that was never a test run (#195)**: the direct sibling of #190, one distiller over. When the runner printed no tally (`runner_summary` → `None`) and nothing was counted (`passed == 0 && failed == 0 && failure_details.is_empty()`), the headline fell back to `format!("Tests: {} passed, {} failed", 0, 0)` and was returned, an invented, completed, empty test run for output that carried no test signal, in a distiller whose whole zero-state was supposed to be guarded by the #143 `require_parsed` invariant. Reproduced deterministically at the hook's own order (`score_segments(raw)` → `distill_with_command(raw)`), read back through the Read tool so the measurement was not itself distilled by OMNI's own Bash post-hook: **`go test ./...` over five packages that each print `[no test files]` (236 B) → `Tests: 0 passed, 0 failed` (25 B)**, the package list destroyed, and the summary reading as "tests ran, 0/0" when the truth is "no test files exist anywhere." Surfaced by the #159 comment's "case 3"; the `python3`/`ruby` cases in that same comment were already #190 and the `git --oneline` case #192, so this was the remaining live fabrication. **The `{ head …; git diff } ; cat out` shape the reporter named does not itself reproduce on current `main`**, that block resolves to `Line` segmentation and passes through, so the fix targets the mechanism (zero-parse in `TestDistiller`), reproduced through a command that genuinely routes to `TestGroup`, rather than the reporter's incidental wrapper. Fixed by failing open: when `summary.is_none() && passed == 0` in that branch, return the input verbatim instead of the canned tally. **A genuine zero-test run is untouched** and is covered by its own regression test, `cargo test` in a crate with no tests prints `test result: ok. 0 passed; 0 failed`, so `summary` is `Some` and the real line is quoted; only output with no tally *and* nothing counted fails open. The fabrication was confirmed present before the change (a probe returned `Tests: 0 passed, 0 failed` for the `go test` input) and the regression test asserts both directions, the no-signal input returns verbatim, and the real 0-test summary survives. Measured on Homebrew rust 1.94.0 (CI pins 1.97.0), so clippy here does not predict CI: `cargo fmt`, `cargo clippy --all-targets -D warnings` and the full suite (1,150 passed, 0 failed) are green on 1.94.0.
- **`python3 -c "..."` and `ruby -c ...` had their real output replaced with a fabricated `Build: ok` (#190)**: `distill_with_command` listed `python`, `python3` and `ruby` among the build tools, and any invocation of them that was not a test fell through to `BuildDistiller`. That distiller counts error/warning segments and, finding none, returns the literal `Build: ok`, 9 bytes, so a script that printed no error line had its entire output substituted with an invented success verdict. This is the never-fabricate moat breaking (#147): `Build: ok` is not a compression of the input, it appears nowhere in it, and it is invented **in the success direction**, a GitOps verification whose real answer was `dangling: NONE` would have reported a passing check for a broken manifest. Same class as #170 (`cargo tree` → `Build: ok`), one layer up: a bare interpreter runs an arbitrary program whose stdout **is** the answer, not a build log. Reproduced deterministically at the hook's own stage order (`score_segments(raw)` → `distill_with_command(raw)`) with an isolated DB: **before, `python3 -c "…"` printing three summary lines → `Build: ok`; after, → the three lines verbatim**, and the same for `ruby check.rb` and the exact `ruby -c …/bubo.rb` command chain from the issue comment (a `ruby`-led block that came back as `Build: ok` on a real Homebrew commit). **The reporter's stated trigger was wrong, and it is worth recording why**: they theorised the distiller keys on a `.yaml`/`.rb` **file extension** in the command string. It does not, routing is by the first executable (`extract_base_executable`), and both repros simply happened to start with `python3`/`ruby`. The `.yaml`-vs-`.txt` difference the issue body reported was not the extension but warm-`~/.omni` nondeterminism (session history feeds the scorer), which the report did not isolate; with `OMNI_DB_PATH` isolated, both extensions fabricate identically. Fixing on the extension would have missed `python3 -c` with no path at all, which also fabricates. `python`/`python3`/`ruby` now always pass through verbatim; `pip`, `rake` and the genuine compilers stay on the build path, their output being build/task oriented. An earlier revision routed to `TestDistiller` on any command containing "test", but review found that matches inside a `-c` code argument or a path segment (`ruby /projects/contest/x.rb`) and `TestDistiller` fabricates too, `Tests: 1 passed` for a script that ran no tests, so it merely moved #190 to another distiller; the boundary is now locked by tests. Real test runners are handled upstream regardless: `signals/tools/pytest.toml` and `mypy.toml` are TOML-first and shadow this arm. **The requested token-overlap guard was deliberately not shipped**: "if the distiller's output shares no token with its input, treat it as a misroute and pass through" also fires on a *legitimate* clean build, where `Build: ok` correctly shares no token with `Compiling …` / `Finished …`, so it would silence BuildDistiller's one real win to fix a routing bug, the honest fix is to route on the invoked binary, not to blunt the distiller. The regression test drives the hook's scorer→distiller order for `python3`/`python`/`ruby` and asserts both that the script output survives byte-for-byte and that it is never `Build: ok`; it was confirmed red against the pre-fix routing (the throwaway repro returned `"Build: ok"` and panicked) and green after, while `still_routes_real_cargo_builds_and_tests_to_their_distillers` guards the other direction so the fix does not turn every interpreter into passthrough. Local gates ran on Homebrew rust **1.94.0**, not CI's pinned **1.97.0**, so clippy here does not predict CI: `cargo fmt`, `cargo clippy --all-targets -D warnings` and the full suite (1,146 passed, 0 failed) are green on 1.94.0.
- **`git log --oneline` came back as a wall of hashes with every subject deleted, reported as an ~89% saving (#107)**: `distill_log`'s second branch matched `^[a-f0-9]{7,40} `, which is *precisely* a `--oneline` entry, hash and subject sharing one line, then kept `chars().take(7)` and discarded the rest, joining every commit with a space. `git log --oneline -5` distilled to `a370713 93db32e 1017f0e 6b2bbfd 5af5a3d` under `89.4% reduction (369 B → 39 B)`: the hash is the cheap part and the subject is the entire signal, so this was 89% fewer bytes and 100% less meaning, an abbreviated hash with no subject can only be handed back to git, not reasoned about. The branch was written for **verbose** logs, where the hash sits alone on a `commit <sha>` line and the subject arrives later indented; that case still has its own branch above and is untouched, so the oneline branch now keeps the whole line. Measured through the pipeline in the hook's own order (`score_segments(raw)` → `distill_with_command(raw)`): **before, 5 commits → one line `a370713 93db32e …`; after, 5 commits → 5 lines, every subject intact**, and a stashed baseline confirmed the before behaviour byte-for-byte. `git log --oneline` is already compact, so the honest outcome is near-passthrough, the 89% "saving" was always the meaning leaving, not noise. **Routing was verified, not assumed** (the #188 lesson): `signals/tools/git_log.toml` matches `^git log` too, but on `--oneline` its Author/Date/Merge strip patterns match nothing, so for a normal history it reduces nothing, fails `beats_guardrail`, and falls through to this distiller, the path the fix is on. For a history longer than the filter's `max_lines = 20`, the filter truncates to 20 lines and wins first, keeping subjects but dropping the older commits with **no marker**; that is the #111 silent-`max_lines` class on a concrete path and is reported there rather than folded in here. A `git_log.txt` oneline fixture already existed with **no snapshot wired to it**, exactly the gap #107 predicted was why this shipped unseen, so it now has one, alongside behavioural tests asserting subjects survive on both oneline and verbose input; both the new snapshot and the oneline test were confirmed to go red against the restored `take(7)` while the verbose test stayed green, since the bug only ever touched oneline.
- **The JS/TS fallback deleted 270 of 300 lines without saying so, and the result was published as a 90% saving (#188)**: `distill_fallback` stopped at 30 lines with a bare `break` and returned `out.trim()`, no marker on any path, no count, no rewind hash. A 300-warning `npm install` and a 30-warning one produced output that looked identical, ending mid-sequence at `npm WARN deprecated fake-package@1.0.29`, and nothing downstream could tell which it was reading. That is the never-drop violation #111 and #176 were filed for, in a distiller neither of them touched, on the path `npm`/`npx`/`pnpm`/`yarn`/`bun` all route through. Measured in the hook's own stage order, `score_segments(raw)` then `distill_with_command(raw)`, which is what `post_tool` has done since #116, **before: 300 lines / 18,490 B → 30 lines / 1,819 B with markers found `[]`; after: → 31 lines / 1,836 B ending `… and 270 more`**. The marker list searched was `omitted`, `collapsed`, `truncated`, `more`, `not shown`, `[OMNI`; none were present. **Two facts were confirmed before any edit, because the routing here misleads**: `npm install` really does reach the Rust distiller, `sys_npm` would claim it, but `sys_build_domain` wins the alphabetical `find()` race, strips nothing, fails `beats_guardrail`, and falls through (the #110 shape already described at `post_tool.rs:283`), and `distill_fallback` really is the arm that runs, since no `is_*_output` detector matches a wall of `npm WARN` and `30` appears nowhere else in the file. An earlier draft of the issue also claimed `generic.rs` shared the defect; **that was wrong and was retracted on the issue**, `GenericDistiller` emits `... [omitted]`, `[N noise lines omitted]` and `... [N more lines]`, all with counts, and is untouched here. The tail reuses `… and N more`, which this file already emitted from `capped_lines`; both now share `more_tail`, so **no fourth phrasing was added** to a repo that already has three (`readfile::omitted_note`, `generic.rs`, `collapse`), and because `capped_lines`' rendered string is unchanged, no snapshot moved. The two nested loops collapsed into one iterator chain so the cap is applied once rather than re-tested per line, the pnpm/yarn filter that was written out twice became a predicate, and the zero-state sample path (nothing scored Critical or Important) was dropping lines just as silently, it now counts against every line there was, not the ten it sampled. Also deleted: an `if filtered_input.len() < input.len()` in `JsTsDistiller::distill` whose two arms called `distill_fallback(segments, session)` with identical arguments, so the condition decided nothing. **Every other cap in the file was audited and deliberately left alone**, each already carrying its count: vitest and playwright print the true failure total in the header before listing 5 and 3 names, tsc adds `+N more files`, and eslint labels its `take(3)` as "top rules" beneath a header holding the real totals. All four regression tests were proven to fail in **both** directions, neutering `more_tail` to return `None` reddens the three that assert the marker's presence and count, and forcing it to always emit reddens exactly the one that asserts silence when nothing was dropped, so none of them is decorative. This was only reachable by a user as of 0.6.5: before #187 the host rejected OMNI's payload and restored the original bytes, so no agent had ever seen the truncated output.

## [0.6.5] - 2026-07-24

### Fixed
- **The distilled output still never reached the agent on Claude Code, #158 fixed the key and left the value shape wrong (#187)**: `hookSpecificOutput.updatedToolOutput` is not a free-form object. Claude Code runs `tool.outputSchema.safeParse(value)` **keyed on the tool that ran**, so there is one shape per host tool, and OMNI was sending the MCP `{status, result}` to all of them. Bash's schema wants `{stdout: string, stderr: string, interrupted: boolean, …}`, so every payload was rejected, `Oe = re.data` restored the original bytes, and the UI rendered `PostToolUse:Bash hook warning`, while the sibling `additionalContext` went through and printed a saving for a distillation that had just been discarded. **This is #158's symptom, not a new one**: that issue was closed on a fix that corrected the key, and the observable behaviour, savings reported, nothing applied, never actually changed on this path. It survived two releases because the only thing that changed was the field name in an error nobody was reading. Measured on Claude Code 2.1.218 from the session transcript rather than from OMNI's own numbers (`grep -c hook_error_during_execution ~/.claude/projects/<proj>/<session>.jsonl`): **27 of 34 Bash calls in a single session were rejected**, each with `stdout: expected string, received undefined` / `stderr: …` / `interrupted: expected boolean, received undefined`, and one of them printed `-3247tok this call | 67% compression` for 300 `npm WARN` lines the agent received **in full**. The 7 quiet calls are the ones where OMNI correctly emits nothing at all, non-zero exit (#120) and the format-safe gate. The reply is now **the response object that arrived, with `stdout` swapped and every other key preserved**: `interrupted`, `isImage`, `backgroundTaskId`, `persistedOutputPath` and `timedOutAfterMs` are schema members, and rebuilding a minimal object would have failed validation exactly as the old shape did. The shape is read off the incoming payload rather than asserted from a table of per-tool schemas, so it stays correct for tools this code has never seen, "reply in the shape you were spoken to in" needs no table to maintain. `stderr` is **blanked rather than echoed**, because `normalize` folds a non-empty stderr into the text that gets distilled and returning the original as well would show it to the agent twice; blanked rather than dropped, since the schema requires the member to be a string. Carrying the original object that far also required `NormalizedInput` to stop discarding it: `content` is a flattened, lossy view, stdout and stderr concatenated, every other key gone, and `interrupted`/`isImage` cannot be reconstructed from it. The **labeled-passthrough branch (F-07) is fixed by the same change**; it emitted the same rejected shape, so that label had never once rendered for a Claude Code user either. Payloads arriving without a host response object (Codex, Pi, generic MCP) **keep the MCP shape deliberately**, those hosts' contracts were not investigated, and guessing at a second one is how the first was got wrong. Verified end to end through the built binary rather than the struct: the same 300-line payload now returns `{stdout, stderr, interrupted, isImage, backgroundTaskId}` with no `status`/`result`, 18,490 B → 1,819 B, and the reported 90% matches a measured **90.2%**. A stashed baseline confirms the distilled *text* is byte-identical before and after, this change moves the envelope only. The test that let both halves through asserted `"status":"success"`, **the same field name it had just serialised**, which is why a wrong key and then a wrong shape both passed it; the replacement asserts against the host's schema and specifically that `status`/`result` are **absent**, and was confirmed to fail on five tests with the old shape restored. One consequence is filed rather than hidden: making the payload apply also delivers the unmarked 30-line truncation in `distillers/jsts.rs` to agents for the first time (#188).

## [0.6.4] - 2026-07-23

### Added
- **`omni doctor` now says when the binary carries changes no release contains (#137)**: `doctor` already checked GitHub for a newer release, and that check is structurally unable to catch the case #127 filed, six correctness fixes merged and unshipped, where the newest release **is** the running version, so `doctor` printed `omni v0.6.2 [LATEST]` and nothing else. Reproduced on this tree before writing the fix: 19 commits and 15 changelog entries unreleased, and `doctor` still said `omni v0.6.3 [LATEST]`, true as an answer to "is there a newer release", useless as an answer to "am I running the current fixes". The two questions now have two answers. `build.rs` counts the entries under `## [Unreleased]` in the tree the binary is compiled from and passes the number in as `OMNI_UNRELEASED_ENTRIES`, so a properly cut release reports **0** and stays silent, while a build from a tree with pending work prints `[15 UNRELEASED] changes built into this binary are in no release, cut a tag`. The count is a compile-time fact about the source, not a network call, so it works offline and cannot disagree with the binary it describes. The parser lives in `src/util/changelog.rs` and is `include!`d by `build.rs` rather than copied into it, so the seven tests cover the code the build script actually runs; `option_env!` with a `unwrap_or(0)` fallback means a missing or malformed value reports nothing rather than raising a false alarm. Proving the tests could fail turned up dead code in the first draft: a `break` on reaching the next `## [` heading could be deleted without reddening anything, because the following assignment already ends the section, it is gone, and the reason is recorded next to the loop.

- **`omni stats --rerun` measures whether a distillation cost the agent a second run (#109)**: reduction % counts bytes removed, so a distiller that emitted `""` for every input would score 100%, and nothing in the pipeline penalised that. It is why #106 (19.5 KB → 14 B, 99.9%), #107 (89.4%) and #108 (86.2%) were each scored as wins while costing more tokens than they saved: the agent re-ran the command to get back what was deleted. The new mode measures the thing reduction cannot see, a repeat of the same command, in the same session, within 300 s of reading its distilled output. **No new instrumentation was needed**: `session_id`, `ts`, `command` and `route` are already on every `distillations` row, so step 1 of #109 ("record the normalised command") was already done, and the metric answers over 28 days of history already collected rather than starting a clock. `route` also supplies a **control arm for free**, `Passthrough` rows are commands whose raw output the agent read, so the two arms differ only in whether OMNI changed what was read. Measured on the maintainer's store (6,682 distillations, 19 sessions): `npm` 48.4% distilled vs 8.0% raw (**+40.4pp**, the #106/#129 shape), `sed` +15.1pp, `grep` +9.8pp, `git` −0.4pp. Two guards keep the metric from becoming the defect it hunts. **First, the arms are not always the same population**: distillation only fires on large output, and `kubectl` averages 244,606 B distilled against 115 B raw, a 2,127× skew that compares `get -A` dumps against one-line config reads. Its **+48.6pp is input size, not lost signal**, so any filter past `RERUN_SIZE_SKEW_LIMIT` (3×) prints `n/a` and is listed as a caveat rather than a finding; `grep` (3,196 B vs 3,240 B) and `npm` (11,419 B vs 14,065 B) are matched and survive. **Second, pre-#158 Claude Code rows are excluded**: on that path nothing was applied, so their `Keep` rows are controls wearing a treatment label. That is not cosmetic, `grep` reads **+9.8pp with them excluded and +0.0pp with them included**, so the dilution alone can hide a real finding, and the cutoff is the commit timestamp of the #158 fix rather than a guess. A filter needs 8 rows on *each* arm before its delta is reported at all; below that the number is noise, and publishing it would be the confident-but-unsupported figure this metric exists to catch. Steps 3 and 4 of #109, net scoring and an adaptive fallback driven by the rate, are **deliberately not shipped**: feeding a guard from a metric with known confounds is how a measurement bug becomes a distillation bug, so the metric is watched for a release first. All three guards were confirmed to fail the right test when neutered: disabling the skew check reddens `flags_arms_of_wildly_different_size_as_confounded`, neutering the pre-#158 predicate (keeping the parameter count, so the failure is logic and not a SQL error) reddens exactly `excludes_claude_code_rows_recorded_before_the_post_hook_fix`, and widening the window reddens `ignores_a_repeat_that_falls_outside_the_window`.
- **`omni stats` gained short scope flags and an hour window (#154)**: `-d` (today), `-w` (last 7 days), `-m` (last 30 days) and the new `--hour` / `-H` (last 60 minutes). There was no scope shorter than a day, so "what did the last hour cost me?", the question you ask *during* a session, had no answer. `-H` rather than `-h` is deliberate: `-h` is help across every OMNI subcommand, and re-pointing it at a time scope would have broken that reflex in one command while leaving it intact in the others. Resolving the four windows also collapsed two divergent copies of the scope logic into one function: `run_detail` and `run_project_stats` each had their own, `--month` appeared in neither, and it was honoured only by being the fall-through default in one of them and silently ignored in the other.

### Changed
- **`omni --help` is one grouped list written around what a user wants, not one flat list of nouns (#166, #152)**: there were two help texts, a hand-written one for `omni help` and bare `omni`, and clap's for `omni --help`, and they had already drifted. Six commands, `exec` among them, appeared only in clap's; `exec` is the harness every issue in this tracker asks reporters to run, and it was invisible in the help a user gets by typing `omni`. Both routes now render from a single `COMMANDS` table, and the two outputs are byte-identical (`diff <(omni --help) <(omni help)`). The list is grouped by intent, **SET UP**, **SEE WHAT IT SAVED**, **TUNE IT**, **MEMORY**, instead of alphabetically, and each line states the payoff rather than restating the name: `learn` went from *"Auto-generate filters from history"* to *"Build filters from the noise in your own history"*, `diff` from *"Compare last original input vs distilled"* to *"The last command's output, before vs after"*. This is the other half of #164: a usage audit found ten subcommands nobody had ever run, and the likeliest reason is that nothing in the help said what they were for or whether they were for you. A `lists_every_subcommand` test walks clap's own subcommand list and fails if a command is missing from `COMMANDS` (invisible in help) or listed without being a real subcommand, confirmed to fail when `patterns` was removed from the table. The footer now points at `omni <command> --help`, which became worth pointing at in #151.

### Removed
- **Three CLI subcommands nobody has ever invoked (#164)**: an audit of real usage, the whole shell history on the maintainer's machine, plus row counts after 6,446 distillations over a month, found `omni stats` run 131 times and `handoff`, `rewind` and `rewrite` run **zero** times each. `handoff` (326 LOC) and `rewind` (197 LOC) are gone entirely; `rewrite` lost its subcommand but **keeps its module**, because `cli::rewrite::rewrite_logic` is called from `hooks/pre_tool.rs:91` on every command and is the subject of open issue #157, deleting the file would have broken the pre-hook, which is why the cut list was checked against the call graph rather than against the usage numbers alone. `rewind` was inert end-to-end and not merely unused: `rewind_hash` is hardcoded `None` on every live path, the only caller of `store_rewind` is a test with the payload `"testing_payload"`, the table held 0 rows, and `store/query.rs` filters `WHERE rewind_hash != ''` so those OmniQL branches could never match. Net **-777 lines**. The RewindStore storage layer, the `Route::Rewind` variant and the `omni_handoff` MCP tool are deliberately left in place: the usage evidence comes from shell history, which says nothing about what an agent calls over MCP, and cutting them would be acting past the evidence. Tracked in #164.

### Fixed
- **A reader that closed early made OMNI panic, and `--help` exited 1 (#155)**: Rust sets `SIGPIPE` to `SIG_IGN` before `main`, so writing to a pipe whose reader has gone returns `EPIPE`, and `println!` panics on it. `omni --help | head -1` printed a panic and a backtrace note where `ls | head` prints nothing. **The mechanism the issue filed had already moved**: it named `e.print().expect("failed to print error")` at `main.rs:309`, which is what released 0.6.3 still panics in, but #166 replaced clap's help with a hand-rolled printer, so on current `main` the panic came from `println!` at `std/src/io/stdio.rs` instead, the same bug one layer down. Fixed at the entry point rather than at the writer, because it was never specific to help: measured on released 0.6.3 with a reader that closes immediately, **`--help`, `doctor`, `stats` and `session` all panicked**; on this build none of them does. Guarding each `println!` would have fixed the one command in the title and left the other three. `main` now restores `SIG_DFL` on Unix (`#[cfg(unix)]`; Windows has no `SIGPIPE` and surfaces a closed pipe as an ordinary write error), using `libc`, which was already in the tree via `rusqlite` and `tempfile` so nothing new compiles. The clap arm keeps a matching fix, `let _ = e.print()` rather than `.expect()`, per `CLAUDE.md`'s rule against `.expect()` on IO. Separately, `--help` and `--version` now exit **0**: clap reports both as an `Err`, the arm did not distinguish them from a parse failure, and `omni --help && echo ok` printed nothing as a result. An unknown flag still exits non-zero, asserted by its own test so the fix cannot quietly swallow #151's guard. Both halves were confirmed to fail independently: removing the `SIGPIPE` restore reddens the two panic tests, reverting the exit-code classification reddens `help_and_version_exit_zero`.

- **All six translated READMEs promised "up to 90%", the exact claim the English one calls a red flag (#132)**: the English README was rewritten in #131 / #150 to lead with the measured figure, and line 119 now warns that *"a tool that claims to save 90% of every command is telling you it summarises output you need"*. The translations were never updated, so `i18n/README-{ja,ko,zh,vi,id,ar}.md` each carried that promise in the hero line, the intro blockquote and a feature bullet, 2 to 5 occurrences per file, on a public repo, in six languages, contradicting the English page they link to. All of them now carry what the English one carries: **58.9% fewer bytes across a real command mix (15.0 MB → 6.2 MB), measured by replaying 1,810 real traces on the release binary, and 63.6% of calls saving nothing at all**, which is published rather than hidden. The one place "90%" survives in each file is the sentence warning against that claim, the translation of English line 119, which was already correct and stays. **Structural parity is not claimed**: the translations still follow an older English layout (the English page has since replaced its feature bullets with benchmark tables), so #132 stays open for the full re-sync; this change fixes the false claims only.
- **`docs/` still called `omni handoff` and `omni rewind`, deleted in #164, and two shipped example scripts failed silently (#180)**: verified against the built binary rather than the source, `./target/debug/omni handoff --json` and `omni rewind list` both exit 1 with `unknown command`, while installed 0.6.3 still answers both, which is why this went unseen. The MCP tool `omni_handoff` is unaffected and still registered; only the shell entry point is gone. Worse than stale docs, two runnable examples broke: `docs/autonomous-loops/shell-loop.sh` guarded its checkpoint with `omni handoff --json 2>/dev/null | jq … || echo "CONTINUE"`, so once the command disappeared **the loop kept running against a default it never computed**, a confident value standing in for a missing one, the defect this project exists to stop, and `mastra-integration.ts` did the same thing twice, wrapping the call in `catch { return true }` and `catch { return "Normal" }`. Those methods were **deleted rather than stubbed**, because a stub that reports a healthy loop from state it never read is the same bug with a nicer name; `getStats()` (`omni stats --json`) still exists and the example keeps it. `omni rewind` had no shell replacement to offer, it was inert end to end, so its section is removed and the MCP `omni_retrieve` path is named instead. #164's cut list was built from shell history and row counts and correctly noted it could not see MCP callers; what neither source covered was the repo's own shipped scripts, which no interactive history would ever show. Next subcommand removal should grep `docs/` and any executable example, not just the call graph.

- **`omni stats` counted savings the host had thrown away (#163)**: every `distillations` row with `agent_id = 'claude_code'` written before the #158 fix records a distillation that was computed, scored, routed and stored, and then dropped by Claude Code, because the hook emitted `updatedResponse`, a key it ignores. The agent read the raw bytes. Those rows sat in the same sums as the `omni exec` and pipe rows, where the same numbers are true, and nothing in a row says which binary wrote it. Measured on the maintainer's store, the same database through both binaries: released 0.6.3 reports `claude_code 2550 calls, 30.2%`; this build reports **`claude_code 237 calls, 44.6%, unverified=2313`**. `aider`, `terminal` and `codex_cli` are unchanged at `unverified=0`, the cutoff touches only the path that was broken. **The issue predicted the honest figure would come out lower and warned it would need explaining as a correction; on this data it goes up**, 66.9% → 69.4% overall, because the discarded rows averaged 29.2% and were dragging the mean down. What actually falls is volume: 6,728 → 4,415 calls, **−34%**. That is the number a user notices, so the excluded rows are **reported rather than removed**, `omni stats --detail` prints an `unverified` line under each agent, and `--json` carries an `unverified` field per agent, because a call count that shrinks without explanation reads as OMNI having stopped working. The gate is scoped to sums over the byte and token columns, **not** applied as a row filter: latency, command, project and file access in those rows really happened and stay queryable, and only the savings columns are fiction, deleting the rows would destroy true history to remove a false column, which is what never-drop argues against. The cutoff is `POST_HOOK_FIX_TS`, already in `pipeline/mod.rs` and already how the #109 re-run metric selects its population, so there is one rule and one constant rather than two. Known limit, chosen deliberately: someone who upgrades late has pre-fix rows written *after* that timestamp, so their figures stay generous; fixing that needs a writer-version column and was rejected as too much migration for a bounded, shrinking set. Three deliberate breaks confirmed the tests can fail, neutering the predicate reddens both exclusion tests, setting the cutoff to `i64::MAX` reddens `counts_claude_code_savings_recorded_after_the_fix` (proving it is a cutoff and not a blanket ban on the agent), and hiding the `unverified` column reddens the reporting test.

- **`readfile` deleted every function body from Python, JS/TS, Go and Java without saying so (#176)**: `distill_rust_file` appended `[Method bodies omitted…]`; the four siblings ended with a bare `out.trim()` and returned a skeleton with no indication anything had been removed. Measured through the post-hook on a 24,999 B Python file: **3,275 B out, 86.9% reported as a win**, and the rule the file was read for, `if order.total > 1000: order.apply_discount(0.1)` with a VIP branch below it, gone, along with every other body, buried under 120 repeated `def process(order):` lines. An agent asked what discount a 1,200 order gets had no way to know the answer had ever been in the file. That is the #111 never-drop violation: if bytes disappear the output says so. One shared `omitted_note(total, kept)` now carries the count and is used by all five distillers, with `distill_rust_file`'s hand-rolled line folded into it, a marker without a number does not let a reader judge whether to re-read. The same file now ends `... [601 of 723 lines omitted, bodies and comments not shown. Re-read with offset/limit for the full file.] ...`. The empty `--- … ---` sections are also qualified: the scan behind each heading runs over the **whole** file including the lines it drops, so `None` was true, but next to a visibly truncated body it could not convey whether it meant "absent from the file" or "absent from what you can see", it now reads `None in the full file`. That half is a labelling fix, not a guard: an earlier draft of #176 called it a false claim of the #112 `helm: 0 deployed` shape, which was wrong on re-reading the loop, and was withdrawn on the issue rather than quietly fixed. **This path is unreachable today**, the Claude Code hook is registered for `Bash` only (#172), and was fixed first for exactly that reason: widening the matcher before this would have shipped it on every source file over the threshold. The regression test walks all five languages and was confirmed to fail for each one individually with that language's marker removed.

- **`cargo tree` was distilled to `Build: ok`, 21.4 KB deleted and reported as a 100% saving (#170)**: every `cargo` command was routed to `BuildDistiller` on the executable name alone, with one exception for commands containing `test`. Most of cargo is not a build. `cargo tree` prints a dependency tree that *is* the answer, and 21,959 bytes of it came back as nine: `Build: ok`, under the footer `100.0% reduction`. This is the defect named in the 0.6.2 changelog, *"`find` reported ~99% by discarding the file paths that were the answer"*, with a different command, and `Build: ok` is additionally a claim nothing supports, since no build ran. It reached users who never type `omni`: `hooks::pre_tool` rewrites any command *starting with* `cargo ` into `omni exec <command>`, so a bare `cargo tree` was silently routed through it, **and redirecting to a file did not help**, because the rewrite wraps the entire command string including the redirect (`cargo tree > out.txt 2>&1` wrote one line where 477 were expected). Whether the tree survived depended on whether the command happened to begin with the word `cargo`; anything prefixed (`cd … && cargo tree`) escaped, which is why this went unseen. `cargo` is now routed by **subcommand**: `build`/`check`/`run`/`clippy`/`rustc`/`fix`/`doc`/`install` to `BuildDistiller`, `test`/`bench` to `TestDistiller`, everything else passthrough, the honest answer for output that is data rather than progress. The parser skips env-var prefixes, the path cargo was called by, `+toolchain` overrides and leading flags, and a missing subcommand (`cargo --version`) also passes through. `cargo metadata` was never affected: `pipeline::format::sniff` recognises its JSON and stands the lossy stages down, which is the argument for fixing this at the command level instead of teaching a content heuristic to recognise a tree. Measured end-to-end: released 0.6.3 returns 9 bytes, this build returns all 21,959. A companion test asserts noisy `cargo build` output is *still* distilled, so the fix cannot quietly end savings on the commands that do work.
- **`omni stats` filed Claude Code's work under `Terminal`, and some of it under an agent that never ran (#160)**: OMNI had **three** private ways of naming the agent, not two. `hooks::normalize::detect_agent_id` reads the payload shape on the hook path; `agents::multiagent::detect_agent_id` reads the environment; and `hooks::pipe::resolve_pipe_agent_id`, the one that actually writes the rows on the `omni exec` and pipe paths, had its own rules and consulted neither. It knew `OMNI_AGENT_ID`, then guessed **`aider`** for anything with `OMNI_CMD` set (a variable OMNI documents for its own pipe mode, which any caller may set, 3,296 rows on one machine were filed under an agent that had not run), and called everything else `terminal`. Meanwhile the env detector had no Claude Code branch at all, so even where it did run, the most widely deployed agent OMNI supports was the one it could not see. Measured end-to-end, same two commands through each binary: released 0.6.3 records `terminal|2`, this build records `claude_code|2`. The pipe resolver now defers to the single env detector, which gained a Claude Code branch keyed on `CLAUDECODE`/`CLAUDE_CODE_ENTRYPOINT`, placed **before** the VSCode branch, since Claude Code in VS Code's integrated terminal sets `VSCODE_PID` and `TERM_PROGRAM` too and would otherwise have been labelled `vscode`, trading a vague answer for a confidently wrong one. The env detector's ids were also reconciled with the payload detector's (`codex` → `codex_cli`, `continue` → `vscode_continue`), so one agent no longer splits into two rows of which neither is the true total. `agent_display_name` stops folding `unknown` into `Terminal`: "a human ran this in a shell" and "OMNI could not tell" are different facts, and collapsing them is what hid the missing branch for the life of the feature, a detection gap now reads as `Unknown`. A second, unreferenced copy of `agent_display_name` in `agents/multiagent.rs`, reachable only from its own test and still carrying the old ids, was deleted. The cross-detector agreement check lives inside the existing env test rather than beside it: as its own `#[test]` it passed alone and failed in the full suite, because env vars are process-global and the ambient `CLAUDECODE` shadowed the variable under test.
- **On Claude Code, the distilled output never reached the agent, and OMNI reported the saving anyway (#158)**: the PostToolUse hook serialised its replacement under `hookSpecificOutput.updatedResponse`. The key the host reads is **`updatedToolOutput`**, and it takes an object (`{status, result}`), not a string. Claude Code drops an unrecognised key without a word, so the model received the **full original stdout** while OMNI recorded the event as `Route::Keep`, documented in `pipeline/mod.rs` as *"score >= 0.7, full distillation"*, and printed a savings footer for it. A 60-line probe (`seq 1 60 | sed 's/^/line /'`, 470 B) arrived complete and was logged as `470 → 48`; a 3,449-byte `sed` of two source files arrived complete under the footer `[OMNI: -904tok this call | 100% compression]`. Everything OMNI exists to do was happening, being measured, and being discarded one field name short of the agent. Two things hid it: the sibling `additionalContext` **is** spelled correctly, so the footer appeared and the failure read as success; and every unit test asserted on the struct's own field, which passes with any key at all. `git log -S` puts `updatedResponse` in the initial Rust-migration commit, so this was never a regression, the Claude Code hook path had **never** applied a distillation. Confirmed from the host side rather than the docs alone: `strings` on the Claude Code 2.1.217 binary finds `updatedToolOutput` 13 times and `updatedResponse` zero (`hookSpecificOutput` 40, `additionalContext` 45). Scope was narrow but the worst-placed it could be: the `omni exec` and pipe paths write stdout directly and were always genuine, an eight-hour window on one machine had 139 `claude_code` rows claiming 377,697 → 286,648 bytes that never landed, beside 54 exec/pipe rows whose 16.9 MB → 224 KB was real. `status` is always `success` because a failed command returns `None` far earlier (#120) and never reaches this struct, so the new field cannot certify a success for a command that failed. The regression test asserts on the **serialized JSON**, not the struct, since that is the only level at which a wrong key is visible; it was confirmed to fail with the old key restored. An existing security test that asserted `updatedResponse.is_string()`, and had been holding the wrong contract in place, was corrected to the real shape.
- **`CloudDistiller` guessed which tool ran from the output's shape, and reported a tool that never ran (#112)**: the dispatcher already resolves the base command, that is how `CloudDistiller` gets selected at all, but the `Distiller` trait had no `command` parameter, so `cloud.rs` threw that away and re-derived identity from the content one stack frame later. Its first content test was `is_docker_logs`, a pure shape heuristic ("≥5 of the first 20 lines are longer than 20 chars and start with a digit or `[`") that any timestamped log matches. So `kubectl exec -it pod-0 -- sh -c 'ls -la /data'`, output beginning `ls: /data/models: No such file or directory` followed by eight heartbeat lines, was handed to the docker summariser and came out, end-to-end through the released 0.6.3 binary, as **`docker logs: 9 lines, no errors detected` (596 B → 40 B, 93.3% reduction)**. Two falsehoods in fourteen words: no `docker` was involved, and the one real error was deleted and replaced with a denial that any error existed. The #143 zero-state guard did not catch it, because `is_docker_logs` was *true*, the guard asks "did I parse anything", and a misrouted payload that matches the wrong tool's shape answers yes. Sibling branches had the same defect: `input.contains("aws ")` claimed any output mentioning `aws `, `input.contains("terraform")` any line naming terraform, `input.contains("kubectl")` any log that quoted a kubectl command. `CloudDistiller` now carries the resolved tool (`CloudDistiller { tool: &base }`) and matches on it first; content heuristics are demoted to what they can actually decide, which *format* within one tool (`docker ps` vs `docker build` vs `docker logs`). The same input now passes through unchanged at 0.0% reduction, error intact. Three zero-states that tool-gating newly makes reachable are guarded in the same change, since gating widens what reaches them: **helm** (`helm: 0 deployed, 0 failed, 0 pending` asserted a cluster state that was never read, now requires helm's own `REVISION`+`CHART`+`STATUS` header, not the `NAME`+`STATUS` prefix half of kubectl's tables also print), **terraform** (`terraform: +0 ~0 -0 resources` reported an empty plan for `terraform init` output that contained no plan, now requires a `Plan:`/`will be …`/`Apply complete!` line), and the generic **fallback**, which returned an empty string when no segment scored Critical or Important, deleting the output entirely and booking it as a 100% saving. All four regression tests were confirmed to fail with `require_parsed` neutered.
- **The CLI accepted flags it did not understand and reported success (#151, partial)**: sixteen of eighteen subcommands are declared as a `trailing_var_arg` catch-all (`extra: Vec<String>`), and each `cli::*::run` then re-parses raw argv by hand with `args.iter().any(|a| a == "--detail")`. Nothing enumerated the valid set, so nothing could detect a value outside it: `omni stats --detial` printed the default overview and exited **0**, and `omni init --curser` ran the interactive default and exited 0 while installing nothing the user asked for, the same defect this project files against its own distillers, confident output over input that was never parsed. Two properties now come from one declaration per command, a `(flag, description)` list that both the help printer and the new `cli::check_flags` read: an undeclared flag is an error with a Levenshtein suggestion (`unknown flag \`--detial\` for \`omni stats\`, did you mean \`--detail\`?`), and help cannot drift from what is accepted. Only long `--flags` are checked unconditionally; a single-letter `-x` is checked only where the command declares short flags, so free text (`omni remember "build with -O2"`) still passes through. `omni init`'s result was also being discarded with `let _ =`, which would have swallowed the new error and kept exit 0. Converted so far: `stats`, `init`, `session`, `doctor`, `learn`, the commands with real flag sets, where a swallowed typo changes what runs. The remaining eleven still accept anything; #151 stays open for them.
- **`--help` printed a stub while the real help was unreachable (#151)**: clap intercepted `--help` and `-h` for every subcommand and printed `Usage: omni stats [EXTRA]...` with no flags at all, while the actual per-command help, accurate, with flags and examples, already written in twelve modules, was reachable only through the undiscoverable bare word `omni stats help`. Twelve subcommands now set `disable_help_flag`, so `--help` and `-h` reach the module that has something to say. That alone moves 47 implemented flags from "documented nowhere" to "documented where a user looks for them". The flag column is now sized to its longest entry rather than a fixed 12, which `--all-commands` and `--validate <file.toml>` both overflowed into their own descriptions.
- **Five more distillers could certify a clean result they never parsed (#143)**: the #115 vitest fix (`✓ 0/0 passed` for a dev server) was one instance of a class, a distiller whose "nothing bad found" branch emits a green string (`no errors`, `passed`, `no problems found`, `no issues found`, `no errors detected`) that is byte-identical to a real success, with no check that any signal was actually parsed. So any upstream misdetection, a `tsc --` echo matched with no `error TS` line (#106 shape), a `@typescript-eslint/` mention in non-eslint output (#114 shape), a manifest that merely names `docker logs` (#112 misroute), converted silently into a confident false pass. `AGENTS.md` requires hooks to *fail open*; emitting `no errors` on unparsed input is failing *closed and confident*. The shared `require_parsed(parsed, input, summary)` helper (added with #115) is now wired at every remaining zero-state: **tsc** (only claims `no errors` when it saw an `error TS` line or a `Found N errors` summary), **playwright** (only when it parsed a passing count or a `✓` line), **eslint** (only on a real `problems (` summary or a parsed finding, a bare file list like prettier's no longer qualifies), **security** (only when at least one severity token was seen), and **docker logs** (only when the input is actually log-shaped per `is_docker_logs`). With no signal, each returns the input unchanged instead of a green string, so a future misdetection degrades to passthrough, not a false claim. Each site carries a regression test that routes a detector-tripping but signal-free payload through `distill_with_command` and asserts the false claim is absent and the original content survives; all six were confirmed to fail with the guard neutered. Individual detector fixes still ship per-issue (#106/#108/#112), this is the guardrail that makes their false-claim class un-reintroducible.

## [0.6.3] - 2026-07-21

### Fixed
- **A failed command could be distilled into output that reads as success (#120)**: OMNI's `normalize` layer parsed each agent's failure signal and then threw it away: Codex `exit_code` was never read into `CodexInput`, Pi `toolResponse.isError` sat behind `#[allow(dead_code)]`, and MCP `result.isError` was named in a comment but never deserialized, so a command that exited non-zero still ran the full distiller. A failed `docker build` (`exit_code 1`) on the heavy-noise fixture came out **9,207 → 6,090 bytes**, silently trimmed by the same `DEBUG`/`INFO` stripping a *successful* build gets; the filed case was a `vault` call that failed `exit=2` on a network timeout yet surfaced a clean, fictional `["n8n"]`. This is the worst failure mode, a fabricated success terminates investigation, while a fabricated error only costs a retry. `NormalizedInput` now carries `failed`, set from each agent's own signal, and `post_tool::process_payload` passes a failed command through verbatim at zero marker cost before any distiller runs. Successful commands are untouched, the same output with `exit_code 0` still distils to 6,090 bytes. Claude Code needed no code change: it already sends a failed command as a bare `tool_response` string (`"Error: Exit code N…"`) that never parses into a success summary, and a regression test locks that in so a future, more-lenient parser cannot silently reintroduce the fabrication. The `omni exec` / `pipe.rs` path reads piped stdout only and never sees the child exit code; that gap is closed by the next entry.
- **`omni exec` distilled failed commands too (#122)**: the same invariant as #120, one layer down and from a different cause. `cli::exec` streamed the child's stdout straight through the distiller and only called `child.wait()` *afterwards*, so by the time the non-zero exit was known the distilled output had already been written, a failed command run through OMNI's own reproduction harness came out distilled. The exit code is only knowable once stdout is fully drained (and draining before `wait()` is also what avoids a full-pipe deadlock), so exec now buffers stdout, waits, then gates: non-zero exit emits stdout **verbatim** and skips distillation; zero exit distils exactly as before. Measured on 60 identical noise lines: `exit 1` now yields 60 verbatim lines where it previously collapsed to a single `[60 similar lines collapsed]` marker; `exit 0` still collapses. Stream-mode commands (`docker`/`npm`/`bun`) emit line-by-line before the exit code exists and cannot be gated, so they keep streaming; the stream-filter lookup is now shared via `pipe::stream_filter_for` so exec and the pipeline agree on which commands stream.
- **Distillers parsed OMNI's own collapse markers as data, and weak TOML filters hid it (#116, #110)**: `collapse` runs before `distill`, rewriting repeated lines into `[N similar lines collapsed]` markers, and a distiller that parses columns then read those markers as rows. A 35-row `kubectl get pods` table (30 Running, 5 CrashLoopBackOff) came out as `k8s: 2 pods | 0 running, 0 pending, 2 error / Problems: [30 (lines), [5 (lines)`, every reported "pod" was OMNI's own scaffolding, the real statuses destroyed. A distiller is just a later stage that parses its input, exactly what `pipeline::format::sniff` already shields structured payloads from; nothing shielded the distiller. It survived unseen because the broad `signals/domains/*.toml` filters won the alphabetical `find()` race and short-circuited the distiller for cargo/npm/docker/kubectl/terraform before it ever ran (#110). Two coupled fixes: (1) the distiller now reads the tool's **raw** output, collapse feeds only scoring and the fallback for commands no distiller claims, chosen by the shared `beats_guardrail` so a distiller that punts still yields the collapsed line savings; (2) a TOML filter only short-circuits the distiller if it actually beat that guardrail (ported from `hooks::pipe`, now shared in `guard::limits`), so weak filters fall through. Together the same table now distils to `k8s: 35 pods | 30 running, 0 pending, 5 error` with the five CrashLoopBackOff pods named. `TomlFilter::priority` remains parsed-but-unread, filed as #119, not fixed here.
- **`omni exec` ran a corrupted command when a shell was involved (#125)**: `cli::exec` flagged `needs_shell` whenever *any* argv element contained a shell metacharacter, then rebuilt the command as `format!("{} {}", cmd, cmd_args.join(" "))` and ran it under `sh -c`. For `omni exec sh -c 'echo A; echo B'` that produced `sh -c "sh -c echo A; echo B"`, a second shell layer, with `join(" ")` having already flattened the original quoting, so the outer shell ran `sh -c echo A` (A became `$0`, not an argument) and then `echo B`, and the output was just `B`. `omni exec` is the repo's own reproduction harness, so any repro of the form `omni exec sh -c '…'` silently ran the wrong command. A shell is now used only when the command arrived as a single unsplit string (`omni exec 'a; b'`); when argv is already split, each element belongs to the program and is passed through verbatim, the metacharacters are the inner program's, not omni's to reinterpret.
- **Composite npm scripts were distilled as a single tool, discarding the other gates (#106)**: a chained script (`npm run verify` = `build && tsc --noEmit && eslint && check:secrets && test`) concatenates several tools' output into one buffer, but the JS/TS dispatcher picked **one** distiller from the first signature it saw and handed it the whole thing. `is_tsc_output` matched `tsc --` inside npm's own `> build && tsc --noEmit && …` echo, so **19.5 KB collapsed to 14 bytes of `tsc: no errors`**, a false success that also erased four gate verdicts, including the secret scan's positive control (the line that proves the scan could see inside the bundle at all). Over-distilling a composite is token-*negative*: the agent re-ran `npm test`, `check:secrets`, and `lint` to recover what was dropped. No per-tool distiller can safely own a composite, an `&&` chain has no delimiter between the tools' outputs, so the dispatcher now detects npm's `> … && …` echo and declines, returning the buffer for the pipeline's generic collapse, which folds the repeated build noise while keeping every gate's distinct verdict line. (`make`/`npm-run-all` composites without an `&&` echo aren't covered yet, filed as #129.)
- **`npm run format` (prettier) was reported as `eslint: no problems found` (#114)**: three compounding prettier defects in `distillers::jsts`. (1) `is_eslint_output` matched the bare word `eslint`, which appears as a *filename* (`eslint.config.js`) in prettier's file list, so a `prettier --write` run that rewrote files across 109 paths was distilled by `distill_eslint` as a clean run of a **different tool** (the same substring-in-data trap as #105/#106; a destructive command reported as finding nothing). (2) `is_prettier_output` only matched lowercase `checking `/`reformatted `, but prettier prints `Checking formatting…` and `[warn]`, so the detector was dead and never fired on real prettier output. (3) `distill_prettier` parsed **black**'s `reformatted N files` summary, which prettier never prints, so both counters stayed 0 and a *failing* `--check` and a *successful* `--write` both rendered as `prettier: 0 files reformatted, 0 unchanged`. All three are fixed: eslint detection now anchors on eslint's real line shape (`✖ N problems (`, a rule id, or a `<line>:<col> error|warning` finding) instead of a substring that occurs in filenames; prettier detection matches what prettier actually prints; and `distill_prettier` is rewritten against prettier's real output, `--check` surfaces the offending filenames (`prettier --check: N file(s) need formatting`), `--write` reports `N reformatted, M unchanged` and names the changed files (capped), and it declines (returns the input) rather than fabricate a count when neither mode is recognisable. Snapshot tests added for both modes. (Defect 4 in the report, `--check` output reordered with the `[warn]` filename dropped, is an upstream stdout/stderr-ordering observation the reporter did not trace; left for a separate issue.)

## [0.6.2] - 2026-07-17

### Fixed
- **Format-safe compression (data integrity)**: The pipeline had no format awareness, so collapse squashed the repeated lines of structured output into `[N similar lines collapsed]` and left the payload unparseable, breaking any downstream `jq` / `json.load` / `kubectl apply`. A JSON dashboard piped through the hook came out with 14 collapse markers and failed `jq` outright. A new format sniffer (`pipeline::format`) now gates both choke points (`hooks::post_tool`, `hooks::pipe`): JSON, NDJSON, YAML, TSV, and CSV pass through byte-for-byte at zero marker cost, while plain text still compresses as before.
- **YAML with an embedded block scalar was not detected as YAML**: `sniff_yaml` judged every line of a document that lacked a leading `---`, so a ConfigMap carrying Vault HCL (`config.hcl: |`) made a 608-line `kubectl kustomize` manifest look like free text, 26 of its 608 lines were HCL, not YAML. The format gate stood down, collapse emitted `[8 similar lines collapsed]` markers, and `is_docker_logs` counted those markers as timestamp prefixes (12 triggers, needs 5), handing the whole manifest to `distill_docker_logs`: **13,463 bytes of Kubernetes config came out as `docker logs: 323 lines, no errors detected`**. OMNI's own collapse markers fabricated the evidence the next stage misread. A block scalar's body is now skipped rather than judged, the `|` that opened it is YAML's own signal that what follows is a value, so detection stays positive.
- **`find` deleted its payload and called it a saving**: the `SystemOpsDistiller` behaviour shipped in 0.6.1 ("strips out raw file path samples entirely", credited below with "up to 99.8% savings") reported ~99% by discarding the file paths that *were* the answer to the question. Replaced with lossless prefix factoring: the shared directory prefix is hoisted to a header and each path stated once relative to it, cut at a separator so every path round-trips byte-for-byte. Real saving on the same output: ~72%, with nothing lost. `grep` now hoists each path once instead of repeating it per match (26-39%, also lossless), and both hand back the input untouched when factoring would grow it.
- **A weak TOML filter shadowed the test distiller**: `sys_build_domain` matches every `cargo` invocation but strips only `Compiling`-style lines. It won the `find()` race, shadowed `TestDistiller`, cut 1%, and had that cut discarded by `best_output()`'s `MIN_REDUCTION_PCT` guardrail, so `cargo test` reported **0%** on output the distiller reduces by 94%. A TOML filter now only short-circuits the distiller if it actually beat the guardrail; a filter that earns its match still wins, user filters included.
- **`cargo test` totals were recounted instead of quoted**: with the distiller reachable again, it counted result-line segments and reported `1 passed` for a run cargo itself called `490 passed`, `CollapseMode::Test` folds the 330 `... ok` lines away before the distiller ever sees them. It now quotes the runner's own summary line (cargo, pytest, jest), falling back to counting only when no summary was printed.
- **No 0.6.2 binaries could be built at all**: `release.yml` asks `dtolnay/rust-toolchain@stable` for each matrix target, which installs that std into *stable*, but `rust-toolchain.toml` (added this release) makes cargo switch to the pinned 1.97.0, which has std for no target but the runner's own. Every cross-compile died with `error[E0463]: can't find crate for 'core'` before compiling a line of OMNI, and fail-fast took the rest of the matrix with it, so the tag produced no release. The matrix targets are now listed in `rust-toolchain.toml` itself, installing them into the toolchain cargo actually uses. `ci.yml` stayed green throughout, it only builds host-native, where std is already present, so this was invisible until a tag was pushed.

### Removed
- **`est_cost_usd` and `ModelPricing`**: OMNI is a hook and never sees the API's `usage` block, so it cannot know whether bytes were billed as fresh input or as a ~10× cheaper prompt-cache read. `ModelPricing` used only `.input`, and its `_cached` / `_cache_creation` fields were dead, every dollar figure OMNI printed was a fresh-input guess presented as a session cost. The formula was also duplicated across four call sites. Removed from `stats`, `diff`, and `guard::config`; unknown keys in an existing `config.toml` still parse.

### Changed
- **Published numbers replaced with measured ones**: the headline claims were not stale, they were untrue. `docs/PERFOMANCE.md` sold `docker build` as 9.2 KB → **49 bytes (99.5%)**; the fixture it names distils to 5,783 bytes (**37.2%**). `git diff` quoted **50.0%** on bytes (397 → 220) that compute to **44.6%**. The **97.3%** all-time figure traced to `find . (-12.6M tokens)`, find deleting its payload, the defect fixed above, so the number was manufactured by a bug. `$35+ USD/month` priced the estimator removed above, `~40% faster TTFT` was never measured, and three testimonials in quote marks were attributed to nobody because nobody said them. Replaced with a replay of **1,810 real execution traces** on the release binary with a fresh `HOME` per invocation: **58.9% net** (15.0 MB → 6.2 MB), `git` 91.3%, `cargo` 96.8%, `cat` 9.1%. Fixing the YAML sniffer moved reported savings **down**, 65.3% → 58.9%, because six of those points were destroyed manifests rather than removed noise. All six i18n READMEs carry the same numbers.
- **Two costs now documented rather than hidden**: OMNI's output is **not deterministic** against a warm `~/.omni/omni.db`, session history feeds the scorer, so the same binary on the same input differed on 21 of 30 traces run-to-run (one gave 1,835 bytes, then 433); any A/B measurement must isolate state per invocation. And latency is real: ~82 ms for a 496-byte `git status` against a fresh database, **~308 ms** against a 97 MB one, per hooked command, growing with history.

## [0.6.1] - 2026-06-24

### Added
- **Pain-First Positioning**: Completely refactored `README.md` to focus on the core narrative solving Terminal Noise and Context Amnesia ("Noise-canceling headphones for your AI agent").
- **Brand Evolution**: Redesigned the primary `logo.svg` with a sleek, neon-style "Cute Brain" wearing headphones, visualizing the integration of Noise-Canceling and the Adaptive Memory OS.

### Improved
- **AI-Native Distillation**: Revamped the `SystemOpsDistiller` for the `find` command. It now strips out raw file path samples entirely and emits a highly compressed, AI-friendly key-value summary of directory distributions and extension counts, maximizing token reduction (up to 99.8% savings).
- **Terminal UI Polish**: Fixed an alignment issue where the `[OMNI Active]` status tag would overlap with command outputs by ensuring trailing newlines in distilled payloads.

### Fixed
- **Clippy Strictness**: Resolved a `clippy::needless_splitn` warning in the directory classification logic, ensuring `make ci` continues to pass with zero warnings.
- **Snapshot Integrity**: Synchronized all `insta` snapshot tests to align with the new, denser `find` output format.

## [0.6.0] - 2026-06-14

### Added
- **Autonomous Loop Engineering**: Native support for iterative, autonomous agent loops (`OMNI_LOOP_BUDGET`, `OMNI_LOOP_GOAL`) with predictive goal-driven constraints.
- **Maker-Checker Verification Pattern**: Introduced `omni_verify` MCP tool to separate execution and validation securely across distinct agent sessions.
- **Test Suite Modernization**: Renamed all generic sprint-based test files to context-aware descriptive names (`session_state_tests.rs`, `security_validation_tests.rs`, etc.) and achieved 100% test coverage across 941 tests.
- **Production Hardening**: Added robust input sanitization, loop context injection prevention, and performance benchmark tests (latency thresholds adapted for CI resilience).
- **Consolidated Documentation**: Unified scattered templates into a cohesive `docs/LOOP_ENGINEERING.md` master guide and updated all global `i18n` READMEs to reflect new multi-agent loop orchestration capabilities.

### Fixed
- **Critical UTF-8 Panic Resolution**: Completely resolved `SIGABRT` crashes caused by multibyte characters (emojis, box-drawing, CJK) in terminal output. Implemented `char`-boundary safe string truncation and slicing utilities globally.
- **Pipeline Data Integrity**: Rewrote ANSI stripping and structural normalization engines to process by Unicode `char` rather than raw `byte`, preventing *mojibake* and data corruption on rich terminal outputs.
- **Release Stability**: Removed `panic = "abort"` from the release profile to allow `catch_unwind` guards to gracefully handle unexpected panics in production builds.

### Performance
- **Zero-Allocation ANSI Stripping**: Redesigned `strip_ansi` to use `Cow<'_, str>`, eliminating heap allocations entirely when processing clean terminal outputs.
- **Pattern Caching**: Implemented a thread-local LRU cache for `normalize_structural`, bypassing expensive regex and grapheme-cluster calculations on highly repetitive log lines.
- **Stream-Aware Output Limits**: Deployed `TruncatingWriter` in the streaming pipeline. Omni now tracks payload bytes on the fly and intelligently truncates multi-gigabyte outputs at valid UTF-8 boundaries, eliminating OOM vulnerabilities on massive terminal bursts.

### Changed
- **Column-Aware Truncation**: Refactored CLI formatting utilities (`diff`, `learn`, `stats`) to use `unicode-width` logic, ensuring terminal tables and summaries render perfectly aligned even when displaying full-width CJK characters or emojis.

## [0.5.9] - 2026-06-04

### Added
- **Engram (Automatic Subtask Digest)**: Rule-based state snapshots capturing subtask progress (e.g., error resolved, commits, test passes) to prevent context amnesia during long sessions.
- **Session Health Dashboard**: Introduced `omni session --health` to visualize context pressure, token savings, engrams, tool activity, and hot files.
- **Smart PreCompact v2**: Intelligent, priority-aware context packing (Errors > Engrams > Tool Summary > Hot Files) with SHA-256 delta detection to skip redundant injections.
- **Session Handoff & Portability**: Added `omni_handoff` MCP tool to export session state as portable markdown, enabling seamless context transfer between terminal sessions.
- **Rolling Tool Call Summary**: Aggregates the last 50 tool calls with success/error rates, exposed via the `omni_session("summary")` tool for agent reference when context pressure is high.
- **Periodic Context Re-injection**: Automatically re-injects critical pinned files (like `AGENTS.md`) into the agent's context when pressure is elevated and after a set interval.

## [0.5.8] - 2026-06-03

### Added
- **Streaming Distillation Pipeline**: Introduced memory-efficient, line-by-line processing (`src/hooks/pipe.rs`) to handle long-running and piped commands without memory exhaustion.
- **Expanded Semantic Distillers**: Added new declarative TOML signal profiles for 9 developer tools: `aws`, `az`, `bun`, `deno`, `docker`, `gcloud`, `npm`, `vite`, and `webpack`.
- **Brand & Documentation Modernization**: Completely overhauled the English `README.md` and all 6 `i18n` localized versions with professional SVG visual assets (`hero.svg`, `architecture.svg`), new "Under the Hood", and "Real-World Use Cases" sections.
- **Advanced Context Analytics**: Implemented context composition metrics in `omni stats` (`src/analytics/context_composition.rs`) to provide deeper visibility into token reduction and signal density.

### Improved
- **SQLite Storage Enhancements**: Updated `src/store/sqlite.rs` to seamlessly support high-throughput, chunked streaming outputs from the new distillation pipeline.
- **MCP & Tooling Synchronization**: Refined `pre_tool` and `post_tool` hook routing to align with the new semantic models.

## [0.5.8-rc3] - 2026-05-29

### Added
- **Context Pressure Management**: Implemented multi-stage context pressure warnings (Normal, Warning, Critical) to proactively manage session token budgets.
- **Critical File Pinning**: Added automatic context pinning for critical rule files (e.g., `.cursorrules`, `AGENTS.md`) during session compaction.
- **File Re-read Guard**: Introduced preventive warnings and hot-file mutation protection to stop redundant reads of files already in context.
- **Performance Documentation**: Added a comprehensive `docs/PERFOMANCE.md` showcase and updated all global `README.md` (and `i18n` translations) with actual ROI and noise-reduction benchmarks.
- **AGENTS.md**: Established `AGENTS.md` to define multi-agent coordination rules, development gates, and context lifecycle management protocols.

### Changed
- **Pipeline Fail-Open Architecture**: Reinforced pipeline hooks (`pre_tool`, `post_tool`, `pre_compact`, `session_start`) with strict fail-open logic to ensure zero disruption to agent operations.
- **Dependency Update**: Validated and integrated `rmcp` 1.7.0 updates using structured `Parameters<T>` and `JsonSchema` across the MCP integration.
- **CLI Formatting**: Minor styling improvements to `omni stats` terminal output.

## [0.5.8-rc2] - 2026-05-28

### Added
- **Pi Agent Integration**: Added first-class support for Pi Agent integration with init, reset, and doctor support, including hooks, extension, and toggle functionality.
- **VS Code MCP Initialization**: Introduced the `--vscode` flag to the `omni init` command for automatic VS Code MCP server configuration.
- **Enhanced Token Metrics**: The `omni stats --detail` pipeline now accurately tracks and displays raw vs filtered token counts in a dedicated "Tokens Reduced" column, providing precise visibility into token savings.

### Changed
- **Semantic Classification Engine**: Refactored the core pipeline filtering system to utilize a semantic classification engine for segments with tool-aware scoring logic.
- **Filter Configuration Layout**: Migrated filter definitions from the legacy `filters/` directory to structured `signals/tools/` and `signals/domains/` configurations.
- **MCP Framework Upgrade**: Upgraded `rmcp` to `1.7.0`, migrating all MCP tool handlers to strongly-typed `Parameters<T>` structs and `JsonSchema` for robust, type-safe request routing.

### Fixed
- **Claude Code Async Hooks**: Ensured Claude emits an empty matcher for async hook entries to prevent stalling.
- **Hermes Integration**: Fixed the detection logic for packaged Hermes OMNI plugins during `doctor` and initialization checks.

## [0.5.8-rc1] - 2026-05-08

### Added
- **Semantic Session Guardrails**:
    - **Hot File Detection**: Triggers `SessionEnd` hook when active files in a session show abnormal mutation frequency, prompting agents to review before committing.
    - **Build Failure Preservation**: `BuildDistiller` now preserves `CommandOutput` for build failures (non-zero exit codes) in the collapse pipeline, ensuring agents see exact errors instead of just exit status.
    - **Diagnostic Context**: `PreBuild` hook runs `cargo check` to surface compiler errors early in the session, reducing wasted tokens on broken states.
- **Passthrough & Thresholding**:
    - **OMNI_PASSTHROUGH**: Support for raw output emission via environment variable for manual debugging.
    - **Smart Bypass**: Automatic distillation bypass for small configuration files and content under a 2000-token minimum threshold.
    - **Extension Hinting**: Improved content-aware heuristics for more accurate token estimation.
- **Omission Transparency**: Added explicit `[OMNI: omitted X lines of noise]` markers in the `GenericDistiller` for improved agent situational awareness.

### Improved
- **Performance & Caching**:
    - **Filter Fingerprinting**: New caching system to reduce redundant TOML filter loading.
    - **Thread-Safe Loading**: Optimized filter registry access using Mutex and fingerprint-based verification.
- **Agent Attribution & Stats**: 
    - Standardized "terminal" as the default agent identifier for untagged sessions.
    - Improved agent distribution grouping and filtering in `omni stats` output.
    - Updated CLI visuals to use `bright_black` for secondary log signals to improve visibility.
- **Test Suite Modernization**: Systematically refactored the entire Rust test suite (~300 tests) to align with modern idiomatic standards.
    - **Naming Convention**: Dropped the redundant `test_` prefix inside `#[cfg(test)]` modules.
    - **Action-Oriented Naming**: Transitioned to behavioral function names (e.g., `returns_*`, `preserves_*`, `rejects_*`) to improve readability and maintainability.
    - **Language Standardization**: Purged all remaining Indonesian terminology and mixed-language test names, ensuring a 100% professional English testing layer.
- **CI Stability**: Verified full CI compliance after the bulk refactor, ensuring all 282 tests remain stable across platforms.

### Fixed
- **Unsafe Environment Access**: Wrapped `std::env::set_var` and `remove_var` calls in `unsafe` blocks within `src/guard/env.rs` to comply with the latest Rust edition requirements for test isolation.
- **Distillation Regression**: Resolved failures in `test_readfile_large_rust_file_distilled` by recalibrating token thresholds in test fixtures, ensuring consistent distiller triggering.
- **Snapshot Integrity**: Synchronized stale `insta` snapshots to match refined pipeline output patterns, maintaining high-fidelity regression tracking.

## [0.5.7-rc3] - 2026-05-07

### Added
- **Hermes Agent Integration**: New native plugin integration for the Hermes Agent. Features automatic `plugin.yaml` and Python hook script generation (`post_tool_call`, `pre_tool_call`, `on_session_start`) to silently filter noise in the background.

### Improved
- **Automated Doctor Fix Mode**: Massively refactored agent integrations (`cline`, `codex`, `cursor`, `gemini`, `antigravity`) to support a standardized configuration path management and automated `--fix` operations.
- **Claude Hook Cleanup**: Implemented robust `uninstall` logic in the Claude Code integration to completely scrub OMNI hooks and MCP server entries from `settings.json` and `.claude.json`.
- **OpenClaw Portability**: Refactored the OpenClaw plugin's TypeScript configuration to use standard `Node16`/`ES2022` settings instead of relying on sandbox-specific file paths. Also renamed the plugin directory from `integrations/openclaw` to `plugins/openclaw`.

### Fixed
- **Clippy Strictness**: Resolved hidden nested-if collapsible warnings (`clippy::collapsible_if`) inside `claude.rs` ensuring zero warnings under `#![deny(warnings)]`.

## [0.5.7-rc2] - 2026-05-06

### Added
- **Lightweight Anti-Hallucination Guards**: Added factual warnings when OMNI knows context is incomplete, including heavy-compression-without-rewind cases and high-impact file reads with many dependents.
- **ReadFile Dependency Context**: `ReadFile` distillation now surfaces dependency impact using graph-derived `imported_by` counts so agents know when a file change may have broad blast radius.
- **Hot File Mutation Warnings**: `PreToolUse` now warns before mutating commands touch files that are already hot in current session context.

### Improved
- **Graceful ReadFile Fallback**: `post_tool` now falls back to base `distill_readfile` flow when graph indexing is unavailable instead of dropping contextual file distillation entirely.
- **Doctor Auto-Repair Coverage**: Agent diagnostics continue to auto-repair missing integrations while preserving stronger validation for installed MCP entries.
- **Cursor MCP Validation**: Refactored Cursor integration checks around structured JSON validation and idempotent install/remove behavior for `~/.cursor/mcp.json`.
- **Code Quality**: Resolved fresh Clippy warnings, including regex construction in loops and nested conditional lint violations, while keeping fallback paths architecturally live.

### Fixed
- **ReadFile Wrapper Regression**: Restored legitimate `distill_readfile` usage through real fallback path instead of silencing dead-code warnings by removal.
- **Context Warning Reliability**: Fixed hook pipeline paths so anti-hallucination warnings only emit from factual runtime signals already known by OMNI.

## [0.5.7-rc1] - 2026-05-03

### Added
- **Automated Tool Distillation**: Implemented automated distillation for MultiEdit and unhandled tool outputs to reduce token usage natively.
- **Context-Aware Estimation**: New token estimation utility for highly accurate cost and usage calculations within the ROI monitor.
- **Positional Boosting**: Extracted positional boost logic to the semantic scorer, implementing dynamic priority-based segment distillation.

### Improved
- **Security Hardening**: Expanded the denylist of restricted environment variables in `sanitize_env` to prevent injection attacks.
- **Hash Entropy**: Increased the RewindStore archive hash length from 8 to 16 hex characters, ensuring 64-bit entropy and preventing collision.
- **Diagnostic Detection**: Enhanced the `BuildDistiller` to natively detect and prioritize single-line diagnostics and preserve git commit hashes in the collapse pipeline.
- **Code Quality**: Applied consistent `rustfmt` formatting and resolved all emerging Clippy warnings across the codebase.

### Fixed
- **Command Routing**: Fixed a critical command detection bug by stripping surrounding quotes from command names during base path extraction, allowing Antigravity IDE and other quoted-path environments to correctly route tools to specific distillers.

## [0.5.7] - 2026-04-27
### Added
- **Multi-Agent Awareness (`omni_agents`)**: New MCP tool allowing agents (e.g., Claude, Cursor, Copilot) to discover and interact with each other's state on the same project.
- **Persistent Project Knowledge (`omni_knowledge`)**: Cross-session memory for agents to permanently learn and store project-specific quirks and filter preferences.
- **Advanced ROI Diagnostics**: Added `omni_history` (distillation log) and `omni_budget` (ROI simulator) MCP tools directly to the agent toolkit.
- **Meta-Harness Outer Loop**: Implemented `omni optimize` to automatically validate generated LLM filters.
- **Non-Bash Tool Distillation**: Expanded engine routing for `ReadFile`, `Grep`, and `WebFetch` output.
- **Distiller Context Preservation**: Added `-->` contextual error block preservation to the Build and Test distillers.
- **Extended Hook Architecture**: New async hooks for `SessionEnd`, `PostToolUseFailure`, `FileChanged`, and `SubagentStart`.
- **Antigravity IDE Integration**: Native MCP server bindings for Google's Antigravity environment (`~/.gemini/antigravity/mcp_config.json`).

### Improved
- **Positional Scorer Boost**: Dynamic positional-based priority bumping for active errors in multi-line outputs.
- **Passthrough Visibility**: Short or low-compression outputs are now explicitly labeled with `[OMNI: Passthrough]` rather than silently omitted.

## [0.5.6-rc3] - 2026-04-14
- **Database Distiller**: New `DatabaseDistiller` for intelligent distillation of PostgreSQL, MySQL, and SQLite CLI output, strips verbose headers and retains only actionable error signals.
- **Security Distiller**: New `SecurityDistiller` for CVE scanners (Trivy, Snyk, Semgrep), collapses verbose scan reports into concise vulnerability summaries.
- **VCS Distiller**: New `VcsDistiller` for version control tools beyond Git (Mercurial, SVN) with output-aware heuristics.
- **Expanded Tool Registry**: Added granular `cargo` subcommand support and new tool categories for Database, Mobile, Cloud, and CI/CD toolchains with accurate distiller routing.

### Improved
- **OpenClaw Portability**: The OpenClaw integration natively fetches plugin files directly from the public GitHub repository, allowing successful 1-click installation without requiring a full local git repository clone.
- **Robust RegEx Generation (`omni learn`)**: Fixed a critical bug where auto-learned numeric patterns used literal `#` instead of functional `\d+` in generated TOML filters. Now delegates TOML string escaping to the `toml` crate for correctness.
- **Enhanced Verify Report (`omni learn --verify`)**: Results are now grouped by source (Built-in vs. User), with clear per-category pass/fail counts and actionable tips when user-learned filters fail.
- **Auto-Clear Learn Queue**: `omni learn --apply` now automatically clears `~/.omni/learn_queue.jsonl` after successful application, preventing stale data from polluting subsequent `--discover` runs.
- **Premium Discover Table (`omni learn --discover`)**: Replaced raw text output with a structured `comfy-table` layout featuring color-coded actions (Strip/Count) and pattern previews.
- **Doctor Filter Diagnostics**: `omni doctor` now reports specific warnings for skipped filters (e.g., missing `match_command`) instead of generic error messages, and `--fix` can auto-repair invalid TOML files.
- **Distiller Robustness**: Replaced strict prefix checks with case-insensitive substring matching across all distillers for more reliable command detection.
- **Filter Loading**: Made `match_command` optional in `FilterConfig`, gracefully skipping filters with empty or missing patterns instead of crashing.

### Fixed
- **Learned Filters Concurrency (`learned.toml`)**: Replaced seconds-based timestamp resolution with `timestamp_micros()` for auto-generated filters to prevent fatal TOML duplication parse errors during high-frequency concurrent learning (fixes the infinite `doctor --fix` `.bak` failure loop).
- **Test Regression (`test_claude_code_stdout_format`)**: Resolved a persistent CI failure caused by state contamination from user-learned filters leaking into the test environment.
- **Stats UX Hint**: Added `--all-commands` usage hint to `omni stats` when showing truncated top-10 results.

## [0.5.6-rc2] - 2026-04-12

### Added
- **OpenClaw Support**: Introduced a native integration plugin for the **OpenClaw** agent framework. Includes a dedicated `omni_shell` tool for distilled execution and an `omni_rewind` tool for full log retrieval directly within the OpenClaw agent loop.
- **Command Grouping & Aggregation**: Enhanced `omni stats` to group identical or structurally similar commands (e.g., variant file paths in `ls -la`) into unified entries. This provides a significantly cleaner and more actionable signal report for repetitive tasks.

### Improved
- **CLI Semantic Clarity**: Renamed the `omni learn` flag `--status` to `--discover` to better align with its role in noise pattern discovery and candidate generation.
- **Null-Safe Telemetry**: Enforced robust null-safety handling in the SQLite storage layer using `COALESCE` for metric summations, preventing potential aggregation errors in sparse data environments.
- **Release Automation**: Hardened `bump_version.sh` and `omni-release.sh` to automatically synchronize version strings across the core Rust engine and the new OpenClaw integration plugin.

### Fixed
- **Integration Test Stability**: Updated `tests/savings_assertions.rs` to handle the expanded 4-tuple telemetry format, ensuring full CI compliance for the new grouping logic.
- **CLI Type Safety**: Resolved a type mismatch in the `omni stats --detail` view that caused formatting failures when processing heavily grouped filter entries.

## [0.5.6-rc1] - 2026-04-12

### Added
- **Magic Pipe Detection V2**: Automatic command source discovery via PGID inspection and parent shell fallback. This eliminates the need for manual `OMNI_CMD` labeling for piped commands in both interactive and scripted environments.
- **Configurable Token Pricing**: Introduced support for custom pricing models in `~/.omni/config.toml`, enabling accurate cost tracking for various models (e.g., GPT-4o, Claude Haiku).
- **Soft Route**: Fully implemented the 'Soft' distillation route for more flexible semantic engine behavior.
- **CLI ROI Metrics**: Expanded `omni stats --json` payload to include `savings_pct` for deeper CI/CD monitoring integration.

### Improved
- **High-Performance Filter Cache**: Implemented `OnceLock` caching for built-in TOML filters, significantly reducing overhead during high-frequency hook execution.
- **Command-First Architecture**: Completed the migration to a simplified engine by removing legacy `Classifier` and `Composer` modules.
- **Refined Stats UX**: Updated `omni stats` to strip redundant `omni exec` prefixes from automatically detected manual pipes for cleaner reports.

### Fixed
- **Post-Tool Telemetry**: Fixed a logic error in `src/hooks/post_tool.rs` that caused `segments_kept` and `segments_dropped` to be recorded as zero.
- **Stats Dead Code**: Cleaned up the "Distill" dead code path in stats color mapping and resolved various Clippy lints across the codebase.
- **Test Stability**: Hardened fragile assertions in `pipe.rs` unit tests to allow for benign local environment warnings.

## [0.5.5] - 2026-04-08

### Added
- **Command-Aware Intelligence**: Implemented path-aware classification heuristics to accurately detect terminal commands (e.g., `git`, `docker`, `kubectl`, `npm`) even when invoked via absolute paths.
- **Historical Data Re-classification**: Integrated "Intelligence Upgrade" into `omni doctor --fix`, allowing users to calibrate legacy 'Unknown' records with the latest classification models.
- **Cloud & Infra Heuristics**: Added native classification support for `kubernetes`, `terraform`, `aws`, `gcloud`, `helm`, and `azure` CLI tools.

### Improved
- **Real-time Update Notifications**: Reduced update check cache from 24 hours to **4 hours** and integrated proactive alerts directly into the `omni stats` dashboard.
- **Statistics UX**: Simplified `Unknown` category labels in the main signal report for a cleaner, more professional analytics display.
- **Classification Performance**: Optimized command-base matching to ensure sub-millisecond overhead during toolchain execution.

### Fixed
- **Code Integrity**: Resolved rusqlite iterator usage issues and addressed various Clippy lints to ensure 100% CI pass rate.

## [0.5.4] - 2026-04-07

### Added
- **OMNI Filter Pack**: Migrated and enhanced 12 new TOML-based filters for modern tools (Playwright, Ruff, golangci-lint, .NET, Prisma, Bun, Cypress, Jest, mypy, Black, pnpm, PHPUnit).
- **Core Distillers**: Implemented 3 new engine Distillers: `CloudDistiller` (Docker, K8s, Terraform), `SystemOpsDistiller` (ls, env, grep, tree), and `JsTsDistiller` (ESLint, TSC).
- **Session-Aware Distillation**: Injected tracking state directly into the distillers to unlock intelligent toolchain-specific context filtering logic.
- **Enhanced Stats Engine**: Upgraded `omni stats` command to fully support multi-period views, breakdown by context type, and valid JSON payload export.
- **Output Collapse Pipeline**: Added an algorithmic pipeline stage to collapse redundant contiguous lines to improve semantic block identification.
- **Quiet Mode Execution**: Introduced `OMNI_QUIET` environment variable to surgically suppress all stderr processing metrics for shell scripts.

### Fixed
- **Silent Exit Control**: Fixed persistent stderr pollution by ensuring OMNI terminates silently on completely blank piped inputs.
- **Security Check Integrity**: Updated the guardrail layer to ensure all denylist environment variable queries are strictly case-insensitive.
- **Windows Compatibility**: Updated GitHub Actions CI to matrix Windows tests and correctly restrict updater imports on unsupported OS.

### Improved
- **Zero-Mutation Tests**: Eradicated `std::env::set_var` to stabilize parallel thread runner execution via dependency injection (fixing deep UB).
- **Zero-Allocation ANSI**: Refactored the `strip_ansi` memory strategy to leverage `Cow<str>`, eliminating allocations for clean text snippets.
- **Pipeline Architecture**: Modularized the monolithic pipeline into cleanly separated `Classify → Score → Compose → Distill → Deliver` abstractions.

## [0.5.4-rc5] - 2026-04-01

### Added
- **Transcript Persistence**: Implemented robust session transcript persistence (`src/store/transcript.rs`) ensuring state is saved atomically to disk to prevent work loss.
- **Pre-Compact Double-Guardrail**: Injected `CRITICAL` at the start and `REMINDER` at the end of the `PreCompact` hook snapshot to drastically improve instruction adherence for Sonnet 4.6+ models.
- **Session Telemetry & ROI**: Enhanced `SessionState` to auto-calculate estimated tokens saved and identify the top data-reducing command purely in-memory (<5ms).
- **Session CLI**: Added new `omni session` commands for resuming and inspecting session transcripts.

### Fixed
- **Dead Code Cleanup**: Activated unused path mapping functions (`src/paths.rs`) and cleared various compiler warnings by completely wiring up the core pipeline.
- **Formatting & Linting**: Cleaned up the repository, removed obsolete GitHub PR templates, and integrated robust error checks for session boundaries.

## [0.5.4-rc4] - 2026-03-25

### Added
- **`omni doctor --fix`**: New `--fix` flag to automatically resolve integration issues, creates missing config directory, reinstalls hooks, registers MCP server, trusts project filters, and renames invalid user filter files to `.bak`.

### Fixed
- **Example Filter Template**: Rewrote `filters/00_example.toml` from legacy `[[filters]]` array-of-tables format to the standard `[filters.name]` schema, eliminating the embedded filter parse error at startup.
- **Stats Column Overflow**: Truncated the "Command" column in `omni stats` to a maximum of 21 characters with `...` ellipsis to prevent table layout breakage from long command names.

### Improved
- **Clippy Compliance**: Collapsed nested `if` statements in `doctor.rs` to satisfy `clippy::collapsible_if` lint.
- **Code Formatting**: Applied `cargo fmt` across all modified files for consistent style.

## [0.5.4-rc3] - 2026-03-25

### Added
- **Signal Comparison Mode**: Introduced `omni diff` command for side-by-side visualization of raw input vs. distilled output with "density gain" metrics.
- **Rewind Management**: Added `omni rewind list` and `omni rewind show <hash>` for local exploration of the RewindStore archive.
- **Real-time ROI Indicator**: New `[OMNI Active]` terminal status line providing immediate feedback on token reduction and latency.
- **Marketing Data Seeding**: New `scripts/seed_marketing.py` for generating high-impact, realistic demonstration data.
  *(Corrected 2026-08-23: this file never reached the repository. A blanket `*.py`
  ignore rule swallowed it, so no release has ever contained it. See #651.)*

### Improved
- **Analytics UI**: Refined `omni stats` with professional English headers, better alignment, and improved financial impact estimation.
- **Log Classification**: Enhanced `RE_LOG_SEV` to recognize common bracket-less severity formats (e.g., `DEBUG:`).
- **Aesthetics**: Updated distillation and retrieval notices with rich ANSI colors and detailed impact summaries.

## [0.5.4-rc2] - 2026-03-25

### Improved
- **Version Awareness**: `omni doctor` and `omni update` now explicitly distinguish between `[LATEST]`, `[UPDATE]`, and `[AHEAD/RC]` statuses.
- **Diagnostic Precision**: Updated `omni doctor` to provide more accurate version status for users on pre-release or development branches.

### Fixed
- **Version Checker**: Corrected semantic comparison in `is_newer` to properly handle pre-release suffixes (e.g., `0.5.4-rc1` is now recognized as newer than `0.5.3`).
- **Release Script**: Updated `bump_version.sh` to support Semantic Versioning with pre-release tags (e.g., `-rc1`).

## [0.5.4-rc1] - 2026-03-25

### Added
- **Filter Priority System**: Introduced alphabetical sorting for built-in filters (e.g., `00_vitest.toml` vs `npm.toml`) to ensure specialized matches take precedence.
- **Enhanced `omni exec`**:
    - Intelligent Shell Detection: Automatically detects and runs commands with pipes, redirects, or semicolons via `sh -c`.
    - Real-time Distillation: Native command output is now seamlessly piped through OMNI's semantic engine.
    - Exit Code Passthrough: Native exit codes are now correctly preserved and returned to the caller.
- **Deep Terraform Support**: Expanded Terraform filters with over 40+ new specialized rules for cleaner infrastructure distillation.

### Improved
- **Filter Precision**: Refactored Vitest and Kubectl filters for higher signal-to-noise ratios.
- **Session Tracking**: Enhanced stability in session state persistence and rule application.

### Fixed
- **Hook Reliability**: Resolved edge cases in `PreToolUse` hook handling for more consistent distillation.

## [0.5.3] - 2026-03-25

### Added
- `omni update` command: Easily upgrade OMNI to the latest version via Homebrew with a confirmation prompt.
- Automated Version Check: OMNI now checks for updates from GitHub (24h cached) and notifies you in `help` and `doctor` screens.
- Safety Confirmations: Added `[y/N]` interactive prompts to `omni reset` and `omni update` to prevent accidental uninstalls or upgrades.
- Full Hook Diagnostics: `omni doctor` now explicitly checks and displays status for all 4 OMNI hooks, including `PreToolUse`.

### Fixed
- Hook Cleanup: `omni reset` and `omni init --uninstall` now correctly remove `PreToolUse` (Bash) hooks from Claude settings.
- Hook Detection: Fixed `omni doctor` logic to correctly identify OMNI hooks using any valid flag variant.
- Clippy Compliance: Resolved `collapsible-if` and other minor lints in the new update module.

### Improved
- CLI Diagnostics: Refined `omni doctor` output with clearer labels ("OMNI Hooks", "OMNI MCP Server") for better readability.

## [0.5.2] - 2026-03-25

### Added
- Native support for `npm run`, `yarn run`, `pnpm run`, and `bun run` scripts in TOML filters.
- Support for `python -m pytest` and `python3 -m pytest` commands.
- Support for `bun test` and `bun run test` runners.

### Improved
- Context Safety: Enhanced preservation of multi-line test failure diffs (Vitest/Jest) by refining empty-line stripping rules.
- Accuracy: Improved token savings calculations in `omni stats` for more precise analytics.

### Fixed
- Clippy Compliance: Resolved all remaining `D warnings` including `implicit-saturating-sub` in distillation hooks.
- Filter coverage gaps: Fixed missing interceptions for common JS/Python test runner variants.

## [0.5.1] - 2026-03-24

### Added
- `omni reset` command: Safely backs up configs to `~/.omni.<ts>.bak` and removes agent integrations (MCP/Hooks).
- Automated Release Workflow: `make release` now handles version bumping, commits, and tagging in one command.

### Fixed
- `omni learn` stability: Resolved stdin "hanging" when run interactively and fixed TOML parsing errors.
- Noise Deduplication: `omni learn` now skips patterns already present in `learned.toml`.
- TOML Generation: Improved escaping for quotes and invisible ANSI control characters in generated filters.
- Project-scoped MCP Detection: `omni doctor` now correctly identifies and validates nested project keys in `~/.claude.json`.

### Improved
- Actionable Suggestions: `omni doctor` now provides direct CLI commands to fix identified issues.
- Latency Assertions: Added deterministic tests to verify sub-50ms distillation performance.
- Clippy Compliance: Resolved all nesting and code quality warnings across the codebase.

## [0.5.0] - 2026-03-23

### Changed: Breaking
- Full rewrite in Rust, zero Node.js, zero Zig runtime
- `omni monitor` renamed to `omni stats`
- Hook format changed, run `omni init --hook` to reinstall

### Added
- Session continuity via SessionStart + PreCompact hooks
- RewindStore: compressed content retrievable via `omni_retrieve(hash)`
- Session-aware distillation: hot files and active errors boost signal priority
- `omni doctor`, installation diagnostics
- `omni learn`, auto-generate TOML filters from passthrough output
- Rust edition 2024
- SQLite WAL mode + FTS5 for session search

### Fixed
- Never Drop: output never silently discarded (RewindStore replaces passthrough)
- Zero startup overhead: native binary vs Node.js V8 startup

## [0.4.5] - 2026-03-20

### Added
- **Codex CLI & OpenCode AI Integration**: Native support for top-tier AI agent platforms. Run `omni generate codex` or `omni generate opencode` to automatically register OMNI and inject specialized filter bundles for each ecosystem.
- **Extensive Polyglot Filters**: Introduced over 60+ new semantic filters covering:
  - **Node/TS**: npm, yarn, pnpm, bun, tsc, eslint, prettier, vitest, jest, cypress, playwright, next.js, vite, webpack, nx.
  - **Python**: pytest, ruff, mypy, black, isort, pip, poetry.
  - **Rust/Go/Zig**: cargo, rustfmt, clippy, go build/test, zig build/test.
  - **DevOps/Cloud**: docker, docker-compose, kubectl, terraform, terragrunt, helm, ansible, skaffold, argocd.
  - **Security**: semgrep, trivy, gitleaks, snyk, hadolint, kubesec.
  - **Mobile/Other**: flutter, react-native, android-build, composer, gradle, make.
- **Hook Integrity Verification**: Implemented SHA256-based verification for OMNI hook scripts with `omni_trust_hooks` command and automatic startup checks to prevent execution of untrusted and potentially malicious scripts.
- **Project Trust Boundary**: Secure local configuration loading via `omni_trust` command. Review and trust project-specific `omni_config.json` rules before they are applied.
- **Autonomous Discovery**: Experimental `omni_learn` tool (via Wasm `discover` export) to automatically identify and suggest filters for repetitive noise patterns.
- **Improved Filter Transparency**: Filter names are now exposed via WASM and logged in real-time in the TypeScript MCP server for better diagnostics and efficiency monitoring.
- **Test Suite Migration**: Migrated core and filter tests from JavaScript to TypeScript using Bun, adding 50+ new ecosystem fixtures for robust verification.

### Fixed
- **MCP Server Stability**: Isolated MCP server tests using temporary home directories to prevent interference with local user configurations.
- **Cat Filter Scoring**: Adjusted confidence scoring for structured markdown to assign lower confidence to short, single-line noise without headers.

### Changed
- **CLI References**: Extensively updated `docs/CLI_REFERENCE.md` and `README.md` to reflect the latest command capabilities and security features.
- **Streamlined Workflow**: Simplified the `CONTRIBUTING.md` pull request process to focus on automated `make verify` checks.

## [0.4.4] - 2026-03-19

### Added
- **Test Infrastructure**: Implemented a comprehensive test suite in the `tests/` directory covering core filters (Git, Docker, SQL, Node) and the MCP server gateway, supported by new test helpers and fixtures.
- **CI/CD Integration**: Fully wired the semantic verification suite (`test-semantic.mjs`) and unit tests into both the `Makefile` and GitHub Actions workflow for automated quality gating.

### Fixed
- **Shell Injection**: Switched to `execFileAsync` with array arguments for `omni_grep_search` and `omni_find_by_name` to prevent shell injection vulnerabilities.
- **Wasm Memory Leak**: Wrapped the Wasm engine compression logic in `try/finally` blocks to ensure allocated memory is always freed, even on errors.
- **SQL Parsing**: Refactored `sql.zig` to use line-based splitting (`std.mem.splitAny`) instead of space-based, and fixed a bug where `--` comments caused the entire distillation to break.
- **Docker False Positive**: Hardened `docker.zig` matching logic to require specific signals like `FROM `, `RUN `, or `COPY ` alongside `Step ` or `CACHED` indicators.
- **Dynamic Scoring**: Replaced hardcoded `1.0` scores in `git`, `docker`, `sql`, and `node` filters with dynamic signal-density calculations for better distillation accuracy.
- **MCP Exit Codes**: Modified `omni_execute` and its aliases to return the actual command exit code in the tool's response metadata for programmatic handling.

## [0.4.3] - 2026-03-19

### Changed
- **Version bump**: Synchronized version strings across all 9 manifest and source files.


## [0.4.2] - 2026-03-18

### Added
- **OMNI Design System**: New shared UI architecture (`ui.zig`) for perfectly aligned boxed layouts and high-resolution performance meters across all CLI subcommands.
- **Agent Autopilot Aliases**: Automatic interception of native agent tools (`Bash`, `run_command`, `ReadFile`, `view_file`) via MCP to ensure transparent token distillation.
- **Custom DSL Rules**: Activated and fully integrated custom token-reduction DSL rules in the main semantic engine.

### Fixed
- **DSL Engine Stability**: Fixed a critical `use-after-free` segmentation fault in the JSON config parser by explicitly allocating memory for config string slices.
- **Filter Precedence**: Ensured user-defined rules from `omni_config.json` correctly take priority over built-in internal core filters.
- **CLI Output Cleanliness**: Removed stray debug prints in the compressor pipeline.

## [0.4.1] - 2026-03-17

### Added
- **`omni examples`**: Display real-world study cases and examples.
- **Proxy Command (`--`)**: Proxy and distill output from other commands (e.g., `omni -- git log`).
- **Antigravity Filter**: Integrated filter for Google Antigravity AI agent.
- **MCP Tools**: Implemented file system exploration and declarative filtering tools.

## [0.4.0] - 2026-03-16

### Added
- **`omni update`**: Check for the latest release from GitHub and get smart update instructions (auto-detects Homebrew vs installer).
- **New Landing Page**: Introduced a redesigned OMNI landing page.
- **FUNDING**: Added `FUNDING.yml`.

### Fixed
- **Homebrew Upgrade Stability**: `omni setup` now uses stable `/opt/omni` paths instead of versioned `/Cellar/omni/X.X.X` paths, preventing broken symlinks after `brew upgrade`.
- **Self-referencing Symlink**: `omni setup` now skips symlinking when source and destination are the same path.
- **Dynamic Versioning**: `build.zig` now defaults to the current release version instead of "development" when `-Dversion` is not specified.

### Changed
- **Release script**: Now synchronizes **9 locations** (added `core/build.zig` default version).
- Simplified `.github/pull_request_template.md` to checklist-only format.

## [0.3.9] - 2026-03-16

### Added
- **`omni uninstall`**: Clean removal of `~/.omni` directory and automatic cleanup of MCP configs from Antigravity, Claude Code CLI, and Claude Desktop.
- **Custom DSL Rules**: Activated and fully integrated custom token-reduction DSL rules configurable via `omni_config.json`.
- **Semantic Confidence Scoring**: Dynamic compression strategies based on filter confidence.
- **Agent Autopilot**: Dedicated UI and documentation to guide AI agent integration.
- **AI PR Describer**: Added `.github/workflows/ai-pr-describer.yml` for automated pull request descriptions.

### Fixed
- **DSL Engine Stability**: Fixed a critical `use-after-free` segmentation fault in the JSON config parser by explicitly allocating memory for config string slices.
- **Filter Precedence**: Ensured user-defined rules from `omni_config.json` correctly take priority over built-in internal core filters.
- **CLI Output Cleanliness**: Removed stray debug prints in the compressor pipeline.

## [0.3.8] - 2026-03-16

### Fixed
- **Version Synchronization**: All 8 versioned files now fully synchronized (`package.json`, `package-lock.json`, `core/build.zig.zon`, `src/index.ts`, `src/index.js`, `scripts/omni-deploy-edge.sh`, `docs/index.html`, `omni.rb`).
- **Release Automation**: `omni-release.sh` updated to handle docs and deploy script versioning.

## [0.3.7] - 2026-03-16

### Added
- **Local Metrics System**: Every `omni distill` and MCP call now records usage to `~/.omni/metrics.csv`.
- **Expanded `omni report`**: Daily, Weekly, and Monthly breakdown tables with token savings (Cmds, Input, Output, Saved, Save%, Time).
- **Agent Filtering**: `omni report --agent=claude-code` to view per-agent metrics.
- **Agent Tagging**: `omni generate` now includes `--agent=<name>` in MCP config for automatic tracking.
- **PR Template**: Added `.github/pull_request_template.md`.

### Fixed
- **`omni setup` symlink**: Now searches 4 candidate paths for `index.js` and removes stale symlinks before creating new ones.
- **Installer (`install.sh`)**: Fixed color formatting (`%b`), version passing (`-Dversion`), and quoting issues.
- **Homebrew formula**: Replaced `post_install` with `caveats` to avoid sandbox issues with `$HOME`.

### Changed
- **Release script**: `omni-release.sh` now auto-bumps `build.zig.zon` and `package.json` versions.
- Removed `ARCHITECTURE.md` link from `CONTRIBUTING.md` and `docs/index.html`.

## [0.2.0] - 2026-03-15

### Added
- **Unified Native CLI**: Replaced shell scripts with high-performance native subcommands.
- Subcommands: `omni distill`, `omni density`, `omni report`, `omni bench`, `omni generate`, `omni setup`.
- **Agent Templates**: Support for generating Antigravity and Claude Code input templates.
- **Zig Build System**: Fully integrated `build.zig` for cross-platform native and Wasm builds.

### Changed
- Moved all legacy shell scripts to `scripts/legacy/`.
- Updated `install.sh` to use the native build pipeline.

## [0.1.3] - 2026-03-15

### Fixed
- Zig 0.15.2 IO API transition: Replaced removed `std.io.getStdOut/getStdIn` with `std.fs.File` equivalents.
- Native build failure on Homebrew environment.

## [0.1.2] - 2026-03-15

## [0.1.1] - 2026-03-15

## [0.1.0] - 2026-03-14

### Added
- Initial Zig core engine implementation.
- Basic Git and Build log filters.
- MCP Server gateway in TypeScript.
- Custom JSON-based rules for masking/removal.

---
*Follow the OMNI vision.*
