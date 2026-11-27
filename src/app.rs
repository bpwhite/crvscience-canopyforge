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
    build_initial_crews, compute_wildlife_recovery_score, initial_species_populations,
    step_crews_and_drones, step_wildlife_populations, sync_and_step_visible_animals,
    tile_world_center, CanopyLidarDrone, SpeciesPopulation, StewardCrew, VisibleAnimalAgent,
    WildlifeSpecies,
};
use crate::camera::{CameraPreset, IsometricZoomCamera};
use crate::ecology::{
    build_initial_watershed, compute_current_vpd_kpa, step_watershed_ecology, ClimateRegime,
    Ecotype, Season, TileState, DAYS_PER_YEAR, GRID_SIZE,
};
use crate::models_3d::{
    build_static_watershed_meshes, compose_watershed_scene_meshes, overlay_legend_color, MapOverlay,
    StaticWatershedMeshCache,
};
use crate::persistence::{SaveDatabase, SimulationSaveState};
use crate::scorecard::{export_century_scorecard_files, CenturyRestorationScorecard};
use crate::strategies::{
    evaluate_and_apply_action, BudgetLedger, CompatibilityOutcome, RemediationAction,
    StrategyPortfolio,
};
use crate::wgpu_renderer::{
    build_sun_light_matrix, SceneFrameUniforms, WatershedGpuResources, WatershedPaintCallback,
};
use eframe::{egui, egui_wgpu};
use glam::{Vec2, Vec3, Vec4};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use std::collections::VecDeque;
use std::sync::Arc;

