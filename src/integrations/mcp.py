"""MCP configuration rendering without persisting secret material."""
from __future__ import annotations

import json
import os
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Mapping, Protocol, Sequence

_SECRET_NAME = re.compile(r"^[A-Za-z][A-Za-z0-9_.-]{0,63}$")


@dataclass(frozen=True)
class SecretRef:
    """A reference to a host-managed secret.

    Only the reference name is serialized. The value is resolved just before
    launching a child process and is never written to the generated config.
    """

    name: str

    def __post_init__(self) -> None:
        if not _SECRET_NAME.fullmatch(self.name):
            raise ValueError("secret names must be 1-64 letters, digits, '.', '_' or '-'")

    @property
    def placeholder(self) -> str:
        return f"${{STEP_ONE_SECRET_{self.name}}}"


class SecretProvider(Protocol):
    def get(self, ref: SecretRef) -> str: ...


@dataclass(frozen=True)
class StaticSecretProvider:
    """Small provider useful for local development and tests.

    Production integrations can implement :class:`SecretProvider` against the
    OS keychain or the desktop app's credential broker.
    """

    values: Mapping[str, str]

    def get(self, ref: SecretRef) -> str:
        try:
            value = self.values[ref.name]
        except KeyError as exc:
            raise KeyError(f"secret {ref.name!r} was not provided") from exc
        if not isinstance(value, str) or not value:
            raise ValueError(f"secret {ref.name!r} must resolve to a non-empty string")
        return value


@dataclass(frozen=True)
class McpServerSpec:
    name: str
    command: str
    args: tuple[str, ...] = ()
    env: Mapping[str, str | SecretRef] = None  # type: ignore[assignment]

    def __post_init__(self) -> None:
        if not self.name or not self.name.strip():
            raise ValueError("MCP server name cannot be empty")
        if not self.command or not self.command.strip():
            raise ValueError("MCP command cannot be empty")
        object.__setattr__(self, "args", tuple(self.args))
        object.__setattr__(self, "env", dict(self.env or {}))
        if any(not isinstance(arg, str) for arg in self.args):
            raise TypeError("MCP args must be strings")


@dataclass(frozen=True)
class RenderedMcpConfig:
    """A config payload plus ephemeral environment values for one launch."""

    payload: Mapping[str, object]
    secret_env: Mapping[str, str]

    def to_json(self) -> str:
        return json.dumps(self.payload, indent=2, sort_keys=True) + "\n"

    def write(self, path: str | os.PathLike[str]) -> Path:
        """Write a placeholder-only config and return its path.

        Secret values are deliberately absent from the file. On POSIX systems
        the file is restricted to the current user where possible.
        """

        target = Path(path)
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(self.to_json(), encoding="utf-8")
        try:
            target.chmod(0o600)
        except OSError:
            pass
        return target


def render_mcp_config(
    servers: Sequence[McpServerSpec],
    provider: SecretProvider | None = None,
) -> RenderedMcpConfig:
    """Render MCP JSON and resolve referenced secrets into launch-only env.

    The resulting JSON contains placeholders such as ``${STEP_ONE_SECRET_TOKEN}``.
    Callers should pass ``secret_env`` directly to ``subprocess`` and avoid
    logging it. A provider is required whenever a :class:`SecretRef` appears.
    """

    payload_servers: dict[str, object] = {}
    secret_env: dict[str, str] = {}
    seen: set[str] = set()
    for server in servers:
        if server.name in seen:
            raise ValueError(f"duplicate MCP server name: {server.name}")
        seen.add(server.name)
        rendered_env: dict[str, str] = {}
        for key, value in server.env.items():
            if not key or "=" in key or "\x00" in key:
                raise ValueError(f"invalid MCP environment key: {key!r}")
            if isinstance(value, SecretRef):
                if provider is None:
                    raise ValueError(f"secret provider required for {value.name!r}")
                env_name = f"STEP_ONE_SECRET_{value.name}"
                secret_env[env_name] = provider.get(value)
                rendered_env[key] = value.placeholder
            elif isinstance(value, str):
                if "\x00" in value:
                    raise ValueError("MCP environment values cannot contain NUL")
                rendered_env[key] = value
            else:
                raise TypeError(f"MCP environment value for {key!r} must be a string or SecretRef")
        item: dict[str, object] = {"command": server.command, "args": list(server.args)}
        if rendered_env:
            item["env"] = rendered_env
        payload_servers[server.name] = item
    return RenderedMcpConfig({"mcpServers": payload_servers}, secret_env)
