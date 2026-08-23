import json
import zipfile

import pytest

from tools.utils.run_analysis import (
    discover_run_sources,
    load_runs,
    plot_flexible_2d,
    plot_frame_pacing,
    plot_run_comparison,
    plot_trial_overview,
    plot_trajectory_3d,
)


def _trial(*, missing_present=False):
    frames = {}
    for number, present in enumerate((10.0, 10.016, 10.050)):
        state = {
            "frame_number": number,
            "render_frame_number": number,
            "present_elapsed_secs": present,
            "current_angle": 1.0 - number * 0.04,
            "current_alignment": number * 0.1,
            "camera_x": 1.0,
            "camera_y": 2.0,
            "camera_z": 3.0 + number,
            "is_animating": False,
            "is_rendering_stopped": False,
        }
        if missing_present:
            state.pop("present_elapsed_secs")
        frames[str(number)] = {
            "state_read": state,
            "commands_sent": {
                "rotate_right": number < 2,
                "check": number == 1,
            },
            "check_input_event_elapsed_secs": 2.5 if number == 1 else None,
        }
    return {
        "level_index": 1,
        "trial_index_in_chain": 2,
        "active_chain": 3,
        "trial_run_counter": 4,
        "outcome": "advance",
        "session_info": {
            "platform": "wasm",
            "refresh_rate_hz": 60,
            "present_mode": "fifo",
            "app_start_unix_ns": 1_000_000_000,
        },
        "frames": frames,
    }


def test_load_zip_and_derive_metrics(tmp_path):
    archive_path = tmp_path / "run.zip"
    with zipfile.ZipFile(archive_path, "w") as archive:
        archive.writestr("run/level_001/trials/trial.json", json.dumps(_trial()))
        archive.writestr("run/level_001/level_summary.json", "{}")

    dataset = load_runs(archive_path)

    assert dataset.compatible
    assert len(dataset.frames) == 3
    assert dataset.run_summary.iloc[0]["late_frames"] == 1
    assert dataset.run_summary.iloc[0]["frame_gaps"] == 0
    assert dataset.frames.iloc[-1]["dt_ms"] == pytest.approx(34.0)
    assert dataset.frames.iloc[1]["check_input_event_elapsed_secs"] == pytest.approx(2.5)
    assert dataset.frames.iloc[1]["check_input_event_unix_s"] == pytest.approx(3.5)
    assert not (dataset.issues["severity"] == "error").any()


def test_missing_timing_field_is_reported(tmp_path):
    path = tmp_path / "trial.json"
    path.write_text(json.dumps(_trial(missing_present=True)), encoding="utf-8")

    dataset = load_runs(path)

    timing_errors = dataset.issues[
        (dataset.issues["severity"] == "error")
        & (dataset.issues["field"] == "present_elapsed_secs")
    ]
    assert len(timing_errors) == 1


def test_discovery_and_all_plots(tmp_path):
    path = tmp_path / "trial.json"
    path.write_text(json.dumps(_trial()), encoding="utf-8")
    dataset = load_runs(path)
    trial_uid = dataset.frames.iloc[0]["trial_uid"]

    assert path.resolve() in discover_run_sources(tmp_path)
    figures = [
        plot_frame_pacing(dataset),
        plot_trial_overview(dataset, trial_uid),
        plot_flexible_2d(dataset, x="trial_time_s", y="current_angle"),
        plot_trajectory_3d(dataset),
        plot_run_comparison(dataset),
    ]
    assert all(len(figure.data) > 0 for figure in figures)
