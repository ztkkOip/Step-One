"""Command line interface for the Step-One desktop sandbox."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import shutil
import sys

from .config import ConfigError, default_config, load_config, resolve_secret
from .executor import CommandSpec, SandboxExecutor
from .policy import PolicyViolation, SandboxPolicy
from integrations.adapters import ClaudeCodeAdapter, CodexAdapter, DshAdapter
from integrations.mcp import McpServerSpec, SecretRef as McpSecretRef, StaticSecretProvider, render_mcp_config


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="step-one", description="A safe, friendly desktop sandbox for agent tools.")
    sub = parser.add_subparsers(dest="command", required=True)

    create = sub.add_parser("create", help="create a new sandbox workspace")
    create.add_argument("path", nargs="?", default=".", help="workspace directory")
    create.add_argument("--name", default=None, help="sandbox name")
    create.add_argument("--force", action="store_true", help="replace an existing config file")

    doctor = sub.add_parser("doctor", help="check a sandbox configuration and local tools")
    doctor.add_argument("-c", "--config", default="sandbox.toml")
    doctor.add_argument("--json", action="store_true", dest="as_json")

    policy = sub.add_parser("policy", help="show the effective security policy")
    policy.add_argument("-c", "--config", default="sandbox.toml")
    policy.add_argument("--json", action="store_true", dest="as_json")

    run = sub.add_parser("run", help="run an agent command inside the configured workspace")
    run.add_argument("-c", "--config", default="sandbox.toml")
    run.add_argument("--agent", choices=("claude-code", "codex", "dsh"), help="launch a supported agent")
    run.add_argument("--", dest="separator", action="store_true", help=argparse.SUPPRESS)
    run.add_argument("command_args", nargs=argparse.REMAINDER, help="command and arguments")
    return parser


def _load(path: str):
    try:
        return load_config(path)
    except ConfigError as exc:
        print(f"step-one: {exc}", file=sys.stderr)
        return None


def _cmd_create(args: argparse.Namespace) -> int:
    root = Path(args.path).expanduser().resolve()
    root.mkdir(parents=True, exist_ok=True)
    (root / "workspace").mkdir(exist_ok=True)
    path = root / "sandbox.toml"
    if path.exists() and not args.force:
        print(f"Already exists: {path} (use --force to replace)", file=sys.stderr)
        return 2
    name = args.name or root.name or "my-sandbox"
    path.write_text(default_config(name), encoding="utf-8")
    print(f"Created sandbox at {root}")
    print(f"Next: step-one doctor --config {path}")
    return 0


def _policy(config) -> dict:
    return {
        "profile": config.profile,
        "network": config.network,
        "read_paths": config.read_paths,
        "write_paths": config.write_paths,
        "agents": config.agents,
        "secret_refs": sorted(config.secret_refs),
        "secrets_persisted": False,
    }


def _cmd_policy(args: argparse.Namespace) -> int:
    config = _load(args.config)
    if config is None:
        return 2
    data = _policy(config)
    if args.as_json:
        print(json.dumps(data, indent=2))
    else:
        print(f"Profile: {data['profile']}")
        print(f"Network: {data['network']}")
        print(f"Agents: {', '.join(data['agents'])}")
        print(f"Read paths: {', '.join(data['read_paths']) or '(none)'}")
        print(f"Write paths: {', '.join(data['write_paths']) or '(none)'}")
        print("Secrets: injected at launch; never persisted")
    return 0


def _cmd_doctor(args: argparse.Namespace) -> int:
    config = _load(args.config)
    if config is None:
        return 2
    checks = {
        "config": "ok",
        "python": sys.version.split()[0],
        "docker": "available" if shutil.which("docker") else "not found (optional)",
        "claude-code": "available" if shutil.which("claude") else "not found (install when needed)",
        "codex": "available" if shutil.which("codex") else "not found (install when needed)",
        "dsh": "available" if shutil.which("dsh") else "not found (install when needed)",
    }
    if args.as_json:
        print(json.dumps(checks, indent=2))
    else:
        for key, value in checks.items():
            print(f"{key:12} {value}")
    return 0


def _make_policy(config, config_path: str | Path) -> SandboxPolicy:
    config_root = Path(config_path).expanduser().resolve().parent
    def resolve_path(value: str) -> Path:
        path = Path(value).expanduser()
        return path if path.is_absolute() else config_root / path

    workspace = config_root
    write_paths = tuple(resolve_path(path) for path in config.write_paths) or (config_root / "workspace",)
    read_paths = tuple(resolve_path(path) for path in config.read_paths) or (config_root,)
    allowed_env = {"PATH", "HOME", "TEMP", "TMP", "USERPROFILE", "STEP_ONE_WORKSPACE"}
    allowed_env.update(name.upper() for name in config.secret_refs)
    allowed_env.update(f"STEP_ONE_SECRET_{name}" for name in config.secret_refs)
    return SandboxPolicy(
        workspace=workspace,
        read_paths=read_paths,
        write_paths=write_paths,
        allowed_network_hosts=() if config.network == "deny" else ("*",),
        allowed_env=tuple(sorted(allowed_env)),
    )


def _mcp_config(config, workspace: Path) -> tuple[Path | None, dict[str, str]]:
    """Render an MCP file with placeholders and return launch-only secrets."""
    if not config.mcp_servers:
        return None, {}
    values: dict[str, str] = {}
    for name, ref in config.secret_refs.items():
        values[name] = resolve_secret(ref)
    specs = []
    for name, raw in config.mcp_servers.items():
        if not isinstance(raw, dict) or not raw.get("command"):
            raise ConfigError(f"mcp_servers.{name} needs a command")
        env = {}
        for key, value in dict(raw.get("env", {})).items():
            if isinstance(value, str) and value.startswith("secret:"):
                secret_name = value[7:]
                if secret_name not in config.secret_refs:
                    raise ConfigError(f"mcp_servers.{name}.env.{key} references unknown secret: {secret_name}")
                env[key] = McpSecretRef(secret_name)
            else:
                env[key] = str(value)
        specs.append(McpServerSpec(name, str(raw["command"]), tuple(str(x) for x in raw.get("args", [])), env))
    rendered = render_mcp_config(specs, StaticSecretProvider(values))
    target = workspace / ".step-one" / "mcp.json"
    rendered.write(target)
    return target, dict(rendered.secret_env)


def _cmd_run(args: argparse.Namespace) -> int:
    config = _load(args.config)
    if config is None:
        return 2
    command = list(args.command_args)
    if command and command[0] == "--":
        command.pop(0)
    if not command:
        print("step-one run: provide a command after --", file=sys.stderr)
        return 2
    config_path = Path(args.config).expanduser().resolve()
    policy = _make_policy(config, config_path)
    env = {}
    for name, ref in config.secret_refs.items():
        try:
            env[name.upper()] = resolve_secret(ref)
        except ConfigError as exc:
            print(f"step-one: {exc}", file=sys.stderr)
            return 2
    cwd = policy.write_paths[0]
    cwd.mkdir(parents=True, exist_ok=True)
    if args.agent:
        if args.agent not in config.agents:
            print(f"step-one: agent is not enabled in sandbox.toml: {args.agent}", file=sys.stderr)
            return 2
        adapter = {"claude-code": ClaudeCodeAdapter, "codex": CodexAdapter, "dsh": DshAdapter}[args.agent]()
        try:
            mcp_path, mcp_env = _mcp_config(config, cwd)
            plan = adapter.build_plan(cwd, mcp_config=mcp_path, env=mcp_env, args=tuple(command))
        except (ConfigError, ValueError) as exc:
            print(f"step-one: cannot prepare agent: {exc}", file=sys.stderr)
            return 2
        command = list(plan.argv)
        cwd = plan.cwd
        env.update(plan.env)
    try:
        result = SandboxExecutor(policy).run(CommandSpec(tuple(command), cwd=str(cwd), env=env))
    except (PolicyViolation, ValueError) as exc:
        print(f"step-one: execution denied: {exc}", file=sys.stderr)
        return 126
    except FileNotFoundError:
        print(f"step-one: command not found: {command[0]}", file=sys.stderr)
        return 127
    if result.stdout:
        print(result.stdout, end="")
    if result.stderr:
        print(result.stderr, end="", file=sys.stderr)
    if result.timed_out:
        print("step-one: command timed out", file=sys.stderr)
        return 124
    return result.returncode


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    return {"create": _cmd_create, "doctor": _cmd_doctor, "policy": _cmd_policy, "run": _cmd_run}[args.command](args)

