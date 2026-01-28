import sys
import time
import math
import json
import os
import tkinter as tk
from tkinter import ttk, messagebox
from enum import Enum, auto

from transitions import Machine

try:
    import monkey_shared
except ImportError:
    print("Error: 'monkey_shared' module not found.")
    print("Build the shared library with 'cargo build --release -p shared --features python' and copy the resulting '.so' to controller_python/monkey_shared.so.")
    sys.exit(1)

# Shared timing constants (matching shared::timing in Rust)
REFRESH_RATE_HZ = 60
WIN_BLANK_DURATION_FRAMES = 60  # 1 second at 60fps

PHASE_LABELS = {
    0: "Playing",
    1: "Won",
}

DEFAULT_COLORS = [
    [1.0, 0.2, 0.2, 1.0],
    [0.2, 0.5, 1.0, 1.0],
    [0.2, 1.0, 0.3, 1.0],
]

DEFAULT_CONFIG = {
    "seed": 69,
    "pyramid_type": 0,
    "base_radius": 2.5,
    "height": 4.0,
    "start_orient": 0.0,
    "target_door": 5,
    "colors": DEFAULT_COLORS,
}


class WinState(Enum):
    """Win state machine for frame-based timing."""
    PLAYING = auto()
    WAITING_FOR_ANIMATION_END = auto()
    BLANK_SCREEN_ACTIVE = auto()


def load_trials(trials_path="trials.jsonl"):
    """Load trials from JSONL file."""
    trials = []
    # Try relative to script directory first
    script_dir = os.path.dirname(os.path.abspath(__file__))
    parent_dir = os.path.dirname(script_dir)
    trial_file = os.path.join(parent_dir, trials_path)

    if not os.path.exists(trial_file):
        # Fallback to current directory
        trial_file = trials_path

    try:
        with open(trial_file, 'r') as f:
            for line in f:
                line = line.strip()
                if line:
                    t = json.loads(line)
                    trials.append({
                        "seed": t["seed"],
                        "pyramid_type": t["pyramid_type"],
                        "base_radius": t["base_radius"],
                        "height": t["height"],
                        "start_orient": t["start_orient"],
                        "target_door": t["target_door"],
                        "colors": t["colors"],
                    })
        print(f"Loaded {len(trials)} trials from {trial_file}")
    except Exception as e:
        print(f"Failed to load trials: {e}. Using DEFAULT_CONFIG.")
        trials = [DEFAULT_CONFIG]
    return trials

DEFAULT_STATE = {
    "phase": 0,
    "frame_number": 0,
    "elapsed_secs": 0.0,
    "camera_radius": 0.0,
    "camera_position": [0.0, 0.0, 0.0],
    "pyramid_yaw_rad": 0.0,
    "nr_attempts": 0,
    "cosine_alignment": None,
    "is_animating": False,
    "has_won": False,
    "win_elapsed_secs": None,
}

BG_COLOR = "#1e1e1e"
CARD_COLOR = "#292929"
TEXT_PRIMARY = "#ffffff"
TEXT_ACCENT = "#00ffff"
TEXT_WARN = "#ffff00"
TEXT_BAD = "#ff5555"
TEXT_GOOD = "#00ff88"


