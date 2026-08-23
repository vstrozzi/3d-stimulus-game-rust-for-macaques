"""Load, validate, compare, and visualize monkey-game run logs.

The public API intentionally returns ordinary pandas DataFrames and Plotly
figures.  The notebook UI is only a thin layer over these functions, so the
same analysis can be reused from tests, scripts, or a different notebook.
"""

from __future__ import annotations

import json
import math
import zipfile
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any, Iterable, Iterator, Sequence

import numpy as np
import pandas as pd
import plotly.express as px
import plotly.graph_objects as go
from plotly.subplots import make_subplots


STATE_FIELDS = (
    "frame_number",
    "render_frame_number",
    "elapsed_secs",
    "render_elapsed_secs",
    "present_elapsed_secs",
    "camera_radius",
    "camera_x",
    "camera_y",
    "camera_z",
    "camera_speed_rotate",
    "attempts",
    "current_alignment",
    "current_angle",
    "is_animating",
    "is_blank",
    "is_rendering_stopped",
    "is_scene_ready",
    "photodiode_white",
    "win_elapsed_secs",
)

COMMAND_FIELDS = (
    "rotate_left",
    "rotate_right",
    "zoom_in",
    "zoom_out",
    "check",
    "reset",
    "toggle_blank",
    "toggle_stop_rendering",
    "animation_door",
    "animation_all_door",
    "animation_colored",
    "shake",
)

REQUIRED_TRIAL_FIELDS = ("frames", "level_index", "trial_run_counter")
REQUIRED_STATE_FIELDS = ("frame_number", "present_elapsed_secs")
RECOMMENDED_STATE_FIELDS = (
    "render_frame_number",
    "current_angle",
    "current_alignment",
    "camera_x",
    "camera_y",
    "camera_z",
)


@dataclass(frozen=True)
class AnalysisConfig:
    """Thresholds used for derived timing and input-response metrics."""

    expected_hz: float = 60.0
    late_multiplier: float = 1.5
    severe_multiplier: float = 3.0
    robust_outlier_mad: float = 6.0
    movement_epsilon_rad: float = 1e-5
    movement_max_catchup_frames: float = 2.0
    max_plot_points: int = 20_000

    @property
    def expected_dt_s(self) -> float:
        return 1.0 / self.expected_hz

    @property
    def late_dt_s(self) -> float:
        return self.expected_dt_s * self.late_multiplier

    @property
    def severe_dt_s(self) -> float:
        return self.expected_dt_s * self.severe_multiplier


@dataclass(frozen=True)
class AnalysisIssue:
    severity: str
    scope: str
    message: str
    run_id: str = ""
    trial_uid: str = ""
    field: str = ""


@dataclass
class RunDataset:
    """Normalized data plus validation and diagnostic tables."""

    frames: pd.DataFrame = field(default_factory=pd.DataFrame)
    trials: pd.DataFrame = field(default_factory=pd.DataFrame)
    levels: pd.DataFrame = field(default_factory=pd.DataFrame)
    trial_summary: pd.DataFrame = field(default_factory=pd.DataFrame)
    run_summary: pd.DataFrame = field(default_factory=pd.DataFrame)
    issues: pd.DataFrame = field(default_factory=pd.DataFrame)
    sources: tuple[str, ...] = ()
    config: AnalysisConfig = field(default_factory=AnalysisConfig)

    @property
    def compatible(self) -> bool:
        if self.frames.empty:
            return False
        return self.issues.empty or not (self.issues["severity"] == "error").any()

    def select(
        self,
        *,
        run_ids: Sequence[str] | None = None,
        trial_uids: Sequence[str] | None = None,
        levels: Sequence[int] | None = None,
    ) -> pd.DataFrame:
        return filter_frames(
            self.frames, run_ids=run_ids, trial_uids=trial_uids, levels=levels
        )


def discover_run_sources(root: str | Path, *, max_depth: int = 3) -> list[Path]:
    """Find ZIPs, JSON files, and top-level run directories below ``root``.

    Directories are returned only at the shallowest level that contains trial
    JSON files, preventing every nested ``level/trials`` folder from appearing
    as a separate selectable run.
    """

    base = Path(root).expanduser()
    if not base.exists():
        return []
    if base.is_file():
        return [base] if base.suffix.lower() in {".zip", ".json"} else []

    def looks_like_trial_json(path: Path) -> bool:
        if path.suffix.lower() != ".json" or "summary" in path.stem.lower():
            return False
        if "trial" in path.stem.lower():
            return True
        try:
            with path.open("r", encoding="utf-8") as handle:
                payload = json.load(handle)
            if isinstance(payload, dict):
                return "frames" in payload
            return bool(isinstance(payload, list) and payload and isinstance(payload[0], dict) and "frames" in payload[0])
        except (OSError, json.JSONDecodeError):
            return False

    results: list[Path] = []
    results.extend(sorted(base.glob("*.zip")))
    results.extend(sorted(path for path in base.glob("*.json") if looks_like_trial_json(path)))

    def has_trial_json(directory: Path) -> bool:
        return any(looks_like_trial_json(path) for path in directory.rglob("*.json"))

    if has_trial_json(base):
        direct_json = any(looks_like_trial_json(path) for path in base.glob("*.json"))
        run_children = [
            child
            for child in sorted(base.iterdir())
            if child.is_dir() and has_trial_json(child)
        ]
        if direct_json:
            results.append(base)
        else:
            results.extend(run_children)
    elif max_depth > 1:
        for child in sorted(base.iterdir()):
            if not child.is_dir():
                continue
            for source in discover_run_sources(child, max_depth=max_depth - 1):
                results.append(source)

    unique: dict[str, Path] = {}
    for result in results:
        unique[str(result.resolve())] = result.resolve()
    return sorted(unique.values(), key=lambda p: (p.is_dir(), p.name.lower()))


def _iter_documents(source: Path) -> Iterator[tuple[str, Any]]:
    if source.is_dir():
        for path in sorted(source.rglob("*.json")):
            try:
                with path.open("r", encoding="utf-8") as handle:
                    yield str(path.relative_to(source)), json.load(handle)
            except (OSError, json.JSONDecodeError) as exc:
                yield str(path.relative_to(source)), exc
        return

    if source.suffix.lower() == ".zip":
        try:
            with zipfile.ZipFile(source) as archive:
                members = sorted(
                    (
                        info
                        for info in archive.infolist()
                        if not info.is_dir()
                        and info.filename.lower().endswith(".json")
                    ),
                    key=lambda info: info.filename,
                )
                for info in members:
                    try:
                        payload = archive.read(info).decode("utf-8")
                        yield info.filename, json.loads(payload)
                    except (UnicodeDecodeError, json.JSONDecodeError, OSError) as exc:
                        yield info.filename, exc
        except (OSError, zipfile.BadZipFile) as exc:
            yield source.name, exc
        return

    if source.suffix.lower() == ".json":
        try:
            with source.open("r", encoding="utf-8") as handle:
                yield source.name, json.load(handle)
        except (OSError, json.JSONDecodeError) as exc:
            yield source.name, exc


