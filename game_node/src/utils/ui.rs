use bevy::prelude::*;
// TODO: add to shared memory
use shared::constants::game_constants::{
    SCORE_BAR_BORDER_THICKNESS, SCORE_BAR_HEIGHT, SCORE_BAR_TOP_OFFSET, SCORE_BAR_WIDTH_PERCENT, UI_REFERENCE_HEIGHT
};

use crate::utils::objects::{
    ScoreBarFill, ScoreBarUI, UIEntity,
};

/// Spawns the energy score bar at the top center of the screen
pub fn spawn_score_bar(commands: &mut Commands) {
    // Container for the score bar (centered at top)
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                top: Val::Px(SCORE_BAR_TOP_OFFSET),
                justify_content: JustifyContent::Center,
                ..default()
            },
            UIEntity,
        ))
        .with_children(|parent| {
            // Outer border/background of the bar
            parent
                .spawn((
                    Node {
                        width: Val::Percent(SCORE_BAR_WIDTH_PERCENT),
                        height: Val::Px(SCORE_BAR_HEIGHT),
                        border: UiRect::all(Val::Px(SCORE_BAR_BORDER_THICKNESS)),
                        padding: UiRect::all(Val::Px(2.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.1, 0.1, 0.1, 0.5)), // Dark subtle background
                    ScoreBarUI,
                ))
                .with_children(|bar_parent| {
                    // Inner fill bar (starts empty)
                    bar_parent.spawn((
                        Node {
                            width: Val::Percent(0.0), // Starts empty
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.2, 0.6, 1.0, 0.3)), // Dim cyan glow when empty
                        ScoreBarFill,
                    ));
                });
        });
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

