# Reading the markers

A marker is OMNI telling you what it did. There are only a few shapes, and knowing
them is the difference between trusting the tool and suspecting it.

## The shapes

```
[OMNI: 406 lines omitted, omni retrieve 0000000000000000 for full output]
```

Content was cut and archived. The 16 characters are a handle:
`omni retrieve <handle>` prints the original back, byte for byte, from any
shell in any session.

```
[OMNI: 40 lines already shown, omni retrieve 0000000000000000]
```

The ledger. These lines were emitted earlier **in this session**, so the claim is that
the agent is still holding them and the handle costs nothing unless it wants to
re-read.

```
[OMNI: 40 lines identical to an earlier run, omni retrieve 0000000000000000]
```

The ledger again, for the case where the identity is the answer. You ran the same
command a second time and it printed the same lines, so the marker says that rather
than "already shown": a poll re-run to find out whether a value moved is answered by
the value not having moved, and a re-read is answered by the file not having changed.

```
[OMNI: 40 lines not shown here, omni retrieve 0000000000000000]
```

Also the ledger, different claim. These lines went to a **different session** of this
project, and this agent has never seen them. The wording is deliberately not "already
shown", because that would be false. Folding them is a bet that the agent will not
need them, and it carries three times the profitability bar for that reason.

That other session may also have been a different agent. The project history is keyed
on the directory, so anything running in this repository contributes to it. See
[what two agents share](../concepts/the-ledger.md#what-two-agents-in-one-repo-share).

```
[OMNI: identical to the 40 lines already shown, omni retrieve 0000000000000000]
[OMNI: identical to 40 lines from an earlier session, none shown here, omni retrieve 0000000000000000]
```

The same two claims, for a reply that is repeated **in full**. When the fold covers
every line, the marker is the whole output rather than a gap inside it, so it says
`identical to` and you get one line where a re-run would have printed the same
hundreds. Anything less than the whole reply keeps the wording above.

```
[OMNI: 40 lines already shown, omni retrieve 0000000000000000]
⋮
⋮
```

In a file read, and only there, the marker is followed by one `⋮` per line it replaced.
Your editor numbers the lines it is handed, counting from the line the read began at, so a
shorter view would put every surviving line on a number it is not at. Keeping the count is
what lets a fold sit between two pieces of content you still need. The filler is never
retrievable content: `omni retrieve` on the handle above prints the real lines.

```
[OMNI: 40 lines already shown from charlie.tf, omni retrieve 0000000000000000]
```

Any of the four can carry `from <source>`, naming the command whose output first
showed those lines. It appears only when that command is **not** the one you just
ran, which is the case you cannot resolve from the marker alone: reading one file
and having a block elided because a different file showed it earlier. Without the
clause, comparing two files to check a shared block matches is answered by deleting
the evidence.

Running the same command again carries no clause, because the marker above already
says so in words. Marker length decides what is worth folding at all, so a source on
every marker would cost savings on the common case to label the rare one.

```
[N similar lines collapsed]
```

Collapse. A run of near-identical lines, replaced by a count.

```
[OMNI Active] ⏺ 93.7% reduction (2.3 KB → 147 B) 3ms
```

The footer, on `omni exec` and pipe mode. Input size, output size, and how long the
pipeline took.

```
[Partial signal]
```

The pipeline recognised some of the output but not all of it.

```
[OMNI: output truncated, 1200 of 4000 lines kept, 2800 dropped from the middle, omni retrieve 0000000000000000]
[OMNI: output truncated, 50000 of 180000 bytes kept]
```

The last cut before a reply leaves, at 50 KB. The line form keeps the start and the end,
because a build or a test run puts its verdict last, and says how much of the middle went.
The byte form is what you get when there is no line structure inside the budget, and it
keeps the start only, so a single enormous line loses its tail. The handle is there when
the dropped part was archived; without one, the marker still says what was removed.

```
[OMNI: 2 sensitive value(s) redacted]
```

Credential redaction on command output. A line that assigns a value to a sensitive name
had that value replaced with `[REDACTED]`, and this counts the values. Quotes and the
punctuation around the value stay, so a redacted line still parses. The assignment can sit
mid-line after `export` or a shell operator, as in a crontab entry
`cd /app && export DB_PASSWORD="..."`.

A token that carries its issuer's prefix is hidden with no name beside it, at the length
the issuer writes it: `sk-ant-`, `ghp_` and its siblings, `github_pat_`, `AKIA`, `xoxb-` and
its siblings. A shorter string with the same prefix, such as `sk-ant-abc123` in a document,
is left alone.

Redaction runs on output OMNI rewrites. A command the host reports as failed is handed
back untouched, and so is a payload read as structured, so a credential in either is
delivered as printed. A host that reports no failure, such as OpenCode or Continue, has
its output rewritten whether the command failed or not.

The name decides, matched per underscore-separated word so `PASSED` and `AUTHORS` are not
caught: `SECRET`, `TOKEN`, `PASSWORD`, `PASSWD`, `PASS`, `AUTH`, `CREDS`, `CREDENTIAL`,
`CREDENTIALS`, `DATABASE_URL`, `REDIS_URL`, `MONGO_URL`, `CLIENT_SECRET`, `ACCESS_KEY`,
`PRIVATE_KEY`, and anything starting `API_`, `AWS_`, `GITHUB_`, `ANTHROPIC_`, `OPENAI_`
or `GEMINI_`. Some values are left alone because they hold no credential
whatever the name: an empty value, a shell expansion like `$DB_PASSWORD`, an unquoted call
or `null`.

`KEY` on its own is the weak pattern, since `key=` is ordinary code. Under it the value has
a say too, and a short lowercase identifier or a `{` expression passes. Under every other
pattern the value never argues, so `hunter2` and `decrypt("ghp_x")` are cut. When in doubt
the value is cut: hiding a harmless value costs a re-read, and printing a real one cannot
be undone.

Redaction protects what the agent receives, not your disk. OMNI's local trace store keeps
the raw output, secret included, for seven days.

```
[OMNI: Re-injecting critical files due to Warning pressure]
```

When the context is under pressure (`Warning` or `Critical`), OMNI periodically puts up to
three of your `pinned_files` back in front of the agent, each cut to about 400 characters.
Nothing was removed; what follows the line is added.

## Reading a percentage correctly

The worst bugs in this project's history reported the **highest** reductions. A
distiller that deletes the answer compresses beautifully.

So a large number is not on its own good news. `omni diff` is the check:

```sh
omni diff     # the last command, raw against distilled
```

If a 99% saving turns out to have removed the file paths that were the answer, that is
a bug worth reporting, and it is the exact class this project cares about most.

## When there is no marker at all

Most of the time, and that is the pipeline working rather than failing. OMNI hands the
output straight back whenever taking anything would be unsafe or would not pay:

- The payload is JSON, YAML, CSV or TSV. Never touched, on purpose.
- The command failed. A non-zero exit passes through verbatim.
- There was no noise to remove. A `kubectl get pods` table is an enumeration where
  every row is a datum.
- The output was too short to be worth a marker.

## Getting content back

```sh
omni retrieve <handle>
```

Works on every host, with or without MCP. Agents with the MCP server wired can call
`omni_retrieve` themselves without asking you.

One boundary a handle cannot promise: the archive is a rolling 30 day window, so
`omni retrieve` on content older than that will not resolve. Verbatim traces are
shorter still at seven days.

## Telling a real marker from a printed one

Markers appear in prose too. This page is full of them, so is OMNI's source, and so
is any bug report that quotes one. That matters if you are measuring whether OMNI
was active on a run, because searching a transcript for the marker shape will find
the examples as readily as the folds.

The handle is what separates them. Every worked example in this manual and in OMNI's
own source uses one reserved value, `0000000000000000`, which no real fold can ever
be assigned:

```sh
omni retrieve 0000000000000000   # exit 1, "the documentation example"
omni retrieve <handle-you-found> # exit 0 if OMNI really folded it
```

So the exit code answers the question, and a marker copied out of documentation
cannot be mistaken for evidence that anything was shortened.
