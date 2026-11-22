# Architecture Note: Tauri vs. Native Bevy UI

Before the spec: **do not wrap the live Bevy game window inside Tauri for in-game menus or HUDs.** Overlaying a Tauri WebView on top of a `wgpu` surface causes brutal cross-platform compositing bugs, input-focus fighting, and IPC latency between your Rust ECS and JavaScript.

* **In-game UI (Title Screen, Pause, Controls, Smash % HUD, Boss Bar):** Use native **Bevy UI** (which uses Flexbox/CSS Grid layout via `taffy`—very familiar if you know web CSS) and stays inside one fast binary.
* **Where Tauri *does* shine:** Use Tauri optionally as a **separate desktop Launcher / Asset Studio** (`launcher/`) that manages local AI generation, triggers headless Blender sprite bakes, edits level configs, and launches the compiled Bevy executable.

The spec below is written to be dropped straight into your repo as `SPEC.md` for your AI coding agent.

---

# Project Spec: *VELOCITY // BREAKER* (Working Title)

## 1. Concept & Core Design Pillars

A high-FPS, 2D pixel-art action-platformer built in **Rust + Bevy** combining three distinct mechanics with strict ownership boundaries:

* **Sonic owns Traversal & Flow:** Slopes, momentum preservation, speed boosters, and 360° loops move the player between encounters.
* **Mega Man owns Level Structure & Precision:** Vertical climbing shafts, moving platforms, hazards, and a reliable ranged **Buster** (tap for rapid shot, hold for charged blast).
* **Super Smash Bros. owns Combat & Game Feel:** Directional normals/aerials, drop-through platforms, **hitstop** (impact freeze-frames), percentage-based knockback, and recovery moves.
* **The Core Bridge (Momentum-to-Smash):** Player velocity directly scales melee knockback and hitstop. Conversely, Smash aerials and Up-Special recovery act as **air-brakes and course-correctors**, letting players snap out of 100 mph Sonic runs into tight Mega Man platforming.

---

## 2. Tech Stack & Repository Layout

### Core Stack

* **Engine:** Bevy (latest stable release) with `wgpu` 2D rendering, HDR, and Bloom enabled.
* **Input:** `leafwing-input-manager` (Gamepad-first with keyboard fallback).
* **Level Authoring:** `bevy_ecs_ldtk` (LDtk `.ldtk` project with hot-reloading).
* **Physics:** **Custom 2D Kinematic Controller** for player, slopes, loops, and Smash knockback. *(Do NOT use `bevy_rapier2d` or `avian2d` for player movement or Sonic loops—rigid-body solvers fight 360° loop snapping and crisp Smash air control.)*
* **Serialization:** `serde` + `ron` for sprite atlas manifests, frame data, and hitbox definitions.
* **Asset Pipeline:** Headless Blender (`blender --background`) Python (`bpy`) sprite baker producing crisp pixel-art multi-pass sheets.

### Repository Structure

```text
velocity-breaker/
├── Cargo.toml
├── SPEC.md
├── raw_assets/                  # User-dropped CC0 packs (Quaternius, KayKit Anims, Kenney)
├── external_assets/             # Agent-cloned git repos (KayKit Adventurers, Kenney UI Audio)
├── tools/
│   └── sprite_baker/
│       ├── bake_sprites.py      # Headless Blender (bpy) multi-pass pixel baker
│       └── pack_atlas.py        # Packs PNG frames into TextureAtlas + generates .ron manifest
├── assets/
│   ├── sprites/                 # Baked outputs: <char>_diffuse.png, _normal.png, _emission.png, .ron
│   ├── levels/                  # demo_gym.ldtk
│   ├── shaders/                 # 2D normal-mapped pixel lighting WGSL shader
│   └── audio/                   # SFX & UI audio
└── src/
    ├── main.rs
    ├── physics/                 # Custom 2D kinematics, slope alignment, loop splines
    ├── combat/                  # Hitboxes, hurtboxes, hitstop, Smash knockback, Mega Man buster
    ├── player/                  # State machine, animation driver, input handling
    ├── enemies/                 # Punching-bag / fighter AI, blast-zone respawn
    ├── rendering/               # Pixel-perfect camera, 2D normal-map shader, afterimages
    ├── level/                   # LDtk entity loaders (loops, drop-throughs, hazards)
    └── ui/                      # Native Bevy UI (Title, Pause, Smash % HUD)

```

