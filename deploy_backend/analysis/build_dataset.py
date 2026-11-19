"""
build_dataset.py — Build the two analysis tables for the ObjectManipulation experiment.

Reads raw data from a project folder (e.g. pilot_Gymer) that contains a `data/`
subfolder with one subfolder per participant, and produces two CSV tables in
`<project>/out_rebuilt/`:

    trials.csv     — one row per trial run (one object attempt)
    positions.csv  — one row per position visit (inherits all trial columns)

See ObjectManipulation_Codebook.pdf for the full column reference and conventions.

Usage:
    python build_dataset.py --project kjpp                       # current game
    python build_dataset.py --project pilot_Gymer --legacy-game  # May-June 2026 game

The two game versions log finished chains differently (see DATA_STRUCTURE.md, "kjpp
differences"). --legacy-game must match the data; a mismatch stops the build.

The newest version of this script was lost; this one was reconstructed from an
older backup so that it reproduces pilot_Gymer/new_version, plus the corrections
listed in CLAUDE.md.

──────────────────────────────────────────────────────────────────────────────
HOW TO ADD A NEW FEATURE (column)
──────────────────────────────────────────────────────────────────────────────
Trial-level column:
    1. Compute the value inside `build_trial_row()` and add it to the returned dict.
    2. Add the column name to TRIAL_COLUMNS (controls order in the CSV).

Position-level column:
    1. Compute the value inside `build_position_rows()` (per-visit loop) and add it
       to the row dict.
    2. Add the column name to POSITION_ONLY_COLUMNS.

Position rows automatically inherit every trial-level column, so you only add
position columns for things that vary within a trial.
──────────────────────────────────────────────────────────────────────────────
"""

import argparse
import csv
import glob
import json
import math
import os
import re
from collections import Counter, defaultdict

# ── Geometry / convention constants ────────────────────────────────────────────
SNAP = math.pi / 3                      # 60° between adjacent snap positions
N_POS = 6

# Which physical prism face (0,1,2) directly faces the camera at each odd
# (face/edge) Type-Guided position. Holds for BOTH object types thanks to the
# Type-Guided alignment (see codebook §1.1). Derived from pyramid.rs: face i sits
# 120° * i ahead of face 0, in the direction the signed angle increases.
FACE_AT_POSITION = {1: 1, 3: 0, 5: 2}

# Position index rises with the rotate_left KEY. That key moves the near surface
# of the prism leftwards, so the face on the LEFT OF THE SCREEN at corner p is the
# one reached by pressing rotate_right: p - 1.
def screen_left_neighbor(p):  return (p - 1) % N_POS
def screen_right_neighbor(p): return (p + 1) % N_POS

TRAIN_LEVELS = {0, 1, 2, 5, 7}          # everything else (3,4,6,8) is test


# ── Small helpers ──────────────────────────────────────────────────────────────

def is_corner_position(p):
    return p % N_POS % 2 == 0           # even = corner, odd = face/edge


def position_type(p):
    return "corner" if is_corner_position(p) else "face"


def object_type_from_door(target_door):
    """Even door -> corner-target object; odd door -> face-target object."""
    return "corner" if target_door % 2 == 0 else "face"


def reward_position_for(object_type):
    """Type-Guided reward position: 0 for corner objects, 3 for face objects."""
    return 0 if object_type == "corner" else 3


def wrap_angle(angle):
    """Wrap an angle to [-pi, pi)."""
    return (angle + math.pi) % (2 * math.pi) - math.pi


def signed_start_angle(start_orient, target_door):
    """Signed distance of the target door from the camera at the start of a trial."""
    return wrap_angle(start_orient + (target_door + 1) * SNAP)


def reconstruct_frames(trial_data):
    """
    Return [(signed_angle, state_read, commands_sent), ...] for all non-blank frames.

    The logged current_angle is acos(alignment): an unsigned distance in [0, pi]
    that cannot tell left of the reward from right of it. Each frame is +a or -a;
    the pair of choices is picked for the whole trial (Viterbi over the two signs)
    so that the motion best fits the keys held. A key held on one frame moves the
    prism on the next, or, in newer games (kjpp), sometimes one frame later, so a
    step may match either of the two previous frames' keys, or no movement.
    Choosing frame by frame instead goes wrong near 0 and pi, where +a and -a are
    close: one late frame there flips the side for the rest of the stretch.
    start_orient anchors the first frame.
    """
    frames = trial_data["frames"]
    door = trial_data["trial_config"]["target_door"]
    speed = trial_data["trial_config"].get("camera_speed_rotate", 0.04)
    start = signed_start_angle(trial_data["start_orient"], door)

    def key_step(cs):
        if cs is None:
            return 0.0
        left, right = cs.get("rotate_left"), cs.get("rotate_right")
        return speed if left and not right else -speed if right and not left else 0.0

    kept = [(frames[k]["state_read"], frames[k]["commands_sent"])
            for k in sorted(frames.keys(), key=int)
            if not frames[k]["state_read"].get("is_blank", False)]
    if not kept:
        return []

    # cost[s] = best total misfit of a path ending in sign s (0: +a, 1: -a)
    a0 = kept[0][0]["current_angle"]
    cost = [abs(wrap_angle(a0 - start)), abs(wrap_angle(-a0 - start))]
    back = []
    for i in range(1, len(kept)):
        prev_a, a = kept[i - 1][0]["current_angle"], kept[i][0]["current_angle"]
        steps = {0.0, key_step(kept[i - 1][1]), key_step(kept[i - 2][1] if i >= 2 else None)}
        new_cost, choice = [], []
        for c in (a, -a):
            options = []
            for s, p in enumerate((prev_a, -prev_a)):
                misfit = min(abs(wrap_angle(c - p - e)) for e in steps)
                options.append((cost[s] + misfit, s))
            best = min(options)
            new_cost.append(best[0])
            choice.append(best[1])
        cost = new_cost
        back.append(choice)

    signs = [0 if cost[0] <= cost[1] else 1]
    for choice in reversed(back):
        signs.append(choice[signs[-1]])
    signs.reverse()
    return [(sr["current_angle"] * (1 if s == 0 else -1), sr, cs)
            for s, (sr, cs) in zip(signs, kept)]


