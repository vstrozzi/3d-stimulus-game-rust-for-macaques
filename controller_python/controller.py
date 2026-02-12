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

# ─── Constants imported from shared/src/constants.rs via monkey_shared ───
REFRESH_RATE_HZ = monkey_shared.REFRESH_RATE_HZ
WIN_BLANK_DURATION_FRAMES = monkey_shared.WIN_BLANK_DURATION_FRAMES

# UI Colors
BG_COLOR = "#1e1e1e"
CARD_COLOR = "#292929"
HEADER_BG = "#333333"
TEXT_PRIMARY = "#ffffff"
TEXT_SECONDARY = "#aaaaaa"
TEXT_ACCENT = "#00ffff"
TEXT_WARN = "#ffff00"
TEXT_BAD = "#ff5555"
TEXT_GOOD = "#00ff88"
CANVAS_BG = "#222222"

# Default colors from Rust constants (PYRAMID_COLORS: [[f32;4];3])
DEFAULT_COLORS = [list(face) for face in monkey_shared.PYRAMID_COLORS]

# Default config sourced from shared/src/constants.rs via monkey_shared
DEFAULT_CONFIG = {
    "decoration_seeds": list(monkey_shared.DECORATION_SEEDS),
    "base_radius": monkey_shared.PYRAMID_BASE_RADIUS,
    "height": monkey_shared.PYRAMID_HEIGHT,
    "start_orient": monkey_shared.PYRAMID_START_ANGLE_OFFSET_RAD,
    "target_door": monkey_shared.PYRAMID_TARGET_DOOR_INDEX,
    "colors": DEFAULT_COLORS,
    # Per-face arrays [3] matching Rust SharedGameStructure
    "decorations_count": list(monkey_shared.PYRAMID_DECORATIONS_COUNT),
    "decorations_size": list(monkey_shared.PYRAMID_DECORATIONS_SIZE),
    "cosine_alignment_threshold": monkey_shared.COSINE_ALIGNMENT_TO_WIN,
    "door_anim_fade_out": monkey_shared.DOOR_ANIM_FADE_OUT,
    "door_anim_stay_open": monkey_shared.DOOR_ANIM_STAY_OPEN,
    "door_anim_fade_in": monkey_shared.DOOR_ANIM_FADE_IN,
    "main_spotlight_intensity": monkey_shared.SPOTLIGHT_LIGHT_INTENSITY,
    "max_spotlight_intensity": monkey_shared.MAX_SPOTLIGHT_INTENSITY,
    "ambient_brightness": monkey_shared.GLOBAL_AMBIENT_LIGHT_INTENSITY,
}

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
    # Config part of the structure (read back)
    "decoration_seeds": [0, 0, 0],
    "base_radius": 0.0,
    "height": 0.0,
    "start_orient": 0.0,
    "target_door": 0,
}

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
                        "decoration_seeds": t.get("decoration_seeds", DEFAULT_CONFIG["decoration_seeds"]),
                        "base_radius": t["base_radius"],
                        "height": t["height"],
                        "start_orient": t["start_orient"],
                        "target_door": t["target_door"],
                        "colors": t["colors"],
                        "decorations_count": t.get("decorations_count", DEFAULT_CONFIG["decorations_count"]),
                        "decorations_size": t.get("decorations_size", DEFAULT_CONFIG["decorations_size"]),
                        "cosine_alignment_threshold": t.get("cosine_alignment_threshold", DEFAULT_CONFIG["cosine_alignment_threshold"]),
                        "door_anim_fade_out": t.get("door_anim_fade_out", DEFAULT_CONFIG["door_anim_fade_out"]),
                        "door_anim_stay_open": t.get("door_anim_stay_open", DEFAULT_CONFIG["door_anim_stay_open"]),
                        "door_anim_fade_in": t.get("door_anim_fade_in", DEFAULT_CONFIG["door_anim_fade_in"]),
                        "main_spotlight_intensity": t.get("main_spotlight_intensity", DEFAULT_CONFIG["main_spotlight_intensity"]),
                        "max_spotlight_intensity": t.get("max_spotlight_intensity", DEFAULT_CONFIG["max_spotlight_intensity"]),
                        "ambient_brightness": t.get("ambient_brightness", DEFAULT_CONFIG["ambient_brightness"]),
                    })
        print(f"Loaded {len(trials)} trials from {trial_file}")
    except Exception as e:
        print(f"Failed to load trials: {e}. Using DEFAULT_CONFIG.")
        trials = [DEFAULT_CONFIG]
    return trials