def _trial_documents(document: Any, member: str) -> Iterator[tuple[str, dict[str, Any]]]:
    if isinstance(document, dict):
        yield member, document
    elif isinstance(document, list):
        for index, value in enumerate(document):
            if isinstance(value, dict):
                yield f"{member}#{index}", value


def _is_level_summary(member: str, document: Any) -> bool:
    return bool(
        isinstance(document, dict)
        and (
            "summary" in Path(member.split("#")[0]).stem.lower()
            or "trials_runs" in document
            or "timing_health" in document
            or "level_completed" in document
        )
    )


def _parse_level_summary(document: dict[str, Any], *, run_id: str, source: Path, member: str) -> dict[str, Any]:
    session = document.get("session_info") if isinstance(document.get("session_info"), dict) else {}
    timing = document.get("timing_health") if isinstance(document.get("timing_health"), dict) else {}
    trial_refs = document.get("trials_runs") if isinstance(document.get("trials_runs"), list) else []
    return {
        "run_id": run_id,
        "source": str(source),
        "source_member": member,
        "participant": document.get("participant") or "",
        "level_index": _safe_number(document.get("level_index")),
        "level_name": document.get("level_name") or "",
        "level_run_counter": _safe_number(document.get("level_run_counter")),
        "timestamp_start": document.get("timestamp_start"),
        "timestamp_end": document.get("timestamp_end"),
        "elapsed_time_no_anim": _safe_number(document.get("elapsed_time_no_anim")),
        "elapsed_time_anim": _safe_number(document.get("elapsed_time_anim")),
        "level_completed": document.get("level_completed"),
        "n_trial_refs": len(trial_refs),
        "prev_file": document.get("prev_file"),
        "next_file": document.get("next_file"),
        "platform": session.get("platform") or "unknown",
        "refresh_rate_hz_reported": _safe_number(session.get("refresh_rate_hz")),
        "summary_present_dt_mean_ms": _safe_number(timing.get("present_dt_mean_ms")),
        "summary_present_dt_std_ms": _safe_number(timing.get("present_dt_std_ms")),
        "summary_refresh_rate_hz_measured": _safe_number(timing.get("refresh_rate_hz_measured")),
        "summary_render_gaps": _safe_number(timing.get("render_gaps")),
        "summary_freeze_events": _safe_number(timing.get("freeze_events")),
        "summary_drift_max_s": _safe_number(timing.get("drift_max_s")),
    }


def _frame_items(raw_frames: Any) -> list[tuple[Any, Any]]:
    if isinstance(raw_frames, dict):
        return list(raw_frames.items())
    if isinstance(raw_frames, list):
        return list(enumerate(raw_frames))
    return []


def _safe_number(value: Any) -> float:
    try:
        if value is None or isinstance(value, bool):
            return math.nan
        number = float(value)
        return number if math.isfinite(number) else math.nan
    except (TypeError, ValueError):
        return math.nan


def _first_non_null(mapping: dict[str, Any], *keys: str) -> Any:
    for key in keys:
        if key in mapping and mapping[key] is not None:
            return mapping[key]
    return None


def _session_platform(trial: dict[str, Any]) -> str:
    session = trial.get("session_info") or {}
    if not isinstance(session, dict):
        return "unknown"
    platform = str(session.get("platform") or "").strip()
    user_agent = str(session.get("user_agent") or "")
    if platform:
        return platform
    if "Android" in user_agent or "Mobile" in user_agent:
        return "wasm-mobile"
    return "unknown"


def _make_trial_uid(run_id: str, trial: dict[str, Any], member: str, index: int) -> str:
    def integer(name: str, default: int) -> int:
        value = _safe_number(trial.get(name))
        return int(value) if math.isfinite(value) else default

    level = integer("level_index", 0)
    trial_index = integer("trial_index_in_chain", 0)
    chain = integer("active_chain", 0)
    run_counter = integer("trial_run_counter", index)
    return (
        f"{run_id}::L{level:03d}-T{trial_index:03d}-"
        f"C{chain:03d}-R{run_counter:04d}::{Path(member.split('#')[0]).stem}"
    )