def angle_to_position(angle, object_type):
    """Convert a signed angle (reward always at angle 0) to a Type-Guided position."""
    snap = int(round(angle / SNAP)) % N_POS
    if object_type == "face":
        return (snap + 3) % N_POS
    return snap


def loaded_feature_for(level_nr):
    """Shape is the rewarded feature for levels 0–6, color for 7–8."""
    return "shape" if level_nr <= 6 else "color"


# server patch: FACE_AT_POSITION holds for the standard reward doors (2 corner, 5 face)
# only. In pyramid.rs face k is centred on door 2k - 1 (mod 6), and position p is the
# door (target_door - (p - reward_position)) mod 6, so for any reward door:
FACE_ON_DOOR = {5: 0, 1: 1, 3: 2}


def face_at_position(p, target_door=5):
    """Prism face at an odd position. Equals FACE_AT_POSITION[p] for doors 2 and 5."""
    reward = reward_position_for(object_type_from_door(target_door))
    return FACE_ON_DOOR[(target_door - (p - reward)) % N_POS]


def face_of_position(p, target_door=5):
    return face_at_position(p, target_door)


def position_of_face(face, target_door=5):
    return next(p for p in (1, 3, 5) if face_at_position(p, target_door) == face)


def train_test_for(level_nr):
    return "train" if level_nr in TRAIN_LEVELS else "test"


# ── Numeric analysis columns (ported from the MATLAB analysis script) ───────────

# Level 4's objects share one predicted type, so their order comes from the shapes.
LEVEL4_OBJECT_ORDER = {(2, 1, 1): 1, (3, 0, 0): 2}


def match_or_none(a, b):
    """1 if a == b, 0 if not, None if either side is missing."""
    return None if a is None or b is None else int(a == b)


def numeric_columns(row, shapes, chain=0):
    """
    Numeric codes built from one finished trial row. Types are 1 corner, 2 face.
    The loaded feature predicts the object's true type. The other feature predicts
    the same type when congruent and the opposite one when not, and nothing in
    levels 0-4, where features_congruent is blank.
    """
    object_type_nr = 1 if row["object_type"] == "corner" else 2
    loaded_feature_nr = 1 if row["loaded_feature"] == "shape" else 2
    congruent = row["features_congruent"]
    first_position = row["first_attempt_position"]
    first_type = {"corner": 1, "face": 2}.get(row["first_attempt_type"])

    other_type = None
    if congruent is not None:
        other_type = object_type_nr if congruent else object_type_nr % 2 + 1
    by_shape, by_color = ((object_type_nr, other_type) if loaded_feature_nr == 1
                          else (other_type, object_type_nr))
    by_shape_x_by_color = None
    if by_shape is not None:
        by_shape_x_by_color = by_shape if by_color is None else by_shape + 2 * (by_color - 1)

    position_of_type = {1: reward_position_for("corner"), 2: reward_position_for("face")}
    position_by_shape = position_of_type.get(by_shape)
    position_by_color = position_of_type.get(by_color)

    return {
        "object_type_nr":        object_type_nr,
        "loaded_feature_nr":     loaded_feature_nr,
        "features_congruent_nr": None if congruent is None else int(congruent),
        "first_attempt_type_nr": first_type,
        "first_attempt_position_nr": first_position,
        "object_type_by_shape":  by_shape,
        "object_type_by_color":  by_color,
        "object_type_by_shape_x_object_type_by_color": by_shape_x_by_color,
        # server patch: a level 4 with other objects than the standard ones is
        # ordered as in the trial file instead of failing the whole participant.
        "object_id_order_by_type": (LEVEL4_OBJECT_ORDER.get(tuple(shapes), chain + 1) if row["level_nr"] == 4
                                    else by_shape_x_by_color),
        "correct_position_by_shape": position_by_shape,
        "correct_position_by_color": position_by_color,
        "first_attempt_type_correct":          match_or_none(first_type, object_type_nr),
        "first_attempt_type_correct_by_shape": match_or_none(first_type, by_shape),
        "first_attempt_type_correct_by_color": match_or_none(first_type, by_color),
        "first_attempt_position_correct":          match_or_none(first_position, row["reward_position"]),
        "first_attempt_position_correct_by_shape": match_or_none(first_position, position_by_shape),
        "first_attempt_position_correct_by_color": match_or_none(first_position, position_by_color),
        "clock__trial_duration_log": (math.log10(row["clock__trial_duration"])
                                      if row["clock__trial_duration"] > 0 else None),
    }


def path_to_reward(pos, reward_pos):
    """Shortest path length + rotate key to press (left=ccw, right=cw, here=at reward)."""
    if pos == reward_pos:
        return 0, "here"
    steps_left = (reward_pos - pos) % N_POS       # going counterclockwise (+)
    steps_right = (pos - reward_pos) % N_POS      # going clockwise (-)
    if steps_left < steps_right:
        return steps_left, "left"
    if steps_right < steps_left:
        return steps_right, "right"
    return steps_left, "left"                     # tie (3 each) -> pick left


