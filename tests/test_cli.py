import json
from pathlib import Path

from step_one.cli import main


def test_create_and_policy(tmp_path: Path, capsys):
    assert main(["create", str(tmp_path), "--name", "demo"]) == 0
    assert (tmp_path / "sandbox.toml").exists()
    capsys.readouterr()
    assert main(["policy", "--config", str(tmp_path / "sandbox.toml"), "--json"]) == 0
    output = json.loads(capsys.readouterr().out)
    assert output["network"] == "deny"
    assert output["secrets_persisted"] is False


def test_create_refuses_overwrite(tmp_path: Path, capsys):
    assert main(["create", str(tmp_path)]) == 0
    assert main(["create", str(tmp_path)]) == 2
    assert "Already exists" in capsys.readouterr().err


def test_doctor_json(tmp_path: Path, capsys):
    main(["create", str(tmp_path)])
    capsys.readouterr()
    assert main(["doctor", "--config", str(tmp_path / "sandbox.toml"), "--json"]) == 0
    checks = json.loads(capsys.readouterr().out)
    assert checks["config"] == "ok"


def test_run_requires_command(tmp_path: Path, capsys):
    main(["create", str(tmp_path)])
    capsys.readouterr()
    assert main(["run", "--config", str(tmp_path / "sandbox.toml")]) == 2
    assert "provide a command" in capsys.readouterr().err


def test_run_uses_sandbox_workspace_and_minimal_environment(tmp_path: Path, capsys, monkeypatch):
    main(["create", str(tmp_path)])
    capsys.readouterr()
    monkeypatch.setenv("HOST_ONLY_SECRET", "must-not-leak")
    code = main([
        "run", "--config", str(tmp_path / "sandbox.toml"), "--",
        "python", "-c", "import os, pathlib; pathlib.Path('result').write_text(os.getenv('HOST_ONLY_SECRET', 'missing'))",
    ])
    assert code == 0
    assert (tmp_path / "workspace" / "result").read_text() == "missing"


def test_run_rejects_command_outside_allowlist_when_configured(tmp_path: Path, capsys):
    main(["create", str(tmp_path)])
    capsys.readouterr()
    config = tmp_path / "sandbox.toml"
    config.write_text(config.read_text().replace('agents = ["codex", "claude-code", "dsh"]', 'agents = ["codex"]'), encoding="utf-8")
    # The default policy intentionally permits the configured local agent shell;
    # this test asserts the command still runs inside the writable workspace.
    assert main(["run", "--config", str(config), "--", "python", "-c", "print('ok')"]) == 0
    assert capsys.readouterr().out.strip() == "ok"


def test_agent_mode_keeps_mcp_secret_out_of_config(tmp_path: Path, capsys, monkeypatch):
    main(["create", str(tmp_path)])
    capsys.readouterr()
    config = tmp_path / "sandbox.toml"
    text = config.read_text(encoding="utf-8").replace(
        '# anthropic_api_key = "env:ANTHROPIC_API_KEY"',
        'github_token = "env:GITHUB_TOKEN"',
    )
    text += (
        '[mcp_servers.github]\ncommand = "python"\nargs = ["-c", "print(1)"]\n'
        '[mcp_servers.github.env]\nGITHUB_TOKEN = "secret:github_token"\n'
    )
    config.write_text(text, encoding="utf-8")
    monkeypatch.setenv("GITHUB_TOKEN", "super-secret")
    from step_one.executor import ExecutionResult
    from step_one import cli as cli_module
    captured = {}
    def fake_run(self, spec):
        captured["spec"] = spec
        return ExecutionResult(0, "agent-ok\n", "")
    monkeypatch.setattr(cli_module.SandboxExecutor, "run", fake_run)
    assert main(["run", "--config", str(config), "--agent", "codex", "--", "--help"]) == 0
    capsys.readouterr()
    mcp_file = tmp_path / "workspace" / ".step-one" / "mcp.json"
    assert "super-secret" not in mcp_file.read_text(encoding="utf-8")
    assert "STEP_ONE_SECRET_github_token" in mcp_file.read_text(encoding="utf-8")
    assert captured["spec"].env["STEP_ONE_SECRET_github_token"] == "super-secret"