def _parse_trial(
    trial: dict[str, Any],
    *,
    run_id: str,
    source: Path,
    member: str,
    ordinal: int,
    issues: list[AnalysisIssue],
) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    uid = _make_trial_uid(run_id, trial, member, ordinal)
    missing_trial = [name for name in REQUIRED_TRIAL_FIELDS if name not in trial]
    for name in missing_trial:
        severity = "error" if name == "frames" else "warning"
        issues.append(
            AnalysisIssue(severity, "schema", f"Required trial field is missing: {name}", run_id, uid, name)
        )

    session = trial.get("session_info") if isinstance(trial.get("session_info"), dict) else {}
    trial_config = trial.get("trial_config") if isinstance(trial.get("trial_config"), dict) else {}
    user_agent = str(session.get("user_agent") or "")
    refresh_rate = _safe_number(session.get("refresh_rate_hz"))
    meta = {
        "run_id": run_id,
        "trial_uid": uid,
        "source": str(source),
        "source_member": member,
        "level_index": _safe_number(trial.get("level_index")),
        "active_chain": _safe_number(trial.get("active_chain")),
        "trial_index_in_chain": _safe_number(trial.get("trial_index_in_chain")),
        "trial_run_counter": _safe_number(trial.get("trial_run_counter")),
        "outcome": trial.get("outcome") or "",
        "nr_attempts": _safe_number(trial.get("nr_attempts")),
        "elapsed_time_no_anim": _safe_number(trial.get("elapsed_time_no_anim")),
        "elapsed_time_anim": _safe_number(trial.get("elapsed_time_anim")),
        "timestamp_start": trial.get("timestamp_start"),
        "timestamp_end": trial.get("timestamp_end"),
        "platform": _session_platform(trial),
        "user_agent": user_agent,
        "is_mobile": bool("Mobile" in user_agent or "Android" in user_agent),
        "refresh_rate_hz_reported": refresh_rate,
        "present_mode": session.get("present_mode") or "",
        "controller_app_start_unix_ns": _safe_number(session.get("app_start_unix_ns")),
        "cross_origin_isolated": session.get("cross_origin_isolated"),
        "build_id": _first_non_null(session, "build_id", "git_commit", "app_version", "version") or "",
        "frames_container": type(trial.get("frames")).__name__,
        "start_orient_rad": _safe_number(trial.get("start_orient")),
        "config_target_door": _safe_number(trial_config.get("target_door")),
        "config_cosine_alignment_threshold": _safe_number(trial_config.get("cosine_alignment_threshold")),
        "config_camera_speed_rotate": _safe_number(trial_config.get("camera_speed_rotate")),
        "config_camera_rotation_sense": _safe_number(trial_config.get("camera_rotation_sense")),
        "config_firefly_count": _safe_number(trial_config.get("firefly_count")),
        "config_decorations_count": _safe_number(trial_config.get("decorations_count")),
    }

    items = _frame_items(trial.get("frames"))
    if not items:
        issues.append(AnalysisIssue("error", "schema", "Trial has no readable frames", run_id, uid, "frames"))
        return meta, []

    state_key_union: set[str] = set()
    state_presence: dict[str, int] = {}
    command_key_union: set[str] = set()
    rows: list[dict[str, Any]] = []
    used_legacy_elapsed = False
    for frame_order, (frame_key, entry) in enumerate(items):
        if not isinstance(entry, dict):
            entry = {}
        nested = isinstance(entry.get("state_read"), dict)
        state = entry.get("state_read", {}) if nested else entry
        commands = entry.get("commands_sent", {}) if isinstance(entry.get("commands_sent"), dict) else {}
        state_key_union.update(state)
        for key in state:
            state_presence[key] = state_presence.get(key, 0) + 1
        command_key_union.update(commands)

        default_frame = _safe_number(frame_key)
        present = _safe_number(state.get("present_elapsed_secs"))
        if math.isnan(present):
            present = _safe_number(state.get("elapsed_secs"))
            used_legacy_elapsed = not math.isnan(present)

        camera_position = state.get("camera_position")
        if not isinstance(camera_position, (list, tuple)) or len(camera_position) < 3:
            camera_position = (math.nan, math.nan, math.nan)

        row: dict[str, Any] = {**meta, "frame_order_source": frame_order}
        row["check_input_event_elapsed_secs"] = _safe_number(
            entry.get("check_input_event_elapsed_secs")
        )
        for state_name in STATE_FIELDS:
            value = state.get(state_name)
            if state_name == "present_elapsed_secs":
                value = present
            elif state_name == "frame_number" and value is None:
                value = default_frame
            elif state_name == "attempts" and value is None:
                value = state.get("nr_attempts")
            elif state_name == "current_alignment" and value is None:
                value = state.get("cosine_alignment")
            elif state_name in {"camera_x", "camera_y", "camera_z"} and value is None:
                value = camera_position[{"camera_x": 0, "camera_y": 1, "camera_z": 2}[state_name]]
            row[state_name] = value
        for command_name in COMMAND_FIELDS:
            row[f"cmd_{command_name}"] = bool(commands.get(command_name, False))
        rows.append(row)

    for name in REQUIRED_STATE_FIELDS:
        if name not in state_key_union and not (name == "present_elapsed_secs" and used_legacy_elapsed):
            issues.append(AnalysisIssue("error", "schema", f"Required frame field is missing: {name}", run_id, uid, name))
        elif state_presence.get(name, 0) not in {0, len(rows)}:
            issues.append(
                AnalysisIssue(
                    "warning",
                    "schema",
                    f"Required frame field {name} is absent from {len(rows) - state_presence.get(name, 0)} of {len(rows)} rows",
                    run_id,
                    uid,
                    name,
                )
            )
    for name in RECOMMENDED_STATE_FIELDS:
        if name not in state_key_union:
            issues.append(AnalysisIssue("warning", "schema", f"Recommended frame field is missing: {name}", run_id, uid, name))
        elif state_presence.get(name, 0) != len(rows):
            issues.append(
                AnalysisIssue(
                    "warning",
                    "schema",
                    f"Recommended frame field {name} is absent from {len(rows) - state_presence.get(name, 0)} of {len(rows)} rows",
                    run_id,
                    uid,
                    name,
                )
            )
    if used_legacy_elapsed:
        issues.append(
            AnalysisIssue(
                "warning",
                "timing",
                "present_elapsed_secs is missing; elapsed_secs was used as a legacy fallback",
                run_id,
                uid,
                "present_elapsed_secs",
            )
        )
    if not command_key_union:
        issues.append(AnalysisIssue("warning", "schema", "commands_sent is absent; input-response checks are unavailable", run_id, uid, "commands_sent"))
    meta["n_frames_raw"] = len(rows)
    return meta, rows


def _coerce_frames(frames: pd.DataFrame) -> pd.DataFrame:
    if frames.empty:
        return frames
    numeric = [
        "frame_number",
        "render_frame_number",
        "elapsed_secs",
        "render_elapsed_secs",
        "present_elapsed_secs",
        "camera_radius",
        "camera_x",
        "camera_y",
        "camera_z",
        "camera_speed_rotate",
        "attempts",
        "current_alignment",
        "current_angle",
        "win_elapsed_secs",
        "level_index",
        "active_chain",
        "trial_index_in_chain",
        "trial_run_counter",
        "elapsed_time_no_anim",
        "elapsed_time_anim",
        "refresh_rate_hz_reported",
        "controller_app_start_unix_ns",
        "check_input_event_elapsed_secs",
        "start_orient_rad",
        "config_target_door",
        "config_cosine_alignment_threshold",
        "config_camera_speed_rotate",
        "config_camera_rotation_sense",
        "config_firefly_count",
        "config_decorations_count",
    ]
    for name in numeric:
        if name in frames:
            frames[name] = pd.to_numeric(frames[name], errors="coerce")
    bool_fields = [
        "is_animating",
        "is_blank",
        "is_rendering_stopped",
        "is_scene_ready",
        "photodiode_white",
        *[f"cmd_{name}" for name in COMMAND_FIELDS],
    ]
    for name in bool_fields:
        if name in frames:
            frames[name] = frames[name].map(
                lambda value: bool(value) if pd.notna(value) else False
            ).astype(bool)
    return frames