# ── Color registry (stable integer id per distinct RGB) ─────────────────────────

class ColorRegistry:
    """Rounds to 2 decimals so the 0.5686 / 0.5725 grays of Level 2 count as one color."""
    def __init__(self):
        self._map = {}

    def id(self, rgba):
        key = tuple(round(float(x), 2) for x in rgba[:3])
        if key not in self._map:
            self._map[key] = len(self._map)
        return self._map[key]


def feature_tuple(obj_cfg, feature, colors):
    """Return the 3-face token tuple for a feature of one object config."""
    if feature == "color":
        return tuple(colors.id(c) for c in obj_cfg["colors"])
    if feature == "shape":
        return tuple(obj_cfg["decorations_shape"])
    if feature == "texture":
        return tuple(obj_cfg["textures"])
    raise ValueError(feature)


# ── Feature-type classification (shared / unique / double / uninformative) ──────

def classify_feature(objects_tokens, is_level4):
    """
    objects_tokens : list of 3-tuples (one per object in the classification pool).
    Returns        : list (per object) of lists (per face) of category strings/None.

    Categories:
      uninformative — all 3 faces of the object share the same value
      shared        — value present in every object of the pool
      double        — value appears on >=2 faces of the same object
      unique        — appears on exactly one face, not shared
    Level 4 is special (no shared structure): doubles stay "double", the
    singleton face is None.
    """
    value_sets = [set(o) for o in objects_tokens]
    out = []
    for o in objects_tokens:
        counts = Counter(o)
        all_same = (o[0] == o[1] == o[2])
        row = []
        for v in o:
            if all_same:
                row.append("uninformative")
            elif is_level4:
                row.append("double" if counts[v] >= 2 else None)
            elif all(v in s for s in value_sets):
                row.append("shared")
            elif counts[v] >= 2:
                row.append("double")
            else:
                row.append("unique")
        out.append(row)
    return out


def group_key(level_nr):
    """Levels 0 and 1 are classified together (the object pair is split across them)."""
    return (0, 1) if level_nr in (0, 1) else (level_nr,)


def compute_classifications(level_objects, colors):
    """
    level_objects : {level_nr: [object_config, ...]}
    Returns       : {(level_nr, obj_idx, feature): [c0, c1, c2]}
    """
    result = {}
    # Build classification groups (pools).
    groups = defaultdict(list)          # group_key -> sorted list of level_nrs
    for lvl in sorted(level_objects):
        gk = group_key(lvl)
        if lvl not in groups[gk]:
            groups[gk].append(lvl)

    for gk, lvls in groups.items():
        lvls = sorted(lvls)
        pooled, index_map = [], {}
        for lvl in lvls:
            for oi, obj in enumerate(level_objects[lvl]):
                index_map[(lvl, oi)] = len(pooled)
                pooled.append(obj)
        is_level4 = (gk == (4,))
        for feature in ("color", "shape", "texture"):
            tokens = [feature_tuple(o, feature, colors) for o in pooled]
            cls = classify_feature(tokens, is_level4)
            for (lvl, oi), pidx in index_map.items():
                result[(lvl, oi, feature)] = cls[pidx]
    return result


def rule_position(tokens, pool, target_door=5):
    """
    Position a feature points at when read on its own (None = no rule).
      two faces match, one differs -> the odd face's own position
      all three differ             -> the corner opposite the marker face, the
                                      face whose value every object in the pool has
    """
    counts = Counter(tokens)
    if len(counts) == 1:
        return None
    if len(counts) == 2:
        odd = next(i for i, v in enumerate(tokens) if counts[v] == 1)
        return position_of_face(odd, target_door)
    shared = [i for i, v in enumerate(tokens) if all(v in o for o in pool)]
    return (position_of_face(shared[0], target_door) + 3) % N_POS if shared else None


# ── Loading ─────────────────────────────────────────────────────────────────────

def find_summaries(participant_dir):
    pattern = os.path.join(participant_dir, "**", "*_summary_*.json")
    files = [f for f in glob.glob(pattern, recursive=True)
             if "Zone.Identifier" not in f]
    summaries = []
    for fp in files:
        with open(fp) as f:
            summaries.append(json.load(f))
    # chronological: by level index, then by start timestamp
    summaries.sort(key=lambda s: (s["level_index"], s.get("timestamp_start", "")))
    return summaries


def build_file_index(participant_dir):
    """
    Index trial files two ways:
      by_name : basename -> full path
      by_run  : global run counter (from _run_RRRR_) -> full path

    The run-counter index is a robust fallback for sessions where the name
    prefix inside the summary's `file` field no longer matches the file on disk
    (e.g. a session recorded as "Mama" but later renamed to "200").
    """
    by_name, by_run = {}, {}
    run_re = re.compile(r"_run_(\d+)_")
    for root, _, files in os.walk(participant_dir):
        for name in files:
            if not name.endswith(".json") or "Zone.Identifier" in name:
                continue
            if "_summary_" in name:
                continue
            full = os.path.join(root, name)
            by_name[name] = full
            m = run_re.search(name)
            if m:
                by_run[int(m.group(1))] = full
    return by_name, by_run


# ── Position visits ──────────────────────────────────────────────────────────────

class Visit:
    __slots__ = ("position", "t_start", "t_last", "frames", "time_spent")

    def __init__(self, position, t_start):
        self.position = position
        self.t_start = t_start
        self.t_last = t_start
        self.frames = []          # list of (state_read, commands_sent)
        self.time_spent = 0.0


