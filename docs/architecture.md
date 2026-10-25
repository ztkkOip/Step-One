# Step-One architecture

Step-One is a Windows-first desktop workspace for running AI agents behind a visible, policy-driven security boundary.

## Current foundation

The repository starts with a Tauri 2 shell, a React control console, and a simulator runtime. The simulator makes the user flow testable without pretending that Hyper-V, credentials, network controls, or MCP authorization are already implemented.

```text
React desktop UI
      |
      | Tauri commands
      v
Rust control plane
      |
      +-- simulator runtime (current)
      +-- Hyper-V runtime (Issue #5)
      +-- credential broker (later issue)
      +-- policy engine (later issue)
      +-- MCP gateway (later issue)
```

## Trust boundary

- The React layer receives status and safe summaries. It never receives secret material.
- The Rust control plane owns the runtime lifecycle and will own keychain access in a later PR.
- A real sandbox must receive only explicitly selected workspace data.
- Host credentials, SSH material, browser profiles, and ambient environment variables are outside the workspace boundary.
- The simulator is a UI and lifecycle development tool. It is not a security boundary.

## Runtime contract

The UI depends on a small command surface:

- `console_snapshot`
- `create_agent`
- `agent_action`
- `request_approval`
- `decide_approval`
- `revoke_permission`
- `runtime_capabilities`

`runtime_capabilities` is a read-only host diagnostic. It reports whether the prerequisites for a future Hyper-V runtime are present and deliberately keeps the effective mode as `simulator` until VM lifecycle, workspace transfer, network isolation, credential handling and cleanup are validated together. The UI remains independent from VM tooling and can show failure states before virtualization is available.

## Planned security layers

1. Hyper-V Generation 2 Linux VM for process and kernel isolation.
2. Explicit workspace mount policy.
3. Egress proxy and domain policy for network access.
4. Host-side credential broker backed by the Windows credential store.
5. MCP gateway with tool-level permission checks and user confirmation for high-risk actions.
6. Host-only audit log with redaction.
