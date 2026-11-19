"""Create the MAIN sampling figures (how participants turned the objects). Run from the project root:

    python make_MAIN_sampling_figures.py

Writes (PNG, 300 dpi, project root):
    MAIN_sampling_participant_example.png        one participant (EXAMPLE below), every trial as a spiral
    MAIN_sampling_group_cells_<project>.png       object x round cells, each participant at a fixed spot in the cell
    MAIN_sampling_group_matrix_<project>.png      participants (rows) x levels (columns); rounds run left to right

Needs numpy, pandas, matplotlib, and build_dataset.py and make_MAIN_figures.py from this folder (data loading,
rounds, colours, object layout). Reads the trial tables in <project>/out_rebuilt and the raw trial JSONs in
<project>/data.

How a trial is drawn
- Angles come from build_dataset.reconstruct_frames: signed angle theta, reward at theta = 0, positions at k*60 deg.
- Every trial is rotated so the rewarded position points up. Drawn at phi = 90 deg - theta, so with the reward in
  front, the element on the left of the screen (position p - 1, DATA_STRUCTURE.md) is on the left; rotate_left
  (which increases theta) therefore runs clockwise.
- Radius = time within the trial, scaled per trial from the start (inner end) to the last check (outer end).
- Line colour = direction of turning (blue = left key, terracotta = right key); pauses keep the last direction.
- Checks: green = correct, yellow = wrong position but right kind (corner/face), red = wrong kind.
"""
import glob
import json
import math
import os
import re

import numpy as np
import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.collections import LineCollection, PatchCollection
from matplotlib.lines import Line2D
from matplotlib.patches import FancyBboxPatch, Rectangle
from matplotlib.path import Path

import build_dataset as B
import make_MAIN_figures as M

EXAMPLE = ("pilot_Gymer", "7")            # participant for MAIN_sampling_participant_example.png
CAP = 15                                 # rounds shown in the group figures
LEFT_C, RIGHT_C = "#4d7fb3", "#cf8a55"   # turning left / right
KIND_COL = {"ok": M.GREEN, "type_ok": M.YELLOW, "wrong": M.RED}
INK, MUTED = M.INK, M.MUTED
STOP_BG = "#fbefc4"                      # last trial, ended by the time limit


# ============================================================================ data
def file_index(proj):
    """(participant, level, index in level) -> trial JSON path, via the level summaries (as build_dataset does)."""
    out = {}
    for d in sorted(os.listdir(f"{proj}/data")):
        full = f"{proj}/data/{d}"
        if not os.path.isdir(full):
            continue
        pid = re.sub(r"_\d{4}-\d{2}-\d{2}.*$", "", d)
        by_name, by_run = B.build_file_index(full)
        for s in B.find_summaries(full):
            for i, run in enumerate(s["trials_runs"]):
                path = by_name.get(run.get("file")) or by_run.get(run.get("trial_run_counter"))
                if path:
                    out[(pid, s["level_index"], i)] = path
    return out


def trajectory(path):
    """Signed angle and time (s from trial start) per frame up to the last check; checks as (angle, time, kind)."""
    d = json.load(open(path))
    cfg = d["trial_config"]
    obj_type = B.object_type_from_door(cfg["target_door"])
    thr = cfg.get("cosine_alignment_threshold", 0.95)
    frames = B.reconstruct_frames(d)
    t0 = frames[0][1]["present_elapsed_secs"]
    th, t, checks = [], [], []
    for theta, sr, cs in frames:
        th.append(theta); t.append(sr["present_elapsed_secs"] - t0)
        if cs.get("check"):
            pos = B.angle_to_position(theta, obj_type)
            kind = ("ok" if B.check_is_correct(sr, thr)
                    else "type_ok" if B.position_type(pos) == obj_type else "wrong")
            checks.append((theta, t[-1], kind))
    th, t = np.array(th), np.array(t)
    t_end = checks[-1][1] if checks else t[-1]
    keep = t <= t_end
    return dict(th=th[keep], t=t[keep], checks=checks, dur=t_end, cfg=cfg, obj_type=obj_type)