const SPEED_STEPS: [f32; 4] = [1.0, 2.0, 5.0, 10.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanopyPaletteAction {
    JumpEcotype(Ecotype),
    SelectTool(RemediationAction),
    ToggleBrush3x3,
    ToggleInspectOnly,
    JumpSeasonDay(u16),
    CameraPreset(CameraPreset),
    TogglePause,
    ToggleAuditModal,
    ExportCenturyScorecard,
    OpenJsonSchema,
    QuickSaveSqlite,
}

fn fuzzy_palette_score(query: &str, candidate: &str) -> Option<i32> {
    let q: Vec<char> = query.trim().to_ascii_lowercase().chars().collect();
    if q.is_empty() {
        return Some(0);
    }
    let cand_lower = candidate.to_ascii_lowercase();
    if let Some(pos) = cand_lower.find(&q.iter().collect::<String>()) {
        return Some(200 - pos as i32);
    }
    let mut qi = 0usize;
    let mut score = 0i32;
    let mut prev_match = false;
    for ch in cand_lower.chars() {
        if qi < q.len() && ch == q[qi] {
            score += if prev_match { 12 } else { 5 };
            prev_match = true;
            qi += 1;
        } else {
            prev_match = false;
        }
    }
    if qi == q.len() {
        Some(score)
    } else {
        None
    }
}

pub struct CanopyForgeApp {
    pub seed: u64,
    pub rng: ChaCha8Rng,
    pub day_of_year: f32,
    pub elapsed_visual_sec: f32,
    pub sim_speed_mult: f32,
    pub paused: bool,
    pub climate_regime: ClimateRegime,
    pub tiles: Vec<TileState>,
    pub ledger: BudgetLedger,
    pub portfolio: StrategyPortfolio,
    pub populations: Vec<SpeciesPopulation>,
    pub crews: Vec<StewardCrew>,
    pub drones: Vec<CanopyLidarDrone>,
    pub visible_animals: Vec<VisibleAnimalAgent>,
    pub camera: IsometricZoomCamera,
    pub map_overlay: MapOverlay,
    pub selected_manual_tool: RemediationAction,
    pub inspect_only_click: bool,
    pub brush_3x3: bool,
    pub selected_tile: Option<(usize, usize)>,
    pub hovered_tile: Option<(usize, usize)>,
    pub followed_species: WildlifeSpecies,
    pub event_log: VecDeque<String>,
    pub last_feedback_banner: String,
    pub save_db: Option<SaveDatabase>,
    pub save_slot_name: String,
    pub cached_save_slots: Vec<(String, u32, f32)>,
    pub json_modal_open: bool,
    pub json_buffer: String,
    pub show_about_window: bool,
    pub show_command_palette: bool,
    pub command_palette_query: String,
    pub command_palette_selected: usize,
    pub palette_focus_pending: bool,
    pub show_audit_modal: bool,
    pub export_dir_input: String,
    pub last_scorecard: Option<CenturyRestorationScorecard>,
    static_mesh_cache: Option<StaticWatershedMeshCache>,
    pub static_mesh_dirty: bool,
    static_rebuild_accum_sec: f32,
}

impl CanopyForgeApp {
    pub fn new_simulation(seed: u64, db_path: Option<&str>) -> Self {
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let tiles = build_initial_watershed(&mut rng);
        let populations = initial_species_populations(&tiles);
        let (crews, drones) = build_initial_crews(&tiles);
        let mut visible_animals = Vec::new();
        sync_and_step_visible_animals(&mut visible_animals, &populations, &tiles, 0.016, &mut rng);

        let save_db = db_path.and_then(|p| SaveDatabase::open_wal(p).ok());
        let cached_save_slots = save_db
            .as_ref()
            .and_then(|db| db.list_slots().ok())
            .unwrap_or_default();
        let mut log = VecDeque::with_capacity(64);
        log.push_back(
            "Year 1 Spring: Watershed Stewardship Grant ($140,000) active. Set Strategy crews standing by."
                .to_string(),
        );

        Self {
            seed,
            rng,
            day_of_year: 15.0,
            elapsed_visual_sec: 0.0,
            sim_speed_mult: 1.0,
            paused: false,
            climate_regime: ClimateRegime::NormalMaritime,
            tiles,
            ledger: BudgetLedger::default(),
            portfolio: StrategyPortfolio::default(),
            populations,
            crews,
            drones,
            visible_animals,
            camera: IsometricZoomCamera::default(),
            map_overlay: MapOverlay::NaturalCanopy,
            selected_manual_tool: RemediationAction::InvasiveRemoval,
            inspect_only_click: false,
            brush_3x3: false,
            selected_tile: Some((4, 4)),
            hovered_tile: None,
            followed_species: WildlifeSpecies::NorthernSpottedOwl,
            event_log: log,
            last_feedback_banner:
                "Select a tile to inspect or apply Manual Interventions — mismatched areas safely yield No Effect."
                    .to_string(),
            save_db,
            save_slot_name: "watershed_slot_1".to_string(),
            cached_save_slots,
            json_modal_open: false,
            json_buffer: String::new(),
            show_about_window: false,
            show_command_palette: false,
            command_palette_query: String::new(),
            command_palette_selected: 0,
            palette_focus_pending: false,
            show_audit_modal: false,
            export_dir_input: "exports".to_string(),
            last_scorecard: None,
            static_mesh_cache: None,
            static_mesh_dirty: true,
            static_rebuild_accum_sec: 0.0,
        }
    }

    pub fn init_with_eframe(cc: &eframe::CreationContext<'_>, seed: u64) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::from_rgb(15, 22, 20);
        visuals.window_fill = egui::Color32::from_rgb(18, 27, 24);
        visuals.window_corner_radius = egui::CornerRadius::same(8);
        cc.egui_ctx.set_visuals(visuals);

        if let Some(wgpu_state) = cc.wgpu_render_state.as_ref() {
            let gpu_res = WatershedGpuResources::new(&wgpu_state.device, wgpu_state.target_format);
            wgpu_state
                .renderer
                .write()
                .callback_resources
                .insert(gpu_res);
        }

        Self::new_simulation(seed, Some("canopyforge_saves.sqlite3"))
    }

    pub fn ecological_recovery_index(&self) -> f32 {
        let tile_mean = self
            .tiles
            .iter()
            .map(TileState::ecological_tile_score)
            .sum::<f32>()
            / (self.tiles.len().max(1) as f32);
        let wildlife = compute_wildlife_recovery_score(&self.populations);
        (tile_mean * 0.68 + wildlife * 0.32).clamp(0.0, 100.0)
    }

    pub fn mean_burn_risk(&self) -> f32 {
        self.tiles.iter().map(|t| t.burn_risk).sum::<f32>() / (self.tiles.len().max(1) as f32)
    }

    pub fn mean_invasive_cover(&self) -> f32 {
        self.tiles.iter().map(|t| t.invasive_cover).sum::<f32>()
            / (self.tiles.len().max(1) as f32)
    }

    pub fn push_log(&mut self, msg: String) {
        if self.event_log.len() >= 48 {
            self.event_log.pop_front();
        }
        self.event_log.push_back(msg);
    }

    pub fn refresh_save_slots(&mut self) {
        if let Some(db) = &self.save_db {
            if let Ok(slots) = db.list_slots() {
                self.cached_save_slots = slots;
            }
        }
    }

    pub fn apply_camera_preset(&mut self, preset: CameraPreset) {
        let hint = if preset == CameraPreset::FollowWildlifeUnderstoryCam {
            self.visible_animals
                .iter()
                .find(|a| a.species == self.followed_species)
                .map(|a| a.pos)
        } else {
            self.selected_tile
                .map(|(x, z)| tile_world_center(&self.tiles[z * GRID_SIZE + x]))
        };
        self.camera.apply_preset(preset, hint);
    }

    pub fn preview_manual_intervention(
        &self,
        center_x: usize,
        center_z: usize,
    ) -> (usize, usize, f64, String) {
        let season = Season::from_day_of_year(self.day_of_year);
        let vpd = compute_current_vpd_kpa(self.day_of_year, self.climate_regime);
        let radius: isize = if self.brush_3x3 { 1 } else { 0 };
        let cost = self.selected_manual_tool.cost_per_tile();
        let mut effective_count = 0_usize;
        let mut neutral_count = 0_usize;
        let mut sample_reason = String::new();

        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let nx = center_x as isize + dx;
                let nz = center_z as isize + dz;
                if nx < 0 || nz < 0 || nx >= GRID_SIZE as isize || nz >= GRID_SIZE as isize {
                    continue;
                }
                let mut probe = self.tiles[(nz as usize) * GRID_SIZE + (nx as usize)].clone();
                match evaluate_and_apply_action(
                    self.selected_manual_tool,
                    &mut probe,
                    season,
                    vpd,
                    true,
                ) {
                    CompatibilityOutcome::EffectiveSuccess { summary, .. } => {
                        effective_count += 1;
                        if sample_reason.is_empty() {
                            sample_reason = summary;
                        }
                    }
                    CompatibilityOutcome::NoEffectNeutral { reason } => {
                        neutral_count += 1;
                        if sample_reason.is_empty() {
                            sample_reason = reason;
                        }
                    }
                }
            }
        }
        let total_cost = (effective_count as f64) * cost;
        (effective_count, neutral_count, total_cost, sample_reason)
    }

    pub fn apply_manual_intervention_at(&mut self, center_x: usize, center_z: usize) {
        let season = Season::from_day_of_year(self.day_of_year);
        let vpd = compute_current_vpd_kpa(self.day_of_year, self.climate_regime);
        let radius: isize = if self.brush_3x3 { 1 } else { 0 };

        let mut applied_count = 0_usize;
        let mut neutral_count = 0_usize;
        let mut sample_reason = String::new();

        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let nx = center_x as isize + dx;
                let nz = center_z as isize + dz;
                if nx < 0 || nz < 0 || nx >= GRID_SIZE as isize || nz >= GRID_SIZE as isize {
                    continue;
                }
                let idx = (nz as usize) * GRID_SIZE + (nx as usize);
                let cost = self.selected_manual_tool.cost_per_tile();
                if self.ledger.remaining_budget < cost {
                    self.last_feedback_banner = format!(
                        "Insufficient remaining grant budget (${:.0} needed for {}). Advance year for new grant!",
                        cost,
                        self.selected_manual_tool.short_label()
                    );
                    return;
                }

                let outcome = evaluate_and_apply_action(
                    self.selected_manual_tool,
                    &mut self.tiles[idx],
                    season,
                    vpd,
                    false,
                );
                match outcome {
                    CompatibilityOutcome::EffectiveSuccess { summary, .. } => {
                        self.ledger.remaining_budget -= cost;
                        self.ledger.spent_manual_this_year += cost;
                        applied_count += 1;
                        sample_reason = summary;
                    }
                    CompatibilityOutcome::NoEffectNeutral { reason } => {
                        neutral_count += 1;
                        if sample_reason.is_empty() {
                            sample_reason = reason;
                        }
                    }
                }
            }
        }

        if applied_count > 0 {
            self.static_mesh_dirty = true;
            let msg = format!(
                "Manual {} on {} sector(s): {} ({} neutral skipped, $0 wasted)",
                self.selected_manual_tool.short_label(),
                applied_count,
                sample_reason,
                neutral_count
            );
            self.last_feedback_banner = msg.clone();
            self.push_log(msg);
        } else {
            self.last_feedback_banner = format!(
                "No Effect (0 harm, $0 charged): {}",
                sample_reason
            );
        }
    }

    pub fn step_simulation(&mut self, dt_real_sec: f32) {
        self.elapsed_visual_sec += dt_real_sec;
        let follow_pos = self
            .visible_animals
            .iter()
            .find(|a| a.species == self.followed_species)
            .map(|a| a.pos);
        self.camera.step_smoothing(dt_real_sec, follow_pos);

        if self.paused {
            return;
        }

        let prev_season = Season::from_day_of_year(self.day_of_year);
        // At 1x speed, 1 ecological year (~365 days) passes in ~60 seconds (~6.08 days/sec)
        let dt_days = dt_real_sec * 6.08 * self.sim_speed_mult;
        self.day_of_year += dt_days;
        self.static_rebuild_accum_sec += dt_real_sec;

        let squirrel_bonus = self
            .populations
            .iter()
            .find(|p| p.species == WildlifeSpecies::NorthernFlyingSquirrel)
            .map(|p| (p.count / 120.0).clamp(0.0, 1.2))
            .unwrap_or(0.15);
        let beaver_bonus = self
            .populations
            .iter()
            .find(|p| p.species == WildlifeSpecies::NorthAmericanBeaver)
            .map(|p| (p.count / 18.0).clamp(0.0, 1.4))
            .unwrap_or(0.15);

        let (_active_fires, ignitions) = step_watershed_ecology(
            &mut self.tiles,
            dt_days,
            self.day_of_year,
            self.climate_regime,
            squirrel_bonus,
            beaver_bonus,
            &mut self.rng,
        );
        if ignitions > 0 {
            self.push_log(format!(
                "Wildfire Alert: {} dry-summer ignition/spread sector(s)! Shaded fuelbreaks & BDA creek refugia slowing front.",
                ignitions
            ));
        }

        let crew_events = step_crews_and_drones(
            &mut self.crews,
            &mut self.drones,
            &mut self.tiles,
            &self.portfolio,
            &mut self.ledger,
            self.day_of_year,
            self.climate_regime,
            dt_real_sec,
            self.sim_speed_mult,
        );
        let had_crew_events = !crew_events.is_empty();
        for ev in crew_events {
            self.push_log(ev);
        }
        if ignitions > 0
            || had_crew_events
            || Season::from_day_of_year(self.day_of_year) != prev_season
            || self.static_rebuild_accum_sec >= 0.25
        {
            self.static_mesh_dirty = true;
            self.static_rebuild_accum_sec = 0.0;
        }

        step_wildlife_populations(&mut self.populations, &self.tiles, dt_days);
        sync_and_step_visible_animals(
            &mut self.visible_animals,
            &self.populations,
            &self.tiles,
            dt_real_sec * self.sim_speed_mult.clamp(0.5, 3.0),
            &mut self.rng,
        );

        // Year rollover & success-scaled Stewardship Budget Grant
        if self.day_of_year >= DAYS_PER_YEAR {
            self.day_of_year -= DAYS_PER_YEAR;
            let eri = self.ecological_recovery_index();
            let wscore = compute_wildlife_recovery_score(&self.populations);
            let mbr = self.mean_burn_risk();
            let minv = self.mean_invasive_cover();
            let audit =
                self.ledger
                    .close_year_and_award_next_budget(eri, wscore, mbr, minv);

            // Cycle multi-year PNW climate oscillation naturally
            self.climate_regime = match self.ledger.current_year % 4 {
                0 => ClimateRegime::SevereVpdDrought,
                2 => ClimateRegime::WarmDrySummer,
                3 => ClimateRegime::CoolWetLaNina,
                _ => ClimateRegime::NormalMaritime,
            };

            self.push_log(format!(
                "=== Year {} Audit: ERI {:.1}/100 | Wildlife {:.1}% -> Year {} Grant Awarded: ${:.0} ({}) ===",
                audit.year,
                audit.ecological_recovery_index,
                audit.wildlife_recovery_score,
                self.ledger.current_year,
                audit.awarded_next_budget,
                self.climate_regime.label()
            ));
        }
    }

    pub fn export_save_state(&self, name: &str) -> SimulationSaveState {
        SimulationSaveState {
            schema_version: "canopyforge.v1".to_string(),
            save_name: name.to_string(),
            seed: self.seed,
            day_of_year: self.day_of_year,
            climate_regime: self.climate_regime,
            ledger: self.ledger.clone(),
            portfolio: self.portfolio.clone(),
            tiles: self.tiles.clone(),
            populations: self.populations.clone(),
            crews: self.crews.clone(),
            drones: self.drones.clone(),
        }
    }

    pub fn apply_save_state(&mut self, state: SimulationSaveState) {
        self.seed = state.seed;
        self.rng = ChaCha8Rng::seed_from_u64(state.seed);
        self.day_of_year = state.day_of_year;
        self.climate_regime = state.climate_regime;
        self.ledger = state.ledger;
        self.portfolio = state.portfolio;
        self.tiles = state.tiles;
        self.populations = state.populations;
        self.crews = state.crews;
        self.drones = state.drones;
        self.visible_animals.clear();
        sync_and_step_visible_animals(
            &mut self.visible_animals,
            &self.populations,
            &self.tiles,
            0.016,
            &mut self.rng,
        );
        self.static_mesh_dirty = true;
        self.refresh_save_slots();
    }

    pub fn jump_to_ecotype(&mut self, ecotype: Ecotype) {
        if let Some(tile) = self.tiles.iter().find(|t| t.ecotype == ecotype) {
            let coords = (tile.x, tile.z);
            let center = tile_world_center(tile);
            self.selected_tile = Some(coords);
            self.camera.apply_preset(CameraPreset::MidCanopyStandZoom, Some(center));
            self.last_feedback_banner = format!(
                "Jumped to {} at sector ({}, {})",
                ecotype.label(),
                coords.0,
                coords.1
            );
        }
    }

    pub fn export_century_scorecard_to_dir(&mut self) {
        let dir = if self.export_dir_input.trim().is_empty() {
            "exports"
        } else {
            self.export_dir_input.trim()
        };
        match export_century_scorecard_files(dir, self.seed, 100) {
            Ok((jp, cp, sc)) => {
                self.last_feedback_banner = format!(
                    "Exported 100-yr scorecard (Active vs Passive ERI {:+.2}) -> {} & {}",
                    sc.eri_advantage_active_vs_passive,
                    jp.display(),
                    cp.display()
                );
                self.last_scorecard = Some(sc);
            }
            Err(err) => {
                self.last_feedback_banner = format!("100-yr scorecard export error: {err}");
            }
        }
    }

    pub fn palette_commands() -> Vec<(&'static str, &'static str, CanopyPaletteAction)> {
        vec![
            ("Ecotype: Remnant Old-Growth Core", "Ecotypes", CanopyPaletteAction::JumpEcotype(Ecotype::OldGrowthCore)),
            ("Ecotype: Dense Second-Growth Plantation", "Ecotypes", CanopyPaletteAction::JumpEcotype(Ecotype::DenseSecondGrowth)),
            ("Ecotype: Incised Salmonid Creek Corridor", "Ecotypes", CanopyPaletteAction::JumpEcotype(Ecotype::RiparianSalmonCreek)),
            ("Ecotype: Degraded Clearcut Edge", "Ecotypes", CanopyPaletteAction::JumpEcotype(Ecotype::DegradedClearcutEdge)),
            ("Ecotype: Upland Oak-Conifer Meadow Ridge", "Ecotypes", CanopyPaletteAction::JumpEcotype(Ecotype::UplandMeadowRidge)),
            ("Action: Crosscut Thinning ($2,400)", "Stewardship", CanopyPaletteAction::SelectTool(RemediationAction::ThinningFromBelow)),
            ("Action: Prescribed Burn ($1,800)", "Stewardship", CanopyPaletteAction::SelectTool(RemediationAction::PrescribedUnderburn)),
            ("Action: Remove Invasives ($2,100)", "Stewardship", CanopyPaletteAction::SelectTool(RemediationAction::InvasiveRemoval)),
            ("Action: BDA & Large Wood ($3,200)", "Stewardship", CanopyPaletteAction::SelectTool(RemediationAction::InstallBdaAndLwd)),
            ("Action: Native Underplant ($2,600)", "Stewardship", CanopyPaletteAction::SelectTool(RemediationAction::NativeMycorrhizalPlanting)),
            ("Action: Snag & Owl Dens ($2,900)", "Stewardship", CanopyPaletteAction::SelectTool(RemediationAction::LegacySnagAndOwlCavity)),
            ("Action: Suppress / Wet ($1,500)", "Stewardship", CanopyPaletteAction::SelectTool(RemediationAction::FirebreakAndMoisten)),
            ("Stewardship: Toggle 1x1 Sector vs 3x3 Brush", "Stewardship", CanopyPaletteAction::ToggleBrush3x3),
            ("Stewardship: Toggle Inspect Only vs Apply Tool", "Stewardship", CanopyPaletteAction::ToggleInspectOnly),
            ("Season: Jump to Spring Freshet (Day 15)", "Seasons", CanopyPaletteAction::JumpSeasonDay(15)),
            ("Season: Jump to Summer VPD Peak (Day 145)", "Seasons", CanopyPaletteAction::JumpSeasonDay(145)),
            ("Season: Jump to Autumn Coho Spawn (Day 220)", "Seasons", CanopyPaletteAction::JumpSeasonDay(220)),
            ("Season: Jump to Winter Rain & Snowpack (Day 310)", "Seasons", CanopyPaletteAction::JumpSeasonDay(310)),
            ("Camera: Full Watershed Isometric (1)", "Views", CanopyPaletteAction::CameraPreset(CameraPreset::FullWatershedIsometric)),
            ("Camera: Mid-Canopy Stand Zoom (2)", "Views", CanopyPaletteAction::CameraPreset(CameraPreset::MidCanopyStandZoom)),
            ("Camera: Salmon & Beaver Creek Zoom (3)", "Views", CanopyPaletteAction::CameraPreset(CameraPreset::SalmonAndBeaverCreekZoom)),
            ("Camera: Follow Wildlife Understory (4)", "Views", CanopyPaletteAction::CameraPreset(CameraPreset::FollowWildlifeUnderstoryCam)),
            ("Simulation: Play / Pause (Space)", "Simulation", CanopyPaletteAction::TogglePause),
            ("Audit: Open Annual Audit & History Summary", "Audit", CanopyPaletteAction::ToggleAuditModal),
            ("Export: 100-Year Watershed Scenario Scorecard (JSON + CSV)", "Exports", CanopyPaletteAction::ExportCenturyScorecard),
            ("Export: Open Save JSON Schema Modal", "Exports", CanopyPaletteAction::OpenJsonSchema),
            ("Save: Quicksave Watershed to SQLite WAL", "Exports", CanopyPaletteAction::QuickSaveSqlite),
        ]
    }

    pub fn filtered_palette_commands(&self) -> Vec<(&'static str, &'static str, CanopyPaletteAction)> {
        let q = self.command_palette_query.trim();
        let all = Self::palette_commands();
        if q.is_empty() {
            return all;
        }
        let mut scored: Vec<(i32, (&'static str, &'static str, CanopyPaletteAction))> = all
            .into_iter()
            .filter_map(|item| {
                let hay = format!("{} {}", item.1, item.0);
                fuzzy_palette_score(q, &hay).map(|s| (s, item))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1 .0.cmp(b.1 .0)));
        scored.into_iter().map(|(_, item)| item).collect()
    }

    pub fn execute_palette_action(&mut self, action: CanopyPaletteAction) {
        match action {
            CanopyPaletteAction::JumpEcotype(eco) => self.jump_to_ecotype(eco),
            CanopyPaletteAction::SelectTool(tool) => {
                self.selected_manual_tool = tool;
                self.last_feedback_banner = format!("Selected manual tool: {}", tool.label());
            }
            CanopyPaletteAction::ToggleBrush3x3 => {
                self.brush_3x3 = !self.brush_3x3;
                self.last_feedback_banner = if self.brush_3x3 {
                    "Switched stewardship brush to 3x3 Watershed Brush".to_string()
                } else {
                    "Switched stewardship brush to 1x1 Sector".to_string()
                };
            }
            CanopyPaletteAction::ToggleInspectOnly => {
                self.inspect_only_click = !self.inspect_only_click;
                self.last_feedback_banner = if self.inspect_only_click {
                    "Switched click mode to Inspect Only".to_string()
                } else {
                    "Switched click mode to Apply Tool".to_string()
                };
            }
            CanopyPaletteAction::JumpSeasonDay(day) => {
                self.day_of_year = day as f32;
                self.static_mesh_dirty = true;
                let s = Season::from_day_of_year(self.day_of_year);
                self.last_feedback_banner = format!("Jumped to {} (Day {})", s.label(), day);
            }
            CanopyPaletteAction::CameraPreset(preset) => self.apply_camera_preset(preset),
            CanopyPaletteAction::TogglePause => {
                self.paused = !self.paused;
            }
            CanopyPaletteAction::ToggleAuditModal => {
                self.show_audit_modal = !self.show_audit_modal;
            }
            CanopyPaletteAction::ExportCenturyScorecard => {
                self.export_century_scorecard_to_dir();
            }
            CanopyPaletteAction::OpenJsonSchema => {
                self.json_buffer = self
                    .export_save_state("json_export")
                    .to_json_pretty()
                    .unwrap_or_default();
                self.json_modal_open = true;
            }
            CanopyPaletteAction::QuickSaveSqlite => {
                let eri = self.ecological_recovery_index();
                let snap = self.export_save_state("quicksave");
                if let Some(db) = &self.save_db {
                    let _ = db.save_slot("quicksave", eri, &snap);
                    self.refresh_save_slots();
                    self.last_feedback_banner =
                        "Saved watershed state to SQLite WAL slot 'quicksave'.".to_string();
                }
            }
        }
    }

    pub fn ui_step(&mut self, ctx: &egui::Context, interactive: bool) {
        let dt = if interactive {
            ctx.input(|i| i.stable_dt).clamp(0.001, 0.1)
        } else {
            1.0 / 60.0
        };

        if interactive {
            let wants_kb = ctx.wants_keyboard_input();
            let allow_hotkeys = !wants_kb && !self.json_modal_open && !self.show_command_palette;
            let mut key_preset: Option<CameraPreset> = None;
            let mut speed_delta: isize = 0;

            ctx.input(|i| {
                let cmd = i.modifiers.ctrl || i.modifiers.command;
                if cmd && i.key_pressed(egui::Key::K) {
                    if self.show_command_palette {
                        self.show_command_palette = false;
                    } else if !wants_kb && !self.json_modal_open {
                        self.show_command_palette = true;
                        self.command_palette_query.clear();
                        self.command_palette_selected = 0;
                        self.palette_focus_pending = true;
                    }
                }
                if i.key_pressed(egui::Key::F1) && !self.show_command_palette {
                    self.show_about_window = !self.show_about_window;
                }
                if i.key_pressed(egui::Key::Escape) {
                    if self.show_command_palette {
                        self.show_command_palette = false;
                    } else if self.show_audit_modal {
                        self.show_audit_modal = false;
                    } else if self.show_about_window {
                        self.show_about_window = false;
                    } else if self.json_modal_open {
                        self.json_modal_open = false;
                    }
                }
                if allow_hotkeys {
                    if i.key_pressed(egui::Key::Space) {
                        self.paused = !self.paused;
                    }
                    if i.key_down(egui::Key::Q) {
                        self.camera.orbit_by(-1.4 * dt, 0.0);
                    }
                    if i.key_down(egui::Key::E) {
                        self.camera.orbit_by(1.4 * dt, 0.0);
                    }
                    if i.key_down(egui::Key::W) {
                        self.camera.pan_planar(0.0, 18.0 * dt);
                    }
                    if i.key_down(egui::Key::S) {
                        self.camera.pan_planar(0.0, -18.0 * dt);
                    }
                    if i.key_down(egui::Key::A) {
                        self.camera.pan_planar(-18.0 * dt, 0.0);
                    }
                    if i.key_down(egui::Key::D) {
                        self.camera.pan_planar(18.0 * dt, 0.0);
                    }
                    if i.key_pressed(egui::Key::Num1) {
                        key_preset = Some(CameraPreset::FullWatershedIsometric);
                    } else if i.key_pressed(egui::Key::Num2) {
                        key_preset = Some(CameraPreset::MidCanopyStandZoom);
                    } else if i.key_pressed(egui::Key::Num3) {
                        key_preset = Some(CameraPreset::SalmonAndBeaverCreekZoom);
                    } else if i.key_pressed(egui::Key::Num4) {
                        key_preset = Some(CameraPreset::FollowWildlifeUnderstoryCam);
                    }
                    if i.key_pressed(egui::Key::OpenBracket) {
                        speed_delta = -1;
                    } else if i.key_pressed(egui::Key::CloseBracket) {
                        speed_delta = 1;
                    }
                }
            });

            if let Some(preset) = key_preset {
                self.apply_camera_preset(preset);
            }
            if speed_delta != 0 {
                let cur_idx = SPEED_STEPS
                    .iter()
                    .enumerate()
                    .min_by(|(_, a), (_, b)| {
                        (*a - self.sim_speed_mult)
                            .abs()
                            .partial_cmp(&(*b - self.sim_speed_mult).abs())
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .map(|(idx, _)| idx)
                    .unwrap_or(0);
                let next_idx = if speed_delta < 0 {
                    cur_idx.saturating_sub(1)
                } else {
                    (cur_idx + 1).min(SPEED_STEPS.len() - 1)
                };
                self.sim_speed_mult = SPEED_STEPS[next_idx];
                self.paused = false;
            }

            self.step_simulation(dt);
        }

        let eri = self.ecological_recovery_index();
        let season = Season::from_day_of_year(self.day_of_year);
        let vpd = compute_current_vpd_kpa(self.day_of_year, self.climate_regime);

        // Top Watershed Telemetry & Time/Budget Bar
        egui::TopBottomPanel::top("top_hud_bar")
            .frame(
                egui::Frame::default()
                    .fill(egui::Color32::from_rgb(13, 21, 18))
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.heading(
                        egui::RichText::new("CanopyForge")
                            .color(egui::Color32::from_rgb(154, 230, 180))
                            .strong(),
                    );
                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!(
                            "Year {} • {} (Day {:.0}/365)",
                            self.ledger.current_year,
                            season.label(),
                            self.day_of_year
                        ))
                        .strong(),
                    );
                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!(
                            "Budget: ${:.0} / ${:.0}",
                            self.ledger.remaining_budget, self.ledger.annual_grant
                        ))
                        .color(egui::Color32::from_rgb(246, 214, 115))
                        .strong(),
                    );
                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!("Recovery Index (ERI): {:.1}/100", eri))
                            .color(egui::Color32::from_rgb(110, 231, 183))
                            .strong(),
                    );
                    ui.separator();
                    ui.label(
                        egui::RichText::new(format!(
                            "VPD: {:.2} kPa ({})",
                            vpd,
                            self.climate_regime.label()
                        ))
                        .color(if vpd > 2.4 {
                            egui::Color32::from_rgb(251, 146, 60)
                        } else {
                            egui::Color32::from_rgb(147, 197, 253)
                        }),
                    );
                    ui.separator();

                    if ui
                        .button(if self.paused { "Resume" } else { "Pause" })
                        .clicked()
                    {
                        self.paused = !self.paused;
                    }
                    for &(spd, lbl) in &[(1.0, "1x"), (2.0, "2x"), (5.0, "5x"), (10.0, "10x")] {
                        if ui
                            .selectable_label(
                                !self.paused && (self.sim_speed_mult - spd).abs() < 0.05,
                                lbl,
                            )
                            .clicked()
                        {
                            self.paused = false;
                            self.sim_speed_mult = spd;
                        }
                    }

                    ui.separator();
                    if ui
                        .button("Save SQLite")
                        .on_hover_text("Save current watershed state into SQLite WAL slot 'quicksave'")
                        .clicked()
                    {
                        let snap = self.export_save_state("quicksave");
                        if let Some(db) = &self.save_db {
                            let _ = db.save_slot("quicksave", eri, &snap);
                            self.refresh_save_slots();
                            self.last_feedback_banner =
                                "Saved watershed state to SQLite WAL slot 'quicksave'.".to_string();
                        }
                    }
                    if ui
                        .button("Load")
                        .on_hover_text("Load SQLite WAL slot 'quicksave'")
                        .clicked()
                    {
                        if let Some(snap) = self
                            .save_db
                            .as_ref()
                            .and_then(|db| db.load_slot("quicksave").ok().flatten())
                        {
                            self.apply_save_state(snap);
                            self.last_feedback_banner =
                                "Loaded watershed state from SQLite WAL slot 'quicksave'."
                                    .to_string();
                        }
                    }
                    if ui.button("JSON Schema").clicked() {
                        self.json_buffer = self
                            .export_save_state("json_export")
                            .to_json_pretty()
                            .unwrap_or_default();
                        self.json_modal_open = true;
                    }
                    if ui
                        .button("100-Yr Scorecard")
                        .on_hover_text("Compare 100-year Passive vs Balanced vs Drought restoration scenarios (JSON + CSV)")
                        .clicked()
                    {
                        self.export_century_scorecard_to_dir();
                    }
                    if ui
                        .button("Palette (Ctrl+K)")
                        .on_hover_text("Open fuzzy command palette (Ctrl+K)")
                        .clicked()
                    {
                        self.show_command_palette = true;
                        self.command_palette_query.clear();
                        self.command_palette_selected = 0;
                        self.palette_focus_pending = true;
                    }
                    if ui.button("About").clicked() {
                        self.show_about_window = !self.show_about_window;
                    }
                });
            });

        // Bottom Field Event Log & Annual Audit History Strip
        egui::TopBottomPanel::bottom("bottom_log_panel")
            .frame(
                egui::Frame::default()
                    .fill(egui::Color32::from_rgb(13, 20, 18))
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Status:")
                            .color(egui::Color32::from_rgb(167, 243, 208))
                            .strong(),
                    );
                    ui.label(&self.last_feedback_banner);
                });
                ui.horizontal(|ui| {
                    if let Some(latest) = self.event_log.back() {
                        ui.small(
                            egui::RichText::new(latest)
                                .color(egui::Color32::from_rgb(185, 205, 195)),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("About CanopyForge").clicked() {
                            self.show_about_window = true;
                        }
                        ui.separator();
                        ui.small("Copyright CRV Science Network  |  GPL-3.0-or-later  |  CanopyForge v1.0.0");
                    });
                });
            });

        // Left Command Dock: "Set Strategy" Automation & Manual Interventions
        egui::SidePanel::left("left_strategy_dock")
            .resizable(true)
            .default_width(305.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("Set Strategy (Idle Crews)");
                    ui.small(
                        "Autonomous crews patrol compatible stands continuously. Mismatched areas never fail or take damage.",
                    );
                    ui.add_space(4.0);

                    for rule in &mut self.portfolio.rules {
                        egui::Frame::default()
                            .fill(egui::Color32::from_rgb(22, 34, 30))
                            .corner_radius(egui::CornerRadius::same(6))
                            .inner_margin(egui::Margin::same(6))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.checkbox(&mut rule.enabled, rule.action.short_label());
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.small(format!(
                                                "${:.0}/ha",
                                                rule.action.cost_per_tile()
                                            ));
                                        },
                                    );
                                });
                                ui.add(
                                    egui::Slider::new(&mut rule.weight, 0.0..=1.0)
                                        .text("Priority"),
                                );
                            });
                        ui.add_space(3.0);
                    }

                    ui.add(
                        egui::Slider::new(
                            &mut self.portfolio.manual_emergency_reserve_ratio,
                            0.0..=0.70,
                        )
                        .text("Manual Reserve"),
                    );

                    ui.separator();
                    ui.heading("Manual Interventions");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.inspect_only_click, false, "Click: Apply Tool")
                            .on_hover_text("Left-click on the 3D watershed selects and applies the selected remediation tool");
                        ui.selectable_value(&mut self.inspect_only_click, true, "Click: Inspect Only")
                            .on_hover_text("Left-click on the 3D watershed inspects tile telemetry without spending budget");
                    });
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.brush_3x3, false, "1x1 Sector");
                        ui.selectable_value(&mut self.brush_3x3, true, "3x3 Watershed Brush");
                    });
                    for action in RemediationAction::ALL_MANUAL {
                        let label = format!(
                            "{} (${:.0})",
                            action.short_label(),
                            action.cost_per_tile()
                        );
                        if ui
                            .selectable_label(self.selected_manual_tool == action, label)
                            .on_hover_text(action.label())
                            .clicked()
                        {
                            self.selected_manual_tool = action;
                        }
                    }

                    ui.separator();
                    ui.heading("Climate & Fire Drills");
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Dry Lightning Test").clicked() {
                            if let Some(target) = self
                                .tiles
                                .iter_mut()
                                .max_by(|a, b| {
                                    a.burn_risk
                                        .partial_cmp(&b.burn_risk)
                                        .unwrap_or(std::cmp::Ordering::Equal)
                                })
                            {
                                target.active_fire_intensity = 0.78;
                                self.static_mesh_dirty = true;
                                let coords = (target.x, target.z);
                                self.push_log(format!(
                                    "Simulated dry-lightning strike ignited high-risk ridge sector ({}, {})!",
                                    coords.0, coords.1
                                ));
                            }
                        }
                        if ui.button("Trigger VPD Drought").clicked() {
                            self.climate_regime = ClimateRegime::SevereVpdDrought;
                            self.day_of_year = 145.0; // Peak inside Season::Summer (91.25..182.5)
                            self.static_mesh_dirty = true;
                            self.push_log(
                                "Entered Severe Summer VPD Drought window (3.65 kPa peak drying)."
                                    .to_string(),
                            );
                        }
                    });

                    ui.separator();
                    ui.collapsing(
                        format!("SQLite Save Slots ({})", self.cached_save_slots.len()),
                        |ui| {
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.save_slot_name)
                                        .desired_width(155.0)
                                        .hint_text("slot_name"),
                                );
                                if ui.button("Save Slot").clicked() {
                                    let slot = if self.save_slot_name.trim().is_empty() {
                                        "watershed_slot_1".to_string()
                                    } else {
                                        self.save_slot_name.trim().to_string()
                                    };
                                    let snap = self.export_save_state(&slot);
                                    if let Some(db) = &self.save_db {
                                        if db.save_slot(&slot, eri, &snap).is_ok() {
                                            self.refresh_save_slots();
                                            self.last_feedback_banner = format!(
                                                "Saved Year {} (ERI {:.1}) to SQLite slot '{slot}'.",
                                                self.ledger.current_year, eri
                                            );
                                        }
                                    }
                                }
                            });
                            if self.cached_save_slots.is_empty() {
                                ui.small("No SQLite slots saved yet.");
                            } else {
                                let mut load_slot_name: Option<String> = None;
                                for (name, yr, slot_eri) in &self.cached_save_slots {
                                    ui.horizontal(|ui| {
                                        ui.small(format!("{name} (Yr {yr}, ERI {slot_eri:.1})"));
                                        if ui.small_button("Load").clicked() {
                                            load_slot_name = Some(name.clone());
                                        }
                                    });
                                }
                                if let Some(slot_name) = load_slot_name {
                                    if let Some(snap) = self
                                        .save_db
                                        .as_ref()
                                        .and_then(|db| db.load_slot(&slot_name).ok().flatten())
                                    {
                                        self.apply_save_state(snap);
                                        self.last_feedback_banner =
                                            format!("Loaded SQLite save slot '{slot_name}'.");
                                    }
                                }
                            }
                        },
                    );
                });
            });

        // Right Inspector: Selected Stand Telemetry & 6 PNW Wildlife Recovery Trackers
        egui::SidePanel::right("right_ecology_inspector")
            .resizable(true)
            .default_width(305.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("PNW Wildlife Recovery");
                    ui.small("Click a species to track it with Follow Wildlife Cam:");
                    ui.add_space(4.0);
                    for pop in &self.populations {
                        let is_followed = self.followed_species == pop.species
                            && self.camera.preset == CameraPreset::FollowWildlifeUnderstoryCam;
                        let resp = egui::Frame::default()
                            .fill(if is_followed {
                                egui::Color32::from_rgb(30, 56, 44)
                            } else {
                                egui::Color32::from_rgb(22, 33, 29)
                            })
                            .corner_radius(egui::CornerRadius::same(6))
                            .inner_margin(egui::Margin::same(6))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.strong(pop.species.common_name());
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let sign = if pop.annual_recovery_rate_pct >= 0.0 {
                                                "+"
                                            } else {
                                                ""
                                            };
                                            ui.colored_label(
                                                egui::Color32::from_rgb(134, 239, 172),
                                                format!(
                                                    "{}{:.1}%/yr",
                                                    sign, pop.annual_recovery_rate_pct
                                                ),
                                            );
                                        },
                                    );
                                });
                                if pop.species == WildlifeSpecies::NorthernSpottedOwl {
                                    ui.small(format!(
                                        "{} • 256 ha Habitat Idx: {:.1} / K: {:.1}",
                                        pop.species.scientific_name(),
                                        pop.count,
                                        pop.carrying_capacity
                                    ));
                                } else {
                                    ui.small(format!(
                                        "{} • Pop: {:.0} / K: {:.0} {}",
                                        pop.species.scientific_name(),
                                        pop.count,
                                        pop.carrying_capacity,
                                        pop.species.unit_label()
                                    ));
                                }
                                let frac = (pop.count / pop.carrying_capacity.max(1.0)).clamp(0.0, 1.0);
                                ui.add(egui::ProgressBar::new(frac).desired_height(6.0));
                            });
                        if resp.response.interact(egui::Sense::click()).clicked() {
                            self.followed_species = pop.species;
                            let pos = self
                                .visible_animals
                                .iter()
                                .find(|a| a.species == pop.species)
                                .map(|a| a.pos);
                            self.camera
                                .apply_preset(CameraPreset::FollowWildlifeUnderstoryCam, pos);
                        }
                        ui.add_space(3.0);
                    }

                    ui.separator();
                    ui.heading("Stand Sector Inspector");
                    if let Some((sx, sz)) = self.selected_tile {
                        let t = &self.tiles[sz * GRID_SIZE + sx];
                        ui.strong(format!("Sector ({}, {}) — {}", sx, sz, t.ecotype.label()));
                        ui.label(format!("Old-Growth Maturity: {:.0}%", t.old_growth_maturity * 100.0));
                        ui.label(format!("Canopy Biomass: {:.0}%", t.canopy_biomass * 100.0));
                        ui.label(format!("Native Sword-Fern Understory: {:.0}%", t.native_understory * 100.0));
                        ui.label(format!("Ladder Fuels: {:.0}%", t.ladder_fuels * 100.0));
                        ui.label(format!("Soil Moisture: {:.0}%", t.soil_moisture * 100.0));
                        ui.label(format!("Burn Risk: {:.0}%", t.burn_risk * 100.0));
                        ui.label(format!(
                            "Invasive Cover: {:.0}% ({})",
                            t.invasive_cover * 100.0,
                            t.dominant_invasive().label()
                        ));
                        if t.ecotype == crate::ecology::Ecotype::RiparianSalmonCreek {
                            ui.label(format!("BDA & Instream LWD: {:.0}%", t.bda_lwd_level * 100.0));
                        }
                        if ui
                            .button(format!(
                                "Apply {} Here",
                                self.selected_manual_tool.short_label()
                            ))
                            .clicked()
                        {
                            self.apply_manual_intervention_at(sx, sz);
                        }
                    }

                    ui.separator();
                    ui.heading("Annual Audit & Budget History");
                    if self.ledger.history.is_empty() {
                        ui.small("Complete Year 1 to record annual ERI & grant scaling history.");
                    } else {
                        let chart_w = ui.available_width().max(180.0);
                        let (chart_rect, _) =
                            ui.allocate_exact_size(egui::vec2(chart_w, 64.0), egui::Sense::hover());
                        let cp = ui.painter_at(chart_rect);
                        cp.rect_filled(
                            chart_rect,
                            egui::CornerRadius::same(4),
                            egui::Color32::from_rgb(12, 20, 18),
                        );
                        let max_budget = self
                            .ledger
                            .history
                            .iter()
                            .map(|r| r.awarded_next_budget)
                            .fold(200_000.0_f64, f64::max);
                        let n = self.ledger.history.len();
                        let bar_slot = chart_rect.width() / (n as f32);
                        let mut eri_pts = Vec::with_capacity(n);
                        for (idx, rec) in self.ledger.history.iter().enumerate() {
                            let x0 = chart_rect.left() + (idx as f32) * bar_slot + 2.0;
                            let bw = (bar_slot - 4.0).max(3.0);
                            let b_frac = (rec.awarded_next_budget / max_budget).clamp(0.05, 1.0) as f32;
                            let bar_h = b_frac * (chart_rect.height() - 10.0);
                            cp.rect_filled(
                                egui::Rect::from_min_size(
                                    egui::pos2(x0, chart_rect.bottom() - bar_h - 2.0),
                                    egui::vec2(bw, bar_h),
                                ),
                                egui::CornerRadius::same(2),
                                egui::Color32::from_rgba_unmultiplied(246, 214, 115, 165),
                            );
                            let ey = chart_rect.bottom()
                                - 4.0
                                - (rec.ecological_recovery_index / 100.0).clamp(0.0, 1.0)
                                    * (chart_rect.height() - 10.0);
                            eri_pts.push(egui::pos2(x0 + bw * 0.5, ey));
                        }
                        for pair in eri_pts.windows(2) {
                            cp.line_segment(
                                [pair[0], pair[1]],
                                egui::Stroke::new(1.8_f32, egui::Color32::from_rgb(110, 231, 183)),
                            );
                        }
                        for pt in &eri_pts {
                            cp.circle_filled(*pt, 2.3, egui::Color32::from_rgb(167, 243, 208));
                        }
                        cp.rect_stroke(
                            chart_rect,
                            egui::CornerRadius::same(4),
                            egui::Stroke::new(0.8_f32, egui::Color32::from_rgb(46, 72, 62)),
                            egui::StrokeKind::Inside,
                        );
                        if let Some(last) = self.ledger.history.last() {
                            ui.small(format!(
                                "Yr {}: ERI {:.1} | Wildlife {:.1}% | Grant ${:.0}",
                                last.year,
                                last.ecological_recovery_index,
                                last.wildlife_recovery_score,
                                last.awarded_next_budget
                            ));
                        }
                    }

                    ui.separator();
                    ui.heading("Field Crew Telemetry");
                    for crew in &self.crews {
                        ui.small(format!(
                            "#{} {}: {}",
                            crew.id,
                            crew.assigned_action.short_label(),
                            crew.last_status
                        ));
                    }
                });
            });

        // Center 3D Isometric + Zoom-In WGPU Viewport
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(egui::Color32::from_rgb(12, 18, 22)))
            .show(ctx, |ui| {
                // Floating Camera Zoom Preset & Overlay Bar
                ui.horizontal_wrapped(|ui| {
                    ui.strong("Camera [1-4]:");
                    for preset in [
                        CameraPreset::FullWatershedIsometric,
                        CameraPreset::MidCanopyStandZoom,
                        CameraPreset::SalmonAndBeaverCreekZoom,
                        CameraPreset::FollowWildlifeUnderstoryCam,
                    ] {
                        if ui
                            .selectable_label(self.camera.preset == preset, preset.label())
                            .clicked()
                        {
                            self.apply_camera_preset(preset);
                        }
                    }
                    ui.separator();
                    ui.strong("Layer:");
                    for ov in MapOverlay::ALL {
                        if ui
                            .selectable_label(self.map_overlay == ov, ov.label())
                            .clicked()
                        {
                            self.map_overlay = ov;
                            self.static_mesh_dirty = true;
                        }
                    }
                    ui.separator();
                    let mode_lbl = if self.inspect_only_click {
                        "Mode: Inspect Only"
                    } else {
                        "Mode: Apply Tool"
                    };
                    if ui
                        .selectable_label(self.inspect_only_click, mode_lbl)
                        .on_hover_text("Toggle left-click between Inspect Only and Apply Manual Intervention")
                        .clicked()
                    {
                        self.inspect_only_click = !self.inspect_only_click;
                    }
                });

                let avail = ui.available_size();
                let (rect, response) =
                    ui.allocate_exact_size(avail, egui::Sense::click_and_drag());

                // Mouse interactions: wheel zoom, right-drag orbit, middle-drag pan, left-click apply/select
                if response.hovered() {
                    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                    if scroll.abs() > 0.1 {
                        self.camera.zoom_by(scroll);
                    }
                }
                if response.dragged_by(egui::PointerButton::Secondary) {
                    let d = response.drag_delta();
                    self.camera.orbit_by(-d.x * 0.006, d.y * 0.005);
                }
                if response.dragged_by(egui::PointerButton::Middle) {
                    let d = response.drag_delta();
                    self.camera.pan_planar(-d.x * 0.08, d.y * 0.08);
                }

                if let Some(pos) = response.hover_pos() {
                    let local = Vec2::new(pos.x - rect.min.x, pos.y - rect.min.y);
                    let vp = Vec2::new(rect.width(), rect.height());
                    self.hovered_tile = self.camera.pick_tile(local, vp, &self.tiles);
                } else {
                    self.hovered_tile = None;
                }

                if response.clicked_by(egui::PointerButton::Primary) {
                    if let Some((tx, tz)) = self.hovered_tile {
                        self.selected_tile = Some((tx, tz));
                        if self.inspect_only_click {
                            let t = &self.tiles[tz * GRID_SIZE + tx];
                            let (eff, neu, cost, reason) = self.preview_manual_intervention(tx, tz);
                            self.last_feedback_banner = format!(
                                "Inspected ({}, {}) [{}] — Dry-run {}: {} effective / {} neutral (${:.0}): {}",
                                tx,
                                tz,
                                t.ecotype.label(),
                                self.selected_manual_tool.short_label(),
                                eff,
                                neu,
                                cost,
                                reason
                            );
                        } else {
                            self.apply_manual_intervention_at(tx, tz);
                        }
                    }
                }

                if let Some((hx, hz)) = self.hovered_tile {
                    let t = &self.tiles[hz * GRID_SIZE + hx];
                    let (eff, neu, cost, reason) = self.preview_manual_intervention(hx, hz);
                    let tool_name = self.selected_manual_tool.short_label();
                    let brush_txt = if self.brush_3x3 { "3x3" } else { "1x1" };
                    let eco_label = t.ecotype.label();
                    let burn_pct = t.burn_risk * 100.0;
                    let inv_pct = t.invasive_cover * 100.0;
                    let og_pct = t.old_growth_maturity * 100.0;
                    response.on_hover_ui(|ui| {
                        ui.strong(format!("Sector ({hx}, {hz}) — {eco_label}"));
                        ui.small(format!(
                            "Old Growth: {og_pct:.0}% | Burn Risk: {burn_pct:.0}% | Invasive: {inv_pct:.0}%"
                        ));
                        ui.separator();
                        if eff > 0 {
                            ui.colored_label(
                                egui::Color32::from_rgb(134, 239, 172),
                                format!(
                                    "Dry-Run ({brush_txt} {tool_name}): EffectiveSuccess ({eff} tile(s), {neu} neutral) | Cost: ${cost:.0}"
                                ),
                            );
                        } else {
                            ui.colored_label(
                                egui::Color32::from_rgb(203, 213, 225),
                                format!(
                                    "Dry-Run ({brush_txt} {tool_name}): NoEffectNeutral | Cost: $0"
                                ),
                            );
                        }
                        ui.small(reason);
                    });
                }

                // Submit WGPU 3D PaintCallback with seasonal sun angle, VPD/smoke fog & cached static geometry
                let ppp = ctx.pixels_per_point();
                let pw = ((rect.width() * ppp) as u32).max(64);
                let ph = ((rect.height() * ppp) as u32).max(64);

                let season_phase = ((self.day_of_year - 145.0) / DAYS_PER_YEAR) * std::f32::consts::TAU;
                let summer_t = (season_phase.cos() * 0.5 + 0.5).clamp(0.0, 1.0);
                let sun_elev = 0.56 + 0.24 * summer_t;
                let sun_dir = Vec3::new(
                    0.50 + 0.08 * season_phase.sin(),
                    sun_elev,
                    0.36 + 0.08 * (1.0 - summer_t),
                )
                .normalize_or_zero();
                let sun_color = Vec3::new(
                    1.16 + 0.26 * summer_t,
                    1.18 + 0.10 * summer_t,
                    1.24 - 0.22 * summer_t,
                );
                let fire_smoke = (self
                    .tiles
                    .iter()
                    .map(|t| t.active_fire_intensity)
                    .sum::<f32>()
                    / 4.0)
                    .clamp(0.0, 1.0);
                let fog_col = Vec3::new(
                    0.44 + fire_smoke * 0.22,
                    0.56 - fire_smoke * 0.08,
                    0.62 - fire_smoke * 0.20,
                );
                let fog_density = 0.0054 + fire_smoke * 0.0048;

                let eye = self.camera.eye_position();
                let uniforms = SceneFrameUniforms {
                    view_proj: self.camera.view_proj(rect.width() / rect.height().max(1.0)),
                    light_view_proj: build_sun_light_matrix(sun_dir),
                    camera_pos: Vec4::new(eye.x, eye.y, eye.z, 1.0),
                    sun_dir_and_time: Vec4::new(
                        sun_dir.x,
                        sun_dir.y,
                        sun_dir.z,
                        self.elapsed_visual_sec,
                    ),
                    sun_color_and_vpd: Vec4::new(sun_color.x, sun_color.y, sun_color.z, vpd),
                    fog_color_and_density: Vec4::new(fog_col.x, fog_col.y, fog_col.z, fog_density),
                };

                let needs_static_rebuild = self.static_mesh_dirty
                    || self
                        .static_mesh_cache
                        .as_ref()
                        .map(|c| c.overlay != self.map_overlay || c.season != season)
                        .unwrap_or(true);
                if needs_static_rebuild || self.static_mesh_cache.is_none() {
                    self.static_mesh_cache = Some(build_static_watershed_meshes(
                        &self.tiles,
                        self.map_overlay,
                        season,
                    ));
                    self.static_mesh_dirty = false;
                }
                let static_cache = self.static_mesh_cache.get_or_insert_with(|| {
                    build_static_watershed_meshes(&self.tiles, self.map_overlay, season)
                });
                let meshes = Arc::new(compose_watershed_scene_meshes(
                    static_cache,
                    &self.tiles,
                    &self.crews,
                    &self.drones,
                    &self.visible_animals,
                    self.selected_tile,
                    self.hovered_tile,
                    self.elapsed_visual_sec,
                ));

                let callback = egui_wgpu::Callback::new_paint_callback(
                    rect,
                    WatershedPaintCallback {
                        uniforms,
                        meshes,
                        pixel_size: (pw, ph),
                    },
                );
                ui.painter().add(callback);

                // Overlay colorbar legend when MapOverlay != NaturalCanopy
                if self.map_overlay != MapOverlay::NaturalCanopy && rect.width() > 280.0 && rect.height() > 140.0 {
                    let painter = ui.painter_at(rect);
                    let card = egui::Rect::from_min_size(
                        egui::pos2(rect.left() + 12.0, rect.bottom() - 68.0),
                        egui::vec2(246.0, 56.0),
                    );
                    painter.rect_filled(
                        card,
                        egui::CornerRadius::same(6),
                        egui::Color32::from_rgba_unmultiplied(12, 20, 18, 224),
                    );
                    painter.rect_stroke(
                        card,
                        egui::CornerRadius::same(6),
                        egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(72, 118, 98)),
                        egui::StrokeKind::Inside,
                    );
                    painter.text(
                        egui::pos2(card.left() + 10.0, card.top() + 13.0),
                        egui::Align2::LEFT_CENTER,
                        self.map_overlay.label(),
                        egui::FontId::proportional(11.0),
                        egui::Color32::from_rgb(226, 232, 240),
                    );
                    let bar = egui::Rect::from_min_size(
                        egui::pos2(card.left() + 10.0, card.top() + 24.0),
                        egui::vec2(card.width() - 20.0, 10.0),
                    );
                    let steps = 24_usize;
                    let seg_w = bar.width() / steps as f32;
                    for i in 0..steps {
                        let t = (i as f32 + 0.5) / steps as f32;
                        let c = overlay_legend_color(self.map_overlay, t);
                        let col = egui::Color32::from_rgb(
                            (c[0] * 255.0).round().clamp(0.0, 255.0) as u8,
                            (c[1] * 255.0).round().clamp(0.0, 255.0) as u8,
                            (c[2] * 255.0).round().clamp(0.0, 255.0) as u8,
                        );
                        painter.rect_filled(
                            egui::Rect::from_min_size(
                                egui::pos2(bar.left() + (i as f32) * seg_w, bar.top()),
                                egui::vec2(seg_w + 0.5, bar.height()),
                            ),
                            egui::CornerRadius::ZERO,
                            col,
                        );
                    }
                    painter.rect_stroke(
                        bar,
                        egui::CornerRadius::ZERO,
                        egui::Stroke::new(0.8_f32, egui::Color32::from_rgb(100, 116, 139)),
                        egui::StrokeKind::Inside,
                    );
                    let (t_min, t_mid, t_max) = match self.map_overlay {
                        MapOverlay::BurnRiskHeatmap => ("0% Low", "50%", "100% Extreme"),
                        MapOverlay::SoilMoistureVpd => ("0% Dry", "50%", "100% Wet"),
                        MapOverlay::InvasiveCover => ("0% Clean", "50%", "100% Dense"),
                        MapOverlay::OldGrowthMaturity => ("0% Young", "50%", "100% Old-Growth"),
                        MapOverlay::NaturalCanopy => ("", "", ""),
                    };
                    let tick_y = bar.bottom() + 10.0;
                    let tick_col = egui::Color32::from_rgb(186, 204, 198);
                    painter.text(
                        egui::pos2(bar.left(), tick_y),
                        egui::Align2::LEFT_CENTER,
                        t_min,
                        egui::FontId::proportional(9.5),
                        tick_col,
                    );
                    painter.text(
                        egui::pos2(bar.center().x, tick_y),
                        egui::Align2::CENTER_CENTER,
                        t_mid,
                        egui::FontId::proportional(9.5),
                        tick_col,
                    );
                    painter.text(
                        egui::pos2(bar.right(), tick_y),
                        egui::Align2::RIGHT_CENTER,
                        t_max,
                        egui::FontId::proportional(9.5),
                        tick_col,
                    );
                }
            });

        if self.json_modal_open {
            egui::Window::new("Cross-Compatible JSON Save Schema")
                .collapsible(false)
                .resizable(true)
                .default_width(520.0)
                .show(ctx, |ui| {
                    ui.small("Compatible 1:1 with CanopyForge Web Preview JSON Import/Export:");
                    egui::ScrollArea::vertical()
                        .max_height(280.0)
                        .show(ui, |ui| {
                            ui.text_edit_multiline(&mut self.json_buffer);
                        });
                    ui.horizontal(|ui| {
                        if ui.button("Import JSON State").clicked() {
                            if let Ok(parsed) = SimulationSaveState::from_json_str(&self.json_buffer)
                            {
                                self.apply_save_state(parsed);
                                self.last_feedback_banner =
                                    "Imported watershed save state from JSON.".to_string();
                                self.json_modal_open = false;
                            }
                        }
                        if ui.button("Close").clicked() {
                            self.json_modal_open = false;
                        }
                    });
                });
        }

        if self.show_about_window {
            let mut open = self.show_about_window;
            egui::Window::new("About CanopyForge")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.heading("CanopyForge v1.0.0");
                    ui.label("3D Isometric Pacific Northwest Old-Growth Ecosystem Recovery Simulator");
                    ui.separator();
                    ui.label(egui::RichText::new("Copyright CRV Science Network").strong());
                    ui.label("License: GPL-3.0-or-later (GNU General Public License v3.0 or later)");
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label("CRV Science Network:");
                        ui.hyperlink_to("https://crvscience.com", "https://crvscience.com");
                    });
                    ui.horizontal(|ui| {
                        ui.label("Open-Source Apps:");
                        ui.hyperlink_to("https://crvscience.com/apps", "https://crvscience.com/apps");
                    });
                    ui.horizontal(|ui| {
                        ui.label("Source Repository:");
                        ui.hyperlink_to(
                            "bpwhite/crvscience-canopyforge",
                            "https://github.com/bpwhite/crvscience-canopyforge",
                        );
                    });
                    ui.add_space(6.0);
                    if ui.button("Close").clicked() {
                        self.show_about_window = false;
                    }
                });
            if !open {
                self.show_about_window = false;
            }
        }

        if self.show_audit_modal {
            let mut open = self.show_audit_modal;
            egui::Window::new("Annual Watershed Audit & 100-Yr Scorecard")
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .default_width(560.0)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "Year {} | ERI {:.2}/100 | Mean Burn Risk {:.1}% | Grant ${:.0}",
                        self.ledger.current_year,
                        eri,
                        self.mean_burn_risk() * 100.0,
                        self.ledger.annual_grant
                    ));
                    ui.separator();
                    if self.ledger.history.is_empty() {
                        ui.small("No completed annual audits yet; run simulation across Year 1 rollover.");
                    } else {
                        egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                            for rec in &self.ledger.history {
                                ui.small(format!(
                                    "Year {}: ERI {:.1} | Wildlife {:.1}% | Next Grant ${:.0}",
                                    rec.year,
                                    rec.ecological_recovery_index,
                                    rec.wildlife_recovery_score,
                                    rec.awarded_next_budget
                                ));
                            }
                        });
                    }
                    ui.separator();
                    if ui.button("Export 100-Year Scenario Scorecard (JSON + CSV)").clicked() {
                        self.export_century_scorecard_to_dir();
                    }
                    if let Some(sc) = &self.last_scorecard {
                        ui.small(format!(
                            "Last Scorecard ({}-yr, seed {}): Active vs Passive ERI {:+.2}, Burn Risk reduction {:+.4}",
                            sc.horizon_years,
                            sc.seed,
                            sc.eri_advantage_active_vs_passive,
                            sc.burn_risk_reduction_active_vs_passive
                        ));
                    }
                });
            if !open {
                self.show_audit_modal = false;
            }
        }

        if self.show_command_palette {
            let filtered = self.filtered_palette_commands();
            if !filtered.is_empty() {
                self.command_palette_selected = self.command_palette_selected.min(filtered.len() - 1);
            } else {
                self.command_palette_selected = 0;
            }
            let (up_pressed, down_pressed, enter_pressed) = ctx.input(|i| {
                (
                    i.key_pressed(egui::Key::ArrowUp),
                    i.key_pressed(egui::Key::ArrowDown),
                    i.key_pressed(egui::Key::Enter),
                )
            });
            if up_pressed && !filtered.is_empty() {
                self.command_palette_selected = if self.command_palette_selected == 0 {
                    filtered.len() - 1
                } else {
                    self.command_palette_selected - 1
                };
            }
            if down_pressed && !filtered.is_empty() {
                self.command_palette_selected = (self.command_palette_selected + 1) % filtered.len();
            }
            let mut exec_act: Option<CanopyPaletteAction> = if enter_pressed {
                filtered.get(self.command_palette_selected).map(|(_, _, a)| *a)
            } else {
                None
            };
            let mut open = self.show_command_palette;
            egui::Window::new("Command Palette (Ctrl+K)")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_TOP, egui::Vec2::new(0.0, 56.0))
                .default_width(540.0)
                .show(ctx, |ui| {
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut self.command_palette_query)
                            .id(egui::Id::new("canopyforge_cmd_palette_input"))
                            .desired_width(f32::INFINITY)
                            .hint_text("Jump to ecotypes, stewardship actions, seasons, audit, or exports..."),
                    );
                    if self.palette_focus_pending {
                        edit.request_focus();
                        self.palette_focus_pending = false;
                    }
                    if edit.changed() {
                        self.command_palette_selected = 0;
                    }
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical().max_height(270.0).show(ui, |ui| {
                        if filtered.is_empty() {
                            ui.small("No matching commands.");
                        } else {
                            for (idx, (label, group, act)) in filtered.iter().enumerate() {
                                let selected = idx == self.command_palette_selected;
                                if ui
                                    .selectable_label(selected, format!("[{group}] {label}"))
                                    .clicked()
                                {
                                    exec_act = Some(*act);
                                }
                            }
                        }
                    });
                    ui.separator();
                    ui.small("Up/Down: Navigate  |  Enter: Execute  |  Esc: Close");
                });
            if !open {
                self.show_command_palette = false;
            }
            if let Some(act) = exec_act {
                self.show_command_palette = false;
                self.execute_palette_action(act);
            }
        }

        if interactive {
            ctx.request_repaint();
        }
    }
}

