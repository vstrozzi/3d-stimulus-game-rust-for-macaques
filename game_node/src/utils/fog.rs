//! Mystical distance fog centered on the pyramid, plus gold firefly particles
//! shown during a winning animation.
//!
//! The fog lives on the persistent 3D camera as a [`DistanceFog`]. Because the
//! camera always looks at and orbits the pyramid (origin), driving the fog's
//! `start` distance off the camera's distance-to-origin keeps a clear bubble of
//! radius `FOG_START_RADIUS` locked around the pyramid while the surroundings
//! dissolve into haze — so it reads as fog centered on the pyramid.
//!
//! Fireflies spawn the moment a correct (green) animation begins and drift
//! until the black screen appears, then despawn.
//!
//! A second, always-on swarm of ambient motes drifts in a ring in front of the
//! back wall. Its size follows the player's consecutive-correct streak, so the
//! scene gets visibly more magical the longer the run within a level.
use std::f32::consts::TAU;

use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;
use rand::Rng;

use crate::utils::objects::{AmbientMote, DoorWinEntities, Firefly, GameStateLocal, PersistentCamera};
use shared::constants::ambient_particle_constants::*;
use shared::constants::fog_constants::*;
use shared::constants::pyramid_constants::{LIGHT_GREEN, PYRAMID_HEIGHT};

/// Outer wall radius (see `setup_environment`). Nothing renders past it, so
/// fireflies are clamped to stay inside.
const WALL_RADIUS: f32 = 9.0;

/// Tracks whether a firefly swarm is currently alive and when it spawned (so
/// the burst-from-center expansion can be timed).
#[derive(Resource, Default)]
pub struct FireflyState {
    active: bool,
    spawn_secs: f32,
    /// World position the swarm bursts out from (the winning hole).
    origin: Vec3,
    /// Intermediate waypoint 1 unit toward the camera from `origin`.
    waypoint: Vec3,
}

/// Fallback burst point if the winning hole transform isn't available — the
/// pyramid's centroid (height/4).
const FIREFLY_ORIGIN: Vec3 = Vec3::new(0.0, PYRAMID_HEIGHT * 0.25, 0.0);

/// Attach the distance fog to the persistent camera once at startup. The fog is
/// always attached; per-level `fog_enabled` is honoured each frame in
/// `update_fog` (pushing the onset past the wall effectively disables it).
pub fn setup_fog(mut commands: Commands, camera: Query<Entity, With<PersistentCamera>>) {
    if let Ok(entity) = camera.single() {
        commands.entity(entity).insert(DistanceFog {
            color: FOG_COLOR,
            // Real values are written every frame by `update_fog`.
            falloff: FogFalloff::Linear { start: 0.0, end: 1.0 },
            ..default()
        });
    }
}

/// Keep the clear bubble centered on the pyramid by deriving the fog onset from
/// the camera's current distance to the origin. `fog_enabled` /
/// `fog_thickness_base` are per-level config read from shared memory.
pub fn update_fog(
    local_game_struct: Res<GameStateLocal>,
    mut camera: Query<(&Transform, &mut DistanceFog), With<PersistentCamera>>,
) {
    let Ok((transform, mut fog)) = camera.single_mut() else {
        return;
    };
    let gs = &local_game_struct.0;
    if !gs.fog_enabled {
        // Push the fog onset far past the outer wall so nothing is fogged.
        fog.falloff = FogFalloff::Linear { start: 1.0e9, end: 1.0e9 + 1.0 };
        return;
    }
    let thickness_base = f32::from_bits(gs.fog_thickness_base);
    let dist_to_center = transform.translation.length();
    let start = dist_to_center + FOG_START_RADIUS;
    let end = start + thickness_base / FOG_DENSITY.max(0.0001);
    fog.falloff = FogFalloff::Linear { start, end };
}

