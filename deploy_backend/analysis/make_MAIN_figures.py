"""Create the MAIN figures that combine pilot_Gymer and kjpp. Run from the project root:

    python make_MAIN_figures.py

Writes (PNG, 300 dpi, project root):
    MAIN_mistakes_per_level_both.png                  A pilot_Gymer | B kjpp
    MAIN_mistakes_per_object_both.png                 A pilot_Gymer | B kjpp, with first-choice strip
    MAIN_object_timeline_group_colors_side_by_side.png   A pilot_Gymer | B kjpp
    (a stacked version can be switched on at the bottom of the file)

Needs numpy, pandas, matplotlib. Reads <project>/out_rebuilt trial tables and the level summaries in
<project>/data (for "level completed" vs "timeout").

Definitions used throughout
- Mistake: a wrong check. In a won trial, the checks before the winning one; otherwise every check.
- Round: within a level the game shows every object once, in random order, before showing any object
  again (a shuffled "bag"; controller_main.js, _refillChainBag). In levels 6 and 8
  (fixed.remove_completed_chains) a completed object leaves the bag. add_rounds() rebuilds this and
  checks it against the data (it must find no violations).
- Chain completed: the trial that wins the last chain step of an object whose chain was not already done.
"""
import glob
import json

import numpy as np
import pandas as pd
import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.lines import Line2D
from matplotlib.patches import FancyBboxPatch, Rectangle

PROJECTS = [("pilot_Gymer", "pilot_Gymer/out_rebuilt/trials.csv"),
            ("kjpp", "kjpp/out_rebuilt/trials_11_09.csv")]
NSTEPS = {0: 5, 1: 5, 2: 5, 3: 5, 4: 4, 5: 4, 6: 4, 7: 4, 8: 4}   # chain length per level (trials.jsonl)
REMOVE_COMPLETED = {6, 8}                                          # levels with fixed.remove_completed_chains

plt.rcParams.update({"font.family": ["Arial", "Helvetica", "DejaVu Sans"], "font.size": 8,
                     "mathtext.fontset": "dejavuserif",          # serif roman numerals for the object types
                     "axes.linewidth": 0.6, "xtick.major.width": 0.6, "ytick.major.width": 0.6,
                     "xtick.major.size": 3, "ytick.major.size": 3})

INK, MUTED, GRID, FAINT, SURF = "#222222", "#6f6f6f", "#e8e8e8", "#d9d9d9", "#ffffff"
BAND, TILE = "#f5f5f3", "#ececea"
COLORS = ["#2a78d6", "#eb6834", "#1baf7a", "#eda100", "#e87ba4", "#008300", "#6250d6", "#e34948"]
TYPE_COL = {1: "#e8692e", 2: "#2a78d6"}                 # object type 1 = corner object, 2 = face object
CONG_MK = {"True": "o", "False": "^", "": "D"}          # congruent / incongruent / no colour rule (L0-4)
CONG_MS = {"True": 5.2, "False": 5.8, "": 4.6}
CHOICE_MK = {"corner": "^", "face": "s"}
CHOICE_COL = {"corner": TYPE_COL[1], "face": TYPE_COL[2]}
GREEN, RED, YELLOW = "#1a9a5c", "#d7362b", "#f0a500"
TY = {1: r"(ty. $\mathrm{I}$)", 2: r"(ty. $\mathrm{II}$)"}      # object type labels, serif numerals


# ============================================================================ data
def add_rounds(T):
    """Add round, pos_in_round and completes per trial; return (T, number of bag-rule violations)."""
    T = T.sort_values(["participant_id", "trial_nr_global"]).copy()
    already = T.chain__already_completed.astype(str) == "True"
    T["completes"] = ((T.outcome__chain_action == "advance")
                      & (T.chain__step == T.level_nr.map(NSTEPS) - 1) & ~already)
    rnd, pos, viol = [], [], 0
    for (_, L), d in T.groupby(["participant_id", "level_nr"], sort=False):
        objs, done, remaining, r, i = set(d.object_id), set(), set(), -1, 0
        for o, comp, act in zip(d.object_id, d.completes, d.outcome__chain_action):
            if not remaining:                                   # new round: refill the bag
                remaining = (objs - done) if L in REMOVE_COMPLETED else set(objs)
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