def load(proj, pids=None):
    """Trial table (with rounds) plus the trajectory of every trial (optionally only some participants)."""
    D = M.load(proj, dict(M.PROJECTS)[proj])
    files = file_index(proj)
    T = D["T"] if pids is None else D["T"][D["T"].participant_id.isin(pids)]
    D["traj"] = {r.Index: trajectory(files[(r.participant_id, r.level_nr, r.trial_nr_in_level)])
                 for r in T.itertuples()}
    last = D["T"].sort_values("trial_nr_global").groupby("participant_id").tail(1)
    D["stopped"] = {r.participant_id: (r.trial_nr_global, r.level_nr) for r in last.itertuples() if not r.level_done}
    return D


# ============================================================================ drawing helpers
def turn_colours(th):
    """One colour per segment from the turning direction; pauses keep the last direction."""
    d = np.array([math.remainder(b - a, 2 * math.pi) for a, b in zip(th[:-1], th[1:])])
    sign, last = np.zeros(len(d)), 0.0
    for i, v in enumerate(d):
        if abs(v) > 1e-6:
            last = np.sign(v)
        sign[i] = last
    if (sign == 0).any():
        first = next((v for v in sign if v != 0), 1.0)
        sign[sign == 0] = first
    return [LEFT_C if v > 0 else RIGHT_C for v in sign]


def spiral(tr, cx, cy, r_in, r_out):
    """Segments, segment colours and check dots of one trial drawn around (cx, cy)."""
    th, t, dur = tr["th"], tr["t"], tr["dur"]
    frac = t / dur if dur > 0 else np.zeros_like(t)
    rad = r_in + (r_out - r_in) * frac
    phi = math.pi / 2 - th
    px, py = cx + rad * np.cos(phi), cy + rad * np.sin(phi)
    seg = (np.stack([np.column_stack([px[:-1], py[:-1]]), np.column_stack([px[1:], py[1:]])], axis=1)
           if len(px) > 1 else np.zeros((0, 2, 2)))
    dots = []
    for theta, tc, kind in tr["checks"]:
        rr = r_in + (r_out - r_in) * (tc / dur if dur > 0 else 1.0)
        dots.append((cx + rr * math.cos(math.pi / 2 - theta), cy + rr * math.sin(math.pi / 2 - theta), KIND_COL[kind]))
    return seg, (turn_colours(th) if len(px) > 1 else []), dots, (px[0], py[0]) if len(px) else (cx, cy)


def _poly(pts):
    pts = list(pts) + [pts[0]]
    return Path(pts, [Path.MOVETO] + [Path.LINETO] * (len(pts) - 2) + [Path.CLOSEPOLY])


_t = np.linspace(0, 2 * np.pi, 40, endpoint=False)
_h = np.linspace(0, np.pi, 24)
SHAPE_MK = {                                            # DecorationShape enum (shared/src/lib.rs)
    0: ("o", 1.0), 1: ("s", 0.9), 2: ("*", 1.35), 3: ("^", 1.0),
    4: (_poly([(-1, -0.45), (1, -0.45), (1, 0.45), (-1, 0.45)]), 1.05),             # rectangle
    5: (_poly(list(zip(np.cos(_t), 0.6 * np.sin(_t)))), 1.05),                      # oval
    6: ("p", 1.05), 7: (_poly([(0, 1), (0.6, 0.25), (0, -1), (-0.6, 0.25)]), 1.1),   # pentagon, kite
    8: ("D", 0.85),                                                                   # rhombus
    9: (_poly([(-1, -0.6), (1, -0.6), (0.5, 0.6), (-0.5, 0.6)]), 1.05),             # trapezoid
    10: (_poly(list(zip(np.cos(_h), np.sin(_h) - 0.4))), 1.05),                      # semicircle
}


def legend_core(lw=1.2, ms=3):
    return [Line2D([], [], color=LEFT_C, lw=lw, label="Turning left"),
            Line2D([], [], color=RIGHT_C, lw=lw, label="Turning right"),
            Line2D([], [], color=M.GREEN, lw=1.0, label="Rewarded direction (up)"),
            Line2D([], [], ls="", marker="o", ms=ms, color=M.GREEN, label="Check correct"),
            Line2D([], [], ls="", marker="o", ms=ms, color=M.YELLOW, label="Wrong position, right kind"),
            Line2D([], [], ls="", marker="o", ms=ms, color=M.RED, label="Wrong kind"),
            Line2D([], [], ls="", marker="s", ms=6, mfc="none", mec=INK, mew=0.35, label="Chain completed"),
            Line2D([], [], ls="", marker="s", ms=6, color=STOP_BG, label="Last trial (time limit)")]


