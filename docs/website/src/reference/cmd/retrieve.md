# `veng retrieve`

Prints the content a marker archived.

```sh
veng retrieve <handle>
```

The handle is the 16 characters inside a marker:

```
[VENG: 406 lines omitted, veng retrieve 0000000000000000 for full output]
[VENG: 40 lines already shown, veng retrieve 0000000000000000]
```

It returns the original bytes. Not a summary, not a re-run of your command, and not an
approximation.

Works on every host, in any session, whether or not MCP is wired. Agents with the MCP
server registered call `veng_retrieve` instead and never have to ask you.

## What can go wrong

**The handle does not resolve.** The archive is a rolling 30 day window, so content
older than that is gone. Verbatim execution traces are pruned sooner still, at seven
days.

A handle that fails to resolve inside the window is a serious bug rather than an
inconvenience, because a marker promising retrievable content is the one thing this
mechanism cannot get wrong. Report it.

**You typed the marker text, not the handle.** Only the hex, no brackets, no prefix.

## Why it can promise this

A run is archived **before** its marker is written, and a failed archive leaves the
run verbatim rather than producing a marker. So a handle you can see is a handle whose
content exists. That ordering was a fix, not the original design: an earlier version
returned a key even when the write had failed.