def load(proj, fn):
    T = pd.read_csv(fn, dtype={"participant_id": str, "features_congruent": str}, keep_default_na=False)
    T, viol = add_rounds(T)
    assert viol == 0, f"{proj}: {viol} trials break the round structure"
    status = {}
    for f in glob.glob(f"{proj}/data/**/*_summary_*.json", recursive=True):
        s = json.load(open(f))
        status[(str(s["participant"]), int(s["level_index"]))] = s["level_completed"]
    won = T.outcome__success.astype(str) == "True"
    T["wrong"] = np.where(won, T.outcome__success_nr - 1, T.nr_attempts)
    T["cong_key"] = T.features_congruent.map({"True": 0, "False": 1, "": 0})
    T["slot"] = list(zip(T.object_type_nr, T.cong_key, T.object_id_order_by_type))   # same object for everyone
    T["y"] = T["round"] + 1
    T["ok"] = T.first_attempt_position_correct.astype(str).isin(["True", "1", "1.0"])
    T["type_ok"] = T.first_attempt_type_correct.astype(str).isin(["True", "1", "1.0"])
    T["noguess"] = T.first_attempt_type == ""
    T["level_done"] = [status.get((p, l)) == "completed" for p, l in zip(T.participant_id, T.level_nr)]

    O = (T.groupby(["participant_id", "level_nr", "object_id"])
           .agg(wrong=("wrong", "sum"), typ=("object_type_nr", "first"), cong=("features_congruent", "first"),
                slot=("slot", "first"), done=("level_done", "first"),
                first_choice=("first_attempt_type", "first"))      # T is in time order: first sighting
           .reset_index())

    lv = T.drop_duplicates(["participant_id", "level_nr"])
    prog = lv.groupby("participant_id").agg(done=("level_done", "sum"), reached=("level_nr", "max"))
    prog["trials"] = T.groupby("participant_id").size()
    order = prog.sort_values(["done", "reached", "trials"], ascending=[False, False, True]).index.tolist()
    pids = list(dict.fromkeys(pd.read_csv(fn, dtype={"participant_id": str}).participant_id))  # CSV order
    return dict(T=T, O=O, status=status, order=order, pids=pids)


def short(p):
    return p.replace("kjpp_vrsafe_bl_pilot_", "pilot ").replace("kjpp_vrsafe_", "")


def panel_label(ax, letter, text, x0=-0.09, dx=0.06, y=1.04, size=12):
    ax.text(x0, y, letter, transform=ax.transAxes, fontsize=size, fontweight="bold", va="bottom")
    ax.text(x0 + dx, y, text, transform=ax.transAxes, fontsize=size - 3, va="bottom")


def clean(ax):
    ax.yaxis.grid(True, color=GRID, lw=0.6); ax.set_axisbelow(True)
    for sp in ("top", "right"):
        ax.spines[sp].set_visible(False)


