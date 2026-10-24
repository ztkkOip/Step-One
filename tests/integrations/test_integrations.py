from pathlib import Path

import pytest

from src.integrations.adapters import ClaudeCodeAdapter, CodexAdapter, DshAdapter
from src.integrations.mcp import McpServerSpec, SecretRef, StaticSecretProvider, render_mcp_config


def test_mcp_config_keeps_secret_out_of_json(tmp_path: Path) -> None:
    rendered = render_mcp_config(
        [
            McpServerSpec(
                "github",
                "npx",
                ("-y", "@modelcontextprotocol/server-github"),
                {"GITHUB_TOKEN": SecretRef("github.token"), "MODE": "readonly"},
            )
        ],
        StaticSecretProvider({"github.token": "super-secret"}),
    )
    text = rendered.to_json()
    assert "super-secret" not in text
    assert "${STEP_ONE_SECRET_github.token}" in text
    assert rendered.secret_env == {"STEP_ONE_SECRET_github.token": "super-secret"}
    target = rendered.write(tmp_path / "mcp.json")
    assert "super-secret" not in target.read_text(encoding="utf-8")


def test_mcp_requires_provider_and_rejects_duplicates() -> None:
    server = McpServerSpec("x", "tool", env={"TOKEN": SecretRef("token")})
    with pytest.raises(ValueError, match="provider"):
        render_mcp_config([server])
    with pytest.raises(ValueError, match="duplicate"):
        render_mcp_config([McpServerSpec("x", "one"), McpServerSpec("x", "two")])


def test_mcp_validation_rejects_invalid_secret_and_nul() -> None:
    with pytest.raises(ValueError):
        SecretRef("bad name")
    with pytest.raises(ValueError):
        render_mcp_config([McpServerSpec("x", "tool", env={"A": "bad\x00value"})])


@pytest.mark.parametrize(
    ("adapter", "flag", "binary"),
    [(ClaudeCodeAdapter(), "--mcp-config", "claude"), (CodexAdapter(), "--config", "codex"), (DshAdapter(), "--mcp-config", "dsh")],
)
def test_agent_adapters_build_explicit_plan(tmp_path: Path, adapter, flag: str, binary: str) -> None:
    plan = adapter.build_plan(tmp_path, mcp_config=tmp_path / "mcp.json", args=("--yes",))
    assert plan.argv == (binary, flag, str((tmp_path / "mcp.json").resolve()), "--yes")
    assert plan.cwd == tmp_path.resolve()
    assert plan.env == {"STEP_ONE_WORKSPACE": str(tmp_path.resolve())}


def test_adapter_rejects_missing_workspace(tmp_path: Path) -> None:
    with pytest.raises(ValueError):
        ClaudeCodeAdapter().build_plan(tmp_path / "missing")