---

## 3. Asset Acquisition & Blender-to-Bevy Pixel Pipeline

### 3.1 Agent Bootstrap (Run Automatically)

The AI coder must clone these CC0 repos into `external_assets/` during Phase 0 so the full pipeline can be tested immediately before custom AI characters are generated:

1. **Test Rig & Animations:** `git clone [https://github.com/KayKit-Game-Assets/KayKit-Character-Pack-Adventures-1.0.git](https://github.com/KayKit-Game-Assets/KayKit-Character-Pack-Adventures-1.0.git) external_assets/kaykit_adventurers`
2. **UI Audio:** `git clone [https://github.com/Calinou/kenney-ui-audio.git](https://github.com/Calinou/kenney-ui-audio.git) external_assets/kenney_ui_audio`

### 3.2 User-Provided Manual Drops (`raw_assets/`)

Expect (and gracefully fall back if not yet present) these CC0 packs in `raw_assets/`:

* `raw_assets/quaternius_anims/` — Quaternius Universal Animation Library
* `raw_assets/kaykit_anims/` — KayKit Character Animations
* `raw_assets/kenney_impact/` — Kenney Impact Sounds
* `raw_assets/kenney_input/` — Kenney Input Prompts

### 3.3 Headless Blender Pixel-Art Baker (`tools/sprite_baker/`)

Build a reproducible CLI script:
`blender --background --python tools/sprite_baker/bake_sprites.py -- --input <model.glb> --out assets/sprites/<name> --size 96`

**Requirements:**

1. **Camera & Style:** Orthographic side-view camera, fixed framing, **nearest-neighbor** pixel output (target **96×96** per frame; configurable to 64×64 or 128×128), Eevee cel/toon shading with zero anti-aliasing on alpha edges (`Film -> Filter Size = 0.01`).
2. **3 Synchronized Render Passes** per animation frame:
* `<name>_diffuse.png` — Quantized pixel-art albedo.
* `<name>_normal.png` — Tangent-space normal map for dynamic 2D lighting in Bevy.
* `<name>_emission.png` — Glow mask (buster muzzle, energy trails, visor) for Bevy HDR Bloom.


3. **Per-Frame Bone Hitbox Extraction:** Project designated bones (`hand.R`, `hand.L`, `foot.R`, `foot.L`, `buster_muzzle`, `chest`) into 2D sprite pixel coordinates for every frame and emit `<name>.ron`:

```ron
SpriteSheetManifest(
    frame_size: (96, 96),
    columns: 8,
    rows: 6,
    animations: {
        "run": AnimationDef(start: 0, end: 7, fps: 16.0, looping: true),
        "f_air": AnimationDef(
            start: 8, end: 13, fps: 18.0, looping: false,
            active_hitboxes: [
                FrameHitbox(frame: 10, offset: (22.0, 4.0), radius: 14.0, damage: 11.0, base_kb: 180.0, kb_scale: 2.8, angle_deg: 35.0),
                FrameHitbox(frame: 11, offset: (24.0, 2.0), radius: 14.0, damage: 11.0, base_kb: 180.0, kb_scale: 2.8, angle_deg: 35.0),
            ],
        ),
    },
)

```

*(Phase 2 Extensibility: Keep the baker input strictly `.glb` so local Image-to-3D outputs from **Hunyuan3D-2** / **TRELLIS.2** rigged via **UniRig** drop into the exact same pipeline with zero code changes.)*

---

## 4. Core Gameplay & Physics Architecture (Bevy ECS)

### 4.1 Movement & Sonic Loop Physics (`src/physics/`)

* **Slope Alignment:** Cast dual downward ground-sensor rays from the player's left/right foot bounds. Snap the player to terrain normals up to 45° naturally, adjusting ground speed by slope gravity ($a = -g \cdot \sin(\theta)$).
* **360° Loop Splines:** Loops are authored in LDtk as a `LoopSpline` entity with ordered control points (entry, apex, exit) and a minimum entry speed threshold (`min_speed`, e.g., `420.0 px/s`).
* When the player crosses the entry trigger at $\ge$ `min_speed`, transition to `MovementState::LoopLocked { spline_id, progress, speed }`.
* Advance `progress` along the cubic Bezier/Catmull-Rom spline, rotating the sprite to match the spline tangent.
* If speed drops below stall threshold near the apex, detach cleanly into `Airborne` with current velocity vector.
* On exit, restore standard velocity along the exit tangent.