class SharedMemory:
    def __init__(self):
        self.inner = None
        self.connect()

    def connect(self):
        try:
            self.inner = monkey_shared.SharedMemoryWrapper("monkey_game")
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
            state = self.inner.read_game_structure()
            data = DEFAULT_STATE.copy()
            if isinstance(state, dict):
                data.update(state)
            return data
        except Exception as exc:
            print(f"SHM Read Error: {exc}")
            self.inner = None
            return DEFAULT_STATE.copy()

    def write_commands(self, rotate_left, rotate_right, zoom_in, zoom_out, check, reset, blank_screen=False, stop_rendering=False, resume_rendering=False, animation_door=False):
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
                bool(animation_door)
            )
        except Exception as exc:
            print(f"SHM Write Error: {exc}")
            self.inner = None

    def write_reset_config(self, decoration_seeds, base_radius, height, start_orient, target_door, colors,
                           decorations_count, decorations_size,
                           cosine_alignment_threshold,
                           door_anim_fade_out, door_anim_stay_open, door_anim_fade_in,
                           main_spotlight_intensity, max_spotlight_intensity, ambient_brightness):
        """Write config to shared memory. decorations_count: [u32;3], decorations_size: [f32;3]."""
        if not self.inner:
            self.connect()
            if not self.inner:
                return False
        try:
            self.inner.write_game_structure(
                [int(x) for x in decoration_seeds],
                float(base_radius),
                float(height),
                float(start_orient),
                int(target_door),
                colors,
                [int(x) for x in decorations_count],
                [float(x) for x in decorations_size],
                float(cosine_alignment_threshold),
                float(door_anim_fade_out),
                float(door_anim_stay_open),
                float(door_anim_fade_in),
                float(main_spotlight_intensity),
                float(ambient_brightness),
                float(max_spotlight_intensity),
            )
            return True
        except Exception as exc:
            print(f"SHM Config Error: {exc}")
            self.inner = None
            return False


