from pathlib import Path
import sys

import pytest

from step_one import (
    CommandSpec,
    NetworkDenied,
    PathDenied,
    SandboxExecutor,
    SandboxPolicy,
    SecretError,
    SecretRef,
    StaticSecretProvider,
    resolve_secret_refs,
)


def test_policy_denies_path_outside_workspace(tmp_path: Path):
    policy = SandboxPolicy(tmp_path)
    assert policy.check_path(tmp_path / "file.txt", "write") == tmp_path / "file.txt"
    with pytest.raises(PathDenied):
        policy.check_path(tmp_path.parent / "outside.txt", "write")


def test_policy_network_allowlist_supports_wildcards(tmp_path: Path):
    policy = SandboxPolicy(tmp_path, allowed_network_hosts=("api.example.com", "*.trusted.test"))
    assert policy.check_host("https://api.example.com/v1") == "api.example.com"
    assert policy.check_host("worker.trusted.test") == "worker.trusted.test"
    with pytest.raises(NetworkDenied):
        policy.check_host("evil.example.com")


def test_secret_references_are_resolved_only_at_execution(tmp_path: Path):
    provider = StaticSecretProvider({"TOKEN": "s3cret"})
    assert SecretRef("TOKEN").token == "${SECRET:TOKEN}"
    assert resolve_secret_refs("Bearer ${SECRET:TOKEN}", provider) == "Bearer s3cret"
    with pytest.raises(SecretError):
        resolve_secret_refs("${SECRET:MISSING}", provider)


def test_executor_applies_allowlist_and_env(tmp_path: Path):
    policy = SandboxPolicy(tmp_path, command_allowlist=(Path(sys.executable).name,))
    result = SandboxExecutor(policy).run(CommandSpec((sys.executable, "-c", "import os; print(os.getenv('TOKEN', 'missing'))"), env={"TOKEN": "hidden"}))
    assert result.returncode == 0
    assert result.stdout.strip() == "missing"


def test_executor_rejects_disallowed_command(tmp_path: Path):
    policy = SandboxPolicy(tmp_path, command_allowlist=("definitely-not-this",))
    with pytest.raises(PermissionError):
        SandboxExecutor(policy).run(CommandSpec((sys.executable, "-c", "pass")))
