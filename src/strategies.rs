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

use crate::ecology::{Ecotype, Season, TileState};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RemediationAction {
    ThinningFromBelow,
    PrescribedUnderburn,
    InvasiveRemoval,
    InstallBdaAndLwd,
    NativeMycorrhizalPlanting,
    LegacySnagAndOwlCavity,
    FirebreakAndMoisten,
}

impl RemediationAction {
    pub const ALL_STRATEGIES: [Self; 6] = [
        Self::ThinningFromBelow,
        Self::PrescribedUnderburn,
        Self::InvasiveRemoval,
        Self::InstallBdaAndLwd,
        Self::NativeMycorrhizalPlanting,
        Self::LegacySnagAndOwlCavity,
    ];

    pub const ALL_MANUAL: [Self; 7] = [
        Self::ThinningFromBelow,
        Self::PrescribedUnderburn,
        Self::InvasiveRemoval,
        Self::InstallBdaAndLwd,
        Self::NativeMycorrhizalPlanting,
        Self::LegacySnagAndOwlCavity,
        Self::FirebreakAndMoisten,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::ThinningFromBelow => "Variable-Density Thinning from Below",
            Self::PrescribedUnderburn => "Cultural & Prescribed Underburn",
            Self::InvasiveRemoval => "Invasive Grubbing & Ivy Ringing",
            Self::InstallBdaAndLwd => "Beaver Dam Analogues (BDA) & LWD",
            Self::NativeMycorrhizalPlanting => "Multi-Cohort Replant & Mycorrhizae",
            Self::LegacySnagAndOwlCavity => "Legacy Snag & Owl/Murrelet Cavities",
            Self::FirebreakAndMoisten => "Emergency Fuelbreak & Foliar Wetting",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::ThinningFromBelow => "Crosscut Thinning",
            Self::PrescribedUnderburn => "Prescribed Burn",
            Self::InvasiveRemoval => "Remove Invasives",
            Self::InstallBdaAndLwd => "BDA & Large Wood",
            Self::NativeMycorrhizalPlanting => "Native Underplant",
            Self::LegacySnagAndOwlCavity => "Snag & Owl Dens",
            Self::FirebreakAndMoisten => "Suppress / Wet",
        }
    }

    pub fn cost_per_tile(self) -> f64 {
        match self {
            Self::ThinningFromBelow => 2_400.0,
            Self::PrescribedUnderburn => 1_800.0,
            Self::InvasiveRemoval => 2_100.0,
            Self::InstallBdaAndLwd => 3_200.0,
            Self::NativeMycorrhizalPlanting => 2_600.0,
            Self::LegacySnagAndOwlCavity => 2_900.0,
            Self::FirebreakAndMoisten => 1_500.0,
        }
    }
}

/// Core Non-Punitive Ecological Invariant:
/// Remediation strategies and manual interventions NEVER "fail" or cause ecological harm.
/// Depending on ecotype and local site condition, they yield either `EffectiveSuccess`
/// or `NoEffectNeutral` with clear scientific rationale.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CompatibilityOutcome {
    EffectiveSuccess {
        summary: String,
        ecological_gain: f32,
    },
    NoEffectNeutral {
        reason: String,
    },
}

impl CompatibilityOutcome {
    pub fn is_effective(&self) -> bool {
        matches!(self, Self::EffectiveSuccess { .. })
    }
}

