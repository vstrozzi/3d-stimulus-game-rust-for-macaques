use bevy::prelude::*;
use shared::constants::game_constants::{
    UI_REFERENCE_HEIGHT,
    PROGRESS_BAR_DOTS_SIZE, PROGRESS_BAR_MAX_SIZE, PROGRESS_BAR_WRAP_AROUND_SIZE,
    SESSION_CLOCK_RADIUS_PX, TRIAL_BAR_PULSE_HZ,
};
use crate::utils::objects::{
    ScoreBarUI, UIEntity, ScoreBarChain, ScoreBarDot, ScoreBarRoot,
    LeftScoreBarRoot, LeftScoreBarFill, LeftScoreBarDelta, GameStateLocal, SessionClock,
    BlankScreen,
};
use std::f32::consts::TAU;
use shared::constants::pyramid_constants::{LIGHT_RED, LIGHT_GREEN};

// Trial-progress bar: mean trial position across chains, filling left to right
// along the bottom of the screen. The components and systems keep their
// historical `LeftScoreBar*` names.
const TRIAL_BAR_HEIGHT_PX: f32 = 24.0;
const TRIAL_BAR_WIDTH_PERCENT: f32 = 60.0;
const TRIAL_BAR_BOTTOM_PX: f32 = 16.0;

// Level chain: one circle per level, filled left to right as levels complete.
const LEVEL_CHAIN_TOP_PX: f32 = 16.0;
const LEVEL_CHAIN_WIDTH_PERCENT: f32 = 40.0;
const LEVEL_CHAIN_ROW_HEIGHT_PX: f32 = PROGRESS_BAR_DOTS_SIZE + 8.0;

/// Spawns the persistent level-chain dot pool at startup.
///
/// Horizontal layout centered at the top: one circle per level, connected by
/// short bars, filling left to right as levels are completed. With more levels
/// than `PROGRESS_BAR_WRAP_AROUND_SIZE` the extra circles wrap onto a second
/// row underneath. `update_score_bar` toggles `Node.display` so only entities
/// with index < `progress_bar_size` are laid out, and colors those below
/// `progress_bar_cur_size` as filled — the controller now feeds it level
/// counts instead of trial counts.
pub fn spawn_score_bar_pool(mut commands: Commands) {
    let num_rows = PROGRESS_BAR_MAX_SIZE.div_ceil(PROGRESS_BAR_WRAP_AROUND_SIZE);

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(LEVEL_CHAIN_TOP_PX),
                left: Val::Percent((100.0 - LEVEL_CHAIN_WIDTH_PERCENT) / 2.0),
                width: Val::Percent(LEVEL_CHAIN_WIDTH_PERCENT),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                ..default()
            },
            ScoreBarRoot,
        ))
        .with_children(|parent| {
            for row in 0..num_rows {
                let row_start = row * PROGRESS_BAR_WRAP_AROUND_SIZE;
                let row_end =
                    (row_start + PROGRESS_BAR_WRAP_AROUND_SIZE).min(PROGRESS_BAR_MAX_SIZE);

                parent
                    .spawn((
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Px(LEVEL_CHAIN_ROW_HEIGHT_PX),
                            padding: UiRect::all(Val::Px(2.0)),
                            flex_direction: FlexDirection::Row,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::SpaceEvenly,
                            margin: UiRect::bottom(Val::Px(2.0)),
                            display: Display::None,
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.1, 0.1, 0.1, 0.0)),
                        ScoreBarUI { row_start },
                    ))
                    .with_children(|row_parent| {
                        for i in row_start..row_end {
                            if i > row_start {
                                row_parent.spawn((
                                    Node {
                                        width: Val::Px(0.0),
                                        height: Val::Px(1.0),
                                        flex_grow: 1.0,
                                        display: Display::None,
                                        ..default()
                                    },
                                    ScoreBarChain { index: i - 1 },
                                ));
                            }
                            row_parent.spawn((
                                Node {
                                    width: Val::Px(PROGRESS_BAR_DOTS_SIZE),
                                    height: Val::Px(PROGRESS_BAR_DOTS_SIZE),
                                    flex_shrink: 0.0,
                                    border_radius: BorderRadius::all(Val::Px(
                                        PROGRESS_BAR_DOTS_SIZE / 2.0,
                                    )),
                                    display: Display::None,
                                    ..default()
                                },
                                ScoreBarDot { index: i },
                            ));
                        }
                    });
            }
        });
}

const LEFT_SCORE_BAR_ALPHA: f32 = 0.30;

