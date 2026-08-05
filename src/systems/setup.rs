//! Loading the model, deriving the effect's geometry, and composing the graph.

use crate::frame::{Mailbox, PendingGeometry};
use crate::geometry;
use crate::pass::MaterializePass;
use crate::plugin::{Materialize, MeshTarget};
use nightshade::ecs::camera::commands::spawn_pan_orbit_camera_at;
use nightshade::ecs::material::components::MaterialRef;
use nightshade::ecs::mesh::components::RenderMesh;
use nightshade::prelude::*;

const HELMET_GLTF: &[u8] = include_bytes!("../../assets/DamagedHelmet.glb");
const HDR_SKYBOX: &[u8] = include_bytes!("../../assets/moonrise.hdr");

pub fn initialize(materialize: &mut Materialize, world: &mut World) {
    world.res_mut::<DebugDraw>().show_grid = false;
    let settings = world.res_mut::<RenderSettings>();
    settings.atmosphere = Atmosphere::Hdr;
    // The environment lights the model but is never drawn: against a near
    // black background the wireframe, the shards and the seam read as light
    // rather than competing with a sky.
    settings.show_sky = false;
    settings.clear_color = [0.004, 0.005, 0.008, 1.0];
    settings.bloom_enabled = true;
    settings.bloom_intensity = 0.08;
    // The shards move fast and the app pass writes no motion vectors, so
    // reprojection would smear them across the frame it reuses.
    settings.taa_enabled = false;
    load_hdr_skybox(world, HDR_SKYBOX.to_vec());

    load_model(materialize, world);

    let camera =
        spawn_pan_orbit_camera_at(world, Vec3::zeros(), 3.0, 0.6, 0.35, "Camera".to_string());
    world.res_mut::<ActiveCamera>().0 = Some(camera);
    materialize.camera = Some(camera);

    crate::systems::ui::build(materialize, world);
}

/// Imports the model, derives the wireframe and shard geometry from the mesh
/// data the import already carries, then hands the meshes to the engine and
/// spawns the prefabs. Deriving before the handoff keeps the geometry read on
/// the loading path instead of waiting for it to surface in the mesh cache.
fn load_model(materialize: &mut Materialize, world: &mut World) {
    let mut import = match nightshade::assets::prefab::import_gltf_from_bytes(HELMET_GLTF) {
        Ok(import) => import,
        Err(error) => {
            tracing::error!("failed to import the model: {error}");
            return;
        }
    };

    let mut mesh_names: Vec<String> = import.meshes.keys().cloned().collect();
    mesh_names.sort();
    let derived: Vec<geometry::DerivedGeometry> = mesh_names
        .iter()
        .map(|name| geometry::derive(&import.meshes[name]))
        .collect();

    nightshade::assets::prefab::queue_gltf_load(world, &mut import);
    let roots: Vec<Entity> = import
        .prefabs
        .iter()
        .map(|prefab| nightshade::assets::prefab::spawn_prefab(world, prefab, Vec3::zeros()))
        .collect();
    materialize.root = roots.first().copied();

    let spawned: Vec<(Entity, String, String)> = world
        .query_ref::<(&RenderMesh, &MaterialRef)>()
        .iter()
        .map(|(entity, (mesh, material))| (entity, mesh.name.clone(), material.name.clone()))
        .collect();

    let mut pending = Vec::new();
    for (name, geometry) in mesh_names.iter().zip(derived) {
        let Some((entity, _, material)) = spawned
            .iter()
            .find(|(_, mesh_name, _)| mesh_name == name)
            .cloned()
        else {
            continue;
        };
        materialize.meshes.push(MeshTarget {
            entity,
            material,
            local_bounds: geometry.bounds,
        });
        pending.push(PendingGeometry {
            shards: geometry.shards,
            wire_vertices: geometry.wire_vertices,
            wire_indices: geometry.wire_indices,
        });
    }

    if let Ok(mut frame) = materialize.mailbox.lock() {
        frame.draws = vec![Default::default(); pending.len()];
        frame.pending_geometry = Some(pending);
    }
}

pub fn configure_render_graph(
    graph: &mut RenderGraph<nightshade::prelude::RenderInputs>,
    device: &wgpu::Device,
    surface_format: wgpu::TextureFormat,
    resources: RenderResources,
    mailbox: Mailbox,
) {
    let materialize_pass = MaterializePass::new(device, mailbox);
    render_graph_pass(graph, Box::new(materialize_pass))
        .slot("color", resources.scene_color)
        .slot("depth", resources.depth)
        .add()
        .unwrap();

    let (width, height) = (1920, 1080);
    let bloom_texture = render_graph_add_color_texture(graph, "bloom")
        .format(wgpu::TextureFormat::Rgba16Float)
        .size(width / 2, height / 2)
        .clear_color(wgpu::Color::BLACK)
        .transient();

    let bloom_pass = passes::BloomPass::new(device, width, height);
    render_graph_pass(graph, Box::new(bloom_pass))
        .slot("hdr", resources.scene_color)
        .slot("bloom", bloom_texture)
        .add()
        .unwrap();

    let postprocess_pass = passes::PostProcessPass::new(device, surface_format, 0.08);
    render_graph_pass(graph, Box::new(postprocess_pass))
        .slot("hdr", resources.scene_color)
        .slot("scene_color", resources.scene_color)
        .slot("bloom", bloom_texture)
        .slot("ssao", resources.ssao)
        .slot("output", resources.compute_output)
        .add()
        .unwrap();

    let aa_output = render_graph_add_color_texture(graph, "aa_output")
        .format(surface_format)
        .size(
            resources.surface_width.max(1),
            resources.surface_height.max(1),
        )
        .transient();

    let taa_pass = passes::TaaPass::new(device, surface_format);
    render_graph_pass(graph, Box::new(taa_pass))
        .slot("input", resources.compute_output)
        .slot("depth", resources.depth)
        .slot("output", aa_output)
        .add()
        .unwrap();

    let swapchain_blit_pass =
        passes::BlitPass::new(device, surface_format).with_name("default_swapchain_blit");
    render_graph_pass(graph, Box::new(swapchain_blit_pass))
        .slot("input", aa_output)
        .slot("output", resources.swapchain)
        .add()
        .unwrap();
}
