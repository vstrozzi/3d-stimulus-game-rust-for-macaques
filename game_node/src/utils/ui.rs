use bevy::prelude::*;
// TODO: add to shared memory
use shared::constants::game_constants::{
    SCORE_BAR_HEIGHT, SCORE_BAR_TOP_OFFSET, SCORE_BAR_WIDTH_PERCENT, UI_REFERENCE_HEIGHT,
    PROGRESS_BAR_DOTS_SIZE, PROGRESS_BAR_WRAP_AROUND_SIZE
};
use crate::utils::objects::{
    ScoreBarUI, UIEntity, ScoreBarChain, ScoreBarDot
};
/// Spawns the energy score bar as a chain of dots at the top center of the screen (trials progression)
pub fn spawn_score_bar(
    commands: &mut Commands,
    progress_bar_size: u32,
    ) {

    // Don't show the bar if the size is 0
    if progress_bar_size == 0 {
        return;
    }

    // Calculate how many rows we need
    let num_rows = (progress_bar_size + PROGRESS_BAR_WRAP_AROUND_SIZE - 1) / PROGRESS_BAR_WRAP_AROUND_SIZE;

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                top: Val::Px(SCORE_BAR_TOP_OFFSET),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                flex_direction: FlexDirection::Column,
                ..default()
            },
            UIEntity,
        ))
        .with_children(|parent| {
            // Create more Rows of levels if needed
            for row in 0..num_rows {
                let row_start = row * PROGRESS_BAR_WRAP_AROUND_SIZE;
                let row_end = (row_start + PROGRESS_BAR_WRAP_AROUND_SIZE).min(progress_bar_size);

                parent
                    .spawn((
                        Node {
                            width: Val::Percent(SCORE_BAR_WIDTH_PERCENT),
                            height: Val::Px(SCORE_BAR_HEIGHT),
                            padding: UiRect::all(Val::Px(2.0)),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::SpaceEvenly,
                            margin: UiRect::bottom(Val::Px(2.0)),
                            ..default()
                        },
                        // Invisible background
                        BackgroundColor(Color::srgba(0.1, 0.1, 0.1, 0.0)),
                        ScoreBarUI,
                    ))
                    .with_children(|bar_parent| {
                        for i in row_start..row_end {
                            // Chain segment before each dot except the first in this row
                            if i > row_start {
                                bar_parent.spawn((
                                    Node {
                                        width: Val::Px(0.0),
                                        height: Val::Px(1.0),
                                        flex_grow: 1.0,
                                        ..default()
                                    },
                                    ScoreBarChain { index: i - 1 },
                                ));
                            }
                            // Dot
                            bar_parent.spawn((
                                Node {
                                    width: Val::Px(PROGRESS_BAR_DOTS_SIZE),
                                    height: Val::Px(PROGRESS_BAR_DOTS_SIZE),
                                    flex_shrink: 0.0,
                                    border_radius: BorderRadius::all(Val::Px(
                                        PROGRESS_BAR_DOTS_SIZE / 2.0,
                                    )),
                                    ..default()
                                },
                                ScoreBarDot { index: i },
                            ));
                        }
                    });
            }
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

