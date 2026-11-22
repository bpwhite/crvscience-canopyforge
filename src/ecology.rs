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

use rand::Rng;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

pub const GRID_SIZE: usize = 16;
pub const CELL_PITCH: f32 = 3.0;
pub const DAYS_PER_SEASON: f32 = 91.25;
pub const DAYS_PER_YEAR: f32 = 365.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Ecotype {
    OldGrowthCore,
    DenseSecondGrowth,
    RiparianSalmonCreek,
    DegradedClearcutEdge,
    UplandMeadowRidge,
}

impl Ecotype {
    pub fn label(self) -> &'static str {
        match self {
            Self::OldGrowthCore => "Remnant Old-Growth Core",
            Self::DenseSecondGrowth => "Dense Second-Growth Plantation",
            Self::RiparianSalmonCreek => "Incised Salmonid Creek Corridor",
            Self::DegradedClearcutEdge => "Degraded Clearcut Edge",
            Self::UplandMeadowRidge => "Upland Oak-Conifer Meadow Ridge",
        }
    }

    pub fn short_code(self) -> &'static str {
        match self {
            Self::OldGrowthCore => "OGC",
            Self::DenseSecondGrowth => "2ND",
            Self::RiparianSalmonCreek => "RIP",
            Self::DegradedClearcutEdge => "CLR",
            Self::UplandMeadowRidge => "UPL",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Season {
    Spring,
    Summer,
    Autumn,
    Winter,
}

impl Season {
    pub fn from_day_of_year(day: f32) -> Self {
        let d = day.rem_euclid(DAYS_PER_YEAR);
        if d < DAYS_PER_SEASON {
            Self::Spring
        } else if d < DAYS_PER_SEASON * 2.0 {
            Self::Summer
        } else if d < DAYS_PER_SEASON * 3.0 {
            Self::Autumn
        } else {
            Self::Winter
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Spring => "Spring (Freshet & Budburst)",
            Self::Summer => "Summer (Dry VPD Window)",
            Self::Autumn => "Autumn (Coho Spawning & Fog)",
            Self::Winter => "Winter (Maritime Rain & Snowpack)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClimateRegime {
    NormalMaritime,
    WarmDrySummer,
    SevereVpdDrought,
    CoolWetLaNina,
}

impl ClimateRegime {
    pub fn label(self) -> &'static str {
        match self {
            Self::NormalMaritime => "Normal Maritime Moisture",
            Self::WarmDrySummer => "Elevated Summer VPD",
            Self::SevereVpdDrought => "Severe Multi-Month VPD Drought",
            Self::CoolWetLaNina => "Cool Wet La Nina Snowpack",
        }
    }

    pub fn peak_summer_vpd_kpa(self) -> f32 {
        match self {
            Self::CoolWetLaNina => 1.35,
            Self::NormalMaritime => 1.95,
            Self::WarmDrySummer => 2.75,
            Self::SevereVpdDrought => 3.65,
        }
    }

    pub fn precipitation_factor(self) -> f32 {
        match self {
            Self::CoolWetLaNina => 1.28,
            Self::NormalMaritime => 1.00,
            Self::WarmDrySummer => 0.76,
            Self::SevereVpdDrought => 0.48,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InvasiveDominant {
    None,
    HimalayanBlackberry,
    ScotchBroom,
    EnglishIvy,
}

impl InvasiveDominant {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Minimal / Native Understory",
            Self::HimalayanBlackberry => "Himalayan Blackberry (Rubus armeniacus)",
            Self::ScotchBroom => "Scotch Broom (Cytisus scoparius)",
            Self::EnglishIvy => "Climbing English Ivy (Hedera helix)",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileState {
    pub x: usize,
    pub z: usize,
    pub elevation: f32,
    pub ecotype: Ecotype,
    /// Structural old-growth maturity & multi-layered complexity (0.0..=1.0)
    pub old_growth_maturity: f32,
    /// Overstory native conifer & hardwood biomass (0.0..=1.0)
    pub canopy_biomass: f32,
    /// Native understory sword fern, salal, huckleberry & mycorrhizal health (0.0..=1.0)
    pub native_understory: f32,
    /// Small-diameter conifer ladder fuels & surface litter continuity (0.0..=1.0)
    pub ladder_fuels: f32,
    /// Volumetric soil moisture & hyporheic water table support (0.0..=1.0)
    pub soil_moisture: f32,
    /// Combined invasive plant cover fraction (0.0..=1.0)
    pub invasive_cover: f32,
    pub blackberry_cover: f32,
    pub scotch_broom_cover: f32,
    pub english_ivy_cover: f32,
    /// Beaver Dam Analogue (BDA) & Large Woody Debris (LWD) level in riparian tiles (0.0..=1.0)
    pub bda_lwd_level: f32,
    /// Legacy snag cavities & mossy murrelet/owl platform density (0.0..=1.0)
    pub snag_cavity_density: f32,
    /// Mycorrhizal truffle & nurse-log soil inoculation level (0.0..=1.0)
    pub mycorrhizal_index: f32,
    /// Computed real-time crown/stand-replacing burn risk (0.0..=1.0)
    pub burn_risk: f32,
    /// Active wildfire intensity (> 0.0 means tile is currently burning)
    pub active_fire_intensity: f32,
    /// Post-fire char cooldown (decays smoothly as moss and herbs flush)
    pub recent_burn_scar: f32,
    /// Temporary firebreak / foliar moisture wetting buffer
    pub fire_retardant_wetness: f32,
}

impl TileState {
    pub fn dominant_invasive(&self) -> InvasiveDominant {
        if self.invasive_cover < 0.04 {
            return InvasiveDominant::None;
        }
        if self.scotch_broom_cover >= self.blackberry_cover
            && self.scotch_broom_cover >= self.english_ivy_cover
        {
            InvasiveDominant::ScotchBroom
        } else if self.english_ivy_cover >= self.blackberry_cover {
            InvasiveDominant::EnglishIvy
        } else {
            InvasiveDominant::HimalayanBlackberry
        }
    }

    pub fn sync_invasive_total(&mut self) {
        self.blackberry_cover = self.blackberry_cover.clamp(0.0, 1.0);
        self.scotch_broom_cover = self.scotch_broom_cover.clamp(0.0, 1.0);
        self.english_ivy_cover = self.english_ivy_cover.clamp(0.0, 1.0);
        self.invasive_cover =
            (self.blackberry_cover + self.scotch_broom_cover + self.english_ivy_cover)
                .clamp(0.0, 1.0);
    }

    pub fn recompute_burn_risk(&mut self, current_vpd_kpa: f32) {
        if self.ecotype == Ecotype::RiparianSalmonCreek && self.bda_lwd_level > 0.35 {
            // Deep BDA beaver ponds and wetted gravel bars act as persistent fire refugia
            let raw = 0.08 * self.ladder_fuels + 0.12 * self.blackberry_cover
                - 0.35 * self.soil_moisture
                - 0.40 * self.fire_retardant_wetness;
            self.burn_risk = raw.clamp(0.01, 0.25);
            return;
        }
        let vpd_norm = ((current_vpd_kpa - 0.6) / 3.2).clamp(0.0, 1.0);
        // Old-growth closed-canopy microclimate buffers interior VPD and wind drying
        let canopy_refugia = self.old_growth_maturity * 0.28 + self.mycorrhizal_index * 0.08;
        let raw = self.ladder_fuels * 0.44
            + vpd_norm * 0.26
            + self.scotch_broom_cover * 0.34
            + self.blackberry_cover * 0.14
            + self.english_ivy_cover * 0.12
            - self.soil_moisture * 0.38
            - canopy_refugia
            - self.fire_retardant_wetness * 0.55;
        self.burn_risk = raw.clamp(0.02, 0.98);
    }

    pub fn ecological_tile_score(&self) -> f32 {
        let inv_health = 1.0 - self.invasive_cover;
        let fire_resilience = 1.0 - self.burn_risk * 0.75;
        let raw = match self.ecotype {
            Ecotype::OldGrowthCore => {
                self.old_growth_maturity * 0.34
                    + self.native_understory * 0.18
                    + self.snag_cavity_density * 0.18
                    + inv_health * 0.16
                    + fire_resilience * 0.14
            }
            Ecotype::DenseSecondGrowth => {
                self.old_growth_maturity * 0.28
                    + self.native_understory * 0.20
                    + (1.0 - self.ladder_fuels) * 0.22
                    + self.mycorrhizal_index * 0.15
                    + inv_health * 0.15
            }
            Ecotype::RiparianSalmonCreek => {
                self.bda_lwd_level * 0.40
                    + self.soil_moisture * 0.22
                    + self.native_understory * 0.18
                    + inv_health * 0.20
            }
            Ecotype::DegradedClearcutEdge => {
                self.canopy_biomass * 0.28
                    + self.native_understory * 0.22
                    + self.mycorrhizal_index * 0.18
                    + inv_health * 0.32
            }
            Ecotype::UplandMeadowRidge => {
                self.native_understory * 0.30
                    + fire_resilience * 0.26
                    + inv_health * 0.26
                    + self.old_growth_maturity * 0.18
            }
        };
        (raw * 100.0).clamp(0.0, 100.0)
    }
}

pub fn build_initial_watershed(rng: &mut ChaCha8Rng) -> Vec<TileState> {
    let mut tiles = Vec::with_capacity(GRID_SIZE * GRID_SIZE);
    for z in 0..GRID_SIZE {
        for x in 0..GRID_SIZE {
            let xf = x as f32;
            let zf = z as f32;
            // Meandering Coast Range salmon creek running from NE ridge (15,2) toward SW valley (0,13)
            let creek_center_z = 13.2 - xf * 0.72 + (xf * 0.48).sin() * 1.35;
            let dist_to_creek = (zf - creek_center_z).abs();

            let ridge_dist = ((xf - 13.5).powi(2) + (zf - 13.0).powi(2)).sqrt();
            let og_core_dist = ((xf - 4.2).powi(2) + (zf - 4.5).powi(2)).sqrt();
            let clearcut_dist = ((xf - 11.8).powi(2) + (zf - 3.2).powi(2)).sqrt();

            let (ecotype, base_elev) = if dist_to_creek < 1.05 {
                (Ecotype::RiparianSalmonCreek, 0.25 + xf * 0.05)
            } else if og_core_dist < 4.3 {
                (
                    Ecotype::OldGrowthCore,
                    0.85 + (4.3 - og_core_dist) * 0.14 + rng.gen_range(-0.05..0.06),
                )
            } else if ridge_dist < 4.6 {
                (
                    Ecotype::UplandMeadowRidge,
                    1.65 + (4.6 - ridge_dist) * 0.28 + rng.gen_range(-0.04..0.08),
                )
            } else if clearcut_dist < 3.9 || x >= 13 && z <= 5 {
                (
                    Ecotype::DegradedClearcutEdge,
                    0.75 + xf * 0.04 + rng.gen_range(-0.04..0.05),
                )
            } else {
                (
                    Ecotype::DenseSecondGrowth,
                    0.90 + (xf * 0.05) + (zf * 0.04) + rng.gen_range(-0.06..0.06),
                )
            };

            let mut tile = match ecotype {
                Ecotype::OldGrowthCore => TileState {
                    x,
                    z,
                    elevation: base_elev,
                    ecotype,
                    old_growth_maturity: rng.gen_range(0.76..0.92),
                    canopy_biomass: rng.gen_range(0.82..0.95),
                    native_understory: rng.gen_range(0.68..0.85),
                    ladder_fuels: rng.gen_range(0.22..0.38),
                    soil_moisture: rng.gen_range(0.64..0.78),
                    invasive_cover: 0.0,
                    blackberry_cover: 0.01,
                    scotch_broom_cover: 0.0,
                    english_ivy_cover: if og_core_dist > 3.1 {
                        rng.gen_range(0.10..0.24)
                    } else {
                        rng.gen_range(0.0..0.04)
                    },
                    bda_lwd_level: 0.15,
                    snag_cavity_density: rng.gen_range(0.55..0.76),
                    mycorrhizal_index: rng.gen_range(0.74..0.90),
                    burn_risk: 0.18,
                    active_fire_intensity: 0.0,
                    recent_burn_scar: 0.0,
                    fire_retardant_wetness: 0.0,
                },
                Ecotype::DenseSecondGrowth => TileState {
                    x,
                    z,
                    elevation: base_elev,
                    ecotype,
                    old_growth_maturity: rng.gen_range(0.20..0.34),
                    canopy_biomass: rng.gen_range(0.64..0.78),
                    native_understory: rng.gen_range(0.20..0.34),
                    ladder_fuels: rng.gen_range(0.66..0.86),
                    soil_moisture: rng.gen_range(0.42..0.56),
                    invasive_cover: 0.0,
                    blackberry_cover: rng.gen_range(0.04..0.12),
                    scotch_broom_cover: rng.gen_range(0.0..0.05),
                    english_ivy_cover: rng.gen_range(0.06..0.18),
                    bda_lwd_level: 0.05,
                    snag_cavity_density: rng.gen_range(0.10..0.22),
                    mycorrhizal_index: rng.gen_range(0.28..0.42),
                    burn_risk: 0.55,
                    active_fire_intensity: 0.0,
                    recent_burn_scar: 0.0,
                    fire_retardant_wetness: 0.0,
                },
                Ecotype::RiparianSalmonCreek => TileState {
                    x,
                    z,
                    elevation: base_elev,
                    ecotype,
                    old_growth_maturity: rng.gen_range(0.28..0.45),
                    canopy_biomass: rng.gen_range(0.36..0.54),
                    native_understory: rng.gen_range(0.38..0.55),
                    ladder_fuels: rng.gen_range(0.12..0.24),
                    soil_moisture: rng.gen_range(0.62..0.76),
                    invasive_cover: 0.0,
                    blackberry_cover: rng.gen_range(0.22..0.44),
                    scotch_broom_cover: rng.gen_range(0.02..0.06),
                    english_ivy_cover: rng.gen_range(0.03..0.09),
                    bda_lwd_level: rng.gen_range(0.08..0.18),
                    snag_cavity_density: rng.gen_range(0.18..0.30),
                    mycorrhizal_index: rng.gen_range(0.40..0.56),
                    burn_risk: 0.14,
                    active_fire_intensity: 0.0,
                    recent_burn_scar: 0.0,
                    fire_retardant_wetness: 0.0,
                },
                Ecotype::DegradedClearcutEdge => TileState {
                    x,
                    z,
                    elevation: base_elev,
                    ecotype,
                    old_growth_maturity: rng.gen_range(0.04..0.12),
                    canopy_biomass: rng.gen_range(0.10..0.22),
                    native_understory: rng.gen_range(0.12..0.24),
                    ladder_fuels: rng.gen_range(0.32..0.48),
                    soil_moisture: rng.gen_range(0.26..0.38),
                    invasive_cover: 0.0,
                    blackberry_cover: rng.gen_range(0.30..0.48),
                    scotch_broom_cover: rng.gen_range(0.18..0.34),
                    english_ivy_cover: rng.gen_range(0.04..0.10),
                    bda_lwd_level: 0.0,
                    snag_cavity_density: rng.gen_range(0.04..0.12),
                    mycorrhizal_index: rng.gen_range(0.10..0.20),
                    burn_risk: 0.62,
                    active_fire_intensity: 0.0,
                    recent_burn_scar: 0.0,
                    fire_retardant_wetness: 0.0,
                },
                Ecotype::UplandMeadowRidge => TileState {
                    x,
                    z,
                    elevation: base_elev,
                    ecotype,
                    old_growth_maturity: rng.gen_range(0.32..0.48),
                    canopy_biomass: rng.gen_range(0.34..0.50),
                    native_understory: rng.gen_range(0.42..0.58),
                    ladder_fuels: rng.gen_range(0.44..0.64),
                    soil_moisture: rng.gen_range(0.28..0.42),
                    invasive_cover: 0.0,
                    blackberry_cover: rng.gen_range(0.05..0.14),
                    scotch_broom_cover: rng.gen_range(0.22..0.42),
                    english_ivy_cover: 0.01,
                    bda_lwd_level: 0.0,
                    snag_cavity_density: rng.gen_range(0.22..0.36),
                    mycorrhizal_index: rng.gen_range(0.34..0.48),
                    burn_risk: 0.66,
                    active_fire_intensity: 0.0,
                    recent_burn_scar: 0.0,
                    fire_retardant_wetness: 0.0,
                },
            };
            tile.sync_invasive_total();
            tile.recompute_burn_risk(1.3);
            tiles.push(tile);
        }
    }
    tiles
}

pub fn compute_current_vpd_kpa(day_of_year: f32, regime: ClimateRegime) -> f32 {
    // Peak drying occurs around day 165-215 (late Summer in western Oregon)
    let phase = ((day_of_year - 185.0) / DAYS_PER_YEAR) * std::f32::consts::TAU;
    let summer_bell = ((phase.cos() + 1.0) * 0.5).powf(1.7);
    let winter_floor = 0.52;
    winter_floor + (regime.peak_summer_vpd_kpa() - winter_floor) * summer_bell
}

pub fn step_watershed_ecology(
    tiles: &mut [TileState],
    dt_days: f32,
    day_of_year: f32,
    regime: ClimateRegime,
    flying_squirrel_mycophagy_bonus: f32,
    beaver_dam_maintenance_bonus: f32,
    rng: &mut ChaCha8Rng,
) -> (usize, usize) {
    let dt_yr = (dt_days / DAYS_PER_YEAR).clamp(0.0, 0.25);
    let current_vpd = compute_current_vpd_kpa(day_of_year, regime);
    let season = Season::from_day_of_year(day_of_year);
    let precip = regime.precipitation_factor()
        * match season {
            Season::Winter => 1.35,
            Season::Spring => 1.15,
            Season::Autumn => 1.05,
            Season::Summer => 0.28,
        };

    // Snapshot BDA pond water-table support and fire neighbor intensities
    let mut bda_support = vec![0.0_f32; tiles.len()];
    let mut fire_neighbor_pressure = vec![0.0_f32; tiles.len()];
    let mut invasive_seed_pressure = vec![0.0_f32; tiles.len()];

    for z in 0..GRID_SIZE {
        for x in 0..GRID_SIZE {
            let idx = z * GRID_SIZE + x;
            let mut hydro = 0.0_f32;
            let mut fire_in = 0.0_f32;
            let mut inv_in = 0.0_f32;

            for dz in -2_isize..=2 {
                for dx in -2_isize..=2 {
                    let nx = x as isize + dx;
                    let nz = z as isize + dz;
                    if nx < 0
                        || nz < 0
                        || nx >= GRID_SIZE as isize
                        || nz >= GRID_SIZE as isize
                    {
                        continue;
                    }
                    let nidx = (nz as usize) * GRID_SIZE + (nx as usize);
                    let dist = ((dx * dx + dz * dz) as f32).sqrt().max(0.5);
                    let ntile = &tiles[nidx];
                    if ntile.ecotype == Ecotype::RiparianSalmonCreek {
                        let pool_strength = 0.25 + ntile.bda_lwd_level * 0.85;
                        hydro += (pool_strength / (dist * 1.15)).min(0.45);
                    }
                    if dist <= 1.5 && (dx != 0 || dz != 0) {
                        if ntile.active_fire_intensity > 0.15 {
                            // Upslope & east-wind fire spread factor
                            let slope_boost =
                                (tiles[idx].elevation - ntile.elevation).clamp(-0.2, 0.5) * 0.45;
                            fire_in +=
                                (ntile.active_fire_intensity / dist) * (1.0 + slope_boost);
                        }
                        if ntile.invasive_cover > 0.28 {
                            inv_in += (ntile.invasive_cover - 0.22) * 0.12 / dist;
                        }
                    }
                }
            }
            bda_support[idx] = hydro.clamp(0.0, 0.55);
            fire_neighbor_pressure[idx] = fire_in;
            invasive_seed_pressure[idx] = inv_in;
        }
    }

    let mut active_fires = 0_usize;
    let mut new_ignitions = 0_usize;

    for (idx, tile) in tiles.iter_mut().enumerate() {
        // Beavers naturally reinforce existing BDAs along riparian creek reaches
        if tile.ecotype == Ecotype::RiparianSalmonCreek && tile.bda_lwd_level > 0.12 {
            tile.bda_lwd_level =
                (tile.bda_lwd_level + beaver_dam_maintenance_bonus * 0.09 * dt_yr).clamp(0.0, 1.0);
        }

        // Soil moisture balance: winter recharge + BDA hyporheic water table - VPD drying
        let canopy_shade = tile.canopy_biomass * 0.24 + tile.old_growth_maturity * 0.18;
        let target_moisture = (0.30 * precip + bda_support[idx] + canopy_shade
            - (current_vpd - 0.8).max(0.0) * 0.14)
            .clamp(0.10, 0.96);
        tile.soil_moisture += (target_moisture - tile.soil_moisture) * (dt_days * 0.08).min(1.0);

        // Retardant / manual wetting decays gently over ~45 days
        tile.fire_retardant_wetness =
            (tile.fire_retardant_wetness - dt_days * 0.022).max(0.0);
        tile.recent_burn_scar = (tile.recent_burn_scar - dt_yr * 0.38).max(0.0);

        // Invasive species spread if native canopy/understory leaves open niches
        let shade_suppression = (tile.canopy_biomass * 0.65 + tile.native_understory * 0.35)
            .clamp(0.0, 0.95);
        let open_niche = (1.0 - shade_suppression).max(0.06);
        match tile.ecotype {
            Ecotype::DegradedClearcutEdge => {
                if tile.blackberry_cover > 0.02 || invasive_seed_pressure[idx] > 0.05 {
                    tile.blackberry_cover +=
                        (0.11 * open_niche + invasive_seed_pressure[idx] * 0.08) * dt_yr;
                }
                if tile.scotch_broom_cover > 0.02 {
                    tile.scotch_broom_cover += 0.09 * open_niche * dt_yr;
                }
            }
            Ecotype::RiparianSalmonCreek => {
                if tile.blackberry_cover > 0.03 {
                    tile.blackberry_cover += 0.08 * open_niche * dt_yr;
                }
            }
            Ecotype::UplandMeadowRidge => {
                if tile.scotch_broom_cover > 0.02 || invasive_seed_pressure[idx] > 0.05 {
                    tile.scotch_broom_cover +=
                        (0.10 * open_niche + invasive_seed_pressure[idx] * 0.07) * dt_yr;
                }
            }
            Ecotype::OldGrowthCore | Ecotype::DenseSecondGrowth => {
                if tile.english_ivy_cover > 0.02 || invasive_seed_pressure[idx] > 0.06 {
                    tile.english_ivy_cover +=
                        (0.055 * (1.0 - tile.native_understory * 0.5)
                            + invasive_seed_pressure[idx] * 0.04)
                            * dt_yr;
                }
            }
        }
        tile.sync_invasive_total();

        // Natural forest succession, mycorrhizal symbiosis & old-growth recruitment
        let inv_drag = (1.0 - tile.invasive_cover * 0.82).max(0.10);
        let moisture_growth = (tile.soil_moisture * 1.35).clamp(0.25, 1.15);
        let myco_boost = 1.0 + tile.mycorrhizal_index * 0.55 + flying_squirrel_mycophagy_bonus * 0.25;

        tile.mycorrhizal_index = (tile.mycorrhizal_index
            + (0.045 * tile.canopy_biomass + flying_squirrel_mycophagy_bonus * 0.05)
                * inv_drag
                * dt_yr)
            .clamp(0.05, 1.0);

        tile.native_understory = (tile.native_understory
            + 0.065 * moisture_growth * inv_drag * myco_boost * (1.0 - tile.ladder_fuels * 0.45)
                * dt_yr
            - tile.invasive_cover * 0.035 * dt_yr)
            .clamp(0.05, 1.0);

        if tile.ecotype != Ecotype::RiparianSalmonCreek {
            tile.canopy_biomass = (tile.canopy_biomass
                + 0.048 * moisture_growth * inv_drag * myco_boost * dt_yr)
                .clamp(0.08, 1.0);
        } else {
            tile.canopy_biomass = (tile.canopy_biomass
                + 0.042 * (0.6 + tile.bda_lwd_level * 0.6) * inv_drag * dt_yr)
                .clamp(0.20, 0.88);
        }

        // Old-growth structural recruitment accelerates when dense second-growth is thinned & free of ivy
        let spacing_bonus = if tile.ladder_fuels < 0.42 { 1.45 } else { 0.65 };
        tile.old_growth_maturity = (tile.old_growth_maturity
            + 0.036 * tile.canopy_biomass * spacing_bonus * myco_boost * inv_drag * dt_yr)
            .clamp(0.04, 1.0);

        // Fine surface fuels slowly accumulate in dense un-thinned stands
        if tile.ecotype == Ecotype::DenseSecondGrowth || tile.ecotype == Ecotype::UplandMeadowRidge {
            tile.ladder_fuels = (tile.ladder_fuels + 0.038 * dt_yr).clamp(0.06, 0.95);
        } else if tile.ecotype == Ecotype::OldGrowthCore {
            tile.ladder_fuels = (tile.ladder_fuels + 0.018 * dt_yr).clamp(0.06, 0.75);
        }

        tile.snag_cavity_density = (tile.snag_cavity_density
            + 0.025 * tile.old_growth_maturity * dt_yr)
            .clamp(0.04, 1.0);

        tile.recompute_burn_risk(current_vpd);

        // Simulated summer dry-lightning ignitions & percolation spread
        if tile.active_fire_intensity > 0.0 {
            active_fires += 1;
            // Rain, wet riparian refugia, or low burn risk naturally quench fire
            let quench = (1.0 - tile.burn_risk) * 0.09
                + tile.soil_moisture * 0.06
                + tile.fire_retardant_wetness * 0.35
                + if season == Season::Winter || season == Season::Spring {
                    0.14
                } else {
                    0.02
                };
            tile.active_fire_intensity -= quench * dt_days;

            // Moderate/high fire consumes ladder fuels & invasives; crown damage occurs ONLY if ladder_fuels > 0.55
            let burn_step = (tile.active_fire_intensity * 0.035 * dt_days).min(0.25);
            tile.ladder_fuels = (tile.ladder_fuels - burn_step * 1.4).max(0.05);
            tile.scotch_broom_cover = (tile.scotch_broom_cover - burn_step * 1.1).max(0.0);
            tile.blackberry_cover = (tile.blackberry_cover - burn_step * 0.8).max(0.0);
            tile.sync_invasive_total();

            if tile.ladder_fuels > 0.52 && tile.burn_risk > 0.58 {
                // High-severity torching in un-thinned ladder-fuel stands
                tile.canopy_biomass = (tile.canopy_biomass - burn_step * 0.45).max(0.12);
                tile.old_growth_maturity = (tile.old_growth_maturity - burn_step * 0.25).max(0.08);
                tile.snag_cavity_density = (tile.snag_cavity_density + burn_step * 0.18).min(0.95);
            } else {
                // Beneficial low-severity surface underburn in restored fire-resilient stands!
                tile.native_understory = (tile.native_understory + burn_step * 0.12).min(1.0);
            }

            if tile.active_fire_intensity <= 0.02 {
                tile.active_fire_intensity = 0.0;
                tile.recent_burn_scar = 0.55;
            }
        } else if tile.recent_burn_scar < 0.25 && tile.fire_retardant_wetness < 0.20 {
            // Neighbor percolation spread
            if fire_neighbor_pressure[idx] > 0.32 && tile.burn_risk > 0.36 {
                let spread_prob =
                    ((fire_neighbor_pressure[idx] * tile.burn_risk) * 0.22 * dt_days).min(0.65);
                if rng.gen::<f32>() < spread_prob {
                    tile.active_fire_intensity = (0.45 + tile.burn_risk * 0.45).min(0.95);
                    active_fires += 1;
                    new_ignitions += 1;
                }
            } else if season == Season::Summer
                && current_vpd > 2.25
                && tile.burn_risk > 0.58
            {
                // Natural summer dry-lightning strike on exposed high-burn-risk stands
                let lightning_prob = (tile.burn_risk - 0.52) * 0.0022 * dt_days;
                if rng.gen::<f32>() < lightning_prob {
                    tile.active_fire_intensity = 0.68;
                    active_fires += 1;
                    new_ignitions += 1;
                }
            }
        }
    }

    (active_fires, new_ignitions)
}
