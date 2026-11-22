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

#![forbid(unsafe_code)]

//! Embedded procedural RGBA window icon (`ViewportBuilder::with_icon`) matching `assets/canopyforge.ico`.

use eframe::egui;

fn blend_px(buf: &mut [u8], idx: usize, r: f32, g: f32, b: f32, a: f32) {
    let alpha = a.clamp(0.0, 1.0);
    if alpha <= 0.001 {
        return;
    }
    let da = f32::from(buf[idx + 3]) / 255.0;
    let out_a = alpha + da * (1.0 - alpha);
    if out_a <= 1e-5 {
        return;
    }
    let br = f32::from(buf[idx]);
    let bg = f32::from(buf[idx + 1]);
    let bb = f32::from(buf[idx + 2]);
    buf[idx] = ((r * alpha + br * da * (1.0 - alpha)) / out_a)
        .round()
        .clamp(0.0, 255.0) as u8;
    buf[idx + 1] = ((g * alpha + bg * da * (1.0 - alpha)) / out_a)
        .round()
        .clamp(0.0, 255.0) as u8;
    buf[idx + 2] = ((b * alpha + bb * da * (1.0 - alpha)) / out_a)
        .round()
        .clamp(0.0, 255.0) as u8;
    buf[idx + 3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}

fn sd_rounded_box(px: f32, py: f32, bx: f32, by: f32, r: f32) -> f32 {
    let qx = px.abs() - bx + r;
    let qy = py.abs() - by + r;
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
}

fn point_in_triangle(
    px: f32,
    py: f32,
    ax: f32,
    ay: f32,
    bx: f32,
    by: f32,
    cx: f32,
    cy: f32,
) -> f32 {
    let edge = |x0: f32, y0: f32, x1: f32, y1: f32| (px - x0) * (y1 - y0) - (py - y0) * (x1 - x0);
    let e1 = edge(ax, ay, bx, by);
    let e2 = edge(bx, by, cx, cy);
    let e3 = edge(cx, cy, ax, ay);
    let area = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);
    let s = if area >= 0.0 { 1.0 } else { -1.0 };
    let m = (s * e1).min(s * e2).min(s * e3);
    let perim_scale = area.abs().max(1e-6).sqrt();
    m / perim_scale
}

/// Renders an anti-aliased `size x size` RGBA CanopyForge badge matching `assets/canopyforge.ico`.
pub fn render_canopyforge_rgba(size: usize) -> Vec<u8> {
    let mut buf = vec![0u8; size * size * 4];
    let aa = 2.2 / (size as f32).max(1.0);
    let border_w = 0.038;
    let trees: [(f32, f32, f32, f32, [f32; 3]); 3] = [
        (-0.32, -0.44, 0.36, 0.36, [22.0, 118.0, 74.0]),
        (0.32, -0.36, 0.38, 0.34, [16.0, 102.0, 68.0]),
        (0.0, -0.62, 0.34, 0.42, [52.0, 211.0, 153.0]),
    ];

    for py in 0..size {
        let y = ((py as f32 + 0.5) / size as f32) * 2.0 - 1.0;
        for px in 0..size {
            let x = ((px as f32 + 0.5) / size as f32) * 2.0 - 1.0;
            let idx = (py * size + px) * 4;
            let box_d = sd_rounded_box(x, y, 0.88, 0.88, 0.24);
            if box_d < aa {
                let bg_a = (0.5 - box_d / aa).clamp(0.0, 1.0);
                let grad = (y + 1.0) * 0.5;
                blend_px(
                    &mut buf,
                    idx,
                    10.0 + 10.0 * (1.0 - grad),
                    28.0 + 16.0 * (1.0 - grad),
                    22.0 + 12.0 * grad,
                    bg_a,
                );
                if box_d.abs() < border_w {
                    blend_px(
                        &mut buf,
                        idx,
                        74.0,
                        222.0,
                        128.0,
                        (1.0 - box_d.abs() / border_w) * 0.85 * bg_a,
                    );
                }
            }

            let trunk_d = sd_rounded_box(x, y - 0.36, 0.06, 0.16, 0.02);
            if trunk_d < aa {
                blend_px(
                    &mut buf,
                    idx,
                    146.0,
                    94.0,
                    56.0,
                    (0.5 - trunk_d / aa).clamp(0.0, 1.0),
                );
            }

            for &(cx, top_y, base_y, hw, col) in &trees {
                let h = base_y - top_y;
                for tier in 0..3 {
                    let t = tier as f32;
                    let t0 = top_y + t * h * 0.24;
                    let t1 = top_y + (t * 0.28 + 0.52) * h;
                    let tw = hw * (0.58 + t * 0.24);
                    let inside = point_in_triangle(x, y, cx, t0, cx - tw, t1, cx + tw, t1);
                    if inside > -aa {
                        let ta = (0.5 + inside / aa).clamp(0.0, 1.0);
                        let shade = 0.85 + 0.15 * (1.0 - (x - cx + tw) / (2.0 * tw).max(0.1));
                        blend_px(&mut buf, idx, col[0] * shade, col[1] * shade, col[2] * shade, ta);
                    }
                }
            }

            let creek_y = 0.54 + 0.08 * (x * 4.2 + 0.6).sin();
            let cd = (y - creek_y).abs();
            if x.abs() < 0.72 && cd < 0.085 + aa {
                let ca = (0.5 + (0.075 - cd) / aa).clamp(0.0, 1.0);
                blend_px(&mut buf, idx, 45.0, 212.0, 191.0, ca * 0.96);
                if cd < 0.025 {
                    blend_px(&mut buf, idx, 250.0, 204.0, 21.0, (1.0 - cd / 0.025) * 0.55);
                }
            }
        }
    }

    buf
}

/// Returns the 64x64 RGBA application icon for `ViewportBuilder::with_icon`.
pub fn app_egui_icon() -> egui::IconData {
    egui::IconData {
        rgba: render_canopyforge_rgba(64),
        width: 64,
        height: 64,
    }
}