def build_visits(trial_data, object_type):
    """Segment non-blank frames into consecutive same-position visits (midpoint rule)."""
    visits = []
    cur = None
    for theta, sr, cs in reconstruct_frames(trial_data):
        pos = angle_to_position(theta, object_type)
        t = sr["present_elapsed_secs"]
        if cur is None or cur.position != pos:
            cur = Visit(pos, t)
            visits.append(cur)
        cur.frames.append((sr, cs))
        cur.t_last = t

    for i, v in enumerate(visits):
        if i < len(visits) - 1:
            v.time_spent = visits[i + 1].t_start - v.t_start
        else:
            v.time_spent = v.t_last - v.t_start
    return visits


def visit_movement_inversions(visit):
    dirs = []
    for sr, cs in visit.frames:
        d = "L" if cs.get("rotate_left") else ("R" if cs.get("rotate_right") else None)
        if d and (not dirs or dirs[-1] != d):
            dirs.append(d)
    return max(0, len(dirs) - 1)


def check_is_correct(sr, align_threshold):
    """
    Ground-truth win condition: a check counts as correct only when the prism is
    aligned with the reward (reward is always at angle 0, i.e. alignment = cos(angle)
    at or above the game's cosine_alignment_threshold).

    NOTE: the animation flags are NOT used for correctness. `animation_colored`
    marks a correct reveal, but `animation_all_door` is ambiguous — on certain
    chain positions it is the "all doors turn red" *error* feedback rather than a
    correct answer, so relying on it misclassifies wrong checks as correct.
    """
    return sr.get("current_alignment", -1.0) >= align_threshold


def scan_checks(checks, revealed):
    """
    Walk one trial's checks. `revealed` says whether the game has already shown this
    object's reward location. Chains start on steps where a wrong check lights the
    target door, which gives the location away, and end on show_all steps, where all
    doors light alike and nothing is given away.
    Returns (success_nr, success_unguided, revealed after the trial).
    """
    success_nr, unguided = -1, False
    for i, (_, _, correct, all_doors) in enumerate(checks):
        if correct:
            success_nr, unguided = i + 1, not revealed
            break
        if not all_doors:
            revealed = True
    return success_nr, unguided, revealed


def visit_attempts(visit, align_threshold):
    """(attempt_made, attempt_correct, attempt_feedback) for one visit."""
    made = False
    correct = None
    feedback = None
    for sr, cs in visit.frames:
        if cs.get("check"):
            made = True
            c = check_is_correct(sr, align_threshold)
            correct = c if correct is None else (correct or c)
            if c:
                feedback = "correct"
            elif feedback != "correct":
                feedback = "wrong_hidden" if cs.get("animation_all_door") else "wrong_revealed"
    return made, (correct if made else None), feedback


def collect_checks(visits, align_threshold):
    """Ordered list of (position, type, correct, all_doors) for every check across the trial."""
    checks = []
    for v in visits:
        for sr, cs in v.frames:
            if cs.get("check"):
                correct = check_is_correct(sr, align_threshold)
                checks.append((v.position, position_type(v.position), correct,
                               bool(cs.get("animation_all_door"))))
    return checks


# ── Trial context ────────────────────────────────────────────────────────────────

class TrialContext:
    """Everything needed to emit one trial row and its position rows."""
    def __init__(self, pid, summary, run_info, idx_in_level, trial_data,
                 classifications, colors, baseline_time, object_id, object_x_level_id,
                 rule_pool, chain_beaten=False):
        self.pid = pid
        self.summary = summary
        self.run_info = run_info
        self.idx_in_level = idx_in_level
        self.data = trial_data
        self.colors = colors
        self.object_id = object_id
        self.object_x_level_id = object_x_level_id
        self.baseline = baseline_time

        self.level_nr = trial_data["level_index"]
        self.active_chain = trial_data.get("active_chain", 0)
        cfg = trial_data["trial_config"]
        self.cfg = cfg
        self.target_door = cfg["target_door"]
        self.object_type = object_type_from_door(self.target_door)
        self.reward_position = reward_position_for(self.object_type)
        self.align_threshold = cfg.get("cosine_alignment_threshold", 0.95)

        # face feature tokens for this object
        self.color_tokens = [colors.id(c) for c in cfg["colors"]]
        self.shape_vals = list(cfg["decorations_shape"])
        self.texture_vals = list(cfg["textures"])
        self.cls = {
            "color":   classifications.get((self.level_nr, self.active_chain, "color"),   [None] * 3),
            "shape":   classifications.get((self.level_nr, self.active_chain, "shape"),   [None] * 3),
            "texture": classifications.get((self.level_nr, self.active_chain, "texture"), [None] * 3),
        }
        self.rule_color = rule_position(tuple(self.color_tokens), rule_pool["color"], self.target_door)   # server patch: door
        self.rule_shape = rule_position(tuple(self.shape_vals), rule_pool["shape"], self.target_door)

        self.visits = build_visits(trial_data, self.object_type)
        self.checks = collect_checks(self.visits, self.align_threshold)

        # Older games (pilot_Gymer) mark a finished chain with an index one past the
        # last trial. That run replays the last config row, so clamp to the trial
        # played. Newer games (kjpp) keep the index in range and log
        # trial_chain_completed instead; the caller turns that into chain_beaten.
        trials_cfg = summary.get("level_config", {}).get("trials", [])
        raw_step = trial_data.get("trial_index_in_chain", 0)
        self.chain_pos = min(raw_step, len(trials_cfg) - 1) if trials_cfg else raw_step
        self.chain_already_completed = bool(chain_beaten or raw_step >= len(trials_cfg))
        # the current game never stores an index past the end, so chain_beaten decides
        self.chain_cfg = trials_cfg[self.chain_pos] if trials_cfg else {}

    # timing relative to participant session start
    def t(self, present_elapsed):
        return present_elapsed - self.baseline

    def first_frame_time(self):
        for v in self.visits:
            if v.frames:
                return v.frames[0][0]["present_elapsed_secs"]
        return self.baseline

    def last_frame_time(self):
        for v in reversed(self.visits):
            if v.frames:
                return v.frames[-1][0]["present_elapsed_secs"]
        return self.baseline


