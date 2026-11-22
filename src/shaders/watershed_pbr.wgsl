// Copyright (C) 2026 CRV Science Network (https://crvscience.com)
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://gnu.org>.

struct SceneUniforms {
    view_proj: mat4x4<f32>,
    light_view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    sun_dir_and_time: vec4<f32>,
    sun_color_and_vpd: vec4<f32>,
    fog_color_and_density: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> scene: SceneUniforms;

@group(0) @binding(1)
var shadow_map: texture_depth_2d;

@group(0) @binding(2)
var shadow_sampler: sampler_comparison;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color_alpha: vec4<f32>,
    @location(3) mat_params: vec4<f32>, // x: roughness, y: sway_weight, z: mat_kind (0=solid,1=foliage,2=water,3=emissive_fire,4=sunbeam), w: ao
};

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) color_alpha: vec4<f32>,
    @location(3) mat_params: vec4<f32>,
    @location(4) shadow_pos: vec4<f32>,
};

fn compute_wind_offset(pos: vec3<f32>, sway_weight: f32, time: f32) -> vec3<f32> {
    if (sway_weight <= 0.001) {
        return vec3<f32>(0.0, 0.0, 0.0);
    }
    let phase_a = pos.x * 0.45 + pos.z * 0.35 + time * 1.65;
    let phase_b = pos.x * 1.15 - pos.z * 0.90 + time * 2.85;
    let gust = sin(phase_a) * 0.09 + cos(phase_b) * 0.035;
    return vec3<f32>(gust * sway_weight, -abs(gust) * 0.02 * sway_weight, gust * 0.65 * sway_weight);
}

@vertex
fn vs_shadow(in: VertexInput) -> @builtin(position) vec4<f32> {
    let time = scene.sun_dir_and_time.w;
    let displaced = in.position + compute_wind_offset(in.position, in.mat_params.y, time);
    return scene.light_view_proj * vec4<f32>(displaced, 1.0);
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let time = scene.sun_dir_and_time.w;
    var displaced = in.position + compute_wind_offset(in.position, in.mat_params.y, time);
    var nrm = normalize(in.normal);

    // Subtle animated surface ripples on salmon creek & beaver pools
    if (abs(in.mat_params.z - 2.0) < 0.25) {
        let wave1 = sin(in.position.x * 3.4 + time * 3.1) * 0.018;
        let wave2 = cos(in.position.z * 4.1 - time * 2.6) * 0.015;
        displaced.y = displaced.y + wave1 + wave2;
        nrm = normalize(vec3<f32>(-cos(in.position.x * 3.4 + time * 3.1) * 0.10, 1.0, sin(in.position.z * 4.1 - time * 2.6) * 0.10));
    }

    out.world_pos = displaced;
    out.world_normal = nrm;
    out.color_alpha = in.color_alpha;
    out.mat_params = in.mat_params;
    out.clip_pos = scene.view_proj * vec4<f32>(displaced, 1.0);
    out.shadow_pos = scene.light_view_proj * vec4<f32>(displaced, 1.0);
    return out;
}

fn sample_pcf_shadow(shadow_clip: vec4<f32>, ndotl: f32) -> f32 {
    let proj = shadow_clip.xyz / max(shadow_clip.w, 0.0001);
    let uv = vec2<f32>(proj.x * 0.5 + 0.5, -proj.y * 0.5 + 0.5);
    if (uv.x < 0.01 || uv.x > 0.99 || uv.y < 0.01 || uv.y > 0.99 || proj.z < 0.0 || proj.z > 1.0) {
        return 1.0;
    }
    let bias = max(0.0022 * (1.0 - ndotl), 0.0007);
    let texel = 1.0 / 2048.0;
    var acc = 0.0;
    for (var y: i32 = -1; y <= 1; y = y + 1) {
        for (var x: i32 = -1; x <= 1; x = x + 1) {
            let offset = vec2<f32>(f32(x), f32(y)) * texel * 1.35;
            acc = acc + textureSampleCompare(shadow_map, shadow_sampler, uv + offset, proj.z - bias);
        }
    }
    return mix(0.32, 1.0, acc / 9.0);
}

fn hash31(p_in: vec3<f32>) -> f32 {
    var p3 = fract(p_in * 0.1031);
    p3 = p3 + vec3<f32>(dot(p3, p3.yzx + vec3<f32>(33.33)));
    return fract((p3.x + p3.y) * p3.z);
}

fn value_noise_3d(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (vec3<f32>(3.0) - 2.0 * f);

    let n000 = hash31(i + vec3<f32>(0.0, 0.0, 0.0));
    let n100 = hash31(i + vec3<f32>(1.0, 0.0, 0.0));
    let n010 = hash31(i + vec3<f32>(0.0, 1.0, 0.0));
    let n110 = hash31(i + vec3<f32>(1.0, 1.0, 0.0));
    let n001 = hash31(i + vec3<f32>(0.0, 0.0, 1.0));
    let n101 = hash31(i + vec3<f32>(1.0, 0.0, 1.0));
    let n011 = hash31(i + vec3<f32>(0.0, 1.0, 1.0));
    let n111 = hash31(i + vec3<f32>(1.0, 1.0, 1.0));

    let nx00 = mix(n000, n100, u.x);
    let nx10 = mix(n010, n110, u.x);
    let nx01 = mix(n001, n101, u.x);
    let nx11 = mix(n011, n111, u.x);

    let nxy0 = mix(nx00, nx10, u.y);
    let nxy1 = mix(nx01, nx11, u.y);
    return mix(nxy0, nxy1, u.z);
}

