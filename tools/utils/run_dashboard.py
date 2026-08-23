"""ipywidgets front end for :mod:`tools.utils.run_analysis`.

The widgets deliberately call the public analysis functions instead of
embedding analysis logic.  This keeps notebook cells short and makes every
view reproducible from ordinary Python code.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any, Sequence

import ipywidgets as widgets
import numpy as np
from IPython.display import HTML as DisplayHTML, clear_output, display

from .run_analysis import (
    RunDataset,
    timing_events,
    discover_run_sources,
    load_runs,
    plot_flexible_2d,
    plot_frame_pacing,
    plot_run_comparison,
    plot_trial_overview,
    plot_trajectory_3d,
)


BUTTON_LAYOUT = widgets.Layout(width="150px")
WIDE_LAYOUT = widgets.Layout(width="98%")


def _selected(value: Sequence[Any]) -> list[Any] | None:
    values = list(value)
    return values or None


def _show_table(frame, *, rows: int = 200) -> None:
    if frame.empty:
        display(DisplayHTML("<em>No rows.</em>"))
        return
    display(frame.head(rows).style.hide(axis="index"))
    if len(frame) > rows:
        display(DisplayHTML(f"<small>Showing {rows:,} of {len(frame):,} rows.</small>"))


class RunDashboard:
    """Interactive multi-view explorer for a loaded :class:`RunDataset`."""

    def __init__(self, dataset: RunDataset):
        self.dataset = dataset
        self.run_ids = tuple(dataset.frames["run_id"].drop_duplicates()) if not dataset.frames.empty else ()
        self.trial_ids = tuple(dataset.frames["trial_uid"].drop_duplicates()) if not dataset.frames.empty else ()
        self.numeric_columns = tuple(
            name
            for name in dataset.frames.select_dtypes(include=[np.number, "bool"]).columns
            if name not in {"frame_order_source"}
        )
        self.color_columns = tuple(
            dict.fromkeys(
                ["run_id", "trial_uid", "outcome", "platform", "level_index", "late_frame", *self.numeric_columns]
            )
        )
        self.widget = self._build()

    def _run_selector(self) -> widgets.SelectMultiple:
        return widgets.SelectMultiple(
            options=self.run_ids,
            value=self.run_ids,
            description="Runs",
            rows=max(2, min(5, len(self.run_ids))),
            layout=widgets.Layout(width="98%"),
        )

    def _button_output(self, label: str):
        button = widgets.Button(description=label, button_style="primary", layout=BUTTON_LAYOUT)
        output = widgets.Output(layout=WIDE_LAYOUT)
        return button, output

    def _summary_tab(self) -> widgets.Widget:
        output = widgets.Output()
        with output:
            display(DisplayHTML("<h3>Run summary</h3>"))
            _show_table(self.dataset.run_summary.round(4))
            display(DisplayHTML("<h3>Highest-latency trials</h3>"))
            columns = [
                "run_id",
                "level_index",
                "trial_index_in_chain",
                "trial_run_counter",
                "n_frames",
                "late_frames",
                "dt_p99_ms",
                "dt_max_ms",
                "frame_gaps",
                "render_gaps",
            ]
            available = [name for name in columns if name in self.dataset.trial_summary]
            worst = self.dataset.trial_summary.sort_values("dt_max_ms", ascending=False)[available]
            _show_table(worst.round(4), rows=30)
            if not self.dataset.levels.empty:
                display(DisplayHTML("<h3>Level-summary structure</h3>"))
                level_columns = [
                    "run_id",
                    "level_index",
                    "level_run_counter",
                    "level_completed",
                    "n_trial_refs",
                    "elapsed_time_no_anim",
                    "elapsed_time_anim",
                    "summary_present_dt_mean_ms",
                    "summary_present_dt_std_ms",
                    "within_trial_present_dt_mean_ms",
                    "summary_boundary_bias_ms",
                    "summary_render_gaps",
                ]
                _show_table(
                    self.dataset.levels[
                        [name for name in level_columns if name in self.dataset.levels]
                    ].sort_values(["run_id", "level_index"]),
                    rows=200,
                )
            display(DisplayHTML("<h3>Largest active-play timing intervals</h3>"))
            _show_table(timing_events(self.dataset, active_only=True, limit=30).round(4), rows=30)
        return output

    def _trial_tab(self) -> widgets.Widget:
        trial = widgets.Dropdown(options=self.trial_ids, description="Trial", layout=WIDE_LAYOUT)
        button, output = self._button_output("Show trial")

        def render(_=None):
            with output:
                clear_output(wait=True)
                if not trial.value:
                    display(DisplayHTML("<em>No trial selected.</em>"))
                    return
                display(plot_trial_overview(self.dataset, trial.value))

        button.on_click(render)
        render()
        return widgets.VBox([trial, button, output])

    def _timing_tab(self) -> widgets.Widget:
        runs = self._run_selector()
        x = widgets.Dropdown(
            options=["session_time_s", "trial_time_s", "frame_number", "render_frame_number"],
            value="session_time_s",
            description="X axis",
        )
        button, output = self._button_output("Update timing")

        def render(_=None):
            with output:
                clear_output(wait=True)
                display(plot_frame_pacing(self.dataset, run_ids=_selected(runs.value), x=x.value))

        button.on_click(render)
        with output:
            display(DisplayHTML("<em>Choose runs and an axis, then click Update timing.</em>"))
        return widgets.VBox([runs, widgets.HBox([x, button]), output])

    def _flexible_tab(self) -> widgets.Widget:
        runs = self._run_selector()
        x_default = "trial_time_s" if "trial_time_s" in self.numeric_columns else self.numeric_columns[0]
        y_default = "current_angle" if "current_angle" in self.numeric_columns else self.numeric_columns[0]
        x = widgets.Dropdown(options=self.numeric_columns, value=x_default, description="X")
        y = widgets.Dropdown(options=self.numeric_columns, value=y_default, description="Y")
        color = widgets.Dropdown(options=self.color_columns, value="run_id", description="Color")
        mode = widgets.ToggleButtons(options=["scatter", "line"], value="scatter", description="Mode")
        button, output = self._button_output("Update 2D")

        def render(_=None):
            with output:
                clear_output(wait=True)
                display(
                    plot_flexible_2d(
                        self.dataset,
                        x=x.value,
                        y=y.value,
                        color=color.value,
                        run_ids=_selected(runs.value),
                        mode=mode.value,
                    )
                )

        button.on_click(render)
        with output:
            display(DisplayHTML("<em>Choose frame fields, then click Update 2D.</em>"))
        return widgets.VBox([runs, widgets.HBox([x, y, color]), widgets.HBox([mode, button]), output])

    def _three_d_tab(self) -> widgets.Widget:
        runs = self._run_selector()
        defaults = {
            "x": "object_heading_x" if "object_heading_x" in self.numeric_columns else self.numeric_columns[0],
            "y": "object_heading_z" if "object_heading_z" in self.numeric_columns else self.numeric_columns[0],
            "z": "trial_time_s" if "trial_time_s" in self.numeric_columns else self.numeric_columns[0],
            "color": "dt_ms" if "dt_ms" in self.color_columns else self.color_columns[0],
        }
        x = widgets.Dropdown(options=self.numeric_columns, value=defaults["x"], description="X")
        y = widgets.Dropdown(options=self.numeric_columns, value=defaults["y"], description="Y")
        z = widgets.Dropdown(options=self.numeric_columns, value=defaults["z"], description="Z")
        color = widgets.Dropdown(options=self.color_columns, value=defaults["color"], description="Color")
        button, output = self._button_output("Update 3D")

        def render(_=None):
            with output:
                clear_output(wait=True)
                display(
                    plot_trajectory_3d(
                        self.dataset,
                        x=x.value,
                        y=y.value,
                        z=z.value,
                        color=color.value,
                        run_ids=_selected(runs.value),
                    )
                )

        button.on_click(render)
        with output:
            display(DisplayHTML("<em>Choose frame fields, then click Update 3D.</em>"))
        note = widgets.HTML(
            "<small>The default heading path reconstructs object yaw from the start orientation, observed angle step, and logged rotation direction. "
            "It is diagnostic rather than a geometry-perfect replay; choose any numeric state fields for another 3D relationship.</small>"
        )
        return widgets.VBox([note, runs, widgets.HBox([x, y, z, color]), button, output])

    def _comparison_tab(self) -> widgets.Widget:
        candidates = [
            name
            for name in ("dt_ms", "instant_fps", "current_angle", "current_alignment", "angle_delta_rad", "drift_s")
            if name in self.numeric_columns
        ]
        metric = widgets.Dropdown(options=candidates, value="dt_ms", description="Metric")
        button, output = self._button_output("Compare runs")

        def render(_=None):
            with output:
                clear_output(wait=True)
                display(plot_run_comparison(self.dataset, metric=metric.value))

        button.on_click(render)
        with output:
            display(DisplayHTML("<em>Choose a metric, then click Compare runs.</em>"))
        return widgets.VBox([widgets.HBox([metric, button]), output])

    def _quality_tab(self) -> widgets.Widget:
        severity = widgets.SelectMultiple(
            options=["error", "warning", "info"],
            value=("error", "warning", "info"),
            description="Severity",
            rows=3,
        )
        output = widgets.Output()

        def render(change=None):
            with output:
                clear_output(wait=True)
                selected = self.dataset.issues[self.dataset.issues["severity"].isin(severity.value)]
                _show_table(selected, rows=500)
                display(DisplayHTML("<h3>Trial/schema inventory</h3>"))
                columns = [
                    "run_id",
                    "source_member",
                    "level_index",
                    "trial_run_counter",
                    "frames_container",
                    "n_frames_raw",
                    "platform",
                    "refresh_rate_hz_reported",
                ]
                _show_table(self.dataset.trials[[name for name in columns if name in self.dataset.trials]], rows=500)

        severity.observe(render, names="value")
        render()
        return widgets.VBox([severity, output])

    def _build(self) -> widgets.Tab:
        children = [
            self._summary_tab(),
            self._trial_tab(),
            self._timing_tab(),
            self._flexible_tab(),
            self._three_d_tab(),
            self._comparison_tab(),
            self._quality_tab(),
        ]
        tabs = widgets.Tab(children=children, layout=WIDE_LAYOUT)
        for index, title in enumerate(("Summary", "Trial", "Timing", "Flexible 2D", "3D", "Compare", "Quality")):
            tabs.set_title(index, title)
        return tabs

    def display(self) -> None:
        display(self.widget)


class RunBrowser:
    """Browse sources, load selected runs, and create a :class:`RunDashboard`."""

    def __init__(self, *, search_root: str | Path, repo_root: str | Path | None = None):
        self.repo_root = Path(repo_root).resolve() if repo_root else Path.cwd().resolve()
        self.dataset: RunDataset | None = None
        self.dashboard: RunDashboard | None = None
        self.root = widgets.Text(value=str(Path(search_root).expanduser().resolve()), description="Folder", layout=WIDE_LAYOUT)
        self.scan_button = widgets.Button(description="Scan", icon="search", layout=BUTTON_LAYOUT)
        self.sources = widgets.SelectMultiple(description="Sources", rows=9, layout=WIDE_LAYOUT)
        self.extra = widgets.Textarea(
            value="",
            placeholder="Optional: one extra .zip, .json, or run directory per line",
            description="Extra paths",
            rows=3,
            layout=WIDE_LAYOUT,
        )
        self.load_button = widgets.Button(description="Load selected", button_style="success", icon="folder-open", layout=BUTTON_LAYOUT)
        self.output = widgets.Output(layout=WIDE_LAYOUT)
        self.widget = widgets.VBox(
            [
                widgets.HTML("<h3>1. Choose run sources</h3>"),
                widgets.HBox([self.root, self.scan_button]),
                self.sources,
                self.extra,
                self.load_button,
                self.output,
            ],
            layout=WIDE_LAYOUT,
        )
        self.scan_button.on_click(self._scan)
        self.load_button.on_click(self._load)
        self._scan()

    def _scan(self, _=None) -> None:
        root = Path(self.root.value).expanduser()
        found = discover_run_sources(root)
        options = [(str(path.relative_to(root)) if path.is_relative_to(root) else str(path), str(path)) for path in found]
        self.sources.options = options
        preferred = tuple(value for label, value in options if "virgil" in label.lower())
        self.sources.value = preferred or tuple(value for _, value in options[:1])
        with self.output:
            clear_output(wait=True)
            display(DisplayHTML(f"Found <strong>{len(found)}</strong> selectable source(s) under <code>{root}</code>."))

    def _paths(self) -> list[Path]:
        raw = list(self.sources.value)
        raw.extend(line.strip() for line in self.extra.value.splitlines() if line.strip())
        paths: list[Path] = []
        for value in raw:
            path = Path(value).expanduser()
            if not path.is_absolute():
                path = self.repo_root / path
            paths.append(path.resolve())
        return list(dict.fromkeys(paths))

    def _load(self, _=None) -> None:
        paths = self._paths()
        with self.output:
            clear_output(wait=True)
            if not paths:
                display(DisplayHTML("<strong>Select at least one source.</strong>"))
                return
            display(DisplayHTML(f"Loading {len(paths)} source(s)…"))
        dataset = load_runs(paths)
        self.dataset = dataset
        self.dashboard = RunDashboard(dataset) if not dataset.frames.empty else None
        with self.output:
            clear_output(wait=True)
            display(
                DisplayHTML(
                    f"<h3>2. Explore</h3><p>Loaded <strong>{dataset.frames['run_id'].nunique() if not dataset.frames.empty else 0}</strong> run(s), "
                    f"<strong>{dataset.frames['trial_uid'].nunique() if not dataset.frames.empty else 0}</strong> trial(s), and "
                    f"<strong>{len(dataset.frames):,}</strong> frame rows.</p>"
                )
            )
            errors = dataset.issues[dataset.issues["severity"] == "error"]
            if not errors.empty:
                display(DisplayHTML("<h4>Compatibility errors</h4>"))
                _show_table(errors)
            if self.dashboard:
                display(self.dashboard.widget)

    def display(self) -> None:
        display(self.widget)
