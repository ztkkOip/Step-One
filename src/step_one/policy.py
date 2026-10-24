"""Declarative policy checks used by the sandbox execution layer."""
from __future__ import annotations

from dataclasses import dataclass
from fnmatch import fnmatch
from pathlib import Path
from typing import Literal
from urllib.parse import urlparse

class PolicyViolation(PermissionError):
    """Raised whenever an operation is outside its declared policy."""
class PathDenied(PolicyViolation):
    """A path is outside the permitted read or write roots."""
class NetworkDenied(PolicyViolation):
    """A network destination is not in the allowlist."""

def _resolved(path: Path) -> Path:
    return path.expanduser().resolve(strict=False)
def _inside(path: Path, root: Path) -> bool:
    try:
        path.relative_to(root)
    except ValueError:
        return False
    return True

@dataclass(frozen=True, slots=True)
class SandboxPolicy:
    """User-facing policy for one sandbox session."""
    workspace: Path
    read_paths: tuple[Path, ...] = ()
    write_paths: tuple[Path, ...] = ()
    allowed_network_hosts: tuple[str, ...] = ()
    allowed_env: tuple[str, ...] = ("PATH", "HOME", "TEMP", "TMP", "USERPROFILE")
    command_allowlist: tuple[str, ...] = ()
    timeout_seconds: float = 120.0
    max_output_bytes: int = 1_048_576
    allow_subprocess: bool = True

    def __post_init__(self) -> None:
        workspace = _resolved(Path(self.workspace))
        object.__setattr__(self, "workspace", workspace)
        object.__setattr__(self, "read_paths", tuple(_resolved(Path(p)) for p in (self.read_paths or (workspace,))))
        object.__setattr__(self, "write_paths", tuple(_resolved(Path(p)) for p in (self.write_paths or (workspace,))))
        object.__setattr__(self, "allowed_network_hosts", tuple(h.lower() for h in self.allowed_network_hosts))
        if self.timeout_seconds <= 0 or self.max_output_bytes <= 0:
            raise ValueError("timeout_seconds and max_output_bytes must be positive")

    def check_path(self, path: str | Path, operation: Literal["read", "write"] = "read") -> Path:
        candidate = _resolved(Path(path))
        roots = self.read_paths if operation == "read" else self.write_paths
        if not any(_inside(candidate, root) for root in roots):
            raise PathDenied(f"{operation} access denied: {candidate}")
        return candidate
    def check_cwd(self, cwd: str | Path | None) -> Path:
        return self.check_path(cwd or self.workspace, "read")
    def check_command(self, executable: str) -> None:
        if not self.allow_subprocess:
            raise PolicyViolation("subprocess execution is disabled")
        if self.command_allowlist and Path(executable).name.lower() not in {Path(x).name.lower() for x in self.command_allowlist}:
            raise PolicyViolation(f"command is not allowed: {Path(executable).name}")
    def check_host(self, host_or_url: str) -> str:
        parsed = urlparse(host_or_url if "://" in host_or_url else f"//{host_or_url}")
        host = (parsed.hostname or "").lower().rstrip(".")
        if not host or not any(fnmatch(host, pattern) for pattern in self.allowed_network_hosts):
            raise NetworkDenied(f"network access denied: {host or host_or_url}")
        return host