* **Mega Man Precision:**
* Variable jump height (releasing Jump early cuts upward velocity by 50%).
* Coyote time (`0.09s`) and Jump buffering (`0.10s`).
* Wall-slide and crisp wall-jump off vertical shaft walls.


* **Smash Drop-Through Platforms:** One-way collision surfaces. Tapping Down while grounded (or holding Down while falling) disables one-way collision for `0.15s`.

### 4.2 Combat System (`src/combat/`)

* **ECS Components:** `Hurtbox`, `Hitbox`, `DamagePercent(f32)`, `Hitstun(Timer)`, `Hitstop(Timer)`, `Invulnerable(Timer)`.
* **Momentum-Scaled Smash Knockback Formula:**

$$KB = \text{base\_kb} + (\text{target\_percent} \times \text{kb\_scale}) + (\Vert{}\vec{v}_{\text{attacker}}\Vert{} \times \text{momentum\_mult})$$


* **Hitstop:** On hitbox-hurtbox contact, pause both attacker and target animation/physics timers for `3` to `8` frames (scaled by knockback) while applying a micro-jitter to the target sprite.
* **Directional Attacks (Minimum Vertical Slice Moveset):**
* **Neutral / Jab:** Fast 3-hit combo.
* **Dash Attack:** High-momentum shoulder/kick launch (preserves Sonic flow).
* **Forward-Air (`F-Air`):** Horizontal launcher; massive knockback if triggered at sprint speed.
* **Down-Air (`D-Air`):** Downward meteor spike + slight upward recoil bounce.
* **Up-Special (`Up-B`):** High vertical recovery burst that doubles as an **instant air-brake** to cancel horizontal Sonic momentum.


* **Mega Man Buster:**
* Tap Buster: Fires fast energy pellet (max 3 active on screen; low knockback, interrupts enemies).
* Hold Buster (`>= 0.7s`): Visual emission charge aura; release fires a piercing Charged Shot with heavy knockback.



---

## 5. Controls & Input Mapping

Gamepad-first (Xbox/DualSense standard) with full keyboard fallback via `leafwing-input-manager`:

| Action | Gamepad | Keyboard |
| --- | --- | --- |
| **Move / Aim / Fast-Fall** | Left Stick / D-Pad | `A` / `D` / `W` / `S` (or Arrow Keys) |
| **Jump** | `A` (Cross) | `Space` / `Z` |
| **Melee Attack (Directional)** | `X` (Square) | `J` / `X` |
| **Special / Recovery (Directional)** | `B` (Circle) | `K` / `C` |
| **Mega Man Buster (Tap / Hold)** | `RB` / `R1` (or `Y` / Triangle) | `L` / `Shift` |
| **Spin Dash / Boost** | `RT` / `R2` | `E` / `Left Ctrl` |
| **Pause** | `Start` / `Options` | `Esc` |

---

## 6. Milestone 1 Demo Level: *"The Foundry Gym"*

A single continuous, interconnected LDtk level (`assets/levels/demo_gym.ldtk`) designed to test every core pillar in under 3 minutes:

1. **Zone A — Sonic Runway & Loop:** Downhill acceleration ramp, speed-boost pads, a full 360° loop-de-loop, and a launch ramp that sends the player across a gap directly into Zone B.
2. **Zone B — Mega Man Vertical Shaft:** Tight vertical climb with alternating wall-jump walls, 2 moving platforms, spike hazards (with brief i-frames + knockback, not instant death), and 2 wall-mounted turrets that reward Buster shots.
3. **Zone C — Smash Arena:** A Battlefield-style chamber with 3 drop-through platforms, side/top blast zones, and **2 AI Enemies** (1 light fast-mover, 1 heavy punching bag that fights back with telegraphed attacks). Knocking an enemy into the blast zone triggers a Smash-style KO flash, sound effect, and 3-second respawn.

---

## 7. Visuals, Shaders & Game Feel