def _robust_outlier_mask(values: pd.Series, scale: float) -> pd.Series:
    valid = values.dropna()
    result = pd.Series(False, index=values.index)
    if valid.empty:
        return result
    median = valid.median()
    mad = (valid - median).abs().median()
    if not math.isfinite(mad) or mad == 0:
        return result
    robust_z = 0.67448975 * (values - median).abs() / mad
    return robust_z > scale


def _derive_frames(frames: pd.DataFrame, config: AnalysisConfig) -> pd.DataFrame:
    if frames.empty:
        return frames
    chunks: list[pd.DataFrame] = []
    for _, raw in frames.groupby("trial_uid", sort=False, observed=True):
        group = raw.sort_values(
            ["frame_number", "frame_order_source"], kind="stable", na_position="last"
        ).copy()
        group["frame_index"] = np.arange(len(group), dtype=int)
        present = group["present_elapsed_secs"]
        group["trial_time_s"] = present - present.iloc[0]
        group["check_input_event_unix_s"] = (
            group["controller_app_start_unix_ns"] / 1e9
            + group["check_input_event_elapsed_secs"]
        )
        group["dt_s"] = present.diff()
        group["dt_ms"] = group["dt_s"] * 1000.0
        group["instant_fps"] = np.where(group["dt_s"] > 0, 1.0 / group["dt_s"], np.nan)
        group["frame_step"] = group["frame_number"].diff()
        group["render_frame_step"] = group["render_frame_number"].diff()
        group["frame_gap_count"] = (group["frame_step"] - 1).clip(lower=0).fillna(0).astype(int)
        group["render_gap_count"] = (group["render_frame_step"] - 1).clip(lower=0).fillna(0).astype(int)
        group["duplicate_frame"] = group["frame_step"].eq(0)
        group["nonpositive_dt"] = group["dt_s"].notna() & group["dt_s"].le(0)
        group["late_frame"] = group["dt_s"].gt(config.late_dt_s)
        group["severe_frame"] = group["dt_s"].gt(config.severe_dt_s)
        group["robust_timing_outlier"] = _robust_outlier_mask(group["dt_s"], config.robust_outlier_mad)
        group["expected_time_s"] = group["frame_index"] * config.expected_dt_s
        group["drift_s"] = group["trial_time_s"] - group["expected_time_s"]
        group["angle_delta_rad"] = group["current_angle"].diff()
        group["next_angle_delta_rad"] = group["current_angle"].shift(-1) - group["current_angle"]
        group["previous_is_animating"] = group["is_animating"].shift(1, fill_value=False)
        group["previous_is_rendering_stopped"] = group["is_rendering_stopped"].shift(1, fill_value=False)
        group["active_interval"] = (
            group["dt_s"].gt(0)
            & ~group["previous_is_animating"]
            & ~group["previous_is_rendering_stopped"]
        )
        group["rotation_command"] = group["cmd_rotate_left"] | group["cmd_rotate_right"]
        group["opposed_rotation_commands"] = group["cmd_rotate_left"] & group["cmd_rotate_right"]
        group["previous_rotation_command"] = group["rotation_command"].shift(1, fill_value=False)
        group["previous_opposed_rotation_commands"] = group["opposed_rotation_commands"].shift(1, fill_value=False)
        previous_left = group["cmd_rotate_left"].shift(1, fill_value=False).astype(int)
        previous_right = group["cmd_rotate_right"].shift(1, fill_value=False).astype(int)
        sense = group["config_camera_rotation_sense"].replace(0, np.nan).fillna(1.0)
        group["rotation_direction"] = (previous_right - previous_left) * sense
        group["rotation_interval"] = (
            group["previous_rotation_command"]
            & ~group["previous_opposed_rotation_commands"]
            & group["dt_s"].gt(0)
            & ~group["is_animating"]
            & ~group["is_rendering_stopped"]
        )
        group["rotation_speed_rad_s"] = np.where(
            group["rotation_interval"], group["angle_delta_rad"].abs() / group["dt_s"], np.nan
        )
        group["movement_expected_factor"] = np.where(
            group["rotation_interval"],
            (group["dt_s"] * config.expected_hz).clip(0, config.movement_max_catchup_frames),
            np.nan,
        )
        signed_step = np.where(
            group["rotation_interval"],
            group["angle_delta_rad"].abs() * group["rotation_direction"],
            0.0,
        )
        start_orient = group["start_orient_rad"].dropna()
        start_yaw = float(start_orient.iloc[0]) if not start_orient.empty else 0.0
        group["object_yaw_reconstructed_rad"] = start_yaw + np.cumsum(signed_step)
        group["object_heading_x"] = np.sin(group["object_yaw_reconstructed_rad"])
        group["object_heading_z"] = np.cos(group["object_yaw_reconstructed_rad"])
        group["rotation_effect_next"] = group["next_angle_delta_rad"].abs().gt(config.movement_epsilon_rad)
        group["rotation_no_effect_next"] = (
            group["rotation_command"]
            & ~group["opposed_rotation_commands"]
            & group["next_angle_delta_rad"].notna()
            & ~group["rotation_effect_next"]
            & ~group["is_animating"]
            & ~group["is_rendering_stopped"]
        )
        chunks.append(group)
    derived = pd.concat(chunks, ignore_index=True)
    derived["movement_baseline_step_rad"] = np.nan
    derived["movement_observed_factor"] = np.nan
    derived["movement_compensation_ratio"] = np.nan
    for _, indices in derived.groupby("run_id", sort=False).groups.items():
        run = derived.loc[indices]
        baseline_candidates = run[
            run["rotation_interval"]
            & run["dt_s"].between(config.expected_dt_s * 0.75, config.expected_dt_s * 1.35)
            & run["angle_delta_rad"].abs().gt(config.movement_epsilon_rad)
        ]["angle_delta_rad"].abs()
        configured = pd.to_numeric(run["config_camera_speed_rotate"], errors="coerce").abs()
        configured = configured.where(configured.gt(0))
        if baseline_candidates.empty and configured.dropna().empty:
            continue
        fallback = float(baseline_candidates.median()) if not baseline_candidates.empty else math.nan
        baseline = configured.fillna(fallback)
        observed = run["angle_delta_rad"].abs() / baseline
        ratio = observed / run["movement_expected_factor"]
        derived.loc[indices, "movement_baseline_step_rad"] = baseline.to_numpy()
        derived.loc[indices, "movement_observed_factor"] = observed.to_numpy()
        derived.loc[indices, "movement_compensation_ratio"] = ratio.to_numpy()
    derived["apparently_undercompensated_late_rotation"] = (
        derived["rotation_interval"]
        & derived["late_frame"]
        & derived["movement_compensation_ratio"].lt(0.75)
    )
    derived["session_time_s"] = np.nan
    for _, indices in derived.groupby("run_id", sort=False).groups.items():
        run = derived.loc[indices]
        starts = pd.to_datetime(run["timestamp_start"], errors="coerce", utc=True)
        if starts.notna().any():
            base = starts.min()
            derived.loc[indices, "session_time_s"] = (
                (starts - base).dt.total_seconds() + run["trial_time_s"]
            ).to_numpy()
        else:
            # Stable concatenated fallback when timestamps are unavailable.
            offset = 0.0
            for _, trial_indices in run.groupby("trial_uid", sort=False).groups.items():
                duration = derived.loc[trial_indices, "trial_time_s"].max()
                derived.loc[trial_indices, "session_time_s"] = (
                    derived.loc[trial_indices, "trial_time_s"] + offset
                )
                offset += (float(duration) if pd.notna(duration) else 0.0) + config.expected_dt_s
    return derived


