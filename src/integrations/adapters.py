"""Launch plans for supported terminal agents."""
from __future__ import annotations

import os
from dataclasses import dataclass
from pathlib import Path
from typing import Mapping, Sequence


@dataclass(frozen=True)
class AgentLaunchPlan:
    """A subprocess invocation prepared by an agent adapter."""

    argv: tuple[str, ...]
    env: Mapping[str, str]
    cwd: Path

    def __post_init__(self) -> None:
        if not self.argv or any(not isinstance(part, str) or not part for part in self.argv):
            raise ValueError("argv must contain non-empty strings")
        if not self.cwd.is_absolute():
            raise ValueError("launch cwd must be absolute")
        object.__setattr__(self, "argv", tuple(self.argv))
        object.__setattr__(self, "env", dict(self.env))


class AgentAdapter:
    """Base adapter shared by the supported CLI agents."""

    executable: str = ""
    mcp_flag: str = "--mcp-config"

    def build_plan(
        self,
        workspace: str | os.PathLike[str],
        *,
        mcp_config: str | os.PathLike[str] | None = None,
        env: Mapping[str, str] | None = None,
        args: Sequence[str] = (),
    ) -> AgentLaunchPlan:
        root = Path(workspace).expanduser().resolve()
        if not root.exists() or not root.is_dir():
            raise ValueError(f"workspace must be an existing directory: {root}")
        command = [self.executable]
        if mcp_config is not None:
            config_path = Path(mcp_config).expanduser().resolve()
            command.extend((self.mcp_flag, str(config_path)))
        command.extend(args)
        launch_env = dict(env or {})
        # Let callers pass only approved values; os.environ is not copied by
        # default so host credentials cannot accidentally bleed into the agent.
        launch_env.setdefault("STEP_ONE_WORKSPACE", str(root))
        return AgentLaunchPlan(tuple(command), launch_env, root)


class ClaudeCodeAdapter(AgentAdapter):
    executable = "claude"
    mcp_flag = "--mcp-config"


class CodexAdapter(AgentAdapter):
    executable = "codex"
    mcp_flag = "--config"


class DshAdapter(AgentAdapter):
    executable = "dsh"
    mcp_flag = "--mcp-config"
