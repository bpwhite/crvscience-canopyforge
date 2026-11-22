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

use crate::ecology::{
    compute_current_vpd_kpa, ClimateRegime, Ecotype, Season, TileState, CELL_PITCH, DAYS_PER_YEAR,
    GRID_SIZE,
};
use crate::strategies::{
    evaluate_and_apply_action, BudgetLedger, CompatibilityOutcome, RemediationAction,
    StrategyPortfolio,
};
use glam::Vec3;
use rand::Rng;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WildlifeSpecies {
    NorthernSpottedOwl,
    OregonCoastCohoSalmon,
    NorthAmericanBeaver,
    RooseveltElk,
    MarbledMurrelet,
    NorthernFlyingSquirrel,
}

impl WildlifeSpecies {
    pub const ALL: [Self; 6] = [
        Self::NorthernSpottedOwl,
        Self::OregonCoastCohoSalmon,
        Self::NorthAmericanBeaver,
        Self::RooseveltElk,
        Self::MarbledMurrelet,
        Self::NorthernFlyingSquirrel,
    ];

    pub fn common_name(self) -> &'static str {
        match self {
            Self::NorthernSpottedOwl => "Northern Spotted Owl",
            Self::OregonCoastCohoSalmon => "Oregon Coast Coho Salmon",
            Self::NorthAmericanBeaver => "North American Beaver",
            Self::RooseveltElk => "Roosevelt Elk",
            Self::MarbledMurrelet => "Marbled Murrelet",
            Self::NorthernFlyingSquirrel => "Northern Flying Squirrel",
        }
    }

    pub fn scientific_name(self) -> &'static str {
        match self {
            Self::NorthernSpottedOwl => "Strix occidentalis caurina",
            Self::OregonCoastCohoSalmon => "Oncorhynchus kisutch",
            Self::NorthAmericanBeaver => "Castor canadensis",
            Self::RooseveltElk => "Cervus canadensis roosevelti",
            Self::MarbledMurrelet => "Brachyramphus marmoratus",
            Self::NorthernFlyingSquirrel => "Glaucomys sabrinus",
        }
    }

    pub fn intrinsic_r(self) -> f32 {
        match self {
            Self::NorthernSpottedOwl => 0.16,
            Self::OregonCoastCohoSalmon => 0.42,
            Self::NorthAmericanBeaver => 0.28,
            Self::RooseveltElk => 0.22,
            Self::MarbledMurrelet => 0.14,
            Self::NorthernFlyingSquirrel => 0.36,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeciesPopulation {
    pub species: WildlifeSpecies,
    pub count: f32,
    pub carrying_capacity: f32,
    /// Realized annual recovery rate (% change per year)
    pub annual_recovery_rate_pct: f32,
    pub baseline_count: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisibleAnimalAgent {
    pub id: u32,
    pub species: WildlifeSpecies,
    pub pos: Vec3,
    pub target_pos: Vec3,
    pub heading_rad: f32,
    pub anim_phase: f32,
    pub speed: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CrewState {
    IdleAtStation,
    MovingToTarget,
    WorkingOnSite,
    ReturningToStation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StewardCrew {
    pub id: u32,
    pub assigned_action: RemediationAction,
    pub pos: Vec3,
    pub target_tile: Option<(usize, usize)>,
    pub state: CrewState,
    pub work_progress: f32,
    pub heading_rad: f32,
    pub last_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanopyLidarDrone {
    pub id: u32,
    pub orbit_angle: f32,
    pub orbit_radius: f32,
    pub altitude: f32,
    pub pos: Vec3,
}

pub fn tile_world_center(tile: &TileState) -> Vec3 {
    let half = (GRID_SIZE as f32 - 1.0) * 0.5;
    let wx = (tile.x as f32 - half) * CELL_PITCH;
    let wz = (tile.z as f32 - half) * CELL_PITCH;
    Vec3::new(wx, tile.elevation, wz)
}

pub fn initial_species_populations(tiles: &[TileState]) -> Vec<SpeciesPopulation> {
    let mut pops = vec![
        SpeciesPopulation {
            species: WildlifeSpecies::NorthernSpottedOwl,
            count: 4.0,
            carrying_capacity: 6.0,
            annual_recovery_rate_pct: 0.0,
            baseline_count: 4.0,
        },
        SpeciesPopulation {
            species: WildlifeSpecies::OregonCoastCohoSalmon,
            count: 42.0,
            carrying_capacity: 60.0,
            annual_recovery_rate_pct: 0.0,
            baseline_count: 42.0,
        },
        SpeciesPopulation {
            species: WildlifeSpecies::NorthAmericanBeaver,
            count: 3.0,
            carrying_capacity: 5.0,
            annual_recovery_rate_pct: 0.0,
            baseline_count: 3.0,
        },
        SpeciesPopulation {
            species: WildlifeSpecies::RooseveltElk,
            count: 9.0,
            carrying_capacity: 14.0,
            annual_recovery_rate_pct: 0.0,
            baseline_count: 9.0,
        },
        SpeciesPopulation {
            species: WildlifeSpecies::MarbledMurrelet,
            count: 6.0,
            carrying_capacity: 9.0,
            annual_recovery_rate_pct: 0.0,
            baseline_count: 6.0,
        },
        SpeciesPopulation {
            species: WildlifeSpecies::NorthernFlyingSquirrel,
            count: 18.0,
            carrying_capacity: 30.0,
            annual_recovery_rate_pct: 0.0,
            baseline_count: 18.0,
        },
    ];
    update_carrying_capacities(&mut pops, tiles);
    pops
}

pub fn update_carrying_capacities(pops: &mut [SpeciesPopulation], tiles: &[TileState]) {
    let mut og_habitat = 0.0_f32;
    let mut snag_platforms = 0.0_f32;
    let mut salmon_stream_quality = 0.0_f32;
    let mut beaver_riparian_quality = 0.0_f32;
    let mut elk_forage_mosaic = 0.0_f32;
    let mut squirrel_mycophagy_stands = 0.0_f32;

    for t in tiles {
        let fire_penalty = 1.0 - t.active_fire_intensity * 0.85;
        let inv_free = (1.0 - t.invasive_cover * 0.8).max(0.15);

        if t.old_growth_maturity > 0.55 {
            og_habitat += t.old_growth_maturity * t.canopy_biomass * fire_penalty;
        }
        snag_platforms += t.snag_cavity_density * t.old_growth_maturity * fire_penalty;

        if t.ecotype == Ecotype::RiparianSalmonCreek {
            // Roni & Quinn (2001) / Coast Coho Partnership: 3.2x Coho juvenile density with LWD & cold BDA pools
            let lwd_mult = 1.0 + t.bda_lwd_level * 2.2;
            salmon_stream_quality += t.soil_moisture * lwd_mult * inv_free * fire_penalty;
            beaver_riparian_quality +=
                (0.35 + t.bda_lwd_level * 0.85) * t.native_understory * inv_free;
        }

        if matches!(
            t.ecotype,
            Ecotype::UplandMeadowRidge
                | Ecotype::DegradedClearcutEdge
                | Ecotype::DenseSecondGrowth
        ) {
            let open_forage = (1.0 - t.ladder_fuels * 0.65) * t.native_understory * inv_free;
            elk_forage_mosaic += open_forage * fire_penalty;
        }

        // Witt (1992) & Rosenberg/Anthony (1992): ~0.85-2.3 flying squirrels/ha in old-growth vs 0.12/ha in dense 2nd growth
        squirrel_mycophagy_stands += (0.12
            + t.old_growth_maturity * 1.15
            + t.mycorrhizal_index * 0.55
            + t.snag_cavity_density * 0.45)
            * t.canopy_biomass
            * fire_penalty;
    }

    let squirrel_count = pops
        .iter()
        .find(|p| p.species == WildlifeSpecies::NorthernFlyingSquirrel)
        .map(|p| p.count)
        .unwrap_or(18.0);

    for p in pops.iter_mut() {
        p.carrying_capacity = match p.species {
            WildlifeSpecies::NorthernSpottedOwl => {
                (2.0 + og_habitat * 0.34 + snag_platforms * 0.22 + squirrel_count * 0.08)
                    .clamp(2.0, 48.0)
            }
            WildlifeSpecies::OregonCoastCohoSalmon => {
                (18.0 + salmon_stream_quality * 7.8).clamp(15.0, 420.0)
            }
            WildlifeSpecies::NorthAmericanBeaver => {
                (2.0 + beaver_riparian_quality * 1.45).clamp(2.0, 44.0)
            }
            WildlifeSpecies::RooseveltElk => (5.0 + elk_forage_mosaic * 0.52).clamp(4.0, 85.0),
            WildlifeSpecies::MarbledMurrelet => {
                (3.0 + og_habitat * 0.42 + snag_platforms * 0.28).clamp(2.0, 64.0)
            }
            WildlifeSpecies::NorthernFlyingSquirrel => {
                (8.0 + squirrel_mycophagy_stands * 0.72).clamp(8.0, 320.0)
            }
        };
    }
}

pub fn step_wildlife_populations(
    pops: &mut [SpeciesPopulation],
    tiles: &[TileState],
    dt_days: f32,
) {
    update_carrying_capacities(pops, tiles);
    let dt_yr = (dt_days / DAYS_PER_YEAR).clamp(0.0, 0.25);
    if dt_yr <= 0.0 {
        return;
    }

    for p in pops.iter_mut() {
        let r = p.species.intrinsic_r();
        let k = p.carrying_capacity.max(1.0);
        // Habitat-coupled logistic recovery + small regional corridor colonization when K > N
        let colonization = if k > p.count {
            (k - p.count) * 0.025
        } else {
            0.0
        };
        let dn_dt = r * p.count * (1.0 - p.count / k) + colonization;
        let next_count = (p.count + dn_dt * dt_yr).max(1.0);
        let inst_annual_pct = ((dn_dt / p.count.max(1.0)) * 100.0).clamp(-35.0, 120.0);
        p.annual_recovery_rate_pct =
            p.annual_recovery_rate_pct * 0.75 + inst_annual_pct * 0.25;
        p.count = next_count;
    }
}

pub fn compute_wildlife_recovery_score(pops: &[SpeciesPopulation]) -> f32 {
    let mut acc = 0.0_f32;
    for p in pops {
        let target = match p.species {
            WildlifeSpecies::NorthernSpottedOwl => 26.0,
            WildlifeSpecies::OregonCoastCohoSalmon => 260.0,
            WildlifeSpecies::NorthAmericanBeaver => 24.0,
            WildlifeSpecies::RooseveltElk => 48.0,
            WildlifeSpecies::MarbledMurrelet => 36.0,
            WildlifeSpecies::NorthernFlyingSquirrel => 180.0,
        };
        acc += (p.count / target).clamp(0.0, 1.0);
    }
    ((acc / pops.len().max(1) as f32) * 100.0).clamp(0.0, 100.0)
}

pub fn build_initial_crews(tiles: &[TileState]) -> (Vec<StewardCrew>, Vec<CanopyLidarDrone>) {
    let station_pos = tile_world_center(&tiles[0]) + Vec3::new(0.0, 0.02, 0.0);
    let crews = RemediationAction::ALL_STRATEGIES
        .iter()
        .enumerate()
        .map(|(i, &action)| {
            let mut pos = station_pos + Vec3::new((i as f32 % 3.0) * 0.9, 0.0, (i as f32 / 3.0) * 0.9);
            pos.y = sample_elevation_smooth(tiles, pos.x, pos.z) + 0.02;
            StewardCrew {
                id: (i + 1) as u32,
                assigned_action: action,
                pos,
                target_tile: None,
                state: CrewState::IdleAtStation,
                work_progress: 0.0,
                heading_rad: 0.0,
                last_status: "Ready at Field Station".to_string(),
            }
        })
        .collect();

    let drones = (0..3)
        .map(|i| {
            let angle = (i as f32) * std::f32::consts::TAU / 3.0;
            let r = 13.5 + (i as f32) * 3.0;
            let alt = 10.5 + (i as f32) * 1.1;
            CanopyLidarDrone {
                id: (i + 1) as u32,
                orbit_angle: angle,
                orbit_radius: r,
                altitude: alt,
                pos: Vec3::new(angle.cos() * r, alt, angle.sin() * r),
            }
        })
        .collect();

    (crews, drones)
}

pub fn step_crews_and_drones(
    crews: &mut [StewardCrew],
    drones: &mut [CanopyLidarDrone],
    tiles: &mut [TileState],
    portfolio: &StrategyPortfolio,
    ledger: &mut BudgetLedger,
    day_of_year: f32,
    regime: ClimateRegime,
    dt_real_sec: f32,
    sim_speed_mult: f32,
) -> Vec<String> {
    let mut events = Vec::new();
    let eff_dt = (dt_real_sec * sim_speed_mult.max(0.0)).min(0.5);
    if eff_dt <= 0.0 {
        return events;
    }

    for drone in drones.iter_mut() {
        drone.orbit_angle =
            (drone.orbit_angle + eff_dt * 0.22) % std::f32::consts::TAU;
        let bob = (drone.orbit_angle * 3.0).sin() * 0.35;
        drone.pos = Vec3::new(
            drone.orbit_angle.cos() * drone.orbit_radius,
            drone.altitude + bob,
            drone.orbit_angle.sin() * drone.orbit_radius,
        );
    }

    let season = Season::from_day_of_year(day_of_year);
    let current_vpd = compute_current_vpd_kpa(day_of_year, regime);
    let reserve_floor =
        ledger.annual_grant * (portfolio.manual_emergency_reserve_ratio.clamp(0.0, 0.85) as f64);

    // Check if any tile has active wildfire requiring priority suppression
    let burning_tile = tiles
        .iter()
        .filter(|t| t.active_fire_intensity > 0.05)
        .max_by(|a, b| {
            a.active_fire_intensity
                .partial_cmp(&b.active_fire_intensity)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|t| (t.x, t.z));

    for crew in crews.iter_mut() {
        let rule = portfolio
            .rules
            .iter()
            .find(|r| r.action == crew.assigned_action);
        let strategy_enabled = rule.map(|r| r.enabled && r.weight > 0.02).unwrap_or(false);

        // Crew #1 doubles as emergency rapid fire-line responder when wildfire breaks out
        let active_action = if crew.id == 1 && burning_tile.is_some() {
            RemediationAction::FirebreakAndMoisten
        } else {
            crew.assigned_action
        };

        match crew.state {
            CrewState::IdleAtStation | CrewState::ReturningToStation => {
                let can_spend = if active_action == RemediationAction::FirebreakAndMoisten {
                    ledger.remaining_budget >= active_action.cost_per_tile()
                } else {
                    strategy_enabled
                        && (ledger.remaining_budget - active_action.cost_per_tile())
                            >= reserve_floor
                };

                if can_spend {
                    if let Some(best_target) =
                        select_best_compatible_tile(active_action, tiles, season, current_vpd, crew.pos)
                    {
                        crew.target_tile = Some(best_target);
                        crew.state = CrewState::MovingToTarget;
                        crew.work_progress = 0.0;
                        crew.last_status = format!(
                            "Dispatching to ({}, {}) for {}",
                            best_target.0,
                            best_target.1,
                            active_action.short_label()
                        );
                        continue;
                    }
                }

                // Return gently toward Field Station at (0, 0)
                let station = tile_world_center(&tiles[0])
                    + Vec3::new((crew.id as f32 % 3.0) * 0.8, 0.02, (crew.id as f32 / 3.0) * 0.8);
                let delta = station - crew.pos;
                let dist = Vec3::new(delta.x, 0.0, delta.z).length();
                if dist > 0.25 {
                    crew.state = CrewState::ReturningToStation;
                    let step = (6.5 * eff_dt).min(dist);
                    let dir = Vec3::new(delta.x, 0.0, delta.z).normalize_or_zero();
                    crew.pos += dir * step;
                    crew.pos.y = sample_elevation_smooth(tiles, crew.pos.x, crew.pos.z) + 0.02;
                    crew.heading_rad = dir.x.atan2(dir.z);
                } else {
                    crew.state = CrewState::IdleAtStation;
                    crew.pos.y = sample_elevation_smooth(tiles, crew.pos.x, crew.pos.z) + 0.02;
                    crew.last_status = if !strategy_enabled {
                        "Standby (Strategy paused)".to_string()
                    } else if (ledger.remaining_budget - active_action.cost_per_tile()) < reserve_floor {
                        "Holding Annual Emergency Reserve".to_string()
                    } else {
                        "Patrol complete — all matching stands healthy".to_string()
                    };
                }
            }
            CrewState::MovingToTarget => {
                if let Some((tx, tz)) = crew.target_tile {
                    let target_tile_ref = &tiles[tz * GRID_SIZE + tx];
                    let target_world =
                        tile_world_center(target_tile_ref) + Vec3::new(0.0, 0.02, 0.0);
                    let delta = target_world - crew.pos;
                    let horiz = Vec3::new(delta.x, 0.0, delta.z);
                    let dist = horiz.length();
                    if dist <= 0.32 {
                        crew.pos = target_world;
                        crew.state = CrewState::WorkingOnSite;
                        crew.work_progress = 0.0;
                    } else {
                        let dir = horiz.normalize_or_zero();
                        let step = (8.2 * eff_dt).min(dist);
                        crew.pos += dir * step;
                        crew.pos.y = sample_elevation_smooth(tiles, crew.pos.x, crew.pos.z) + 0.02;
                        crew.heading_rad = dir.x.atan2(dir.z);
                    }
                } else {
                    crew.state = CrewState::IdleAtStation;
                }
            }
            CrewState::WorkingOnSite => {
                let weight_speed = rule.map(|r| 0.55 + r.weight * 0.95).unwrap_or(1.0);
                crew.work_progress += eff_dt * 0.75 * weight_speed;
                if crew.work_progress >= 1.0 {
                    if let Some((tx, tz)) = crew.target_tile.take() {
                        let cost = active_action.cost_per_tile();
                        if ledger.remaining_budget >= cost {
                            let tile = &mut tiles[tz * GRID_SIZE + tx];
                            let outcome = evaluate_and_apply_action(
                                active_action,
                                tile,
                                season,
                                current_vpd,
                                false,
                            );
                            if let CompatibilityOutcome::EffectiveSuccess { summary, .. } = outcome
                            {
                                ledger.remaining_budget -= cost;
                                ledger.spent_automated_this_year += cost;
                                crew.last_status =
                                    format!("Completed ({}, {}): {}", tx, tz, active_action.short_label());
                                events.push(format!(
                                    "[Crew #{}] ({}, {}) {} — {}",
                                    crew.id,
                                    tx,
                                    tz,
                                    active_action.short_label(),
                                    summary
                                ));
                            }
                        }
                    }
                    crew.state = CrewState::IdleAtStation;
                    crew.work_progress = 0.0;
                }
            }
        }
    }

    events
}

fn select_best_compatible_tile(
    action: RemediationAction,
    tiles: &[TileState],
    season: Season,
    current_vpd: f32,
    from_pos: Vec3,
) -> Option<(usize, usize)> {
    let mut best: Option<((usize, usize), f32)> = None;
    for tile in tiles {
        let mut probe = tile.clone();
        if let CompatibilityOutcome::EffectiveSuccess {
            ecological_gain, ..
        } = evaluate_and_apply_action(action, &mut probe, season, current_vpd, true)
        {
            let world = tile_world_center(tile);
            let dist_penalty = (from_pos.distance(world) * 0.004).min(0.18);
            let fire_urgency = if action == RemediationAction::FirebreakAndMoisten {
                tile.active_fire_intensity * 2.5 + tile.burn_risk * 0.4
            } else {
                0.0
            };
            let score = ecological_gain + fire_urgency - dist_penalty;
            if best.map(|(_, s)| score > s).unwrap_or(true) {
                best = Some(((tile.x, tile.z), score));
            }
        }
    }
    best.map(|(coords, _)| coords)
}

pub fn sample_elevation_at(tiles: &[TileState], wx: f32, wz: f32) -> f32 {
    let half = (GRID_SIZE as f32 - 1.0) * 0.5;
    let gx = ((wx / CELL_PITCH) + half).round().clamp(0.0, (GRID_SIZE - 1) as f32) as usize;
    let gz = ((wz / CELL_PITCH) + half).round().clamp(0.0, (GRID_SIZE - 1) as f32) as usize;
    tiles[gz * GRID_SIZE + gx].elevation
}

/// Bilinearly interpolates watershed elevation smoothly between tile centers.
pub fn sample_elevation_smooth(tiles: &[TileState], wx: f32, wz: f32) -> f32 {
    if tiles.len() < GRID_SIZE * GRID_SIZE {
        return 0.0;
    }
    let half = (GRID_SIZE as f32 - 1.0) * 0.5;
    let max_idx = (GRID_SIZE - 1) as f32;
    let fx = ((wx / CELL_PITCH) + half).clamp(0.0, max_idx);
    let fz = ((wz / CELL_PITCH) + half).clamp(0.0, max_idx);
    let x0 = (fx.floor() as usize).min(GRID_SIZE - 1);
    let z0 = (fz.floor() as usize).min(GRID_SIZE - 1);
    let x1 = (x0 + 1).min(GRID_SIZE - 1);
    let z1 = (z0 + 1).min(GRID_SIZE - 1);
    let tx = (fx - x0 as f32).clamp(0.0, 1.0);
    let tz = (fz - z0 as f32).clamp(0.0, 1.0);

    let e00 = tiles[z0 * GRID_SIZE + x0].elevation;
    let e10 = tiles[z0 * GRID_SIZE + x1].elevation;
    let e01 = tiles[z1 * GRID_SIZE + x0].elevation;
    let e11 = tiles[z1 * GRID_SIZE + x1].elevation;

    let ex0 = e00 + (e10 - e00) * tx;
    let ex1 = e01 + (e11 - e01) * tx;
    ex0 + (ex1 - ex0) * tz
}

pub fn sync_and_step_visible_animals(
    agents: &mut Vec<VisibleAnimalAgent>,
    pops: &[SpeciesPopulation],
    tiles: &[TileState],
    dt_sec: f32,
    rng: &mut ChaCha8Rng,
) {
    // Target visible 3D mesh instances per species scales smoothly with real population
    for pop in pops {
        let desired_vis = match pop.species {
            WildlifeSpecies::NorthernSpottedOwl => ((pop.count * 0.35).ceil() as usize).clamp(1, 8),
            WildlifeSpecies::OregonCoastCohoSalmon => {
                ((pop.count * 0.06).ceil() as usize).clamp(2, 18)
            }
            WildlifeSpecies::NorthAmericanBeaver => {
                ((pop.count * 0.35).ceil() as usize).clamp(1, 8)
            }
            WildlifeSpecies::RooseveltElk => ((pop.count * 0.30).ceil() as usize).clamp(2, 12),
            WildlifeSpecies::MarbledMurrelet => ((pop.count * 0.28).ceil() as usize).clamp(1, 9),
            WildlifeSpecies::NorthernFlyingSquirrel => {
                ((pop.count * 0.08).ceil() as usize).clamp(2, 12)
            }
        };

        let current_vis = agents.iter().filter(|a| a.species == pop.species).count();
        if current_vis < desired_vis {
            for _ in current_vis..desired_vis {
                let spawn_pos = pick_habitat_waypoint(pop.species, tiles, rng);
                let next_pos = pick_habitat_waypoint(pop.species, tiles, rng);
                let init_dir = next_pos - spawn_pos;
                let init_heading = if init_dir.x.abs() + init_dir.z.abs() > 1e-4 {
                    init_dir.x.atan2(init_dir.z)
                } else {
                    0.0
                };
                let id = (agents.len() as u32) + 100;
                agents.push(VisibleAnimalAgent {
                    id,
                    species: pop.species,
                    pos: spawn_pos,
                    target_pos: next_pos,
                    heading_rad: init_heading,
                    anim_phase: rng.gen_range(0.0..std::f32::consts::TAU),
                    speed: match pop.species {
                        WildlifeSpecies::NorthernSpottedOwl => 3.4,
                        WildlifeSpecies::OregonCoastCohoSalmon => 2.2,
                        WildlifeSpecies::NorthAmericanBeaver => 1.25,
                        WildlifeSpecies::RooseveltElk => 1.65,
                        WildlifeSpecies::MarbledMurrelet => 5.6,
                        WildlifeSpecies::NorthernFlyingSquirrel => 3.9,
                    },
                });
            }
        }
    }

    let step_dt = dt_sec.clamp(0.0, 0.1);
    for animal in agents.iter_mut() {
        animal.anim_phase = (animal.anim_phase + step_dt * 5.2) % std::f32::consts::TAU;
        let delta = animal.target_pos - animal.pos;
        let dist = delta.length();
        if dist < 0.45 {
            animal.target_pos = pick_habitat_waypoint(animal.species, tiles, rng);
        } else {
            let dir = delta / dist;
            animal.pos += dir * (animal.speed * step_dt).min(dist);
            match animal.species {
                WildlifeSpecies::NorthernFlyingSquirrel => {
                    // Smooth catenary glide arc between conifer trunks
                    animal.pos.y += (animal.anim_phase * 0.5).sin() * 0.015;
                }
                WildlifeSpecies::OregonCoastCohoSalmon => {
                    // Occasional riffle leap anchored in creek water channel
                    let leap = (animal.anim_phase * 1.4).sin().max(0.0).powi(4) * 0.28;
                    let base_y = sample_elevation_smooth(tiles, animal.pos.x, animal.pos.z) + 0.04;
                    animal.pos.y = base_y + leap;
                }
                WildlifeSpecies::RooseveltElk | WildlifeSpecies::NorthAmericanBeaver => {
                    animal.pos.y = sample_elevation_smooth(tiles, animal.pos.x, animal.pos.z) + 0.02;
                }
                WildlifeSpecies::NorthernSpottedOwl | WildlifeSpecies::MarbledMurrelet => {}
            }
            animal.heading_rad = dir.x.atan2(dir.z);
        }
    }
}

fn pick_habitat_waypoint(
    species: WildlifeSpecies,
    tiles: &[TileState],
    rng: &mut ChaCha8Rng,
) -> Vec3 {
    let candidates: Vec<&TileState> = tiles
        .iter()
        .filter(|t| match species {
            WildlifeSpecies::OregonCoastCohoSalmon | WildlifeSpecies::NorthAmericanBeaver => {
                t.ecotype == Ecotype::RiparianSalmonCreek
            }
            WildlifeSpecies::NorthernSpottedOwl
            | WildlifeSpecies::MarbledMurrelet
            | WildlifeSpecies::NorthernFlyingSquirrel => {
                t.old_growth_maturity > 0.45 || t.ecotype == Ecotype::OldGrowthCore
            }
            WildlifeSpecies::RooseveltElk => {
                matches!(
                    t.ecotype,
                    Ecotype::UplandMeadowRidge
                        | Ecotype::DegradedClearcutEdge
                        | Ecotype::DenseSecondGrowth
                ) && t.active_fire_intensity <= 0.01
            }
        })
        .collect();

    let chosen = if candidates.is_empty() {
        &tiles[rng.gen_range(0..tiles.len())]
    } else {
        candidates[rng.gen_range(0..candidates.len())]
    };
    let base = tile_world_center(chosen);
    let jitter_x = rng.gen_range(-1.05..1.05);
    let jitter_z = rng.gen_range(-1.05..1.05);
    let wx = base.x + jitter_x;
    let wz = base.z + jitter_z;
    let smooth_y = sample_elevation_smooth(tiles, wx, wz);
    let height_offset = match species {
        WildlifeSpecies::NorthernSpottedOwl => rng.gen_range(4.2..6.8),
        WildlifeSpecies::MarbledMurrelet => rng.gen_range(6.8..9.2),
        WildlifeSpecies::NorthernFlyingSquirrel => rng.gen_range(2.8..5.2),
        WildlifeSpecies::OregonCoastCohoSalmon => 0.04,
        WildlifeSpecies::NorthAmericanBeaver => 0.02,
        WildlifeSpecies::RooseveltElk => 0.02,
    };
    Vec3::new(wx, smooth_y + height_offset, wz)
}