# ── Row builders ─────────────────────────────────────────────────────────────────

TRIAL_COLUMNS = [
    "participant_id", "level_nr", "object_id", "object_x_level_id", "object_type",
    "reward_position", "starting_position", "loaded_feature", "train_test",
    "features_congruent", "trial_nr_in_level", "trial_nr_in_level_inv",
    "trial_nr_global", "trial_nr_global_inv", "levels_left",
    "object_rep_nr", "object_rep_nr_inv", "object_rep_nr_x_level", "object_rep_nr_x_level_inv",
    "positions_visited", "nr_position_visits", "nr_attempts",
    "first_attempt_position", "first_attempt_type", "attempts_positions", "attempts_types",
    "outcome__success", "outcome__success_nr", "outcome__success_unguided",
    "outcome__success_generalization", "outcome__chain_action",
    "correct_by_color", "correct_by_shape",
    "clock__trial_start_time", "clock__trial_end_time", "clock__trial_duration",
    "chain__step", "chain__already_completed", "chain__attempts_to_win",
    "chain__attempts_to_hint", "chain__attempts_to_retroceed", "chain__time_to_win",
    "chain__time_to_retroceed", "chain__reveals_on_wrong",
    "object_type_nr", "loaded_feature_nr", "features_congruent_nr",
    "first_attempt_type_nr", "first_attempt_position_nr",
    "object_type_by_shape", "object_type_by_color",
    "object_type_by_shape_x_object_type_by_color",
    "object_type_by_shape_x_object_type_by_color_x_level_nr",
    "object_id_order_by_type", "object_id_order_by_type_x_level_nr",
    "object_id_order_by_type_x_level_nr_x_participant_id",
    "correct_position_by_shape", "correct_position_by_color",
    "first_attempt_type_correct", "first_attempt_type_correct_by_shape",
    "first_attempt_type_correct_by_color",
    "first_attempt_position_correct", "first_attempt_position_correct_by_shape",
    "first_attempt_position_correct_by_color",
    "clock__trial_duration_log",
]

POSITION_ONLY_COLUMNS = [
    "color", "shape", "texture", "color_role", "shape_role", "texture_role",
    "color_left", "shape_left", "texture_left", "color_role_left", "shape_role_left",
    "color_right", "shape_right", "texture_right", "color_role_right", "shape_role_right",
    "position", "next_position", "previous_position",
    "all_previous_positions", "all_upcoming_positions", "nr_direction_changes",
    "nr_visits_in_trial", "nr_visits_in_session", "nr_visits_in_session_inv",
    "path_to_reward_len", "path_to_reward_dir",
    "nr_seen__color", "nr_seen__color_inv", "nr_seen__shape", "nr_seen__shape_inv",
    "attempt_made", "attempt_correct", "attempt_feedback", "clock__time_spent",
]

POSITION_COLUMNS = TRIAL_COLUMNS + POSITION_ONLY_COLUMNS


def build_trial_row(ctx, trial_nr, object_rep_nr, object_rep_nr_x_level, revealed):
    """`revealed` is the participant's set of object_x_level_ids whose reward location
    the game has shown. It is keyed per level, so a reveal carries across trials but
    NOT into a new level: a level 2 object starts unrevealed even if levels 0 or 1
    showed its location. Kept on purpose for now; see CLAUDE.md."""
    checks = ctx.checks
    nr_attempts = len(checks)
    success_nr, success_unguided, was_revealed = scan_checks(
        checks, ctx.object_x_level_id in revealed)
    if was_revealed:
        revealed.add(ctx.object_x_level_id)
    success = success_nr != -1

    # A participant who never pressed check has no guess to score.
    first = checks[0][0] if checks else None
    positions_visited = [v.position for v in ctx.visits]
    congruent = None
    if ctx.rule_color is not None and ctx.rule_shape is not None:
        congruent = ctx.rule_color == ctx.rule_shape

    row = {
        "participant_id":     ctx.pid,
        "level_nr":           ctx.level_nr,
        "object_id":          ctx.object_id,
        "object_x_level_id":  ctx.object_x_level_id,
        "object_type":        ctx.object_type,
        "reward_position":    ctx.reward_position,
        "starting_position":  ctx.visits[0].position if ctx.visits else None,
        "loaded_feature":     loaded_feature_for(ctx.level_nr),
        "train_test":         train_test_for(ctx.level_nr),
        "features_congruent": congruent,
        "trial_nr_in_level":  trial_nr,
        "object_rep_nr":      object_rep_nr,
        "object_rep_nr_x_level": object_rep_nr_x_level,
        "positions_visited":  positions_visited,
        "nr_position_visits": len(positions_visited),
        "nr_attempts":        nr_attempts,
        "first_attempt_position": first,
        "first_attempt_type": checks[0][1] if checks else None,
        "attempts_positions": [c[0] for c in checks],
        "attempts_types":     [c[1] for c in checks],
        "outcome__success":   success,
        "outcome__success_nr": success_nr,
        "outcome__success_unguided": success_unguided,
        # Unguided success on the participant's first ever sighting of this object.
        "outcome__success_generalization": bool(success_unguided and object_rep_nr == 0),
        "outcome__chain_action": ctx.run_info.get("outcome", ctx.data.get("outcome")),
        "correct_by_color":   (first == ctx.rule_color) if checks and ctx.rule_color is not None else None,
        "correct_by_shape":   (first == ctx.rule_shape) if checks and ctx.rule_shape is not None else None,
        "clock__trial_start_time": round(ctx.t(ctx.first_frame_time()), 4),
        "clock__trial_end_time":   round(ctx.t(ctx.last_frame_time()), 4),
        "clock__trial_duration":   round(ctx.last_frame_time() - ctx.first_frame_time(), 4),
        "chain__step":        ctx.chain_pos,
        "chain__already_completed": ctx.chain_already_completed,
        "chain__attempts_to_win":   ctx.chain_cfg.get("nr_attempts_to_win"),
        "chain__attempts_to_hint":  ctx.chain_cfg.get("nr_attempts_suggestion"),
        "chain__attempts_to_retroceed": ctx.chain_cfg.get("nr_attempts_to_retroceed"),
        "chain__time_to_win":       ctx.chain_cfg.get("elapsed_time_to_win"),
        "chain__time_to_retroceed": ctx.chain_cfg.get("elapsed_time_to_retroceed"),
        "chain__reveals_on_wrong":  not ctx.chain_cfg.get("show_all", False),
    }
    row.update(numeric_columns(row, ctx.shape_vals, ctx.active_chain))   # server patch: chain
    return row


