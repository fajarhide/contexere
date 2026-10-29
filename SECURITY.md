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

**Do not open a public issue.** OMNI runs in the hook path of every command a
developer types, so a report needs an embargo until there is a release to upgrade
to.

1. **[Report a vulnerability privately](https://github.com/fajarhide/omni/security/advisories/new)**.
   GitHub's private reporting is enabled on this repository, and the thread stays
   between you and the maintainer.
2. **Email** [security@weekndlabs.com](mailto:security@weekndlabs.com) if you would
   rather not use GitHub.

Please include a description, steps to reproduce, and what an attacker gets.

You will get an acknowledgement within 48 hours and a timeline with it.

## Security Considerations

- **Local-only processing**: OMNI processes all data locally. No data is sent to external servers during distillation.
- **Local SQLite Persistence**: Usage stats and archived contexts are stored locally in the SQLite database `~/.omni/omni.db`. **No data ever leaves your machine.**
- **MCP Server**: The MCP server runs locally via `stdio` transport and does not expose any network ports.
- **`omni update`**: Only reads the public GitHub Releases API (no authentication required) to download the latest binary. No data is uploaded.

## Verifying a Release

Every release archive carries a signed statement of which workflow built it and
from which commit. `SHA256SUMS` only proves a file has not changed since it was
published; provenance is what says who published it.

```bash
gh attestation verify omni-v0.7.7-aarch64-apple-darwin.tar.gz --repo fajarhide/omni
```

Releases from 0.7.7 onward are attested. Anything older has `SHA256SUMS` only.

---

## 1. Project Trust Boundary

OMNI will **not** load project-local configurations or custom TOML signals (inside `.omni/signals/`) until you explicitly trust the project. This prevents a malicious repository from injecting custom signal rules that could hide important output from your AI agent.

### How it Works

OMNI uses `omni_config.json` as the trust anchor for a repository. 

```
 Your Project/
 ├── omni_config.json   ← OMNI sees this but WON'T load signals unless trusted
 ├── .omni/signals/     ← Local custom rules
 └── ...

 ~/.omni/
 └── trusted-projects.json  ← Trust registry (path + SHA-256 hash)
```

1. OMNI detects project-local configurations.
2. It checks `~/.omni/trusted-projects.json` for the project path **and** a matching SHA-256 hash of the `omni_config.json` anchor file.
3. If not found or hash doesn't match → **local configs & signals are skipped**, OMNI logs a warning.
4. If trusted and hash matches → configs and local `.omni/signals/` are loaded normally.

### Quick Start

**Trust a project for the first time:**
```bash
omni trust
```
Or call the `omni_trust` MCP tool manually from Claude Code.

The tool will:
- Display the config contents for your review.
- Show the SHA-256 fingerprint.
- Add the project to `~/.omni/trusted-projects.json`.

**After editing your local config:**
```bash
omni trust
```
Run it again to re-verify and update the hash.

> [!IMPORTANT]
> If you modify `omni_config.json` after trusting, OMNI will **stop loading project signals** until you re-trust. This protects against silent repo tampering.

### Trust Flow

| Scenario | OMNI Behavior |
| :--- | :--- |
| No local config exists | Global and Built-in filters only (normal) |
| Local config exists, **not trusted** | Skipped. Logs: `⚠ Local config not trusted. Run omni trust to review and trust.` |
| Local config exists, **trusted** | Loaded and merged with global configs |
| Local config **modified** after trust | Skipped. Logs: `⚠ Local config modified since last trust. Run omni trust to re-verify.` |

---

## 2. Sandbox Environment Denylist

OMNI **strips ~25 dangerous environment variables** from child processes it manages (e.g., when routing commands through `omni exec`). This prevents environment-based attacks where malicious env vars could hijack command execution.

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

### What OMNI Blocks

All commands that are wrapped by OMNI (e.g., `omni exec <cmd>`) receive a **sanitized** copy of `process.env` with these categories removed:

- **Shell injection**: `BASH_ENV`, `ENV`, `ZDOTDIR`, `BASH_PROFILE`, `PROMPT_COMMAND`, `IFS`, etc.
- **Runtime hijacking**: `NODE_OPTIONS`, `PYTHONSTARTUP`, `RUBYOPT`, `JAVA_TOOL_OPTIONS`
- **Dynamic linker**: `LD_PRELOAD`, `LD_LIBRARY_PATH`, `DYLD_INSERT_LIBRARIES`, `DYLD_FORCE_FLAT_NAMESPACE`
- **Path manipulation**: `PYTHONPATH`, `PYTHONHOME`, `RUBYLIB`
- **Git injection**: `GIT_ASKPASS`, `GIT_EXEC_PATH`, `GIT_TEMPLATE_DIR`

> [!NOTE]
> This is transparent. You don't need to configure anything: OMNI sanitizes the environment automatically to protect command executions.

---

## Security Tools Summary

| Tool | Purpose | When to Use |
| :--- | :--- | :--- |
| `omni trust` | Trust a project's local configurations | After cloning a repo with custom filters, or after editing the config anchor |

---

Thank you for helping keep OMNI secure!
