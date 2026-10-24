"""Constrained subprocess execution; platform sandboxes can be plugged in later."""
from __future__ import annotations
import os
import subprocess
from dataclasses import dataclass
from typing import Mapping, Sequence
from .policy import SandboxPolicy
from .secrets import SecretProvider, resolve_secret_refs

@dataclass(frozen=True, slots=True)
class CommandSpec:
    argv: tuple[str, ...]
    cwd: str | None = None
    env: Mapping[str, str] | None = None
    def __post_init__(self) -> None:
        if not self.argv or not all(isinstance(arg, str) and arg for arg in self.argv):
            raise ValueError("argv must contain at least one non-empty string")
@dataclass(frozen=True, slots=True)
class ExecutionResult:
    returncode: int
    stdout: str
    stderr: str
    timed_out: bool = False
class SandboxExecutor:
    def __init__(self, policy: SandboxPolicy, secret_provider: SecretProvider | None = None):
        self.policy, self.secret_provider = policy, secret_provider
    def run(self, spec: CommandSpec) -> ExecutionResult:
        executable = resolve_secret_refs(spec.argv[0], self.secret_provider) if self.secret_provider else spec.argv[0]
        self.policy.check_command(executable)
        cwd = self.policy.check_cwd(spec.cwd)
        argv = tuple(resolve_secret_refs(arg, self.secret_provider) if self.secret_provider else arg for arg in spec.argv)
        env = {k: v for k, v in os.environ.items() if k in self.policy.allowed_env}
        if spec.env:
            env.update({k: resolve_secret_refs(v, self.secret_provider) if self.secret_provider else v for k, v in spec.env.items() if k in self.policy.allowed_env})
        try:
            completed = subprocess.run(argv, cwd=cwd, env=env, capture_output=True, text=True, timeout=self.policy.timeout_seconds, check=False)
        except subprocess.TimeoutExpired as exc:
            return ExecutionResult(-1, (exc.stdout or "")[:self.policy.max_output_bytes], (exc.stderr or "")[:self.policy.max_output_bytes], True)
        return ExecutionResult(completed.returncode, completed.stdout[:self.policy.max_output_bytes], completed.stderr[:self.policy.max_output_bytes])