/// Spawn fireflies on win start, animate their drift/twinkle, and despawn them
/// when the black screen appears.
pub fn update_fireflies(
    mut commands: Commands,
    time: Res<Time>,
    door_win: Res<DoorWinEntities>,
    local_game_struct: Res<GameStateLocal>,
    mut state: ResMut<FireflyState>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    transforms: Query<&GlobalTransform>,
    camera: Query<&GlobalTransform, With<PersistentCamera>>,
    mut fireflies: Query<(Entity, &Firefly, &mut Transform)>,
) {
    let gs = &local_game_struct.0;
    let win_active = gs.is_animating && door_win.color == LIGHT_GREEN && !door_win.animate_all;
    let firefly_count = gs.firefly_count;
    let firefly_size = f32::from_bits(gs.firefly_size);
    let expand_secs = f32::from_bits(gs.firefly_expand_secs);

    // Spawn once, at the start of a correct animation.
    if win_active && !state.active && firefly_count > 0 {
        // Burst out of the winning hole's current world position.
        state.origin = door_win
            .winning_light
            .and_then(|e| transforms.get(e).ok())
            .map(|t| t.translation())
            .unwrap_or(FIREFLY_ORIGIN);
        // Waypoint one unit from the hole toward the player camera.
        let toward_cam = camera
            .single()
            .map(|t| (t.translation() - state.origin).normalize_or_zero())
            .unwrap_or(Vec3::Y);
        state.waypoint = state.origin + toward_cam * FIREFLY_BURST_TOWARD_CAMERA;
        spawn_fireflies(&mut commands, &mut meshes, &mut materials, state.origin, firefly_count, firefly_size);
        state.active = true;
        state.spawn_secs = time.elapsed_secs();
    }

    // Persist through the whole animation; clear out when the screen blanks.
    if state.active && gs.is_blank {
        for (entity, _, _) in &fireflies {
            commands.entity(entity).despawn();
        }
        state.active = false;
        return;
    }

    if !state.active {
        return;
    }

    let t = time.elapsed_secs();
    let p = ((t - state.spawn_secs) / expand_secs.max(0.0001)).clamp(0.0, 1.0);
    // Scale grows in over the whole burst (ease-out).
    let grow = 1.0 - (1.0 - p).powi(3);
    let ease_out = |x: f32| 1.0 - (1.0 - x).powi(3);
    let phase1 = FIREFLY_BURST_PHASE1.clamp(0.0001, 0.9999);
    for (_, fly, mut transform) in &mut fireflies {
        let resting = fly.base
            + Vec3::new(
                fly.amp.x * (fly.freq.x * t + fly.phase.x).sin(),
                fly.amp.y * (fly.freq.y * t + fly.phase.y).sin(),
                fly.amp.z * (fly.freq.z * t + fly.phase.z).sin(),
            );
        // Phase 1: hole -> toward-camera waypoint. Phase 2: waypoint -> resting.
        transform.translation = if p < phase1 {
            state.origin.lerp(state.waypoint, ease_out(p / phase1))
        } else {
            state
                .waypoint
                .lerp(resting, ease_out((p - phase1) / (1.0 - phase1)))
        };
        // Fire-like flicker (two frequencies), grown in over the expansion.
        let flicker = 1.0
            + 0.35 * (7.0 * t + fly.flicker_phase).sin()
            + 0.15 * (19.0 * t + fly.flicker_phase * 1.7).sin();
        transform.scale = Vec3::splat((flicker * grow).max(0.0));
    }
}

fn spawn_fireflies(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    origin: Vec3,
    firefly_count: u32,
    firefly_size: f32,
) {
    // Low-poly, like the ambient motes: at this size a full-resolution UV
    // sphere is invisible detail on a swarm that can be hundreds strong.
    let mesh = meshes.add(Sphere::new(firefly_size).mesh().uv(6, 4));
    let glow = FIREFLY_COLOR.to_linear();
    let material = materials.add(StandardMaterial {
        base_color: FIREFLY_COLOR,
        emissive: LinearRgba::new(
            glow.red * FIREFLY_GLOW,
            glow.green * FIREFLY_GLOW,
            glow.blue * FIREFLY_GLOW,
            1.0,
        ),
        unlit: true,
        ..default()
    });

    // Clamp the band so no firefly spawns at or beyond the wall.
    let max_radius = (WALL_RADIUS - firefly_size).min(FIREFLY_RADIUS + FIREFLY_SPREAD);
    let min_radius = (FIREFLY_RADIUS - FIREFLY_SPREAD).max(0.0);

    let mut rng = rand::rng();
    for _ in 0..firefly_count {
        let angle = rng.random_range(0.0..TAU);
        let radius = rng.random_range(min_radius..=max_radius);
        let base = Vec3::new(
            radius * angle.cos(),
            rng.random_range(0.5..(PYRAMID_HEIGHT + 0.5)),
            radius * angle.sin(),
        );
        let firefly = Firefly {
            base,
            amp: Vec3::new(
                rng.random_range(0.3..0.9),
                rng.random_range(0.3..0.9),
                rng.random_range(0.3..0.9),
            ),
            freq: Vec3::new(
                rng.random_range(0.2..0.8) * FIREFLY_SPEED,
                rng.random_range(0.2..0.8) * FIREFLY_SPEED,
                rng.random_range(0.2..0.8) * FIREFLY_SPEED,
            ),
            phase: Vec3::new(
                rng.random_range(0.0..TAU),
                rng.random_range(0.0..TAU),
                rng.random_range(0.0..TAU),
            ),
            flicker_phase: rng.random_range(0.0..TAU),
        };
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            // Start at the winning hole, invisible: the first frame they render
            // is already the start of the burst, so no outside-position flash.
            Transform::from_translation(origin).with_scale(Vec3::ZERO),
            // Glowing dots: keep them out of the spotlight's shadow pass.
            NotShadowCaster,
            NotShadowReceiver,
            firefly,
        ));
    }
}