impl eframe::App for CanopyForgeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ui_step(ctx, true);
    }
}

struct NoopWaker;

impl std::task::Wake for NoopWaker {
    fn wake(self: std::sync::Arc<Self>) {}
}

fn block_on_future<F: std::future::Future>(fut: F) -> F::Output {
    let mut fut = std::pin::pin!(fut);
    let waker = std::task::Waker::from(std::sync::Arc::new(NoopWaker));
    let mut cx = std::task::Context::from_waker(&waker);
    loop {
        match fut.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(val) => return val,
            std::task::Poll::Pending => std::thread::yield_now(),
        }
    }
}

fn write_rgba_png(
    path: &std::path::Path,
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<(), Box<dyn std::error::Error>> {
    let expected = (width as usize) * (height as usize) * 4;
    if rgba.len() != expected {
        return Err(format!(
            "Unexpected RGBA buffer length {} (expected {})",
            rgba.len(),
            expected
        )
        .into());
    }
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let file = std::fs::File::create(path)?;
    let writer = std::io::BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    encoder.set_filter(png::Filter::Adaptive);
    let mut png_writer = encoder.write_header()?;
    png_writer.write_image_data(rgba)?;
    png_writer.finish()?;
    Ok(())
}

pub fn render_headless_screenshot(
    out_path: &std::path::Path,
    view: u8,
) -> Result<(), Box<dyn std::error::Error>> {
    use eframe::egui_wgpu::{wgpu, Renderer, ScreenDescriptor};

    let width: u32 = 1600;
    let height: u32 = 1000;
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = block_on_future(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok_or("No GPU adapter available for offscreen rendering")?;
    let (device, queue) = block_on_future(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("canopyforge-offscreen-device"),
            ..Default::default()
        },
        None,
    ))?;

    let format = wgpu::TextureFormat::Rgba8Unorm;
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("canopyforge-offscreen-target"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let bytes_per_row = width * 4;
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("canopyforge-offscreen-staging"),
        size: (bytes_per_row * height) as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let mut renderer = Renderer::new(&device, format, None, 1, false);
    let gpu_res = WatershedGpuResources::new(&device, format);
    renderer.callback_resources.insert(gpu_res);

    let egui_ctx = egui::Context::default();
    egui_ctx.set_pixels_per_point(1.0);
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgb(15, 22, 20);
    visuals.window_fill = egui::Color32::from_rgb(18, 27, 24);
    visuals.window_corner_radius = egui::CornerRadius::same(8);
    egui_ctx.set_visuals(visuals);

    let mut app = CanopyForgeApp::new_simulation(2026, None);
    app.sim_speed_mult = 10.0;
    while !(app.ledger.current_year >= 5 && app.day_of_year >= 165.0) {
        app.step_simulation(0.1);
    }

    // Ensure riparian creek tiles showcase active woven-willow BDAs & pools
    for t in &mut app.tiles {
        if t.ecotype == Ecotype::RiparianSalmonCreek {
            t.bda_lwd_level = t.bda_lwd_level.max(0.82);
            t.soil_moisture = t.soil_moisture.max(0.88);
        }
    }
    app.elapsed_visual_sec = 4.2;

    match view {
        2 => {
            let focus_tile = app
                .tiles
                .iter()
                .filter(|t| t.ecotype == Ecotype::DenseSecondGrowth || t.ecotype == Ecotype::UplandMeadowRidge)
                .find(|t| t.x == 10 && t.z == 10)
                .or_else(|| app.tiles.iter().find(|t| t.ecotype == Ecotype::OldGrowthCore));
            let hint = focus_tile.map(tile_world_center);
            if let Some(t) = focus_tile {
                app.selected_tile = Some((t.x, t.z));
            }
            app.camera.apply_preset(CameraPreset::MidCanopyStandZoom, hint);
            app.camera.desired_yaw_rad = 0.42;
            app.camera.desired_pitch_rad = 38.0_f32.to_radians();
            app.camera.desired_distance = 26.0;
        }
        3 => {
            let focus_tile = app
                .tiles
                .iter()
                .filter(|t| t.ecotype == Ecotype::RiparianSalmonCreek && t.x >= 11 && t.x <= 13)
                .max_by(|a, b| {
                    a.bda_lwd_level
                        .partial_cmp(&b.bda_lwd_level)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .or_else(|| {
                    app.tiles
                        .iter()
                        .find(|t| t.ecotype == Ecotype::RiparianSalmonCreek)
                });
            let hint = focus_tile.map(tile_world_center);
            if let Some(t) = focus_tile {
                app.selected_tile = Some((t.x, t.z));
            }
            app.camera.apply_preset(CameraPreset::SalmonAndBeaverCreekZoom, hint);
            // Look across the creek from the low clearcut (-Z, +X) bank so no tall trunks block the view
            app.camera.desired_yaw_rad = 2.65;
            app.camera.desired_pitch_rad = 41.0_f32.to_radians();
            app.camera.desired_distance = 17.5;
        }
        _ => {
            app.camera.apply_preset(CameraPreset::FullWatershedIsometric, None);
            app.camera.desired_pitch_rad = 38.0_f32.to_radians();
            app.camera.desired_distance = 76.0;
        }
    }
    app.map_overlay = MapOverlay::NaturalCanopy;
    app.camera.target = app.camera.desired_target;
    app.camera.yaw_rad = app.camera.desired_yaw_rad;
    app.camera.pitch_rad = app.camera.desired_pitch_rad;
    app.camera.distance = app.camera.desired_distance;
    app.camera.step_smoothing(2.0, None);

    let warmup_delays_ms = [40u64, 20, 20, 20];
    for (frame_idx, delay_ms) in warmup_delays_ms.into_iter().enumerate() {
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width as f32, height as f32),
            )),
            viewports: std::iter::once((
                egui::ViewportId::ROOT,
                egui::ViewportInfo {
                    native_pixels_per_point: Some(1.0),
                    ..Default::default()
                },
            ))
            .collect(),
            time: Some(frame_idx as f64 / 60.0),
            predicted_dt: 1.0 / 60.0,
            ..Default::default()
        };
        let full_output = egui_ctx.run(raw_input, |ctx| {
            app.ui_step(ctx, false);
        });
        for (id, image_delta) in &full_output.textures_delta.set {
            renderer.update_texture(&device, &queue, *id, image_delta);
        }
        let paint_jobs = egui_ctx.tessellate(full_output.shapes, full_output.pixels_per_point);
        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [width, height],
            pixels_per_point: 1.0,
        };
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("canopyforge-offscreen-encoder"),
        });
        let user_cmd_bufs = renderer.update_buffers(
            &device,
            &queue,
            &mut encoder,
            &paint_jobs,
            &screen_descriptor,
        );
        {
            let render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("canopyforge-offscreen-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &texture_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            let mut render_pass = render_pass.forget_lifetime();
            renderer.render(&mut render_pass, &paint_jobs, &screen_descriptor);
        }
        if frame_idx == 3 {
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &staging,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(bytes_per_row),
                        rows_per_image: Some(height),
                    },
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        }
        queue.submit(user_cmd_bufs.into_iter().chain(std::iter::once(encoder.finish())));
        device.poll(wgpu::Maintain::Wait);
        for id in &full_output.textures_delta.free {
            renderer.free_texture(id);
        }
    }

    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |res| {
        let _ = tx.send(res);
    });
    device.poll(wgpu::Maintain::Wait);
    rx.recv()??;
    let mapped = slice.get_mapped_range();
    write_rgba_png(out_path, width, height, &mapped)?;
    drop(mapped);
    staging.unmap();
    Ok(())
}