# ============================================================================ 1. one participant
def participant_figure(proj, pid, D, out):
    R_IN, R_OUT, R_POS, HALF = 0.13, 0.335, 0.40, 0.46
    SPOKE, TILE_C, FACE_MS, FAINT = "#d3d3ce", "#f1f1ee", 2.6, 0.28
    T = D["T"][D["T"].participant_id == pid].sort_values("trial_nr_global")
    slots, x, bounds, xmax = M.timeline_layout(T)
    ymax = int(T.y.max())
    status = {l: v for (p, l), v in D["status"].items() if p == pid}
    stop_trial = D["stopped"].get(pid, (None,))[0]

    fig, ax = plt.subplots(figsize=(17, 1.9 + 0.5 * ymax))
    for L, a, b in bounds[1:]:
        ax.plot([a - 0.15, a - 0.15], [0.5, ymax + 0.5], color="#e2e2de", lw=0.6, zorder=0)
    for r in T.itertuples():
        cx, cy = x[(r.level_nr, r.slot)], r.y
        tr = D["traj"][r.Index]
        outline = MUTED if r.noguess else M.GREEN if r.ok else M.YELLOW if r.type_ok else M.RED   # first check
        ax.add_patch(FancyBboxPatch((cx - HALF, cy - HALF), 2 * HALF, 2 * HALF, boxstyle="round,pad=0,rounding_size=0.06",
                                    fc=STOP_BG if r.trial_nr_global == stop_trial else TILE_C, ec=outline, lw=0.75,
                                    zorder=1))
        offset = 0 if tr["obj_type"] == "corner" else 3   # theta = k*60 deg is position k (+3 for face objects)
        for k in range(6):                                # spokes, faces (faint) and corners
            a = math.pi / 2 - k * math.pi / 3
            ax.plot([cx, cx + R_OUT * math.cos(a)], [cy, cy + R_OUT * math.sin(a)], color=M.GREEN if k == 0 else SPOKE,
                    lw=0.6 if k == 0 else 0.35, alpha=0.7 if k == 0 else 1, zorder=2, solid_capstyle="butt")
            p = (k + offset) % 6
            mx, my = cx + R_POS * math.cos(a), cy + R_POS * math.sin(a)
            if B.is_corner_position(p):
                ax.plot(mx, my, "o", ms=1.2, color=M.GREEN if k == 0 else "#c4c4bf", mec="none", zorder=2)
            else:
                f = B.face_at_position(p, tr["cfg"]["target_door"])   # server patch: any reward door
                mk, sc = SHAPE_MK[tr["cfg"]["decorations_shape"][f]]
                ax.plot(mx, my, ls="", marker=mk, ms=FACE_MS * sc, mfc=tuple(tr["cfg"]["colors"][f][:3]),
                        mec="none", alpha=FAINT, zorder=2)
        seg, cols, dots, start = spiral(tr, cx, cy, R_IN, R_OUT)
        if len(seg):
            ax.add_collection(LineCollection(seg, colors=cols, lw=1.15, capstyle="round", zorder=3))
        ax.plot(*start, "o", ms=2.4, color="#555555", mec="white", mew=0.3, zorder=4)
        for qx, qy, c in dots:
            ax.plot(qx, qy, "o", ms=4.0, color=c, mec="white", mew=0.5, zorder=9)
        if r.completes:                                   # chain completed: black line just outside the outline
            g = 0.03
            ax.add_patch(FancyBboxPatch((cx - HALF - g, cy - HALF - g), 2 * (HALF + g), 2 * (HALF + g),
                                        boxstyle="round,pad=0,rounding_size=0.08", fc="none", ec=INK, lw=0.85,
                                        zorder=1))
        ax.text(cx - 0.425, cy + 0.425, str(r.pos_in_round), ha="left", va="top", fontsize=5, color="#9a9a96",
                zorder=6)

    ax.set_xlim(-0.2, xmax + 0.2); ax.set_ylim(0.45, ymax + 0.75); ax.set_aspect("equal", adjustable="box")
    ax.set_yticks(range(1, ymax + 1)); ax.set_ylabel("Round (= times the object was played in the level)", fontsize=8)
    for sp in ("top", "right", "bottom"):
        ax.spines[sp].set_visible(False)
    ax.spines["left"].set_color(MUTED); ax.tick_params(axis="y", colors=MUTED, labelcolor=INK, labelsize=7)
    ax.set_xticks([x[k] for k in slots])
    ax.set_xticklabels([("corner\n" + M.TY[1]) if s[0] == 1 else ("face\n" + M.TY[2]) for _, s in slots],
                       fontsize=7, linespacing=1.25)
    ax.tick_params(axis="x", length=0, pad=3)
    for L, s in slots:
        if L >= 5:
            ax.annotate("cong." if s[1] == 0 else "incong.", xy=(x[(L, s)], 0), xycoords=("data", "axes fraction"),
                        xytext=(0, -26), textcoords="offset points", ha="center", va="top", fontsize=6.5, color=MUTED)
    for L, a, b in bounds:
        ax.annotate("", xy=(a + 0.05, ymax + 0.62), xytext=(b - 0.05, ymax + 0.62), xycoords="data",
                    arrowprops=dict(arrowstyle="-", color=MUTED, lw=0.6), annotation_clip=False)
        ax.text((a + b) / 2, ymax + 0.78, f"Level {L}" + ("" if status.get(L) == "completed" else " (not finished)"),
                ha="center", va="bottom", fontsize=8.5, fontweight="bold", clip_on=False)
    ax.set_title(f"How the objects were turned, {proj} participant {M.short(pid)}", loc="left", fontsize=11,
                 fontweight="bold", pad=34)
    h = [Line2D([], [], color=LEFT_C, lw=1.3, label="Turning left (key)"),
         Line2D([], [], color=RIGHT_C, lw=1.3, label="Turning right (key)"),
         Line2D([], [], ls="", marker="o", ms=3, color="#555555", mec="white", mew=0.3,
                label="Start (inner end); outer end = last check"),
         Line2D([], [], color=M.GREEN, lw=1.0, label="Rewarded position (always at the top)"),
         Line2D([], [], color=SPOKE, lw=0.8, marker="o", ms=1.8, mfc="#a9a9a4", mec="none", markevery=[1],
                label="Other positions (dot = corner)"),
         Line2D([], [], ls="", marker="*", ms=6.5, mfc="#8f8f8f", mec="none", alpha=FAINT + 0.15,
                label="Face: its shape in its colour (faint)"),
         Line2D([], [], ls="", marker="o", ms=5, color=M.GREEN, mec="white", label="Check: correct"),
         Line2D([], [], ls="", marker="o", ms=5, color=M.YELLOW, mec="white", label="Check: wrong position, right kind"),
         Line2D([], [], ls="", marker="o", ms=5, color=M.RED, mec="white", label="Check: wrong kind (corner vs. face)"),
         Line2D([], [], ls="", marker="s", ms=9, mfc=TILE_C, mec=M.GREEN, mew=0.8, label="Outline: first check correct"),
         Line2D([], [], ls="", marker="s", ms=9, mfc=TILE_C, mec=M.YELLOW, mew=0.8,
                label="Outline: first check wrong position, right kind"),
         Line2D([], [], ls="", marker="s", ms=9, mfc=TILE_C, mec=M.RED, mew=0.8, label="Outline: first check wrong kind"),
         Line2D([], [], ls="", marker="s", ms=10, mfc="none", mec=INK, mew=0.85, label="Outer black line: chain completed"),
         Line2D([], [], ls="", marker="s", ms=9, mfc=STOP_BG, mec="none", label="Last trial (time limit)"),
         Line2D([], [], ls="", marker="$2$", ms=5, color=MUTED, label="Order within the round")]
    ax.legend(handles=h, loc="upper left", bbox_to_anchor=(1.005, 1.0), frameon=False, fontsize=7.5, labelspacing=0.9)
    fig.text(0.01, 0.01, "Each cell is one trial, turned so that the rewarded position points up; the six spokes are the "
             "six positions (faces show their shape in their colour); left and right are as on the screen when the reward "
             "faces the participant (turning left runs clockwise). Radius = time within the trial, scaled per trial from "
             "start (inner end) to the last check (outer end).", fontsize=6.5, color=MUTED)
    fig.tight_layout(rect=(0, 0.03, 1, 1))
    fig.savefig(out, dpi=300)
    plt.close(fig)


