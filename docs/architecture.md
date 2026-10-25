# Architecture

Step-One uses a React control console, Tauri IPC and a standalone Rust control-plane crate. Both desktop and browser currently use explicit simulators. Neither simulator runs real Agents or enforces isolation.

## Interfaces

`console_snapshot` returns agents, approvals, audit events, operations and runtime capabilities. `choose_workspace` returns a host-issued grant ID and display path from the native folder picker. `create_agent` accepts name, provider, workspaceGrantId, securityMode and optional runtimeKind; arbitrary paths are not accepted. `agent_action`, `request_approval`, `decide_approval` and `revoke_permission` retain the approval workflow.

Agent process status is separate from runtimeState. operationId marks work in progress; lastError contains safe diagnostics. Unknown process state is not stopped. isolationEnforced is false for simulators, and permissions represent only simulated policy.

## Durability and failures

ControlService validates requests and commits operation/resource ownership before side effects. Runtime execution and failure reconciliation happen outside the state mutex. Conflicting operations on one Agent are rejected; snapshots and other Agents remain available. Failed cleanup retains ownership. Failed start is re-inspected; unknown status prevents destruction until a confirmed stop.

SQLite transactions persist non-secret state in the application's local data directory. A process-held file lease prevents concurrent instances from overwriting snapshots. Unsupported schemas and corrupt data fail closed. Restart expires old approvals and inspects resources rather than trusting saved running flags. If a commit fails after a runtime operation, the pending record is retained; restart the application to reconcile before continuing.

Runtime implementations must enforce bounded timeouts and process-tree cleanup. A waiting-thread timeout alone cannot cancel external operations.

## Workspace boundary

The native picker is the only desktop grant issuer. Creation, start and permission grants revalidate directory identity. Roots, home ancestors, credential/browser locations, links and Windows reparse points are denied. Selection alone grants no write permission. Private file identity remains in storage, outside IPC responses.

This is authorization groundwork, not filesystem isolation. No host directory is currently mounted. The future broker must use handle-relative file access and reject child reparse points and replacement races on every request. Database paths belong to the application, never to guests. No credentials are currently accessed.

## Remaining acceptance

Issue #5 covers real Hyper-V VM lifecycle, verified images, resource limits, controlled workspace transport and default-denied network. Issue #6 covers actual pinned Agent installations. Issue #2 tracks the Windows credential store, model proxy, OAuth and MCP gateway. Simulation and CI cannot prove VM isolation; hardware validation remains required.
