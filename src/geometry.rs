//! Derives the wireframe and glass-shard geometry from an imported mesh.
//!
//! Both derivations are pure functions of the source mesh, run once when the
//! model loads. The shard soup unwelds every triangle so each one carries a
//! flat outward normal plus its own centroid and seed, which is what lets the
//! glass vertex stage fly each shard in independently. The wireframe collects
//! the mesh's unique edges and expands each into a quad the vertex stage turns
//! to face the camera.

use bytemuck::{Pod, Zeroable};
use nalgebra_glm::Vec3;
use nightshade::render::geometry::Mesh;
use std::collections::HashSet;

/// One corner of an unwelded triangle. `centroid` and `seed` are constant
/// across the triangle's three vertices, so the vertex stage can treat them as
/// per-shard values.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct ShardVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub centroid: [f32; 3],
    pub seed: f32,
}

/// One corner of an edge ribbon. The vertex stage expands the quad about the
/// segment from `position` to `other`, pushing this corner `side` of the way
/// across the ribbon's width and `normal` off the surface so it clears the
/// shell it is drawn over.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct WireVertex {
    pub position: [f32; 3],
    pub other: [f32; 3],
    pub normal: [f32; 3],
    pub side: f32,
}

/// The world-space extent of a set of local-space points under a transform.
#[derive(Clone, Copy)]
pub struct Bounds {
    pub minimum: Vec3,
    pub maximum: Vec3,
}

impl Bounds {
    pub fn center(&self) -> Vec3 {
        (self.minimum + self.maximum) * 0.5
    }

    pub fn radius(&self) -> f32 {
        ((self.maximum - self.minimum) * 0.5).norm()
    }

    /// Extent along the reveal axis, which is what every sweep length in the
    /// effect is expressed as a fraction of.
    pub fn height(&self) -> f32 {
        (self.maximum.y - self.minimum.y).max(1.0e-3)
    }

    pub fn merge(&mut self, other: &Bounds) {
        self.minimum = nalgebra_glm::min2(&self.minimum, &other.minimum);
        self.maximum = nalgebra_glm::max2(&self.maximum, &other.maximum);
    }
}

/// Everything one source mesh contributes to the effect.
pub struct DerivedGeometry {
    pub shards: Vec<ShardVertex>,
    pub wire_vertices: Vec<WireVertex>,
    pub wire_indices: Vec<u32>,
    pub bounds: Bounds,
}

/// Unwelds `mesh` into a triangle soup, recomputing a flat outward normal per
/// triangle and stamping each corner with the triangle's centroid and a seed
/// derived from it. Seeding from the centroid rather than a counter keeps the
/// scatter stable if the mesh is ever reordered.
fn shard_soup(mesh: &Mesh) -> Vec<ShardVertex> {
    let mut shards = Vec::with_capacity(mesh.indices.len());
    for triangle in mesh.indices.chunks_exact(3) {
        let corners = [
            mesh.vertices[triangle[0] as usize],
            mesh.vertices[triangle[1] as usize],
            mesh.vertices[triangle[2] as usize],
        ];
        let positions = corners.map(|corner| Vec3::from(corner.position));
        let edge_a = positions[1] - positions[0];
        let edge_b = positions[2] - positions[0];
        let face_normal = nalgebra_glm::cross(&edge_a, &edge_b);
        let normal = if face_normal.norm() > 1.0e-12 {
            nalgebra_glm::normalize(&face_normal)
        } else {
            Vec3::from(corners[0].normal)
        };
        let centroid = (positions[0] + positions[1] + positions[2]) / 3.0;
        let seed = fract(
            (centroid.x * 127.1 + centroid.y * 311.7 + centroid.z * 74.7)
                .sin()
                .abs()
                * 43758.547,
        );

        for position in positions {
            shards.push(ShardVertex {
                position: [position.x, position.y, position.z],
                normal: [normal.x, normal.y, normal.z],
                centroid: [centroid.x, centroid.y, centroid.z],
                seed,
            });
        }
    }
    shards
}

fn fract(value: f32) -> f32 {
    value - value.floor()
}

/// Collects the mesh's unique edges and expands each into a two-triangle
/// ribbon. The vertex positions are the same for both corners of an end; the
/// `side` sign is what pulls them apart at draw time, so the ribbon can face
/// the camera without rebuilding the buffer.
fn edge_ribbons(mesh: &Mesh) -> (Vec<WireVertex>, Vec<u32>) {
    let mut seen: HashSet<(u32, u32)> = HashSet::new();
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    for triangle in mesh.indices.chunks_exact(3) {
        for pair in [
            (triangle[0], triangle[1]),
            (triangle[1], triangle[2]),
            (triangle[2], triangle[0]),
        ] {
            let key = if pair.0 <= pair.1 {
                (pair.0, pair.1)
            } else {
                (pair.1, pair.0)
            };
            if !seen.insert(key) {
                continue;
            }

            let start = mesh.vertices[key.0 as usize];
            let end = mesh.vertices[key.1 as usize];
            let base = vertices.len() as u32;

            // The vertex stage takes its across-the-ribbon axis from
            // `other - position`, which points opposite ways at the two ends,
            // so the far end's sides are flipped to cancel that and keep the
            // quad from crossing itself.
            for (vertex, other, orientation) in [(start, end, 1.0), (end, start, -1.0)] {
                for side in [-1.0, 1.0] {
                    vertices.push(WireVertex {
                        position: vertex.position,
                        other: other.position,
                        normal: vertex.normal,
                        side: side * orientation,
                    });
                }
            }

            indices.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
        }
    }

    (vertices, indices)
}

fn local_bounds(mesh: &Mesh) -> Bounds {
    let mut minimum = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
    let mut maximum = Vec3::new(f32::MIN, f32::MIN, f32::MIN);
    for vertex in &mesh.vertices {
        let position = Vec3::from(vertex.position);
        minimum = nalgebra_glm::min2(&minimum, &position);
        maximum = nalgebra_glm::max2(&maximum, &position);
    }
    Bounds { minimum, maximum }
}

/// Derives every pass's geometry for one source mesh.
pub fn derive(mesh: &Mesh) -> DerivedGeometry {
    let (wire_vertices, wire_indices) = edge_ribbons(mesh);
    DerivedGeometry {
        shards: shard_soup(mesh),
        wire_vertices,
        wire_indices,
        bounds: local_bounds(mesh),
    }
}