# ============================================================================ 2. group: participants inside each cell
def group_cells_figure(proj, D, out):
    CELL, CELL_BG, SPOT_BG = 0.94, "#f3f3f0", "#ffffff"
    TT = D["T"]; T = TT[TT.y <= CAP]
    slots, x, bounds, xmax = M.timeline_layout(TT)
    order = D["order"]; g = int(np.ceil(np.sqrt(len(order))))
    spot = {p: (k % g, k // g) for k, p in enumerate(order)}        # furthest first: bottom left, rows upwards
    step = CELL / g
    r_out, r_in = 0.43 * step, 0.15 * step
    stopped = D["stopped"]
    ymax = min(CAP, int(T.y.max()))

    W = 22
    fig, ax = plt.subplots(figsize=(W, 1.6 + W * (ymax + 1.0) / (xmax + 2.2)))
    for L, a, b in bounds[1:]:
        ax.plot([a - 0.15, a - 0.15], [0.5, ymax + 0.5], color="#e6e6e2", lw=0.6, zorder=0)
    for (L, s, y) in T.groupby(["level_nr", "slot", "y"]).size().index:
        ax.add_patch(FancyBboxPatch((x[(L, s)] - CELL / 2, y - CELL / 2), CELL, CELL,
                                    boxstyle="round,pad=0,rounding_size=0.04", fc=CELL_BG, ec="none", zorder=1))
    spots, stops, frames, segs, cols, dots, ticks = [], [], [], [], [], [], []
    for r in T.itertuples():
        i, j = spot[r.participant_id]
        sx = x[(r.level_nr, r.slot)] - CELL / 2 + step * (i + 0.5)
        sy = r.y - CELL / 2 + step * (j + 0.5)
        rect = Rectangle((sx - step * 0.47, sy - step * 0.47), step * 0.94, step * 0.94)
        (stops if stopped.get(r.participant_id, (None,))[0] == r.trial_nr_global else spots).append(rect)
        if r.completes:
            frames.append(Rectangle((sx - step * 0.47, sy - step * 0.47), step * 0.94, step * 0.94))
        ticks.append([(sx, sy + r_out * 0.9), (sx, sy + r_out * 1.12)])
        seg, c, d, _ = spiral(D["traj"][r.Index], sx, sy, r_in, r_out)
        segs.append(seg); cols.extend(c); dots.extend(d)
    ax.add_collection(PatchCollection(spots, facecolor=SPOT_BG, edgecolor="none", zorder=2))
    ax.add_collection(PatchCollection(stops, facecolor=STOP_BG, edgecolor="none", zorder=2))
    ax.add_collection(LineCollection(ticks, colors=M.GREEN, lw=0.5, zorder=3))
    ax.add_collection(LineCollection(np.concatenate(segs), colors=cols, lw=0.6, capstyle="butt", zorder=4))
    qx, qy, qc = zip(*dots)
    ax.scatter(qx, qy, s=1.4, c=qc, lw=0, zorder=6)
    ax.add_collection(PatchCollection(frames, facecolor="none", edgecolor=INK, linewidth=0.22, zorder=5))
    for (L, s), d in TT[TT.y > CAP].groupby(["level_nr", "slot"]):
        ax.text(x[(L, s)], CAP + 0.8, f"+{len(d)} trials\nafter round {CAP}", ha="center", va="center",
                fontsize=4.5, color=MUTED, linespacing=1.1)

    top = ymax + (1.1 if (TT.y > CAP).any() else 0.6)
    ax.set_xlim(-0.2, xmax + 0.2); ax.set_ylim(0.45, top); ax.set_aspect("equal", adjustable="box")
    ax.set_yticks(range(1, ymax + 1)); ax.set_ylabel("Round", fontsize=8)
    for sp in ("top", "right", "bottom"):
        ax.spines[sp].set_visible(False)
    ax.spines["left"].set_color("#bdbdb8")
    ax.tick_params(axis="y", colors="#bdbdb8", labelcolor=MUTED, labelsize=6.5)
    ax.set_xticks([x[k] for k in slots])
    ax.set_xticklabels([("corner\n" + M.TY[1]) if s[0] == 1 else ("face\n" + M.TY[2]) for _, s in slots],
                       fontsize=6.5, color=MUTED, linespacing=1.2)
    ax.tick_params(axis="x", length=0, pad=2)
    for L, s in slots:
        if L >= 5:
            ax.annotate("cong." if s[1] == 0 else "incong.", xy=(x[(L, s)], 0), xycoords=("data", "axes fraction"),
                        xytext=(0, -22), textcoords="offset points", ha="center", va="top", fontsize=6,
                        color="#a3a39e")
    for L, a, b in bounds:
        ax.text((a + b) / 2, top + 0.05, f"Level {L}", ha="center", va="bottom", fontsize=8, fontweight="bold",
                clip_on=False)
    # key: participant spots, in the empty top of the last level
    ks = 0.42
    Lk = [bd for bd in bounds if bd[0] == max(b[0] for b in bounds)][0]
    kx0, ky0 = (Lk[1] + Lk[2]) / 2 - g * ks / 2, ymax - g * ks - 0.3
    ax.add_patch(FancyBboxPatch((kx0, ky0), g * ks, g * ks, boxstyle="round,pad=0,rounding_size=0.08",
                                fc=CELL_BG, ec="none", zorder=0.5))
    for p, (i, j) in spot.items():
        ax.text(kx0 + ks * (i + 0.5), ky0 + ks * (j + 0.5), M.short(p).replace("pilot ", "p"), ha="center",
                va="center", fontsize=6, color=INK if p not in stopped else MUTED,
                fontweight="bold" if p not in stopped else "normal")
    ax.text(kx0 + g * ks / 2, ky0 + g * ks + 0.08, "Participant in each cell", ha="center", va="bottom", fontsize=6.5)
    ax.text(kx0 + g * ks / 2, ky0 - 0.08, "bold = finished all levels", ha="center", va="top", fontsize=5.5,
            color=MUTED)

    fig.suptitle(f"How the objects were turned, {proj} (n = {len(order)})", x=0.01, ha="left", fontsize=10,
                 fontweight="bold")
    fig.legend(handles=legend_core(1.0), loc="upper left", bbox_to_anchor=(0.01, 0.975), ncol=8, frameon=False,
               fontsize=7, columnspacing=1.8, handletextpad=0.5)
    fig.text(0.01, 0.006, "Each cell is one object in one round; each participant has a fixed spot in it (key in the last "
             "level) showing how they turned the object in that trial, rotated so the rewarded position points up "
             "(left/right as on screen; turning left runs clockwise). Inner end = start, outer end = last check.",
             fontsize=6, color=MUTED)
    fig.tight_layout(rect=(0, 0.015, 1, 0.955))
    fig.savefig(out, dpi=300)
    plt.close(fig)


# ============================================================================ 3. group: participants x levels
def group_matrix_figure(proj, D, out):
    BOX, R_OUT, R_IN, GAP_L, GAP_P, NOBJ = 0.95, 0.42, 0.15, 0.6, 0.35, 4
    TILE_BG, BLOCK_BG = "#f1f1ee", "#fafaf8"
    TT = D["T"]; T = TT[TT.y <= CAP]
    order, stopped = D["order"], D["stopped"]
    levels = sorted(TT.level_nr.unique())
    slots_of = {L: sorted(set(TT[TT.level_nr == L].slot)) for L in levels}
    width = {L: min(CAP, int(TT[TT.level_nr == L].y.max())) for L in levels}
    lvl_off, pos = {}, 0.0
    for L in levels:
        lvl_off[L] = pos; pos += width[L] + GAP_L
    lvl_total = pos - GAP_L
    p_off = {p: k * (NOBJ + GAP_P) for k, p in enumerate(order)}
    p_total = len(order) * (NOBJ + GAP_P) - GAP_P

    def cell(L, s, y, pid):                      # rounds left -> right, objects stacked (centred in the band)
        k = len(slots_of[L]); obj = (NOBJ - k) / 2 + slots_of[L].index(s)
        return lvl_off[L] + (y - 1) + 0.5, p_total - (p_off[pid] + obj + 0.5)

    fig, ax = plt.subplots(figsize=(0.2 * lvl_total + 2.2, 0.2 * p_total + 1.6))
    tiles, stops, frames, segs, cols, dots, ticks = [], [], [], [], [], [], []
    for r in T.itertuples():
        cx, cy = cell(r.level_nr, r.slot, r.y, r.participant_id)
        rect = Rectangle((cx - BOX / 2, cy - BOX / 2), BOX, BOX)
        (stops if stopped.get(r.participant_id, (None,))[0] == r.trial_nr_global else tiles).append(rect)
        if r.completes:
            frames.append(Rectangle((cx - BOX / 2, cy - BOX / 2), BOX, BOX))
        ticks.append([(cx, cy + R_OUT * 0.92), (cx, cy + R_OUT * 1.12)])
        seg, c, d, _ = spiral(D["traj"][r.Index], cx, cy, R_IN, R_OUT)
        segs.append(seg); cols.extend(c); dots.extend(d)
    for L in levels:
        for pid in order:
            sel = (TT.participant_id == pid) & (TT.level_nr == L)
            if not sel.any():
                continue
            bx, by = lvl_off[L], p_total - p_off[pid] - NOBJ
            ax.add_patch(Rectangle((bx - 0.05, by - 0.05), width[L] + 0.1, NOBJ + 0.1, fc=BLOCK_BG, ec="#e4e4e0",
                                   lw=0.3, zorder=0))
            n_over = (sel & (TT.y > CAP)).sum()
            if n_over:
                ax.text(bx + width[L] + 0.05, by + NOBJ / 2, f"+{n_over}", ha="left", va="center", fontsize=3.8,
                        color=MUTED)
    ax.add_collection(PatchCollection(tiles, facecolor=TILE_BG, edgecolor="none", zorder=1))
    ax.add_collection(PatchCollection(stops, facecolor=STOP_BG, edgecolor="none", zorder=1))
    ax.add_collection(LineCollection(ticks, colors=M.GREEN, lw=0.45, zorder=2))
    ax.add_collection(LineCollection(np.concatenate(segs), colors=cols, lw=0.55, capstyle="butt", zorder=3))
    qx, qy, qc = zip(*dots)
    ax.scatter(qx, qy, s=1.3, c=qc, lw=0, zorder=5)
    ax.add_collection(PatchCollection(frames, facecolor="none", edgecolor=INK, linewidth=0.25, zorder=4))

    for L in levels:
        ax.text(lvl_off[L] + width[L] / 2, p_total + 0.75, f"Level {L}", ha="center", va="bottom", fontsize=7.5,
                fontweight="bold")
        ax.text(lvl_off[L], p_total + 0.15, "round →", ha="left", va="bottom", fontsize=4.5, color="#a3a39e")
    for p in order:
        yc = p_total - p_off[p] - NOBJ / 2
        ax.text(-0.6, yc + 0.35, M.short(p), ha="right", va="center", fontsize=7, fontweight="bold")
        ax.text(-0.6, yc - 0.6, "✓ all levels" if p not in stopped else f"time limit L{stopped[p][1]}", ha="right",
                va="center", fontsize=4.8, color=MUTED)
    ax.set_xlim(-5.0, lvl_total + 0.8); ax.set_ylim(-0.3, p_total + 2.0)
    ax.set_aspect("equal"); ax.axis("off")

    FH = fig.get_figheight()
    fig.text(0.01, 1 - 0.1 / FH, f"How the objects were turned, {proj} (n = {len(order)}): participants × levels",
             ha="left", va="top", fontsize=10, fontweight="bold")
    fig.legend(handles=legend_core(), loc="upper left", bbox_to_anchor=(0.005, 1 - 0.38 / FH), ncol=8, frameon=False,
               fontsize=7, columnspacing=1.6, handletextpad=0.5)
    fig.text(0.01, 0.004, f"Each tile is one trial, rotated so the rewarded position points up; inner end = start, outer "
             f"end = last check; left/right as on screen. Participants ordered by progress. Rounds 1–{CAP} shown "
             f"(+n = later trials); objects stacked inside each band, top to bottom: corner {M.TY[1]} congruent, corner "
             f"incongruent, face {M.TY[2]} congruent, face incongruent (Levels 0–4: no congruency).",
             fontsize=5.8, color=MUTED, va="bottom")
    fig.subplots_adjust(left=0.005, right=0.995, top=1 - 0.75 / FH, bottom=0.25 / FH)
    fig.savefig(out, dpi=300)
    plt.close(fig)


# ============================================================================ main
if __name__ == "__main__":
    for proj, _ in M.PROJECTS:
        D = load(proj)
        group_cells_figure(proj, D, f"MAIN_sampling_group_cells_{proj}.png")
        group_matrix_figure(proj, D, f"MAIN_sampling_group_matrix_{proj}.png")
        if proj == EXAMPLE[0]:
            participant_figure(proj, EXAMPLE[1], D, "MAIN_sampling_participant_example.png")
        print(f"{proj}: done")
    print("Saved the MAIN sampling figures in the project root.")
