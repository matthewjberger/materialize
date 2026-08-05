//! A model phasing into existence in three stages over the same geometry.
//!
//! A wireframe forms from the bottom up, glass shards fly in and assemble over
//! it, and the real surface reveals itself behind a glowing seam. One eased
//! progress value drives three staggered sweep fronts, and every stage compares
//! against the same noise-wobbled boundary so their edges line up.
//!
//! The reveal is the engine's own material feature. The wireframe and the glass
//! are an app-owned render pass, because both displace their geometry in the
//! vertex stage.

mod frame;
mod geometry;
mod pass;
mod plugin;
mod settings;
mod systems;

use nightshade::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugin(CameraControllerPlugin)
        .add_plugin(ExitOnEscapePlugin)
        .add_plugin(plugin::MaterializePlugin)
        .run()
}
