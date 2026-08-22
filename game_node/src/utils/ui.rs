use bevy::prelude::*;
use shared::constants::game_constants::{
    UI_REFERENCE_HEIGHT,
    PROGRESS_BAR_DOTS_SIZE, PROGRESS_BAR_MAX_SIZE, PROGRESS_BAR_WRAP_AROUND_SIZE,
    SESSION_CLOCK_RADIUS_PX,
};
use crate::utils::objects::{
    ScoreBarUI, UIEntity, ScoreBarChain, ScoreBarDot, ScoreBarRoot,
    LeftScoreBarRoot, LeftScoreBarFill, GameStateLocal, SessionClock, BlankScreen,
};
use std::f32::consts::TAU;
use shared::constants::pyramid_constants::{LIGHT_RED, LIGHT_GREEN};

const LEFT_SCORE_BAR_WIDTH_PX: f32 = 24.0;
const LEFT_SCORE_BAR_HEIGHT_PERCENT: f32 = 60.0;
const LEFT_SCORE_BAR_LEFT_PX: f32 = 16.0;

const TRIAL_BAR_LEFT_PX: f32 = LEFT_SCORE_BAR_LEFT_PX + LEFT_SCORE_BAR_WIDTH_PX + 10.0;
const TRIAL_BAR_HEIGHT_PERCENT: f32 = LEFT_SCORE_BAR_HEIGHT_PERCENT;
const TRIAL_BAR_COL_WIDTH_PX: f32 = PROGRESS_BAR_DOTS_SIZE + 8.0;

/// Spawns the persistent trial-progress dot pool at startup.
///
/// Vertical layout: dots stack top→bottom in a column; when there are more
/// trials than `PROGRESS_BAR_WRAP_AROUND_SIZE`, extra dots wrap into another
/// column to the right. Total vertical span is constant (matches the score
/// bar) regardless of trial count. `update_score_bar` toggles `Node.display`
/// so only entities with index < `progress_bar_size` are laid out.
pub fn spawn_score_bar_pool(mut commands: Commands) {
    let num_cols = PROGRESS_BAR_MAX_SIZE.div_ceil(PROGRESS_BAR_WRAP_AROUND_SIZE);

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(TRIAL_BAR_LEFT_PX),
                top: Val::Percent((100.0 - TRIAL_BAR_HEIGHT_PERCENT) / 2.0),
                height: Val::Percent(TRIAL_BAR_HEIGHT_PERCENT),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Stretch,
                ..default()
            },
            ScoreBarRoot,
        ))
        .with_children(|parent| {
            for col in 0..num_cols {
                let col_start = col * PROGRESS_BAR_WRAP_AROUND_SIZE;
                let col_end =
                    (col_start + PROGRESS_BAR_WRAP_AROUND_SIZE).min(PROGRESS_BAR_MAX_SIZE);

                parent
                    .spawn((
                        Node {
                            width: Val::Px(TRIAL_BAR_COL_WIDTH_PX),
                            height: Val::Percent(100.0),
                            padding: UiRect::all(Val::Px(2.0)),
                            flex_direction: FlexDirection::ColumnReverse,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::SpaceEvenly,
                            margin: UiRect::right(Val::Px(2.0)),
                            display: Display::None,
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.1, 0.1, 0.1, 0.0)),
                        ScoreBarUI { row_start: col_start },
                    ))
                    .with_children(|col_parent| {
                        for i in col_start..col_end {
                            if i > col_start {
                                col_parent.spawn((
                                    Node {
                                        width: Val::Px(1.0),
                                        height: Val::Px(0.0),
                                        flex_grow: 1.0,
                                        display: Display::None,
                                        ..default()
                                    },
                                    ScoreBarChain { index: i - 1 },
                                ));
                            }
                            col_parent.spawn((
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
                left: Val::Px(LEFT_SCORE_BAR_LEFT_PX),
                top: Val::Percent((100.0 - LEFT_SCORE_BAR_HEIGHT_PERCENT) / 2.0),
                width: Val::Px(LEFT_SCORE_BAR_WIDTH_PX),
                height: Val::Percent(LEFT_SCORE_BAR_HEIGHT_PERCENT),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::FlexEnd,
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
                    width: Val::Percent(100.0),
                    height: Val::Percent(50.0),
                    ..default()
                },
                BackgroundColor(LIGHT_RED.with_alpha(LEFT_SCORE_BAR_ALPHA)),
                LeftScoreBarFill,
            ));
        });
}

pub fn update_left_score_bar(
    local_game_struct: Res<GameStateLocal>,
    mut root_query: Query<&mut Node, (With<LeftScoreBarRoot>, Without<LeftScoreBarFill>)>,
    mut fill_query: Query<(&mut Node, &mut BackgroundColor), With<LeftScoreBarFill>>,
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

    let value = gs.score_bar_value.min(max);
    let t = value as f32 / max as f32;
    let r = LIGHT_RED.to_linear();
    let g = LIGHT_GREEN.to_linear();
    let lerped = LinearRgba::new(
        r.red   + (g.red   - r.red)   * t,
        r.green + (g.green - r.green) * t,
        r.blue  + (g.blue  - r.blue)  * t,
        LEFT_SCORE_BAR_ALPHA,
    );
    let target_color = Color::LinearRgba(lerped);
    let target_height = Val::Percent(t * 100.0);

    for (mut node, mut bg) in fill_query.iter_mut() {
        if node.height != target_height {
            node.height = target_height;
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
const SESSION_CLOCK_MARGIN_PX: f32 = 16.0;
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
            top: Val::Px(SESSION_CLOCK_MARGIN_PX),
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

