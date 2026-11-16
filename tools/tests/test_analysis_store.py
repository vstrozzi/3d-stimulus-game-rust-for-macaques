import json
from pathlib import Path

from deploy_backend.analysis_store import AnalysisStore


def write(root, path, data):
    file = root / path
    file.parent.mkdir(parents=True, exist_ok=True)
    file.write_text(json.dumps(data))
    return file


def test_retrieval_updates_pagination_and_deletion(tmp_path):
    root = tmp_path / "logs"
    store = AnalysisStore(root)
    base = "2026-11-15/Alice/Alice_run/level_000/120000/"
    summary = {"participant": "Alice", "level_index": 0, "timestamp_start": "2026-11-15T23:30:00Z", "trials_runs": []}
    write(root, base + "summary.json", summary)
    trial = {"level_index": 0, "trial_run_counter": 1, "trial_config": {}, "timestamp_start": "2026-11-15T23:30:01Z",
             "frames": {"0": {"state_read": {"current_alignment": 1}, "commands_sent": {"check": True}},
                        "1": {"state_read": {}, "commands_sent": {"check": False}}},
             "hr_meas": [{"hr": 80}], "hr_ecg": [{"uV": [1, 2], "first_elapsed_secs": 1}]}
    file = write(root, base + "trials/trial.json", trial)
    catalog = store.catalog()
    assert len(catalog["sessions"]) == 1
    session = catalog["sessions"][0]
    assert session["date"] == "2026-11-16"  # Display day in Zurich, not upload day.
    assert session["participant"] == "Alice"
    assert session["has_hr"]
    first = store.data([session["id"]], limit=1)
    assert first["total"] == 2 and first["next_offset"] == 1
    second = store.data([session["id"]], offset=1)
    data = second["documents"][0]["data"]
    assert "frames" not in data and "hr_ecg" not in data
    assert len(data["check_frames"]) == 1
    assert "waveforms" not in data
    trial["hr_meas"] = [{"hr": 90}]
    file.write_text(json.dumps(trial))
    store.catalog()
    assert store.data([session["id"]], offset=1)["documents"][0]["data"]["hr_meas"][0]["hr"] == 90
    trial["check_events"] = [trial["frames"]["0"]]
    file.write_text(json.dumps(trial))
    store.catalog()
    compact = store.data([session["id"]], offset=1)["documents"][0]["data"]
    assert compact["check_events"] == trial["check_events"]
    assert "check_frames" not in compact  # New logs already contain compact events.
    file.unlink()
    assert store.catalog()["sessions"][0]["trials"] == 0
    assert store.data(["../../anything"])["documents"] == []


def test_bad_files_symlinks_and_midnight(tmp_path):
    root = tmp_path / "logs"
    store = AnalysisStore(root)
    data = {"level_index": 0, "trial_run_counter": 1, "trial_config": {}, "timestamp_start": "2026-11-15T22:00:00Z"}
    write(root, "2026-11-15/Bob/run/level_000/120000/trials/a.json", data)
    write(root, "2026-11-16/Bob/run/level_000/120000/trials/b.json", data)
    write(root, "2026-11-16/Bob/run/broken.json", {}).write_text("{")
    external = write(tmp_path, "external/secret.json", data)
    (root / "linked.json").symlink_to(external)
    (root / "linked_directory").symlink_to(external.parent, target_is_directory=True)
    result = store.catalog()
    assert len(result["sessions"]) == 1
    assert result["sessions"][0]["trials"] == 2
    assert len(result["errors"]) == 1
    assert result["errors"][0]["path"].endswith("broken.json")


def test_api_authentication_validation_and_download(tmp_path, monkeypatch):
    from importlib import import_module
    from fastapi.testclient import TestClient
    from passlib.hash import argon2
    import io
    import zipfile

    monkeypatch.setenv("SECRET_KEY", "analysis-test-secret")
    monkeypatch.setenv("PLAYER_PW_HASH", argon2.hash("player"))
    monkeypatch.setenv("ADMIN_PW_HASH", argon2.hash("admin"))
    server = import_module("deploy_backend.log_server")
    monkeypatch.setattr(server, "LOG_ROOT", str(tmp_path))
    monkeypatch.setattr(server, "analysis_store", AnalysisStore(tmp_path))
    write(tmp_path, "2026-11-16/Alice/run/level_000/120000/trials/t.json",
          {"level_index": 0, "trial_run_counter": 1, "trial_config": {}, "timestamp_start": "2026-11-16T12:00:00Z"})
    with TestClient(server.app) as client:
        assert client.get("/admin/analysis/catalog").status_code == 401
        client.cookies.set("auth", server._signer.dumps({"role": "player"}))
        assert client.get("/admin/analysis/catalog").status_code == 403
        assert client.post("/admin/analysis/data", json={"sessions": []}).status_code == 403
        client.cookies.set("auth", server._signer.dumps({"role": "admin"}))
        catalog = client.get("/admin/analysis/catalog").json()
        session = catalog["sessions"][0]["id"]
        result = client.post("/admin/analysis/data", json={"sessions": [session]}).json()
        assert result["total"] == 1
        for body in [[], {}, {"sessions": "bad"}, {"sessions": [1]}, {"sessions": [], "offset": -1}, {"sessions": [], "offset": True}]:
            assert client.post("/admin/analysis/data", json=body).status_code == 400
        assert client.post("/admin/analysis/data", content="{").status_code == 400
        assert ".explorer" not in client.get("/admin/list").json()["dirs"]
        archive = zipfile.ZipFile(io.BytesIO(client.get("/admin/zip").content))
        assert not any(".explorer" in name for name in archive.namelist())
        assert len(archive.namelist()) == 1
