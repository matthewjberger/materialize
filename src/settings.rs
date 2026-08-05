//! Every tunable of the effect, and the playback state that drives it.
//!
//! Lengths are fractions of the model's height along the reveal axis, so the
//! same numbers read the same way on a model of any size. They are scaled into
//! world units once, where they are written into the materials and the pass
//! uniforms.

use nalgebra_glm::Vec3;

/// Look and timing inputs. One eased progress value drives three staggered
/// sweep fronts, and every stage compares against the same noise-wobbled
/// boundary so their edges line up.
pub struct Settings {
    pub wire_thickness: f32,
    pub wire_color: Vec3,
    pub wire_alpha: f32,
    pub wire_glow: f32,

    pub glass_tint: Vec3,
    pub glass_alpha: f32,
    pub glass_glow_color: Vec3,
    pub glass_glow_strength: f32,
    pub fly_direction: Vec3,
    pub fly_distance: f32,
    pub center_distance: f32,
    pub normal_distance: f32,
    pub jitter: f32,
    pub fade_portion: f32,
    pub cool_span: f32,
    pub tumble: f32,
    pub glass_band: f32,

    pub seam_width: f32,
    pub seam_color: Vec3,
    pub seam_strength: f32,

    pub noise_scale: Vec3,
    pub noise_amplitude: f32,

    pub duration: f32,
    pub lag_wire_to_glass: f32,
    pub lag_glass_to_solid: f32,

    pub spin_speed: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            wire_thickness: 0.002,
            wire_color: Vec3::new(0.16, 0.64, 1.0),
            wire_alpha: 0.85,
            wire_glow: 27.5,

            glass_tint: Vec3::new(0.5, 0.85, 1.0),
            glass_alpha: 0.2,
            glass_glow_color: Vec3::new(1.0, 1.0, 0.6),
            glass_glow_strength: 1.9,
            fly_direction: Vec3::new(0.0, 1.0, 0.0),
            fly_distance: 0.0,
            center_distance: 0.45,
            normal_distance: 1.5,
            jitter: 0.32,
            fade_portion: 0.64,
            cool_span: 0.62,
            tumble: 2.3,
            glass_band: 0.84,

            seam_width: 0.12,
            seam_color: Vec3::new(0.3, 0.9, 1.0),
            seam_strength: 40.0,

            noise_scale: Vec3::new(3.4, 0.5, 3.9),
            noise_amplitude: 0.11,

            duration: 7.0,
            lag_wire_to_glass: 0.0,
            lag_glass_to_solid: 1.5,

            spin_speed: 0.25,
        }
    }
}

/// Sweep overshoot past both ends of the model, so every fade band fully
/// clears before the cycle restarts.
const SWEEP_PADDING: f32 = 0.15;

/// The three sweep fronts for one instant, as world-space distances along the
/// reveal axis.
#[derive(Clone, Copy, Default)]
pub struct Fronts {
    pub wire: f32,
    pub glass: f32,
    pub solid: f32,
}

impl Settings {
    /// Maps eased progress onto the three staggered fronts. The wire front
    /// leads, the glass front trails it, and the solid front trails that, each
    /// by its own lag expressed in model heights.
    pub fn fronts(&self, progress: f32, minimum_height: f32, height: f32) -> Fronts {
        let eased = progress * progress * (3.0 - 2.0 * progress);
        let padding = SWEEP_PADDING + self.noise_amplitude;
        let start = -padding;
        let end = 1.0 + self.lag_wire_to_glass + self.lag_glass_to_solid + padding;
        let wire = start + eased * (end - start);

        let front = |normalized: f32| minimum_height + normalized * height;
        Fronts {
            wire: front(wire),
            glass: front(wire - self.lag_wire_to_glass),
            solid: front(wire - self.lag_wire_to_glass - self.lag_glass_to_solid),
        }
    }
}

/// Playback position of the effect, as a phase that runs the sweep out and
/// back. Progress rises from zero to one over the first half of the phase and
/// falls back over the second, so the model materializes and then dematerializes
/// through the same three stages in reverse. Every stage is a pure function of
/// progress, so the way back needs no separate path, and the loop closes on the
/// hidden model instead of cutting to it.
#[derive(Default)]
pub struct Timeline {
    phase: f32,
    pub playing: bool,
}

impl Timeline {
    pub fn new() -> Self {
        Self {
            phase: 0.0,
            playing: true,
        }
    }

    /// Triangle wave over the phase: out on the first half, back on the second.
    pub fn progress(&self) -> f32 {
        1.0 - (2.0 * self.phase - 1.0).abs()
    }

    /// Whether the effect is currently taking the model apart rather than
    /// building it.
    pub fn reversing(&self) -> bool {
        self.phase > 0.5
    }

    pub fn restart(&mut self) {
        self.phase = 0.0;
        self.playing = true;
    }

    /// Scrubs within the building half, which is the half a dragged slider
    /// reads as "how far along".
    pub fn scrub(&mut self, progress: f32) {
        self.phase = progress.clamp(0.0, 1.0) * 0.5;
    }

    /// Advances by one frame. `cycle_seconds` is the time for one direction, so
    /// a full round trip takes twice that.
    pub fn advance(&mut self, delta_seconds: f32, cycle_seconds: f32) {
        if !self.playing || delta_seconds <= 0.0 || cycle_seconds <= 0.0 {
            return;
        }
        self.phase = (self.phase + delta_seconds / (2.0 * cycle_seconds)).fract();
    }
}
