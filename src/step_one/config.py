"""Human-friendly, security-first workspace configuration.

The configuration intentionally stores references to secrets (for example,
``env:ANTHROPIC_API_KEY``), never their values.  ``load_config`` rejects
obvious secret-looking literals so accidental check-ins fail early.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
import os
import re
import tomllib
from typing import Any


class ConfigError(ValueError):
    """A configuration cannot be used safely."""


_SECRET_NAME = re.compile(r"(key|token|password|secret|credential)", re.I)


@dataclass(slots=True)
class WorkspaceConfig:
    name: str = "my-sandbox"
    profile: str = "standard"
    read_paths: list[str] = field(default_factory=list)
    write_paths: list[str] = field(default_factory=list)
    network: str = "deny"
    agents: list[str] = field(default_factory=lambda: ["codex"])
    mcp_servers: dict[str, dict[str, Any]] = field(default_factory=dict)
    secret_refs: dict[str, str] = field(default_factory=dict)

    def validate(self) -> list[str]:
        errors: list[str] = []
        if not self.name or any(ch not in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_" for ch in self.name):
            errors.append("name must contain only letters, numbers, '-' or '_'")
        if self.profile not in {"strict", "standard", "developer"}:
            errors.append("profile must be strict, standard, or developer")
        if self.network not in {"deny", "allowlist", "full"}:
            errors.append("network must be deny, allowlist, or full")
        supported = {"claude-code", "codex", "dsh"}
        unknown = sorted(set(self.agents) - supported)
        if unknown:
            errors.append(f"unsupported agents: {', '.join(unknown)}")
        for key, value in self.secret_refs.items():
            if not _is_secret_ref(value):
                errors.append(f"secret_refs.{key} must use env:NAME or file:PATH")
        for key, value in _walk(self.mcp_servers):
            if _SECRET_NAME.search(key) and isinstance(value, str) and not _is_secret_ref(value) and not _is_mcp_secret_ref(value):
                errors.append(f"mcp_servers contains a literal secret at {key}")
        return errors


def _is_secret_ref(value: str) -> bool:
    return value.startswith("env:") and len(value) > 4 or value.startswith("file:") and len(value) > 5


def _is_mcp_secret_ref(value: str) -> bool:
    return value.startswith("secret:") and len(value) > 7


def _walk(value: Any, prefix: str = ""):
    if isinstance(value, dict):
        for key, child in value.items():
            path = f"{prefix}.{key}" if prefix else str(key)
            yield from _walk(child, path)
    elif isinstance(value, list):
        for index, child in enumerate(value):
            yield from _walk(child, f"{prefix}[{index}]")
    else:
        yield prefix, value


def from_mapping(data: dict[str, Any]) -> WorkspaceConfig:
    sandbox = data.get("sandbox", data)
    config = WorkspaceConfig(
        name=str(sandbox.get("name", "my-sandbox")),
        profile=str(sandbox.get("profile", "standard")),
        read_paths=[str(item) for item in sandbox.get("read_paths", [])],
        write_paths=[str(item) for item in sandbox.get("write_paths", [])],
        network=str(sandbox.get("network", "deny")),
        agents=[str(item) for item in sandbox.get("agents", ["codex"])],
        mcp_servers=dict(data.get("mcp_servers", {})),
        secret_refs={str(k): str(v) for k, v in data.get("secret_refs", {}).items()},
    )
    errors = config.validate()
    if errors:
        raise ConfigError("; ".join(errors))
    return config


def load_config(path: str | Path) -> WorkspaceConfig:
    config_path = Path(path)
    try:
        with config_path.open("rb") as stream:
            data = tomllib.load(stream)
    except FileNotFoundError as exc:
        raise ConfigError(f"configuration not found: {config_path}") from exc
    except tomllib.TOMLDecodeError as exc:
        raise ConfigError(f"invalid TOML: {exc}") from exc
    return from_mapping(data)


def default_config(name: str = "my-sandbox") -> str:
    """Return a safe starter file suitable for ``step-one create``."""
    return f'''# Step-One sandbox configuration. Secret values must stay outside this file.
[sandbox]
name = "{name}"
profile = "standard"
network = "deny"
agents = ["codex", "claude-code", "dsh"]
read_paths = []
write_paths = ["./workspace"]

# Reference environment variables instead of writing API keys or passwords.
[secret_refs]
# anthropic_api_key = "env:ANTHROPIC_API_KEY"

[mcp_servers]
# [mcp_servers.filesystem]
# command = "npx"
# args = ["-y", "@modelcontextprotocol/server-filesystem", "./workspace"]
'''


def resolve_secret(ref: str, environ: dict[str, str] | None = None) -> str:
    """Resolve a secret reference at launch time without persisting it."""
    if not _is_secret_ref(ref):
        raise ConfigError("only env:NAME and file:PATH references are supported")
    if ref.startswith("env:"):
        value = (environ or os.environ).get(ref[4:])
        if not value:
            raise ConfigError(f"environment variable is missing: {ref[4:]}")
        return value
    path = Path(ref[5:]).expanduser()
    try:
        return path.read_text(encoding="utf-8").rstrip("\r\n")
    except OSError as exc:
        raise ConfigError(f"secret file cannot be read: {path}") from exc

