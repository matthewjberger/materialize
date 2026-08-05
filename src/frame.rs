//! The handoff between the app's systems and its render pass.
//!
//! The pass is built when the render graph is configured, before the model has
//! loaded, and it runs on the renderer's side of the frame. A mailbox is the
//! seam: the update systems publish the derived geometry once and the per-frame
//! uniforms every frame, and the pass drains whichever is waiting when it
//! prepares.

use crate::geometry::{ShardVertex, WireVertex};
use bytemuck::{Pod, Zeroable};
use std::sync::{Arc, Mutex};

/// Per-frame inputs for one mesh's wireframe ribbons.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
pub struct WireUniforms {
    pub view_projection: [[f32; 4]; 4],
    pub model: [[f32; 4]; 4],
    pub camera_position: [f32; 4],
    /// Ribbon color in rgb, opacity in w.
    pub color: [f32; 4],
    /// Boundary noise frequency in xyz, amplitude in w.
    pub noise: [f32; 4],
    pub wire_front: f32,
    pub solid_front: f32,
    pub band: f32,
    pub thickness: f32,
    pub glow_strength: f32,
    pub _padding: [f32; 3],
}

/// Per-frame inputs for one mesh's glass shards.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
pub struct GlassUniforms {
    pub view_projection: [[f32; 4]; 4],
    pub model: [[f32; 4]; 4],
    pub camera_position: [f32; 4],
    /// World-space center of the whole model, for the radial scatter.
    pub model_center: [f32; 4],
    /// Glass tint in rgb, base translucency in w.
    pub color: [f32; 4],
    /// Landing glow color in rgb, its strength in w.
    pub glow: [f32; 4],
    /// Shared scatter bias direction in xyz, its distance in w.
    pub fly: [f32; 4],
    /// Boundary noise frequency in xyz, amplitude in w.
    pub noise: [f32; 4],
    pub glass_front: f32,
    pub solid_front: f32,
    pub band: f32,
    pub time: f32,
    pub center_distance: f32,
    pub normal_distance: f32,
    pub jitter: f32,
    pub fade_portion: f32,
    pub cool_span: f32,
    pub tumble: f32,
    pub _padding: [f32; 2],
}

/// One mesh's derived geometry, waiting to be uploaded.
pub struct PendingGeometry {
    pub shards: Vec<ShardVertex>,
    pub wire_vertices: Vec<WireVertex>,
    pub wire_indices: Vec<u32>,
}

/// One mesh's uniforms for the current frame.
#[derive(Clone, Copy, Default)]
pub struct Draw {
    pub wire: WireUniforms,
    pub glass: GlassUniforms,
}

/// What the systems publish and the pass consumes.
#[derive(Default)]
pub struct Frame {
    /// Set once when the model's geometry has been derived. The pass takes it,
    /// builds its buffers, and leaves it empty.
    pub pending_geometry: Option<Vec<PendingGeometry>>,
    /// One entry per uploaded mesh, in the same order.
    pub draws: Vec<Draw>,
}

/// Shared handle to the frame mailbox.
pub type Mailbox = Arc<Mutex<Frame>>;

pub fn mailbox() -> Mailbox {
    Arc::new(Mutex::new(Frame::default()))
}