# ============================================================================ 1. mistakes per level
def mistakes_per_level(data, out="MAIN_mistakes_per_level_both.png"):
    shift = 0.2                                           # mean drawn just right of the participant points
    fig, axes = plt.subplots(1, 2, figsize=(10.5, 3.6), sharey=True)
    for k, (ax, (proj, D)) in enumerate(zip(axes, data)):
        M = D["T"].groupby(["participant_id", "level_nr"]).wrong.sum()
        handles = []
        for i, pid in enumerate(D["pids"]):
            c, ls = COLORS[i % 8], ("-" if i < 8 else (0, (3, 1.6)))   # 9th+ participant: same hues, dashed
            d = M.loc[pid]
            ax.plot(d.index, d.values, color=c, lw=1.0, ls=ls, alpha=0.8, zorder=2)
            for l, y in d.items():
                done = D["status"].get((pid, l)) == "completed"
                ax.plot(l, y, "o", ms=3.4, color=c, mfc=c if done else "white", mec=c, mew=0.9, zorder=3)
            handles.append(Line2D([], [], color=c, ls=ls, lw=1.0, marker="o", ms=3.4, label=short(pid)))
        st = M.groupby("level_nr").agg(["mean", "std", "size"])
        ax.errorbar(st.index + shift, st["mean"], yerr=st["std"], color=INK, lw=1.7, elinewidth=1.0, capsize=2.5,
                    capthick=1.0, marker="o", ms=5, mfc=INK, mec="white", mew=0.8, zorder=5)
        for l, r in st.iterrows():
            ax.text(l + shift / 2, -0.09, f"n = {int(r['size'])}", transform=ax.get_xaxis_transform(),
                    ha="center", va="top", fontsize=6.5, color=MUTED)
        ax.set_xticks(range(9)); ax.set_xlim(-0.4, 8.5); ax.set_ylim(0, None)
        ax.set_xlabel("Level", labelpad=13)
        if k == 0:
            ax.set_ylabel("Mistakes per level (wrong checks)")
        clean(ax); panel_label(ax, "AB"[k], f"{proj}  (n = {len(D['pids'])})")
        ax.legend(handles=handles, title="Participant", frameon=False, fontsize=6.5, title_fontsize=7,
                  loc="upper center", bbox_to_anchor=(0.5, -0.22), ncol=8, handlelength=1.8, columnspacing=1.0)
    shared = [Line2D([], [], color=INK, lw=1.7, marker="o", ms=5, mec="white", mew=0.8, label="Mean ± SD"),
              Line2D([], [], ls="", marker="o", ms=3.4, mfc="white", mec=MUTED, mew=0.9,
                     label="Level not finished (time limit)")]
    fig.legend(handles=shared, loc="upper right", frameon=False, fontsize=7.5, ncol=2, bbox_to_anchor=(0.99, 1.0))
    fig.subplots_adjust(left=0.065, right=0.99, top=0.87, bottom=0.3, wspace=0.12)
    fig.savefig(out, dpi=300)
    plt.close(fig)


