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

use crate::agents::tile_world_center;
use crate::ecology::{TileState, CELL_PITCH, GRID_SIZE};
use glam::{Mat4, Vec2, Vec3, Vec4};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CameraPreset {
    /// Main default view: zoomed-out 3D isometric tabletop diorama of the full 16x16 watershed
    FullWatershedIsometric,
    /// 3x zoom into mid-canopy trunks, sword ferns, nurse logs, and active restoration crews
    MidCanopyStandZoom,
    /// 5x close-up along the Coho salmon & Beaver Dam Analogue stream corridor
    SalmonAndBeaverCreekZoom,
    /// 6x close-up tracking active Spotted Owls, Roosevelt Elk, or Gliding Flying Squirrels
    FollowWildlifeUnderstoryCam,
}

impl CameraPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::FullWatershedIsometric => "1. Full Watershed Iso",
            Self::MidCanopyStandZoom => "2. Mid-Canopy Stand",
            Self::SalmonAndBeaverCreekZoom => "3. Creek & BDA Pool",
            Self::FollowWildlifeUnderstoryCam => "4. Follow Wildlife",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsometricZoomCamera {
    pub preset: CameraPreset,
    pub target: Vec3,
    pub desired_target: Vec3,
    pub yaw_rad: f32,
    pub desired_yaw_rad: f32,
    pub pitch_rad: f32,
    pub desired_pitch_rad: f32,
    pub distance: f32,
    pub desired_distance: f32,
    /// Narrow telephoto FOV at zoomed-out isometric preset, widening slightly on intimate understory zoom
    pub fov_y_rad: f32,
}

impl Default for IsometricZoomCamera {
    fn default() -> Self {
        let iso_pitch = 35.264_39_f32.to_radians();
        let iso_yaw = std::f32::consts::FRAC_PI_4;
        Self {
            preset: CameraPreset::FullWatershedIsometric,
            target: Vec3::new(0.0, 0.9, 0.0),
            desired_target: Vec3::new(0.0, 0.9, 0.0),
            yaw_rad: iso_yaw,
            desired_yaw_rad: iso_yaw,
            pitch_rad: iso_pitch,
            desired_pitch_rad: iso_pitch,
            distance: 66.0,
            desired_distance: 66.0,
            fov_y_rad: 28.0_f32.to_radians(),
        }
    }
}

impl IsometricZoomCamera {
    pub fn apply_preset(&mut self, preset: CameraPreset, focus_hint: Option<Vec3>) {
        self.preset = preset;
        match preset {
            CameraPreset::FullWatershedIsometric => {
                self.desired_target = Vec3::new(0.0, 0.9, 0.0);
                self.desired_yaw_rad = std::f32::consts::FRAC_PI_4;
                self.desired_pitch_rad = 35.264_39_f32.to_radians();
                self.desired_distance = 66.0;
            }
            CameraPreset::MidCanopyStandZoom => {
                self.desired_target = focus_hint.unwrap_or(Vec3::new(-6.0, 1.4, -5.0));
                self.desired_pitch_rad = 33.0_f32.to_radians();
                self.desired_distance = 24.0;
            }
            CameraPreset::SalmonAndBeaverCreekZoom => {
                self.desired_target = focus_hint.unwrap_or(Vec3::new(1.5, 0.6, 3.5));
                self.desired_pitch_rad = 27.0_f32.to_radians();
                self.desired_distance = 14.0;
            }
            CameraPreset::FollowWildlifeUnderstoryCam => {
                if let Some(pos) = focus_hint {
                    self.desired_target = pos;
                }
                self.desired_pitch_rad = 21.0_f32.to_radians();
                self.desired_distance = 10.5;
            }
        }
    }

    pub fn zoom_by(&mut self, scroll_delta: f32) {
        let factor = (1.0 - scroll_delta * 0.0018).clamp(0.75, 1.30);
        self.desired_distance = (self.desired_distance * factor).clamp(8.5, 82.0);
    }

    pub fn orbit_by(&mut self, delta_yaw: f32, delta_pitch: f32) {
        self.desired_yaw_rad += delta_yaw;
        self.desired_pitch_rad =
            (self.desired_pitch_rad + delta_pitch).clamp(14.0_f32.to_radians(), 68.0_f32.to_radians());
    }

