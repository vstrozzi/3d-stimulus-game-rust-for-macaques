import json
import math
import shutil
from pathlib import Path

from deploy_backend.analysis import build_dataset as bd
from deploy_backend.analysis_store import AnalysisStore

OBJECTS = [
    {"target_door": 2, "colors": [[1, 0, 0, 1], [0, 0, 1, 1], [0, 0, 1, 1]], "textures": [0, 3, 7], "decorations_shape": [1, 0, 3]},
    {"target_door": 5, "colors": [[0, 0, 1, 1], [1, 0, 0, 1], [0, 0, 1, 1]], "textures": [0, 9, 9], "decorations_shape": [1, 2, 2]},
]


def write(root, path, data):
    file = root / path
    file.parent.mkdir(parents=True, exist_ok=True)
    file.write_text(json.dumps(data))
    return file


def frames(t0, wrong):
    """A rotation towards the reward, `wrong` early checks, then a correct check."""
    out, angle = {}, 1.2
    for i in range(40):
        angle = max(0.0, angle - 0.04)
        check = (i in range(5, 5 + wrong)) or i == 39
        out[str(i)] = {"state_read": {"current_angle": angle, "current_alignment": math.cos(angle), "present_elapsed_secs": t0 + i / 60, "is_blank": False},
                       "commands_sent": {"rotate_left": angle > 0, "rotate_right": False, "check": check, "animation_all_door": False}}
    return out


def session(root, date, pid, name, levels, start, current=True, split_level=None, next_date=None):
    """Write one session: level summaries plus trial files in the server's layout."""
    run, t = 0, 0.0
    for level in levels:
        folder = f"{next_date if level == split_level else date}/{pid}/{name}/level_{level:03d}/120000"
        runs = []
        for k, chain in enumerate([0, 1, 0, 1]):
            cfg = {**OBJECTS[chain], "cosine_alignment_threshold": 0.95}
            fname = f"{pid}_level_{level:03d}_trial_{k:03d}_run_{run:04d}_object_{chain:03d}_x.json"
            trial = {"level_index": level, "active_chain": chain, "trial_index_in_chain": k // 2, "trial_run_counter": run,
                     "trial_config": cfg, "start_orient": 0.3, "outcome": "advance", "timestamp_start": start,
                     "frames": frames(t, wrong=(run % 3)),
                     "hr_info": {"device": "Polar H10", "trial_start_elapsed_secs": t, "trial_end_elapsed_secs": t + 1},
                     "hr_meas": [{"beat_elapsed_secs": t + 0.5, "hr": 70 + run, "rr_ms": [800]}]}
            if current:
                trial["trial_chain_completed"] = k >= 2
            write(root, f"{folder}/trials/{fname}", trial)
            runs.append({"file": fname, "trial_run_counter": run, "outcome": "advance"})
            run += 1; t += 2
        write(root, f"{folder}/{pid}_level_{level:03d}_summary_x.json", {
            "participant": pid, "level_index": level, "timestamp_start": start, "level_completed": "completed",
            "level_config": {"objects": OBJECTS, "trials": [{"show_all": False}, {"show_all": True}], "fixed": {"remove_completed_chains": False}},
            "trials_runs": runs})


def as_project(root, tmp):
    """The same sessions in the layout build_dataset.py expects: data/<participant folder>/..."""
    project = tmp / "project"
    for summary in root.rglob("*_summary_*.json"):
        rel = summary.relative_to(root)
        target = project / "data" / rel.parts[2] / Path(*rel.parts[3:]).parent
        shutil.copytree(summary.parent, target, dirs_exist_ok=True)
    return project


def test_tables_match_build_dataset(tmp_path):
    root = tmp_path / "logs"
    session(root, "2026-11-15", "Alice", "Alice_2026-11-15_10-00-00_aaaa", [0, 5], "2026-11-15T10:00:00Z")
    # Bob's session crosses midnight: level 5 was uploaded under the next day.
    session(root, "2026-11-15", "Bob", "Bob_2026-11-15_23-50-00_bbbb", [0, 5], "2026-11-15T22:50:00Z", split_level=5, next_date="2026-11-16")
    store = AnalysisStore(root)
    ids = [s["id"] for s in store.catalog()["sessions"]]
    assert [s.split("/")[0] for s in ids] == ["Alice", "Bob"]
    trials, positions, errors = store.tables(ids)
    assert not errors and len(trials) == 16
    from deploy_backend.analysis.dataset import write_tables
    write_tables(trials, positions, tmp_path / "mine")
    project = as_project(root, tmp_path)
    bd.build_project(str(project))
    for name in ("trials.csv", "positions.csv"):
        assert (tmp_path / "mine" / name).read_bytes() == (project / "out_rebuilt" / name).read_bytes(), name
    assert all(isinstance(t["features_congruent"], bool) for t in trials if t["level_nr"] == 5), "congruency comes from the script"

    data = store.data(ids)
    assert len(data["rows"]) == 16 and data["participants"] == ["Alice", "Bob"]
    rows = [dict(zip(data["columns"], r)) for r in data["rows"]]
    assert {r["round"] for r in rows} == {0, 1} and all(r["pos_in_round"] in (1, 2) for r in rows)
    assert [r["wrong"] for r in rows[:4]] == [0, 1, 2, 0]
    assert len(data["heart"]) == 16 and data["heart"][0]["readings"][0][2:] == [70, True]


