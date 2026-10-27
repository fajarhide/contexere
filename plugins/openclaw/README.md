# VENG Semantic Signal Engine for OpenClaw

This plugin provides a secure bridge between your OpenClaw agent and the [OMNI Signal Engine](https://github.com/fajarhide/omni).

> [!IMPORTANT]
> **Dependency Required**: This plugin is a wrapper and requires the `veng` CLI binary to be installed on your local system path.

## Setup

## Prerequisites

- **VENG** must be installed and available in your PATH.
- **OpenClaw** (Gateway) must be installed.

### Installation

**via ClawHub (Recommended)**
```bash
openclaw plugins install clawhub:@fajarhide/veng-signal-engine
```

**Automatic Install (No Clone Required)**
VENG natively fetches the integration files securely from public GitHub.
```bash
veng doctor --fix
```

**Local Developer Install**
If you cloned the repository locally:
```bash
openclaw plugins install ./plugins/openclaw
```

## Configuration (Optional)

**VENG for OpenClaw is designed to be "Zero Config".** If `veng` is already in your `PATH`, it will work immediately after installation without any additional settings.

If you have a custom setup, you can modify your OpenClaw settings (`~/.openclaw/config.yaml`):

```yaml
plugins:
  veng-signal-engine:
    vengPath: "/usr/local/bin/veng"  # Optional: path to veng binary
```

## Usage

Once installed, your OpenClaw agent will have access to a new tool:

### `veng_cmd`
Use this exactly like the standard `shell` or `bash` tool.
- **Input**: `{ "command": "npm install" }`
- **Behavior**: Runs the command via `veng exec`, filtering out noise and keeping only the signal (errors, summaries).

## Monitoring Savings

You can track your token and cost savings by running:

```bash
veng stats --today
```

## Security & Privacy (Trust Model)

VENG is built with a **Privacy-First** design intent. This plugin acts as a secure proxy to facilitate safe execution:

- **Node-Level Sanitization**: This plugin explicitly strips ~25 dangerous environment variables (like `LD_PRELOAD`, `NODE_OPTIONS`, `BASH_ENV`) at the process level before execution. Other environment variables are passed through to the command.
- **Local-Only Architecture**: The VENG engine is designed for local processing. No terminal output is ever sent to external cloud services by the engine.
- **Local Persistence**: Usage statistics are stored strictly in a local SQLite database at `~/.veng/veng.db`.
- **Trust & Verification**: As VENG is a tool for developers, we encourage you to audit the full source code and security policies at the [OMNI GitHub Repository](https://github.com/fajarhide/omni/blob/main/SECURITY.md) to verify these claims for yourself.

> ⚠️ **Note**: This plugin passes through most environment variables to the invoked command. If you run untrusted commands, consider doing so in a restricted environment (container, CI runner, or limited account) with minimal secrets in the environment.

## Benefits
- **Cheaper Tasks**: Massive savings on API bills for long-running autonomous tasks.
- **Higher Accuracy**: Agents focus on the real errors instead of being distracted by 10,000 lines of build logs.