pub fn spawn_left_score_bar(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(TRIAL_BAR_BOTTOM_PX),
                left: Val::Percent((100.0 - TRIAL_BAR_WIDTH_PERCENT) / 2.0),
                width: Val::Percent(TRIAL_BAR_WIDTH_PERCENT),
                height: Val::Px(TRIAL_BAR_HEIGHT_PX),
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::FlexStart,
                border: UiRect::all(Val::Px(2.0)),
                display: Display::None,
                ..default()
            },
            BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.25)),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.15)),
            LeftScoreBarRoot,
        ))
        .with_children(|parent| {
            parent.spawn((
                Node {
                    width: Val::Percent(50.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                BackgroundColor(LIGHT_RED.with_alpha(LEFT_SCORE_BAR_ALPHA)),
                LeftScoreBarFill,
            ));
            // Out of flow, on top of the fill: the step being gained or lost.
            parent.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    left: Val::Percent(0.0),
                    width: Val::Percent(0.0),
                    display: Display::None,
                    ..default()
                },
                BackgroundColor(LIGHT_GREEN.with_alpha(LEFT_SCORE_BAR_ALPHA)),
                LeftScoreBarDelta,
            ));
        });
}

/// The bar's fill colour at fill fraction `frac`: red at empty, lerping to
/// green as it fills. Used for the fill itself and for the blinking step, so
/// the two always agree.
fn bar_fill_color(frac: f32, alpha: f32) -> Color {
    let r = LIGHT_RED.to_linear();
    let g = LIGHT_GREEN.to_linear();
    Color::LinearRgba(LinearRgba::new(
        r.red + (g.red - r.red) * frac,
        r.green + (g.green - r.green) * frac,
        r.blue + (g.blue - r.blue) * frac,
        alpha,
    ))
}

pub fn update_left_score_bar(
    time: Res<Time>,
    local_game_struct: Res<GameStateLocal>,
    // Value that finished animating; the controller pushes the new one when it
    // issues the terminal door animation, so the gap is the step in transition.
    mut settled: Local<u32>,
    mut root_query: Query<
        &mut Node,
        (With<LeftScoreBarRoot>, Without<LeftScoreBarFill>, Without<LeftScoreBarDelta>),
    >,
    mut fill_query: Query<
        (&mut Node, &mut BackgroundColor),
        (With<LeftScoreBarFill>, Without<LeftScoreBarDelta>),
    >,
    mut delta_query: Query<(&mut Node, &mut BackgroundColor), With<LeftScoreBarDelta>>,
) {
    let gs = &local_game_struct.0;
    let max = gs.score_bar_max;
    let desired_display = if max == 0 { Display::None } else { Display::Flex };
    for mut node in root_query.iter_mut() {
        if node.display != desired_display {
            node.display = desired_display;
        }
    }
    if max == 0 {
        return;
    }

    // The bar only moves when the door animation ends; until then it holds the
    // old value and the delta overlay blinks over the step being won or lost.
    let target = gs.score_bar_value.min(max);
    if !gs.is_animating {
        *settled = target;
    }
    let value = (*settled).min(max);
    let t = value as f32 / max as f32;

    let in_transition = gs.is_animating && target != value;
    let pulse = if in_transition {
        (time.elapsed_secs() * TRIAL_BAR_PULSE_HZ * TAU).sin() * 0.5 + 0.5
    } else {
        0.0
    };
    let lo = value.min(target) as f32 / max as f32;
    let hi = value.max(target) as f32 / max as f32;
    // Gaining a step: fade between empty and the colour the bar will have once
    // it settles. Losing one: swing between that colour and red. Alpha stays at
    // the bar's own transparency in both cases (except the fade-from-empty).
    let delta_color = if target > value {
        bar_fill_color(hi, LEFT_SCORE_BAR_ALPHA * pulse)
    } else {
        let from = bar_fill_color(hi, LEFT_SCORE_BAR_ALPHA).to_linear();
        let to = LIGHT_RED.to_linear();
        Color::LinearRgba(LinearRgba::new(
            from.red + (to.red - from.red) * pulse,
            from.green + (to.green - from.green) * pulse,
            from.blue + (to.blue - from.blue) * pulse,
            LEFT_SCORE_BAR_ALPHA,
        ))
    };
    for (mut node, mut bg) in delta_query.iter_mut() {
        let desired = if in_transition { Display::Flex } else { Display::None };
        if node.display != desired {
            node.display = desired;
        }
        if !in_transition {
            continue;
        }
        node.left = Val::Percent(lo * 100.0);
        node.width = Val::Percent((hi - lo) * 100.0);
        *bg = BackgroundColor(delta_color);
    }
    let target_color = bar_fill_color(t, LEFT_SCORE_BAR_ALPHA);
    let target_width = Val::Percent(t * 100.0);

    for (mut node, mut bg) in fill_query.iter_mut() {
        if node.width != target_width {
            node.width = target_width;
        }
        if bg.0 != target_color {
            *bg = BackgroundColor(target_color);
        }
    }
}