* **Pixel-Perfect Rendering:** Render world to a crisp low-res internal target (e.g., `480×270` or `640×360`) and integer-upscale with `ImageSampler::nearest()` so pixel art never shimmers or rotates unevenly.
* **Custom 2D Normal-Map Shader (`assets/shaders/sprite_lit.wgsl`):** Custom Bevy `Material2d` sampling `_diffuse`, `_normal`, and `_emission`. Supports up to 16 dynamic 2D point lights (player buster shots, loop boost strips, arena lamps, hit sparks).
* **Juice Requirements:**
* HDR Bloom driven exclusively by `_emission` textures and hit sparks.
* Translucent cyan/amber **afterimage ghost trails** spawned every `0.04s` when player speed exceeds `80%` max sprint speed.
* Directional camera trauma/shake on heavy hits and Charged Buster release.
* Smooth look-ahead camera that leads the player's velocity vector during high-speed Sonic sections and frames the arena cleanly in Zone C.



---

## 8. Execution Phases & Verification Criteria

The AI coder must execute sequentially and keep `cargo test` green at every phase:

### Phase 0: Workspace & Asset Bootstrap

* Initialize Cargo workspace with Bevy, `leafwing-input-manager`, `bevy_ecs_ldtk`, `serde`, `ron`.
* Clone `kaykit_adventurers` and `kenney_ui_audio` into `external_assets/`.
* Create a minimal compiling Bevy window with pixel-perfect camera and native Bevy UI Title -> Play -> Pause state flow.

### Phase 1: Headless Blender Sprite Baker

* Implement `tools/sprite_baker/bake_sprites.py` and `pack_atlas.py`.
* Run the pipeline against a KayKit Adventurers `.gltf` character to produce `_diffuse.png`, `_normal.png`, `_emission.png`, and `.ron` manifest in `assets/sprites/hero/`.
* **Verification:** Load the baked sheets into Bevy with the custom `sprite_lit.wgsl` shader and a moving 2D point light confirming visible normal-map shading and bloom.

### Phase 2: Sonic Movement + Mega Man Platforming

* Implement custom 2D kinematics, slope alignment, wall-jump, drop-through platforms, and `LoopSpline` traversal.
* **Verification (Automated Unit Tests):**
* `test_loop_entry_requires_min_speed`: Entering below threshold stays grounded; entering above threshold locks to spline and exits with tangent velocity.
* `test_drop_through_platform_timer`: Pressing Down disables one-way collision and re-enables after `0.15s`.
* `test_coyote_time_and_variable_jump`: Jump triggers within `0.09s` of leaving ledge; early release clamps upward velocity.



### Phase 3: Smash Combat & Mega Man Buster

* Implement `Hitbox`/`Hurtbox` overlap, momentum-scaled knockback, hitstop freeze frames, Buster tap/charge shots, and Smash `%` HUD.
* **Verification (Automated Unit Tests):**
* `test_momentum_scales_knockback`: Identical hit at sprint speed yields strictly higher knockback than stationary hit.
* `test_hitstop_pauses_physics`: Entities with active `Hitstop` do not integrate position until timer expires.
* `test_buster_cap_and_charge`: Max 3 uncharged pellets active; holding `>= 0.7s` fires charged variant.



### Phase 4: *"The Foundry Gym"* Demo Stage & Polish

* Author `assets/levels/demo_gym.ldtk` containing Zones A, B, and C.
* Wire up the 2 AI enemies, blast-zone KOs, Kenney SFX (if present in `raw_assets/`, else procedural fallback beeps), afterimages, and screen shake.
* **Final Acceptance Check:** Player can boot via Title Screen, run through the 360° loop, climb the vertical shaft shooting turrets, enter the Smash arena, combo an enemy at high speed into the blast zone, and pause/restart cleanly on both Gamepad and Keyboard at a locked 60+ FPS.

---

Drop this into `SPEC.md` in a fresh repo, grab those 4 CC0 zips into `raw_assets/` whenever you're ready, and let your coder rip through Phase 0 and Phase 1 first. Want me to also draft a short **`CLAUDE.md` / agent prompt** that locks down the coding rules (e.g., no Rapier for player movement, strict headless test gates per phase) before you kick it off?
