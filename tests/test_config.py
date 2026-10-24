from pathlib import Path

import pytest

from step_one.config import ConfigError, default_config, from_mapping, load_config, resolve_secret


def test_default_config_is_loadable(tmp_path: Path):
    path = tmp_path / "sandbox.toml"
    path.write_text(default_config("demo"), encoding="utf-8")
    config = load_config(path)
    assert config.name == "demo"
    assert config.network == "deny"
    assert config.agents == ["codex", "claude-code", "dsh"]


def test_literal_secret_is_rejected():
    with pytest.raises(ConfigError, match="literal secret"):
        from_mapping({"mcp_servers": {"api": {"api_key": "sk-secret"}}})


def test_secret_references_resolve_without_persisting_values(tmp_path: Path):
    assert resolve_secret("env:TOKEN", {"TOKEN": "value"}) == "value"
    secret_file = tmp_path / "secret"
    secret_file.write_text("from-file\n", encoding="utf-8")
    assert resolve_secret(f"file:{secret_file}") == "from-file"


def test_missing_environment_secret_is_clear():
    with pytest.raises(ConfigError, match="missing"):
        resolve_secret("env:DOES_NOT_EXIST", {})