// ── Session clock ──────────────────────────────────────────────────────────
// A disc centered at the top of the screen, shown only during the black
// screen between trials — never while the player is manipulating the object.
// The time already spent is a dark wedge that grows clockwise from noon; what
// is left stays pale. Drawn as a conic gradient with two hard stops, so there
// is no mesh and no shader: only the two stop angles move each frame.
// Directly under the level chain, both centered at the top.
const SESSION_CLOCK_TOP_PX: f32 = LEVEL_CHAIN_TOP_PX + LEVEL_CHAIN_ROW_HEIGHT_PX + 12.0;
const SESSION_CLOCK_LEFT: Color = Color::srgba(1.0, 1.0, 1.0, 0.30);
const SESSION_CLOCK_SPENT: Color = Color::srgba(0.0, 0.0, 0.0, 0.45);

/// Conic gradient for a clock that has `spent_angle` radians swept off.
fn session_clock_gradient(spent_angle: f32) -> Gradient {
    Gradient::Conic(ConicGradient::new(
        UiPosition::CENTER,
        vec![
            AngularColorStop::new(SESSION_CLOCK_SPENT, 0.0),
            AngularColorStop::new(SESSION_CLOCK_SPENT, spent_angle),
            AngularColorStop::new(SESSION_CLOCK_LEFT, spent_angle),
            AngularColorStop::new(SESSION_CLOCK_LEFT, TAU),
        ],
    ))
}

pub fn spawn_session_clock(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            // Horizontally centered: the left edge sits at 50% of the screen,
            // pulled back by one radius. Both are px scaled by the same
            // `UiScale`, so it stays centered at every resolution.
            left: Val::Percent(50.0),
            margin: UiRect::left(Val::Px(-SESSION_CLOCK_RADIUS_PX)),
            top: Val::Px(SESSION_CLOCK_TOP_PX),
            width: Val::Px(SESSION_CLOCK_RADIUS_PX * 2.0),
            height: Val::Px(SESSION_CLOCK_RADIUS_PX * 2.0),
            border_radius: BorderRadius::MAX,
            display: Display::None,
            ..default()
        },
        // The blank screen is at 1000; the clock has to sit on top of it.
        GlobalZIndex(1001),
        BackgroundGradient(vec![session_clock_gradient(0.0)]),
        SessionClock,
    ));
}

/// Redraws the clock from `session_time_left` (fraction of the session left,
/// pushed by the controller every tick). Only visible while the between-trial
/// black screen is up; a negative fraction hides it entirely.
pub fn update_session_clock(
    local_game_struct: Res<GameStateLocal>,
    blank_query: Query<(), With<BlankScreen>>,
    mut query: Query<(&mut Node, &mut BackgroundGradient), With<SessionClock>>,
) {
    let left = f32::from_bits(local_game_struct.0.session_time_left);
    let in_break = !blank_query.is_empty();
    let desired_display = if left < 0.0 || !in_break { Display::None } else { Display::Flex };
    for (mut node, mut gradient) in query.iter_mut() {
        if node.display != desired_display {
            node.display = desired_display;
        }
        if desired_display == Display::None {
            continue;
        }
        let spent_angle = (1.0 - left.clamp(0.0, 1.0)) * TAU;
        let next = session_clock_gradient(spent_angle);
        if gradient.0.first() != Some(&next) {
            gradient.0 = vec![next];
        }
    }
}

/// Updates UI scale based on window size for responsive design
/// Targets 1080p (1920x1080) as the reference resolution
pub fn update_ui_scale(mut ui_scale: ResMut<UiScale>, window_query: Query<&Window>) {
    let Ok(window) = window_query.single() else {
        return;
    };

    // Calculate scale based on window height (reference: 1080p)
    let scale = window.height() / UI_REFERENCE_HEIGHT;

    // Clamp scale to reasonable bounds (0.5x to 2.0x)
    let clamped_scale = scale.clamp(0.5, 2.0);

    ui_scale.0 = clamped_scale;
}

/// Helper to despawn ui entities given a mutable commands reference
pub fn despawn_ui(commands: &mut Commands, query: &Query<Entity, With<UIEntity>>) {
    for entity in query {
        commands.entity(entity).despawn();
    }
}

