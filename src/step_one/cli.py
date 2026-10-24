"""Command line interface for the Step-One desktop sandbox."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

from .config import ConfigError, default_config, load_config


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
    # Keep a clear integration seam for the hardened executor supplied by the
    # core package. The local fallback is deliberately conservative and strips
    # the inherited environment except for explicitly declared secret refs.
    env = {"PATH": os.environ.get("PATH", "")}
    for name, ref in config.secret_refs.items():
        if ref.startswith("env:") and ref[4:] in os.environ:
            env[name.upper()] = os.environ[ref[4:]]
    cwd = Path(config.write_paths[0]).expanduser() if config.write_paths else Path.cwd()
    if not cwd.is_absolute():
        cwd = Path(args.config).resolve().parent / cwd
    cwd.mkdir(parents=True, exist_ok=True)
    try:
        return subprocess.run(command, cwd=cwd, env=env, check=False).returncode
    except FileNotFoundError:
        print(f"step-one: command not found: {command[0]}", file=sys.stderr)
        return 127


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    return {"create": _cmd_create, "doctor": _cmd_doctor, "policy": _cmd_policy, "run": _cmd_run}[args.command](args)

