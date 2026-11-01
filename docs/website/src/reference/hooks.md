# Hooks

The entry points an agent host invokes. You never type these; `contexere init` writes them
into the host's configuration.

| entry point | when the host calls it |
|---|---|
| `contexere --pre-hook` | Before a tool runs |
| `contexere --post-hook` | After a tool produces output |
| `contexere --hook` | Universal dispatcher, for hosts with one hook slot |
| `contexere --session-start` | Session begins |
| `contexere --session-end` | Session ends |
| `contexere --pre-compact` | Before the host compacts the conversation |
| `contexere --mcp` | Run as an MCP server over stdio |
| `cmd \| omni` | Pipe mode, no host involved |

## One call, both hooks

![One Bash tool call passes through Contexere twice: the pre-hook may rewrite the command before the shell runs it, and the post-hook distills the output afterwards, reading the local database for lines already shown and archiving what it removes.](../media/hook-lifecycle.svg)

The shell runs whatever the pre-hook handed it and never knows Contexere exists. Only the
reply is rewritten, which is why nothing here can change what your command did.

## What each one does

**Pre-tool** refuses a command that would print a credential source. On Codex and Gemini
CLI it can also rewrite a command into `contexere exec`, wrapping the **entire** command
string, redirection included, so a matched command's log file on disk can turn out to be
the distilled version. Break the prefix (`env cargo test`) when you need the raw log.

Claude Code gets no rewrite. It checks permissions against the command it runs, so a
rewrite would need `allow`, which skips your prompt. The post-tool hook distills the same
output there.

**Post-tool** is the main event: the raw output arrives, the pipeline runs, and the
distilled result is handed back for the host to substitute.

**Post-tool-failure** exists because a failed command must pass through verbatim, and
hosts disagree wildly about how they say a command failed. Claude Code sends a plain
string, `Error: Exit code N`. Others carry structured error flags. Reading only one
shape is a bug this project has had.

**Session start** injects project context: hot files, the last active error, stored
knowledge, the pinned goal.

**Session end** writes the summary and can export CSV.

**Pre-compact** is the host's warning that the conversation is about to be shortened.

## Two doors into one pipeline

`post_tool` and `pipe` are separate entry points that run the same stages, and keeping
them in step has been a recurring source of bugs. Three separate fixes each corrected
one copy and left the other. The ledger stage existed in `post_tool` for a release
before `pipe` had it at all, so a command the pre-hook rewrote into `contexere exec` got
the filters and nothing else.

If you are changing pipeline behaviour, change both, or check why not.

## Why it never crashes your agent

Every hook runs inside `catch_unwind`, at the highest entry point. A panic in one
stage costs that distillation, not the session. A database that will not open costs
session context, not the pipeline.

That is the **fail open** rule, and it has one sharp edge worth stating: failing open
means handing back the raw bytes. It does not mean emitting a cheerful summary. A
distiller that parsed nothing returning `0 tests passed` is failing **closed**, and
confidently.

## What a host has to do for any of this to matter

Register the hook, and then honour what it returns.

The second half is not guaranteed. Contexere once emitted its distilled output under a key
Claude Code ignores, so nothing was applied on that path for two releases while Contexere
recorded a saving and printed a footer for each one. The fix corrected the key and
left the value shape wrong, and the symptom survived unnoticed.

Two things that taught, both non-obvious:

- The rewrite is validated against **the host tool's own output schema**, one shape per
  tool. There is no universal shape.
- The fields are independent. A rejected rewrite still lets the context message
  through, so the savings footer prints for a distillation that was reverted.

So the proof that a hook is working is not the footer and it is not `contexere stats`. It
is the host's own session transcript:

```sh
grep -c hook_error_during_execution ~/.claude/projects/<project>/<session>.jsonl
```

A warning you can see is not a warning the agent can see. Those attachments never
enter the model's context, so an agent can tell you the hook is fine while your
terminal fills with rejections.

## Testing a hook by hand

Feed it a payload directly rather than guessing which path ran:

```sh
echo '<host payload json>' | contexere --post-hook
```

`contexere exec` and the post-hook route differently, so a result from one is not evidence
about the other.

### The payload shape, which differs per tool

Getting this wrong fails silently and identically: the hook exits 0, prints nothing, and a
probe reads that as `0.0% saved`. There is no error to notice, so a distiller that is in
fact cutting 96% can be written off as not firing.

`Bash` puts the output at the top of `tool_response`:

```json
{ "session_id": "s1", "tool_name": "Bash",
  "tool_input":    { "command": "cat server.log" },
  "tool_response": { "content": "line one\nline two\n" } }
```

`Read` wraps it in `file`, and the extra keys are not decoration. `startLine` is what the
host counts `cat -n` numbering from, so a fold that removes lines above the survivors has
to move it:

```json
{ "session_id": "s1", "tool_name": "Read",
  "tool_input":    { "path": "notes.txt" },
  "tool_response": { "file": { "filePath": "notes.txt", "content": "...",
                               "startLine": 1, "numLines": 40, "totalLines": 400 } } }
```

The reply goes under `hookSpecificOutput.updatedToolOutput`, and Claude Code validates it
against **the host tool's own output schema**. A wrapped `Read` gets a `file` reply back,
which is the shape the host accepts.

A bare `tool_response.content` does **not** get a bare reply. Verified rather than assumed:
it comes back as `{status, result}`, which is Contexere's own shape and is what #187 was about.
So the bare form is fine for asking what the ledger did, and its reply is not what a real
`Read` would accept.

**Both `Read` shapes are real and they reach different stages.** A `Read` payload written
with a bare `tool_response.content` is accepted and reaches the ledger, while
`tool_response.file.content` reaches the `readfile` distiller and the `startLine`
adjustment. Neither is wrong; they answer different questions. A probe aimed at one and
built on the other returns a clean nothing and looks like a verdict.