# ============================================================================ 2. mistakes per object
def mistakes_per_object(data, out="MAIN_mistakes_per_object_both.png"):
    span, center, tot_x = 0.64, -0.08, 0.37               # objects left of the level centre, total to the right
    npmax = max(len(D["order"]) for _, D in data)
    fig = plt.figure(figsize=(12.5, 5.6))
    gs = fig.add_gridspec(2, 2, height_ratios=[1, 0.62], hspace=0.2, wspace=0.17,
                          left=0.07, right=0.83, top=0.93, bottom=0.075)
    axes = []
    for k, (proj, D) in enumerate(data):
        ax = fig.add_subplot(gs[0, k], sharey=axes[0][0] if axes else None)
        sx = fig.add_subplot(gs[1, k], sharex=ax)
        axes.append((ax, sx))
        O, prow = D["O"], D["order"]
        rng = np.random.default_rng(1)
        row_of = {pid: npmax - 1 - i for i, pid in enumerate(prow)}      # same row spacing in both panels
        mean_y = npmax + 0.25
        for L in range(0, 9, 2):
            for a_ in (ax, sx):
                a_.axvspan(L - 0.5, L + 0.5, color=BAND, lw=0, zorder=0)
        for L, dl in O.groupby("level_nr"):
            slots = sorted(set(dl.slot)); kk = len(slots); w = span / kk
            for i, s in enumerate(slots):
                x = L + center + ((i - (kk - 1) / 2) * w if kk > 1 else 0)
                d = dl[dl.slot == s]; c = TYPE_COL[int(s[0])]; mk = CONG_MK[d.cong.iloc[0]]
                jit = rng.uniform(-w * 0.28, w * 0.28, len(d)) if kk > 1 else rng.uniform(-0.08, 0.08, len(d))
                dn = d.done.values
                ax.scatter((x + jit)[dn], d.wrong[dn], s=8, marker=mk, color=c, alpha=0.3, lw=0, zorder=2)
                ax.scatter((x + jit)[~dn], d.wrong[~dn], s=8, marker=mk, facecolors="white", edgecolors=c,
                           alpha=0.6, lw=0.6, zorder=2)
                m, sd = d.wrong.mean(), d.wrong.std(ddof=1)
                ax.errorbar(x, m, yerr=sd, fmt="none", ecolor=c, elinewidth=0.9, capsize=1.8, capthick=0.9, zorder=3)
                ax.plot(x, m, mk, ms=CONG_MS[d.cong.iloc[0]], color=c, mec="white", mew=0.6, zorder=4)
                for r in d.itertuples():                                 # first choice per participant
                    if r.first_choice in CHOICE_MK:
                        sx.plot(x, row_of[r.participant_id], CHOICE_MK[r.first_choice], ms=2.7,
                                color=CHOICE_COL[r.first_choice], mec="none", zorder=3)
                    else:
                        sx.plot(x, row_of[r.participant_id], "x", ms=2.4, color=MUTED, mew=0.7, zorder=3)
                ch = d.first_choice[d.first_choice.isin(["corner", "face"])]
                if len(ch):                                              # share choosing a corner
                    pc, bw = (ch == "corner").mean(), min(w * 0.62, 0.16)
                    sx.bar(x, pc, bottom=mean_y - 0.5, width=bw, color=CHOICE_COL["corner"], lw=0, zorder=3)
                    sx.bar(x, 1 - pc, bottom=mean_y - 0.5 + pc, width=bw, color=CHOICE_COL["face"], lw=0, zorder=3)
        tot = O.groupby(["participant_id", "level_nr"]).wrong.sum().groupby("level_nr").agg(["mean", "std", "size"])
        ax.errorbar(tot.index + tot_x, tot["mean"], yerr=tot["std"], color=INK, lw=1.2, elinewidth=0.8, capsize=2.5,
                    capthick=0.9, marker="s", ms=4.2, mfc=INK, mec="white", mew=0.6, zorder=5)
        ax.set_xticks(range(9)); ax.set_xlim(-0.5, 8.5); ax.set_ylim(0, None)
        for L in range(9):
            ax.text(L, -0.1, f"n = {int(tot.loc[L, 'size'])}", transform=ax.get_xaxis_transform(),
                    ha="center", va="top", fontsize=6.5, color=MUTED)
        ax.tick_params(axis="x", length=0, labelbottom=True); sx.tick_params(axis="x", length=0, labelbottom=False)
        ax.annotate("Level", xy=(0, 0), xycoords="axes fraction", xytext=(-8, -8.5), textcoords="offset points",
                    ha="right", va="center", fontsize=8, fontweight="bold")
        clean(ax)
        if k == 0:
            ax.set_ylabel("Mistakes (wrong checks)")
        else:
            plt.setp(ax.get_yticklabels(), visible=False)
        panel_label(ax, "AB"[k], f"{proj}  (n = {len(prow)})")
        sx.set_ylim(-0.6, mean_y + 0.6)
        sx.set_yticks([mean_y] + [row_of[pid] for pid in prow])
        sx.set_yticklabels(["corner share"] + [short(pid) for pid in prow], fontsize=6)
        sx.get_yticklabels()[0].set_color(MUTED)
        sx.tick_params(axis="y", length=0)
        for sp in ("top", "right", "left", "bottom"):
            sx.spines[sp].set_visible(False)
        sx.axhline(npmax - 0.45, color=GRID, lw=0.6, zorder=1)
        sx.axhspan(-0.6, npmax - 1 - len(prow) + 0.5, color="white", zorder=0.5)   # blank below the last row
        if k == 0:
            sx.set_ylabel("First check on\nfirst sighting", fontsize=7.5, labelpad=4)
    h = [Line2D([], [], color=INK, lw=1.2, marker="s", ms=4.2, mec="white", mew=0.6, label="Level total, mean ± SD"),
         Line2D([], [], ls="", marker="o", ms=5, color=TYPE_COL[1], mec="white", label=f"Corner object {TY[1]}"),
         Line2D([], [], ls="", marker="o", ms=5, color=TYPE_COL[2], mec="white", label=f"Face object {TY[2]}"),
         Line2D([], [], ls="", marker="D", ms=4.4, color=MUTED, mec="white", label="No colour rule (L0–4)"),
         Line2D([], [], ls="", marker="o", ms=5, color=MUTED, mec="white", label="Congruent (L5–8)"),
         Line2D([], [], ls="", marker="^", ms=5.5, color=MUTED, mec="white", label="Incongruent (L5–8)"),
         Line2D([], [], ls="", marker="o", ms=3.5, color=MUTED, alpha=0.4, mec="none", label="Participant"),
         Line2D([], [], ls="", marker="o", ms=3.5, mfc="white", mec=MUTED, mew=0.6,
                label="Participant, level not finished")]
    axes[1][0].legend(handles=h, frameon=False, fontsize=7, loc="upper left", bbox_to_anchor=(1.03, 1.0),
                      handlelength=1.6, labelspacing=0.55)
    h2 = [Line2D([], [], ls="", marker="^", ms=4.5, color=TYPE_COL[1], label="Checked a corner first"),
          Line2D([], [], ls="", marker="s", ms=4, color=TYPE_COL[2], label="Checked a face first"),
          Line2D([], [], ls="", marker="x", ms=3.5, color=MUTED, mew=0.7, label="No check"),
          Line2D([], [], ls="", marker="s", ms=6, color=TYPE_COL[1], label="Share corner (bar)")]
    leg2 = axes[1][1].legend(handles=h2, frameon=False, fontsize=7, loc="upper left", bbox_to_anchor=(1.03, 1.0),
                             handlelength=1.2, labelspacing=0.55,
                             title="Same colour as the object\n= right kind of position", title_fontsize=6.5)
    leg2._legend_box.align = "left"; leg2.get_title().set_color(MUTED)
    fig.text(0.01, 0.008, "Top: mistakes per object, mean ± SD across participants. Bottom: first check on the first "
             "presentation of each object in the level, one row per participant (ordered by progress, top = furthest); "
             "bar (top row) = share who checked a corner first. n = participants who played the level.",
             fontsize=6.5, color=MUTED)
    fig.savefig(out, dpi=300)
    plt.close(fig)