def test_cache_legacy_and_folder_selection(tmp_path):
    root = tmp_path / "logs"
    session(root, "2026-11-15", "Old", "Old_2026-11-15_10-00-00_cccc", [0], "2026-11-15T10:00:00Z", current=False)
    session(root, "2026-11-16", "Old", "Old_2026-11-16_10-00-00_dddd", [0], "2026-11-16T10:00:00Z")
    store = AnalysisStore(root)
    catalog = store.catalog()
    assert [s["label"] for s in catalog["sessions"]] == ["Old #1", "Old #2"], "repeat sessions get their own participant label"
    ids = [s["id"] for s in catalog["sessions"]]
    trials, _, errors = store.tables(ids)
    assert not errors and len(trials) == 8, "the old game version is detected per session"
    with store._connect() as db:
        stamps = {r["session"]: r["stamp"] for r in db.execute("SELECT session, stamp FROM builds")}
    store.tables(ids)
    trial = next((root / "2026-11-15").rglob("*_run_0000_*.json"))
    doc = json.loads(trial.read_text()); doc["outcome"] = "stay"; trial.write_text(json.dumps(doc))
    store.catalog(); store.tables(ids)
    with store._connect() as db:
        after = {r["session"]: r["stamp"] for r in db.execute("SELECT session, stamp FROM builds")}
    changed = [s for s in ids if stamps[s] != after[s]]
    assert changed == [ids[0]], "only the edited session is rebuilt"
    assert store.sessions_under("2026-11-16") == [ids[1]]
    assert sorted(store.sessions_under("")) == sorted(ids)
    # A session that mixes game versions is reported, not silently mixed.
    doc = json.loads(trial.read_text()); doc["trial_chain_completed"] = False; trial.write_text(json.dumps(doc))
    store.catalog()
    _, _, errors = store.tables(ids)
    assert "game version" in errors[ids[0]]
    assert any(e["error"].startswith("Not analysed") for e in store.catalog()["errors"])


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
    assert result["sessions"][0]["date"] == "2026-11-15"
    assert any(e["path"].endswith("broken.json") for e in result["errors"])


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
    session(tmp_path, "2026-11-16", "Alice", "Alice_2026-11-16_12-00-00_aaaa", [0], "2026-11-16T12:00:00Z")
    with TestClient(server.app) as client:
        assert client.get("/admin/analysis/catalog").status_code == 401
        client.cookies.set("auth", server._signer.dumps({"role": "player"}))
        assert client.get("/admin/analysis/catalog").status_code == 403
        assert client.post("/admin/analysis/data", json={"sessions": []}).status_code == 403
        client.cookies.set("auth", server._signer.dumps({"role": "admin"}))
        session_id = client.get("/admin/analysis/catalog").json()["sessions"][0]["id"]
        assert len(client.post("/admin/analysis/data", json={"sessions": [session_id]}).json()["rows"]) == 4
        for body in [[], {}, {"sessions": "bad"}, {"sessions": [1]}]:
            assert client.post("/admin/analysis/data", json=body).status_code == 400
        assert client.post("/admin/analysis/data", content="{").status_code == 400
        tables = zipfile.ZipFile(io.BytesIO(client.post("/admin/analysis/tables", json={"sessions": [session_id]}).content))
        assert sorted(tables.namelist()) == ["positions.csv", "trials.csv"]
        assert ".explorer" not in client.get("/admin/list").json()["dirs"]
        plain = zipfile.ZipFile(io.BytesIO(client.get("/admin/zip").content)).namelist()
        assert not any(".explorer" in name or name.startswith("out_rebuilt/") for name in plain)
        parsed = zipfile.ZipFile(io.BytesIO(client.get("/admin/zip?path=2026-11-16&parsed=1").content)).namelist()
        assert "out_rebuilt/trials.csv" in parsed and "out_rebuilt/positions.csv" in parsed
