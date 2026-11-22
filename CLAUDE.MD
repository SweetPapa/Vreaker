Save the first block as `CLAUDE.md` in your repo root, then paste the kickoff prompt into your agent to start Phase 0.

---

## `CLAUDE.md`

```markdown
# CLAUDE.md — VELOCITY // BREAKER

## Source of Truth
Read `SPEC.md` before writing any code. Execute strictly in order (**Phase 0 → Phase 1 → Phase 2 → Phase 3 → Phase 4**). Do not jump ahead to later phases until the current phase's verification gates pass.

---

## Pinned Crate Versions & Fast-Compile Setup
Pin these compatible versions in `Cargo.toml` (avoid mixing Bevy minor versions):
- `bevy = "0.20"` (MSRV 1.97+)
- `leafwing-input-manager = "0.22"`
- `bevy_ecs_ldtk = "0.16"`
- `serde = { version = "1", features = ["derive"] }`
- `ron = "0.8"`

**Critical Bevy 0.20 / Leafwing 0.22 API Note:**
- `ActionState` and `InputMap` in `leafwing-input-manager` 0.22 are **components only** (not resources). Always attach both to the same entity (e.g., the player entity or a dedicated menu-input entity).

**Mandatory `Cargo.toml` dev profile** (keeps Bevy 60+ FPS in debug builds while keeping incremental compiles fast):
```toml
[profile.dev]
opt-level = 1

[profile.dev.package."*"]
opt-level = 3

```

---

## Non-Negotiable Architecture Rules

1. **NO rigid-body physics engines for gameplay.** Do NOT add `bevy_rapier2d`, `avian2d`, or similar for player movement, slopes, Sonic loops, or Smash knockback. Use custom 2D kinematics and spline snapping in `src/physics/`.
2. **NO Tauri or WebView overlays.** All in-game UI (Title, Pause, Smash `%` HUD) MUST use native Bevy UI.
3. **Deterministic Simulation in `FixedUpdate`.** Run all physics, slope alignment, loop spline progression, hitstop timers, hitbox/hurtbox checks, and knockback math inside `FixedUpdate` (60 Hz) so unit tests can step the simulation deterministically without a GPU or window.
4. **Decouple Logic from Rendering.** Physics (`src/physics/`) and combat (`src/combat/`) modules must NOT depend on `RenderPlugin`, window handles, or loaded PNG textures so they can be tested headlessly with `MinimalPlugins`.
5. **Pixel-Perfect Discipline.** Render the world to a fixed low-res internal target (`480x270` or `640x360`) and integer-upscale with `ImageSampler::nearest()`. Never allow fractional sub-pixel blur on sprites.
6. **Graceful Asset Fallbacks.** `external_assets/` is cloned by you in Phase 0. `raw_assets/` is user-supplied and may be empty—never panic if `raw_assets/` files are missing; fall back cleanly to procedural audio/placeholder shapes and log a warning.

---

## Blender Pixel-Art Baker Rules (`tools/sprite_baker/`)

* All Blender commands must run headless: `blender --background --python ...`.
* Output must be **nearest-neighbor pixel art** (default `96x96` per frame) with zero anti-aliasing on alpha edges (`Film -> Filter Size = 0.01`).
* Every bake must emit 4 synchronized artifacts in `assets/sprites/<name>/`:
1. `<name>_diffuse.png`
2. `<name>_normal.png`
3. `<name>_emission.png`
4. `<name>.ron` (matching the `SpriteSheetManifest` schema in `SPEC.md`).


* Keep the baker input strictly `.glb`/`.gltf` so future local AI Image-to-3D meshes drop in unchanged.

---

## Mandatory Phase Gates & Testing Discipline

Before declaring ANY phase complete, run and pass ALL of the following:

1. `cargo check --all-targets`
2. `cargo test` (including all headless ECS simulation tests required by `SPEC.md`)
3. `cargo clippy --all-targets -- -D warnings`

**Hard Rules on Tests:**

* Never delete, comment out, or loosen assertions in an existing test to make a build pass.
* Write the phase's required unit tests **before or alongside** the implementation (`test_loop_entry_requires_min_speed`, `test_drop_through_platform_timer`, `test_coyote_time_and_variable_jump`, `test_momentum_scales_knockback`, `test_hitstop_pauses_physics`, `test_buster_cap_and_charge`).
* At the end of each phase, report: (a) files created/modified, (b) exact command outputs for `cargo test` and `cargo clippy`, and (c) how to manually verify the milestone in-window, then **STOP and wait for user approval** before starting the next phase.

```

*(Version pins above are verified against current crates.io releases: `bevy` 0.20.1, `bevy_ecs_ldtk` 0.16.0, and `leafwing-input-manager` 0.22.0, including the 0.21+ removal of resource-based `ActionState`/`InputMap`.)*

---

## Kickoff Prompt (Paste Into Your AI Coder)

> Read `CLAUDE.md` and `SPEC.md` completely.
>
> Start with **Phase 0 (Workspace & Asset Bootstrap)** only:
> 1. Initialize the Cargo project with the pinned Bevy 0.20 dependencies and dev optimization profile from `CLAUDE.md`.
> 2. Clone the two CC0 repos into `external_assets/` (`kaykit_adventurers` and `kenney_ui_audio`) and create the empty `raw_assets/` directory with a short `README.md` noting where manual zips go.
> 3. Build the compiling Bevy app skeleton with a pixel-perfect camera, `leafwing-input-manager` input map (gamepad + keyboard), and a working native Bevy UI flow (`Title -> Playing -> Paused`).
> 4. Run `cargo check --all-targets`, `cargo test`, and `cargo clippy --all-targets -- -D warnings`.
>
> Stop after Phase 0 passes all checks and report back so I can run the window before we move to Phase 1 (the headless Blender pixel baker).

```
