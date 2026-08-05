//! Drives the three stages from one progress value.
//!
//! World bounds are recomputed every frame from each mesh's current transform.
//! The model only spins about the world up axis, so its extent along the reveal
//! axis is stable under the spin, and the center follows the model round the way
//! the radial scatter expects.

use crate::frame::{Draw, GlassUniforms, WireUniforms};
use crate::geometry::Bounds;
use crate::plugin::Materialize;
use nalgebra_glm::{Mat4, Vec3, Vec4};
use nightshade::ecs::material::resources::{MaterialRegistry, material_registry_mutate};
use nightshade::prelude::*;

/// Advances playback, spins the model, and writes the reveal onto the model's
/// own materials so the engine cuts the surface and draws the seam.
pub fn advance(materialize: &mut Materialize, world: &mut World) {
    let delta_seconds = world.res::<Time>().delta_time;
    materialize.elapsed_seconds += delta_seconds;

    let duration = materialize.settings.duration;
    materialize.timeline.advance(delta_seconds, duration);

    if let Some(root) = materialize.root {
        let angle = materialize.elapsed_seconds * materialize.settings.spin_speed;
        if let Some(transform) = world.get_mut::<LocalTransform>(root) {
            transform.rotation = nalgebra_glm::quat_angle_axis(angle, &Vec3::y());
        }
    }

    let Some(bounds) = world_bounds(materialize, world) else {
        return;
    };
    frame_camera(materialize, world, &bounds);

    let height = bounds.height();
    let fronts =
        materialize
            .settings
            .fronts(materialize.timeline.progress(), bounds.minimum.y, height);
    let settings = &materialize.settings;
    let noise_scale = settings.noise_scale / height;
    let noise_amplitude = settings.noise_amplitude * height;

    for target in &materialize.meshes {
        material_registry_mutate(
            world.res_mut::<MaterialRegistry>(),
            &target.material,
            |material| {
                material.reveal_normal = [0.0, 1.0, 0.0];
                material.reveal_front = fronts.solid;
                material.reveal_noise_scale = [noise_scale.x, noise_scale.y, noise_scale.z];
                material.reveal_noise_amplitude = noise_amplitude;
                material.reveal_seam_width = settings.seam_width * height;
                material.reveal_seam_color = [
                    settings.seam_color.x,
                    settings.seam_color.y,
                    settings.seam_color.z,
                ];
                material.reveal_seam_strength = settings.seam_strength;
            },
        );
    }
}

/// Publishes this frame's uniforms for the wireframe and glass stages.
pub fn publish(materialize: &mut Materialize, world: &mut World) {
    if materialize.meshes.is_empty() {
        return;
    }
    let Some(bounds) = world_bounds(materialize, world) else {
        return;
    };
    let height = bounds.height();
    let center = bounds.center();
    let fronts =
        materialize
            .settings
            .fronts(materialize.timeline.progress(), bounds.minimum.y, height);
    let settings = &materialize.settings;
    let noise_scale = settings.noise_scale / height;
    let noise = [
        noise_scale.x,
        noise_scale.y,
        noise_scale.z,
        settings.noise_amplitude * height,
    ];

    let draws: Vec<Draw> = materialize
        .meshes
        .iter()
        .map(|target| {
            let model = world
                .get::<GlobalTransform>(target.entity)
                .map(|transform| transform.0)
                .unwrap_or_else(Mat4::identity);
            let model = matrix_to_array(&model);
            Draw {
                wire: WireUniforms {
                    model,
                    color: [
                        settings.wire_color.x,
                        settings.wire_color.y,
                        settings.wire_color.z,
                        settings.wire_alpha,
                    ],
                    noise,
                    wire_front: fronts.wire,
                    solid_front: fronts.solid,
                    band: height * 0.06,
                    thickness: height * settings.wire_thickness,
                    glow_strength: settings.wire_glow,
                    _padding: [0.0; 3],
                    ..Default::default()
                },
                glass: GlassUniforms {
                    model,
                    model_center: [center.x, center.y, center.z, 1.0],
                    color: [
                        settings.glass_tint.x,
                        settings.glass_tint.y,
                        settings.glass_tint.z,
                        settings.glass_alpha,
                    ],
                    glow: [
                        settings.glass_glow_color.x,
                        settings.glass_glow_color.y,
                        settings.glass_glow_color.z,
                        settings.glass_glow_strength,
                    ],
                    fly: [
                        settings.fly_direction.x,
                        settings.fly_direction.y,
                        settings.fly_direction.z,
                        height * settings.fly_distance,
                    ],
                    noise,
                    glass_front: fronts.glass,
                    solid_front: fronts.solid,
                    band: height * settings.glass_band,
                    time: materialize.elapsed_seconds,
                    center_distance: height * settings.center_distance,
                    normal_distance: height * settings.normal_distance,
                    jitter: height * settings.jitter,
                    fade_portion: settings.fade_portion,
                    cool_span: settings.cool_span,
                    tumble: settings.tumble,
                    _padding: [0.0; 2],
                    ..Default::default()
                },
            }
        })
        .collect();

    if let Ok(mut frame) = materialize.mailbox.lock() {
        frame.draws = draws;
    }
}

/// The union of every mesh's local bounds under its current world transform.
fn world_bounds(materialize: &Materialize, world: &World) -> Option<Bounds> {
    let mut result: Option<Bounds> = None;
    for target in &materialize.meshes {
        let transform = world
            .get::<GlobalTransform>(target.entity)
            .map(|transform| transform.0)?;
        let local = target.local_bounds;
        let mut bounds: Option<Bounds> = None;
        for index in 0..8 {
            let corner = Vec3::new(
                if index & 1 == 0 {
                    local.minimum.x
                } else {
                    local.maximum.x
                },
                if index & 2 == 0 {
                    local.minimum.y
                } else {
                    local.maximum.y
                },
                if index & 4 == 0 {
                    local.minimum.z
                } else {
                    local.maximum.z
                },
            );
            let world_corner = transform * Vec4::new(corner.x, corner.y, corner.z, 1.0);
            let point = Vec3::new(world_corner.x, world_corner.y, world_corner.z);
            match &mut bounds {
                Some(bounds) => {
                    bounds.minimum = nalgebra_glm::min2(&bounds.minimum, &point);
                    bounds.maximum = nalgebra_glm::max2(&bounds.maximum, &point);
                }
                None => {
                    bounds = Some(Bounds {
                        minimum: point,
                        maximum: point,
                    })
                }
            }
        }
        let bounds = bounds?;
        match &mut result {
            Some(result) => result.merge(&bounds),
            None => result = Some(bounds),
        }
    }
    result
}

/// Pulls the orbit camera in around the model once its world bounds are known.
fn frame_camera(materialize: &mut Materialize, world: &mut World, bounds: &Bounds) {
    if materialize.camera_framed {
        return;
    }
    let Some(camera) = materialize.camera else {
        return;
    };
    let center = bounds.center();
    let radius = bounds.radius().max(1.0e-3) * 2.6;
    if let Some(orbit) = world.get_mut::<PanOrbitCamera>(camera) {
        orbit.focus = center;
        orbit.target_focus = center;
        orbit.radius = radius;
        orbit.target_radius = radius;
        orbit.pan_distance = Some(radius);
    }
    materialize.camera_framed = true;
}

fn matrix_to_array(matrix: &Mat4) -> [[f32; 4]; 4] {
    let mut result = [[0.0; 4]; 4];
    for column in 0..4 {
        for row in 0..4 {
            result[column][row] = matrix[(row, column)];
        }
    }
    result
}
