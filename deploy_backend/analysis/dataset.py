"""Server wrapper around the analysis scripts.

`build_dataset.py` is kept byte-for-byte as the analysis team wrote it; this file
only feeds it server sessions and reuses its functions:

- build_session(): run `process_participant()` on one session (one or more folders,
  when a session crosses midnight). The game version is detected per session
  instead of the --legacy-game flag. The result is cacheable per session.
- assemble(): combine cached sessions into the tables `build_project()` would write
  for the same participants in the same order. Colour and object ids are
  registries shared across participants in the script, so each session's local ids
  are mapped onto shared ones in first-seen order (identical to one shared
  registry); the ranked ids are then filled by the script's `add_ranked_ids()`.
- figure_table(): the data preparation of `make_MAIN_figures.py` (`add_rounds()`
  and `load()`), without matplotlib. The browser draws the figures from it.
- session_turns(): `make_MAIN_sampling_figures.trajectory()` of every trial, for the
  Rotation Strategy figures the browser draws (admin_analysis.js).
"""
import csv
import io
import json
import math
import os
import threading

import numpy as np
import pandas as pd

from . import build_dataset as bd

_PATCH_LOCK = threading.Lock()

# make_MAIN_figures.py constants, used only when a level summary lacks them.
NSTEPS = {0: 5, 1: 5, 2: 5, 3: 5, 4: 4, 5: 4, 6: 4, 7: 4, 8: 4}
REMOVE_COMPLETED = {6, 8}

# Part of every cached build's key: bump when build_dataset.py or this wrapper changes
# what a session's tables contain, so cached sessions (and cached errors) are rebuilt.
BUILD_VERSION = "2"   # 2: server patches for any reward door and other level 4 objects

# Extra keys carried next to the script's columns (never written to the CSVs).
EXTRA = ("session", "trial_run_counter")


def _multi(fn, dirs):
    """Run a per-folder script helper over several folders."""
    def run(_):
        out = None
        for d in dirs:
            part = fn(d)
            if out is None:
                out = part
            elif isinstance(out, list):
                out.extend(part)
            else:
                for a, b in zip(out, part):
                    a.update(b)
        if isinstance(out, list):  # find_summaries sorts its result
            out.sort(key=lambda s: (s["level_index"], s.get("timestamp_start", "")))
        return out
    return run


def build_session(pid, session, dirs, legacy_game):
    """Run the script for one session. Returns a JSON-serialisable dict."""
    colors = bd.ColorRegistry()
    objreg = bd.ObjectRegistry(colors)
    trials, positions = [], []
    dirs = [str(d) for d in dirs]
    with _PATCH_LOCK:
        find, index = bd.find_summaries, bd.build_file_index
        if len(dirs) > 1:
            bd.find_summaries, bd.build_file_index = _multi(find, dirs), _multi(index, dirs)
        try:
            bd.process_participant(pid, dirs[0], colors, objreg, trials, positions, legacy_game)
            summaries = bd.find_summaries(dirs[0])
            by_name, by_run = bd.build_file_index(dirs[0])
        finally:
            bd.find_summaries, bd.build_file_index = find, index
    # Same run order and skip rule as process_participant(), to label each row.
    runs = sorted((r for s in summaries for r in s["trials_runs"]), key=lambda r: r.get("trial_run_counter", 0))
    runs = [r for r in runs if by_name.get(r.get("file")) or by_run.get(r.get("trial_run_counter"))]
    if len(runs) != len(trials):
        raise ValueError(f"{session}: {len(trials)} trial rows for {len(runs)} runs")
    for row, run in zip(trials, runs):
        row["session"], row["trial_run_counter"] = session, run.get("trial_run_counter")
    levels = {}
    for s in summaries:  # latest summary per level wins
        cfg = s.get("level_config") or {}
        levels[str(s["level_index"])] = {
            "status": s.get("level_completed"),
            "nsteps": len(cfg.get("trials") or []) or None,
            "remove_completed": (cfg.get("fixed") or {}).get("remove_completed_chains"),
        }
    return {
        "pid": pid, "session": session, "trials": trials, "positions": positions, "levels": levels,
        "colors": [list(k) for k, _ in sorted(colors._map.items(), key=lambda kv: kv[1])],
        "objects": [_sig_json(k) for k, _ in sorted(objreg._map.items(), key=lambda kv: kv[1])],
        "x_level": [[lvl, _sig_json(sig)] for (lvl, sig), _ in sorted(objreg._level_map.items(), key=lambda kv: kv[1])],
    }


