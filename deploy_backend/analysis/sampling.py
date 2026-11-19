"""The figures of `make_MAIN_sampling_figures.py`, drawn on the server by the script itself.

The script and `make_MAIN_figures.py` are kept byte-for-byte as the analysis team wrote
them (like `build_dataset.py`); their drawing functions `participant_figure()`,
`group_cells_figure()` and `group_matrix_figure()` make the PNG. Only the script's
`load()` is replaced, because it reads `<project>/out_rebuilt/trials.csv` and
`<project>/data`. Here the same `D` comes from the analysis store:

- T: `dataset.figure_table()`, which is `make_MAIN_figures.load()`'s table (chain length
  and remove_completed_chains read from each level's summary, as for the explorer's
  other plots), plus the `slot` column that `load()` adds;
- status, order: as `make_MAIN_figures.load()`; stopped: as the script's `load()`;
- traj: the script's `trajectory()` of each trial's raw JSON.
"""
import io
import logging
import os
import sys
import threading

import pandas as pd

PLOTS = ("participant", "cells", "matrix")
VERSION = "sampling2"          # part of the figure cache key: bump when the drawing changes
_LOCK = threading.Lock()       # pyplot keeps global state
_scripts = None


def scripts():
    """The script modules, imported on first use (matplotlib is slow to import)."""
    global _scripts
    if _scripts is None:
        here = os.path.dirname(os.path.abspath(__file__))
        if here not in sys.path:
            sys.path.append(here)   # the scripts import build_dataset and make_MAIN_figures by file name
        import matplotlib
        import make_MAIN_figures as M
        import make_MAIN_sampling_figures as S
        # The script asks for Arial; Liberation Sans (fonts-liberation, in the Docker image) has
        # the same metrics, so the figures keep their layout where Arial is not installed.
        matplotlib.rcParams["font.family"] = ["Arial", "Helvetica", "Liberation Sans", "Arimo", "DejaVu Sans"]
        logging.getLogger("matplotlib.font_manager").setLevel(logging.ERROR)
        _scripts = (M, S)
    return _scripts


def apply_filters(T, levels=None, kind="", congruency="", completion=""):
    """The explorer's filters (admin_analysis.js filter())."""
    keep = pd.Series(True, index=T.index)
    if levels:
        keep &= T.level_nr.isin(levels)
    if kind:
        keep &= T.object_type == kind
    if congruency:
        keep &= T.features_congruent == {"congruent": "True", "incongruent": "False", "no rule": ""}[congruency]
    if completion:
        keep &= T.level_done if completion == "complete" else ~T.level_done
    return T[keep]


def figure_input(T, levels, paths):
    """D as the script's load() returns it. `levels` maps (participant, level) to the level
    summary fields; `paths` maps (session, trial_run_counter) to the trial JSON. Trials whose
    file cannot be read are left out and listed in the returned notes."""
    M, S = scripts()
    T = T.copy()
    T["slot"] = list(zip(T.object_type_nr, T.cong_key, T.object_id_order_by_type))
    traj, notes = {}, []
    for r in T.itertuples():
        path = paths.get((r.session, int(r.trial_run_counter)))
        try:
            traj[r.Index] = S.trajectory(path)
        except Exception as exc:  # a broken file leaves an empty place, not a failed figure
            notes.append(f"{r.participant_id}, level {r.level_nr}, trial {r.trial_run_counter}: "
                         f"not drawn ({type(exc).__name__}: {exc})")
    T = T.loc[list(traj)]
    status = {(p, int(L)): info.get("status") for (p, L), info in levels.items()}
    # make_MAIN_figures.load(): progress order
    lv = T.drop_duplicates(["participant_id", "level_nr"])
    prog = lv.groupby("participant_id").agg(done=("level_done", "sum"), reached=("level_nr", "max"))
    prog["trials"] = T.groupby("participant_id").size()
    order = prog.sort_values(["done", "reached", "trials"], ascending=[False, False, True]).index.tolist()
    # make_MAIN_sampling_figures.load(): the last trial of a participant stopped by the time limit
    last = T.sort_values("trial_nr_global").groupby("participant_id").tail(1)
    stopped = {r.participant_id: (r.trial_nr_global, r.level_nr) for r in last.itertuples() if not r.level_done}
    return dict(T=T, status=status, order=order, traj=traj, stopped=stopped), notes


def render(plot, D, name, participant=None):
    """PNG bytes of one of the script's figures, exactly as the script saves it (300 dpi)."""
    M, S = scripts()
    if not len(D["T"]):
        raise ValueError("No trials to draw for this selection.")
    if plot == "participant" and participant not in set(D["T"].participant_id):
        raise ValueError("No trials for this participant in the selection.")
    import matplotlib.pyplot as plt
    with _LOCK:
        buf = io.BytesIO()
        try:
            if plot == "participant":
                S.participant_figure(name, participant, D, buf)
            elif plot == "cells":
                S.group_cells_figure(name, D, buf)
            elif plot == "matrix":
                S.group_matrix_figure(name, D, buf)
            else:
                raise ValueError("Unknown plot")
        finally:
            plt.close("all")
        return buf.getvalue()
