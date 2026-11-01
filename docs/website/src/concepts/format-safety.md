# What it refuses to touch

Before anything else runs, the payload is classified. If it looks like something a
later step is going to parse, the whole pipeline stands down and the bytes come back
exactly as they arrived.

Four kinds are recognised: **JSON**, **YAML**, **CSV** and **TSV**. Recognising any
of them ends the matter.

One exception, on the hook path: a secret. A value under a key such as `password` is replaced in
place, and a CSV or TSV column whose header names a secret is hidden. A quoted or uneven row in
such a table is hidden whole. `contexere exec` and pipe mode hand structured output back unchanged.

This is the stage people mistake for a failure. `kubectl get pods -o json` coming back
at full length is not Contexere missing an opportunity, it is Contexere declining one.

## Why declining is the right answer

A distilled JSON document is not a smaller JSON document. It is a broken one. The
`jq` two steps later fails, the agent reads the failure, and the cost of that round
trip is larger than anything the compression could have saved.

So the gate is deliberately biased. Bracketed but unparseable input, truncated JSON,
JSON carrying comments: all treated as structured. Compression cannot repair a
malformed payload but it can certainly make it worse.

## How it decides, and where it has been wrong

**JSON**: a whole document that parses. Above a size threshold a full `serde_json`
parse would blow the latency budget, so bracket shape alone decides. Free text almost
never carries `"key":`, which is the cheap signal for the ambiguous cases.

**YAML**: key-shaped lines, plus one rule that exists because of a real failure.
A block scalar (`config.hcl: |`) hands the rest of the block to whatever the value
happens to be: Vault HCL, a shell script, a PEM certificate. Those lines carry no
`key:` and are not YAML-shaped, so a naive sniff calls them prose. One embedded
ConfigMap sank a whole 608-line `kubectl kustomize` manifest that way: the sniff said
"not YAML", the gate stood down, and the manifest went down the lossy path. Lines
introduced by a block indicator are now skipped rather than judged.

**CSV and TSV**: a consistent delimiter count across a minimum number of rows. One
row proves nothing.

## Turning it off, and when to

```sh
OMNI_PASSTHROUGH=1 <your command>
```

Skips the pipeline entirely. Use it when you are debugging Contexere itself and need to
see what a command really printed, or when reading a file whose exact bytes matter.

The prefix works on every path, including inside an agent, but not for the reason it
looks like. A hook is a separate process the host spawned, so it inherits the host's
environment and never sees a variable you assign in front of a command. What it does
see is the command string, so Contexere reads the assignment there. Two consequences worth
knowing: only a **leading** assignment counts, the same position a shell would apply
it in, and `echo OMNI_PASSTHROUGH=1` mentions the name without setting anything and is
still distilled. Exporting it for the whole session works the ordinary way.

This is the single most useful environment variable here, and it is the first thing
to reach for when you suspect Contexere has changed something it should not have. If the
output is identical with and without it, Contexere was not involved.

## Things that look like this gate and are not

**Negative savings on small output.** A short payload can come back a few percent
larger, because the marker costs more than the compression saves. Expected, not a
defect.

**A command whose output arrives intact anyway.** Most calls are handed back untouched,
because taking anything would be unsafe or would not pay for its own marker. That is the
pipeline working.

**`kubectl` binary streams.** SPDY corrupts those with or without Contexere in the picture.

**Shell quoting.** Word splitting is your shell, not this program.
