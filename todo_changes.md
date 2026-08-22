# Change list (2026-08 round)

Status: `todo` · `wip` · `done` · `dropped`. One line per item; details only
where a decision was made. Both platforms (native + web) must be checked for
anything controller- or SHM-side.

| # | Item | Status | Notes |
|---|------|--------|-------|
| 1 | Faces per object, one that allows you to win | dropped | Not needed (2026-08-22). |
| 2 | Background/foreground textures configurable per level | todo | Ground + wall are hardcoded (`Rock024` / `Tiles017`) and spawned once in `setup_environment` ([setup.rs](game_node/src/utils/setup.rs)). Needs per-level SHM fields + material swap on reset. |
| 3 | Particles always active + controllable | todo | [fog.rs](game_node/src/utils/fog.rs) spawns fireflies only on a green win. |
| 4 | "mandami quelle" (send background candidates) | dropped | Not needed (2026-08-22). |
| 5 | Left bar = average trial position across chains | todo | Today `score_bar_value` is a ±1 score counter, controller-owned. Decide: replace or add a second bar. |
| 6 | Chain of circles at the top = level progression | todo | New UI in [ui.rs](game_node/src/utils/ui.rs) + SHM (level index, level count). |
| 7 | Clock showing remaining session time | wip | Round disc, spent time sweeping clockwise from noon, top-center, shown **only on the between-trial black screen**. Anchored at the end of the game's loading countdown — the same anchor now used by the `MAX_SESSION_DURATION` cap (moved from first-trial-start). New live SHM f32 `session_time_left` (fraction left, `<0` hides) written per tick by both controllers; game draws it as a conic gradient in [ui.rs](game_node/src/utils/ui.rs). On web the break screen is a DOM overlay over the canvas; its black backdrop is now dropped as soon as the game reports `is_blank` (`setOverlayOpaque`), so the one game-side clock is visible on both platforms. Radius in `game_constants::SESSION_CLOCK_RADIUS_PX`. Native side not yet compiled/verified. |
| 8 | Tutorial as video | todo | Hosting constraint: the VM has 2 GB RAM. |
| 9 | German instructions | done | 2026-08-22 — EN/DE toggle on the player name + instructions pages in [index.html](deploy_frontend/index.html); English is the default and lives in the markup, German in `I18N_DE`. The choice is passed to the game via `sessionStorage.lang`, and `controller_main.js` translates its own player-facing text (`TEXT_EN` / `TEXT_DE`): texture-loading, first-level instructions, the per-trial "press the screen" prompt, and the end-of-session popup. Requires a terser rebuild of `controller_main.min.js`. |
| 10 | Widen the back-wall light beam | done | 2026-08-22, fixed by the user directly: `outer_angle` PI/4 → PI/2.5 in [pyramid.rs:241](game_node/src/utils/pyramid.rs#L241). |
| 11 | Textures saved for analysis | todo (verify) | `trial_config` in each trial log already carries `textures` + `decorations_texture` as enum indices. Confirm what is actually missing. |
| 12 | Win recorded as a bool, not `idx + 1` | todo | **Priority.** A chain that beats its last trial goes to `idx = n`, but the trial actually shown is `min(idx, n-1)`, and a later RETROCEED erases the fact that it was beaten. Log the played index + an explicit "beaten" bool. |
