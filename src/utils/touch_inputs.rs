//! Touch input handling for mobile/touchscreen support.
//! Implements swipe gestures for camera rotation and zoom, and tap for space action.

use bevy::prelude::*;

use crate::utils::constants::camera_3d_constants::{
    CAMERA_3D_INITIAL_Y, CAMERA_3D_MAX_RADIUS, CAMERA_3D_MIN_RADIUS, CAMERA_3D_SPEED_X,
    CAMERA_3D_SPEED_Z,
};
use crate::utils::objects::RotableComponent;

/// Resource to track touch state for gesture recognition
#[derive(Resource, Default)]
pub struct TouchState {
    /// Starting position of the current touch
    pub start_position: Option<Vec2>,
    /// Current/last position of the touch
    pub current_position: Option<Vec2>,
    /// Touch ID being tracked
    pub active_touch_id: Option<u64>,
    /// Time when touch started (for tap detection)
    pub touch_start_time: Option<f32>,
    /// Whether this is a potential tap (hasn't moved much)
    pub is_potential_tap: bool,
}

/// Constants for touch gesture detection
const TAP_MAX_DURATION_SECS: f32 = 0.3; // Maximum duration for a tap
const TAP_MAX_DISTANCE: f32 = 20.0; // Maximum movement for a tap (in pixels)
const SWIPE_SENSITIVITY_X: f32 = 0.005; // Horizontal swipe sensitivity for rotation
const SWIPE_SENSITIVITY_Y: f32 = 0.02; // Vertical swipe sensitivity for zoom

/// Plugin for touch input handling
pub struct TouchInputPlugin;

impl Plugin for TouchInputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TouchState>()
            .add_message::<TouchTapEvent>()
            .add_systems(Update, (track_touch_gestures, process_touch_swipe));
    }
}

/// Event fired when a tap is detected (using Message trait for Bevy 0.17)
#[derive(Message)]
pub struct TouchTapEvent;

/// System to track touch gestures and detect taps
pub fn track_touch_gestures(
    touches: Res<Touches>,
    time: Res<Time>,
    mut touch_state: ResMut<TouchState>,
    mut tap_events: MessageWriter<TouchTapEvent>,
) {
    // Handle new touch start
    for touch in touches.iter_just_pressed() {
        // Only track the first touch for single-finger gestures
        if touch_state.active_touch_id.is_none() {
            touch_state.active_touch_id = Some(touch.id());
            touch_state.start_position = Some(touch.position());
            touch_state.current_position = Some(touch.position());
            touch_state.touch_start_time = Some(time.elapsed_secs());
            touch_state.is_potential_tap = true;
        }
    }

    // Track touch movement
    for touch in touches.iter() {
        if Some(touch.id()) == touch_state.active_touch_id {
            let new_position = touch.position();
            touch_state.current_position = Some(new_position);

            // Check if moved too far to be a tap
            if let Some(start) = touch_state.start_position {
                let distance = (new_position - start).length();
                if distance > TAP_MAX_DISTANCE {
                    touch_state.is_potential_tap = false;
                }
            }
        }
    }

    // Handle touch release
    for touch in touches.iter_just_released() {
        if Some(touch.id()) == touch_state.active_touch_id {
            // Check if it was a tap
            if touch_state.is_potential_tap {
                if let Some(start_time) = touch_state.touch_start_time {
                    let duration = time.elapsed_secs() - start_time;
                    if duration <= TAP_MAX_DURATION_SECS {
                        // It's a tap! Send the message
                        tap_events.write(TouchTapEvent);
                    }
                }
            }

            // Reset touch state
            touch_state.active_touch_id = None;
            touch_state.start_position = None;
            touch_state.current_position = None;
            touch_state.touch_start_time = None;
            touch_state.is_potential_tap = true;
        }
    }

    // Handle cancelled touches
    for touch in touches.iter_just_canceled() {
        if Some(touch.id()) == touch_state.active_touch_id {
            // Reset touch state
            touch_state.active_touch_id = None;
            touch_state.start_position = None;
            touch_state.current_position = None;
            touch_state.touch_start_time = None;
            touch_state.is_potential_tap = true;
        }
    }
}

/// System to process touch swipes for camera rotation and zoom
pub fn process_touch_swipe(
    touches: Res<Touches>,
    touch_state: Res<TouchState>,
    timer: Res<Time>,
    mut camera_query: Query<&mut Transform, With<Camera3d>>,
    mut rot_entities: Query<&mut Transform, (With<RotableComponent>, Without<Camera3d>)>,
    gamestate: Res<crate::utils::objects::GameState>,
) {
    if gamestate.is_animating {
        return; // Do not allow camera inputs while animating
    }

    // Only process if we have an active touch that's not a tap
    if touch_state.active_touch_id.is_none() || touch_state.is_potential_tap {
        return;
    }

    // Get the delta movement from the touch
    for touch in touches.iter() {
        if Some(touch.id()) == touch_state.active_touch_id {
            let delta = touch.delta();

            // Skip if no significant movement
            if delta.length() < 0.1 {
                continue;
            }

            let delta_x = delta.x;
            let delta_y = delta.y;

            // Determine primary gesture direction based on cumulative movement
            if let (Some(start), Some(current)) = (touch_state.start_position, touch_state.current_position) {
                let total_delta = current - start;
                let abs_x = total_delta.x.abs();
                let abs_y = total_delta.y.abs();

                // Use hysteresis: once a direction is established, stick with it
                // Horizontal swipe -> rotation (left/right)
                if abs_x > abs_y {
                    // Rotate objects based on horizontal swipe
                    let rotation_speed = CAMERA_3D_SPEED_X * timer.delta_secs();
                    let rotation_amount = delta_x * SWIPE_SENSITIVITY_X * rotation_speed * 10.0;

                    for mut rot_entity_transform in &mut rot_entities {
                        let (mut yaw, _, _) = rot_entity_transform.rotation.to_euler(EulerRot::YXZ);
                        yaw += rotation_amount;
                        rot_entity_transform.rotation = Quat::from_rotation_y(yaw);
                    }
                }
                // Vertical swipe -> zoom (up/down)
                else {
                    let Ok(mut transform) = camera_query.single_mut() else {
                        return;
                    };

                    let zoom_speed = CAMERA_3D_SPEED_Z * timer.delta_secs();
                    let (yaw, _, _) = transform.rotation.to_euler(EulerRot::YXZ);
                    let mut radius = transform.translation.xz().length();

                    // Swipe up = zoom in (decrease radius), swipe down = zoom out (increase radius)
                    // Note: In screen coordinates, Y increases downward, so we invert
                    radius += delta_y * SWIPE_SENSITIVITY_Y * zoom_speed * 10.0;

                    // Clamp the camera's zoom level
                    radius = radius.clamp(CAMERA_3D_MIN_RADIUS, CAMERA_3D_MAX_RADIUS);

                    transform.translation = Vec3::new(
                        radius * yaw.sin(),
                        CAMERA_3D_INITIAL_Y,
                        radius * yaw.cos(),
                    );
                    transform.look_at(Vec3::ZERO, Vec3::Y);
                }
            }
        }
    }
}