def _tail_fps(dt: pd.Series, fraction: float) -> float:
    values = dt[dt > 0].dropna().sort_values(ascending=False)
    if values.empty:
        return math.nan
    count = max(1, int(math.ceil(len(values) * fraction)))
    tail_dt = values.iloc[:count].mean()
    return 1.0 / tail_dt if tail_dt > 0 else math.nan


def _summary_row(group: pd.DataFrame, *, group_name: str, group_value: Any) -> dict[str, Any]:
    dt = group["dt_s"]
    positive = dt[dt > 0].dropna()
    active = group.loc[group["active_interval"], "dt_s"].dropna()
    animation = group.loc[~group["active_interval"] & group["dt_s"].gt(0), "dt_s"].dropna()
    total_duration = group.groupby("trial_uid", observed=True)["trial_time_s"].max().sum()
    rotations = group["rotation_command"] & ~group["opposed_rotation_commands"]
    misses = group["rotation_no_effect_next"]
    late_rotation = group["rotation_interval"] & group["late_frame"]
    undercompensated = group["apparently_undercompensated_late_rotation"]
    # Pool lag pairs formed inside trials; never correlate the last interval
    # of one trial with the first interval of the next.
    lag_left: list[float] = []
    lag_right: list[float] = []
    for _, trial_group in group.groupby("trial_uid", sort=False, observed=True):
        values = trial_group.loc[trial_group["dt_s"].gt(0), "dt_s"].to_numpy()
        if len(values) >= 2:
            lag_left.extend(values[:-1])
            lag_right.extend(values[1:])
    if (
        len(lag_left) >= 2
        and np.std(lag_left) > 0
        and np.std(lag_right) > 0
    ):
        dt_lag1_autocorr = float(np.corrcoef(lag_left, lag_right)[0, 1])
    else:
        dt_lag1_autocorr = math.nan
    row = {
        group_name: group_value,
        "n_trials": group["trial_uid"].nunique(),
        "n_frames": len(group),
        "duration_s": float(total_duration),
        "frame_gaps": int(group["frame_gap_count"].sum()),
        "render_gaps": int(group["render_gap_count"].sum()),
        "duplicate_frames": int(group["duplicate_frame"].sum()),
        "nonpositive_dt": int(group["nonpositive_dt"].sum()),
        "late_frames": int(group["late_frame"].sum()),
        "late_frame_pct": 100.0 * float(group["late_frame"].sum()) / max(1, len(positive)),
        "severe_frames": int(group["severe_frame"].sum()),
        "active_late_frames": int((group["active_interval"] & group["late_frame"]).sum()),
        "active_late_frame_pct": 100.0 * float((group["active_interval"] & group["late_frame"]).sum()) / max(1, len(active)),
        "active_severe_frames": int((group["active_interval"] & group["severe_frame"]).sum()),
        "active_dt_max_ms": active.max() * 1000.0 if not active.empty else math.nan,
        "animation_dt_max_ms": animation.max() * 1000.0 if not animation.empty else math.nan,
        "robust_outliers": int(group["robust_timing_outlier"].sum()),
        "dt_mean_ms": positive.mean() * 1000.0 if not positive.empty else math.nan,
        "dt_median_ms": positive.median() * 1000.0 if not positive.empty else math.nan,
        "dt_p95_ms": positive.quantile(0.95) * 1000.0 if not positive.empty else math.nan,
        "dt_p99_ms": positive.quantile(0.99) * 1000.0 if not positive.empty else math.nan,
        "dt_max_ms": positive.max() * 1000.0 if not positive.empty else math.nan,
        "dt_lag1_autocorr": dt_lag1_autocorr,
        "avg_fps": 1.0 / positive.mean() if not positive.empty and positive.mean() > 0 else math.nan,
        "fps_1pct_low": _tail_fps(positive, 0.01),
        "fps_0_1pct_low": _tail_fps(positive, 0.001),
        "rotation_commands": int(rotations.sum()),
        "rotation_no_effect_next": int(misses.sum()),
        "rotation_no_effect_pct": 100.0 * float(misses.sum()) / max(1, int(rotations.sum())),
        "late_rotation_intervals": int(late_rotation.sum()),
        "apparently_undercompensated_late_rotation": int(undercompensated.sum()),
        "apparently_undercompensated_pct": 100.0 * float(undercompensated.sum()) / max(1, int(late_rotation.sum())),
        "max_abs_drift_s": group.groupby("trial_uid", observed=True)["drift_s"].apply(lambda s: s.abs().max()).max(),
    }
    return row


def _build_summaries(frames: pd.DataFrame) -> tuple[pd.DataFrame, pd.DataFrame]:
    if frames.empty:
        return pd.DataFrame(), pd.DataFrame()
    trial_rows = [
        _summary_row(group, group_name="trial_uid", group_value=uid)
        for uid, group in frames.groupby("trial_uid", sort=False, observed=True)
    ]
    trial_summary = pd.DataFrame(trial_rows)
    trial_meta = frames.drop_duplicates("trial_uid")[
        [
            "trial_uid",
            "run_id",
            "level_index",
            "trial_index_in_chain",
            "trial_run_counter",
            "outcome",
            "platform",
            "is_mobile",
            "source_member",
        ]
    ]
    trial_summary = trial_meta.merge(trial_summary, on="trial_uid", how="right")
    run_summary = pd.DataFrame(
        [
            _summary_row(group, group_name="run_id", group_value=run_id)
            for run_id, group in frames.groupby("run_id", sort=False, observed=True)
        ]
    )
    run_meta = frames.drop_duplicates("run_id")[["run_id", "platform", "is_mobile", "user_agent", "refresh_rate_hz_reported", "present_mode", "build_id"]]
    run_summary = run_meta.merge(run_summary, on="run_id", how="right")
    return trial_summary, run_summary