class SharedMemory:
    def __init__(self):
        self.inner = None
        self.connect()

    def connect(self):
        try:
            self.inner = monkey_shared.SharedMemoryWrapper("monkey_game", False)
            print("Connected to shared memory interface.")
        except Exception as exc:
            print(f"SHM Connection Error: {exc}")
            self.inner = None

    def read_game_state(self):
        if not self.inner:
            self.connect()
            if not self.inner:
                return DEFAULT_STATE.copy()
        try:
            state = self.inner.read_game_state()
            data = DEFAULT_STATE.copy()
            if isinstance(state, dict):
                data.update(state)
            return data
        except Exception as exc:
            print(f"SHM Read Error: {exc}")
            self.inner = None
            return DEFAULT_STATE.copy()

    def write_commands(self, rotate_left, rotate_right, zoom_in, zoom_out, check, reset, blank_screen=False, stop_rendering=False, resume_rendering=False):
        if not self.inner:
            self.connect()
            if not self.inner:
                return
        try:
            self.inner.write_commands(
                bool(rotate_left),
                bool(rotate_right),
                bool(zoom_in),
                bool(zoom_out),
                bool(check),
                bool(reset),
                bool(blank_screen),
                bool(stop_rendering),
                bool(resume_rendering),
            )
        except Exception as exc:
            print(f"SHM Write Error: {exc}")
            self.inner = None

    def write_reset_config(self, seed, pyramid_type, base_radius, height, start_orient, target_door, colors):
        if not self.inner:
            self.connect()
            if not self.inner:
                return False
        try:
            self.inner.write_reset_config(
                int(seed),
                int(pyramid_type),
                float(base_radius),
                float(height),
                float(start_orient),
                int(target_door),
                colors,
            )
            return True
        except Exception as exc:
            print(f"SHM Config Error: {exc}")
            self.inner = None
            return False


