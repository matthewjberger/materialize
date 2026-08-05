# Materialize

A model phasing into existence, built on the [Nightshade](https://github.com/matthewjberger/nightshade) engine.

Three stages run over the same helmet, each its own draw:

1. A wireframe forms from the bottom up, drawn as view-facing ribbons over the mesh's unique edges with a glowing sweep front.
2. Glass shards fly in and assemble over it. The mesh is unwelded into a triangle soup where every triangle carries its own centroid and seed, and the vertex stage flies each shard in from a scattered pose, tumbling, easing into a soft landing, then glowing on the surface until the shell phases over it.
3. The real surface reveals itself behind a hot emissive seam, using the engine's reveal material.

One eased progress value drives three staggered sweep fronts. Progress runs out and back, so the model materializes and then dematerializes through the same stages in reverse and the loop closes on the hidden model. Every stage compares against the same simplex-noise-wobbled boundary, so their edges line up.

The environment lights the model through image-based lighting but is never drawn, which leaves the wireframe, the shards and the seam reading as light against a near black background.

## Quickstart

```bash
# native
just run

# wasm (webgpu)
just run-wasm

# steam deck
just build-steamdeck
just deploy-steamdeck        # copies binary to ~/Downloads on deck
just deploy-steamdeck-quick  # copies as 'game' for quick launching
```

> All chromium-based browsers like Brave, Vivaldi, Chrome, etc support WebGPU.

## Layout

- `src/settings.rs` — every tunable of the effect, and the timeline that runs the sweep out and back.
- `src/geometry.rs` — derives the shard soup and the edge ribbons from the imported mesh.
- `src/pass.rs` — the app's render pass, drawing the wireframe and the glass. Both stages displace their geometry in the vertex stage, which is why they are drawn here rather than through the engine's material path.
- `src/shaders/` — the wireframe and glass WGSL. Both import the engine's reveal noise so their boundaries match the surface reveal exactly.
- `src/systems/` — `setup` loads the model and composes the render graph, `effect` drives the three stages each frame, `ui` builds the tuning panel.
