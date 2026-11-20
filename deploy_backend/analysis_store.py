"""Catalog and analysis tables for the admin explorer. Raw JSON remains authoritative.

The catalog (participants, dates, which files belong to which session) is
rebuilt incrementally on request, including overwrites made by Polar's
end-of-session synchronization. The behavioural tables come from the analysis
team's scripts (deploy_backend/analysis): `build_dataset.py` runs once per
session and its result is cached until a file of that session changes; requested
sessions are then combined exactly as the script combines participants.
"""
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import re
import sqlite3
import threading
import zlib
from datetime import datetime
from zoneinfo import ZoneInfo

from deploy_backend.analysis import dataset


class AnalysisStore:
    def __init__(self, root, timezone="Europe/Zurich"):
        self.root = Path(root).resolve()
        self.timezone = timezone
        self.lock = threading.Lock()
        self.build_lock = threading.Lock()
        self._warming = False

    @contextmanager
    def _connect(self):
        directory = self.root / ".explorer"
        directory.mkdir(parents=True, exist_ok=True)
        db = sqlite3.connect(directory / "catalog.sqlite3", timeout=60)
        db.row_factory = sqlite3.Row
        db.execute("""CREATE TABLE IF NOT EXISTS documents (
            path TEXT PRIMARY KEY, stamp TEXT, session TEXT, kind TEXT,
            participant TEXT, started TEXT, payload TEXT, error TEXT)""")
        db.execute("CREATE INDEX IF NOT EXISTS by_session ON documents(session, path)")
        db.execute("CREATE TABLE IF NOT EXISTS builds (session TEXT PRIMARY KEY, stamp TEXT, result BLOB, error TEXT)")
        db.execute("DROP TABLE IF EXISTS figures")   # server-drawn PNGs of an earlier version
        db.execute("CREATE TABLE IF NOT EXISTS turns (session TEXT PRIMARY KEY, stamp TEXT, result BLOB)")
        try:
            with db:
                yield db
        finally:
            db.close()

    def _session(self, path):
        parts = list(path.parts)
        if parts and re.fullmatch(r"\d{4}-\d{2}-\d{2}", parts[0]):
            parts.pop(0)  # Upload day may change when a session crosses midnight.
        level = next((i for i, p in enumerate(parts) if re.fullmatch(r"level_\d+", p)), None)
        prefix = parts[:level] if level is not None else parts[:-1]
        return "/".join(prefix)

    @staticmethod
    def _payload(doc):
        # Everything but the frames and raw sensor samples: heart rate readings,
        # level summaries and the game-version marker are read from here.
        return {k: v for k, v in doc.items() if k not in ("frames", "hr_ecg", "hr_ppg")}

    def refresh(self):
        with self.lock, self._connect() as db:
            existing = {r["path"]: r["stamp"] for r in db.execute("SELECT path, stamp FROM documents")}
            seen = set()
            for directory, dirs, files in os.walk(self.root, followlinks=False):
                dirs[:] = sorted(d for d in dirs if not d.startswith(".") and not Path(directory, d).is_symlink())
                for name in sorted(files):
                    if not name.endswith(".json"):
                        continue
                    file = Path(directory, name)
                    if file.is_symlink():
                        continue
                    rel = file.relative_to(self.root)
                    path = rel.as_posix()
                    seen.add(path)
                    try:
                        stat = file.stat()
                        stamp = f"{stat.st_mtime_ns}:{stat.st_size}"
                        if existing.get(path) == stamp:
                            continue
                        doc = json.loads(file.read_text(encoding="utf-8"))
                        if not isinstance(doc, dict):
                            raise ValueError("Expected a JSON object")
                        kind = "trial" if "trial_run_counter" in doc and "trial_config" in doc else "level" if "trials_runs" in doc else "other"
                        session = self._session(rel)
                        parts = session.split("/")
                        participant = str(doc.get("participant") or parts[0])
                        started = doc.get("timestamp_start") or ""
                        payload = json.dumps(self._payload(doc) if kind in ("trial", "level") else {}, separators=(",", ":"), allow_nan=False)
                        db.execute("INSERT OR REPLACE INTO documents VALUES (?, ?, ?, ?, ?, ?, ?, NULL)",
                                   (path, stamp, session, kind, participant, started, payload))
                    except (OSError, ValueError, TypeError, AttributeError) as exc:
                        # Keep indexing other files; make missing data visible.
                        db.execute("INSERT OR REPLACE INTO documents VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                                   (path, "error", self._session(rel), "error", "", "", "{}", str(exc)))
            for path in existing.keys() - seen:
                db.execute("DELETE FROM documents WHERE path = ?", (path,))

    def _sessions(self, db):
        """Sessions in start order, with the participant label used in the tables: the
        participant's name, plus #k when the same name has several sessions."""
        rows = [dict(r) for r in db.execute("""SELECT session, MAX(participant) participant,
                MIN(NULLIF(started, '')) started, SUM(kind = 'trial') trials, SUM(kind = 'level') levels
                FROM documents WHERE kind IN ('trial', 'level') GROUP BY session ORDER BY started, session""")]
        count, seen = {}, {}
        for r in rows:
            count[r["participant"]] = count.get(r["participant"], 0) + 1
        for r in rows:
            seen[r["participant"]] = seen.get(r["participant"], 0) + 1
            r["label"] = r["participant"] + (f" #{seen[r['participant']]}" if count[r["participant"]] > 1 else "")
        return rows

    def catalog(self):
        self.refresh()
        with self._connect() as db:
            sessions = []
            for item in self._sessions(db):
                item["id"] = item.pop("session")
                try:
                    started = datetime.fromisoformat(item["started"].replace("Z", "+00:00"))
                    zone = ZoneInfo(self.timezone)
                    if started.tzinfo is None:
                        started = started.replace(tzinfo=zone)
                    local = started.astimezone(zone)
                    item["date"] = local.date().isoformat()
                    item["time"] = local.strftime("%H:%M:%S")
                except (ValueError, AttributeError):
                    item["date"] = "unknown"
                    item["time"] = ""
                item["has_hr"] = False
                item["paradigms"] = set()
                item["level_indices"] = set()
                for document in db.execute("SELECT payload FROM documents WHERE session = ? AND kind IN ('trial', 'level')", (item["id"],)):
                    doc = json.loads(document["payload"])
                    item["has_hr"] |= bool(doc.get("hr_meas") or doc.get("hr_measurements") or doc.get("hr_info"))
                    paradigm = doc.get("session_info", {}).get("paradigm")
                    if paradigm:
                        item["paradigms"].add(str(paradigm))
                    if isinstance(doc.get("level_index"), int):
                        item["level_indices"].add(doc["level_index"])
                item["paradigms"] = sorted(item["paradigms"])
                item["level_indices"] = sorted(item["level_indices"])
                sessions.append(item)
            errors = [dict(r) for r in db.execute("SELECT path, error FROM documents WHERE error IS NOT NULL")]
            errors += [{"path": r["session"], "error": "Not analysed: " + r["error"]}
                       for r in db.execute("SELECT session, error FROM builds WHERE error IS NOT NULL")]
        self._warm([s["id"] for s in sessions])
        return {"sessions": sessions, "errors": errors, "timezone": self.timezone}

    # ── Script tables ────────────────────────────────────────────────────────
    def _build(self, sessions):
        """Cached build_session() results for these sessions, building missing or stale ones.
        Returns ({session: result}, {session: error})."""
        with self.build_lock:
            with self._connect() as db:
                known = {r["session"]: r for r in self._sessions(db)}
                results, errors = {}, {}
                for session in sessions:
                    if session not in known:
                        continue
                    docs = db.execute("SELECT path, stamp, kind, payload FROM documents WHERE session = ? AND kind IN ('trial', 'level') ORDER BY path",
                                      (session,)).fetchall()
                    stamp = self._stamp(docs, known[session])
                    cached = db.execute("SELECT stamp, result, error FROM builds WHERE session = ?", (session,)).fetchone()
                    if cached and cached["stamp"] == stamp:
                        if cached["error"]:
                            errors[session] = cached["error"]
                        else:
                            results[session] = json.loads(zlib.decompress(cached["result"]))
                        continue
                    try:
                        trial_docs = [json.loads(d["payload"]) for d in docs if d["kind"] == "trial"]
                        marked = {"trial_chain_completed" in d for d in trial_docs}
                        if len(marked) > 1:
                            raise ValueError("mixes trial logs from the old and the current game version")
                        dirs = self._dirs(docs)
                        result = dataset.build_session(known[session]["label"], session, dirs, legacy_game=marked == {False})
                        blob = zlib.compress(dataset.dumps(result).encode())
                        db.execute("INSERT OR REPLACE INTO builds VALUES (?, ?, ?, NULL)", (session, stamp, blob))
                        results[session] = json.loads(zlib.decompress(blob))
                    except Exception as exc:  # one broken session must not hide the others
                        errors[session] = f"{type(exc).__name__}: {exc}"
                        db.execute("INSERT OR REPLACE INTO builds VALUES (?, ?, NULL, ?)", (session, stamp, errors[session]))
                return results, errors

    @staticmethod
    def _stamp(docs, session):
        """Changes whenever a file of the session, its participant label or the build code changes."""
        return (hashlib.sha1("\n".join(f"{d['path']}:{d['stamp']}" for d in docs).encode()).hexdigest()
                + ":" + session["label"] + ":v" + dataset.BUILD_VERSION)

    def turns(self, sessions):
        """How each trial's object was turned (make_MAIN_sampling_figures.trajectory()), thinned
        for drawing: {"session|trial_run_counter": trajectory}. Read from the raw trial files on
        first use and cached per session like the builds; sessions the script cannot analyse are
        left out."""
        sessions = self._ordered(sessions)
        _, errors = self._build(sessions)
        out = {}
        with self.build_lock, self._connect() as db:
            known = {r["session"]: r for r in self._sessions(db)}
            for session in sessions:
                if session in errors or session not in known:
                    continue
                docs = db.execute("SELECT path, stamp FROM documents WHERE session = ? AND kind IN ('trial', 'level') ORDER BY path",
                                  (session,)).fetchall()
                stamp = "turns2:" + self._stamp(docs, known[session])
                cached = db.execute("SELECT stamp, result FROM turns WHERE session = ?", (session,)).fetchone()
                if cached and cached["stamp"] == stamp:
                    result = json.loads(zlib.decompress(cached["result"]))
                else:
                    result = dataset.session_turns(self._dirs(docs))
                    db.execute("INSERT OR REPLACE INTO turns VALUES (?, ?, ?)", (session, stamp, zlib.compress(dataset.dumps(result).encode())))
                out.update({f"{session}|{k}": v for k, v in result.items()})
        return {"turns": out}

    def _dirs(self, docs):
        """The folders holding these files of one session (several when it crosses midnight)."""
        return sorted({str(self.root.joinpath(*self._session_dir(Path(d["path"])))) for d in docs})

    def _session_dir(self, path):
        parts = list(path.parts)
        level = next((i for i, p in enumerate(parts) if re.fullmatch(r"level_\d+", p)), len(parts) - 1)
        return parts[:level]

    def _warm(self, sessions):
        """Build new or changed sessions in the background so plotting is quick."""
        if self._warming:
            return
        self._warming = True

        def run():
            try:
                self._build(sessions)
            finally:
                self._warming = False
        threading.Thread(target=run, daemon=True).start()

    def _ordered(self, sessions):
        with self._connect() as db:
            order = [r["session"] for r in self._sessions(db)]
        wanted = set(sessions)
        return [s for s in order if s in wanted]

    def tables(self, sessions):
        """(trials, positions, errors): the script's tables for these sessions, combined in start order."""
        sessions = self._ordered(sessions)
        results, errors = self._build(sessions)
        trials, positions = dataset.assemble([results[s] for s in sessions if s in results])
        return trials, positions, errors

    def data(self, sessions):
        """Everything the explorer plots for these sessions: the figure table derived by the
        scripts, level status, and heart-rate readings per trial (raw values)."""
        sessions = self._ordered(sessions)
        results, errors = self._build(sessions)
        built = [results[s] for s in sessions if s in results]
        trials, _ = dataset.assemble(built)
        levels = {(r["pid"], int(level)): info for r in built for level, info in r["levels"].items()}
        columns, rows, warnings = dataset.figure_table(trials, levels)
        heart = []
        with self._connect() as db:
            for session in sessions:
                for d in db.execute("SELECT payload FROM documents WHERE session = ? AND kind = 'trial'", (session,)):
                    doc = json.loads(d["payload"])
                    info = doc.get("hr_info") or {}
                    meas = doc.get("hr_meas") or []
                    if not meas and not info:
                        continue
                    device = info.get("device")
                    heart.append({
                        "session": session, "trial_run_counter": doc.get("trial_run_counter"),
                        "start": info.get("trial_start_elapsed_secs"), "end": info.get("trial_end_elapsed_secs"),
                        "device": str((device or {}).get("name") if isinstance(device, dict) else device or ""),
                        # [beat time, arrival time, bpm, has RR intervals]
                        "readings": [[m.get("beat_elapsed_secs"), m.get("arrival_elapsed_secs"), m.get("hr"),
                                      any(isinstance(r, (int, float)) and r > 0 for r in (m.get("rr_ms") or []))] for m in meas],
                    })
        warnings += [f"{s}: not analysed ({e})" for s, e in errors.items()]
        return {"columns": columns, "rows": rows, "participants": [r["pid"] for r in built],
                "sessions": {r["session"]: r["pid"] for r in built}, "heart": heart, "warnings": warnings}

    def sessions_under(self, rel):
        """Sessions with at least one trial or level file inside this folder."""
        prefix = rel.strip("/") + "/" if rel.strip("/") else ""
        with self._connect() as db:
            rows = db.execute("SELECT DISTINCT session FROM documents WHERE kind IN ('trial', 'level') AND substr(path, 1, ?) = ?",
                              (len(prefix), prefix)).fetchall()
        return [r["session"] for r in rows]
