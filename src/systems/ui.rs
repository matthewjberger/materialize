//! The playback bar and the effect's tuning panel.

use crate::plugin::{Materialize, UiHandles};
use crate::settings::Settings;
use nightshade::prelude::*;

pub fn build(materialize: &mut Materialize, world: &mut World) {
    let settings = &materialize.settings;
    let wire_thickness = settings.wire_thickness;
    let wire_alpha = settings.wire_alpha;
    let wire_glow = settings.wire_glow;
    let glass_alpha = settings.glass_alpha;
    let glass_glow_strength = settings.glass_glow_strength;
    let center_distance = settings.center_distance;
    let normal_distance = settings.normal_distance;
    let jitter = settings.jitter;
    let fade_portion = settings.fade_portion;
    let cool_span = settings.cool_span;
    let tumble = settings.tumble;
    let glass_band = settings.glass_band;
    let seam_width = settings.seam_width;
    let seam_strength = settings.seam_strength;
    let noise_amplitude = settings.noise_amplitude;
    let duration = settings.duration;
    let lag_glass_to_solid = settings.lag_glass_to_solid;
    let spin_speed = settings.spin_speed;

    let mut tree = UiTreeBuilder::new(world);
    let panel = tree.add_docked_panel_right("materialize", "Effect", 300.0);
    let content = panel_content(tree.world_mut(), panel);

    let mut handles = UiHandles::default();
    tree.in_parent(content, |tree| {
        // The controls are taller than any window, so they live in a scroll
        // area rather than running off the bottom of the panel.
        let scroll = tree.add_scroll_area_fill(8.0, 4.0);
        let body = widget::<UiScrollAreaData>(tree.world_mut(), scroll)
            .map(|data| data.content_entity)
            .expect("scroll area body missing");
        tree.in_parent(body, |tree| {
            handles.reset = tree.add_button("Reset to defaults");

            tree.add_label("");
            tree.add_label("Playback");
            handles.play = tree.add_button("Pause");
            handles.restart = tree.add_button("Restart");
            handles.progress_label = tree.add_label("Progress 0.00");
            handles.scrub = tree.add_slider(0.0, 1.0, 0.0);

            tree.add_label("");
            tree.add_label("Wireframe");
            tree.add_label("Thickness");
            handles.wire_thickness = tree.add_slider(0.001, 0.03, wire_thickness);
            tree.add_label("Opacity");
            handles.wire_alpha = tree.add_slider(0.0, 1.0, wire_alpha);
            tree.add_label("Front glow");
            handles.wire_glow = tree.add_slider(0.0, 30.0, wire_glow);

            tree.add_label("");
            tree.add_label("Glass");
            tree.add_label("Translucency");
            handles.glass_alpha = tree.add_slider(0.0, 1.0, glass_alpha);
            tree.add_label("Glow intensity");
            handles.glass_glow_strength = tree.add_slider(0.0, 30.0, glass_glow_strength);
            tree.add_label("From center");
            handles.center_distance = tree.add_slider(0.0, 3.0, center_distance);
            tree.add_label("Off normal");
            handles.normal_distance = tree.add_slider(0.0, 2.0, normal_distance);
            tree.add_label("Jitter");
            handles.jitter = tree.add_slider(0.0, 1.0, jitter);
            tree.add_label("Fade portion");
            handles.fade_portion = tree.add_slider(0.05, 1.0, fade_portion);
            tree.add_label("Glow cool span");
            handles.cool_span = tree.add_slider(0.05, 4.0, cool_span);
            tree.add_label("Tumble");
            handles.tumble = tree.add_slider(0.0, 3.0, tumble);
            tree.add_label("Assembly band");
            handles.glass_band = tree.add_slider(0.05, 1.0, glass_band);

            tree.add_label("");
            tree.add_label("Reveal");
            tree.add_label("Seam thickness");
            handles.seam_width = tree.add_slider(0.005, 0.3, seam_width);
            tree.add_label("Seam brightness");
            handles.seam_strength = tree.add_slider(0.0, 40.0, seam_strength);
            tree.add_label("Noise amount");
            handles.noise_amplitude = tree.add_slider(0.0, 0.3, noise_amplitude);

            tree.add_label("");
            tree.add_label("Timing");
            tree.add_label("Cycle seconds");
            handles.duration = tree.add_slider(3.0, 20.0, duration);
            tree.add_label("Glass to solid lag");
            handles.lag_glass_to_solid = tree.add_slider(0.0, 1.5, lag_glass_to_solid);
            tree.add_label("Spin speed");
            handles.spin_speed = tree.add_slider(0.0, 1.5, spin_speed);
        });
    });
    tree.finish();

    materialize.ui = handles;
}