def _sig_json(sig):
    return [list(part) for part in sig]


def assemble(results):
    """Tables for these sessions, as build_project() would write them (same order)."""
    colors, objects, x_level = {}, {}, {}
    trials, positions = [], []
    for res in results:
        cmap = {}
        for local, key in enumerate(res["colors"]):
            cmap[local] = colors.setdefault(tuple(key), len(colors))
        sig = lambda s: (tuple(cmap[c] for c in s[0]), tuple(s[1]), tuple(s[2]))
        omap = {local: objects.setdefault(sig(s), len(objects)) for local, s in enumerate(res["objects"])}
        xmap = {local: x_level.setdefault((lvl, sig(s)), len(x_level)) for local, (lvl, s) in enumerate(res["x_level"])}
        for row in res["trials"]:
            trials.append({**row, "object_id": omap[row["object_id"]], "object_x_level_id": xmap[row["object_x_level_id"]]})
        for row in res["positions"]:
            row = {**row, "object_id": omap[row["object_id"]], "object_x_level_id": xmap[row["object_x_level_id"]]}
            for key in ("color", "color_left", "color_right"):
                if row.get(key) is not None:
                    row[key] = cmap[row[key]]
            positions.append(row)
    bd.add_ranked_ids(trials, positions)
    return trials, positions


def write_tables(trials, positions, directory):
    """trials.csv and positions.csv, written by the script's own writer."""
    os.makedirs(directory, exist_ok=True)
    bd.write_csv(os.path.join(directory, "trials.csv"), bd.TRIAL_COLUMNS, trials)
    bd.write_csv(os.path.join(directory, "positions.csv"), bd.POSITION_COLUMNS, positions)


# ─────────────────────────────── make_MAIN_figures.py: data part ────────────────────────────────
def add_rounds(T):
    """make_MAIN_figures.add_rounds(), with chain length and remove_completed_chains read from
    each level's summary (falling back to the script's constants). Violations are returned."""
    T = T.sort_values(["participant_id", "trial_nr_global"], kind="stable").copy()
    already = T.chain__already_completed.astype(str) == "True"
    T["completes"] = ((T.outcome__chain_action == "advance")
                      & (T.chain__step == T.nsteps - 1) & ~already)
    rnd, pos, viol = [], [], 0
    for (_, L), d in T.groupby(["participant_id", "level_nr"], sort=False):
        objs, done, remaining, r, i = set(d.object_id), set(), set(), -1, 0
        remove = bool(d.remove_completed.iloc[0])
        for o, comp, act in zip(d.object_id, d.completes, d.outcome__chain_action):
            if not remaining:                                   # new round: refill the bag
                remaining = (objs - done) if remove else set(objs)
                r += 1; i = 0
            if o not in remaining:
                viol += 1
            remaining.discard(o); i += 1
            rnd.append(r); pos.append(i)
            if comp:
                done.add(o)
            elif act == "retroceed":
                done.discard(o)
    T["round"], T["pos_in_round"] = rnd, pos
    return T, viol