def build_position_rows(ctx, trial_row, pstate):
    """
    Emit one row per visit. pstate holds per-participant running counters:
      pstate.visits_total : {position: count}
      pstate.seen_color   : {color_token: count}
      pstate.seen_shape   : {shape_value: count}
    """
    visits = ctx.visits
    positions = [v.position for v in visits]
    rows = []
    trial_visit_count = defaultdict(int)

    for i, v in enumerate(visits):
        p = v.position
        trial_visit_count[p] += 1
        pstate.visits_total[p] += 1

        made, correct, feedback = visit_attempts(v, ctx.align_threshold)
        plen, pdir = path_to_reward(p, ctx.reward_position)

        row = dict(trial_row)              # inherit all trial columns
        row.update({
            "position":            p,
            "clock__time_spent":   round(v.time_spent, 4),
            "next_position":       positions[i + 1] if i + 1 < len(positions) else None,
            "previous_position":   positions[i - 1] if i > 0 else None,
            "all_previous_positions": positions[:i],
            "all_upcoming_positions": positions[i + 1:],
            "nr_direction_changes": visit_movement_inversions(v),
            "nr_visits_in_trial":  trial_visit_count[p],
            "nr_visits_in_session": pstate.visits_total[p],
            "path_to_reward_len":  plen,
            "path_to_reward_dir":  pdir,
            "attempt_made":        made,
            "attempt_correct":     correct,
            "attempt_feedback":    feedback,
        })

        if is_corner_position(p):
            lf = face_at_position(screen_left_neighbor(p), ctx.target_door)   # server patch: door
            rf = face_at_position(screen_right_neighbor(p), ctx.target_door)
            row.update({
                "color_left":  ctx.color_tokens[lf], "shape_left":  ctx.shape_vals[lf],
                "texture_left": ctx.texture_vals[lf],
                "color_role_left": ctx.cls["color"][lf], "shape_role_left": ctx.cls["shape"][lf],
                "color_right": ctx.color_tokens[rf], "shape_right": ctx.shape_vals[rf],
                "texture_right": ctx.texture_vals[rf],
                "color_role_right": ctx.cls["color"][rf], "shape_role_right": ctx.cls["shape"][rf],
            })
        else:
            f = face_at_position(p, ctx.target_door)   # server patch: door
            ctok, sval = ctx.color_tokens[f], ctx.shape_vals[f]
            row.update({
                "color": ctok, "shape": sval, "texture": ctx.texture_vals[f],
                "color_role": ctx.cls["color"][f], "shape_role": ctx.cls["shape"][f],
                "texture_role": ctx.cls["texture"][f],
                # cross-trial memory: count BEFORE this visit, then increment
                "nr_seen__color": pstate.seen_color[ctok],
                "nr_seen__shape": pstate.seen_shape[sval],
            })
            pstate.seen_color[ctok] += 1
            pstate.seen_shape[sval] += 1

        rows.append(row)
    return rows


# ── Per-participant running state ────────────────────────────────────────────────

class ParticipantState:
    def __init__(self):
        self.visits_total = defaultdict(int)
        self.seen_color = defaultdict(int)
        self.seen_shape = defaultdict(int)
        self.obj_trial_count = defaultdict(int)        # object_id -> prior trial count
        self.obj_level_trial_count = defaultdict(int)  # object_x_level_id -> prior trial count
        self.revealed = set()                          # object_x_level_ids whose reward was shown
        self.chain_beaten = defaultdict(bool)          # (level_nr, chain) -> last step beaten


def update_chain_beaten(beaten, trial_data):
    """
    Newer games only. The game's chainLastBeaten flag: set by the run that beats the
    last step (trial_chain_completed), cleared by a retroceed, kept by a stay.
    """
    if trial_data.get("trial_chain_completed"):
        return True
    if trial_data.get("outcome") == "retroceed":
        return False
    return beaten


