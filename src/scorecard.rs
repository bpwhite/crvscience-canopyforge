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
    step_crews_and_drones, step_wildlife_populations, WildlifeSpecies,
};
use crate::ecology::{
    build_initial_watershed, step_watershed_ecology, ClimateRegime, TileState, DAYS_PER_YEAR,
};
use crate::strategies::{BudgetLedger, StrategyPortfolio};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScenarioPresetKind {
    PassiveNoStewardship,
    BalancedActiveStewardship,
    SevereDroughtActiveStewardship,
}

impl ScenarioPresetKind {
    pub const ALL: [Self; 3] = [
        Self::PassiveNoStewardship,
        Self::BalancedActiveStewardship,
        Self::SevereDroughtActiveStewardship,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::PassiveNoStewardship => "passive_baseline",
            Self::BalancedActiveStewardship => "balanced_stewardship",
            Self::SevereDroughtActiveStewardship => "severe_drought_stewardship",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::PassiveNoStewardship => "Passive Baseline (No Active Crews)",
            Self::BalancedActiveStewardship => "Balanced Active Stewardship",
            Self::SevereDroughtActiveStewardship => "Severe VPD Drought + Active Stewardship",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatershedScenarioResult {
    pub scenario_code: String,
    pub scenario_label: String,
    pub years_simulated: u32,
    pub initial_eri: f32,
    pub final_eri: f32,
    pub eri_delta: f32,
    pub final_wildlife_score: f32,
    pub final_burn_risk: f32,
    pub final_invasive_cover: f32,
    pub final_old_growth_maturity: f32,
    pub owl_habitat_index: f32,
    pub coho_salmon_count: f32,
    pub cumulative_grant_usd: f64,
    pub decadal_eri_trajectory: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CenturyRestorationScorecard {
    pub schema_version: String,
    pub seed: u64,
    pub horizon_years: u32,
    pub eri_advantage_active_vs_passive: f32,
    pub burn_risk_reduction_active_vs_passive: f32,
    pub scenarios: Vec<WatershedScenarioResult>,
}

fn compute_eri(tiles: &[TileState], pops: &[crate::agents::SpeciesPopulation]) -> f32 {
    let tile_mean = tiles
        .iter()
        .map(TileState::ecological_tile_score)
        .sum::<f32>()
        / (tiles.len().max(1) as f32);
    let wildlife = compute_wildlife_recovery_score(pops);
    (tile_mean * 0.68 + wildlife * 0.32).clamp(0.0, 100.0)
}

fn run_single_watershed_scenario(
    kind: ScenarioPresetKind,
    seed: u64,
    years: u32,
) -> WatershedScenarioResult {
    let years = years.clamp(1, 200);
    let mut rng = ChaCha8Rng::seed_from_u64(seed);
    let mut tiles = build_initial_watershed(&mut rng);
    let mut pops = initial_species_populations(&tiles);
    let (mut crews, mut drones) = build_initial_crews(&tiles);
    let mut ledger = BudgetLedger::default();
    let mut portfolio = StrategyPortfolio::default();
    let mut climate = match kind {
        ScenarioPresetKind::SevereDroughtActiveStewardship => ClimateRegime::SevereVpdDrought,
        _ => ClimateRegime::NormalMaritime,
    };

    if kind == ScenarioPresetKind::PassiveNoStewardship {
        for rule in &mut portfolio.rules {
            rule.enabled = false;
        }
    }

    let initial_eri = compute_eri(&tiles, &pops);
    let mut decadal_eri = Vec::new();
    let mut cumulative_grant_usd = ledger.annual_grant;

    let steps_per_year = 24_u32;
    let dt_days = DAYS_PER_YEAR / (steps_per_year as f32);
    let dt_real_sec = dt_days / (6.08 * 10.0);

    for yr in 1..=years {
        for step_idx in 0..steps_per_year {
            let doy = (step_idx as f32) * dt_days + 7.5;
            let _ = step_watershed_ecology(
                &mut tiles,
                dt_days,
                doy,
                climate,
                0.55,
                0.55,
                &mut rng,
            );
            let _ = step_crews_and_drones(
                &mut crews,
                &mut drones,
                &mut tiles,
                &portfolio,
                &mut ledger,
                doy,
                climate,
                dt_real_sec,
                10.0,
            );
            step_wildlife_populations(&mut pops, &tiles, dt_days);
        }

        let eri = compute_eri(&tiles, &pops);
        let wscore = compute_wildlife_recovery_score(&pops);
        let mbr = tiles.iter().map(|t| t.burn_risk).sum::<f32>() / (tiles.len().max(1) as f32);
        let minv =
            tiles.iter().map(|t| t.invasive_cover).sum::<f32>() / (tiles.len().max(1) as f32);
        let audit = ledger.close_year_and_award_next_budget(eri, wscore, mbr, minv);
        cumulative_grant_usd += audit.awarded_next_budget;

        if kind != ScenarioPresetKind::SevereDroughtActiveStewardship {
            climate = match yr % 4 {
                0 => ClimateRegime::WarmDrySummer,
                3 => ClimateRegime::CoolWetLaNina,
                _ => ClimateRegime::NormalMaritime,
            };
        }

        if yr % 10 == 0 || yr == years {
            decadal_eri.push((eri * 100.0).round() / 100.0);
        }
    }

    let final_eri = compute_eri(&tiles, &pops);
    let final_wildlife_score = compute_wildlife_recovery_score(&pops);
    let n = tiles.len().max(1) as f32;
    let final_burn_risk = tiles.iter().map(|t| t.burn_risk).sum::<f32>() / n;
    let final_invasive_cover = tiles.iter().map(|t| t.invasive_cover).sum::<f32>() / n;
    let final_old_growth_maturity = tiles.iter().map(|t| t.old_growth_maturity).sum::<f32>() / n;
    let owl_habitat_index = pops
        .iter()
        .find(|p| p.species == WildlifeSpecies::NorthernSpottedOwl)
        .map(|p| p.count)
        .unwrap_or(0.0);
    let coho_salmon_count = pops
        .iter()
        .find(|p| p.species == WildlifeSpecies::OregonCoastCohoSalmon)
        .map(|p| p.count)
        .unwrap_or(0.0);

    WatershedScenarioResult {
        scenario_code: kind.code().to_string(),
        scenario_label: kind.label().to_string(),
        years_simulated: years,
        initial_eri: (initial_eri * 100.0).round() / 100.0,
        final_eri: (final_eri * 100.0).round() / 100.0,
        eri_delta: ((final_eri - initial_eri) * 100.0).round() / 100.0,
        final_wildlife_score: (final_wildlife_score * 100.0).round() / 100.0,
        final_burn_risk: (final_burn_risk * 10000.0).round() / 10000.0,
        final_invasive_cover: (final_invasive_cover * 10000.0).round() / 10000.0,
        final_old_growth_maturity: (final_old_growth_maturity * 10000.0).round() / 10000.0,
        owl_habitat_index: (owl_habitat_index * 100.0).round() / 100.0,
        coho_salmon_count: (coho_salmon_count * 100.0).round() / 100.0,
        cumulative_grant_usd: cumulative_grant_usd.round(),
        decadal_eri_trajectory: decadal_eri,
    }
}

/// Evaluates a 3-scenario (default `100`-year) watershed restoration comparison scorecard.
pub fn evaluate_century_restoration_scorecard(
    seed: u64,
    horizon_years: u32,
) -> CenturyRestorationScorecard {
    let horizon = horizon_years.clamp(1, 200);
    let scenarios: Vec<WatershedScenarioResult> = ScenarioPresetKind::ALL
        .into_iter()
        .map(|kind| run_single_watershed_scenario(kind, seed, horizon))
        .collect();

    let passive_eri = scenarios
        .first()
        .map(|s| s.final_eri)
        .unwrap_or(0.0);
    let active_eri = scenarios
        .get(1)
        .map(|s| s.final_eri)
        .unwrap_or(passive_eri);
    let passive_burn = scenarios
        .first()
        .map(|s| s.final_burn_risk)
        .unwrap_or(0.0);
    let active_burn = scenarios
        .get(1)
        .map(|s| s.final_burn_risk)
        .unwrap_or(passive_burn);

    CenturyRestorationScorecard {
        schema_version: "canopyforge_century_scorecard_v1".to_string(),
        seed,
        horizon_years: horizon,
        eri_advantage_active_vs_passive: ((active_eri - passive_eri) * 100.0).round() / 100.0,
        burn_risk_reduction_active_vs_passive: ((passive_burn - active_burn) * 10000.0).round()
            / 10000.0,
        scenarios,
    }
}

pub fn csv_escape(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

/// Formats a [`CenturyRestorationScorecard`] as an RFC 4180 CSV scorecard table.
pub fn format_century_scorecard_csv(scorecard: &CenturyRestorationScorecard) -> String {
    let mut out = String::from(
        "scenario_code,scenario_label,seed,horizon_years,initial_eri,final_eri,eri_delta,final_wildlife_score,final_burn_risk,final_invasive_cover,final_old_growth_maturity,owl_habitat_index,coho_salmon_count,cumulative_grant_usd,decadal_eri_trajectory\n",
    );
    for s in &scorecard.scenarios {
        let traj = s
            .decadal_eri_trajectory
            .iter()
            .map(|v| format!("{v:.2}"))
            .collect::<Vec<_>>()
            .join(";");
        out.push_str(&format!(
            "{},{},{},{},{:.2},{:.2},{:.2},{:.2},{:.4},{:.4},{:.4},{:.2},{:.2},{:.0},{}\n",
            csv_escape(&s.scenario_code),
            csv_escape(&s.scenario_label),
            scorecard.seed,
            s.years_simulated,
            s.initial_eri,
            s.final_eri,
            s.eri_delta,
            s.final_wildlife_score,
            s.final_burn_risk,
            s.final_invasive_cover,
            s.final_old_growth_maturity,
            s.owl_habitat_index,
            s.coho_salmon_count,
            s.cumulative_grant_usd,
            csv_escape(&traj),
        ));
    }
    out
}

/// Evaluates and writes `canopyforge_100yr_scorecard.json` and `.csv` into `out_dir`.
pub fn export_century_scorecard_files(
    out_dir: impl AsRef<Path>,
    seed: u64,
    horizon_years: u32,
) -> Result<(PathBuf, PathBuf, CenturyRestorationScorecard), String> {
    let dir = out_dir.as_ref();
    fs::create_dir_all(dir).map_err(|e| format!("Failed creating export dir: {e}"))?;
    let scorecard = evaluate_century_restoration_scorecard(seed, horizon_years);
    let json_path = dir.join("canopyforge_100yr_scorecard.json");
    let csv_path = dir.join("canopyforge_100yr_scorecard.csv");

    let json_txt = serde_json::to_string_pretty(&scorecard)
        .map_err(|e| format!("Scorecard JSON serialize error: {e}"))?;
    fs::write(&json_path, json_txt).map_err(|e| format!("Write scorecard JSON failed: {e}"))?;
    let csv_txt = format_century_scorecard_csv(&scorecard);
    fs::write(&csv_path, csv_txt).map_err(|e| format!("Write scorecard CSV failed: {e}"))?;

    Ok((json_path, csv_path, scorecard))
}