def diagnose_dataset(
    frames: pd.DataFrame,
    run_summary: pd.DataFrame,
    *,
    config: AnalysisConfig | None = None,
) -> list[AnalysisIssue]:
    """Turn metrics into conservative, human-readable diagnostic findings."""

    cfg = config or AnalysisConfig()
    findings: list[AnalysisIssue] = []
    if frames.empty:
        findings.append(AnalysisIssue("error", "dataset", "No compatible frame rows were loaded"))
        return findings

    findings.append(
        AnalysisIssue(
            "info",
            "timing",
            "present_elapsed_secs is a monotonic software marker paired by render-frame ID immediately after wgpu present(); it is a useful pacing proxy, not a direct photodiode measurement of physical onset",
        )
    )
    if run_summary["run_id"].nunique() > 1 and run_summary["build_id"].fillna("").eq("").any():
        findings.append(
            AnalysisIssue(
                "warning",
                "comparison",
                "At least one run has no build/version identifier; cross-run differences cannot be proven to come from the same game/controller build",
                field="session_info.build_id",
            )
        )
    for row in run_summary.itertuples(index=False):
        run_id = row.run_id
        if not math.isfinite(float(row.refresh_rate_hz_reported)):
            findings.append(
                AnalysisIssue(
                    "warning",
                    "timing",
                    f"Display refresh rate was not reported; {cfg.expected_hz:g} Hz is assumed for late-frame and drift thresholds",
                    run_id,
                    field="session_info.refresh_rate_hz",
                )
            )
        if row.frame_gaps:
            findings.append(AnalysisIssue("warning", "continuity", f"{row.frame_gaps} game frame number(s) are absent", run_id, field="frame_number"))
        if row.render_gaps:
            findings.append(AnalysisIssue("warning", "continuity", f"{row.render_gaps} rendered frame number(s) are absent from the log", run_id, field="render_frame_number"))
        if row.nonpositive_dt:
            findings.append(AnalysisIssue("warning", "timing", f"{row.nonpositive_dt} presentation intervals are zero or negative", run_id, field="present_elapsed_secs"))
        if row.active_late_frame_pct >= 1.0 or row.active_severe_frames:
            findings.append(
                AnalysisIssue(
                    "warning",
                    "timing",
                    f"{row.active_late_frames} active-play intervals ({row.active_late_frame_pct:.2f}%) exceed {cfg.late_dt_s * 1000:.1f} ms; {row.active_severe_frames} exceed {cfg.severe_dt_s * 1000:.1f} ms and the active-play maximum is {row.active_dt_max_ms:.2f} ms",
                    run_id,
                    field="present_elapsed_secs",
                )
            )
        if row.rotation_no_effect_next:
            findings.append(
                AnalysisIssue(
                    "warning",
                    "input",
                    f"{row.rotation_no_effect_next}/{row.rotation_commands} logged rotation-command rows have no angle change on the next row; inspect context before treating these as lost input",
                    run_id,
                    field="commands_sent",
                )
            )
        if (
            row.apparently_undercompensated_late_rotation >= 3
            and row.apparently_undercompensated_pct >= 20.0
        ):
            findings.append(
                AnalysisIssue(
                    "warning",
                    "movement",
                    f"{row.apparently_undercompensated_late_rotation}/{row.late_rotation_intervals} late intervals during logged rotation moved less than 75% of the configured time-scaled expectation; verify the deployed build and inspect these intervals",
                    run_id,
                    field="current_angle",
                )
            )
        findings.append(
            AnalysisIssue(
                "info",
                "input",
                "Ring-buffer catch-up rows contain recovered game state but commands_sent is unavailable and is logged as false; false command rows cannot by themselves prove that no key was held",
                run_id,
                field="commands_sent",
            )
        )
    return findings