pub fn evaluate_and_apply_action(
    action: RemediationAction,
    tile: &mut TileState,
    season: Season,
    current_vpd_kpa: f32,
    dry_run_only: bool,
) -> CompatibilityOutcome {
    match action {
        RemediationAction::ThinningFromBelow => {
            if matches!(
                tile.ecotype,
                Ecotype::RiparianSalmonCreek | Ecotype::DegradedClearcutEdge
            ) {
                return CompatibilityOutcome::NoEffectNeutral {
                    reason: format!(
                        "No overstocked conifer ladder fuels in {} — zero ecological harm.",
                        tile.ecotype.label()
                    ),
                };
            }
            if tile.ladder_fuels <= 0.20 {
                return CompatibilityOutcome::NoEffectNeutral {
                    reason: "Stand already has open variable-density spacing and low ladder fuels."
                        .to_string(),
                };
            }
            let delta = (tile.ladder_fuels - 0.16).min(0.34);
            if !dry_run_only {
                tile.ladder_fuels = (tile.ladder_fuels - delta).max(0.12);
                tile.old_growth_maturity = (tile.old_growth_maturity + 0.045).min(1.0);
                tile.native_understory = (tile.native_understory + 0.05).min(1.0);
                tile.recompute_burn_risk(current_vpd_kpa);
            }
            CompatibilityOutcome::EffectiveSuccess {
                summary:
                    "Crosscut sawyers removed small ladder-fuel stems, accelerating large conifer crown release."
                        .to_string(),
                ecological_gain: delta,
            }
        }
        RemediationAction::PrescribedUnderburn => {
            if tile.ecotype == Ecotype::RiparianSalmonCreek {
                return CompatibilityOutcome::NoEffectNeutral {
                    reason: "Wetted salmonid creek channel does not carry surface underburns."
                        .to_string(),
                };
            }
            if tile.active_fire_intensity > 0.05 {
                return CompatibilityOutcome::NoEffectNeutral {
                    reason: "Active wildfire already burning here — use Fuelbreak & Foliar Wetting."
                        .to_string(),
                };
            }
            if current_vpd_kpa > 2.65 && season == Season::Summer {
                return CompatibilityOutcome::NoEffectNeutral {
                    reason:
                        "Burn window paused due to high summer VPD (>2.65 kPa) — crew safely holds ignition."
                            .to_string(),
                };
            }
            if tile.ladder_fuels <= 0.18 && tile.scotch_broom_cover <= 0.05 {
                return CompatibilityOutcome::NoEffectNeutral {
                    reason: "Fine surface litter is already low — stand needs no underburn right now."
                        .to_string(),
                };
            }
            let fuel_drop = (tile.ladder_fuels - 0.12).max(0.0).min(0.28);
            let broom_drop = tile.scotch_broom_cover.min(0.22);
            if !dry_run_only {
                tile.ladder_fuels = (tile.ladder_fuels - fuel_drop).max(0.10);
                tile.scotch_broom_cover = (tile.scotch_broom_cover - broom_drop).max(0.0);
                tile.sync_invasive_total();
                tile.native_understory = (tile.native_understory + 0.075).min(1.0);
                tile.mycorrhizal_index = (tile.mycorrhizal_index + 0.03).min(1.0);
                tile.recompute_burn_risk(current_vpd_kpa);
            }
            CompatibilityOutcome::EffectiveSuccess {
                summary:
                    "Low-intensity planned underburn consumed surface litter & broom seeds, stimulating native herbs."
                        .to_string(),
                ecological_gain: fuel_drop + broom_drop + 0.06,
            }
        }
        RemediationAction::InvasiveRemoval => {
            if tile.invasive_cover < 0.03 {
                return CompatibilityOutcome::NoEffectNeutral {
                    reason:
                        "Understory already clean (<3% invasive cover) — native sword ferns & salal intact."
                            .to_string(),
                };
            }
            let removed = tile.invasive_cover.min(0.42);
            if !dry_run_only {
                tile.blackberry_cover = (tile.blackberry_cover - 0.28).max(0.0);
                tile.scotch_broom_cover = (tile.scotch_broom_cover - 0.28).max(0.0);
                tile.english_ivy_cover = (tile.english_ivy_cover - 0.32).max(0.0);
                tile.sync_invasive_total();
                tile.native_understory = (tile.native_understory + 0.08).min(1.0);
                tile.recompute_burn_risk(current_vpd_kpa);
            }
            CompatibilityOutcome::EffectiveSuccess {
                summary:
                    "Grubbed blackberry/broom root crowns and survival-ringed English ivy off conifer trunks."
                        .to_string(),
                ecological_gain: removed,
            }
        }
        RemediationAction::InstallBdaAndLwd => {
            if tile.ecotype != Ecotype::RiparianSalmonCreek {
                return CompatibilityOutcome::NoEffectNeutral {
                    reason: format!(
                        "Beaver Dam Analogues & instream LWD require an active stream channel (no effect on {})." ,
                        tile.ecotype.label()
                    ),
                };
            }
            if tile.bda_lwd_level >= 0.94 {
                return CompatibilityOutcome::NoEffectNeutral {
                    reason:
                        "Stream reach already features deep cold beaver pools and complex root-wad LWD."
                            .to_string(),
                };
            }
            let gain = (1.0 - tile.bda_lwd_level).min(0.36);
            if !dry_run_only {
                tile.bda_lwd_level = (tile.bda_lwd_level + gain).min(1.0);
                tile.soil_moisture = (tile.soil_moisture + 0.18).min(0.98);
                tile.native_understory = (tile.native_understory + 0.07).min(1.0);
                tile.recompute_burn_risk(current_vpd_kpa);
            }
            CompatibilityOutcome::EffectiveSuccess {
                summary:
                    "Installed woven willow BDA & root-wad LWD — cold pool formed, raising hyporheic water table for Coho."
                        .to_string(),
                ecological_gain: gain,
            }
        }
        RemediationAction::NativeMycorrhizalPlanting => {
            if tile.ecotype == Ecotype::RiparianSalmonCreek && tile.bda_lwd_level > 0.65 {
                return CompatibilityOutcome::NoEffectNeutral {
                    reason: "Deep open beaver pond channel cannot be planted with upland conifer seedlings."
                        .to_string(),
                };
            }
            if tile.canopy_biomass > 0.90 && tile.native_understory > 0.88 && tile.mycorrhizal_index > 0.88 {
                return CompatibilityOutcome::NoEffectNeutral {
                    reason: "Old-growth multi-cohort canopy and sword-fern/mycorrhizal mat already at carrying capacity."
                        .to_string(),
                };
            }
            let gain = ((1.0 - tile.native_understory) * 0.35 + (1.0 - tile.mycorrhizal_index) * 0.25)
                .clamp(0.08, 0.38);
            if !dry_run_only {
                tile.canopy_biomass = (tile.canopy_biomass + 0.11).min(1.0);
                tile.native_understory = (tile.native_understory + 0.15).min(1.0);
                tile.mycorrhizal_index = (tile.mycorrhizal_index + 0.18).min(1.0);
                tile.old_growth_maturity = (tile.old_growth_maturity + 0.05).min(1.0);
                tile.recompute_burn_risk(current_vpd_kpa);
            }
            CompatibilityOutcome::EffectiveSuccess {
                summary:
                    "Underplanted Western redcedar, hemlock & sword fern with hypogeous mycorrhizal inoculum."
                        .to_string(),
                ecological_gain: gain,
            }
        }
        RemediationAction::LegacySnagAndOwlCavity => {
            if !matches!(
                tile.ecotype,
                Ecotype::OldGrowthCore | Ecotype::DenseSecondGrowth | Ecotype::UplandMeadowRidge
            ) || tile.canopy_biomass < 0.38
            {
                return CompatibilityOutcome::NoEffectNeutral {
                    reason:
                        "Supplementary owl/murrelet platforms & flying-squirrel dens require mature tree boles (>38% canopy)."
                            .to_string(),
                };
            }
            if tile.snag_cavity_density >= 0.92 {
                return CompatibilityOutcome::NoEffectNeutral {
                    reason: "Stand already has abundant legacy snags, mossy limb platforms & den cavities."
                        .to_string(),
                };
            }
            let gain = (1.0 - tile.snag_cavity_density).min(0.32);
            if !dry_run_only {
                tile.snag_cavity_density = (tile.snag_cavity_density + gain).min(1.0);
                tile.old_growth_maturity = (tile.old_growth_maturity + 0.06).min(1.0);
            }
            CompatibilityOutcome::EffectiveSuccess {
                summary:
                    "Installed supplementary flying-squirrel dens & protected mossy Spotted Owl / Murrelet nest platforms."
                        .to_string(),
                ecological_gain: gain,
            }
        }
        RemediationAction::FirebreakAndMoisten => {
            if tile.active_fire_intensity <= 0.01 && tile.burn_risk < 0.22 {
                return CompatibilityOutcome::NoEffectNeutral {
                    reason: "Tile is already cool and moist with minimal burn risk — no suppression needed."
                        .to_string(),
                };
            }
            let had_fire = tile.active_fire_intensity;
            if !dry_run_only {
                tile.active_fire_intensity = (tile.active_fire_intensity - 0.72).max(0.0);
                tile.fire_retardant_wetness = (tile.fire_retardant_wetness + 0.65).min(1.0);
                tile.soil_moisture = (tile.soil_moisture + 0.14).min(0.95);
                tile.recompute_burn_risk(current_vpd_kpa);
            }
            CompatibilityOutcome::EffectiveSuccess {
                summary: if had_fire > 0.02 {
                    "Quenched active flames and established a shaded wet fuelbreak line.".to_string()
                } else {
                    "Pre-wetted fine fuels and cleared a shaded fuelbreak buffer.".to_string()
                },
                ecological_gain: 0.30 + had_fire * 0.5,
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyRule {
    pub action: RemediationAction,
    pub enabled: bool,
    /// Relative budget share weight (0.0..=1.0)
    pub weight: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyPortfolio {
    pub rules: Vec<StrategyRule>,
    /// Reserve fraction of annual budget kept for manual interventions / fire response (0.0..=0.80)
    pub manual_emergency_reserve_ratio: f32,
}

impl Default for StrategyPortfolio {
    fn default() -> Self {
        Self {
            rules: vec![
                StrategyRule {
                    action: RemediationAction::ThinningFromBelow,
                    enabled: true,
                    weight: 0.75,
                },
                StrategyRule {
                    action: RemediationAction::PrescribedUnderburn,
                    enabled: false,
                    weight: 0.45,
                },
                StrategyRule {
                    action: RemediationAction::InvasiveRemoval,
                    enabled: true,
                    weight: 0.85,
                },
                StrategyRule {
                    action: RemediationAction::InstallBdaAndLwd,
                    enabled: true,
                    weight: 0.80,
                },
                StrategyRule {
                    action: RemediationAction::NativeMycorrhizalPlanting,
                    enabled: true,
                    weight: 0.65,
                },
                StrategyRule {
                    action: RemediationAction::LegacySnagAndOwlCavity,
                    enabled: true,
                    weight: 0.55,
                },
            ],
            manual_emergency_reserve_ratio: 0.20,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnualAuditRecord {
    pub year: u32,
    pub ecological_recovery_index: f32,
    pub wildlife_recovery_score: f32,
    pub mean_burn_risk: f32,
    pub mean_invasive_cover: f32,
    pub awarded_next_budget: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetLedger {
    pub current_year: u32,
    pub annual_grant: f64,
    pub remaining_budget: f64,
    pub spent_automated_this_year: f64,
    pub spent_manual_this_year: f64,
    pub history: Vec<AnnualAuditRecord>,
}

impl Default for BudgetLedger {
    fn default() -> Self {
        Self {
            current_year: 1,
            annual_grant: 140_000.0,
            remaining_budget: 140_000.0,
            spent_automated_this_year: 0.0,
            spent_manual_this_year: 0.0,
            history: Vec::new(),
        }
    }
}

impl BudgetLedger {
    /// Success = Higher Budget Next Year:
    /// Base watershed grant ($120,000) scales positively with Ecological Recovery Index (0..100),
    /// Wildlife Trophic Recovery Score (0..100), Fire Refugia Integrity, and up to 25% rollover.
    pub fn close_year_and_award_next_budget(
        &mut self,
        eri: f32,
        wildlife_score: f32,
        mean_burn_risk: f32,
        mean_invasive_cover: f32,
    ) -> AnnualAuditRecord {
        let base_grant = 120_000.0;
        let eri_bonus = (eri.clamp(0.0, 100.0) as f64) * 2_650.0;
        let wildlife_bonus = (wildlife_score.clamp(0.0, 100.0) as f64) * 980.0;
        let refugia_bonus = ((1.0 - mean_burn_risk.clamp(0.0, 1.0)) as f64) * 28_000.0;
        let rollover = (self.remaining_budget * 0.25).clamp(0.0, 45_000.0);

        let next_budget =
            ((base_grant + eri_bonus + wildlife_bonus + refugia_bonus + rollover) / 100.0).round()
                * 100.0;

        let record = AnnualAuditRecord {
            year: self.current_year,
            ecological_recovery_index: eri,
            wildlife_recovery_score: wildlife_score,
            mean_burn_risk,
            mean_invasive_cover,
            awarded_next_budget: next_budget,
        };
        self.history.push(record.clone());
        self.current_year += 1;
        self.annual_grant = next_budget;
        self.remaining_budget = next_budget;
        self.spent_automated_this_year = 0.0;
        self.spent_manual_this_year = 0.0;
        record
    }
}