FIGURE_COLUMNS = [
    "participant_id", "session", "trial_run_counter", "level_nr", "object_id", "object_type", "object_type_nr",
    "features_congruent", "cong_key", "object_id_order_by_type", "trial_nr_global", "trial_nr_in_level",
    "round", "pos_in_round", "y", "completes", "wrong", "ok", "type_ok", "noguess", "level_done",
    "first_attempt_type", "first_attempt_position", "nr_attempts", "outcome__success", "outcome__chain_action",
    "chain__step", "clock__trial_duration", "clock__trial_start_time", "object_rep_nr", "object_rep_nr_x_level",
    "starting_position", "nr_position_visits", "loaded_feature", "train_test",
]


def figure_table(trials, levels):
    """make_MAIN_figures.load() for these rows. `levels` maps (participant_id, level) to the
    level summary fields from build_session(). Returns (columns, rows, warnings)."""
    if not trials:
        return FIGURE_COLUMNS, [], []
    buf = io.StringIO()
    columns = bd.TRIAL_COLUMNS + list(EXTRA)
    writer = csv.DictWriter(buf, fieldnames=columns, extrasaction="ignore")
    writer.writeheader()
    for row in trials:
        writer.writerow({c: bd.serialize(row.get(c)) for c in columns})
    buf.seek(0)
    # Read exactly as the script reads trials.csv.
    T = pd.read_csv(buf, dtype={"participant_id": str, "features_congruent": str, "session": str}, keep_default_na=False)
    info = [levels.get((p, int(L)), {}) for p, L in zip(T.participant_id, T.level_nr)]
    T["nsteps"] = [i.get("nsteps") or NSTEPS.get(int(L), 4) for i, L in zip(info, T.level_nr)]
    T["remove_completed"] = [i.get("remove_completed") if i.get("remove_completed") is not None else int(L) in REMOVE_COMPLETED
                             for i, L in zip(info, T.level_nr)]
    T, viol = add_rounds(T)
    warnings = [f"{viol} trials break the round structure (each object once per round)."] if viol else []
    won = T.outcome__success.astype(str) == "True"
    T["wrong"] = np.where(won, T.outcome__success_nr - 1, T.nr_attempts)
    T["cong_key"] = T.features_congruent.map({"True": 0, "False": 1, "": 0})
    T["y"] = T["round"] + 1
    T["ok"] = T.first_attempt_position_correct.astype(str).isin(["True", "1", "1.0"])
    T["type_ok"] = T.first_attempt_type_correct.astype(str).isin(["True", "1", "1.0"])
    T["noguess"] = T.first_attempt_type == ""
    T["level_done"] = [levels.get((p, int(L)), {}).get("status") == "completed" for p, L in zip(T.participant_id, T.level_nr)]
    out = T[FIGURE_COLUMNS].astype(object)
    rows = [[_plain(v) for v in r] for r in out.itertuples(index=False, name=None)]
    return FIGURE_COLUMNS, rows, warnings


def _plain(v):
    if isinstance(v, np.generic):
        v = v.item()
    if isinstance(v, float) and not np.isfinite(v):
        return None
    return v


def dumps(result):
    return json.dumps(result, separators=(",", ":"), allow_nan=False)


# ── make_MAIN_sampling_figures.py (Rotation Strategy): what the browser draws ─────
TURN_STEP = math.radians(3)   # thinning: a point at least every 3 degrees of rotation


def session_trial_paths(dirs):
    """{trial_run_counter: trial JSON path} for one session, matched to the level summaries as
    build_session() and make_MAIN_sampling_figures.file_index() match them."""
    dirs = [str(d) for d in dirs]
    summaries = _multi(bd.find_summaries, dirs)(None)
    by_name, by_run = _multi(bd.build_file_index, dirs)(None)
    out = {}
    for s in summaries:
        for run in s["trials_runs"]:
            path = by_name.get(run.get("file")) or by_run.get(run.get("trial_run_counter"))
            if path:
                out[run.get("trial_run_counter")] = path
    return out