def load_runs(
    sources: str | Path | Iterable[str | Path],
    *,
    config: AnalysisConfig | None = None,
) -> RunDataset:
    """Load one or more directories, ZIP archives, or JSON logs."""

    cfg = config or AnalysisConfig()
    if isinstance(sources, (str, Path)):
        paths = [Path(sources).expanduser().resolve()]
    else:
        paths = [Path(source).expanduser().resolve() for source in sources]
    issues: list[AnalysisIssue] = []
    trial_rows: list[dict[str, Any]] = []
    level_rows: list[dict[str, Any]] = []
    frame_rows: list[dict[str, Any]] = []
    seen_run_ids: dict[str, int] = {}

    for path in paths:
        if not path.exists():
            issues.append(AnalysisIssue("error", "source", f"Source does not exist: {path}"))
            continue
        base_run_id = path.stem if path.is_file() else path.name
        occurrence = seen_run_ids.get(base_run_id, 0)
        seen_run_ids[base_run_id] = occurrence + 1
        run_id = base_run_id if occurrence == 0 else f"{base_run_id}_{occurrence + 1}"
        found_document = False
        ordinal = 0
        for member, document in _iter_documents(path):
            found_document = True
            if isinstance(document, Exception):
                issues.append(AnalysisIssue("error", "source", f"Could not parse {member}: {document}", run_id))
                continue
            if _is_level_summary(member, document):
                level_rows.append(
                    _parse_level_summary(document, run_id=run_id, source=path, member=member)
                )
                continue
            found_trial = False
            for trial_member, trial in _trial_documents(document, member):
                found_trial = True
                meta, rows = _parse_trial(
                    trial,
                    run_id=run_id,
                    source=path,
                    member=trial_member,
                    ordinal=ordinal,
                    issues=issues,
                )
                ordinal += 1
                trial_rows.append(meta)
                frame_rows.extend(rows)
            if not found_trial:
                issues.append(AnalysisIssue("error", "schema", f"JSON root in {member} is not a trial object or list", run_id))
        if not found_document:
            issues.append(AnalysisIssue("error", "source", f"No trial JSON files found in {path}", run_id))

    frames = _derive_frames(_coerce_frames(pd.DataFrame(frame_rows)), cfg)
    trials = pd.DataFrame(trial_rows)
    levels = pd.DataFrame(level_rows)
    if not levels.empty:
        for name in ("level_index", "level_run_counter", "n_trial_refs"):
            levels[name] = pd.to_numeric(levels[name], errors="coerce")
        actual_counts = (
            trials.groupby(["run_id", "level_index"], dropna=False)
            .size()
            .rename("n_trial_files")
            .reset_index()
        )
        reference_counts = (
            levels.groupby(["run_id", "level_index"], dropna=False)["n_trial_refs"]
            .sum()
            .reset_index()
        )
        crosscheck = reference_counts.merge(
            actual_counts, on=["run_id", "level_index"], how="outer"
        ).fillna(0)
        for row in crosscheck.itertuples(index=False):
            if int(row.n_trial_refs) != int(row.n_trial_files):
                issues.append(
                    AnalysisIssue(
                        "warning",
                        "structure",
                        f"Level {int(row.level_index)} summary references {int(row.n_trial_refs)} trials but {int(row.n_trial_files)} trial JSON files were loaded",
                        row.run_id,
                        field="trials_runs",
                    )
                )
        if not frames.empty:
            positive = frames[frames["dt_s"].gt(0)]
            within_level = (
                positive.groupby(["run_id", "level_index"], dropna=False)["dt_ms"]
                .agg(
                    within_trial_present_dt_mean_ms="mean",
                    within_trial_present_dt_std_ms="std",
                )
                .reset_index()
            )
            levels = levels.merge(
                within_level, on=["run_id", "level_index"], how="left"
            )
            levels["summary_boundary_bias_ms"] = (
                levels["summary_present_dt_mean_ms"]
                - levels["within_trial_present_dt_mean_ms"]
            )
            biased = levels[levels["summary_boundary_bias_ms"].abs().gt(0.25)]
            if not biased.empty:
                issues.append(
                    AnalysisIssue(
                        "warning",
                        "timing",
                        "Level timing_health concatenates trials before differencing, so between-trial pauses bias present_dt_mean_ms and refresh_rate_hz_measured; use the notebook's within-trial metrics for frame pacing",
                        field="timing_health.refresh_rate_hz_measured",
                    )
                )
    elif not trials.empty:
        issues.append(
            AnalysisIssue(
                "info",
                "structure",
                "No level-summary JSON files were found; frame/trial analysis is still available",
            )
        )
    trial_summary, run_summary = _build_summaries(frames)
    issues.extend(diagnose_dataset(frames, run_summary, config=cfg))
    issue_df = pd.DataFrame([asdict(issue) for issue in issues])
    if issue_df.empty:
        issue_df = pd.DataFrame(columns=["severity", "scope", "message", "run_id", "trial_uid", "field"])
    return RunDataset(
        frames=frames,
        trials=trials,
        levels=levels,
        trial_summary=trial_summary,
        run_summary=run_summary,
        issues=issue_df,
        sources=tuple(str(path) for path in paths),
        config=cfg,
    )


def filter_frames(
    frames: pd.DataFrame,
    *,
    run_ids: Sequence[str] | None = None,
    trial_uids: Sequence[str] | None = None,
    levels: Sequence[int] | None = None,
) -> pd.DataFrame:
    selected = frames
    if run_ids:
        selected = selected[selected["run_id"].isin(run_ids)]
    if trial_uids:
        selected = selected[selected["trial_uid"].isin(trial_uids)]
    if levels:
        selected = selected[selected["level_index"].isin(levels)]
    return selected.copy()


def timing_events(
    dataset: RunDataset,
    *,
    min_dt_ms: float | None = None,
    active_only: bool = False,
    run_ids: Sequence[str] | None = None,
    limit: int | None = None,
) -> pd.DataFrame:
    """Return the longest presentation intervals with movement context."""

    frames = dataset.select(run_ids=run_ids)
    threshold = dataset.config.late_dt_s * 1000.0 if min_dt_ms is None else min_dt_ms
    events = frames[frames["dt_ms"].ge(threshold)]
    if active_only:
        events = events[events["active_interval"]]
    columns = [
        "run_id",
        "trial_uid",
        "level_index",
        "trial_run_counter",
        "frame_number",
        "render_frame_number",
        "dt_ms",
        "frame_step",
        "render_frame_step",
        "active_interval",
        "previous_rotation_command",
        "cmd_rotate_left",
        "cmd_rotate_right",
        "angle_delta_rad",
        "movement_compensation_ratio",
        "is_animating",
        "is_rendering_stopped",
        "source_member",
    ]
    events = events[[name for name in columns if name in events]].sort_values("dt_ms", ascending=False)
    return events.head(limit) if limit is not None else events


def continuity_events(
    dataset: RunDataset,
    *,
    run_ids: Sequence[str] | None = None,
) -> pd.DataFrame:
    """Return rows with game/render counter gaps, duplicates, or bad timestamps."""

    frames = dataset.select(run_ids=run_ids)
    events = frames[
        frames["frame_gap_count"].gt(0)
        | frames["render_gap_count"].gt(0)
        | frames["duplicate_frame"]
        | frames["nonpositive_dt"]
    ]
    columns = [
        "run_id",
        "trial_uid",
        "frame_number",
        "render_frame_number",
        "frame_step",
        "render_frame_step",
        "dt_ms",
        "duplicate_frame",
        "nonpositive_dt",
        "source_member",
    ]
    return events[[name for name in columns if name in events]].copy()


def _downsample(frames: pd.DataFrame, max_points: int) -> pd.DataFrame:
    if len(frames) <= max_points:
        return frames
    positions = np.linspace(0, len(frames) - 1, max_points, dtype=int)
    return frames.iloc[np.unique(positions)]


def _empty_figure(message: str) -> go.Figure:
    figure = go.Figure()
    figure.add_annotation(text=message, x=0.5, y=0.5, xref="paper", yref="paper", showarrow=False)
    figure.update_layout(template="plotly_white")
    return figure


def plot_frame_pacing(
    dataset: RunDataset,
    *,
    run_ids: Sequence[str] | None = None,
    trial_uids: Sequence[str] | None = None,
    x: str = "session_time_s",
) -> go.Figure:
    frames = _downsample(dataset.select(run_ids=run_ids, trial_uids=trial_uids), dataset.config.max_plot_points)
    if frames.empty:
        return _empty_figure("No frames match this selection")
    if x not in frames:
        raise KeyError(f"Unknown x axis: {x}")
    figure = px.scatter(
        frames,
        x=x,
        y="dt_ms",
        color="run_id",
        symbol="late_frame",
        hover_data=["trial_uid", "frame_number", "render_frame_number", "rotation_command", "current_angle"],
        title="Frame pacing: interval between logged presentation timestamps",
        render_mode="webgl",
    )
    figure.add_hline(y=dataset.config.expected_dt_s * 1000, line_dash="dot", annotation_text=f"{dataset.config.expected_hz:g} Hz target")
    figure.add_hline(y=dataset.config.late_dt_s * 1000, line_dash="dash", line_color="orange", annotation_text="late threshold")
    figure.update_layout(template="plotly_white", yaxis_title="present Δt (ms)")
    return figure


