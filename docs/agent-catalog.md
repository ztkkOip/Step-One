# Agent catalog evidence (2026-10-25)

This is a version-pinned installation and launch plan, not a claim that the agents have been installed, run, or isolated. No CLI was installed or executed during this research, and no model request was sent. The first supported target is a Linux x86_64 guest using glibc. Native binaries may use musl internally (Codex), but the entire guest support claim remains Linux/glibc until execution tests exist.

## Codex 0.164.0

Primary sources:

- [Official configuration reference](https://developers.openai.com/codex/config-reference/) (currently redirects to the official [configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference)). Fetched and read the provider fields.
- [Official npm wrapper metadata](https://registry.npmjs.org/@openai%2fcodex/0.164.0).
- [Official Linux x64 native metadata](https://registry.npmjs.org/@openai%2fcodex/0.164.0-linux-x64).
- [Wrapper archive](https://registry.npmjs.org/@openai/codex/-/codex-0.164.0.tgz), downloaded in memory, SHA-512 verified, and inspected without execution.

The configuration reference documents `base_url`, `wire_api = "responses"`, and `requires_openai_auth` (default false) for custom providers. It also explains that provider/auth configuration belongs in user-level config rather than the project's `.codex/config.toml`. Use a fresh guest-only `CODEX_HOME`, set the host-broker provider and explicit model, and do not set `env_key`, `auth`, `experimental_bearer_token`, or any real API key.

Wrapper metadata requires Node >=16. Its launcher resolves the platform alias `@openai/codex-linux-x64` to the versioned package `@openai/codex@0.164.0-linux-x64`; the expected native entry is `vendor/x86_64-unknown-linux-musl/bin/codex`. Preserve the native vendor tree if using direct extraction. Native archive contents and a real executable version check remain unverified because the native archive was not downloaded.

The wrapper digest alone is insufficient: both wrapper and native SHA-512 SRI values are locked in `agent-catalog.json`.

## Claude Code 2.1.310

Primary sources:

- [Official npm wrapper metadata](https://registry.npmjs.org/@anthropic-ai%2fclaude-code/2.1.310).
- [Official Linux x64/glibc package metadata](https://registry.npmjs.org/@anthropic-ai%2fclaude-code-linux-x64/2.1.310).
- [Wrapper archive](https://registry.npmjs.org/@anthropic-ai/claude-code/-/claude-code-2.1.310.tgz), downloaded in memory, SHA-512 verified, and inspected without execution.
- [Connect to a gateway](https://code.claude.com/docs/en/llm-gateway-connect), fetched through the official legacy-domain redirect `https://docs.anthropic.com/en/docs/claude-code/llm-gateway-connect`.
- [Other LLM gateways](https://code.claude.com/docs/en/llm-gateway), fetched through the equivalent legacy-domain redirect.
- [Managed settings](https://code.claude.com/docs/en/managed-settings) and [environment variables](https://code.claude.com/docs/en/env-vars), fetched through their equivalent legacy-domain redirects.

The wrapper requires Node >=22.0.0. Its `bin/claude.exe` name does not imply a Windows-only package: `install.cjs` selects the matching optional native package, resolves its `claude` binary on Linux, and copies/hardlinks it into the common destination. Unix ignores the `.exe` extension. A controlled guest installer can verify and extract the native package directly instead of running npm lifecycle scripts; the expected native archive member `package/claude` is inferred from that wrapper code and must be checked against the native archive before installation.

The official gateway guide requires a gateway credential variable and describes `ANTHROPIC_AUTH_TOKEN` as a bearer header. It does **not** document authentication-free operation. Therefore the plan uses the public, fixed protocol sentinel `step-one-local-bridge` only to satisfy the CLI's auth mode. This value is not a secret, not an API key, and must never authorize a host request. The host broker authenticates the guest transport/session independently and discards incoming credential headers before adding its real upstream credential. Users must not type or copy any provider secret into the guest.

`ANTHROPIC_BASE_URL` points only at the guest-local bridge. Managed settings with `allowedProviders: ["customEndpoint"]` and the same managed base URL pin that route (the official guide states this behavior requires v2.1.285+). Linux managed settings are deployed by the guest provisioning service to `/etc/claude-code/managed-settings.json`, not to the mounted project. Set `DISABLE_UPDATES=1` for fixed-version operation and `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`. These application settings supplement, not replace, VM network enforcement.

A 2.1.310 real-guest first-run/headless request test is still required to prove that the sentinel works without login prompts, that managed settings load, and that all model traffic uses the bridge. It is not yet verified support.

## dsh remains operator-configured

[PyPI metadata for deepseek-harness 0.3.1](https://pypi.org/pypi/deepseek-harness/0.3.1/json) points to [HenryZ838978/deepseek-harness](https://github.com/HenryZ838978/deepseek-harness) and author Henry Zhang. That is a candidate project; neither its name nor its PyPI metadata establishes that it is the DeepSeek Harness intended by Issue #6 or that DeepSeek publishes it. No package was installed and no CLI contract was assumed.

Keep dsh `NOT_CONFIGURED`, with no default executable download. Issue #6 permits an operator-configured source and CLI name; such configuration must include an exact version, source, digest, argv contract, non-secret broker configuration, and a successful guest smoke test before support is enabled. Do not silently download this candidate or mark #6 closed.

## Minimum guest execution contract

- Host code builds a typed plan and calls an authenticated `GuestChannel`; there is no host `subprocess`/shell fallback.
- The plan carries only catalog artifact IDs, digest-locked installation paths, fixed executable paths/argv, selected model, and a guest-loopback endpoint supplied by the bridge. It does not accept arbitrary installer command strings or arbitrary remote base URLs.
- The host artifact cache verifies archive SHA-512 before transfer. The guest installer rejects traversal, absolute members and link escape, preserves required native supporting files, and installs into a versioned directory inaccessible to the ordinary Agent user.
- Installing packages must not open general guest network access. Runtime model requests use only the host-broker bridge; public sentinel headers confer no identity or authorization.
- Probe `--version` before launching. Return separate installation, process and VM states. Stop, timeout and abnormal exits reap the complete guest process group and stream bounded, safe status/output.
- Unit tests using fake `GuestChannel` establish plan construction and failure behavior only. They cannot establish archive execution, model compatibility, clean exit, VM isolation or no-key integration.

## Remaining acceptance gates

Native archive SRI/member verification; Linux version probes; guest installation and process lifecycle; broker endpoint/session binding; streamed model request compatibility; Claude managed settings and sentinel behavior; dsh source selection; actual VM isolation. Keep these explicit in PRs and keep Issue #6 open until supported agents pass their real-guest checks.