    pub fn pan_planar(&mut self, right_delta: f32, forward_delta: f32) {
        let scale = (self.distance / 50.0).clamp(0.18, 1.4);
        let forward = Vec3::new(-self.yaw_rad.sin(), 0.0, -self.yaw_rad.cos()).normalize_or_zero();
        let right = Vec3::new(self.yaw_rad.cos(), 0.0, -self.yaw_rad.sin()).normalize_or_zero();
        let bound = (GRID_SIZE as f32) * CELL_PITCH * 0.52;
        self.desired_target += (right * right_delta + forward * forward_delta) * scale;
        self.desired_target.x = self.desired_target.x.clamp(-bound, bound);
        self.desired_target.z = self.desired_target.z.clamp(-bound, bound);
    }

    pub fn step_smoothing(&mut self, dt_sec: f32, follow_pos: Option<Vec3>) {
        if self.preset == CameraPreset::FollowWildlifeUnderstoryCam {
            if let Some(pos) = follow_pos {
                self.desired_target = pos;
            }
        }
        let alpha = (1.0 - (-dt_sec * 8.5).exp()).clamp(0.05, 1.0);
        self.target = self.target.lerp(self.desired_target, alpha);
        self.yaw_rad += (self.desired_yaw_rad - self.yaw_rad) * alpha;
        self.pitch_rad += (self.desired_pitch_rad - self.pitch_rad) * alpha;
        self.distance += (self.desired_distance - self.distance) * alpha;

        // Telephoto isometric projection when zoomed out; wider perspective when zoomed inside canopy
        let zoom_t = ((self.distance - 10.0) / 56.0).clamp(0.0, 1.0);
        let target_fov = (42.0 - zoom_t * 15.0).to_radians();
        self.fov_y_rad += (target_fov - self.fov_y_rad) * alpha;
    }

    pub fn eye_position(&self) -> Vec3 {
        let horiz = self.distance * self.pitch_rad.cos();
        let vert = self.distance * self.pitch_rad.sin();
        self.target
            + Vec3::new(
                self.yaw_rad.sin() * horiz,
                vert,
                self.yaw_rad.cos() * horiz,
            )
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        let eye = self.eye_position();
        let view = Mat4::look_at_rh(eye, self.target, Vec3::Y);
        let proj = Mat4::perspective_rh(self.fov_y_rad, aspect.max(0.2), 0.5, 260.0);
        proj * view
    }

    pub fn pick_tile(
        &self,
        local_cursor: Vec2,
        viewport_size: Vec2,
        tiles: &[TileState],
    ) -> Option<(usize, usize)> {
        if viewport_size.x < 4.0 || viewport_size.y < 4.0 {
            return None;
        }
        let ndc_x = (local_cursor.x / viewport_size.x) * 2.0 - 1.0;
        let ndc_y = 1.0 - (local_cursor.y / viewport_size.y) * 2.0;
        let inv_vp = self.view_proj(viewport_size.x / viewport_size.y).inverse();

        let near4 = inv_vp * Vec4::new(ndc_x, ndc_y, 0.0, 1.0);
        let far4 = inv_vp * Vec4::new(ndc_x, ndc_y, 1.0, 1.0);
        if near4.w.abs() < 1e-6 || far4.w.abs() < 1e-6 {
            return None;
        }
        let ray_origin = near4.truncate() / near4.w;
        let ray_end = far4.truncate() / far4.w;
        let ray_dir = (ray_end - ray_origin).normalize_or_zero();

        let mut best: Option<((usize, usize), f32)> = None;
        for tile in tiles {
            let center = tile_world_center(tile);
            // Intersect ray with horizontal tile cap plane at tile.elevation
            if ray_dir.y.abs() > 1e-4 {
                let t = (center.y - ray_origin.y) / ray_dir.y;
                if t > 0.0 {
                    let hit = ray_origin + ray_dir * t;
                    let half = CELL_PITCH * 0.5;
                    if (hit.x - center.x).abs() <= half && (hit.z - center.z).abs() <= half {
                        if best.map(|(_, bt)| t < bt).unwrap_or(true) {
                            best = Some(((tile.x, tile.z), t));
                        }
                    }
                }
            }
        }
        best.map(|(coords, _)| coords)
    }
}
