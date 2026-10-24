# Security model

Step-One treats the sandbox as a boundary around an agent process. The default
profile has no network access, no read paths outside the workspace, and a
single writable directory. A future desktop backend can enforce those rules
with OS-level namespaces, containers, or a VM without changing the user-facing
configuration.

Secrets are capabilities, not configuration values. A config may point to an
environment variable or a host file, but validation rejects values under keys
that look like API keys, passwords, tokens, or credentials. Resolution happens
only immediately before process launch. The CLI creates a clean environment
and never serializes resolved values.

This MVP deliberately reports optional tools as “not found” rather than
installing them. Agent adapters own each tool's command-line details, while
the policy layer remains shared. Before production use, the executor should
add OS enforcement, audit logs with secret redaction, and explicit network
allowlists.