class MonkeyGameController(tk.Tk):
    def __init__(self):
        super().__init__()
        self.title("Monkey 3D Game Controller (Target FSM Monitor)")
        self.geometry("1400x900")
        self.configure(bg=BG_COLOR)

        # Game State FSM (Shadow + Control)
        # States: playing, won, animating, blank
        self.states = ['playing', 'won', 'animating', 'blank']
        self.machine = Machine(model=self, states=self.states, initial='playing')
        
        # Transitions
        self.machine.add_transition('win_game', 'playing', 'won')
        self.machine.add_transition('start_anim', 'won', 'animating') # Usually implies win -> anim
        self.machine.add_transition('start_blank', 'animating', 'blank')
        self.machine.add_transition('reset_game', 'blank', 'playing')
        
        # Manual overrides (for robustness)
        self.machine.add_transition('force_reset', '*', 'playing')
        self.machine.add_transition('force_anim', 'playing', 'animating') # If we detect anim in playing (e.g. door opening?)

        self.shm_wrapper = SharedMemory()
        self.inputs = {
            "rotate_left": False, "rotate_right": False,
            "zoom_in": False, "zoom_out": False
        }
        self.triggers = {
            "check": False, "reset": False, 
            "blank": False, "pause": False, "resume": False,
            "animation_door": False, "retry": False
        }
        
        # Configuration
        self.trials = load_trials()
        self.current_trial_index = 0
        self.color_entries = []
        
        # Automation State
        self.blank_start_frame = 0
        self.inferred_win = False
        
        # State capture for Pause/Resume
        self.paused_state = None  # Will hold (config, yaw, camera)
        self.is_paused = False
        
        # UI Setup
        self.setup_ui()
        
        # Bindings
        self.bind_all("<KeyPress>", self.on_key_press, add="+")
        self.bind_all("<KeyRelease>", self.on_key_release, add="+")
        
        # Loop
        self.after(16, self.loop)

    def setup_ui(self):
        # Main Layout: 2 Columns (Left: Controls/Data, Right: FSM)
        self.columnconfigure(0, weight=6, uniform="group1") # More width for data
        self.columnconfigure(1, weight=4, uniform="group1")
        self.rowconfigure(0, weight=1)

        # LEFT PANEL: Controls & Data Monitor
        left_panel = tk.Frame(self, bg=BG_COLOR)
        left_panel.grid(row=0, column=0, sticky="nsew", padx=10, pady=10)
        
        # Header
        tk.Label(left_panel, text="DASHBOARD & CONTROLS", font=("Courier", 16, "bold"), fg=TEXT_ACCENT, bg=BG_COLOR).pack(anchor="w")
        
        # 1. Inputs Section
        input_frame = tk.LabelFrame(left_panel, text="CONTROLS", font=("Courier", 12, "bold"), fg=TEXT_PRIMARY, bg=CARD_COLOR)
        input_frame.pack(fill="x", pady=5)
        self.create_input_grid(input_frame)
        
        # 2. Data Monitor Section (Split View)
        monitor_frame = tk.LabelFrame(left_panel, text="SYSTEM MONITOR", font=("Courier", 12, "bold"), fg=TEXT_PRIMARY, bg=CARD_COLOR)
        monitor_frame.pack(fill="both", expand=True, pady=5)
        
        # Split into Config (Left) vs Real-Time (Right)
        monitor_frame.columnconfigure(0, weight=1)
        monitor_frame.columnconfigure(1, weight=1)
        
        # --- Config Column ---
        tk.Label(monitor_frame, text="CONFIGURATION (Reset)", font=("Courier", 11, "bold"), fg=TEXT_WARN, bg=CARD_COLOR).grid(row=0, column=0, sticky="w", padx=10, pady=5)
        self.tree_config = ttk.Treeview(monitor_frame, columns=("Val"), show="tree", height=15)
        self.tree_config.column("#0", width=180)
        self.tree_config.column("Val", width=120)
        self.tree_config.grid(row=1, column=0, sticky="nsew", padx=5, pady=5)
        
        # --- State Column ---
        tk.Label(monitor_frame, text="REAL-TIME STATE (Engine)", font=("Courier", 11, "bold"), fg=TEXT_GOOD, bg=CARD_COLOR).grid(row=0, column=1, sticky="w", padx=10, pady=5)
        self.tree_state = ttk.Treeview(monitor_frame, columns=("Val"), show="tree", height=15)
        self.tree_state.column("#0", width=180)
        self.tree_state.column("Val", width=120)
        self.tree_state.grid(row=1, column=1, sticky="nsew", padx=5, pady=5)
        
        # Style
        style = ttk.Style()
        style.theme_use("clam")
        style.configure("Treeview", background="#333333", foreground="white", fieldbackground="#333333", font=("Courier", 10))

        # RIGHT PANEL: FSM Visualization
        right_panel = tk.Frame(self, bg=BG_COLOR)
        right_panel.grid(row=0, column=1, sticky="nsew", padx=10, pady=10)
        
        tk.Label(right_panel, text="LOGIC FSM", font=("Courier", 16, "bold"), fg=TEXT_ACCENT, bg=BG_COLOR).pack(anchor="w")
        self.fsm_canvas = tk.Canvas(right_panel, bg=CANVAS_BG, highlightthickness=0)
        self.fsm_canvas.pack(fill="both", expand=True, pady=5)
        
        self.fsm_nodes = {}
        self.fsm_arrows = {}
        self.draw_fsm_layout()
        
        self.lbl_written_command = tk.Label(left_panel, text="", bg=BG_COLOR) # Dummy/Hidden

    def draw_fsm_layout(self):
        cw = 680
        ch = 800
        cx = cw // 2
        cy = ch // 2
        
        # Define Node Positions
        node_pos = {
            "playing": (cx, cy - 250),
            "won": (cx + 200, cy - 50),
            "animating": (cx, cy + 150),
            "blank": (cx - 200, cy - 50)
        }
        
        # Draw arrows (Edges)
        # Format: (start_node, end_node, label, tag)
        edges = [
            ("playing", "won", "Win (Phase=1)", "edge_win"),
            ("won", "animating", "Wait Clean", "edge_won_anim"),
            ("animating", "blank", "Anim Done", "edge_anim_blank"),
            ("blank", "playing", "Timeout (Reset)", "edge_blank_play"),
            
            ("playing", "playing", "Reset (Manual)", "edge_manual_reset"),
            ("playing", "animating", "Anim Active", "edge_play_anim"),
            ("animating", "playing", "Anim Done", "edge_anim_play")
        ]
        
        for start, end, label, tag in edges:
            self.draw_arrow(node_pos[start], node_pos[end], label, tag)
            
        # Draw Nodes
        for name, (x, y) in node_pos.items():
            self.draw_node(x, y, name)

    def draw_node(self, x, y, name):
        r = 60
        tag = f"node_{name}"
        # Shadow
        self.fsm_canvas.create_oval(x-r+4, y-r+4, x+r+4, y+r+4, fill="#111111", outline="", tags="shadow")
        # Body
        self.fsm_canvas.create_oval(x-r, y-r, x+r, y+r, fill="#444444", outline="#777777", width=3, tags=(tag, "node", "oval"))
        # Text
        self.fsm_canvas.create_text(x, y, text=name.upper(), font=("Courier", 12, "bold"), fill="white", tags=(tag, "node_text", "text"))
        self.fsm_nodes[name] = tag

    def draw_arrow(self, p1, p2, label, tag):
        x1, y1 = p1
        x2, y2 = p2
        
        # Simple shortening to not overlap nodes
        angle = math.atan2(y2 - y1, x2 - x1)
        r = 70 # Node radius + spacing
        sx = x1 + r * math.cos(angle)
        sy = y1 + r * math.sin(angle)
        ex = x2 - r * math.cos(angle)
        ey = y2 - r * math.sin(angle)
        
        if p1 == p2:
            # Self loop
             self.fsm_canvas.create_arc(x1-50, y1-80, x1+50, y1-20, start=0, extent=180, style="arc", outline="#666666", width=2, tags=(tag, "arrow", "arc"))
             midx, midy = x1, y1-90
        else:
            self.fsm_canvas.create_line(sx, sy, ex, ey, arrow=tk.LAST, fill="#666666", width=2, tags=(tag, "arrow", "line"))
            midx, midy = (sx+ex)/2, (sy+ey)/2
            
        self.fsm_canvas.create_text(midx, midy - 10, text=label, font=("Courier", 9), fill="#aaaaaa", tags=(tag, "arrow_text", "text"))

    def highlight_node(self, name):
        # Reset all nodes
        self.fsm_canvas.itemconfig("oval", fill="#444444", outline="#777777")
        self.fsm_canvas.itemconfig("text", fill="white")
        self.fsm_canvas.itemconfig("arrow_text", fill="#aaaaaa")
        
        # Highlight current
        tag = self.fsm_nodes.get(name)
        if tag:
            items = self.fsm_canvas.find_withtag(tag)
            for item in items:
                t = self.fsm_canvas.type(item)
                if t == "oval":
                    self.fsm_canvas.itemconfig(item, fill=TEXT_ACCENT, outline="white")
                elif t == "text":
                     self.fsm_canvas.itemconfig(item, fill="black")

    def highlight_arrow(self, tag, active=False):
        color = TEXT_WARN if active else "#666666"
        width = 4 if active else 2
        
        items = self.fsm_canvas.find_withtag(tag)
        for item in items:
            t = self.fsm_canvas.type(item)
            if t == "line":
                self.fsm_canvas.itemconfig(item, fill=color, width=width)
            elif t == "arc":
                self.fsm_canvas.itemconfig(item, outline=color, width=width)
            elif t == "text":
                self.fsm_canvas.itemconfig(item, fill=TEXT_WARN if active else "#aaaaaa")

    def create_input_grid(self, parent):
        grid = tk.Frame(parent, bg=CARD_COLOR)
        grid.pack(fill="x", padx=10, pady=10)
        self.indicators = {}
        input_layout = [
            ("L/R Arrow (Rot)", "rotate_left"),
            ("U/D Arrow (Zoom)", "zoom_in"),
            ("Space (Check/Anim)", "check"),
            ("R (Reset)", "reset"),
            ("B (Blank)", "blank"),
            ("P (Pause)", "pause"),
            ("O (Resume)", "resume"),
        ]
        
        for i, (label, key) in enumerate(input_layout):
            row = i // 4
            col = i % 4
            f = tk.Frame(grid, bg=CARD_COLOR)
            f.grid(row=row, column=col, sticky="w", padx=10, pady=2)
            ind = tk.Label(f, text="●", font=("Arial", 14), fg="#555555", bg=CARD_COLOR)
            ind.pack(side="left")
            lbl = tk.Label(f, text=label, font=("Courier", 10), fg=TEXT_PRIMARY, bg=CARD_COLOR)
            lbl.pack(side="left", padx=2)
            self.indicators[key] = ind
            # For zoom/rot pairs, map multiple keys safely or simplify
            if key == "rotate_left": self.indicators["rotate_right"] = ind # Shared indicator 
            if key == "zoom_in": self.indicators["zoom_out"] = ind

    def update_data_table(self, state):
        # 1. Update Config Tree (Static-ish)
        # We use DEFAULT_CONFIG or last reset values if we tracked them better, 
        # but DEFAULT_CONFIG is what we have for now plus self.trials logic.
        
        # Get Current Trial Config
        trial = self.trials[self.current_trial_index % len(self.trials)]
        
        cfg_data = {
            "Seeds": str(trial.get("decoration_seeds", DEFAULT_CONFIG["decoration_seeds"])),
            "Target Door": trial.get("target_door"),
            "Threshold": trial.get("cosine_alignment_threshold", DEFAULT_CONFIG["cosine_alignment_threshold"]),
            "Decors Count": str(trial.get("decorations_count", DEFAULT_CONFIG["decorations_count"])),
            "Decors Size": str(trial.get("decorations_size", DEFAULT_CONFIG["decorations_size"])),
            "Spot Intensity": f"{trial.get('main_spotlight_intensity', DEFAULT_CONFIG['main_spotlight_intensity']):.1e}",
            "Anim Open": trial.get("door_anim_fade_out"),
            "Anim Stay": trial.get("door_anim_stay_open"),
        }
        
        # Rebuild or update details? Rebuild is safer for simplicity
        if not self.tree_config.get_children():
            for k, v in cfg_data.items():
                self.tree_config.insert("", "end", iid=k, text=k, values=(str(v),))
        else:
            for k, v in cfg_data.items():
                if self.tree_config.exists(k):
                    self.tree_config.item(k, values=(str(v),))

        # 2. Update State Tree (Dynamic)
        align = state.get("cosine_alignment")
        align_str = f"{align:.4f}" if (align is not None and align <= 1.5) else "N/A"
        
        st_data = {
            "Frame": state.get("frame_number"),
            "Time": f"{state.get('elapsed_secs', 0.0):.2f}s",
            "Attempts": state.get("attempts", 0),
            "Alignment": align_str,
            "Angle (Rad)": f"{state.get('current_angle', 0.0):.4f}",
            "Yaw (Rad)": f"{state.get('pyramid_yaw_rad', 0.0):.4f}",
            "Animating": str(state.get("is_animating", False)),
            "Cam Radius": f"{state.get('camera_radius', 0.0):.2f}",
            "FSM State": self.state.upper()
        }
        
        if not self.tree_state.get_children():
            for k, v in st_data.items():
                self.tree_state.insert("", "end", iid=k, text=k, values=(str(v),))
        else:
            for k, v in st_data.items():
                if self.tree_state.exists(k):
                    self.tree_state.item(k, values=(str(v),))

    def loop(self):
        # 1. Read Game State
        state = self.shm_wrapper.read_game_state()
        current_frame = state.get("frame_number", 0)
        is_animating = state.get("is_animating", False)
        current_alignment = state.get("cosine_alignment")
        
        auto_reset = False
        auto_blank = False
        auto_stop = False
        auto_resume = False
        auto_anim = False # Triggers animation_door
        
        # ---------------------------------------------------------
        # PAUSE / RESUME LOGIC (SHM Flags)
        # ---------------------------------------------------------
        if self.triggers['pause'] and not self.is_paused:
            print("Action: PAUSE - Stopping Rendering.")
            self.is_paused = True
            auto_stop = True # Send Stop Rendering flag

        elif self.triggers['resume'] and self.is_paused:
            print("Action: RESUME - Resuming Rendering.")
            self.is_paused = False
            auto_resume = True # Send Resume Rendering flag

        if self.is_paused:
            # While paused, we still update UI but don't run game logic FSM
            if auto_stop or auto_resume: 
                 # We must send these commands immediately
                 pass
            else:
                 # Just update UI and wait
                 self.process_inputs_and_update_ui(state, auto_stop, auto_resume)
                 self.after(16, self.loop)
                 return

        # ---------------------------------------------------------
        # NORMAL FSM LOGIC
        # ---------------------------------------------------------
        if self.state == 'playing':
            threshold = state.get("cosine_alignment_threshold", 0.9)
            
            # Win Inference Logic (Require Check + Good Alignment)
            if self.triggers["check"]:
                # User pressed Space
                # We ALWAYS trigger animation (door open) visually per request
                auto_anim = True 
                
                # Check if it counts as a WIN
                if current_alignment is not None and current_alignment <= 1.5:
                    if current_alignment > threshold:
                        print(f"Valid Win: {current_alignment:.4f} > {threshold}")
                        self.inferred_win = True
                        self.win_game() # -> won
                    else:
                        print(f"Check Failed: {current_alignment:.4f} < {threshold}")
                
        elif self.state == 'won':
            if is_animating:
                self.start_anim() # -> animating
            else:
                auto_anim = True # Ensure it starts

        elif self.state == 'animating':
            if not is_animating:
                if self.inferred_win:
                    self.start_blank() # -> blank
                    self.blank_start_frame = current_frame
                    # Prepare next trial
                    self.current_trial_index += 1
                    trial = self.trials[self.current_trial_index % len(self.trials)]
                    
                    # Ensure commands_seq > 0 before writing config (required by Rust guard)
                    self.shm_wrapper.write_commands(
                        False, False, False, False,
                        False, True, False, False, False, False  # reset=True
                    )
                    self.shm_wrapper.write_reset_config(
                        trial.get("decoration_seeds", DEFAULT_CONFIG["decoration_seeds"]), trial["base_radius"], 
                        trial["height"], trial["start_orient"], trial["target_door"], trial["colors"],
                        trial.get("decorations_count", DEFAULT_CONFIG["decorations_count"]),
                        trial.get("decorations_size", DEFAULT_CONFIG["decorations_size"]),
                        trial.get("cosine_alignment_threshold", DEFAULT_CONFIG["cosine_alignment_threshold"]),
                        trial.get("door_anim_fade_out", DEFAULT_CONFIG["door_anim_fade_out"]),
                        trial.get("door_anim_stay_open", DEFAULT_CONFIG["door_anim_stay_open"]),
                        trial.get("door_anim_fade_in", DEFAULT_CONFIG["door_anim_fade_in"]),
                        trial.get("main_spotlight_intensity", DEFAULT_CONFIG["main_spotlight_intensity"]),
                        trial.get("max_spotlight_intensity", DEFAULT_CONFIG["max_spotlight_intensity"]),
                        trial.get("ambient_brightness", DEFAULT_CONFIG["ambient_brightness"])
                    )
                    auto_reset = True
                    auto_blank = True
                else:
                    self.force_reset() # -> playing (Animation done, back to game)

        elif self.state == 'blank':
            if (current_frame - self.blank_start_frame) >= WIN_BLANK_DURATION_FRAMES:
                auto_blank = True # Toggle OFF (Actually Reset clears it? No, Blank is separate)
                # Wait, blank command toggles. If we want it OFF, we send it again if active?
                # Actually reset handles clean slate? 
                # Let's just send reset.
                self.reset_game() # -> playing

        # Apply triggers
        if auto_reset: self.triggers['reset'] = True
        if auto_blank: self.triggers['blank'] = True
        if auto_stop: self.triggers['pause'] = True
        if auto_resume: self.triggers['resume'] = True
        if auto_anim: self.triggers['animation_door'] = True

        self.process_inputs_and_update_ui(state)
        self.after(16, self.loop)

    def process_inputs_and_update_ui(self, state, f_stop=False, f_resume=False):
        # Write to SHM
        self.shm_wrapper.write_commands(
            self.inputs["rotate_left"], self.inputs["rotate_right"],
            self.inputs["zoom_in"], self.inputs["zoom_out"],
            self.triggers["check"],
            self.triggers["reset"],
            self.triggers["blank"],
            self.triggers["pause"] or f_stop,
            self.triggers["resume"] or f_resume,
            self.triggers["animation_door"]
        )
        
        # Clear triggers
        for k in self.triggers: self.triggers[k] = False
        
        # Update UI
        self.update_data_table(state)
        self.highlight_node(self.state)
        self.highlight_arrow("edge_win", active=(self.state == 'won'))

    def on_key_release(self, event):
        key = event.keysym.lower()
        if key == "left": self.inputs["rotate_left"] = False
        elif key == "right": self.inputs["rotate_right"] = False
        elif key == "up": self.inputs["zoom_in"] = False
        elif key == "down": self.inputs["zoom_out"] = False
        elif key == "space": self.triggers["check"] = False
        elif key == "r": self.triggers["reset"] = False
        elif key == "c": self.triggers["retry"] = False
        elif key == "b": self.triggers["blank"] = False
        elif key == "p": self.triggers["pause"] = False
        elif key == "o": self.triggers["resume"] = False

    def trigger_reset_config(self):
        # Pick next trial
        trial = self.trials[self.current_trial_index % len(self.trials)]
        self.current_trial_index += 1
        
        # Ensure commands_seq > 0 by sending a write_commands first (required by Rust guard)
        self.shm_wrapper.write_commands(
            self.inputs["rotate_left"], self.inputs["rotate_right"],
            self.inputs["zoom_in"], self.inputs["zoom_out"],
            False, True, False, False, False, False  # reset=True
        )
        
        print(f"Sending Reset Config (Trial {self.current_trial_index})")
        self.shm_wrapper.write_reset_config(
            trial.get("decoration_seeds", DEFAULT_CONFIG["decoration_seeds"]),
            trial["base_radius"],
            trial["height"],
            trial["start_orient"],
            trial["target_door"],
            trial["colors"],
            trial.get("decorations_count", DEFAULT_CONFIG["decorations_count"]),
            trial.get("decorations_size", DEFAULT_CONFIG["decorations_size"]),
            trial.get("cosine_alignment_threshold", DEFAULT_CONFIG["cosine_alignment_threshold"]),
            trial.get("door_anim_fade_out", DEFAULT_CONFIG["door_anim_fade_out"]),
            trial.get("door_anim_stay_open", DEFAULT_CONFIG["door_anim_stay_open"]),
            trial.get("door_anim_fade_in", DEFAULT_CONFIG["door_anim_fade_in"]),
            trial.get("main_spotlight_intensity", DEFAULT_CONFIG["main_spotlight_intensity"]),
            trial.get("max_spotlight_intensity", DEFAULT_CONFIG["max_spotlight_intensity"]),
            trial.get("ambient_brightness", DEFAULT_CONFIG["ambient_brightness"])
        )

    def trigger_retry(self):
        print("Action: RETRY (C) - Resetting to current trial start.")
        self.triggers["retry"] = True
        
        try:
            # 2. Pause Controller
            self.is_paused = True
            
            # 3. Get Current Trial Config
            # Note: current_trial_index points to NEXT trial usually if we just finished one, or CURRENT if playing?
            # In `trigger_reset_config`, we do `current_trial_index % len`. Then increment.
            # So `current_trial_index` points to the *next* one to be loaded.
            # The *currently active* one is `current_trial_index - 1`.
            
            idx = (self.current_trial_index - 1) % len(self.trials)
            trial = self.trials[idx]
            
            # Save for Resume logic capabilities (if paused state matters)
            # For Retry, we want to stay paused until Resume is pressed? 
            # User said "pause".
            self.paused_state = {
                "trial_idx": idx,
                "yaw": trial["start_orient"], # Reset orientation to initial!
                "config": trial.copy()
            }
            
            # 4. Ensure commands_seq > 0 before writing config (required by Rust guard)
            self.shm_wrapper.write_commands(
                False, False, False, False,
                False, True, False, False, False, False  # reset=True
            )
            # Send Reset Config (Initial Layout)
            self.shm_wrapper.write_reset_config(
                trial.get("decoration_seeds", DEFAULT_CONFIG["decoration_seeds"]), trial["base_radius"], 
                trial["height"], trial["start_orient"], trial["target_door"], trial["colors"],
                trial.get("decorations_count", DEFAULT_CONFIG["decorations_count"]),
                trial.get("decorations_size", DEFAULT_CONFIG["decorations_size"]),
                trial.get("cosine_alignment_threshold", DEFAULT_CONFIG["cosine_alignment_threshold"]),
                trial.get("door_anim_fade_out", DEFAULT_CONFIG["door_anim_fade_out"]),
                trial.get("door_anim_stay_open", DEFAULT_CONFIG["door_anim_stay_open"]),
                trial.get("door_anim_fade_in", DEFAULT_CONFIG["door_anim_fade_in"]),
                trial.get("main_spotlight_intensity", DEFAULT_CONFIG["main_spotlight_intensity"]),
                trial.get("max_spotlight_intensity", DEFAULT_CONFIG["max_spotlight_intensity"]),
                trial.get("ambient_brightness", DEFAULT_CONFIG["ambient_brightness"])
            )
            
            # 5. Send Commands: Reset + Blank
            self.triggers["reset"] = True
            self.triggers["blank"] = True # Set blank=True
            
            # 6. Schedule Unblank (0.2s)
            self.after(200, self.unblank_callback)
            
        except Exception as e:
            print(f"Retry error: {e}")

    def unblank_callback(self):
        # Turn off blank screen
        # We need to send blank=False.
        # But `process_inputs` clears triggers every frame.
        # If we just set trigger here, it will be sent next frame.
        self.triggers["blank"] = True # Toggle? 
        # Wait, the native game toggle logic is: if true, toggle. 
        # If we sent True in trigger_retry, screen went BLACK.
        # Now we send True again to toggle OFF?
        # Yes, `apply_blank_screen` in rust toggles if pending is true.
        # So sending True again triggers toggle.
        pass # Wait, if we set triggers["blank"] = True here, loop will pick it up next frame.
             # However, we are PAUSED. loop() calls process_inputs_and_update_ui which calls process_inputs.
             # process_inputs writes triggers.
             
        # Force a write immediately? Or just set trigger.
        print("Retry: Unblanking.")
        self.triggers["blank"] = True 

    def on_key_press(self, event):
        key = event.keysym.lower()
        if key == "left": self.inputs["rotate_left"] = True
        elif key == "right": self.inputs["rotate_right"] = True
        elif key == "up": self.inputs["zoom_in"] = True
        elif key == "down": self.inputs["zoom_out"] = True
        elif key == "space": 
            self.triggers["check"] = True
            self.triggers["animation_door"] = True
        elif key == "r": 
            self.triggers["reset"] = True
            self.trigger_reset_config() # Send new config once
        elif key == "c":
            self.trigger_retry()
        elif key == "b": self.triggers["blank"] = True
        elif key == "p": self.triggers["pause"] = True
        elif key == "o": self.triggers["resume"] = True
        elif key == "q": self.destroy()

if __name__ == "__main__":
    app = MonkeyGameController()
    app.mainloop()