# ── Object id registry ───────────────────────────────────────────────────────────

class ObjectRegistry:
    """object_id per visual object, object_x_level_id per object within a level."""
    def __init__(self, colors):
        self._map = {}
        self._level_map = {}
        self.colors = colors

    def _sig(self, cfg):
        return (
            tuple(self.colors.id(c) for c in cfg["colors"]),
            tuple(cfg["textures"]),
            tuple(cfg["decorations_shape"]),
        )

    def id(self, cfg):
        sig = self._sig(cfg)
        if sig not in self._map:
            self._map[sig] = len(self._map)
        return self._map[sig]

    def x_level_id(self, level_nr, cfg):
        key = (level_nr, self._sig(cfg))
        if key not in self._level_map:
            self._level_map[key] = len(self._level_map)
        return self._level_map[key]


# ── Main processing ──────────────────────────────────────────────────────────────

def discover_participants(data_dir):
    out = []
    for entry in os.listdir(data_dir):
        full = os.path.join(data_dir, entry)
        if os.path.isdir(full):
            # "1_2026-06-01_14-15-59" -> "1"; "kjpp_vrsafe_001" has no date -> itself
            pid = re.sub(r"_\d{4}-\d{2}-\d{2}.*$", "", entry)
            out.append((pid, full))

    def sort_key(item):
        pid = item[0]
        return (0, int(pid)) if pid.isdigit() else (1, pid)

    out.sort(key=sort_key)
    return out


def add_backward_counters(trial_rows, position_rows):
    """Fill the *_inv columns and levels_left, which need one participant's full session."""
    level_runs = Counter(r["level_nr"] for r in trial_rows)
    obj_total = Counter(r["object_id"] for r in trial_rows)
    obj_level_total = Counter(r["object_x_level_id"] for r in trial_rows)
    last_level = max(level_runs)
    n_trials = len(trial_rows)
    for i, r in enumerate(trial_rows):
        r["trial_nr_global"] = i
        r["trial_nr_global_inv"] = n_trials - 1 - i
        r["trial_nr_in_level_inv"] = level_runs[r["level_nr"]] - 1 - r["trial_nr_in_level"]
        r["levels_left"] = last_level - r["level_nr"]
        r["object_rep_nr_inv"] = obj_total[r["object_id"]] - 1 - r["object_rep_nr"]
        r["object_rep_nr_x_level_inv"] = (obj_level_total[r["object_x_level_id"]] - 1
                                          - r["object_rep_nr_x_level"])

    visits_total = Counter(r["position"] for r in position_rows)
    color_total = Counter(r["color"] for r in position_rows if "color" in r)
    shape_total = Counter(r["shape"] for r in position_rows if "shape" in r)
    by_run = {(r["level_nr"], r["trial_nr_in_level"]): r for r in trial_rows}
    for r in position_rows:
        r.update({k: by_run[(r["level_nr"], r["trial_nr_in_level"])][k]
                  for k in ("trial_nr_global", "trial_nr_global_inv", "trial_nr_in_level_inv",
                            "levels_left", "object_rep_nr_inv", "object_rep_nr_x_level_inv")})
        r["nr_visits_in_session_inv"] = visits_total[r["position"]] - r["nr_visits_in_session"]
        if "color" in r:
            r["nr_seen__color_inv"] = color_total[r["color"]] - 1 - r["nr_seen__color"]
            r["nr_seen__shape_inv"] = shape_total[r["shape"]] - 1 - r["nr_seen__shape"]


def add_ranked_ids(trial_rows, position_rows):
    """
    Fill the ids that are ranks across the whole dataset, so they need all participants.
    Rows must arrive in participant order. Ranks run from 1, sorted by level, then code,
    then participant, as MATLAB's unique does when participants are numbered from 1.
    """
    def rank(keys):
        return {k: i + 1 for i, k in enumerate(sorted(set(keys)))}

    participant_order = {p: i for i, p in enumerate(dict.fromkeys(r["participant_id"] for r in trial_rows))}

    def object_key(r):
        return (r["level_nr"], r["object_id_order_by_type"])

    def object_participant_key(r):
        return (*object_key(r), participant_order[r["participant_id"]])

    type_ranks = rank((r["level_nr"], r["object_type_by_shape_x_object_type_by_color"])
                      for r in trial_rows)
    object_ranks = rank(object_key(r) for r in trial_rows)
    object_participant_ranks = rank(object_participant_key(r) for r in trial_rows)

    for r in trial_rows + position_rows:
        r["object_type_by_shape_x_object_type_by_color_x_level_nr"] = type_ranks[
            (r["level_nr"], r["object_type_by_shape_x_object_type_by_color"])]
        r["object_id_order_by_type_x_level_nr"] = object_ranks[object_key(r)]
        r["object_id_order_by_type_x_level_nr_x_participant_id"] = (
            object_participant_ranks[object_participant_key(r)])


class GameVersionError(ValueError):
    pass


def check_game_version(trial_data, legacy_game, path):
    """Current games log trial_chain_completed; the legacy game (pilot_Gymer) does not."""
    current = "trial_chain_completed" in trial_data
    if current and legacy_game:
        raise GameVersionError(f"{path} is from the current game version; drop --legacy-game.")
    if not current and not legacy_game:
        raise GameVersionError(f"{path} has no trial_chain_completed, so it is from the outdated "
                               "game version (as pilot_Gymer). Rerun with --legacy-game.")


