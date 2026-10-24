# Friendly workflow

Step-One keeps the common path short:

1. Run `step-one create ./my-sandbox`.
2. Open `my-sandbox/sandbox.toml` and choose agents. Leave `network = "deny"`
   unless a tool genuinely needs an allowlist.
3. Put API keys in the host keychain, an environment variable, or a protected
   file. Add only a reference such as `env:ANTHROPIC_API_KEY` to the config.
4. Run `step-one doctor` before the first launch. It explains which optional
   agent binaries are installed.
5. Inspect `step-one policy` whenever a project is handed to another person.

The policy output is intentionally plain language. Team members should be able
to understand what an agent can read, write, and reach over the network without
learning container flags or shell syntax.

## Troubleshooting

If `doctor` reports a missing agent, install that agent on the host and run the
check again. A missing optional binary does not weaken the sandbox. If `run`
reports a missing secret, export the referenced environment variable in the
same terminal session and retry; the value is never copied into `sandbox.toml`.