/// Number of motes to show for a given consecutive-correct streak. A streak of
/// 0 always means none; from there the count steps linearly from
/// `AMBIENT_COUNT_MIN` up to `AMBIENT_COUNT_MAX` over `AMBIENT_STEPS` wins.
fn ambient_mote_count(streak: u32) -> u32 {
    if streak == 0 {
        return 0;
    }
    let steps = AMBIENT_STEPS.max(1);
    let step = streak.min(steps);
    let frac = (step - 1) as f32 / (steps - 1).max(1) as f32;
    let count = AMBIENT_COUNT_MIN as f32 + (AMBIENT_COUNT_MAX as f32 - AMBIENT_COUNT_MIN as f32) * frac;
    count.round().max(0.0) as u32
}

/// Spawn the full mote pool once at startup. They are not `GameEntity`, so
/// trial resets leave them alone, and density changes cost nothing at runtime.
pub fn setup_ambient_motes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if AMBIENT_COUNT_MAX == 0 {
        return;
    }
    // Low-poly: the motes are sub-centimeter glowing dots, a full-resolution
    // UV sphere each is pure vertex cost.
    let mesh = meshes.add(Sphere::new(AMBIENT_SIZE).mesh().uv(6, 4));
    let glow = AMBIENT_COLOR.to_linear();
    let material = materials.add(StandardMaterial {
        base_color: AMBIENT_COLOR,
        emissive: LinearRgba::new(
            glow.red * AMBIENT_GLOW,
            glow.green * AMBIENT_GLOW,
            glow.blue * AMBIENT_GLOW,
            1.0,
        ),
        unlit: true,
        ..default()
    });

    // Band in front of the wall, clamped so nothing pokes through it.
    let outer = (WALL_RADIUS - AMBIENT_WALL_GAP).clamp(0.0, WALL_RADIUS - AMBIENT_SIZE);
    let inner = AMBIENT_INNER_RADIUS.clamp(0.0, outer);

    // Arc centered on the middle of the wall, i.e. straight away from the
    // camera along -Z, which is `(cos, sin) = (0, -1)`.
    let arc = AMBIENT_ARC_DEG.to_radians().clamp(0.0, TAU);
    let arc_center = -std::f32::consts::FRAC_PI_2;

    let mut rng = rand::rng();
    for index in 0..AMBIENT_COUNT_MAX {
        let angle = arc_center + rng.random_range(-arc / 2.0..=arc / 2.0);
        let radius = if inner < outer { rng.random_range(inner..=outer) } else { outer };
        let base = Vec3::new(
            radius * angle.cos(),
            rng.random_range(AMBIENT_Y_MIN..=AMBIENT_Y_MAX.max(AMBIENT_Y_MIN)),
            radius * angle.sin(),
        );
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            // Hidden until `update_ambient_motes` decides otherwise.
            Transform::from_translation(base).with_scale(Vec3::ZERO),
            // Glowing dots: keep them out of the spotlight's shadow pass.
            NotShadowCaster,
            NotShadowReceiver,
            AmbientMote {
                index,
                base,
                amp: Vec3::new(
                    rng.random_range(0.3..0.9),
                    rng.random_range(0.3..0.9),
                    rng.random_range(0.3..0.9),
                ),
                freq: Vec3::new(
                    rng.random_range(0.2..0.8) * AMBIENT_SPEED,
                    rng.random_range(0.2..0.8) * AMBIENT_SPEED,
                    rng.random_range(0.2..0.8) * AMBIENT_SPEED,
                ),
                phase: Vec3::new(
                    rng.random_range(0.0..TAU),
                    rng.random_range(0.0..TAU),
                    rng.random_range(0.0..TAU),
                ),
                flicker_phase: rng.random_range(0.0..TAU),
            },
        ));
    }
}

/// Drift and twinkle the motes the current streak calls for, and keep the rest
/// scaled to zero. Hidden entirely while the black screen is up.
pub fn update_ambient_motes(
    time: Res<Time>,
    local_game_struct: Res<GameStateLocal>,
    mut motes: Query<(&AmbientMote, &mut Transform)>,
) {
    let gs = &local_game_struct.0;
    let count = if gs.is_blank { 0 } else { ambient_mote_count(gs.correct_streak) };
    let t = time.elapsed_secs();
    for (mote, mut transform) in &mut motes {
        if mote.index >= count {
            transform.scale = Vec3::ZERO;
            continue;
        }
        transform.translation = mote.base
            + Vec3::new(
                mote.amp.x * (mote.freq.x * t + mote.phase.x).sin(),
                mote.amp.y * (mote.freq.y * t + mote.phase.y).sin(),
                mote.amp.z * (mote.freq.z * t + mote.phase.z).sin(),
            );
        let flicker = 1.0
            + 0.35 * (7.0 * t + mote.flicker_phase).sin()
            + 0.15 * (19.0 * t + mote.flicker_phase * 1.7).sin();
        transform.scale = Vec3::splat(flicker.max(0.0));
    }
}