def process_participant(pid, participant_dir, colors, objreg, trial_rows, position_rows,
                        legacy_game=False):
    summaries = find_summaries(participant_dir)
    if not summaries:
        return 0
    file_by_name, file_by_run = build_file_index(participant_dir)

    # Objects per level (for feature classification).
    level_objects = {}
    for s in summaries:
        lvl = s["level_index"]
        objs = s.get("level_config", {}).get("objects", [])
        if lvl not in level_objects and objs:
            level_objects[lvl] = objs
    classifications = compute_classifications(level_objects, colors)

    # Objects each level's rule is read against (same pools as the classification).
    rule_pools = {}
    for lvl in level_objects:
        pool = [o for l in level_objects if group_key(l) == group_key(lvl) for o in level_objects[l]]
        rule_pools[lvl] = {
            "color": [set(feature_tuple(o, "color", colors)) for o in pool],
            "shape": [set(feature_tuple(o, "shape", colors)) for o in pool],
        }

    # All runs across all levels, in global chronological order.
    runs = []
    for s in summaries:
        for idx_in_level, run_info in enumerate(s["trials_runs"]):
            runs.append((s, idx_in_level, run_info))
    runs.sort(key=lambda r: r[2].get("trial_run_counter", 0))

    pstate = ParticipantState()
    baseline = None
    my_trials, my_positions = [], []

    for summary, idx_in_level, run_info in runs:
        fname = run_info.get("file")
        path = file_by_name.get(fname)
        if path is None:
            path = file_by_run.get(run_info.get("trial_run_counter"))
        if path is None:
            continue
        with open(path) as f:
            trial_data = json.load(f)
        check_game_version(trial_data, legacy_game, path)

        cfg = trial_data["trial_config"]
        level_nr = trial_data["level_index"]
        object_id = objreg.id(cfg)
        object_x_level_id = objreg.x_level_id(level_nr, cfg)
        chain_key = (level_nr, trial_data.get("active_chain", 0))
        ctx = TrialContext(pid, summary, run_info, idx_in_level, trial_data,
                           classifications, colors, baseline or 0.0, object_id,
                           object_x_level_id, rule_pools[level_nr],
                           pstate.chain_beaten[chain_key])
        pstate.chain_beaten[chain_key] = update_chain_beaten(pstate.chain_beaten[chain_key],
                                                             trial_data)

        # establish session baseline from the first processed trial
        if baseline is None:
            baseline = ctx.first_frame_time()
            ctx.baseline = baseline

        trial_row = build_trial_row(ctx, idx_in_level,
                                    pstate.obj_trial_count[object_id],
                                    pstate.obj_level_trial_count[object_x_level_id],
                                    pstate.revealed)
        pstate.obj_trial_count[object_id] += 1
        pstate.obj_level_trial_count[object_x_level_id] += 1

        my_trials.append(trial_row)
        my_positions.extend(build_position_rows(ctx, trial_row, pstate))

    add_backward_counters(my_trials, my_positions)
    trial_rows.extend(my_trials)
    position_rows.extend(my_positions)
    return len(my_trials)


def serialize(value):
    if value is None:
        return ""
    if isinstance(value, (list, tuple)):
        return json.dumps(list(value))
    if isinstance(value, bool):
        return "True" if value else "False"
    return value


def write_csv(path, columns, rows):
    with open(path, "w", newline="") as f:
        writer = csv.DictWriter(f, fieldnames=columns, extrasaction="ignore")
        writer.writeheader()
        for row in rows:
            writer.writerow({c: serialize(row.get(c)) for c in columns})


def build_project(project_dir, out_subdir="out_rebuilt", legacy_game=False):
    data_dir = os.path.join(project_dir, "data")
    if not os.path.isdir(data_dir):
        raise FileNotFoundError(f"No data/ folder found in {project_dir}")

    out_dir = os.path.join(project_dir, out_subdir)
    os.makedirs(out_dir, exist_ok=True)

    colors = ColorRegistry()
    objreg = ObjectRegistry(colors)
    trial_rows, position_rows = [], []

    participants = discover_participants(data_dir)
    print(f"Found {len(participants)} participant(s) in {data_dir}")
    for pid, pdir in participants:
        n = process_participant(pid, pdir, colors, objreg, trial_rows, position_rows,
                                legacy_game)
        print(f"  [{pid}] {n} trials")
    add_ranked_ids(trial_rows, position_rows)

    trials_path = os.path.join(out_dir, "trials.csv")
    positions_path = os.path.join(out_dir, "positions.csv")
    write_csv(trials_path, TRIAL_COLUMNS, trial_rows)
    write_csv(positions_path, POSITION_COLUMNS, position_rows)

    print(f"\nWrote {len(trial_rows)} trial rows    -> {trials_path}")
    print(f"Wrote {len(position_rows)} position rows -> {positions_path}")
    return trials_path, positions_path


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description="Build trial- and position-level CSV tables.")
    ap.add_argument("--project", "-P", required=True,
                    help="Project folder containing data/, e.g. kjpp or pilot_Gymer")
    ap.add_argument("--legacy-game", action="store_true",
                    help="Data is from the outdated game version of May-June 2026 (pilot_Gymer)")
    ap.add_argument("--out", "-o", default="out_rebuilt",
                    help="Output subfolder name (default: out_rebuilt)")
    args = ap.parse_args()
    try:
        build_project(args.project, out_subdir=args.out, legacy_game=args.legacy_game)
    except GameVersionError as e:
        raise SystemExit(f"Game version mismatch: {e}")