fn procedural_detail(pos: vec3<f32>, freq: f32) -> f32 {
    let n_coarse = value_noise_3d(pos * freq);
    let n_fine = value_noise_3d(pos * (freq * 2.35) + vec3<f32>(7.1, 13.7, 5.3));
    return (n_coarse * 0.65 + n_fine * 0.35) - 0.5;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let mat_kind = in.mat_params.z;
    let time = scene.sun_dir_and_time.w;

    // Volumetric cathedral sunbeam shafts (additive/translucent atmospheric scattering)
    if (abs(mat_kind - 4.0) < 0.25) {
        let view_dir = normalize(scene.camera_pos.xyz - in.world_pos);
        let edge_fade = pow(1.0 - abs(dot(normalize(in.world_normal), view_dir)), 1.6);
        let height_fade = clamp(1.0 - (in.world_pos.y / 11.0), 0.05, 1.0);
        return vec4<f32>(in.color_alpha.rgb * 1.45, in.color_alpha.a * edge_fade * height_fade);
    }

    // Emissive active wildfire flame & glowing char embers
    if (abs(mat_kind - 3.0) < 0.25) {
        let flicker = 0.85 + 0.25 * sin(time * 14.0 + in.world_pos.x * 5.0 + in.world_pos.y * 7.0);
        return vec4<f32>(in.color_alpha.rgb * 2.4 * flicker, in.color_alpha.a);
    }

    var n = normalize(in.world_normal);
    let l = normalize(scene.sun_dir_and_time.xyz);
    let v = normalize(scene.camera_pos.xyz - in.world_pos);

    var base_rgb = in.color_alpha.rgb;
    var roughness = clamp(in.mat_params.x, 0.08, 0.98);
    let ao = clamp(in.mat_params.w, 0.25, 1.0);

    // Subtle procedural organic mottling on solid terrain, bark, basalt, and wildlife
    if (abs(mat_kind) < 0.25) {
        let detail = procedural_detail(in.world_pos, 2.6);
        base_rgb = clamp(base_rgb * (1.0 + detail * 0.14), vec3<f32>(0.0), vec3<f32>(1.0));
        roughness = clamp(roughness + detail * 0.08, 0.08, 0.98);
    } else if (abs(mat_kind - 1.0) < 0.25) {
        // High-frequency needle & frond cluster dapple
        let needle = procedural_detail(in.world_pos, 6.4);
        base_rgb = clamp(base_rgb * (1.0 + needle * 0.18), vec3<f32>(0.0), vec3<f32>(1.0));
    } else if (abs(mat_kind - 2.0) < 0.25) {
        // Fragment-level animated dual-wave water normal perturbation for crisp creek sparkles
        let rpx = in.world_pos.x * 8.5 + time * 3.4;
        let rpz = in.world_pos.z * 9.2 - time * 2.8;
        let ripple_x = cos(rpx + sin(rpz * 0.7)) * 0.16;
        let ripple_z = sin(rpz - cos(rpx * 0.6)) * 0.16;
        n = normalize(vec3<f32>(n.x + ripple_x, n.y, n.z + ripple_z));
    }

    let h = normalize(l + v);
    let ndotl = max(dot(n, l), 0.0);
    let shadow = sample_pcf_shadow(in.shadow_pos, ndotl);

    // Hemisphere ambient: Pacific sky zenith blue-teal + mossy forest floor bounce
    let hemi_t = n.y * 0.5 + 0.5;
    let sky_amb = vec3<f32>(0.24, 0.34, 0.40);
    let ground_amb = vec3<f32>(0.10, 0.14, 0.08);
    let ambient = mix(ground_amb, sky_amb, hemi_t) * ao;

    // GGX-inspired microfacet specular highlight
    let spec_pow = mix(96.0, 8.0, roughness * roughness);
    let spec_strength = mix(0.55, 0.04, roughness);
    let spec = pow(max(dot(n, h), 0.0), spec_pow) * spec_strength * shadow;

    var lit = base_rgb * (ambient + scene.sun_color_and_vpd.rgb * ndotl * shadow) + vec3<f32>(spec);

    // Subsurface warm needle/fern translucency when backlit by golden PNW sun
    if (abs(mat_kind - 1.0) < 0.25) {
        let back_scatter = pow(max(dot(v, -l), 0.0), 3.0) * 0.38 * shadow;
        lit = lit + base_rgb * vec3<f32>(0.98, 1.06, 0.46) * back_scatter;
    }

    // Subtle cool-sky silhouette rim lighting to define close-up trunks and wildlife
    let rim = pow(1.0 - max(dot(n, v), 0.0), 3.0) * 0.14 * ao;
    lit = lit + vec3<f32>(0.34, 0.52, 0.60) * rim;

    // Creek & beaver pond Fresnel sky reflection
    if (abs(mat_kind - 2.0) < 0.25) {
        let fresnel = pow(1.0 - max(dot(n, v), 0.0), 4.0);
        let sky_refl = vec3<f32>(0.48, 0.68, 0.80);
        lit = mix(lit, sky_refl, clamp(fresnel * 0.68, 0.08, 0.74)) + vec3<f32>(spec * 2.1);
    }

    // Atmospheric coastal valley mist
    let dist = length(scene.camera_pos.xyz - in.world_pos);
    let height_mist = exp(-max(in.world_pos.y - 0.3, 0.0) * 0.24);
    let fog_amount = clamp((1.0 - exp(-dist * scene.fog_color_and_density.w)) * (0.45 + 0.55 * height_mist), 0.0, 0.72);
    let final_rgb = mix(lit, scene.fog_color_and_density.rgb, fog_amount);

    return vec4<f32>(final_rgb, in.color_alpha.a);
}
