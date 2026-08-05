//! App state and registration.

use crate::frame::Mailbox;
use crate::geometry::Bounds;
use crate::settings::{Settings, Timeline};
use crate::systems;
use nightshade::prelude::*;

/// One mesh of the loaded model, and the material the reveal is driven on.
/// The order of these matches the order of the geometry published to the
/// render pass, which is how a draw finds its buffers.
pub struct MeshTarget {
    pub entity: Entity,
    pub material: String,
    pub local_bounds: Bounds,
}

pub struct Materialize {
    pub settings: Settings,
    pub timeline: Timeline,
    pub mailbox: Mailbox,
    pub meshes: Vec<MeshTarget>,
    pub root: Option<Entity>,
    pub camera: Option<Entity>,
    pub camera_framed: bool,
    pub elapsed_seconds: f32,
    pub ui: UiHandles,
}

/// Retained-UI entities the panel polls each frame.
#[derive(Default, Clone, Copy)]
pub struct UiHandles {
    pub reset: Entity,
    pub play: Entity,
    pub restart: Entity,
    pub scrub: Entity,
    pub progress_label: Entity,

    pub wire_thickness: Entity,
    pub wire_alpha: Entity,
    pub wire_glow: Entity,

    pub glass_alpha: Entity,
    pub glass_glow_strength: Entity,
    pub center_distance: Entity,
    pub normal_distance: Entity,
    pub jitter: Entity,
    pub fade_portion: Entity,
    pub cool_span: Entity,
    pub tumble: Entity,
    pub glass_band: Entity,

    pub seam_width: Entity,
    pub seam_strength: Entity,

    pub noise_amplitude: Entity,

    pub duration: Entity,
    pub lag_glass_to_solid: Entity,
    pub spin_speed: Entity,
}

impl Materialize {
    fn new(mailbox: Mailbox) -> Self {
        Self {
            settings: Settings::default(),
            timeline: Timeline::new(),
            mailbox,
            meshes: Vec::new(),
            root: None,
            camera: None,
            camera_framed: false,
            elapsed_seconds: 0.0,
            ui: UiHandles::default(),
        }
    }
}

pub struct MaterializePlugin;

impl Plugin for MaterializePlugin {
    fn build(&self, app: &mut App) {
        app.world.res_mut::<Window>().title = "Materialize".to_string();

        let mailbox = crate::frame::mailbox();
        let pass_mailbox = mailbox.clone();
        app.insert_resource(Materialize::new(mailbox));

        app.add_system(Stage::Startup, systems::setup::initialize);
        app.add_systems(
            Stage::Update,
            (
                systems::ui::poll,
                systems::effect::advance,
                systems::effect::publish,
            ),
        );
        app.add_render_graph_config(move |graph, device, surface_format, resources| {
            systems::setup::configure_render_graph(
                graph,
                device,
                surface_format,
                resources,
                pass_mailbox.clone(),
            );
        });
    }
}