class MonkeyGameController(tk.Tk):
    def __init__(self):
        super().__init__()
        self.title("Monkey 3D Game Controller (Python)")
        self.geometry("640x820")
        self.configure(bg=BG_COLOR)

        self.inputs = {
            "rotate_left": False,
            "rotate_right": False,
            "zoom_in": False,
            "zoom_out": False,
        }
        self.pending_check = False
        self.pending_reset = False
        self.pending_blank_screen = False
        self.pending_stop_rendering = False
        self.pending_resume_rendering = False

        self.states = ["stopped", "running"]
        self.machine = Machine(
            model=self,
            states=self.states,
            initial="stopped",
            auto_transitions=False,
        )
        self.machine.add_transition("toggle", "stopped", "running")
        self.machine.add_transition("toggle", "running", "stopped")

        self.shm_wrapper = SharedMemory()
        self.color_entries = []

        # Trials system
        self.trials = load_trials()
        self.current_trial_index = 0
        
        # Frame-based win state machine
        self.win_state = WinState.PLAYING
        self.blank_start_frame = 0

        self.setup_ui()

        self.bind_all("<KeyPress>", self.on_key_press, add="+")
        self.bind_all("<KeyRelease>", self.on_key_release, add="+")
        self.focus_set()

        self.after(16, self.loop)

    def on_enter_stopped(self):
        print("State: stopped")
        self.update_state_visual()

    def on_enter_running(self):
        print("State: running")
        self.update_state_visual()

    def get_current_trial(self):
        """Get the current trial configuration."""
        if not self.trials:
            return DEFAULT_CONFIG
        return self.trials[self.current_trial_index % len(self.trials)]

    def advance_to_next_trial(self):
        """Advance to the next trial (loops)."""
        self.current_trial_index = (self.current_trial_index + 1) % len(self.trials)
        print(f"Advancing to trial {self.current_trial_index + 1}/{len(self.trials)}")
        return self.get_current_trial()

    def push_trial_config(self, trial):
        """Push a trial configuration to shared memory."""
        return self.shm_wrapper.write_reset_config(
            trial["seed"],
            trial["pyramid_type"],
            trial["base_radius"],
            trial["height"],
            trial["start_orient"],
            trial["target_door"],
            trial["colors"],
        )

    def setup_ui(self):
        header = tk.Frame(self, bg=BG_COLOR)
        header.pack(fill="x", padx=10, pady=(10, 0))

        tk.Label(
            header,
            text="MONKEY 3D CONTROLLER",
            font=("Courier", 20, "bold"),
            fg=TEXT_ACCENT,
            bg=BG_COLOR,
        ).pack()

        self.connection_label = tk.Label(
            header,
            text="Shared Memory: Connecting...",
            font=("Courier", 10),
            fg=TEXT_WARN,
            bg=BG_COLOR,
        )
        self.connection_label.pack(pady=(6, 0))

        state_diagram = tk.Frame(self, bg=BG_COLOR)
        state_diagram.pack(fill="x", padx=10, pady=10)

        self.state_left_text = tk.Label(
            state_diagram,
            text="Phase: -\nStatus: -\nAttempts: -\nTime: -",
            font=("Courier", 10),
            fg=TEXT_PRIMARY,
            bg=BG_COLOR,
            justify="left",
            anchor="nw",
            width=18,
        )
        self.state_left_text.pack(side=tk.LEFT, padx=(0, 8), fill="y")

        self.vis_canvas = tk.Canvas(state_diagram, width=360, height=180, bg=BG_COLOR, highlightthickness=0)
        self.vis_canvas.pack(side=tk.LEFT, expand=True, fill="both")

        self.state_right_text = tk.Label(
            state_diagram,
            text="Frame: -\nAlignment: -\nCam Radius: -\nCam XYZ: -, -, -",
            font=("Courier", 10),
            fg=TEXT_PRIMARY,
            bg=BG_COLOR,
            justify="left",
            anchor="nw",
            width=22,
        )
        self.state_right_text.pack(side=tk.LEFT, padx=(8, 0), fill="y")

        self.node_stopped = self.create_state_node(120, 90, "stopped")
        self.node_running = self.create_state_node(240, 90, "running")
        self.vis_canvas.create_line(170, 90, 210, 90, arrow=tk.LAST, fill="#888888", width=2)
        self.vis_canvas.create_line(210, 110, 170, 110, arrow=tk.LAST, fill="#888888", width=2)
        self.vis_canvas.create_text(180, 70, text="Enter", fill=TEXT_PRIMARY, font=("Courier", 11))

        control_bar = tk.Frame(self, bg=BG_COLOR)
        control_bar.pack(fill="x", padx=10, pady=(0, 10))

        self.btn_toggle = tk.Button(
            control_bar,
            text="Toggle State (Enter)",
            command=self.toggle_state,
            bg="#3a3a3a",
            fg=TEXT_PRIMARY,
            relief=tk.GROOVE,
            padx=10,
            pady=6,
        )
        self.btn_toggle.pack(side=tk.LEFT, padx=(0, 10))

        self.btn_check = tk.Button(
            control_bar,
            text="Check Alignment (Space)",
            command=self.trigger_check,
            bg="#3a3a3a",
            fg=TEXT_PRIMARY,
            relief=tk.GROOVE,
            padx=10,
            pady=6,
        )
        self.btn_check.pack(side=tk.LEFT, padx=(0, 10))

        telemetry = tk.LabelFrame(
            self,
            text="Game Telemetry",
            fg=TEXT_ACCENT,
            bg=CARD_COLOR,
            font=("Courier", 12, "bold"),
            padx=12,
            pady=10,
        )
        telemetry.pack(fill="x", padx=10, pady=6)

        self.lbl_phase = self.create_metric_label(telemetry, "Phase")
        self.lbl_time = self.create_metric_label(telemetry, "Elapsed (s)")
        self.lbl_frame = self.create_metric_label(telemetry, "Frame #")
        self.lbl_attempts = self.create_metric_label(telemetry, "Attempts")
        self.lbl_status = self.create_metric_label(telemetry, "Status")
        self.lbl_alignment = self.create_metric_label(telemetry, "Alignment")
        self.lbl_win_state = self.create_metric_label(telemetry, "Win State")

        camera_frame = tk.LabelFrame(
            self,
            text="Camera",
            fg=TEXT_ACCENT,
            bg=CARD_COLOR,
            font=("Courier", 12, "bold"),
            padx=12,
            pady=10,
        )
        camera_frame.pack(fill="x", padx=10, pady=6)

        self.lbl_camera_pos = self.create_metric_label(camera_frame, "Position [x,y,z]")
        self.lbl_camera_radius = self.create_metric_label(camera_frame, "Radius")

        config_frame = tk.LabelFrame(
            self,
            text="Reset Configuration",
            fg=TEXT_ACCENT,
            bg=CARD_COLOR,
            font=("Courier", 12, "bold"),
            padx=12,
            pady=10,
        )
        config_frame.pack(fill="x", padx=10, pady=6)

        row = tk.Frame(config_frame, bg=CARD_COLOR)
        row.pack(fill="x", pady=2)
        tk.Label(row, text="Seed:", font=("Courier", 10), fg=TEXT_PRIMARY, bg=CARD_COLOR).pack(side=tk.LEFT)
        self.ent_seed = tk.Entry(row, width=12)
        self.ent_seed.pack(side=tk.LEFT, padx=(4, 12))
        self.bind_entry_return(self.ent_seed)

        tk.Label(row, text="Pyramid Type:", font=("Courier", 10), fg=TEXT_PRIMARY, bg=CARD_COLOR).pack(side=tk.LEFT)
        self.cmb_type = ttk.Combobox(row, values=["Type 1", "Type 2"], state="readonly", width=8)
        self.cmb_type.pack(side=tk.LEFT, padx=(4, 12))
        self.bind_entry_return(self.cmb_type)

        row2 = tk.Frame(config_frame, bg=CARD_COLOR)
        row2.pack(fill="x", pady=2)
        tk.Label(row2, text="Base Radius:", font=("Courier", 10), fg=TEXT_PRIMARY, bg=CARD_COLOR).pack(side=tk.LEFT)
        self.ent_base_radius = tk.Entry(row2, width=10)
        self.ent_base_radius.pack(side=tk.LEFT, padx=(4, 12))
        self.bind_entry_return(self.ent_base_radius)

        tk.Label(row2, text="Height:", font=("Courier", 10), fg=TEXT_PRIMARY, bg=CARD_COLOR).pack(side=tk.LEFT)
        self.ent_height = tk.Entry(row2, width=10)
        self.ent_height.pack(side=tk.LEFT, padx=(4, 12))
        self.bind_entry_return(self.ent_height)

        row3 = tk.Frame(config_frame, bg=CARD_COLOR)
        row3.pack(fill="x", pady=2)
        tk.Label(row3, text="Start Orient (deg):", font=("Courier", 10), fg=TEXT_PRIMARY, bg=CARD_COLOR).pack(side=tk.LEFT)
        self.ent_orientation = tk.Entry(row3, width=10)
        self.ent_orientation.pack(side=tk.LEFT, padx=(4, 12))
        self.bind_entry_return(self.ent_orientation)

        tk.Label(row3, text="Target Door:", font=("Courier", 10), fg=TEXT_PRIMARY, bg=CARD_COLOR).pack(side=tk.LEFT)
        self.ent_target_door = tk.Entry(row3, width=6)
        self.ent_target_door.pack(side=tk.LEFT, padx=(4, 12))
        self.bind_entry_return(self.ent_target_door)

        colors_header = tk.Label(
            config_frame,
            text="Face Colors (RGBA 0..1):",
            font=("Courier", 10, "bold"),
            fg=TEXT_PRIMARY,
            bg=CARD_COLOR,
        )
        colors_header.pack(pady=(8, 2))

        for idx in range(3):
            face_frame = tk.Frame(config_frame, bg=CARD_COLOR)
            face_frame.pack(fill="x", pady=2)
            tk.Label(
                face_frame,
                text=f"Face {idx + 1}:",
                font=("Courier", 10),
                fg=TEXT_PRIMARY,
                bg=CARD_COLOR,
            ).pack(side=tk.LEFT)
            entry = tk.Entry(face_frame, width=28)
            entry.pack(side=tk.LEFT, padx=(4, 0))
            self.bind_entry_return(entry)
            self.color_entries.append(entry)

        btn_row = tk.Frame(config_frame, bg=CARD_COLOR)
        btn_row.pack(fill="x", pady=(10, 0))
        self.btn_reset = tk.Button(
            btn_row,
            text="Send Config + Reset (R)",
            command=self.trigger_reset,
            bg="#aa3333",
            fg=TEXT_PRIMARY,
            relief=tk.GROOVE,
            padx=10,
            pady=6,
        )
        self.btn_reset.pack(side=tk.LEFT)

        controls_frame = tk.LabelFrame(
            self,
            text="Keyboard Shortcuts",
            fg=TEXT_ACCENT,
            bg=CARD_COLOR,
            font=("Courier", 12, "bold"),
            padx=12,
            pady=10,
        )
        controls_frame.pack(fill="x", padx=10, pady=6)

        for text in [
            "← / → : Rotate Pyramid",
            "↑ / ↓ : Zoom Camera",
            "Enter   : Toggle Controller State",
            "Space   : Check Alignment",
            "R       : Send Config + Reset",
            "B       : Blank Screen",
            "P       : Pause Rendering",
            "O       : Resume Rendering",
            "Q       : Quit",
        ]:
            tk.Label(
                controls_frame,
                text=text,
                font=("Courier", 10),
                fg=TEXT_PRIMARY,
                bg=CARD_COLOR,
                anchor="w",
            ).pack(fill="x")

        self.lbl_inputs = tk.Label(
            self,
            text="Active Inputs: None",
            font=("Courier", 10),
            fg=TEXT_WARN,
            bg=BG_COLOR,
            pady=8,
        )
        self.lbl_inputs.pack(fill="x")

        self.populate_config_defaults()
        self.update_state_visual()

    def bind_entry_return(self, widget):
        widget.bind("<Return>", self.on_entry_return)
        widget.bind("<KP_Enter>", self.on_entry_return)

    def create_state_node(self, x, y, state_name):
        radius = 50
        tag = f"node_{state_name}"
        self.vis_canvas.create_oval(
            x - radius,
            y - radius,
            x + radius,
            y + radius,
            fill="#3a3a3a",
            outline="#555555",
            width=2,
            tags=tag,
        )
        self.vis_canvas.create_text(x, y, text=state_name.capitalize(), fill=TEXT_PRIMARY, font=("Courier", 12, "bold"))
        return tag

    def create_metric_label(self, parent, label):
        frame = tk.Frame(parent, bg=CARD_COLOR)
        frame.pack(fill="x", pady=2)
        tk.Label(
            frame,
            text=f"{label}:",
            font=("Courier", 10, "bold"),
            fg=TEXT_PRIMARY,
            bg=CARD_COLOR,
            width=18,
            anchor="w",
        ).pack(side=tk.LEFT)
        value = tk.Label(
            frame,
            text="-",
            font=("Courier", 10),
            fg=TEXT_PRIMARY,
            bg=CARD_COLOR,
            anchor="w",
        )
        value.pack(side=tk.LEFT, fill="x")
        return value

    def populate_config_defaults(self):
        self.ent_seed.delete(0, tk.END)
        self.ent_seed.insert(0, str(DEFAULT_CONFIG["seed"]))

        self.cmb_type.set("Type 1" if DEFAULT_CONFIG["pyramid_type"] == 0 else "Type 2")

        self.ent_base_radius.delete(0, tk.END)
        self.ent_base_radius.insert(0, f"{DEFAULT_CONFIG['base_radius']:.2f}")

        self.ent_height.delete(0, tk.END)
        self.ent_height.insert(0, f"{DEFAULT_CONFIG['height']:.2f}")

        self.ent_orientation.delete(0, tk.END)
        self.ent_orientation.insert(0, f"{math.degrees(DEFAULT_CONFIG['start_orient']):.2f}")

        self.ent_target_door.delete(0, tk.END)
        self.ent_target_door.insert(0, str(DEFAULT_CONFIG["target_door"]))

        for entry, values in zip(self.color_entries, DEFAULT_CONFIG["colors"]):
            entry.delete(0, tk.END)
            entry.insert(0, ", ".join(f"{v:.2f}" for v in values))

    def toggle_state(self, *_):
        try:
            self.toggle()
        except Exception as exc:
            print(f"State toggle failed: {exc}")
        self.update_state_visual()

    def trigger_check(self, *_):
        if self.state == "running" and self.win_state == WinState.PLAYING:
            self.pending_check = True

    def trigger_reset(self, *_):
        if self.push_reset_config():
            self.pending_reset = True
            self.win_state = WinState.PLAYING

    def collect_reset_payload(self):
        try:
            seed = int(self.ent_seed.get())
            base_radius = float(self.ent_base_radius.get())
            height = float(self.ent_height.get())
            orient_deg = float(self.ent_orientation.get())
            target_door = int(self.ent_target_door.get())
        except ValueError as exc:
            raise ValueError(f"Invalid numeric value: {exc}") from exc

        type_label = self.cmb_type.get().strip().lower()
        pyramid_type = 1 if type_label.endswith("2") else 0
        start_orient = math.radians(orient_deg)

        colors = []
        for idx, entry in enumerate(self.color_entries):
            raw = entry.get().replace(",", " ").split()
            try:
                values = [float(token) for token in raw]
            except ValueError as exc:
                raise ValueError(f"Face {idx + 1} color values must be floats.") from exc
            if len(values) != 4:
                raise ValueError(f"Face {idx + 1} requires 4 values (r g b a).")
            colors.append(values)

        return {
            "seed": seed,
            "pyramid_type": pyramid_type,
            "base_radius": base_radius,
            "height": height,
            "start_orient": start_orient,
            "target_door": target_door,
            "colors": colors,
        }

    def push_reset_config(self):
        try:
            payload = self.collect_reset_payload()
        except ValueError as exc:
            messagebox.showerror("Invalid Reset Configuration", str(exc), parent=self)
            return False

        success = self.shm_wrapper.write_reset_config(
            payload["seed"],
            payload["pyramid_type"],
            payload["base_radius"],
            payload["height"],
            payload["start_orient"],
            payload["target_door"],
            payload["colors"],
        )
        if not success:
            messagebox.showwarning(
                "Shared Memory",
                "Failed to write reset configuration. Is the game node running?",
                parent=self,
            )
        return success

    def on_entry_return(self, event):
        self.focus_set()
        return "break"

    def on_key_press(self, event):
        key = event.keysym.lower()

        # Only process inputs when in PLAYING win state
        if self.win_state == WinState.PLAYING:
            if key == "left":
                self.inputs["rotate_left"] = True
            elif key == "right":
                self.inputs["rotate_right"] = True
            elif key == "up":
                self.inputs["zoom_in"] = True
            elif key == "down":
                self.inputs["zoom_out"] = True
            elif key == "space":
                self.trigger_check()
            elif key == "r":
                self.trigger_reset()
            elif key == "b":
                self.pending_blank_screen = True
                print("Blank screen toggled")
            elif key == "p":
                self.pending_stop_rendering = True
                print("Rendering paused")
            elif key == "o":
                self.pending_resume_rendering = True
                print("Rendering resumed")
        
        if key in ("return", "kp_enter"):
            self.toggle_state()
        elif key == "q":
            self.destroy()

    def on_key_release(self, event):
        key = event.keysym.lower()

        if key == "left":
            self.inputs["rotate_left"] = False
        elif key == "right":
            self.inputs["rotate_right"] = False
        elif key == "up":
            self.inputs["zoom_in"] = False
        elif key == "down":
            self.inputs["zoom_out"] = False

    def loop(self):
        connected = self.shm_wrapper.inner is not None
        self.connection_label.config(
            text="Shared Memory: Connected" if connected else "Shared Memory: Connecting...",
            fg=TEXT_GOOD if connected else TEXT_WARN,
        )

        state = self.shm_wrapper.read_game_state()

        # Read game state
        has_won = state.get("has_won", False)
        is_animating = state.get("is_animating", False)
        current_frame = state.get("frame_number", 0)

        # Win state machine (frame-based timing)
        if self.win_state == WinState.PLAYING:
            if has_won:
                print(f"Trial {self.current_trial_index + 1} won! Waiting for animation to complete...")
                self.win_state = WinState.WAITING_FOR_ANIMATION_END
        elif self.win_state == WinState.WAITING_FOR_ANIMATION_END:
            if not is_animating:
                print(f"Animation complete. Activating blank screen for {WIN_BLANK_DURATION_FRAMES} frames")
                
                # Prepare next trial config
                next_trial_index = (self.current_trial_index + 1) % len(self.trials)
                next_trial = self.trials[next_trial_index]
                self.push_trial_config(next_trial)
                
                # Send commands: reset + blank_screen + stop_rendering
                self.pending_reset = True
                self.pending_blank_screen = True
                self.pending_stop_rendering = True
                
                self.blank_start_frame = current_frame
                self.win_state = WinState.BLANK_SCREEN_ACTIVE
        elif self.win_state == WinState.BLANK_SCREEN_ACTIVE:
            frames_elapsed = current_frame - self.blank_start_frame
            
            if frames_elapsed >= WIN_BLANK_DURATION_FRAMES:
                print(f"Blank screen complete ({frames_elapsed} frames). Resuming.")
                
                # Send commands: blank_screen (toggle off) + resume_rendering
                self.pending_blank_screen = True
                self.pending_resume_rendering = True
                
                # Advance trial index
                self.current_trial_index = (self.current_trial_index + 1) % len(self.trials)
                print(f"Advancing to trial {self.current_trial_index + 1}/{len(self.trials)}")
                
                self.win_state = WinState.PLAYING

        # Update UI
        phase_code = int(state.get("phase", 0))
        phase_name = PHASE_LABELS.get(phase_code, f"Unknown ({phase_code})")
        self.lbl_phase.config(text=f"{phase_name} [{phase_code}]")

        elapsed = state.get("elapsed_secs") or 0.0
        self.lbl_time.config(text=f"{elapsed:.2f}s")

        frame_number = state.get("frame_number", 0)
        self.lbl_frame.config(text=str(frame_number))

        attempts = state.get("nr_attempts", 0)
        self.lbl_attempts.config(text=str(attempts))

        camera_position = state.get("camera_position") or [0.0, 0.0, 0.0]
        if len(camera_position) < 3:
            camera_position = [0.0, 0.0, 0.0]
        self.lbl_camera_pos.config(
            text=f"[{camera_position[0]:.2f}, {camera_position[1]:.2f}, {camera_position[2]:.2f}]"
        )

        camera_radius = state.get("camera_radius") or 0.0
        self.lbl_camera_radius.config(text=f"{camera_radius:.2f}")

        align = state.get("cosine_alignment")
        if align is None:
            align_display = "n/a"
            self.lbl_alignment.config(text=align_display, fg=TEXT_PRIMARY)
        else:
            color = TEXT_GOOD if align > 0.95 else TEXT_BAD
            align_display = f"{align:.3f}"
            self.lbl_alignment.config(text=align_display, fg=color)

        # Win state display
        if self.win_state == WinState.PLAYING:
            win_state_text = "Playing"
            win_state_color = TEXT_PRIMARY
        elif self.win_state == WinState.WAITING_FOR_ANIMATION_END:
            win_state_text = "Wait Anim"
            win_state_color = TEXT_WARN
        else:
            frames_elapsed = current_frame - self.blank_start_frame
            win_state_text = f"Blank {frames_elapsed}/{WIN_BLANK_DURATION_FRAMES}"
            win_state_color = TEXT_ACCENT
        self.lbl_win_state.config(text=win_state_text, fg=win_state_color)

        if state.get("has_won"):
            win_time = state.get("win_elapsed_secs")
            if win_time:
                status_text = f"WINNER! ({win_time:.2f}s)"
            else:
                status_text = "WINNER!"
            status_color = TEXT_GOOD
        elif state.get("is_animating"):
            status_text = "Animating..."
            status_color = TEXT_WARN
        else:
            status_text = "Ready"
            status_color = TEXT_PRIMARY
        self.lbl_status.config(text=status_text, fg=status_color)

        left_text = (
            f"Phase: {phase_name}\n"
            f"Status: {status_text}\n"
            f"Attempts: {attempts}\n"
            f"Time: {elapsed:.2f}s"
        )
        self.state_left_text.config(text=left_text)

        right_text = (
            f"Frame: {frame_number}\n"
            f"Alignment: {align_display}\n"
            f"Cam Radius: {camera_radius:.2f}\n"
            f"Cam XYZ: {camera_position[0]:.2f}, {camera_position[1]:.2f}, {camera_position[2]:.2f}"
        )
        self.state_right_text.config(text=right_text)

        check_trigger = self.pending_check
        reset_trigger = self.pending_reset
        blank_screen_trigger = self.pending_blank_screen
        stop_rendering_trigger = self.pending_stop_rendering
        resume_rendering_trigger = self.pending_resume_rendering

        # Only send movement inputs when in PLAYING win state and controller is running
        if self.state == "running" and self.win_state == WinState.PLAYING:
            rotate_left = self.inputs["rotate_left"]
            rotate_right = self.inputs["rotate_right"]
            zoom_in = self.inputs["zoom_in"]
            zoom_out = self.inputs["zoom_out"]
        else:
            rotate_left = rotate_right = zoom_in = zoom_out = False

        self.shm_wrapper.write_commands(
            rotate_left,
            rotate_right,
            zoom_in,
            zoom_out,
            check_trigger,
            reset_trigger,
            blank_screen_trigger,
            stop_rendering_trigger,
            resume_rendering_trigger,
        )

        active_inputs = []
        if rotate_left:
            active_inputs.append("Rotate Left")
        if rotate_right:
            active_inputs.append("Rotate Right")
        if zoom_in:
            active_inputs.append("Zoom In")
        if zoom_out:
            active_inputs.append("Zoom Out")
        if check_trigger:
            active_inputs.append("Check")
        if reset_trigger:
            active_inputs.append("Reset")
        if blank_screen_trigger:
            active_inputs.append("Blank")
        if stop_rendering_trigger:
            active_inputs.append("Pause")
        if resume_rendering_trigger:
            active_inputs.append("Resume")

        if active_inputs:
            self.lbl_inputs.config(text=f"Active Inputs: {', '.join(active_inputs)}")
        else:
            self.lbl_inputs.config(text="Active Inputs: None")

        self.pending_check = False
        self.pending_reset = False
        self.pending_blank_screen = False
        self.pending_stop_rendering = False
        self.pending_resume_rendering = False

        self.after(16, self.loop)

    def update_state_visual(self):
        self.vis_canvas.itemconfig(self.node_stopped, fill="#3a3a3a")
        self.vis_canvas.itemconfig(self.node_running, fill="#3a3a3a")
        active_tag = self.node_running if self.state == "running" else self.node_stopped
        self.vis_canvas.itemconfig(active_tag, fill="#006644" if self.state == "running" else "#663300")


def main():
    print("Starting Monkey Game Controller (Python GUI)...")
    print(f"Frame-based timing: {WIN_BLANK_DURATION_FRAMES} frames = {WIN_BLANK_DURATION_FRAMES / REFRESH_RATE_HZ:.2f}s at {REFRESH_RATE_HZ}Hz")
    print("Waiting for shared memory region 'monkey_game'...")

    shm_test = None
    for attempt in range(20):
        try:
            shm_test = monkey_shared.SharedMemoryWrapper("monkey_game", False)
            print("Shared memory is available.")
            break
        except Exception:
            time.sleep(0.5)
            print("Retrying...")
    if shm_test is None:
        print("Shared memory not ready yet. The controller will continue attempting to connect.")
    else:
        del shm_test

    app = MonkeyGameController()
    app.mainloop()


if __name__ == "__main__":
    main()
