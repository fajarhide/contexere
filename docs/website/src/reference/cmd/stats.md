# `omni stats`

Token savings analytics, read from your own database.

```sh
omni stats
```

Leads with the bytes that never reached your model, then one line per engine, each
percentage against its own base, then the command classes those bytes came from. The
aggregate below them mixes in every call OMNI deliberately declined, so it is a
diagnostic for one host's pipeline rather than a product claim.

Every view draws the same frame, `OMNI · <view> · <window>` between two rules, and
`--view context` carries no window because it reads the live session rather than a
period.

## Flags

| flag | effect |
|---|---|
| `--since <window>` | `hour`, `today`, `week`, `month` (default), `all` |
| `--view <name>` | `summary` (default), `detail`, `projects`, `context`, `rerun`, `folds`, `bill`, `share` |
| `--limit <n>` | Rows in a table view, default 10, `0` for all. Read by `detail`, `projects`, `rerun` and `bill`; a table it cuts says how many rows it hid |
| `--by <axis>` | `project`, `model` or `session`: the one spend table `--view bill` shows. Without it, model and project |
| `--json` | Machine readable, scoped by `--since` |
| `--card` | Write the summary as an image, sized for social posts |
| `--help`, `-h` | Help |

Every earlier spelling still resolves: `--detail`, `--today`, `--day`, `-d`, `--week`,
`-w`, `--month`, `-m`, `--hour`, `-H`, `--all-commands`, `--project`, `--context`,
`--rerun`, `--share` and `--view commands`. They are not listed because there is one way
to say each thing now, and they print no deprecation notice: the rename was ours, not
yours. `--view commands` is in that list rather than the table above because it renders
the detail view and always did.

`--json` and `--card` are output formats rather than views. `--card` outranks everything,
since naming it can only mean writing the file; `--json` outranks `--view`, since there is
one machine-readable report and it is not per view. Both used to be read as views, which is
how `--view detail --card` came to write no image.

## `--view folds` is the ledger's own calibration

A fold is a claim: the reader does not need these bytes. A retrieve on that marker's
handle is the reader disagreeing. This view puts the two side by side, per marker
shape, so the guard with the highest rate is the one to look at.

```sh
omni stats --view folds              # the last 30 days
omni stats --view calibration --since week
```

`calibration` is the same view under the word most readers reach for first.

There is no threshold and no verdict colour in it. No bar has been measured yet, and a
number that looks judged when nothing judged it is the defect this project exists to
fight. A store written before markers were recorded says so rather than printing a
confident zero.

## `--view bill` is the host's side of the books

Every other view reports what OMNI did, from OMNI's own tables. This one reads Claude
Code's transcripts, which the host writes and OMNI does not: billed tokens by class,
context bytes by source, tool result bytes by tool, and what that cost by model and by
project.

```sh
omni stats --view bill               # the last 30 days
omni stats --view bill --since week --limit 0
omni stats --view bill --by session  # which sessions cost the most
```

A request is counted once by its id, because the host repeats `usage` on every record
of a request.

Each request is priced from the model that served it, at the API list price on the date
the view prints. Cache writes are priced by their TTL (1.25x input for five minutes, 2x
for the hour), `speed: "fast"` at twice the rate, and Haiku 5.5 at its higher rate past
100K prompt tokens. A model the table does not know is shown as `unpriced` with its
tokens, never as $0, and the total says it is a lower bound. A subscription or a
negotiated rate bills differently, so read the figure as what the same work costs on the
API.

The spend tables count subagent requests, because they are billed. The token and context
tables leave them out, because a subagent's context is not the session's.

The last paragraph of the output is OMNI's own figure for the same window, labelled as
its claim, so the two sides sit on one screen without being mixed. `--json` does not
carry this view. It reads every transcript in the window, which takes seconds, and the
machine-readable report is meant to stay cheap.

## `--rerun` is the one to know

Reduction percentage cannot tell you whether a distiller removed something the agent
then had to fetch again. If it did, the reduction was a deferral, not a saving. This
flag is the check that percentage cannot make.

## Traps

**Terminal rows are not tokens.** Output written to a TTY is read by a human, not a
model. On one installation those rows were 73% of every byte OMNI claimed to have
saved. `stats` excludes them now, and so does the benchmark harness, but anyone
querying `~/.omni/omni.db` directly has to filter by `agent_id` themselves.

**A high number deserves suspicion.** The worst defects in this project reported the
highest reductions, because deleting the answer compresses very well. Pair any figure
with `omni diff` on a real command.

**A low aggregate is usually right, and it is not the number to judge OMNI by.** Most
calls are handed back untouched by design, so read the per-class rows to see where the
work actually happened.

**`--share` and `--card` cannot drift from the report.** Both read the same
aggregation as `omni stats` itself, which was a deliberate choice after an earlier
version computed them separately.