def plot_trial_overview(dataset: RunDataset, trial_uid: str) -> go.Figure:
    frames = dataset.select(trial_uids=[trial_uid])
    if frames.empty:
        return _empty_figure("Trial not found")
    frames = frames.sort_values("frame_index")
    x = frames["trial_time_s"]
    figure = make_subplots(
        rows=4,
        cols=1,
        shared_xaxes=True,
        vertical_spacing=0.05,
        subplot_titles=("Angle and alignment", "Frame pacing", "Rotation input", "Camera position"),
        specs=[[{"secondary_y": True}], [{}], [{}], [{}]],
    )
    figure.add_trace(go.Scattergl(x=x, y=frames["current_angle"], name="angle (rad)", mode="lines"), row=1, col=1, secondary_y=False)
    figure.add_trace(go.Scattergl(x=x, y=frames["current_alignment"], name="alignment", mode="lines"), row=1, col=1, secondary_y=True)
    figure.add_trace(go.Scattergl(x=x, y=frames["dt_ms"], name="present Δt (ms)", mode="lines+markers", marker={"size": 3}), row=2, col=1)
    late = frames[frames["late_frame"]]
    figure.add_trace(go.Scattergl(x=late["trial_time_s"], y=late["dt_ms"], name="late", mode="markers", marker={"color": "red", "size": 7}), row=2, col=1)
    figure.add_hline(y=dataset.config.late_dt_s * 1000, line_dash="dash", line_color="orange", row=2, col=1)
    figure.add_trace(go.Scattergl(x=x, y=frames["cmd_rotate_left"].astype(int), name="left", mode="lines", line_shape="hv"), row=3, col=1)
    figure.add_trace(go.Scattergl(x=x, y=-frames["cmd_rotate_right"].astype(int), name="right", mode="lines", line_shape="hv"), row=3, col=1)
    for axis, color in (("camera_x", "#1f77b4"), ("camera_y", "#2ca02c"), ("camera_z", "#d62728")):
        figure.add_trace(go.Scattergl(x=x, y=frames[axis], name=axis, mode="lines", line={"color": color}), row=4, col=1)
    figure.update_xaxes(title_text="trial time (s)", row=4, col=1)
    figure.update_yaxes(title_text="radians", row=1, col=1, secondary_y=False)
    figure.update_yaxes(title_text="cosine", row=1, col=1, secondary_y=True)
    figure.update_yaxes(title_text="ms", row=2, col=1)
    figure.update_yaxes(title_text="left / right", row=3, col=1)
    figure.update_layout(height=900, template="plotly_white", title=f"Trial overview — {trial_uid}", hovermode="x unified")
    return figure


def plot_flexible_2d(
    dataset: RunDataset,
    *,
    x: str,
    y: str,
    color: str = "run_id",
    run_ids: Sequence[str] | None = None,
    trial_uids: Sequence[str] | None = None,
    mode: str = "scatter",
) -> go.Figure:
    frames = _downsample(dataset.select(run_ids=run_ids, trial_uids=trial_uids), dataset.config.max_plot_points)
    if frames.empty:
        return _empty_figure("No frames match this selection")
    missing = [column for column in (x, y, color) if column not in frames]
    if missing:
        raise KeyError(f"Unknown columns: {', '.join(missing)}")
    hover = [name for name in ("trial_uid", "frame_number", "dt_ms", "current_angle") if name not in {x, y, color}]
    if mode == "line":
        figure = px.line(frames, x=x, y=y, color=color, hover_data=hover, line_group="trial_uid")
    else:
        figure = px.scatter(frames, x=x, y=y, color=color, hover_data=hover, render_mode="webgl")
    figure.update_layout(template="plotly_white", title=f"Flexible frame view: {y} vs {x}")
    return figure


def plot_trajectory_3d(
    dataset: RunDataset,
    *,
    x: str = "object_heading_x",
    y: str = "object_heading_z",
    z: str = "trial_time_s",
    color: str = "dt_ms",
    run_ids: Sequence[str] | None = None,
    trial_uids: Sequence[str] | None = None,
) -> go.Figure:
    frames = _downsample(dataset.select(run_ids=run_ids, trial_uids=trial_uids), dataset.config.max_plot_points)
    if frames.empty:
        return _empty_figure("No frames match this selection")
    missing = [column for column in (x, y, z, color) if column not in frames]
    if missing:
        raise KeyError(f"Unknown columns: {', '.join(missing)}")
    figure = px.scatter_3d(
        frames,
        x=x,
        y=y,
        z=z,
        color=color,
        symbol="run_id",
        hover_data=["trial_uid", "frame_number", "trial_time_s", "current_angle", "rotation_command"],
        title=f"3D state view: {x}, {y}, {z}",
    )
    figure.update_traces(marker={"size": 3})
    figure.update_layout(template="plotly_white", height=700)
    return figure


def plot_run_comparison(dataset: RunDataset, *, metric: str = "dt_ms") -> go.Figure:
    if dataset.frames.empty:
        return _empty_figure("No run data loaded")
    if metric not in dataset.frames:
        raise KeyError(f"Unknown comparison metric: {metric}")
    frames = _downsample(dataset.frames.dropna(subset=[metric]), dataset.config.max_plot_points)
    figure = px.box(
        frames,
        x="run_id",
        y=metric,
        color="run_id",
        points="outliers",
        hover_data=["trial_uid", "frame_number"],
        title=f"Cross-run distribution: {metric}",
    )
    figure.update_layout(template="plotly_white", showlegend=False)
    return figure


def compact_summary(dataset: RunDataset) -> dict[str, Any]:
    """Small JSON-serializable summary useful in tests and scripts."""

    return {
        "sources": list(dataset.sources),
        "compatible": dataset.compatible,
        "n_runs": int(dataset.frames["run_id"].nunique()) if not dataset.frames.empty else 0,
        "n_level_summaries": int(len(dataset.levels)),
        "n_trials": int(dataset.frames["trial_uid"].nunique()) if not dataset.frames.empty else 0,
        "n_frames": int(len(dataset.frames)),
        "issues": dataset.issues.to_dict(orient="records"),
        "runs": dataset.run_summary.to_dict(orient="records"),
    }
