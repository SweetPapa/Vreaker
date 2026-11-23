// GPU-accelerated multi-mode procedural 2D energy/VFX shader for *VELOCITY // BREAKER*.
// Eliminates blocky rectangle placeholders for shields, plasma lasers, lockdown barriers,
// charge auras, shockwave rings, sculpted curved saber crescents, and volumetric lamp cones!
//
// Modes (`params0.x`):
//   0.0 = Hexagonal Holographic Smash Shield (Fresnel sphere + animated hex honeycomb + scanline)
//   1.0 = High-Voltage Plasma Laser Beam / Lockdown Barrier (hot core + lightning arcs + bloom)
//   2.0 = Swirling Energy Charge Vortex (inward spiraling plasma rings)
//   3.0 = Silky Expanding Energy Shockwave Ring (radial halo + electric ripple)
//   4.0 = Sculpted Curved Crescent Blade Smear (tapered anime sword arc + white-hot edge)
//   5.0 = Volumetric Foundry Lamp Light Cone (soft downward beam + drifting dust shimmer)

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(2) uv: vec2<f32>,
};

struct VfxEnergyUniform {
    // x = mode (0..5), y = elapsed time (secs), z = progress/health (0..1), w = intensity/alpha (0..1)
    params0: vec4<f32>,
    // x = aspect (width/height), y = sweep/speed/extra, z = telegraph_flag (0/1), w = flip_x (+1/-1)
    params1: vec4<f32>,
    // Primary HDR energy color (linear RGB, A = master opacity)
    color_a: vec4<f32>,
    // Secondary / core highlight HDR color (linear RGB, A = secondary mix)
    color_b: vec4<f32>,
};

@group(2) @binding(0) var<uniform> vfx: VfxEnergyUniform;

const PI: f32 = 3.14159265359;
const TAU: f32 = 6.28318530718;

fn hash21(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(123.34, 456.21));
    let r = q + dot(q, q + 45.32);
    return fract(r.x * r.y);
}

