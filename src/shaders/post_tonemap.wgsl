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

@group(0) @binding(0)
var hdr_tex: texture_2d<f32>;

@group(0) @binding(1)
var hdr_sampler: sampler;

struct TonemapParams {
    apply_gamma: f32,
    _pad0: f32,
    _pad1: f32,
    _pad2: f32,
};

@group(0) @binding(2)
var<uniform> tonemap_params: TonemapParams;

struct FullscreenOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_tonemap(@builtin(vertex_index) vid: u32) -> FullscreenOut {
    var out: FullscreenOut;
    let x = f32((vid << 1u) & 2u);
    let y = f32(vid & 2u);
    out.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, y);
    return out;
}

fn aces_filmic(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_tonemap(in: FullscreenOut) -> @location(0) vec4<f32> {
    let hdr = textureSample(hdr_tex, hdr_sampler, in.uv).rgb;
    let mapped = aces_filmic(hdr * 1.08);
    // Gentle corner optical vignette
    let centered = in.uv - vec2<f32>(0.5, 0.5);
    let vig = clamp(1.0 - dot(centered, centered) * 0.22, 0.84, 1.0);
    let linear_out = mapped * vig;
    // Linear headless targets (Rgba8Unorm) encode 1/2.2 here; sRGB swapchains encode via hardware.
    let srgb = select(
        linear_out,
        pow(linear_out, vec3<f32>(1.0 / 2.2)),
        tonemap_params.apply_gamma > 0.5,
    );
    return vec4<f32>(srgb, 1.0);
}
