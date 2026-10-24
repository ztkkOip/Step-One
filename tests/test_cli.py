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

