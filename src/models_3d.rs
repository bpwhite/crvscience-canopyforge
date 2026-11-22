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

use crate::agents::{
    sample_elevation_smooth, tile_world_center, CanopyLidarDrone, CrewState, StewardCrew,
    VisibleAnimalAgent, WildlifeSpecies,
};
use crate::ecology::{Ecotype, Season, TileState, CELL_PITCH, GRID_SIZE};
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::f32::consts::{PI, TAU};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MapOverlay {
    NaturalCanopy,
    BurnRiskHeatmap,
    SoilMoistureVpd,
    InvasiveCover,
    OldGrowthMaturity,
}

impl MapOverlay {
    pub const ALL: [Self; 5] = [
        Self::NaturalCanopy,
        Self::BurnRiskHeatmap,
        Self::SoilMoistureVpd,
        Self::InvasiveCover,
        Self::OldGrowthMaturity,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::NaturalCanopy => "Natural PBR Forest",
            Self::BurnRiskHeatmap => "Burn Risk Heatmap",
            Self::SoilMoistureVpd => "Soil Moisture & BDA",
            Self::InvasiveCover => "Invasive Plant Cover",
            Self::OldGrowthMaturity => "Old-Growth Maturity",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Vertex3D {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
    /// [roughness, sway_weight, mat_kind, ao]
    pub mat_params: [f32; 4],
}

impl Vertex3D {
    pub const BYTE_STRIDE: u64 = 14 * 4;

    pub fn append_le_bytes(&self, buf: &mut Vec<u8>) {
        for v in self.pos {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        for v in self.normal {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        for v in self.color {
            buf.extend_from_slice(&v.to_le_bytes());
        }
        for v in self.mat_params {
            buf.extend_from_slice(&v.to_le_bytes());
        }
    }
}

pub struct SceneMeshBuffers {
    pub opaque_bytes: Vec<u8>,
    pub opaque_vertex_count: u32,
    pub alpha_bytes: Vec<u8>,
    pub alpha_vertex_count: u32,
}

#[derive(Debug, Clone, Copy)]
struct SpineStation {
    pos: Vec3,
    half_width: f32,
    dorsal_height: f32,
    ventral_depth: f32,
    dorsal_col: [f32; 4],
    ventral_col: [f32; 4],
}

fn lerp_rgba(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    let u = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * u,
        a[1] + (b[1] - a[1]) * u,
        a[2] + (b[2] - a[2]) * u,
        a[3] + (b[3] - a[3]) * u,
    ]
}

fn shade_rgba(c: [f32; 4], factor: f32) -> [f32; 4] {
    [
        (c[0] * factor).clamp(0.0, 1.0),
        (c[1] * factor).clamp(0.0, 1.0),
        (c[2] * factor).clamp(0.0, 1.0),
        c[3],
    ]
}

struct MeshBuilder {
    verts: Vec<Vertex3D>,
}

impl MeshBuilder {
    fn with_capacity(cap: usize) -> Self {
        Self {
            verts: Vec::with_capacity(cap),
        }
    }

    fn push_tri(
        &mut self,
        a: Vec3,
        b: Vec3,
        c: Vec3,
        color: [f32; 4],
        mat_params: [f32; 4],
    ) {
        let mut n = (b - a).cross(c - a).normalize_or_zero();
        if n.length_squared() < 1e-6 {
            n = Vec3::Y;
        }
        let n_arr = [n.x, n.y, n.z];
        self.verts.push(Vertex3D {
            pos: a.to_array(),
            normal: n_arr,
            color,
            mat_params,
        });
        self.verts.push(Vertex3D {
            pos: b.to_array(),
            normal: n_arr,
            color,
            mat_params,
        });
        self.verts.push(Vertex3D {
            pos: c.to_array(),
            normal: n_arr,
            color,
            mat_params,
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn push_tri_smooth(
        &mut self,
        a: Vec3,
        b: Vec3,
        c: Vec3,
        na: Vec3,
        nb: Vec3,
        nc: Vec3,
        ca: [f32; 4],
        cb: [f32; 4],
        cc: [f32; 4],
        mpa: [f32; 4],
        mpb: [f32; 4],
        mpc: [f32; 4],
    ) {
        let fallback = (b - a).cross(c - a).normalize_or_zero();
        let fix_n = |n: Vec3| -> [f32; 3] {
            let nn = n.normalize_or_zero();
            if nn.length_squared() > 0.1 {
                nn.to_array()
            } else if fallback.length_squared() > 0.1 {
                fallback.to_array()
            } else {
                [0.0, 1.0, 0.0]
            }
        };
        self.verts.push(Vertex3D {
            pos: a.to_array(),
            normal: fix_n(na),
            color: ca,
            mat_params: mpa,
        });
        self.verts.push(Vertex3D {
            pos: b.to_array(),
            normal: fix_n(nb),
            color: cb,
            mat_params: mpb,
        });
        self.verts.push(Vertex3D {
            pos: c.to_array(),
            normal: fix_n(nc),
            color: cc,
            mat_params: mpc,
        });
    }

    fn push_quad(
        &mut self,
        a: Vec3,
        b: Vec3,
        c: Vec3,
        d: Vec3,
        color: [f32; 4],
        mat_params: [f32; 4],
    ) {
        self.push_tri(a, b, c, color, mat_params);
        self.push_tri(a, c, d, color, mat_params);
    }

    #[allow(clippy::too_many_arguments)]
    fn push_quad_smooth(
        &mut self,
        a: Vec3,
        b: Vec3,
        c: Vec3,
        d: Vec3,
        na: Vec3,
        nb: Vec3,
        nc: Vec3,
        nd: Vec3,
        ca: [f32; 4],
        cb: [f32; 4],
        cc: [f32; 4],
        cd: [f32; 4],
        mpa: [f32; 4],
        mpb: [f32; 4],
        mpc: [f32; 4],
        mpd: [f32; 4],
    ) {
        self.push_tri_smooth(a, b, c, na, nb, nc, ca, cb, cc, mpa, mpb, mpc);
        self.push_tri_smooth(a, c, d, na, nc, nd, ca, cc, cd, mpa, mpc, mpd);
    }

    fn push_box(
        &mut self,
        center: Vec3,
        half_extents: Vec3,
        yaw_rad: f32,
        color: [f32; 4],
        mat_params: [f32; 4],
    ) {
        self.push_oriented_box(
            center,
            half_extents,
            Quat::from_rotation_y(yaw_rad),
            color,
            mat_params,
        );
    }

    fn push_oriented_box(
        &mut self,
        center: Vec3,
        half_extents: Vec3,
        rot: Quat,
        color: [f32; 4],
        mat_params: [f32; 4],
    ) {
        let hx = half_extents.x;
        let hy = half_extents.y;
        let hz = half_extents.z;
        let corners = [
            Vec3::new(-hx, -hy, -hz),
            Vec3::new(hx, -hy, -hz),
            Vec3::new(hx, hy, -hz),
            Vec3::new(-hx, hy, -hz),
            Vec3::new(-hx, -hy, hz),
            Vec3::new(hx, -hy, hz),
            Vec3::new(hx, hy, hz),
            Vec3::new(-hx, hy, hz),
        ]
        .map(|p| center + rot * p);

        // +Z, -Z, +X, -X, +Y, -Y (all outward CCW)
        self.push_quad(corners[4], corners[5], corners[6], corners[7], color, mat_params);
        self.push_quad(corners[1], corners[0], corners[3], corners[2], color, mat_params);
        self.push_quad(corners[5], corners[1], corners[2], corners[6], color, mat_params);
        self.push_quad(corners[0], corners[4], corners[7], corners[3], color, mat_params);
        self.push_quad(corners[7], corners[6], corners[2], corners[3], color, mat_params);
        self.push_quad(corners[0], corners[1], corners[5], corners[4], color, mat_params);
    }

    #[allow(clippy::too_many_arguments)]
    fn push_tapered_tube(
        &mut self,
        p0: Vec3,
        p1: Vec3,
        r0: f32,
        r1: f32,
        sides: usize,
        col0: [f32; 4],
        col1: [f32; 4],
        mp0: [f32; 4],
        mp1: [f32; 4],
        cap_ends: bool,
    ) {
        let axis = p1 - p0;
        let len = axis.length();
        if len < 1e-4 {
            return;
        }
        let dir = axis / len;
        let up_ref = if dir.y.abs() < 0.92 { Vec3::Y } else { Vec3::X };
        let t = dir.cross(up_ref).normalize_or_zero();
        let b = dir.cross(t).normalize_or_zero();
        let n_sides = sides.max(3);
        let slope = (r0 - r1) / len;

        for i in 0..n_sides {
            let a0 = (i as f32 / n_sides as f32) * TAU;
            let a1 = ((i + 1) as f32 / n_sides as f32) * TAU;
            let d0 = t * a0.cos() + b * a0.sin();
            let d1 = t * a1.cos() + b * a1.sin();

            let b0 = p0 + d0 * r0;
            let b1 = p0 + d1 * r0;
            let t0 = p1 + d0 * r1;
            let t1 = p1 + d1 * r1;

            let n0 = (d0 + dir * slope).normalize_or_zero();
            let n1 = (d1 + dir * slope).normalize_or_zero();

            if r1 <= 0.002 {
                self.push_tri_smooth(b0, b1, p1, n0, n1, dir, col0, col0, col1, mp0, mp0, mp1);
            } else {
                self.push_quad_smooth(
                    b0, b1, t1, t0, n0, n1, n1, n0, col0, col0, col1, col1, mp0, mp0, mp1, mp1,
                );
            }

            if cap_ends {
                if r0 > 0.002 {
                    self.push_tri(p0, b1, b0, col0, mp0);
                }
                if r1 > 0.002 {
                    self.push_tri(p1, t0, t1, col1, mp1);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push_ellipsoid(
        &mut self,
        center: Vec3,
        radii: Vec3,
        rot: Quat,
        rings: usize,
        segs: usize,
        col_dorsal: [f32; 4],
        col_ventral: [f32; 4],
        mp: [f32; 4],
    ) {
        let r_cnt = rings.max(3);
        let s_cnt = segs.max(5);
        let inv_r = Vec3::new(
            1.0 / radii.x.max(1e-4),
            1.0 / radii.y.max(1e-4),
            1.0 / radii.z.max(1e-4),
        );
        let pt_and_n = |v: f32, u: f32| -> (Vec3, Vec3, [f32; 4]) {
            let local_dir = Vec3::new(v.sin() * u.cos(), v.cos(), v.sin() * u.sin());
            let local_pos = local_dir * radii;
            let world_pos = center + rot * local_pos;
            let n = (rot * (local_dir * inv_r)).normalize_or_zero();
            let dorsal_t = (local_dir.y * 0.5 + 0.5).clamp(0.0, 1.0);
            let col = lerp_rgba(col_ventral, col_dorsal, dorsal_t);
            (world_pos, n, col)
        };

        for r in 0..r_cnt {
            let v0 = (r as f32 / r_cnt as f32) * PI;
            let v1 = ((r + 1) as f32 / r_cnt as f32) * PI;
            for s in 0..s_cnt {
                let u0 = (s as f32 / s_cnt as f32) * TAU;
                let u1 = ((s + 1) as f32 / s_cnt as f32) * TAU;
                let (p00, n00, c00) = pt_and_n(v0, u0);
                let (p10, n10, c10) = pt_and_n(v0, u1);
                let (p11, n11, c11) = pt_and_n(v1, u1);
                let (p01, n01, c01) = pt_and_n(v1, u0);
                if r > 0 {
                    self.push_tri_smooth(p00, p10, p11, n00, n10, n11, c00, c10, c11, mp, mp, mp);
                }
                if r + 1 < r_cnt {
                    self.push_tri_smooth(p00, p11, p01, n00, n11, n01, c00, c11, c01, mp, mp, mp);
                }
            }
        }
    }

    fn push_lofted_spine(&mut self, stations: &[SpineStation], ring_verts: usize, mp: [f32; 4]) {
        if stations.len() < 2 {
            return;
        }
        let n_ring = ring_verts.max(6);
        let mut rings: Vec<Vec<(Vec3, Vec3, [f32; 4])>> = Vec::with_capacity(stations.len());
        let mut tangents: Vec<Vec3> = Vec::with_capacity(stations.len());

        for (idx, st) in stations.iter().enumerate() {
            let tangent = if idx == 0 {
                (stations[1].pos - stations[0].pos).normalize_or_zero()
            } else if idx + 1 == stations.len() {
                (stations[idx].pos - stations[idx - 1].pos).normalize_or_zero()
            } else {
                (stations[idx + 1].pos - stations[idx - 1].pos).normalize_or_zero()
            };
            let t = if tangent.length_squared() < 1e-4 {
                Vec3::Z
            } else {
                tangent
            };
            tangents.push(t);
            let up_ref = if t.y.abs() < 0.92 { Vec3::Y } else { Vec3::X };
            let lateral = t.cross(up_ref).normalize_or_zero();
            let dorsal = lateral.cross(t).normalize_or_zero();

            let mut ring = Vec::with_capacity(n_ring);
            for k in 0..n_ring {
                let ang = (k as f32 / n_ring as f32) * TAU;
                let ca = ang.cos();
                let sa = ang.sin();
                let ry = if sa >= 0.0 {
                    st.dorsal_height
                } else {
                    st.ventral_depth
                };
                let pt = st.pos
                    + lateral * (ca * st.half_width)
                    + dorsal * (sa * ry);
                let nrm = (lateral * (ca / st.half_width.max(0.01))
                    + dorsal * (sa / ry.max(0.01)))
                .normalize_or_zero();
                let blend = (sa * 0.5 + 0.5).clamp(0.0, 1.0);
                let col = lerp_rgba(st.ventral_col, st.dorsal_col, blend);
                ring.push((pt, nrm, col));
            }
            rings.push(ring);
        }

        // Side faces (CCW outward: p00 -> p10 -> p11 -> p01)
        for i in 0..(rings.len() - 1) {
            for k in 0..n_ring {
                let kn = (k + 1) % n_ring;
                let (p00, n00, c00) = rings[i][k];
                let (p01, n01, c01) = rings[i][kn];
                let (p11, n11, c11) = rings[i + 1][kn];
                let (p10, n10, c10) = rings[i + 1][k];
                self.push_quad_smooth(
                    p00, p10, p11, p01, n00, n10, n11, n01, c00, c10, c11, c01, mp, mp, mp, mp,
                );
            }
        }

        // Rounded start & end nose caps
        let first_st = stations[0];
        let first_n = -tangents[0];
        let cap_start = first_st.pos + first_n * (first_st.half_width * 0.45);
        for k in 0..n_ring {
            let kn = (k + 1) % n_ring;
            let (p0, n0, c0) = rings[0][k];
            let (p1, n1, c1) = rings[0][kn];
            self.push_tri_smooth(cap_start, p0, p1, first_n, n0, n1, first_st.dorsal_col, c0, c1, mp, mp, mp);
        }

        let last_idx = stations.len() - 1;
        let last_st = stations[last_idx];
        let last_n = tangents[last_idx];
        let cap_end = last_st.pos + last_n * (last_st.half_width * 0.45);
        for k in 0..n_ring {
            let kn = (k + 1) % n_ring;
            let (p0, n0, c0) = rings[last_idx][k];
            let (p1, n1, c1) = rings[last_idx][kn];
            self.push_tri_smooth(cap_end, p1, p0, last_n, n1, n0, last_st.dorsal_col, c1, c0, mp, mp, mp);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push_drooping_conifer_whorl(
        &mut self,
        center: Vec3,
        outer_r: f32,
        drop_h: f32,
        tip_h: f32,
        bough_count: usize,
        phase: f32,
        col_upper: [f32; 4],
        col_under: [f32; 4],
        sway: f32,
        ao: f32,
    ) {
        let count = bough_count.max(5);
        let mp_inner = [0.72, sway * 0.35, 1.0, (ao * 0.88).clamp(0.35, 1.0)];
        let mp_outer = [0.64, sway, 1.0, ao.clamp(0.45, 1.0)];
        let half_fan = (PI / count as f32) * 0.92;
        let apex = center + Vec3::new(0.0, tip_h, 0.0);

        for i in 0..count {
            let jitter = ((phase + i as f32 * 2.17).sin()) * 0.14;
            let ang = phase + (i as f32 / count as f32) * TAU + jitter;
            let len_mult = 0.88 + 0.22 * ((phase * 1.7 + i as f32 * 3.1).cos() * 0.5 + 0.5);
            let r = outer_r * len_mult;
            let droop = drop_h * (0.85 + 0.3 * len_mult);

            let tip = center + Vec3::new(ang.cos() * r, -droop, ang.sin() * r);
            let edge_r = r * 0.78;
            let edge_y = -droop * 0.58;
            let left = center
                + Vec3::new(
                    (ang + half_fan).cos() * edge_r,
                    edge_y,
                    (ang + half_fan).sin() * edge_r,
                );
            let right = center
                + Vec3::new(
                    (ang - half_fan).cos() * edge_r,
                    edge_y,
                    (ang - half_fan).sin() * edge_r,
                );
            let under_hub = center + Vec3::new(0.0, -droop * 0.45, 0.0);

            // Sunlit upper bough needles (two sloped faceted planes)
            let n_l = (left - apex).cross(tip - apex).normalize_or_zero();
            let n_r = (tip - apex).cross(right - apex).normalize_or_zero();
            let col_tip = lerp_rgba(col_upper, shade_rgba(col_upper, 1.12), 0.5);
            self.push_tri_smooth(
                apex, left, tip, n_l, n_l, n_l, col_upper, col_upper, col_tip, mp_inner, mp_outer,
                mp_outer,
            );
            self.push_tri_smooth(
                apex, tip, right, n_r, n_r, n_r, col_upper, col_tip, col_upper, mp_inner, mp_outer,
                mp_outer,
            );

            // Shaded underside of bough spray
            let n_ul = (tip - under_hub).cross(left - under_hub).normalize_or_zero();
            let n_ur = (right - under_hub).cross(tip - under_hub).normalize_or_zero();
            self.push_tri_smooth(
                under_hub, tip, left, n_ul, n_ul, n_ul, col_under, col_under, col_under, mp_inner,
                mp_outer, mp_outer,
            );
            self.push_tri_smooth(
                under_hub, right, tip, n_ur, n_ur, n_ur, col_under, col_under, col_under, mp_inner,
                mp_outer, mp_outer,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn push_frond_blade(
        &mut self,
        base: Vec3,
        dir_xz: Vec3,
        length: f32,
        width: f32,
        arch_h: f32,
        col_base: [f32; 4],
        col_tip: [f32; 4],
        sway: f32,
    ) {
        let fwd = Vec3::new(dir_xz.x, 0.0, dir_xz.z).normalize_or_zero();
        if fwd.length_squared() < 1e-4 {
            return;
        }
        let side = Vec3::new(-fwd.z, 0.0, fwd.x);
        let mid = base + fwd * (length * 0.52) + Vec3::new(0.0, arch_h, 0.0);
        let tip = base + fwd * length + Vec3::new(0.0, arch_h * 0.35, 0.0);
        let bl = base + side * (width * 0.28);
        let br = base - side * (width * 0.28);
        let ml = mid + side * (width * 0.5) + Vec3::new(0.0, -width * 0.08, 0.0);
        let mr = mid - side * (width * 0.5) + Vec3::new(0.0, -width * 0.08, 0.0);

        let col_mid = lerp_rgba(col_base, col_tip, 0.55);
        let mp0 = [0.62, sway * 0.2, 1.0, 0.80];
        let mp1 = [0.58, sway * 0.7, 1.0, 0.92];
        let mp2 = [0.54, sway, 1.0, 1.0];

        let n0 = (br - bl).cross(mid - bl).normalize_or_zero();
        self.push_quad_smooth(
            bl, br, mr, ml, n0, n0, Vec3::Y, Vec3::Y, col_base, col_base, col_mid, col_mid, mp0,
            mp0, mp1, mp1,
        );
        let n1 = (mr - ml).cross(tip - ml).normalize_or_zero();
        self.push_tri_smooth(
            ml, mr, tip, n1, n1, n1, col_mid, col_mid, col_tip, mp1, mp1, mp2,
        );
    }

    fn to_bytes(&self) -> (Vec<u8>, u32) {
        let mut out = Vec::with_capacity(self.verts.len() * (Vertex3D::BYTE_STRIDE as usize));
        for v in &self.verts {
            v.append_le_bytes(&mut out);
        }
        (out, self.verts.len() as u32)
    }
}

pub fn build_watershed_scene_meshes(
    tiles: &[TileState],
    crews: &[StewardCrew],
    drones: &[CanopyLidarDrone],
    animals: &[VisibleAnimalAgent],
    selected_tile: Option<(usize, usize)>,
    hovered_tile: Option<(usize, usize)>,
    overlay: MapOverlay,
    season: Season,
) -> SceneMeshBuffers {
    build_watershed_scene_meshes_animated(
        tiles,
        crews,
        drones,
        animals,
        selected_tile,
        hovered_tile,
        overlay,
        season,
        0.0,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn build_watershed_scene_meshes_animated(
    tiles: &[TileState],
    crews: &[StewardCrew],
    drones: &[CanopyLidarDrone],
    animals: &[VisibleAnimalAgent],
    selected_tile: Option<(usize, usize)>,
    hovered_tile: Option<(usize, usize)>,
    overlay: MapOverlay,
    season: Season,
    elapsed_sec: f32,
) -> SceneMeshBuffers {
    let mut opaque = MeshBuilder::with_capacity(140_000);
    let mut alpha = MeshBuilder::with_capacity(10_000);

    // 1. Continuous 32x32 Sculpted Terrain Heightfield & Columnar-Basalt Cliff Diorama
    let half_span = (GRID_SIZE as f32) * CELL_PITCH * 0.5;
    build_sculpted_terrain(&mut opaque, tiles, half_span, overlay, season);

    // Selection / hover 4-bar perimeter frame hugging terrain height
    for &(coords, rim_col, y_bias) in &[
        (selected_tile, [0.96, 0.82, 0.28, 1.0], 0.055),
        (hovered_tile, [0.52, 0.90, 0.72, 1.0], 0.045),
    ] {
        if let Some((tx, tz)) = coords {
            if let Some(tile) = tiles.get(tz * GRID_SIZE + tx) {
                let c = tile_world_center(tile);
                let h = CELL_PITCH * 0.485;
                let thick = 0.065;
                let rim_mp = [0.35, 0.0, 0.0, 1.0];
                let y = c.y + y_bias;
                opaque.push_box(Vec3::new(c.x, y, c.z - h), Vec3::new(h, 0.024, thick), 0.0, rim_col, rim_mp);
                opaque.push_box(Vec3::new(c.x, y, c.z + h), Vec3::new(h, 0.024, thick), 0.0, rim_col, rim_mp);
                opaque.push_box(Vec3::new(c.x - h, y, c.z), Vec3::new(thick, 0.024, h), 0.0, rim_col, rim_mp);
                opaque.push_box(Vec3::new(c.x + h, y, c.z), Vec3::new(thick, 0.024, h), 0.0, rim_col, rim_mp);
            }
        }
    }

    // 2. Per-Tile Flora, Riparian Creek, BDAs, LWD, Snags, Nurse Logs, Invasives & Wildfire
    for tile in tiles {
        let center = tile_world_center(tile);
        let hash_seed = ((tile.x * 73856093) ^ (tile.z * 19349663)) as f32;

        if tile.ecotype == Ecotype::RiparianSalmonCreek {
            build_riparian_creek_tile(&mut opaque, &mut alpha, tile, tiles, center, hash_seed);
        } else if tile.ecotype == Ecotype::UplandMeadowRidge {
            build_upland_meadow_tile(&mut opaque, tile, tiles, center, season, hash_seed);
        } else {
            build_conifer_stand_tile(&mut opaque, tile, tiles, center, hash_seed);
        }

        // Invasive Himalayan blackberry & Scotch broom
        if tile.blackberry_cover > 0.16 {
            build_blackberry_thicket(&mut opaque, tiles, center, tile.blackberry_cover, season, hash_seed);
        }
        if tile.scotch_broom_cover > 0.15 {
            build_scotch_broom(&mut opaque, tiles, center, tile.scotch_broom_cover, hash_seed);
        }

        // Volumetric cathedral sunbeam shafts piercing healthy Old-Growth Core canopies
        if tile.ecotype == Ecotype::OldGrowthCore
            && tile.old_growth_maturity > 0.78
            && (tile.x + tile.z) % 3 == 0
        {
            alpha.push_tapered_tube(
                center + Vec3::new(0.0, 0.10, 0.0),
                center + Vec3::new(0.0, 8.2, 0.0),
                1.10,
                0.36,
                8,
                [1.0, 0.92, 0.66, 0.075],
                [1.0, 0.92, 0.66, 0.04],
                [0.5, 0.0, 4.0, 1.0],
                [0.5, 0.0, 4.0, 1.0],
                false,
            );
        }

        // Multi-layered active wildfire flames & smoke puffs
        if tile.active_fire_intensity > 0.04 {
            build_wildfire_fx(
                &mut opaque,
                &mut alpha,
                center,
                tile.active_fire_intensity,
                elapsed_sec,
                hash_seed,
            );
        }
    }

    // 3. Pacific Northwest Timber Field Research Station at Tile (0, 0)
    if let Some(t0) = tiles.first() {
        build_field_research_station(&mut opaque, tiles, tile_world_center(t0));
    }

    // 4. Articulated Field Crews & Quadrotor LiDAR Drones
    for crew in crews {
        build_steward_crew(&mut opaque, crew, elapsed_sec);
    }
    for drone in drones {
        build_lidar_drone(&mut opaque, &mut alpha, drone, elapsed_sec);
    }

    // 5. Anatomically Articulated 3D PNW Wildlife Species
    for animal in animals {
        append_realistic_animal(&mut opaque, animal);
    }

    let (opaque_bytes, opaque_vertex_count) = opaque.to_bytes();
    let (alpha_bytes, alpha_vertex_count) = alpha.to_bytes();
    SceneMeshBuffers {
        opaque_bytes,
        opaque_vertex_count,
        alpha_bytes,
        alpha_vertex_count,
    }
}

fn terrain_vertex_height_and_color(
    wx: f32,
    wz: f32,
    tiles: &[TileState],
    overlay: MapOverlay,
    season: Season,
) -> (f32, [f32; 4]) {
    let base_elev = sample_elevation_smooth(tiles, wx, wz);
    let half = (GRID_SIZE as f32 - 1.0) * 0.5;
    let fx = ((wx / CELL_PITCH) + half).clamp(0.0, (GRID_SIZE - 1) as f32);
    let fz = ((wz / CELL_PITCH) + half).clamp(0.0, (GRID_SIZE - 1) as f32);
    let x0 = (fx.floor() as usize).min(GRID_SIZE - 1);
    let z0 = (fz.floor() as usize).min(GRID_SIZE - 1);
    let x1 = (x0 + 1).min(GRID_SIZE - 1);
    let z1 = (z0 + 1).min(GRID_SIZE - 1);
    let tx = (fx - x0 as f32).clamp(0.0, 1.0);
    let tz = (fz - z0 as f32).clamp(0.0, 1.0);

    let t00 = &tiles[z0 * GRID_SIZE + x0];
    let t10 = &tiles[z0 * GRID_SIZE + x1];
    let t01 = &tiles[z1 * GRID_SIZE + x0];
    let t11 = &tiles[z1 * GRID_SIZE + x1];

    let c00 = compute_tile_surface_color(t00, overlay, season);
    let c10 = compute_tile_surface_color(t10, overlay, season);
    let c01 = compute_tile_surface_color(t01, overlay, season);
    let c11 = compute_tile_surface_color(t11, overlay, season);
    let cx0 = lerp_rgba(c00, c10, tx);
    let cx1 = lerp_rgba(c01, c11, tx);
    let mut color = lerp_rgba(cx0, cx1, tz);

    let creek_w0 = if t00.ecotype == Ecotype::RiparianSalmonCreek { 1.0 - tx } else { 0.0 }
        + if t10.ecotype == Ecotype::RiparianSalmonCreek { tx } else { 0.0 };
    let creek_w1 = if t01.ecotype == Ecotype::RiparianSalmonCreek { 1.0 - tx } else { 0.0 }
        + if t11.ecotype == Ecotype::RiparianSalmonCreek { tx } else { 0.0 };
    let creek_frac = (creek_w0 * (1.0 - tz) + creek_w1 * tz).clamp(0.0, 1.0);

    let micro = (wx * 0.95).sin() * (wz * 1.15).cos() * 0.032;
    let creek_carve = creek_frac * -0.18;
    if creek_frac > 0.25 && overlay == MapOverlay::NaturalCanopy {
        // Pale river gravel & wet basalt cobble tint inside streambed
        color = lerp_rgba(color, [0.34, 0.35, 0.33, 1.0], (creek_frac - 0.25) * 0.65);
    } else if micro < -0.01 && overlay == MapOverlay::NaturalCanopy {
        color = shade_rgba(color, 0.92);
    }

    (base_elev + micro + creek_carve, color)
}

fn build_sculpted_terrain(
    opaque: &mut MeshBuilder,
    tiles: &[TileState],
    half_span: f32,
    overlay: MapOverlay,
    season: Season,
) {
    const SUBDIV: usize = GRID_SIZE * 2;
    let stride = SUBDIV + 1;
    let span = half_span * 2.0;

    let mut positions = Vec::with_capacity(stride * stride);
    let mut colors = Vec::with_capacity(stride * stride);
    for r in 0..=SUBDIV {
        let wz = -half_span + (r as f32 / SUBDIV as f32) * span;
        for c in 0..=SUBDIV {
            let wx = -half_span + (c as f32 / SUBDIV as f32) * span;
            let (wy, col) = terrain_vertex_height_and_color(wx, wz, tiles, overlay, season);
            positions.push(Vec3::new(wx, wy, wz));
            colors.push(col);
        }
    }

    let mut normals = vec![Vec3::ZERO; stride * stride];
    for r in 0..SUBDIV {
        for c in 0..SUBDIV {
            let i00 = r * stride + c;
            let i10 = i00 + 1;
            let i01 = (r + 1) * stride + c;
            let i11 = i01 + 1;

            let n1 = (positions[i01] - positions[i00]).cross(positions[i11] - positions[i00]);
            let n2 = (positions[i11] - positions[i00]).cross(positions[i10] - positions[i00]);
            normals[i00] += n1 + n2;
            normals[i01] += n1;
            normals[i11] += n1 + n2;
            normals[i10] += n2;
        }
    }
    for n in &mut normals {
        let nn = n.normalize_or_zero();
        *n = if nn.length_squared() > 0.1 { nn } else { Vec3::Y };
    }

    let terrain_mp = [0.88, 0.0, 0.0, 0.94];
    for r in 0..SUBDIV {
        for c in 0..SUBDIV {
            let i00 = r * stride + c;
            let i10 = i00 + 1;
            let i01 = (r + 1) * stride + c;
            let i11 = i01 + 1;
            opaque.push_tri_smooth(
                positions[i00],
                positions[i01],
                positions[i11],
                normals[i00],
                normals[i01],
                normals[i11],
                colors[i00],
                colors[i01],
                colors[i11],
                terrain_mp,
                terrain_mp,
                terrain_mp,
            );
            opaque.push_tri_smooth(
                positions[i00],
                positions[i11],
                positions[i10],
                normals[i00],
                normals[i11],
                normals[i10],
                colors[i00],
                colors[i11],
                colors[i10],
                terrain_mp,
                terrain_mp,
                terrain_mp,
            );
        }
    }

    // Columnar-basalt diorama cliff skirts on all 4 edges down to y = -1.9
    let base_y = -1.9_f32;
    let rock_top = [0.22, 0.23, 0.25, 1.0];
    let rock_bot = [0.12, 0.13, 0.15, 1.0];
    let rock_mp = [0.90, 0.0, 0.0, 0.78];

    let push_cliff_seg = |opaque: &mut MeshBuilder, p_a: Vec3, p_b: Vec3, idx: usize| {
        let striation = 0.90 + 0.14 * ((idx as f32) * 1.7).sin();
        let top_c = shade_rgba(rock_top, striation);
        let bot_c = shade_rgba(rock_bot, striation);
        let b_a = Vec3::new(p_a.x, base_y, p_a.z);
        let b_b = Vec3::new(p_b.x, base_y, p_b.z);
        opaque.push_quad_smooth(
            b_a, b_b, p_b, p_a, Vec3::ZERO, Vec3::ZERO, Vec3::ZERO, Vec3::ZERO, bot_c, bot_c,
            top_c, top_c, rock_mp, rock_mp, rock_mp, rock_mp,
        );
    };

    // North edge (-Z, r=0): c from SUBDIV -> 0 so outward normal is -Z
    for c in (0..SUBDIV).rev() {
        push_cliff_seg(opaque, positions[c + 1], positions[c], c);
    }
    // South edge (+Z, r=SUBDIV): c from 0 -> SUBDIV so outward normal is +Z
    let south_off = SUBDIV * stride;
    for c in 0..SUBDIV {
        push_cliff_seg(opaque, positions[south_off + c], positions[south_off + c + 1], c + 7);
    }
    // West edge (-X, c=0): r from 0 -> SUBDIV so outward normal is -X
    for r in 0..SUBDIV {
        push_cliff_seg(opaque, positions[r * stride], positions[(r + 1) * stride], r + 13);
    }
    // East edge (+X, c=SUBDIV): r from SUBDIV -> 0 so outward normal is +X
    for r in (0..SUBDIV).rev() {
        push_cliff_seg(
            opaque,
            positions[(r + 1) * stride + SUBDIV],
            positions[r * stride + SUBDIV],
            r + 23,
        );
    }

    // Closed geological bottom slab at y = -1.9
    let bl = Vec3::new(-half_span, base_y, -half_span);
    let br = Vec3::new(half_span, base_y, -half_span);
    let fr = Vec3::new(half_span, base_y, half_span);
    let fl = Vec3::new(-half_span, base_y, half_span);
    opaque.push_quad(bl, br, fr, fl, rock_bot, [0.95, 0.0, 0.0, 0.55]);
}

fn build_riparian_creek_tile(
    opaque: &mut MeshBuilder,
    alpha: &mut MeshBuilder,
    tile: &TileState,
    tiles: &[TileState],
    center: Vec3,
    hash_seed: f32,
) {
    let pool_width = CELL_PITCH * (0.36 + tile.bda_lwd_level * 0.14);
    let water_depth_tint = [
        0.07 + (1.0 - tile.bda_lwd_level) * 0.08,
        0.27 + tile.bda_lwd_level * 0.12,
        0.38 + tile.bda_lwd_level * 0.17,
        0.82,
    ];
    let wy = center.y - 0.04 + tile.bda_lwd_level * 0.11;
    let water_mp = [0.08, 0.0, 2.0, 1.0];

    // 2x2 subdivided creek water sheet for organic animated ripples
    for rz in 0..2 {
        for cx in 0..2 {
            let x0 = center.x - pool_width + (cx as f32) * pool_width;
            let x1 = x0 + pool_width;
            let z0 = center.z - pool_width + (rz as f32) * pool_width;
            let z1 = z0 + pool_width;
            alpha.push_quad(
                Vec3::new(x0, wy, z1),
                Vec3::new(x1, wy, z1),
                Vec3::new(x1, wy, z0),
                Vec3::new(x0, wy, z0),
                water_depth_tint,
                water_mp,
            );
        }
    }

    // 3 river-worn basalt cobbles & mossy boulders along creek banks
    let boulder_col = [0.34, 0.36, 0.37, 1.0];
    let mossy_top = [0.24, 0.38, 0.22, 1.0];
    for bi in 0..3 {
        let ang = hash_seed * 0.13 + bi as f32 * 2.1;
        let rad = 0.72 + 0.45 * ((bi as f32 * 1.9 + hash_seed).cos() * 0.5 + 0.5);
        let bx = center.x + ang.cos() * rad;
        let bz = center.z + ang.sin() * rad;
        let by = sample_elevation_smooth(tiles, bx, bz) - 0.04;
        let rot = Quat::from_rotation_y(ang);
        opaque.push_ellipsoid(
            Vec3::new(bx, by, bz),
            Vec3::new(0.22 + (bi as f32) * 0.05, 0.12, 0.17),
            rot,
            3,
            6,
            mossy_top,
            boulder_col,
            [0.86, 0.0, 0.0, 0.88],
        );
    }

    // Beaver Dam Analogue (BDA) & LWD rootwad cover log
    if tile.bda_lwd_level > 0.24 {
        let dam_yaw = 0.42;
        let rot = Quat::from_rotation_y(dam_yaw);
        let stake_bot_col = [0.32, 0.20, 0.11, 1.0];
        let stake_top_col = [0.46, 0.32, 0.19, 1.0];
        let stake_mp = [0.88, 0.0, 0.0, 0.86];

        // Curved arc of 7 driven willow/cedar stakes across creek
        let mut stake_bases = [Vec3::ZERO; 7];
        for (si, sb) in stake_bases.iter_mut().enumerate() {
            let t = (si as f32 / 6.0) * 2.0 - 1.0;
            let arc_z = (1.0 - t * t) * 0.22;
            let local = Vec3::new(t * 0.92, 0.0, arc_z);
            let wpos = center + rot * local;
            let wy_ground = sample_elevation_smooth(tiles, wpos.x, wpos.z) - 0.12;
            let p0 = Vec3::new(wpos.x, wy_ground, wpos.z);
            let p1 = p0 + Vec3::new(t * 0.03, 0.42, -0.02);
            opaque.push_tapered_tube(
                p0,
                p1,
                0.048,
                0.034,
                5,
                stake_bot_col,
                stake_top_col,
                stake_mp,
                stake_mp,
                true,
            );
            *sb = p0;
        }

        // 4 interwoven horizontal willow wattle branches packed between stakes
        for layer in 0..4 {
            let h = 0.08 + layer as f32 * 0.075;
            let wattle_col = if layer % 2 == 0 {
                [0.40, 0.28, 0.16, 1.0]
            } else {
                [0.30, 0.22, 0.13, 1.0]
            };
            for seg in 0..6 {
                let p0 = stake_bases[seg] + Vec3::new(0.0, h, 0.0);
                let p1 = stake_bases[seg + 1] + Vec3::new(0.0, h + 0.015, 0.0);
                opaque.push_tapered_tube(
                    p0, p1, 0.042, 0.038, 4, wattle_col, wattle_col, stake_mp, stake_mp, false,
                );
            }
        }

        // Pale translucent downstream riffle spill foam
        let foam_a = center + rot * Vec3::new(-0.85, 0.0, 0.36);
        let foam_b = center + rot * Vec3::new(0.85, 0.0, 0.36);
        let foam_y = wy + 0.015;
        let perp = rot * Vec3::new(0.0, 0.0, 0.26);
        alpha.push_quad(
            Vec3::new(foam_a.x, foam_y, foam_a.z) + perp,
            Vec3::new(foam_b.x, foam_y, foam_b.z) + perp,
            Vec3::new(foam_b.x, foam_y, foam_b.z),
            Vec3::new(foam_a.x, foam_y, foam_a.z),
            [0.72, 0.88, 0.94, 0.45],
            water_mp,
        );

        // Submerged Coho cover LWD trunk with flared rootwad fingers
        let lwd_p0 = center + Vec3::new(-0.82, -0.06, 0.62);
        let lwd_p1 = center + Vec3::new(-0.18, -0.02, 0.22);
        let lwd_col = [0.28, 0.20, 0.12, 1.0];
        let lwd_moss = [0.18, 0.34, 0.16, 1.0];
        opaque.push_tapered_tube(
            lwd_p0, lwd_p1, 0.11, 0.07, 6, lwd_col, lwd_moss, stake_mp, stake_mp, true,
        );
        for rf in 0..5 {
            let a = (rf as f32 / 5.0) * TAU + 0.3;
            let tip = lwd_p0 + Vec3::new(a.cos() * 0.24 - 0.14, 0.14 + (rf % 2) as f32 * 0.07, a.sin() * 0.24);
            opaque.push_tapered_tube(
                lwd_p0, tip, 0.045, 0.012, 4, lwd_col, lwd_col, stake_mp, stake_mp, false,
            );
        }
    }
}

fn build_upland_meadow_tile(
    opaque: &mut MeshBuilder,
    tile: &TileState,
    tiles: &[TileState],
    center: Vec3,
    season: Season,
    hash_seed: f32,
) {
    // Gnarled Oregon White Oaks (Quercus garryana) on upland meadow ridges
    if tile.canopy_biomass > 0.25 {
        let tree_count = if tile.canopy_biomass > 0.42 { 2 } else { 1 };
        let offsets = [Vec3::new(-0.58, 0.0, -0.46), Vec3::new(0.64, 0.0, 0.48)];
        let bark_col = [0.36, 0.34, 0.31, 1.0];
        let leaf_col = if season == Season::Autumn {
            [0.64, 0.40, 0.16, 1.0]
        } else {
            [0.24, 0.44, 0.18, 1.0]
        };
        let leaf_top = shade_rgba(leaf_col, 1.15);
        let bark_mp = [0.90, 0.0, 0.0, 0.85];
        let leaf_mp = [0.64, 0.35, 1.0, 0.94];

        for ti in 0..tree_count {
            let wx = center.x + offsets[ti].x + (hash_seed + ti as f32).sin() * 0.15;
            let wz = center.z + offsets[ti].z;
            let wy = sample_elevation_smooth(tiles, wx, wz) - 0.03;
            let h = 1.65 + tile.canopy_biomass * 2.1 + tile.old_growth_maturity * 1.1;
            let fork = Vec3::new(wx + 0.06, wy + h * 0.48, wz - 0.04);

            opaque.push_tapered_tube(
                Vec3::new(wx, wy, wz),
                fork,
                0.15,
                0.10,
                6,
                bark_col,
                bark_col,
                bark_mp,
                bark_mp,
                false,
            );
            for li in 0..3 {
                let la = hash_seed + (li as f32) * (TAU / 3.0) + ti as f32;
                let limb_end = fork + Vec3::new(la.cos() * 0.56, h * 0.42, la.sin() * 0.56);
                opaque.push_tapered_tube(
                    fork, limb_end, 0.08, 0.035, 5, bark_col, bark_col, bark_mp, bark_mp, false,
                );
                opaque.push_ellipsoid(
                    limb_end + Vec3::new(0.0, 0.14, 0.0),
                    Vec3::new(0.56, 0.38, 0.52),
                    Quat::from_rotation_y(la),
                    3,
                    6,
                    leaf_top,
                    shade_rgba(leaf_col, 0.72),
                    leaf_mp,
                );
            }
        }
    }

    // Dry golden upland grass tufts & native fern patches
    let grass_col = [0.58, 0.52, 0.22, 1.0];
    let tip_col = [0.76, 0.68, 0.30, 1.0];
    for g in 0..2 {
        let gx = center.x + if g == 0 { -0.65 } else { 0.42 };
        let gz = center.z + if g == 0 { 0.62 } else { -0.68 };
        let gy = sample_elevation_smooth(tiles, gx, gz) + 0.01;
        for b in 0..4 {
            let a = hash_seed + b as f32 * (TAU / 4.0) + g as f32;
            opaque.push_frond_blade(
                Vec3::new(gx, gy, gz),
                Vec3::new(a.cos(), 0.0, a.sin()),
                0.38,
                0.08,
                0.24,
                grass_col,
                tip_col,
                0.35,
            );
        }
    }

    if tile.native_understory > 0.48 {
        build_sword_fern_rosettes(opaque, tiles, center, tile.native_understory, hash_seed);
    }
}

fn build_conifer_stand_tile(
    opaque: &mut MeshBuilder,
    tile: &TileState,
    tiles: &[TileState],
    center: Vec3,
    hash_seed: f32,
) {
    let tree_count = if tile.ecotype == Ecotype::DenseSecondGrowth {
        if tile.ladder_fuels > 0.55 { 4 } else { 3 }
    } else if tile.ecotype == Ecotype::OldGrowthCore {
        3
    } else if tile.canopy_biomass > 0.30 {
        2
    } else if tile.canopy_biomass > 0.16 {
        1
    } else {
        0
    };

    let offsets = [
        Vec3::new(-0.72, 0.0, -0.65),
        Vec3::new(0.68, 0.0, 0.58),
        Vec3::new(-0.55, 0.0, 0.72),
        Vec3::new(0.64, 0.0, -0.62),
    ];

    let maturity = tile.old_growth_maturity;
    for ti in 0..tree_count {
        let jitter = (hash_seed + ti as f32 * 17.3).sin() * 0.18;
        let wx = center.x + offsets[ti].x + jitter;
        let wz = center.z + offsets[ti].z - jitter;
        let wy = sample_elevation_smooth(tiles, wx, wz) - 0.04;
        let base = Vec3::new(wx, wy, wz);

        let species_kind = (tile.x + tile.z + ti) % 3;
        let height = 1.7 + tile.canopy_biomass * 3.1 + maturity * 3.7 - (ti as f32) * 0.34;
        let trunk_r = 0.095 + maturity * 0.21;

        let (bark_bot, bark_top, col_upper, col_under) = match species_kind {
            // Douglas-fir (Pseudotsuga menziesii)
            0 => (
                [0.25, 0.17, 0.12, 1.0],
                [0.34, 0.22, 0.15, 1.0],
                [0.09, 0.32, 0.16, 1.0],
                [0.05, 0.18, 0.09, 1.0],
            ),
            // Western Redcedar (Thuja plicata)
            1 => (
                [0.33, 0.20, 0.14, 1.0],
                [0.42, 0.25, 0.16, 1.0],
                [0.12, 0.37, 0.17, 1.0],
                [0.06, 0.21, 0.10, 1.0],
            ),
            // Western Hemlock (Tsuga heterophylla)
            _ => (
                [0.28, 0.23, 0.19, 1.0],
                [0.36, 0.30, 0.25, 1.0],
                [0.14, 0.38, 0.15, 1.0],
                [0.07, 0.22, 0.09, 1.0],
            ),
        };

        // 2-segment slightly curved conifer bole
        let mid_lean = Vec3::new((hash_seed + ti as f32).cos() * 0.05, height * 0.44, 0.04);
        let top_lean = Vec3::new((hash_seed + ti as f32).cos() * 0.08, height * 0.88, 0.02);
        let p_mid = base + mid_lean;
        let p_top = base + top_lean;
        let mp_b = [0.90, 0.0, 0.0, 0.78];
        let mp_t = [0.88, 0.15, 0.0, 0.90];
        opaque.push_tapered_tube(base, p_mid, trunk_r * 1.24, trunk_r * 0.84, 6, bark_bot, bark_top, mp_b, mp_b, false);
        opaque.push_tapered_tube(p_mid, p_top, trunk_r * 0.84, trunk_r * 0.26, 5, bark_top, bark_top, mp_b, mp_t, true);

        // Buttress root flares gripping the forest floor
        let root_cnt = if species_kind == 1 { 5 } else if ti == 0 { 4 } else { 3 };
        let root_start = base + Vec3::new(0.0, (height * 0.14).clamp(0.18, 0.48), 0.0);
        for r in 0..root_cnt {
            let a = (r as f32 / root_cnt as f32) * TAU + hash_seed * 0.1 + ti as f32;
            let root_out = trunk_r * if species_kind == 1 { 2.8 } else { 2.3 };
            let r_tip = base + Vec3::new(a.cos() * root_out, -0.05, a.sin() * root_out);
            opaque.push_tapered_tube(
                root_start, r_tip, trunk_r * 0.52, trunk_r * 0.18, 4, bark_bot, bark_bot, mp_b, mp_b, false,
            );
        }

        // Dense dead lower ladder-fuel twig stubs on un-thinned high-fuel stands
        if tile.ladder_fuels > 0.55 && ti < 2 {
            let twig_col = [0.36, 0.24, 0.16, 1.0];
            for k in 0..3 {
                let th = base.y + height * (0.14 + k as f32 * 0.07);
                let ta = hash_seed + k as f32 * 2.1 + ti as f32;
                let t0 = Vec3::new(base.x, th, base.z);
                let t1 = t0 + Vec3::new(ta.cos() * 0.34, -0.08, ta.sin() * 0.34);
                opaque.push_tapered_tube(t0, t1, 0.032, 0.010, 4, twig_col, twig_col, mp_b, mp_b, false);
            }
        }

        // Climbing English ivy vine stems & glossy leaf clusters
        if tile.english_ivy_cover > 0.10 {
            let ivy_h = height * (0.20 + tile.english_ivy_cover * 0.35);
            let ivy_col = [0.09, 0.29, 0.11, 1.0];
            let ivy_mp = [0.70, 0.06, 1.0, 0.80];
            for v in 0..2 {
                let mut prev = base + Vec3::new(((v as f32) * PI).cos() * trunk_r * 1.25, 0.04, 0.0);
                for seg in 1..=3 {
                    let frac = seg as f32 / 3.0;
                    let ang = frac * 3.8 + (v as f32) * PI;
                    let r_ring = trunk_r * (1.20 - frac * 0.45);
                    let nxt = base + Vec3::new(ang.cos() * r_ring, ivy_h * frac, ang.sin() * r_ring);
                    opaque.push_tapered_tube(prev, nxt, 0.028, 0.022, 4, ivy_col, ivy_col, ivy_mp, ivy_mp, false);
                    opaque.push_ellipsoid(
                        nxt,
                        Vec3::new(0.085, 0.065, 0.085),
                        Quat::IDENTITY,
                        3,
                        5,
                        ivy_col,
                        shade_rgba(ivy_col, 0.72),
                        ivy_mp,
                    );
                    prev = nxt;
                }
            }
        }

        // 4 or 5 organic drooping conifer bough whorls
        let whorl_count = if ti < 2 { 5 } else { 4 };
        let bough_count = if ti == 0 && maturity > 0.55 { 7 } else { 6 };
        let crown_base_frac = match species_kind {
            1 => 0.24, // Redcedar sweeps lower
            _ => if tile.ladder_fuels > 0.55 { 0.20 } else { 0.36 },
        };
        for w in 0..whorl_count {
            let wf = w as f32 / (whorl_count - 1).max(1) as f32;
            let h_frac = crown_base_frac + wf * (0.94 - crown_base_frac);
            let w_pos = base + Vec3::new(mid_lean.x * h_frac, height * h_frac, mid_lean.z * h_frac);
            let wr = (0.64 + maturity * 0.56) * (1.0 - wf * 0.62);
            let drop = (0.20 + (1.0 - wf) * 0.22) * (height * 0.22).clamp(0.45, 1.45);
            let tip_h = (height * 0.14).clamp(0.28, 0.75);
            let sway = 0.22 + wf * 0.85;
            opaque.push_drooping_conifer_whorl(
                w_pos,
                wr,
                drop,
                tip_h,
                bough_count,
                hash_seed + ti as f32 * 1.9 + w as f32 * 1.15,
                col_upper,
                col_under,
                sway,
                0.76 + wf * 0.24,
            );
        }

        // Signature species crown tip (curved weeping hemlock leader vs erect fir/cedar spire)
        let tip_start = base + top_lean;
        let tip_end = if species_kind == 2 {
            tip_start + Vec3::new(0.22, height * 0.14, 0.08)
        } else {
            tip_start + Vec3::new(0.02, height * 0.15, 0.0)
        };
        opaque.push_tapered_tube(
            tip_start,
            tip_end,
            0.14,
            0.012,
            5,
            col_upper,
            shade_rgba(col_upper, 1.15),
            [0.64, 0.9, 1.0, 0.96],
            [0.62, 1.0, 1.0, 1.0],
            false,
        );
    }

    // Mossy Nurse Log with exposed heartwood ends & colonizing hemlock seedlings
    if tile.old_growth_maturity > 0.58 {
        let log_dir = Vec3::new(0.65_f32.cos(), 0.0, 0.65_f32.sin());
        let lx = center.x + 0.15;
        let lz = center.z - 0.10;
        let ly = sample_elevation_smooth(tiles, lx, lz) + 0.06;
        let p0 = Vec3::new(lx, ly, lz) - log_dir * 0.72;
        let p1 = Vec3::new(lx, ly, lz) + log_dir * 0.72;
        let log_col = [0.29, 0.21, 0.14, 1.0];
        let moss_col = [0.18, 0.36, 0.14, 1.0];
        let nurse_mp = [0.88, 0.0, 0.0, 0.80];

        opaque.push_tapered_tube(p0, p1, 0.14, 0.11, 7, log_col, log_col, nurse_mp, nurse_mp, false);
        // Exposed pale weathered heartwood end caps
        opaque.push_ellipsoid(p0, Vec3::new(0.13, 0.12, 0.05), Quat::from_rotation_y(0.65), 3, 6, [0.56, 0.44, 0.30, 1.0], [0.46, 0.35, 0.22, 1.0], nurse_mp);
        opaque.push_ellipsoid(p1, Vec3::new(0.10, 0.10, 0.04), Quat::from_rotation_y(0.65), 3, 6, [0.56, 0.44, 0.30, 1.0], [0.46, 0.35, 0.22, 1.0], nurse_mp);
        // Lush emerald moss blanket mounded on top
        opaque.push_ellipsoid(
            Vec3::new(lx, ly + 0.08, lz),
            Vec3::new(0.15, 0.08, 0.62),
            Quat::from_rotation_y(0.65),
            3,
            6,
            moss_col,
            shade_rgba(moss_col, 0.75),
            [0.85, 0.0, 0.0, 0.88],
        );
        // 2 tiny colonizing hemlock seedlings
        for s in 0..2 {
            let sp = Vec3::new(lx, ly + 0.13, lz) + log_dir * if s == 0 { -0.25 } else { 0.28 };
            opaque.push_drooping_conifer_whorl(
                sp + Vec3::new(0.0, 0.18, 0.0),
                0.20,
                0.08,
                0.18,
                5,
                s as f32 * 1.4,
                [0.16, 0.48, 0.19, 1.0],
                [0.09, 0.28, 0.11, 1.0],
                0.35,
                0.95,
            );
        }
    }

    // Legacy Wildlife Snag with splintered top, dead stubs & dark nesting cavity
    if tile.snag_cavity_density > 0.52 && tile.canopy_biomass > 0.40 {
        let sx = center.x + 0.88;
        let sz = center.z - 0.18;
        let sy = sample_elevation_smooth(tiles, sx, sz) - 0.03;
        let snag_base = Vec3::new(sx, sy, sz);
        let snag_top = snag_base + Vec3::new(0.04, 3.35, -0.02);
        let snag_col = [0.46, 0.42, 0.37, 1.0];
        let snag_top_col = [0.54, 0.50, 0.45, 1.0];
        let snag_mp = [0.92, 0.0, 0.0, 0.86];

        opaque.push_tapered_tube(snag_base, snag_top, 0.19, 0.11, 7, snag_col, snag_top_col, snag_mp, snag_mp, false);
        // Lightning-splintered jagged spire tip
        opaque.push_tapered_tube(
            snag_top,
            snag_top + Vec3::new(-0.04, 0.38, 0.03),
            0.09,
            0.015,
            4,
            snag_top_col,
            snag_top_col,
            snag_mp,
            snag_mp,
            false,
        );
        // Broken dead branch stubs
        for bi in 0..3 {
            let bh = 1.3 + bi as f32 * 0.65;
            let ba = bi as f32 * 2.1 + 0.5;
            let b0 = snag_base + Vec3::new(0.0, bh, 0.0);
            let b1 = b0 + Vec3::new(ba.cos() * 0.42, 0.12, ba.sin() * 0.42);
            opaque.push_tapered_tube(b0, b1, 0.055, 0.022, 4, snag_col, snag_top_col, snag_mp, snag_mp, true);
        }
        // Dark recessed oval nesting cavity near upper bole
        opaque.push_ellipsoid(
            snag_base + Vec3::new(0.09, 2.55, 0.08),
            Vec3::new(0.075, 0.12, 0.06),
            Quat::from_rotation_y(0.8),
            3,
            5,
            [0.05, 0.04, 0.03, 1.0],
            [0.03, 0.02, 0.02, 1.0],
            [0.95, 0.0, 0.0, 0.35],
        );
    }

    // Sword fern rosettes (Polystichum munitum)
    if tile.native_understory > 0.48 {
        build_sword_fern_rosettes(opaque, tiles, center, tile.native_understory, hash_seed);
    }
}

fn build_sword_fern_rosettes(
    opaque: &mut MeshBuilder,
    tiles: &[TileState],
    center: Vec3,
    native_understory: f32,
    hash_seed: f32,
) {
    let clump_count = if native_understory > 0.72 { 3 } else { 2 };
    let offsets = [
        Vec3::new(-0.26, 0.0, 0.32),
        Vec3::new(0.38, 0.0, -0.35),
        Vec3::new(-0.55, 0.0, -0.42),
    ];
    let col_base = [0.10, 0.32, 0.12, 1.0];
    let col_tip = [0.22, 0.54, 0.20, 1.0];

    for f in 0..clump_count {
        let fx = center.x + offsets[f].x;
        let fz = center.z + offsets[f].z;
        let fy = sample_elevation_smooth(tiles, fx, fz) + 0.01;
        let crown = Vec3::new(fx, fy, fz);
        let len = 0.28 + native_understory * 0.24;

        for b in 0..7 {
            let a = (b as f32 / 7.0) * TAU + hash_seed * 0.2 + f as f32 * 0.9;
            opaque.push_frond_blade(
                crown,
                Vec3::new(a.cos(), 0.0, a.sin()),
                len,
                0.11,
                0.14 + native_understory * 0.08,
                col_base,
                col_tip,
                0.45,
            );
        }
    }
}

fn build_blackberry_thicket(
    opaque: &mut MeshBuilder,
    tiles: &[TileState],
    center: Vec3,
    cover: f32,
    season: Season,
    hash_seed: f32,
) {
    let bx = center.x - 0.78;
    let bz = center.z + 0.76;
    let by = sample_elevation_smooth(tiles, bx, bz);
    let hub = Vec3::new(bx, by + 0.02, bz);
    let cane_col = [0.26, 0.13, 0.18, 1.0];
    let leaf_col = [0.16, 0.28, 0.14, 1.0];
    let mp_c = [0.82, 0.08, 1.0, 0.72];

    let scale = 0.45 + cover * 0.7;
    for i in 0..5 {
        let a = (i as f32 / 5.0) * TAU + hash_seed * 0.1;
        let apex = hub + Vec3::new(a.cos() * 0.22 * scale, 0.28 * scale, a.sin() * 0.22 * scale);
        let tip = hub + Vec3::new(a.cos() * 0.48 * scale, 0.04, a.sin() * 0.48 * scale);
        opaque.push_tapered_tube(hub, apex, 0.025, 0.018, 4, cane_col, cane_col, mp_c, mp_c, false);
        opaque.push_tapered_tube(apex, tip, 0.018, 0.008, 4, cane_col, cane_col, mp_c, mp_c, false);
        opaque.push_ellipsoid(
            apex,
            Vec3::new(0.16 * scale, 0.12 * scale, 0.16 * scale),
            Quat::IDENTITY,
            3,
            5,
            leaf_col,
            shade_rgba(leaf_col, 0.72),
            mp_c,
        );
        if matches!(season, Season::Spring | Season::Summer) && i % 2 == 0 {
            opaque.push_ellipsoid(
                apex + Vec3::new(0.04, 0.11 * scale, 0.0),
                Vec3::splat(0.045),
                Quat::IDENTITY,
                3,
                5,
                [0.98, 0.86, 0.90, 1.0],
                [0.92, 0.78, 0.84, 1.0],
                mp_c,
            );
        }
    }
}

fn build_scotch_broom(
    opaque: &mut MeshBuilder,
    tiles: &[TileState],
    center: Vec3,
    cover: f32,
    hash_seed: f32,
) {
    let sx = center.x + 0.75;
    let sz = center.z + 0.72;
    let sy = sample_elevation_smooth(tiles, sx, sz);
    let root = Vec3::new(sx, sy + 0.01, sz);
    let stem_col = [0.18, 0.36, 0.14, 1.0];
    let bloom_col = [0.95, 0.80, 0.14, 1.0];
    let mp = [0.62, 0.28, 1.0, 0.90];
    let h_scale = 0.55 + cover * 0.55;

    for i in 0..6 {
        let a = (i as f32 / 6.0) * TAU + hash_seed * 0.15;
        let tip = root + Vec3::new(a.cos() * 0.28 * h_scale, 0.52 * h_scale, a.sin() * 0.28 * h_scale);
        opaque.push_tapered_tube(root, tip, 0.022, 0.008, 4, stem_col, stem_col, mp, mp, false);
        opaque.push_ellipsoid(
            tip,
            Vec3::new(0.11 * h_scale, 0.13 * h_scale, 0.11 * h_scale),
            Quat::IDENTITY,
            3,
            5,
            bloom_col,
            [0.82, 0.65, 0.10, 1.0],
            mp,
        );
    }
}

fn build_wildfire_fx(
    opaque: &mut MeshBuilder,
    alpha: &mut MeshBuilder,
    center: Vec3,
    intensity: f32,
    elapsed_sec: f32,
    hash_seed: f32,
) {
    // Dark scorched ember bed
    opaque.push_ellipsoid(
        center + Vec3::new(0.0, 0.02, 0.0),
        Vec3::new(0.85, 0.06, 0.85),
        Quat::IDENTITY,
        3,
        6,
        [0.14, 0.06, 0.03, 1.0],
        [0.06, 0.04, 0.03, 1.0],
        [0.95, 0.0, 0.0, 0.4],
    );

    let fh = 1.15 + intensity * 3.2;
    let flame_mp_b = [0.3, 0.35, 3.0, 1.0];
    let flame_mp_t = [0.3, 0.95, 3.0, 1.0];

    // 3 outer crimson-orange flickering tongues
    for f in 0..3 {
        let a = (f as f32 / 3.0) * TAU + hash_seed;
        let sway_x = (elapsed_sec * 5.2 + f as f32 * 2.1).sin() * 0.14;
        let sway_z = (elapsed_sec * 4.4 + f as f32 * 1.3).cos() * 0.14;
        let f_base = center + Vec3::new(a.cos() * 0.28, 0.08, a.sin() * 0.28);
        let f_top = f_base + Vec3::new(sway_x, fh * (0.78 + 0.18 * (f as f32)), sway_z);
        alpha.push_tapered_tube(
            f_base,
            f_top,
            0.36,
            0.01,
            5,
            [1.0, 0.34, 0.06, 0.90],
            [1.0, 0.62, 0.10, 0.65],
            flame_mp_b,
            flame_mp_t,
            false,
        );
    }

    // Bright white-gold inner plasma core
    alpha.push_tapered_tube(
        center + Vec3::new(0.0, 0.10, 0.0),
        center + Vec3::new(0.0, fh * 0.65, 0.0),
        0.24,
        0.01,
        5,
        [1.0, 0.88, 0.42, 0.95],
        [1.0, 0.96, 0.72, 0.75],
        flame_mp_b,
        flame_mp_t,
        false,
    );

    // 3 rising translucent dark-grey smoke puffs
    for s in 0..3 {
        let cycle = ((elapsed_sec * 0.55 + s as f32 * 0.33 + hash_seed * 0.01) % 1.0).clamp(0.0, 1.0);
        let sy = center.y + fh * 0.75 + cycle * 2.4;
        let drift_x = cycle * 0.42 + (s as f32 - 1.0) * 0.25;
        let r = 0.28 + cycle * 0.42;
        alpha.push_ellipsoid(
            Vec3::new(center.x + drift_x, sy, center.z + (cycle * 0.22)),
            Vec3::new(r, r * 0.75, r),
            Quat::IDENTITY,
            3,
            6,
            [0.24, 0.24, 0.26, 0.38 * (1.0 - cycle * 0.7)],
            [0.16, 0.16, 0.18, 0.32 * (1.0 - cycle * 0.7)],
            [0.95, 0.25, 0.0, 0.85],
        );
    }
}

fn build_field_research_station(opaque: &mut MeshBuilder, tiles: &[TileState], st: Vec3) {
    let base_center = st + Vec3::new(-0.48, 0.0, -0.48);
    let fy = sample_elevation_smooth(tiles, base_center.x, base_center.z);
    let origin = Vec3::new(base_center.x, fy, base_center.z);

    // Basalt stone foundation footing
    opaque.push_box(
        origin + Vec3::new(0.0, 0.05, 0.0),
        Vec3::new(0.70, 0.08, 0.60),
        0.0,
        [0.24, 0.25, 0.27, 1.0],
        [0.90, 0.0, 0.0, 0.82],
    );
    // Warm cedar timber cabin walls
    opaque.push_box(
        origin + Vec3::new(0.0, 0.40, -0.05),
        Vec3::new(0.60, 0.30, 0.42),
        0.0,
        [0.54, 0.37, 0.23, 1.0],
        [0.76, 0.0, 0.0, 0.92],
    );
    // Covered front porch deck & two cedar columns
    opaque.push_box(
        origin + Vec3::new(0.0, 0.11, 0.48),
        Vec3::new(0.62, 0.04, 0.16),
        0.0,
        [0.44, 0.31, 0.20, 1.0],
        [0.82, 0.0, 0.0, 0.90],
    );
    for side in [-0.52_f32, 0.52] {
        opaque.push_tapered_tube(
            origin + Vec3::new(side, 0.14, 0.56),
            origin + Vec3::new(side, 0.72, 0.56),
            0.04,
            0.035,
            5,
            [0.46, 0.32, 0.20, 1.0],
            [0.46, 0.32, 0.20, 1.0],
            [0.78, 0.0, 0.0, 0.92],
            [0.78, 0.0, 0.0, 0.92],
            true,
        );
    }
    // Warm amber-lit cabin window panes (front + right wall)
    opaque.push_box(
        origin + Vec3::new(-0.24, 0.42, 0.38),
        Vec3::new(0.14, 0.11, 0.02),
        0.0,
        [0.98, 0.78, 0.32, 1.0],
        [0.25, 0.0, 3.0, 1.0],
    );
    opaque.push_box(
        origin + Vec3::new(0.61, 0.42, -0.05),
        Vec3::new(0.02, 0.11, 0.16),
        0.0,
        [0.98, 0.78, 0.32, 1.0],
        [0.25, 0.0, 3.0, 1.0],
    );

    // Steep pitched gable corrugated forest-green roof
    let roof_col = [0.14, 0.30, 0.24, 1.0];
    let roof_mp = [0.42, 0.0, 0.0, 0.98];
    let ridge_h = 0.98_f32;
    let eave_h = 0.68_f32;
    let eave_z = 0.66_f32;
    let slope_angle = (ridge_h - eave_h).atan2(eave_z);
    let slope_half_len = (eave_z * eave_z + (ridge_h - eave_h) * (ridge_h - eave_h)).sqrt() * 0.5;
    let mid_y = (ridge_h + eave_h) * 0.5;

    // North roof slope (-Z) and South roof slope (+Z)
    opaque.push_oriented_box(
        origin + Vec3::new(0.0, mid_y, -eave_z * 0.5 - 0.02),
        Vec3::new(0.72, 0.035, slope_half_len + 0.04),
        Quat::from_rotation_x(-slope_angle),
        roof_col,
        roof_mp,
    );
    opaque.push_oriented_box(
        origin + Vec3::new(0.0, mid_y, eave_z * 0.5 - 0.02),
        Vec3::new(0.72, 0.035, slope_half_len + 0.04),
        Quat::from_rotation_x(slope_angle),
        roof_col,
        roof_mp,
    );
    // Ridge cap beam & gable triangles
    opaque.push_box(
        origin + Vec3::new(0.0, ridge_h + 0.02, -0.02),
        Vec3::new(0.74, 0.04, 0.04),
        0.0,
        [0.10, 0.22, 0.18, 1.0],
        roof_mp,
    );
    for side_x in [-0.59_f32, 0.59] {
        let a = origin + Vec3::new(side_x, 0.70, -0.47);
        let b = origin + Vec3::new(side_x, 0.70, 0.37);
        let c = origin + Vec3::new(side_x, ridge_h, -0.02);
        if side_x < 0.0 {
            opaque.push_tri(a, c, b, [0.46, 0.31, 0.19, 1.0], [0.80, 0.0, 0.0, 0.9]);
        } else {
            opaque.push_tri(a, b, c, [0.46, 0.31, 0.19, 1.0], [0.80, 0.0, 0.0, 0.9]);
        }
    }

    // Angled photovoltaic solar panel on south roof slope
    opaque.push_oriented_box(
        origin + Vec3::new(0.12, mid_y + 0.05, 0.28),
        Vec3::new(0.32, 0.02, 0.22),
        Quat::from_rotation_x(slope_angle),
        [0.10, 0.16, 0.32, 1.0],
        [0.18, 0.0, 0.0, 1.0],
    );

    // Tall galvanized weather / VPD monitoring mast with anemometer crossarm
    let mast_base = origin + Vec3::new(0.56, 0.10, 0.52);
    let mast_top = mast_base + Vec3::new(0.0, 1.48, 0.0);
    let metal_col = [0.74, 0.78, 0.82, 1.0];
    let metal_mp = [0.30, 0.0, 0.0, 1.0];
    opaque.push_tapered_tube(mast_base, mast_top, 0.025, 0.015, 5, metal_col, metal_col, metal_mp, metal_mp, true);
    opaque.push_tapered_tube(
        mast_top - Vec3::new(0.18, 0.08, 0.0),
        mast_top + Vec3::new(0.18, -0.08, 0.0),
        0.012,
        0.012,
        4,
        metal_col,
        metal_col,
        metal_mp,
        metal_mp,
        true,
    );
    opaque.push_ellipsoid(
        mast_top + Vec3::new(0.0, 0.04, 0.0),
        Vec3::splat(0.045),
        Quat::IDENTITY,
        3,
        5,
        [0.92, 0.95, 0.98, 1.0],
        metal_col,
        metal_mp,
    );
}

fn build_steward_crew(opaque: &mut MeshBuilder, crew: &StewardCrew, elapsed_sec: f32) {
    let yaw = Quat::from_rotation_y(crew.heading_rad);
    let is_moving = matches!(
        crew.state,
        CrewState::MovingToTarget | CrewState::ReturningToStation
    );
    let stride = if is_moving {
        (elapsed_sec * 8.5 + crew.id as f32).sin() * 0.24
    } else {
        0.0
    };
    let work_swing = if crew.state == CrewState::WorkingOnSite {
        (elapsed_sec * 6.0 + crew.id as f32).sin() * 0.35
    } else {
        0.0
    };

    let vest_col = if crew.state == CrewState::WorkingOnSite {
        [0.98, 0.56, 0.10, 1.0]
    } else {
        [0.20, 0.78, 0.48, 1.0]
    };
    let pant_col = [0.18, 0.22, 0.19, 1.0];
    let boot_col = [0.12, 0.09, 0.07, 1.0];
    let pack_col = [0.28, 0.34, 0.22, 1.0];
    let skin_col = [0.82, 0.65, 0.52, 1.0];
    let hat_col = [0.96, 0.86, 0.16, 1.0];
    let mp_cloth = [0.68, 0.0, 0.0, 0.95];
    let mp_hat = [0.28, 0.0, 0.0, 1.0];

    let base = crew.pos;
    // Two 2-segment legs & boots
    for (idx, side) in [-0.075_f32, 0.075].iter().enumerate() {
        let phase = if idx == 0 { stride } else { -stride };
        let hip = base + yaw * Vec3::new(*side, 0.34, 0.0);
        let knee = base + yaw * Vec3::new(*side, 0.18, phase * 0.55 + 0.02);
        let ankle = base + yaw * Vec3::new(*side, 0.04, phase);
        opaque.push_tapered_tube(hip, knee, 0.055, 0.045, 5, pant_col, pant_col, mp_cloth, mp_cloth, false);
        opaque.push_tapered_tube(knee, ankle, 0.045, 0.036, 5, pant_col, boot_col, mp_cloth, mp_cloth, false);
        opaque.push_ellipsoid(
            ankle + yaw * Vec3::new(0.0, -0.02, 0.035),
            Vec3::new(0.045, 0.032, 0.075),
            yaw,
            3,
            5,
            boot_col,
            boot_col,
            [0.8, 0.0, 0.0, 0.9],
        );
    }

    // Torso high-vis vest & olive field backpack (-Z)
    let torso_c = base + Vec3::new(0.0, 0.49, 0.0);
    opaque.push_ellipsoid(
        torso_c,
        Vec3::new(0.145, 0.18, 0.115),
        yaw,
        4,
        6,
        vest_col,
        shade_rgba(vest_col, 0.78),
        [0.52, 0.0, 0.0, 0.98],
    );
    opaque.push_oriented_box(
        torso_c + yaw * Vec3::new(0.0, 0.01, -0.11),
        Vec3::new(0.11, 0.14, 0.065),
        yaw,
        pack_col,
        mp_cloth,
    );

    // Two articulated arms + Pulaski forestry tool on right arm (-X in +Z forward coord)
    let left_sh = torso_c + yaw * Vec3::new(0.155, 0.11, 0.0);
    let left_hand = torso_c + yaw * Vec3::new(0.165, -0.10, -stride * 0.5 + 0.06);
    opaque.push_tapered_tube(left_sh, left_hand, 0.042, 0.030, 5, vest_col, skin_col, mp_cloth, mp_cloth, true);

    let right_sh = torso_c + yaw * Vec3::new(-0.155, 0.11, 0.0);
    let right_hand = torso_c
        + yaw
            * Vec3::new(
                -0.165,
                -0.06 + work_swing * 0.25,
                0.10 + work_swing.abs() * 0.10,
            );
    opaque.push_tapered_tube(right_sh, right_hand, 0.042, 0.030, 5, vest_col, skin_col, mp_cloth, mp_cloth, true);

    // Forestry Pulaski/spade handle + steel head
    let tool_tip = right_hand + yaw * Vec3::new(-0.02, -0.26 - work_swing * 0.18, 0.34);
    opaque.push_tapered_tube(
        right_hand - yaw * Vec3::new(0.0, -0.12, 0.16),
        tool_tip,
        0.014,
        0.014,
        4,
        [0.48, 0.34, 0.18, 1.0],
        [0.48, 0.34, 0.18, 1.0],
        mp_cloth,
        mp_cloth,
        true,
    );
    opaque.push_oriented_box(
        tool_tip,
        Vec3::new(0.02, 0.065, 0.075),
        yaw,
        [0.58, 0.62, 0.66, 1.0],
        [0.25, 0.0, 0.0, 1.0],
    );

    // Head + rimmed yellow forestry hard hat
    let head_c = base + Vec3::new(0.0, 0.75, 0.0);
    opaque.push_ellipsoid(
        head_c,
        Vec3::new(0.088, 0.095, 0.092),
        yaw,
        3,
        6,
        skin_col,
        shade_rgba(skin_col, 0.85),
        mp_cloth,
    );
    opaque.push_ellipsoid(
        head_c + Vec3::new(0.0, 0.04, 0.0),
        Vec3::new(0.105, 0.075, 0.112),
        yaw,
        3,
        6,
        hat_col,
        hat_col,
        mp_hat,
    );
    opaque.push_ellipsoid(
        head_c + yaw * Vec3::new(0.0, 0.015, 0.02),
        Vec3::new(0.135, 0.014, 0.150),
        yaw,
        3,
        6,
        hat_col,
        shade_rgba(hat_col, 0.85),
        mp_hat,
    );
}

fn build_lidar_drone(
    opaque: &mut MeshBuilder,
    alpha: &mut MeshBuilder,
    drone: &CanopyLidarDrone,
    elapsed_sec: f32,
) {
    let yaw = Quat::from_rotation_y(drone.orbit_angle);
    let c = drone.pos;
    let body_col = [0.90, 0.94, 0.96, 1.0];
    let carbon_col = [0.16, 0.18, 0.21, 1.0];
    let mp_white = [0.22, 0.0, 0.0, 1.0];
    let mp_carbon = [0.42, 0.0, 0.0, 0.96];

    // Aerodynamic white/carbon avionics fuselage
    opaque.push_ellipsoid(
        c,
        Vec3::new(0.18, 0.075, 0.26),
        yaw,
        3,
        6,
        body_col,
        carbon_col,
        mp_white,
    );

    // 4 diagonal X-frame arms + motor pods + spinning translucent rotor discs
    for i in 0..4 {
        let a = (i as f32) * (PI * 0.5) + PI * 0.25;
        let arm_end = c + yaw * Vec3::new(a.cos() * 0.38, 0.02, a.sin() * 0.38);
        opaque.push_tapered_tube(
            c, arm_end, 0.022, 0.016, 4, carbon_col, carbon_col, mp_carbon, mp_carbon, false,
        );
        opaque.push_tapered_tube(
            arm_end - Vec3::new(0.0, 0.02, 0.0),
            arm_end + Vec3::new(0.0, 0.055, 0.0),
            0.036,
            0.032,
            6,
            carbon_col,
            body_col,
            mp_carbon,
            mp_white,
            true,
        );
        let shimmer = 0.28 + 0.08 * (elapsed_sec * 18.0 + i as f32).sin();
        alpha.push_ellipsoid(
            arm_end + Vec3::new(0.0, 0.06, 0.0),
            Vec3::new(0.20, 0.008, 0.20),
            Quat::IDENTITY,
            3,
            8,
            [0.72, 0.90, 0.98, shimmer],
            [0.58, 0.80, 0.92, shimmer],
            [0.15, 0.0, 0.0, 1.0],
        );
    }

    // Dual landing skids
    for side in [-0.14_f32, 0.14] {
        let skid_a = c + yaw * Vec3::new(side, -0.15, -0.22);
        let skid_b = c + yaw * Vec3::new(side, -0.15, 0.22);
        opaque.push_tapered_tube(skid_a, skid_b, 0.014, 0.014, 4, carbon_col, carbon_col, mp_carbon, mp_carbon, true);
        for z_off in [-0.11_f32, 0.11] {
            opaque.push_tapered_tube(
                c + yaw * Vec3::new(side * 0.7, -0.04, z_off),
                c + yaw * Vec3::new(side, -0.15, z_off),
                0.012,
                0.012,
                4,
                carbon_col,
                carbon_col,
                mp_carbon,
                mp_carbon,
                false,
            );
        }
    }

    // Underside gimbaled LiDAR turret + cyan lens + scanning swath cone
    let turret = c + Vec3::new(0.0, -0.10, 0.0);
    opaque.push_ellipsoid(
        turret,
        Vec3::splat(0.065),
        yaw,
        3,
        6,
        carbon_col,
        [0.18, 0.88, 0.82, 1.0],
        mp_carbon,
    );
    alpha.push_tapered_tube(
        c - Vec3::new(0.0, 2.6, 0.0),
        turret,
        0.85,
        0.07,
        6,
        [0.25, 0.88, 0.82, 0.10],
        [0.25, 0.88, 0.82, 0.18],
        [0.4, 0.0, 4.0, 1.0],
        [0.4, 0.0, 4.0, 1.0],
        false,
    );
}

fn append_realistic_animal(opaque: &mut MeshBuilder, animal: &VisibleAnimalAgent) {
    let yaw = Quat::from_rotation_y(animal.heading_rad);
    let p = animal.pos;
    let ph = animal.anim_phase;

    match animal.species {
        WildlifeSpecies::NorthernSpottedOwl => {
            let s = 1.15;
            let choc = [0.38, 0.25, 0.15, 1.0];
            let breast_spot = [0.84, 0.74, 0.58, 1.0];
            let cream_disc = [0.92, 0.86, 0.74, 1.0];
            let mp = [0.70, 0.0, 0.0, 0.96];

            // Lofted avian torso (+Z forward)
            let st = [
                SpineStation { pos: p + yaw * Vec3::new(0.0, -0.02 * s, -0.18 * s), half_width: 0.06 * s, dorsal_height: 0.05 * s, ventral_depth: 0.05 * s, dorsal_col: choc, ventral_col: breast_spot },
                SpineStation { pos: p + yaw * Vec3::new(0.0, 0.01 * s, 0.0), half_width: 0.14 * s, dorsal_height: 0.12 * s, ventral_depth: 0.11 * s, dorsal_col: choc, ventral_col: breast_spot },
                SpineStation { pos: p + yaw * Vec3::new(0.0, 0.04 * s, 0.14 * s), half_width: 0.10 * s, dorsal_height: 0.09 * s, ventral_depth: 0.08 * s, dorsal_col: choc, ventral_col: breast_spot },
            ];
            opaque.push_lofted_spine(&st, 8, mp);

            // Round owl head + twin pale facial discs + dark eyes + hooked yellow beak
            let hd = p + yaw * Vec3::new(0.0, 0.08 * s, 0.21 * s);
            opaque.push_ellipsoid(hd, Vec3::splat(0.11 * s), yaw, 4, 7, choc, cream_disc, mp);
            for side in [-0.045_f32, 0.045] {
                opaque.push_ellipsoid(
                    hd + yaw * Vec3::new(side * s, 0.01 * s, 0.088 * s),
                    Vec3::new(0.044 * s, 0.046 * s, 0.02 * s),
                    yaw,
                    3,
                    6,
                    cream_disc,
                    cream_disc,
                    mp,
                );
                opaque.push_ellipsoid(
                    hd + yaw * Vec3::new(side * s, 0.015 * s, 0.102 * s),
                    Vec3::splat(0.018 * s),
                    yaw,
                    3,
                    5,
                    [0.05, 0.04, 0.04, 1.0],
                    [0.05, 0.04, 0.04, 1.0],
                    [0.18, 0.0, 0.0, 1.0],
                );
            }
            opaque.push_tapered_tube(
                hd + yaw * Vec3::new(0.0, -0.005 * s, 0.10 * s),
                hd + yaw * Vec3::new(0.0, -0.03 * s, 0.155 * s),
                0.020 * s,
                0.004 * s,
                4,
                [0.92, 0.78, 0.24, 1.0],
                [0.72, 0.58, 0.16, 1.0],
                [0.4, 0.0, 0.0, 1.0],
                [0.4, 0.0, 0.0, 1.0],
                false,
            );

            // Barred fan tail (-Z)
            let tail_p = p + yaw * Vec3::new(0.0, -0.02 * s, -0.20 * s);
            opaque.push_oriented_box(tail_p, Vec3::new(0.09 * s, 0.015 * s, 0.10 * s), yaw, [0.46, 0.34, 0.22, 1.0], mp);

            // Articulated 2-segment flapping wings
            let flap = ph.sin() * 0.36;
            for side in [-1.0_f32, 1.0] {
                let sh = p + yaw * Vec3::new(side * 0.11 * s, 0.04 * s, 0.05 * s);
                let elbow = sh + yaw * Vec3::new(side * 0.24 * s, flap * 0.16 * s + 0.02 * s, 0.04 * s);
                let tip = elbow + yaw * Vec3::new(side * 0.26 * s, flap * 0.22 * s - 0.02 * s, -0.14 * s);
                opaque.push_tapered_tube(sh, elbow, 0.065 * s, 0.045 * s, 5, choc, [0.54, 0.40, 0.26, 1.0], mp, mp, true);
                opaque.push_tapered_tube(elbow, tip, 0.045 * s, 0.015 * s, 4, [0.54, 0.40, 0.26, 1.0], breast_spot, mp, mp, false);
            }
        }

        WildlifeSpecies::OregonCoastCohoSalmon => {
            let s = 1.1;
            let green_dorsal = [0.16, 0.36, 0.24, 1.0];
            let crimson = [0.84, 0.18, 0.22, 1.0];
            let silver_belly = [0.78, 0.80, 0.82, 1.0];
            let mp = [0.24, 0.0, 0.0, 0.98];
            let wag1 = ph.sin() * 0.055 * s;
            let wag2 = (ph + 0.9).sin() * 0.085 * s;

            let st = [
                SpineStation { pos: p + yaw * Vec3::new(wag2, 0.0, -0.34 * s), half_width: 0.022 * s, dorsal_height: 0.045 * s, ventral_depth: 0.042 * s, dorsal_col: green_dorsal, ventral_col: crimson },
                SpineStation { pos: p + yaw * Vec3::new(wag1 * 0.6, 0.0, -0.16 * s), half_width: 0.055 * s, dorsal_height: 0.09 * s, ventral_depth: 0.08 * s, dorsal_col: crimson, ventral_col: silver_belly },
                SpineStation { pos: p, half_width: 0.075 * s, dorsal_height: 0.12 * s, ventral_depth: 0.105 * s, dorsal_col: crimson, ventral_col: silver_belly },
                SpineStation { pos: p + yaw * Vec3::new(-wag1 * 0.2, 0.005 * s, 0.16 * s), half_width: 0.068 * s, dorsal_height: 0.10 * s, ventral_depth: 0.09 * s, dorsal_col: green_dorsal, ventral_col: silver_belly },
                SpineStation { pos: p + yaw * Vec3::new(0.0, 0.0, 0.28 * s), half_width: 0.042 * s, dorsal_height: 0.065 * s, ventral_depth: 0.055 * s, dorsal_col: green_dorsal, ventral_col: green_dorsal },
            ];
            opaque.push_lofted_spine(&st, 8, mp);

            // Spawning kype jaw
            opaque.push_tapered_tube(
                p + yaw * Vec3::new(0.0, -0.015 * s, 0.27 * s),
                p + yaw * Vec3::new(0.0, 0.01 * s, 0.36 * s),
                0.024 * s,
                0.008 * s,
                4,
                green_dorsal,
                [0.24, 0.44, 0.30, 1.0],
                mp,
                mp,
                false,
            );
            // Dorsal fin & tiny adipose fin
            let df = p + yaw * Vec3::new(0.0, 0.10 * s, -0.02 * s);
            opaque.push_tri(
                df + yaw * Vec3::new(0.0, 0.0, 0.06 * s),
                df + yaw * Vec3::new(0.0, 0.0, -0.07 * s),
                df + yaw * Vec3::new(0.0, 0.11 * s, -0.04 * s),
                [0.42, 0.20, 0.18, 1.0],
                mp,
            );
            opaque.push_tri(
                p + yaw * Vec3::new(0.0, 0.075 * s, -0.17 * s),
                p + yaw * Vec3::new(0.0, 0.07 * s, -0.22 * s),
                p + yaw * Vec3::new(0.0, 0.11 * s, -0.21 * s),
                crimson,
                mp,
            );
            // Paired pectoral fins
            for side in [-1.0_f32, 1.0] {
                let pf0 = p + yaw * Vec3::new(side * 0.065 * s, -0.04 * s, 0.14 * s);
                let pf1 = pf0 + yaw * Vec3::new(side * 0.08 * s, -0.02 * s, -0.09 * s);
                opaque.push_tapered_tube(pf0, pf1, 0.026 * s, 0.008 * s, 4, silver_belly, crimson, mp, mp, false);
            }
            // Broad forked caudal tail fin
            let tail_root = st[0].pos;
            for v_dir in [-1.0_f32, 1.0] {
                opaque.push_tri(
                    tail_root,
                    tail_root + yaw * Vec3::new(wag2 * 0.6, v_dir * 0.11 * s, -0.14 * s),
                    tail_root + yaw * Vec3::new(wag2 * 0.3, v_dir * 0.02 * s, -0.08 * s),
                    green_dorsal,
                    mp,
                );
            }
        }

        WildlifeSpecies::NorthAmericanBeaver => {
            let s = 1.15;
            let chestnut = [0.38, 0.23, 0.14, 1.0];
            let guard_dark = [0.24, 0.15, 0.09, 1.0];
            let slate_tail = [0.16, 0.15, 0.16, 1.0];
            let mp = [0.68, 0.0, 0.0, 0.95];

            let st = [
                SpineStation { pos: p + yaw * Vec3::new(0.0, 0.08 * s, -0.22 * s), half_width: 0.11 * s, dorsal_height: 0.09 * s, ventral_depth: 0.07 * s, dorsal_col: guard_dark, ventral_col: chestnut },
                SpineStation { pos: p + yaw * Vec3::new(0.0, 0.11 * s, -0.03 * s), half_width: 0.17 * s, dorsal_height: 0.14 * s, ventral_depth: 0.10 * s, dorsal_col: guard_dark, ventral_col: chestnut },
                SpineStation { pos: p + yaw * Vec3::new(0.0, 0.10 * s, 0.16 * s), half_width: 0.13 * s, dorsal_height: 0.11 * s, ventral_depth: 0.09 * s, dorsal_col: guard_dark, ventral_col: chestnut },
            ];
            opaque.push_lofted_spine(&st, 8, mp);

            // Blunt head, small rounded ears & amber incisors
            let hd = p + yaw * Vec3::new(0.0, 0.11 * s, 0.26 * s);
            opaque.push_ellipsoid(hd, Vec3::new(0.105 * s, 0.09 * s, 0.11 * s), yaw, 4, 6, guard_dark, chestnut, mp);
            for side in [-0.07_f32, 0.07] {
                opaque.push_ellipsoid(
                    hd + yaw * Vec3::new(side * s, 0.07 * s, -0.03 * s),
                    Vec3::splat(0.028 * s),
                    yaw,
                    3,
                    5,
                    guard_dark,
                    chestnut,
                    mp,
                );
            }
            opaque.push_oriented_box(
                hd + yaw * Vec3::new(0.0, -0.045 * s, 0.105 * s),
                Vec3::new(0.018 * s, 0.022 * s, 0.01 * s),
                yaw,
                [0.92, 0.48, 0.12, 1.0],
                [0.35, 0.0, 0.0, 1.0],
            );

            // 4 short paddling/walking limbs
            for (li, (lx, lz)) in [(-0.11_f32, -0.14), (0.11, -0.14), (-0.10, 0.12), (0.10, 0.12)].iter().enumerate() {
                let step = (ph + li as f32 * 1.6).sin() * 0.04 * s;
                let hip = p + yaw * Vec3::new(*lx * s, 0.07 * s, *lz * s);
                let foot = p + yaw * Vec3::new(*lx * 1.15 * s, 0.0, *lz * s + step);
                opaque.push_tapered_tube(hip, foot, 0.045 * s, 0.032 * s, 5, guard_dark, slate_tail, mp, mp, true);
            }

            // Iconic broad flattened scaly paddle tail (-Z)
            let tail_sway = ph.sin() * 0.03 * s;
            opaque.push_ellipsoid(
                p + yaw * Vec3::new(tail_sway, 0.05 * s, -0.40 * s),
                Vec3::new(0.115 * s, 0.022 * s, 0.21 * s),
                yaw,
                3,
                7,
                slate_tail,
                [0.10, 0.10, 0.11, 1.0],
                [0.52, 0.0, 0.0, 0.96],
            );
        }

        WildlifeSpecies::RooseveltElk => {
            let s = 1.15;
            let buff_body = [0.64, 0.48, 0.30, 1.0];
            let dark_mane = [0.24, 0.16, 0.11, 1.0];
            let rump_cream = [0.88, 0.80, 0.64, 1.0];
            let mp = [0.68, 0.0, 0.0, 0.96];

            let torso_y = 0.46 * s;
            let st = [
                SpineStation { pos: p + yaw * Vec3::new(0.0, torso_y, -0.36 * s), half_width: 0.14 * s, dorsal_height: 0.16 * s, ventral_depth: 0.14 * s, dorsal_col: buff_body, ventral_col: rump_cream },
                SpineStation { pos: p + yaw * Vec3::new(0.0, torso_y - 0.01 * s, -0.05 * s), half_width: 0.18 * s, dorsal_height: 0.20 * s, ventral_depth: 0.19 * s, dorsal_col: buff_body, ventral_col: shade_rgba(buff_body, 0.78) },
                SpineStation { pos: p + yaw * Vec3::new(0.0, torso_y + 0.03 * s, 0.26 * s), half_width: 0.16 * s, dorsal_height: 0.21 * s, ventral_depth: 0.18 * s, dorsal_col: dark_mane, ventral_col: dark_mane },
            ];
            opaque.push_lofted_spine(&st, 8, mp);

            // Creamy-tan rump patch & short tail
            opaque.push_ellipsoid(
                p + yaw * Vec3::new(0.0, torso_y + 0.02 * s, -0.40 * s),
                Vec3::new(0.11 * s, 0.11 * s, 0.08 * s),
                yaw,
                3,
                5,
                rump_cream,
                rump_cream,
                mp,
            );

            // 4 articulated 2-segment stride legs + dark hooves
            for (li, (lx, lz)) in [(-0.11_f32, -0.26), (0.11, -0.26), (-0.11, 0.22), (0.11, 0.22)].iter().enumerate() {
                let phase = if li == 0 || li == 3 { ph.sin() } else { -ph.sin() } * 0.12 * s;
                let hip = p + yaw * Vec3::new(*lx * s, torso_y - 0.08 * s, *lz * s);
                let knee = p + yaw * Vec3::new(*lx * s, 0.22 * s, *lz * s + phase * 0.55);
                let hoof = p + yaw * Vec3::new(*lx * s, 0.02 * s, *lz * s + phase);
                opaque.push_tapered_tube(hip, knee, 0.062 * s, 0.042 * s, 5, buff_body, buff_body, mp, mp, false);
                opaque.push_tapered_tube(knee, hoof, 0.038 * s, 0.026 * s, 5, dark_mane, [0.12, 0.10, 0.09, 1.0], mp, mp, true);
            }

            // Upright shaggy neck, tapered head, dark muzzle & pointed ears
            let neck_base = p + yaw * Vec3::new(0.0, torso_y + 0.08 * s, 0.26 * s);
            let head_base = p + yaw * Vec3::new(0.0, 0.80 * s, 0.42 * s);
            opaque.push_tapered_tube(neck_base, head_base, 0.145 * s, 0.095 * s, 6, dark_mane, dark_mane, mp, mp, false);
            opaque.push_ellipsoid(
                head_base + yaw * Vec3::new(0.0, 0.02 * s, 0.09 * s),
                Vec3::new(0.078 * s, 0.076 * s, 0.145 * s),
                yaw,
                4,
                6,
                dark_mane,
                buff_body,
                mp,
            );
            for side in [-0.07_f32, 0.07] {
                opaque.push_tapered_tube(
                    head_base + yaw * Vec3::new(side * s, 0.06 * s, -0.02 * s),
                    head_base + yaw * Vec3::new(side * 1.8 * s, 0.16 * s, -0.07 * s),
                    0.026 * s,
                    0.008 * s,
                    4,
                    buff_body,
                    dark_mane,
                    mp,
                    mp,
                    false,
                );
            }

            // Sweeping branched multi-tine ivory bull antlers on bulls (even IDs)
            if animal.id % 2 == 0 {
                let ant_col = [0.88, 0.82, 0.68, 1.0];
                let ant_mp = [0.50, 0.0, 0.0, 1.0];
                for side in [-1.0_f32, 1.0] {
                    let root = head_base + yaw * Vec3::new(side * 0.045 * s, 0.08 * s, -0.01 * s);
                    let mid = root + yaw * Vec3::new(side * 0.18 * s, 0.16 * s, -0.14 * s);
                    let tip = mid + yaw * Vec3::new(side * 0.08 * s, 0.18 * s, -0.16 * s);
                    opaque.push_tapered_tube(root, mid, 0.024 * s, 0.018 * s, 4, ant_col, ant_col, ant_mp, ant_mp, false);
                    opaque.push_tapered_tube(mid, tip, 0.018 * s, 0.006 * s, 4, ant_col, ant_col, ant_mp, ant_mp, false);
                    // Brow & trez forward tines + upper crown tine
                    opaque.push_tapered_tube(
                        root + yaw * Vec3::new(side * 0.03 * s, 0.03 * s, -0.02 * s),
                        root + yaw * Vec3::new(side * 0.08 * s, 0.11 * s, 0.14 * s),
                        0.014 * s,
                        0.004 * s,
                        4,
                        ant_col,
                        ant_col,
                        ant_mp,
                        ant_mp,
                        false,
                    );
                    opaque.push_tapered_tube(
                        mid,
                        mid + yaw * Vec3::new(side * 0.05 * s, 0.16 * s, 0.06 * s),
                        0.013 * s,
                        0.004 * s,
                        4,
                        ant_col,
                        ant_col,
                        ant_mp,
                        ant_mp,
                        false,
                    );
                }
            }
        }

        WildlifeSpecies::MarbledMurrelet => {
            let s = 1.05;
            let umber = [0.32, 0.25, 0.20, 1.0];
            let buff_marble = [0.68, 0.58, 0.46, 1.0];
            let mp = [0.62, 0.0, 0.0, 0.98];

            let st = [
                SpineStation { pos: p + yaw * Vec3::new(0.0, 0.0, -0.18 * s), half_width: 0.04 * s, dorsal_height: 0.035 * s, ventral_depth: 0.03 * s, dorsal_col: umber, ventral_col: buff_marble },
                SpineStation { pos: p, half_width: 0.085 * s, dorsal_height: 0.07 * s, ventral_depth: 0.06 * s, dorsal_col: umber, ventral_col: buff_marble },
                SpineStation { pos: p + yaw * Vec3::new(0.0, 0.02 * s, 0.15 * s), half_width: 0.055 * s, dorsal_height: 0.055 * s, ventral_depth: 0.045 * s, dorsal_col: umber, ventral_col: buff_marble },
            ];
            opaque.push_lofted_spine(&st, 8, mp);
            // Slender pointed dark bill
            opaque.push_tapered_tube(
                p + yaw * Vec3::new(0.0, 0.02 * s, 0.18 * s),
                p + yaw * Vec3::new(0.0, 0.015 * s, 0.29 * s),
                0.018 * s,
                0.003 * s,
                4,
                [0.14, 0.14, 0.16, 1.0],
                [0.14, 0.14, 0.16, 1.0],
                mp,
                mp,
                false,
            );
            // Rapid-beating narrow swept wings
            let flap = (ph * 2.2).sin() * 0.26;
            for side in [-1.0_f32, 1.0] {
                let sh = p + yaw * Vec3::new(side * 0.07 * s, 0.02 * s, 0.03 * s);
                let tip = sh + yaw * Vec3::new(side * 0.34 * s, flap * 0.18 * s, -0.14 * s);
                opaque.push_tapered_tube(sh, tip, 0.055 * s, 0.012 * s, 4, umber, buff_marble, mp, mp, false);
            }
        }

        WildlifeSpecies::NorthernFlyingSquirrel => {
            let s = 1.1;
            let cinn_grey = [0.56, 0.46, 0.36, 1.0];
            let cream_belly = [0.92, 0.88, 0.80, 1.0];
            let mp = [0.72, 0.0, 0.0, 0.98];
            let bank = Quat::from_rotation_z(ph.sin() * 0.14);
            let rot = yaw * bank;

            // Lofted slender dorsal body + creamy belly
            let st = [
                SpineStation { pos: p + rot * Vec3::new(0.0, 0.0, -0.14 * s), half_width: 0.05 * s, dorsal_height: 0.04 * s, ventral_depth: 0.03 * s, dorsal_col: cinn_grey, ventral_col: cream_belly },
                SpineStation { pos: p, half_width: 0.07 * s, dorsal_height: 0.055 * s, ventral_depth: 0.04 * s, dorsal_col: cinn_grey, ventral_col: cream_belly },
                SpineStation { pos: p + rot * Vec3::new(0.0, 0.01 * s, 0.14 * s), half_width: 0.055 * s, dorsal_height: 0.045 * s, ventral_depth: 0.035 * s, dorsal_col: cinn_grey, ventral_col: cream_belly },
            ];
            opaque.push_lofted_spine(&st, 8, mp);

            // Round head + large glossy dark nocturnal eyes
            let hd = p + rot * Vec3::new(0.0, 0.025 * s, 0.18 * s);
            opaque.push_ellipsoid(hd, Vec3::new(0.052 * s, 0.048 * s, 0.06 * s), rot, 3, 6, cinn_grey, cream_belly, mp);
            for side in [-0.035_f32, 0.035] {
                opaque.push_ellipsoid(
                    hd + rot * Vec3::new(side * s, 0.015 * s, 0.038 * s),
                    Vec3::splat(0.016 * s),
                    rot,
                    3,
                    5,
                    [0.04, 0.04, 0.05, 1.0],
                    [0.04, 0.04, 0.05, 1.0],
                    [0.12, 0.0, 0.0, 1.0],
                );
            }

            // Taut outstretched furry patagium gliding membrane + tiny limbs
            opaque.push_oriented_box(
                p + rot * Vec3::new(0.0, -0.005 * s, 0.0),
                Vec3::new(0.22 * s, 0.012 * s, 0.14 * s),
                rot,
                cinn_grey,
                mp,
            );
            for (px, pz) in [(-0.21_f32, 0.12), (0.21, 0.12), (-0.19, -0.12), (0.19, -0.12)] {
                opaque.push_ellipsoid(
                    p + rot * Vec3::new(px * s, 0.0, pz * s),
                    Vec3::splat(0.024 * s),
                    rot,
                    3,
                    5,
                    cream_belly,
                    cinn_grey,
                    mp,
                );
            }

            // Long broad flattened feathery gliding tail (-Z)
            opaque.push_ellipsoid(
                p + rot * Vec3::new(0.0, 0.01 * s, -0.32 * s),
                Vec3::new(0.068 * s, 0.016 * s, 0.18 * s),
                rot,
                3,
                6,
                cinn_grey,
                shade_rgba(cinn_grey, 0.78),
                mp,
            );
        }
    }
}

fn compute_tile_surface_color(tile: &TileState, overlay: MapOverlay, season: Season) -> [f32; 4] {
    match overlay {
        MapOverlay::NaturalCanopy => {
            if tile.recent_burn_scar > 0.15 {
                return [0.16, 0.14, 0.13, 1.0];
            }
            let autumn_warmth = if season == Season::Autumn { 0.05 } else { 0.0 };
            match tile.ecotype {
                Ecotype::OldGrowthCore => [0.11, 0.26, 0.13, 1.0],
                Ecotype::DenseSecondGrowth => [0.15, 0.24, 0.14, 1.0],
                Ecotype::RiparianSalmonCreek => [0.22, 0.28, 0.20, 1.0],
                Ecotype::DegradedClearcutEdge => {
                    let green_rec = tile.native_understory * 0.18;
                    [0.34 - green_rec * 0.4 + autumn_warmth, 0.26 + green_rec, 0.17, 1.0]
                }
                Ecotype::UplandMeadowRidge => [0.28 + autumn_warmth, 0.33, 0.16, 1.0],
            }
        }
        MapOverlay::BurnRiskHeatmap => {
            let r = (tile.burn_risk * 1.35).clamp(0.12, 0.96);
            let g = (1.0 - tile.burn_risk * 1.1).clamp(0.10, 0.78);
            [r, g, 0.14, 1.0]
        }
        MapOverlay::SoilMoistureVpd => {
            let m = tile.soil_moisture.clamp(0.0, 1.0);
            [0.45 * (1.0 - m), 0.30 + m * 0.35, 0.18 + m * 0.72, 1.0]
        }
        MapOverlay::InvasiveCover => {
            let inv = tile.invasive_cover.clamp(0.0, 1.0);
            [0.18 + inv * 0.75, 0.42 * (1.0 - inv * 0.7), 0.20 + inv * 0.48, 1.0]
        }
        MapOverlay::OldGrowthMaturity => {
            let og = tile.old_growth_maturity.clamp(0.0, 1.0);
            [0.24 * (1.0 - og * 0.6), 0.18 + og * 0.58, 0.14 + og * 0.24, 1.0]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{
        build_initial_crews, initial_species_populations, sync_and_step_visible_animals,
    };
    use crate::ecology::build_initial_watershed;
    use rand::SeedableRng;
    use rand_chacha::ChaCha8Rng;

    #[test]
    fn realistic_scene_meshes_are_finite_and_populated() {
        let mut rng = ChaCha8Rng::seed_from_u64(2026);
        let tiles = build_initial_watershed(&mut rng);
        let pops = initial_species_populations(&tiles);
        let (crews, drones) = build_initial_crews(&tiles);
        let mut animals = Vec::new();
        sync_and_step_visible_animals(&mut animals, &pops, &tiles, 0.05, &mut rng);

        let bufs = build_watershed_scene_meshes_animated(
            &tiles,
            &crews,
            &drones,
            &animals,
            Some((4, 4)),
            Some((5, 5)),
            MapOverlay::NaturalCanopy,
            Season::Spring,
            1.25,
        );
        assert!(bufs.opaque_vertex_count > 30_000);
        assert!(bufs.alpha_vertex_count > 500);
        assert_eq!(
            bufs.opaque_bytes.len(),
            (bufs.opaque_vertex_count as usize) * (Vertex3D::BYTE_STRIDE as usize)
        );
        assert_eq!(
            bufs.alpha_bytes.len(),
            (bufs.alpha_vertex_count as usize) * (Vertex3D::BYTE_STRIDE as usize)
        );
        for chunk in bufs.opaque_bytes.chunks_exact(4) {
            let val = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            assert!(val.is_finite(), "Non-finite float in opaque mesh buffer");
        }
        for chunk in bufs.alpha_bytes.chunks_exact(4) {
            let val = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            assert!(val.is_finite(), "Non-finite float in alpha mesh buffer");
        }
    }

    #[test]
    fn all_six_wildlife_species_generate_articulated_multi_part_geometry() {
        for (idx, species) in WildlifeSpecies::ALL.into_iter().enumerate() {
            let agent = VisibleAnimalAgent {
                id: (idx as u32) * 2,
                species,
                pos: Vec3::ZERO,
                target_pos: Vec3::Z,
                heading_rad: 0.4,
                anim_phase: 0.7,
                speed: 2.0,
            };
            let mut mb = MeshBuilder::with_capacity(1024);
            append_realistic_animal(&mut mb, &agent);
            assert!(
                mb.verts.len() >= 90,
                "Species {:?} generated only {} vertices; expected >= 90",
                species,
                mb.verts.len()
            );
            for v in &mb.verts {
                assert!(
                    v.pos.iter().all(|x| x.is_finite()) && v.normal.iter().all(|x| x.is_finite()),
                    "Non-finite vertex in {:?}",
                    species
                );
            }
        }
    }
}
