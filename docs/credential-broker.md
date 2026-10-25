# Host credential and model proxy core

Issue #14 adds host-only library APIs, not a guest listener, desktop credential form, OAuth flow, or working Agent connection.

WindowsCredentialStore reads/writes only current-user Generic credentials named step-one/model/<canonical UUID>. Secret values cannot be serialized or debug-printed through the Secret type; native and owned temporary credential buffers are wiped. Native calls are compiled on Windows; automated tests use an injected fake store and do not inspect or change the user's vault.

HostSessions binds an opaque, non-deserializable SessionId to allowed provider/model/credential references. An authenticated guest channel must establish this binding from verified peer identity; accepting an arbitrary session ID from a request is not authentication. Authorization is checked before vault access and again before dispatch; revocation blocks subsequent authorization, while requests already in the authorization/dispatch path may complete. The second check reduces the race during vault access but is not a cancellation or a linearizable dispatch lease.

ModelBroker accepts only enumerated OpenAI Responses, Anthropic Messages and DeepSeek Chat endpoints. Callers cannot choose hosts, paths, authorization headers or proxies. Ordinary function tools are allowed; remote MCP tools and arbitrary URL/header overrides are rejected. Stream=true is explicitly unsupported until the streaming channel is implemented.

HttpsModelTransport uses verified HTTPS, fixed URLs, no redirects and no inherited proxy. It bounds connection time, total time and response reads. Upstream failures are safe summaries, not raw bodies. Exact credential echoes (including escaped JSON) are rejected as an accidental-disclosure defense; this is not a proof against maliciously encoded/fragmented disclosures from an upstream. Providers remain trusted.

No live model request, paid API call, OAuth handshake or real credential-store modification was used in tests. The guest transport, streaming, credential onboarding, trusted session binding, rate/budget limits and real Agent end-to-end validation remain outstanding. Do not enable this core as an unauthenticated local HTTP gateway.

Configuration reference verified during implementation: https://developers.openai.com/codex/config-reference/ documents fixed custom-provider base_url and requires_openai_auth=false. That reference alone does not demonstrate an end-to-end Codex session.
