# Step-One

Step-One is a desktop security sandbox for agent applications. It is designed
for people who want to run Claude Code, Codex, dsh and MCP tools without
copying API keys or passwords into a sandbox configuration file.

The project is security-first: new workspaces deny network access by default,
limit writes to `./workspace`, and accept secrets only as launch-time
references such as `env:ANTHROPIC_API_KEY` or `file:~/.config/step-one/token`.
The hardened executor is selected by the core runtime; the CLI keeps a small
fallback runner for local development.

## Quick start

```bash
python -m pip install -e .
step-one create ./demo
step-one doctor --config ./demo/sandbox.toml
step-one policy --config ./demo/sandbox.toml
step-one run --config ./demo/sandbox.toml -- echo "hello from the sandbox"
```

Edit `sandbox.toml` to choose the agents you use. Keep values out of the file:

```toml
[secret_refs]
anthropic_api_key = "env:ANTHROPIC_API_KEY"
```

Then export the secret in the host session before launching an agent. Step-One
passes it to the adapter at runtime and never writes it to the workspace.

## Commands

* `create [PATH]` creates a starter workspace and a safe policy.
* `doctor -c PATH` checks configuration and optional local agent binaries.
* `policy -c PATH` prints the effective policy; use `--json` for integrations.
* `run -c PATH -- COMMAND ...` launches a command using the configured write
  directory and a minimal inherited environment.

## Development

```bash
python -m pytest
```

The repository keeps baseline and incremental commits so changes can be
reviewed as pull requests. See `docs/security.md` for the threat model and
the boundaries of the current MVP.

