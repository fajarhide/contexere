# Security Policy

## Supported Versions

Fixes go into the next release off `main`. Only the current minor line gets them.

| Version | Supported          |
| ------- | ------------------ |
| 0.8.x   | :white_check_mark: |
| 0.7.x   | :x:                |
| 0.6.x   | :x:                |
| 0.5.x   | :x:                |

## Reporting a Vulnerability

**Do not open a public issue.** Contexere runs in the hook path of every command a
developer types, so a report needs an embargo until there is a release to upgrade
to.

1. **[Report a vulnerability privately](https://github.com/fajarhide/contexere/security/advisories/new)**.
   GitHub's private reporting is enabled on this repository, and the thread stays
   between you and the maintainer.
2. **Email** [security@weekndlabs.com](mailto:security@weekndlabs.com) if you would
   rather not use GitHub.

Please include a description, steps to reproduce, and what an attacker gets.

You will get an acknowledgement within 48 hours and a timeline with it.

## Security Considerations

- **Local-only processing**: Contexere processes all data locally. No data is sent to external servers during distillation.
- **Local SQLite Persistence**: Usage stats and archived contexts are stored locally in the SQLite database `~/.contexere/contexere.db` by default. **No data ever leaves your machine.**
- **MCP Server**: The MCP server runs locally via `stdio` transport and does not expose any network ports.
- **`contexere update`**: Only reads the public GitHub Releases API (no authentication required) to download the latest binary. No data is uploaded.

## Verifying a Release

Every release archive carries a signed statement of which workflow built it and
from which commit. `SHA256SUMS` only proves a file has not changed since it was
published; provenance is what says who published it.

```bash
gh attestation verify contexere-v0.8.6-aarch64-apple-darwin.tar.gz --repo fajarhide/contexere
```

Releases from 0.7.7 onward are attested. Anything older has `SHA256SUMS` only.

---

## 1. Nothing in a repository configures Contexere

Contexere reads its settings from one file, `config.toml` in its config home (`~/.contexere` by default, or the directory a config-home or `XDG_CONFIG_HOME` variable names), and from its own environment variables. It loads no configuration, filter or rule from the project it runs in, so cloning a repository cannot change what Contexere hides from your agent.

Earlier versions documented a `contexere trust` command and a per-project trust registry here. Neither exists: the project-local filter layer they guarded was removed in #449 and #505 (#1169).

---

## 2. Sandbox Environment Denylist

Contexere **strips the dangerous environment variables listed in `guard/env.rs`** from child processes it manages (e.g., when routing commands through `contexere exec`). This prevents environment-based attacks where malicious env vars could hijack command execution.

### Why This Matters

Some environment variables can inject code into any process that reads them:

| Variable | Risk |
| :--- | :--- |
| `BASH_ENV` / `ENV` | Shell runs this file **before** executing any command |
| `NODE_OPTIONS` | Injects flags/code into every Node.js process |
| `LD_PRELOAD` | Loads a shared library into **every** process (Linux) |
| `DYLD_INSERT_LIBRARIES` | Same as `LD_PRELOAD` (macOS) |
| `PYTHONSTARTUP` | Python executes this file on startup |
| `JAVA_TOOL_OPTIONS` | Injects JVM arguments into every Java process |

### What Contexere Blocks

All commands that are wrapped by Contexere (e.g., `contexere exec <cmd>`) receive a **sanitized** copy of `process.env` with these categories removed:

- **Shell injection**: `BASH_ENV`, `ENV`, `ZDOTDIR`, `BASH_PROFILE`, `PROMPT_COMMAND`, `IFS`, etc.
- **Runtime hijacking**: `NODE_OPTIONS`, `PYTHONSTARTUP`, `RUBYOPT`, `JAVA_TOOL_OPTIONS`
- **Dynamic linker**: `LD_PRELOAD`, `LD_LIBRARY_PATH`, `DYLD_INSERT_LIBRARIES`, `DYLD_FORCE_FLAT_NAMESPACE`
- **Path manipulation**: `PYTHONPATH`, `PYTHONHOME`, `RUBYLIB`
- **Git injection**: `GIT_ASKPASS`, `GIT_EXEC_PATH`, `GIT_TEMPLATE_DIR`

> [!NOTE]
> This is transparent. You don't need to configure anything: Contexere sanitizes the environment automatically to protect command executions.

---

Thank you for helping keep Contexere secure!