// Smooth distance to a hexagonal cell border for futuristic honeycomb shields.
fn hex_edge_mask(uv: vec2<f32>, scale: f32) -> f32 {
    let r = vec2<f32>(1.0, 1.7320508);
    let h = r * 0.5;
    let p = uv * scale;
    let a = (p - r * floor(p / r)) - h;
    let p_off = p - h;
    let b = (p_off - r * floor(p_off / r)) - h;
    let gv = select(b, a, dot(a, a) < dot(b, b));
    let ap = abs(gv);
    let dist_center = max(dot(ap, normalize(r)), ap.x);
    let edge = 0.5 - dist_center;
    return 1.0 - smoothstep(0.025, 0.085, edge);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let mode = i32(vfx.params0.x + 0.5);
    let t = vfx.params0.y;
    let prog = clamp(vfx.params0.z, 0.0, 1.0);
    let master = clamp(vfx.params0.w, 0.0, 1.5);

    if master <= 0.002 {
        discard;
    }

    let flip = select(1.0, -1.0, vfx.params1.w < 0.0);
    let uv = vec2<f32>(select(in.uv.x, 1.0 - in.uv.x, flip < 0.0), in.uv.y);
    let p = uv * 2.0 - vec2<f32>(1.0); // [-1..1]

    // -------------------------------------------------------------------------
    // MODE 0: HEXAGONAL HOLOGRAPHIC SMASH SHIELD DOME
    // -------------------------------------------------------------------------
    if mode == 0 {
        let r = length(p);
        if r > 0.99 {
            discard;
        }
        // Spherical pseudo-normal Z for crisp Fresnel rim glow
        let sphere_z = sqrt(max(0.0, 1.0 - r * r));
        let fresnel = (1.0 - sphere_z) * (1.0 - sphere_z);
        // Subtle spherized UV warp so hex cells curve around the energy bubble
        let hex_uv = p / (0.65 + 0.45 * sphere_z) + vec2<f32>(t * 0.08, -t * 0.12);
        let hex = hex_edge_mask(hex_uv, 4.8);
        let rim_ring = smoothstep(0.76, 0.94, r) * (1.0 - smoothstep(0.94, 1.0, r));
        let scan = 0.5 + 0.5 * sin(p.y * 22.0 - t * 9.0);
        // Crackling shimmer increases when shield health gets low
        let danger = 1.0 - prog;
        let crackle = smoothstep(0.78, 0.98, sin(p.x * 28.0 + p.y * 19.0 + t * 24.0)) * danger;

        let alpha_body = (0.14 + 0.44 * fresnel + 0.24 * hex * (0.35 + 0.65 * fresnel) + 0.68 * rim_ring + 0.25 * crackle) * master;
        let rgb = mix(vfx.color_a.rgb, vfx.color_b.rgb, clamp(rim_ring * 0.85 + hex * 0.32 + crackle * 0.7 + scan * 0.08, 0.0, 1.0));
        return vec4<f32>(rgb, clamp(alpha_body * vfx.color_a.a, 0.0, 0.95));
    }

    // -------------------------------------------------------------------------
    // MODE 1: HIGH-VOLTAGE PLASMA LASER BEAM / LOCKDOWN ENERGY BARRIER
    // -------------------------------------------------------------------------
    if mode == 1 {
        let is_telegraph = vfx.params1.z > 0.5;
        let aspect = max(vfx.params1.x, 0.05);
        // Vertical axis along beam = p.y, radial distance from beam center = abs(p.x)
        let ax = abs(p.x);
        let scroll_y = uv.y * max(aspect, 2.0);

        if is_telegraph {
            // Crisp flickering targeting laser sightline with scrolling caution dashes!
            let dash = smoothstep(0.15, 0.35, sin(scroll_y * 24.0 - t * 26.0));
            let core = exp(-ax * ax * 48.0);
            let blink = 0.55 + 0.45 * sin(t * 36.0);
            let a = core * (0.45 + 0.55 * dash) * blink * master;
            if a < 0.03 {
                discard;
            }
            let rgb = mix(vfx.color_a.rgb, vfx.color_b.rgb, core);
            return vec4<f32>(rgb, clamp(a, 0.0, 0.95));
        }

        // Animated sinusoidal plasma wobble + crackling dual-frequency electric arcs!
        let wobble = 0.08 * sin(scroll_y * 10.0 - t * 24.0) + 0.04 * sin(scroll_y * 23.0 + t * 34.0);
        let dx = abs(p.x + wobble);
        // End-cap taper so beams terminate cleanly inside emitter nozzles
        let end_fade = smoothstep(0.0, 0.06, uv.y) * smoothstep(0.0, 0.06, 1.0 - uv.y);
        let core = exp(-dx * dx * 26.0);
        let outer = exp(-dx * dx * 4.4);
        // Twin spiralling electric filaments hugging the plasma core (squared directly for Metal NaN safety)
        let arc1_x = 0.34 * sin(scroll_y * 14.0 - t * 28.0);
        let arc2_x = -0.28 * sin(scroll_y * 19.0 + t * 22.0 + 1.4);
        let d_arc1 = (p.x - arc1_x) * 9.5;
        let d_arc2 = (p.x - arc2_x) * 10.5;
        let arc_glow = exp(-(d_arc1 * d_arc1)) + exp(-(d_arc2 * d_arc2));
        // Scrolling thermal plasma density bands
        let bands = 0.80 + 0.20 * sin(scroll_y * 18.0 - t * 22.0);

        let energy = (outer * 0.78 * bands + core * 1.15 + arc_glow * 0.55) * end_fade * master;
        if energy < 0.04 {
            discard;
        }
        let rgb = mix(vfx.color_a.rgb, vfx.color_b.rgb, clamp(core * 0.92 + arc_glow * 0.48, 0.0, 1.0));
        return vec4<f32>(rgb, clamp(energy * vfx.color_a.a, 0.0, 0.98));
    }

    // -------------------------------------------------------------------------
    // MODE 2: SWIRLING RIFT BUSTER CHARGE VORTEX
    // -------------------------------------------------------------------------
    if mode == 2 {
        let r = length(p);
        if r > 0.98 {
            discard;
        }
        let ang = atan2(p.y, p.x);
        // Inward-pulling dual-arm spiral plasma ribbons
        let spiral = 0.5 + 0.5 * sin(ang * 3.0 + r * 14.0 + t * 18.0);
        let ring = smoothstep(0.18, 0.45, r) * (1.0 - smoothstep(0.55, 0.98, r));
        let center_glow = exp(-r * r * 6.0) * (0.4 + 0.6 * prog);
        let shimmer = 0.75 + 0.25 * sin(t * 26.0 + r * 10.0);
        let alpha = (ring * (0.28 + 0.72 * spiral) * shimmer + center_glow * 0.55) * master;
        if alpha < 0.025 {
            discard;
        }
        let rgb = mix(vfx.color_a.rgb, vfx.color_b.rgb, clamp(center_glow + spiral * prog * 0.55, 0.0, 1.0));
        return vec4<f32>(rgb, clamp(alpha * vfx.color_a.a, 0.0, 0.92));
    }

    // -------------------------------------------------------------------------
    // MODE 3: SILKY EXPANDING RADIAL ENERGY SHOCKWAVE RING
    // -------------------------------------------------------------------------
    if mode == 3 {
        let r = length(p);
        let target_r = mix(0.16, 0.88, prog);
        let thickness = mix(0.16, 0.055, prog);
        let d = abs(r - target_r);
        let ang = atan2(p.y, p.x);
        let ripple = 1.0 + 0.12 * sin(ang * 12.0 + t * 24.0);
        let q_ring = d / max(thickness * ripple, 0.015);
        let q_hot = d / max(thickness * 0.35, 0.008);
        let ring = exp(-(q_ring * q_ring));
        let inner_hot = exp(-(q_hot * q_hot));
        let fade = (1.0 - prog) * master;
        let alpha = ring * fade;
        if alpha < 0.03 {
            discard;
        }
        let rgb = mix(vfx.color_a.rgb, vfx.color_b.rgb, clamp(inner_hot * 0.9, 0.0, 1.0));
        return vec4<f32>(rgb, clamp(alpha * vfx.color_a.a, 0.0, 0.96));
    }

    // -------------------------------------------------------------------------
    // MODE 4: SCULPTED CURVED CRESCENT SABER SMEAR / BREAKER WAVE
    // -------------------------------------------------------------------------
    if mode == 4 {
        let r = length(p);
        let ang = atan2(p.y, p.x); // 0 = forward (+X), +/-PI = behind
        let sweep_half = max(vfx.params1.y, 0.9);
        let norm_ang = abs(ang) / sweep_half;
        if norm_ang > 1.05 || r > 0.98 || r < 0.18 {
            discard;
        }
        // Tapered crescent blade profile: widest at nose (ang=0), needle-fine tips at +-sweep_half!
        let ang_taper = max(0.0001, 1.0 - norm_ang * norm_ang);
        let outer_r = 0.88 - 0.10 * norm_ang * norm_ang;
        let blade_w = (0.34 + 0.14 * (1.0 - prog)) * pow(ang_taper, 0.85);
        let inner_r = outer_r - blade_w;
        if r < inner_r || r > outer_r + 0.08 {
            discard;
        }
        let across = clamp((r - inner_r) / max(outer_r - inner_r, 0.01), 0.0, 1.0);
        let outer_feather = 1.0 - smoothstep(outer_r - 0.03, outer_r + 0.06, r);
        let inner_feather = smoothstep(0.0, 0.32, across);
        // Incandescent razor leading edge right along outer crescent rim
        let razor_edge = smoothstep(0.68, 0.96, across) * outer_feather;
        // Subtle sweeping plasma streaks along the blade curve
        let streak = 0.82 + 0.18 * sin(r * 38.0 - t * 28.0 + ang * 4.0);
        let fade = (1.0 - prog * 0.85) * master;
        let alpha = inner_feather * outer_feather * sqrt(ang_taper) * streak * fade;
        if alpha < 0.03 {
            discard;
        }
        let rgb = mix(vfx.color_a.rgb, vfx.color_b.rgb, clamp(razor_edge * 0.95 + across * 0.25, 0.0, 1.0));
        return vec4<f32>(rgb, clamp(alpha * vfx.color_a.a, 0.0, 0.98));
    }

    // -------------------------------------------------------------------------
    // MODE 5: VOLUMETRIC INDUSTRIAL LAMP LIGHT CONE
    // -------------------------------------------------------------------------
    // uv.y = 0 at lamp hood (top), uv.y = 1 at bottom of cone
    let y_down = uv.y;
    let cone_half_w = mix(0.12, 0.96, y_down);
    let side_d = abs(p.x) / max(cone_half_w, 0.01);
    if side_d >= 1.0 {
        discard;
    }
    let radial_base = max(0.0001, 1.0 - side_d * side_d);
    let radial_soft = pow(radial_base, 1.65);
    let vert_base = max(0.0001, 1.0 - y_down);
    let vertical_fade = pow(vert_base, 1.55) * smoothstep(0.0, 0.08, y_down);
    // Subtle drifting dust-mote volumetric shimmer inside the light shaft
    let dust = 0.86 + 0.14 * sin(uv.x * 16.0 + uv.y * 12.0 - t * 1.6) * sin(uv.y * 22.0 + t * 1.1);
    let alpha = radial_soft * vertical_fade * dust * master * vfx.color_a.a;
    if alpha < 0.004 {
        discard;
    }
    let rgb = mix(vfx.color_a.rgb, vfx.color_b.rgb, (1.0 - y_down) * 0.35);
    return vec4<f32>(rgb, clamp(alpha, 0.0, 0.42));
}