# ============================================================================ 3. object timeline (point cloud)
CAP = 15            # rounds shown individually; later rounds summarised in one row
CELL = 0.88         # occupied part of a 1 x 1 cell


def timeline_layout(T):
    slots, x, bounds, pos = [], {}, [], 0.0
    for L in range(9):
        objs = sorted(set(T[T.level_nr == L].slot))
        if not objs:
            continue
        width = max(len(objs), 1.6)
        start = pos + (width - len(objs)) / 2 + 0.5
        for i, s in enumerate(objs):
            x[(L, s)] = start + i; slots.append((L, s))
        bounds.append((L, pos, pos + width)); pos += width + 0.3
    return slots, x, bounds, pos - 0.3


def timeline_panel(ax, kax, D, label, title):
    T = D["T"]
    slots, x, bounds, xmax = timeline_layout(T)
    order = D["order"]; g = int(np.ceil(np.sqrt(len(order))))
    spot = {p: (k % g, k // g) for k, p in enumerate(order)}      # furthest first: bottom left, rows upwards
    step = CELL / g
    ms, fs = (5.6, 3.3) if g >= 4 else (6.9, 4.1)                  # dot size and order-number size
    last = T.sort_values("trial_nr_global").groupby("participant_id").tail(1)
    stopped = {r.participant_id: r for r in last.itertuples() if not r.level_done}   # time limit

    for k, (L, a, b) in enumerate(bounds):                         # level bands
        if k % 2 == 0:
            ax.add_patch(Rectangle((a - 0.15, 0.5), b - a + 0.3, CAP + 1.0, fc=BAND, ec="none", zorder=0))
    for (L, s, y) in T[T.y <= CAP].groupby(["level_nr", "slot", "y"]).size().index:   # occupied cells
        ax.add_patch(FancyBboxPatch((x[(L, s)] - CELL / 2, y - CELL / 2), CELL, CELL,
                                    boxstyle="round,pad=0,rounding_size=0.06", fc=TILE, ec="none", zorder=1))
    for r in T[T.y <= CAP].itertuples():
        i, j = spot[r.participant_id]
        px = x[(r.level_nr, r.slot)] - CELL / 2 + step * (i + 0.5)
        py = r.y - CELL / 2 + step * (j + 0.5)
        if r.completes:
            ax.plot(px, py, "o", ms=ms + 2.6, mfc="none", mec=INK, mew=0.55, zorder=3)
        if r.participant_id in stopped and stopped[r.participant_id].trial_nr_global == r.trial_nr_global:
            ax.plot(px, py, "s", ms=ms + 2.6, mfc="none", mec=INK, mew=1.0, zorder=5)
        if r.noguess:
            ax.plot(px, py, "o", ms=ms, mfc="white", mec=MUTED, mew=0.6, zorder=4)
            ax.text(px, py, str(r.pos_in_round), ha="center", va="center", fontsize=fs, color=MUTED, zorder=6)
            continue
        c = GREEN if r.ok else (YELLOW if r.type_ok else RED)
        ax.plot(px, py, "o", ms=ms, color=c, mec="none", zorder=4)
        ax.text(px, py, str(r.pos_in_round), ha="center", va="center", fontsize=fs, zorder=6,
                color=INK if c == YELLOW else "white", fontweight="bold")
    for (L, s), d in T[T.y > CAP].groupby(["level_nr", "slot"]):     # overflow row
        ax.text(x[(L, s)], CAP + 1.18, f"{d.participant_id.nunique()} p. · {len(d)} tr.\n{d.ok.mean():.0%}",
                ha="center", va="center", fontsize=5.5, color=MUTED, linespacing=1.15)
    for pid, r in stopped.items():                                  # stops after round CAP
        if r.y > CAP:
            i, _ = spot[pid]
            px = x[(r.level_nr, r.slot)] - CELL / 2 + step * (i + 0.5)
            ax.plot(px, CAP + 0.78, "s", ms=ms + 2.6, mfc="none", mec=INK, mew=1.0, zorder=5)
            ax.text(px, CAP + 0.78, f"{r.y}", ha="center", va="center", fontsize=4.5, color=INK, zorder=6)
    ax.axhline(CAP + 0.5, color=FAINT, lw=0.6, ls=(0, (2, 2)), zorder=1)

    ax.set_xlim(-0.2, xmax + 0.2); ax.set_ylim(0.45, CAP + 1.6); ax.set_aspect("equal", adjustable="box")
    ax.set_yticks(list(range(1, CAP + 2)))
    ax.set_yticklabels([str(i) for i in range(1, CAP + 1)] + [f">{CAP}"])
    ax.set_ylabel("Round", fontsize=9)
    for sp in ("top", "right", "bottom"):
        ax.spines[sp].set_visible(False)
    ax.spines["left"].set_color(MUTED); ax.tick_params(axis="y", colors=MUTED, labelcolor=INK, labelsize=7)
    ax.set_xticks([x[k] for k in slots])
    ax.set_xticklabels([("corner\n" + TY[1]) if s[0] == 1 else ("face\n" + TY[2]) for _, s in slots], fontsize=7,
                       linespacing=1.25)
    ax.tick_params(axis="x", length=0, pad=3)
    for L, s in slots:
        if L >= 5:
            ax.annotate("cong." if s[1] == 0 else "incong.", xy=(x[(L, s)], 0), xycoords=("data", "axes fraction"),
                        xytext=(0, -26), textcoords="offset points", ha="center", va="top", fontsize=6.5, color=MUTED)
    for L, a, b in bounds:
        ax.annotate("", xy=(a + 0.05, CAP + 1.75), xytext=(b - 0.05, CAP + 1.75), xycoords="data",
                    arrowprops=dict(arrowstyle="-", color=MUTED, lw=0.6), annotation_clip=False)
        ax.text((a + b) / 2, CAP + 1.95, f"Level {L}", ha="center", va="bottom", fontsize=8.5,
                fontweight="bold", clip_on=False)
    ax.text(-0.065, 1.07, label, transform=ax.transAxes, fontsize=14, fontweight="bold", va="bottom")
    ax.text(-0.035, 1.07, title, transform=ax.transAxes, fontsize=10, va="bottom")

    # key: position of each participant inside a cell
    kax.set_xlim(0, g); kax.set_ylim(0, g); kax.set_aspect("equal"); kax.axis("off")
    kax.add_patch(FancyBboxPatch((0.04, 0.04), g - 0.08, g - 0.08, boxstyle="round,pad=0,rounding_size=0.25",
                                 fc=TILE, ec="none"))
    fk = 7 if g >= 4 else 7.5
    for p, (i, j) in spot.items():
        name = short(p).replace("pilot ", "pilot\n")
        if p in stopped:
            kax.text(i + 0.5, j + 0.6, name, ha="center", va="center", fontsize=fk, color=MUTED, linespacing=0.95)
            kax.text(i + 0.5, j + 0.2, f"stop L{stopped[p].level_nr}", ha="center", va="center",
                     fontsize=5, color=MUTED)
        else:
            kax.text(i + 0.5, j + 0.5, name, ha="center", va="center", fontsize=fk, fontweight="bold",
                     linespacing=0.95)
    kax.set_title("Participant\nposition in a cell", fontsize=7.5, pad=4)
    kax.text(g / 2, -0.25, "furthest progress bottom left\nbold = finished all levels\ngrey = stopped by time limit",
             ha="center", va="top", fontsize=6.5, color=MUTED, linespacing=1.3)


def timeline_figure(data, layout, out):
    if layout == "stacked":
        W = 14.4
        fig = plt.figure(figsize=(W, W * 1.2), facecolor=SURF)
        gs = fig.add_gridspec(2, 2, width_ratios=[1, 0.085], hspace=0.2, wspace=0.015,
                              left=0.055, right=0.99, top=0.93, bottom=0.05)
        cells, foot_x = [(gs[0, 0], gs[0, 1]), (gs[1, 0], gs[1, 1])], 0.06
    else:
        W = 28.8
        fig = plt.figure(figsize=(W, W * 0.285), facecolor=SURF)
        gs = fig.add_gridspec(1, 4, width_ratios=[1, 0.09, 1, 0.09], wspace=0.02,
                              left=0.03, right=0.985, top=0.87, bottom=0.085)
        cells, foot_x = [(gs[0, 0], gs[0, 1]), (gs[0, 2], gs[0, 3])], 0.03
    for k, ((proj, D), (c_ax, c_key)) in enumerate(zip(data, cells)):
        ax, kax = fig.add_subplot(c_ax), fig.add_subplot(c_key)
        kax.set_anchor("NW"); ax.set_anchor("E")
        timeline_panel(ax, kax, D, "AB"[k], f"{proj}  (n = {len(D['order'])})")
    handles = [Line2D([], [], ls="", marker="o", ms=6, color=GREEN, label="First check correct"),
               Line2D([], [], ls="", marker="o", ms=6, color=YELLOW, label="Wrong position, right kind (corner/face)"),
               Line2D([], [], ls="", marker="o", ms=6, color=RED, label="Wrong kind (corner vs. face)"),
               Line2D([], [], ls="", marker="o", ms=6, mfc="white", mec=MUTED, mew=0.6, label="No check"),
               Line2D([], [], ls="", marker="o", ms=8, mfc="none", mec=INK, mew=0.6, label="Chain completed"),
               Line2D([], [], ls="", marker="s", ms=8, mfc="none", mec=INK, mew=1.0, label="Last trial (time limit)"),
               Line2D([], [], ls="", marker="$2$", ms=6, color=INK, label="Number = order within the round")]
    fig.legend(handles=handles, loc="upper center", ncol=7, frameon=False, fontsize=8.5,
               bbox_to_anchor=(0.5, 0.998), handletextpad=0.4, columnspacing=1.8)
    fig.text(foot_x, 0.012, "Each cell is one object in one round; each participant has a fixed position inside the cell "
             "(key on the right). The number in a dot is when the participant saw this object within the round "
             f"(each object once per round). Row >{CAP}: participants (p.), trials (tr.) and % correct first checks "
             f"after round {CAP}; a square there marks a stop after round {CAP} (number = round).",
             fontsize=7, color=MUTED, ha="left")
    fig.savefig(out, dpi=300, facecolor=SURF)
    plt.close(fig)


# ============================================================================ main
if __name__ == "__main__":
    data = [(proj, load(proj, fn)) for proj, fn in PROJECTS]
    mistakes_per_level(data)
    mistakes_per_object(data)
    timeline_figure(data, "side", "MAIN_object_timeline_group_colors_side_by_side.png")
    # stacked alternative: timeline_figure(data, "stacked", "MAIN_object_timeline_group_colors_stacked.png")
    print("Saved the MAIN figures in the project root.")
