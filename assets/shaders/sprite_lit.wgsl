// Custom 2D normal-mapped pixel-art lighting shader for *VELOCITY // BREAKER*.
// Samples synchronized diffuse, tangent/camera-space normal, and emission atlases,
// evaluates up to 16 dynamic 2D point lights, and routes HDR Bloom exclusively via emission.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) world_position: vec4<f32>,
    @location(2) uv: vec2<f32>,
};

struct PointLight2dUniform {
    // xy = world position (px), z = height above sprite plane (px), w = radius (px)
    pos_height_radius: vec4<f32>,
    // rgb = linear color * intensity, w = falloff exponent
    color_falloff: vec4<f32>,
};

struct SpriteLitUniform {
    // xy = frame top-left UV offset, zw = frame UV size (1/cols, 1/rows)
    atlas_rect: vec4<f32>,
    // rgb = linear ambient color, a = ambient strength
    ambient: vec4<f32>,
    // x = flip_x (+1.0 right, -1.0 left), y = cos(rot), z = sin(rot), w = active light count
    transform_params: vec4<f32>,
    // x = emission_strength (HDR), y = hit_flash (0..1), z = cel_steps, w = rim_spec_strength
    fx_params: vec4<f32>,
    // rgb = tint multiplier, a = alpha multiplier (supports translucent afterimages)
    tint: vec4<f32>,
    // Up to 16 dynamic 2D point lights
    lights: array<PointLight2dUniform, 16>,
};

@group(2) @binding(0) var<uniform> material: SpriteLitUniform;
@group(2) @binding(1) var diffuse_texture: texture_2d<f32>;
@group(2) @binding(2) var diffuse_sampler: sampler;
@group(2) @binding(3) var normal_texture: texture_2d<f32>;
@group(2) @binding(4) var normal_sampler: sampler;
@group(2) @binding(5) var emission_texture: texture_2d<f32>;
@group(2) @binding(6) var emission_sampler: sampler;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let flip_sign = select(-1.0, 1.0, material.transform_params.x >= 0.0);
    let local_u = select(1.0 - in.uv.x, in.uv.x, flip_sign > 0.0);
    let atlas_uv = material.atlas_rect.xy + vec2<f32>(local_u, in.uv.y) * material.atlas_rect.zw;

    // Sample all three passes unconditionally in uniform control flow before alpha discard.
    let diffuse = textureSample(diffuse_texture, diffuse_sampler, atlas_uv);
    let n_tex = textureSample(normal_texture, normal_sampler, atlas_uv).xyz;
    let emission_sample = textureSample(emission_texture, emission_sampler, atlas_uv).rgb;

    if diffuse.a < 0.5 {
        discard;
    }

    // Decode linear camera-space normal map [0..1] -> [-1..1], mirror X when flipped,
    // and rotate XY by sprite Z rotation so normals stay accurate through 360° loops.
    var n_local = n_tex * 2.0 - vec3<f32>(1.0);
    n_local.x = n_local.x * flip_sign;

    let cos_r = material.transform_params.y;
    let sin_r = material.transform_params.z;
    let world_n = normalize(vec3<f32>(
        n_local.x * cos_r - n_local.y * sin_r,
        n_local.x * sin_r + n_local.y * cos_r,
        max(n_local.z, 0.08),
    ));

    let albedo = diffuse.rgb * material.tint.rgb;
    // Balanced hemisphere ambient fill keeps silhouettes readable without over-brightening sprites.
    let hemi = mix(0.90, 1.10, clamp(world_n.y * 0.5 + 0.5, 0.0, 1.0));
    let ambient_rgb = albedo * material.ambient.rgb * material.ambient.a * hemi;

    var lit_rgb = vec3<f32>(0.0);
    let light_count = min(u32(max(material.transform_params.w, 0.0)), 16u);
    let cel_steps = material.fx_params.z;
    let rim_spec = material.fx_params.w;

    for (var i: u32 = 0u; i < light_count; i = i + 1u) {
        let light = material.lights[i];
        let radius = max(light.pos_height_radius.w, 1.0);
        let delta = vec3<f32>(
            light.pos_height_radius.xy - in.world_position.xy,
            light.pos_height_radius.z,
        );
        let dist = length(delta);

        if dist < radius {
            let atten = clamp(1.0 - dist / radius, 0.0, 1.0);
            if atten > 0.0001 {
                let falloff = pow(atten, max(light.color_falloff.w, 0.5));
                let l_dir = delta / max(dist, 0.0001);

                var ndotl = max(dot(world_n, l_dir), 0.0);
                if cel_steps >= 2.0 {
                    let stepped = floor(ndotl * cel_steps + 0.5) / cel_steps;
                    ndotl = mix(ndotl, stepped, 0.40);
                }

                // Crisp metallic pixel highlight along the half-vector toward the camera (+Z)
                // Computed via squaring rather than pow() to avoid Metal pow(0, y) NaN edge cases.
                let h_dir = normalize(l_dir + vec3<f32>(0.0, 0.0, 1.0));
                let ndoth = max(dot(world_n, h_dir), 0.0);
                let s2 = ndoth * ndoth;
                let s4 = s2 * s2;
                let s8 = s4 * s4;
                let spec = s8 * s8 * rim_spec;

                lit_rgb = lit_rgb + (albedo * ndotl + vec3<f32>(spec)) * light.color_falloff.rgb * falloff;
            }
        }
    }

    // Subtle cool-silver Fresnel edge rim-light separating character & weapon silhouettes cleanly
    // from dark industrial backgrounds without washing out shadows or HDR emissive accents.
    let edge = 1.0 - clamp(world_n.z, 0.0, 1.0);
    let rim = edge * edge * (0.06 + rim_spec * 0.14);
    let rim_rgb = mix(albedo, vec3<f32>(0.46, 0.62, 0.78), 0.38) * rim;

    // Attenuate stacked point lights gently & cap diffuse below 0.78 so sprites stay rich and grounded
    // while HDR emission (sabers, visors, furnace cores) pops crisply via Bloom.
    let shaded_rgb = min(ambient_rgb + lit_rgb * 0.68 + rim_rgb, vec3<f32>(0.78));

    let emissive_rgb = emission_sample * material.fx_params.x;

    let hit_flash = clamp(material.fx_params.y, 0.0, 1.0);
    let final_rgb = mix(shaded_rgb + emissive_rgb, vec3<f32>(2.4, 2.4, 2.6), hit_flash);

    return vec4<f32>(final_rgb, diffuse.a * material.tint.a);
}