pub fn poll(materialize: &mut Materialize, world: &mut World) {
    let handles = materialize.ui;

    let mut play_clicked = false;
    let mut restart_clicked = false;
    let mut reset_clicked = false;
    for event in ui_events(world) {
        if let UiEvent::ButtonClicked(entity) = event {
            play_clicked |= *entity == handles.play;
            restart_clicked |= *entity == handles.restart;
            reset_clicked |= *entity == handles.reset;
        }
    }
    if play_clicked {
        materialize.timeline.playing = !materialize.timeline.playing;
    }
    if restart_clicked {
        materialize.timeline.restart();
    }

    if let Some(value) = ui_slider_value_changed(world, handles.scrub) {
        materialize.timeline.scrub(value);
        materialize.timeline.playing = false;
    } else if materialize.timeline.playing {
        ui_slider_set_value(world, handles.scrub, materialize.timeline.progress());
    }

    if play_clicked || restart_clicked {
        let label = if materialize.timeline.playing {
            "Pause"
        } else {
            "Play"
        };
        ui_button_set_text(world, handles.play, label);
    }
    let direction = if materialize.timeline.reversing() {
        "Dematerializing"
    } else {
        "Materializing"
    };
    ui_set_text(
        world,
        handles.progress_label,
        &format!("{direction} {:.2}", materialize.timeline.progress()),
    );

    for (entity, field) in slider_bindings(&handles) {
        if let Some(value) = ui_slider_value_changed(world, entity) {
            *field(&mut materialize.settings) = value;
        }
    }

    if reset_clicked {
        materialize.settings = Settings::default();
        for (entity, field) in slider_bindings(&handles) {
            let value = *field(&mut materialize.settings);
            ui_slider_set_value(world, entity, value);
        }
    }
}

/// A slider and the setting it edits.
type SliderBinding = (Entity, fn(&mut Settings) -> &mut f32);

/// Which setting each slider edits. One list drives both directions: reading a
/// dragged slider into the settings, and writing the settings back out when
/// they are reset.
fn slider_bindings(handles: &UiHandles) -> [SliderBinding; 18] {
    [
        (handles.wire_thickness, |settings| {
            &mut settings.wire_thickness
        }),
        (handles.wire_alpha, |settings| &mut settings.wire_alpha),
        (handles.wire_glow, |settings| &mut settings.wire_glow),
        (handles.glass_alpha, |settings| &mut settings.glass_alpha),
        (handles.glass_glow_strength, |settings| {
            &mut settings.glass_glow_strength
        }),
        (handles.center_distance, |settings| {
            &mut settings.center_distance
        }),
        (handles.normal_distance, |settings| {
            &mut settings.normal_distance
        }),
        (handles.jitter, |settings| &mut settings.jitter),
        (handles.fade_portion, |settings| &mut settings.fade_portion),
        (handles.cool_span, |settings| &mut settings.cool_span),
        (handles.tumble, |settings| &mut settings.tumble),
        (handles.glass_band, |settings| &mut settings.glass_band),
        (handles.seam_width, |settings| &mut settings.seam_width),
        (handles.seam_strength, |settings| {
            &mut settings.seam_strength
        }),
        (handles.noise_amplitude, |settings| {
            &mut settings.noise_amplitude
        }),
        (handles.duration, |settings| &mut settings.duration),
        (handles.lag_glass_to_solid, |settings| {
            &mut settings.lag_glass_to_solid
        }),
        (handles.spin_speed, |settings| &mut settings.spin_speed),
    ]
}

fn panel_content(world: &mut DynEcs, panel: Entity) -> Entity {
    widget::<UiPanelData>(world, panel)
        .map(|data| data.content_entity)
        .expect("panel content missing")
}
