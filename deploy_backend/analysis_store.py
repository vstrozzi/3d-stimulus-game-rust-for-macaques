"""Retrieval-only catalog for the admin explorer. Raw JSON remains authoritative.

The catalog is rebuilt incrementally on request, including overwrites made by
Polar's end-of-session synchronization. No behavioral statistics live here.
"""
from contextlib import contextmanager
import json
import os
from pathlib import Path
import re
import sqlite3
import threading
from datetime import datetime
from zoneinfo import ZoneInfo


class AnalysisStore:
    def __init__(self, root, timezone="Europe/Zurich"):
        self.root = Path(root).resolve()
        self.timezone = timezone
        self.lock = threading.Lock()

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
        payload = {k: v for k, v in doc.items() if k not in ("frames", "hr_ecg", "hr_ppg")}
        # Select raw check-command rows; correctness, regions, rounds and all
        # other analysis are calculated in JavaScript, never in this catalog.
        if "frames" in doc:
            frames = doc["frames"]
            rows = frames.values() if isinstance(frames, dict) else frames
            if "check_events" not in doc:
                payload["check_frames"] = [row for row in rows if row.get("commands_sent", {}).get("check")]
        return payload

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

    def catalog(self):
        self.refresh()
        with self._connect() as db:
            sessions = []
            for row in db.execute("""SELECT session, MAX(participant) participant,
                    MIN(NULLIF(started, '')) started,
                    SUM(kind = 'trial') trials, SUM(kind = 'level') levels
                    FROM documents WHERE kind IN ('trial', 'level') GROUP BY session ORDER BY started, session"""):
                item = dict(row)
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
        return {"sessions": sessions, "errors": errors, "timezone": self.timezone}

    def data(self, sessions, offset=0, limit=25):
        if not sessions:
            return {"documents": [], "next_offset": None, "total": 0}
        placeholders = ",".join("?" for _ in sessions)
        where = f"session IN ({placeholders}) AND kind IN ('trial', 'level')"
        with self._connect() as db:
            total = db.execute(f"SELECT COUNT(*) FROM documents WHERE {where}", sessions).fetchone()[0]
            rows = db.execute(f"SELECT path, session, kind, payload FROM documents WHERE {where} ORDER BY session, started, path LIMIT ? OFFSET ?",
                              [*sessions, limit, offset]).fetchall()
            documents = []
            for row in rows:
                documents.append({"path": row["path"], "session": row["session"], "kind": row["kind"], "data": json.loads(row["payload"])})
        next_offset = offset + len(documents)
        return {"documents": documents, "total": total, "next_offset": next_offset if next_offset < total else None}