def trajectory(path):
    """make_MAIN_sampling_figures.trajectory(): signed angle and time (s from trial start) per
    frame up to the last check; checks as (angle, time, kind). Also the indices of the check
    frames; None when the trial has no frames."""
    with open(path) as fh:
        d = json.load(fh)
    cfg = d["trial_config"]
    obj_type = bd.object_type_from_door(cfg["target_door"])
    thr = cfg.get("cosine_alignment_threshold", 0.95)
    frames = bd.reconstruct_frames(d)
    if not frames:
        return None
    t0 = frames[0][1]["present_elapsed_secs"]
    th, t, checks, check_idx = [], [], [], set()
    for theta, sr, cs in frames:
        th.append(theta); t.append(sr["present_elapsed_secs"] - t0)
        if cs.get("check"):
            pos = bd.angle_to_position(theta, obj_type)
            kind = ("ok" if bd.check_is_correct(sr, thr)
                    else "type_ok" if bd.position_type(pos) == obj_type else "wrong")
            checks.append((theta, t[-1], kind)); check_idx.add(len(th) - 1)
    th, t = np.array(th), np.array(t)
    t_end = checks[-1][1] if checks else t[-1]
    keep = t <= t_end
    return dict(th=th[keep], t=t[keep], checks=checks, dur=t_end, cfg=cfg, obj_type=obj_type, check_idx=check_idx)


def turn_signs(th):
    """make_MAIN_sampling_figures.turn_colours() as signs: +1 left key, -1 right key; pauses keep the last direction."""
    d = np.array([math.remainder(b - a, 2 * math.pi) for a, b in zip(th[:-1], th[1:])])
    sign, last = np.zeros(len(d)), 0.0
    for i, v in enumerate(d):
        if abs(v) > 1e-6:
            last = np.sign(v)
        sign[i] = last
    if (sign == 0).any():
        first = next((v for v in sign if v != 0), 1.0)
        sign[sign == 0] = first
    return sign


def _hex(rgba):
    return "#" + "".join(f"{max(0, min(255, round(float(v) * 255))):02x}" for v in rgba[:3])


def thin(tr):
    """A trajectory for the browser: the frames where the direction changes, the checks, the ends,
    and enough frames in between that no step turns more than TURN_STEP (pauses are straight
    radial lines, so nothing is lost there). Each kept stretch has one direction, as in the script."""
    th, t, dur = tr["th"], tr["t"], tr["dur"]
    n = len(th)
    frac = t / dur if dur > 0 else np.zeros_like(t)
    sign = turn_signs(th) if n > 1 else np.zeros(0)
    keep, turned = [0], 0.0
    for i in range(1, n):
        turned += abs(math.remainder(th[i] - th[i - 1], 2 * math.pi))
        if (i == n - 1 or i in tr["check_idx"] or turned >= TURN_STEP
                or (i < n - 1 and sign[i] != sign[i - 1])):
            keep.append(i); turned = 0.0
    cfg = tr["cfg"]
    return {
        "th": [round(float(th[i]), 4) for i in keep],
        "f": [round(float(frac[i]), 4) for i in keep],
        "dir": "".join("l" if sign[a] > 0 else "r" for a in keep[:-1]),
        "chk": [[round(float(a), 4), round(tc / dur, 4) if dur > 0 else 1.0, {"ok": 0, "type_ok": 1, "wrong": 2}[k]]
                for a, tc, k in tr["checks"]],
        "door": int(cfg["target_door"]),
        "shapes": list(cfg.get("decorations_shape") or []),
        "faces": [_hex(c) for c in cfg.get("colors") or []],
        "dur": round(float(dur), 3),
    }


def session_turns(dirs):
    """{trial_run_counter: thinned trajectory, or None when it cannot be read} for one session."""
    out = {}
    for counter, path in session_trial_paths(dirs).items():
        try:
            tr = trajectory(path)
            out[str(counter)] = thin(tr) if tr else None
        except (KeyError, ValueError, TypeError, IndexError, OSError, json.JSONDecodeError):
            out[str(counter)] = None   # drawn as an empty tile
    return out
