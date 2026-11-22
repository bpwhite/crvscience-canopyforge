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

use canopyforge::agents::{
    compute_wildlife_recovery_score, initial_species_populations, step_wildlife_populations,
    WildlifeSpecies,
};
use canopyforge::app::CanopyForgeApp;
use canopyforge::ecology::{build_initial_watershed, Ecotype, Season};
use canopyforge::persistence::SaveDatabase;
use canopyforge::strategies::{
    evaluate_and_apply_action, BudgetLedger, CompatibilityOutcome, RemediationAction,
};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

#[test]
fn interventions_never_fail_or_degrade_ecological_score() {
    let mut rng = ChaCha8Rng::seed_from_u64(1337);
    let tiles = build_initial_watershed(&mut rng);

    for base_tile in &tiles {
        for action in RemediationAction::ALL_MANUAL {
            let mut candidate = base_tile.clone();
            let before_score = candidate.ecological_tile_score();
            let outcome = evaluate_and_apply_action(
                action,
                &mut candidate,
                Season::Spring,
                1.4,
                false,
            );
            let after_score = candidate.ecological_tile_score();

            match outcome {
                CompatibilityOutcome::EffectiveSuccess { ecological_gain, .. } => {
                    assert!(ecological_gain > 0.0);
                    assert!(
                        after_score + 1e-3 >= before_score,
                        "Action {:?} on {:?} unexpectedly lowered score from {} to {}",
                        action,
                        base_tile.ecotype,
                        before_score,
                        after_score
                    );
                }
                CompatibilityOutcome::NoEffectNeutral { reason } => {
                    assert!(!reason.is_empty());
                    assert!(
                        (after_score - before_score).abs() < 1e-4,
                        "Neutral outcome mutated tile score!"
                    );
                }
            }
        }
    }
}

#[test]
fn bda_has_no_effect_on_ridge_and_succeeds_in_salmon_creek() {
    let mut rng = ChaCha8Rng::seed_from_u64(99);
    let mut tiles = build_initial_watershed(&mut rng);

    let ridge = tiles
        .iter_mut()
        .find(|t| t.ecotype == Ecotype::UplandMeadowRidge)
        .expect("ridge tile exists");
    let ridge_res = evaluate_and_apply_action(
        RemediationAction::InstallBdaAndLwd,
        ridge,
        Season::Spring,
        1.2,
        false,
    );
    assert!(matches!(ridge_res, CompatibilityOutcome::NoEffectNeutral { .. }));

    let creek = tiles
        .iter_mut()
        .find(|t| t.ecotype == Ecotype::RiparianSalmonCreek)
        .expect("creek tile exists");
    let before_bda = creek.bda_lwd_level;
    let creek_res = evaluate_and_apply_action(
        RemediationAction::InstallBdaAndLwd,
        creek,
        Season::Spring,
        1.2,
        false,
    );
    assert!(creek_res.is_effective());
    assert!(creek.bda_lwd_level > before_bda);
}

#[test]
fn higher_ecological_success_awards_higher_next_year_budget() {
    let mut low_ledger = BudgetLedger::default();
    low_ledger.remaining_budget = 10_000.0;
    let low_audit = low_ledger.close_year_and_award_next_budget(32.0, 20.0, 0.62, 0.35);

    let mut high_ledger = BudgetLedger::default();
    high_ledger.remaining_budget = 10_000.0;
    let high_audit = high_ledger.close_year_and_award_next_budget(78.0, 72.0, 0.18, 0.05);

    assert!(high_audit.awarded_next_budget > low_audit.awarded_next_budget);
    assert!(high_ledger.annual_grant > 140_000.0);
}

#[test]
fn salmon_and_owl_populations_recover_with_bda_and_snag_stewardship() {
    let mut rng = ChaCha8Rng::seed_from_u64(42);
    let mut tiles = build_initial_watershed(&mut rng);
    let mut pops = initial_species_populations(&tiles);
    let initial_score = compute_wildlife_recovery_score(&pops);

    for t in &mut tiles {
        if t.ecotype == Ecotype::RiparianSalmonCreek {
            let _ = evaluate_and_apply_action(
                RemediationAction::InstallBdaAndLwd,
                t,
                Season::Spring,
                1.2,
                false,
            );
            let _ = evaluate_and_apply_action(
                RemediationAction::InvasiveRemoval,
                t,
                Season::Spring,
                1.2,
                false,
            );
        } else {
            let _ = evaluate_and_apply_action(
                RemediationAction::LegacySnagAndOwlCavity,
                t,
                Season::Spring,
                1.2,
                false,
            );
        }
    }

    for _ in 0..16 {
        step_wildlife_populations(&mut pops, &tiles, 90.0);
    }

    let final_score = compute_wildlife_recovery_score(&pops);
    let coho = pops
        .iter()
        .find(|p| p.species == WildlifeSpecies::OregonCoastCohoSalmon)
        .unwrap();
    let owl = pops
        .iter()
        .find(|p| p.species == WildlifeSpecies::NorthernSpottedOwl)
        .unwrap();

    assert!(final_score > initial_score);
    assert!(coho.count > coho.baseline_count);
    assert!(owl.count > owl.baseline_count);
}

#[test]
fn sqlite_wal_and_json_roundtrip_preserve_simulation_state() {
    let mut app = CanopyForgeApp::new_simulation(2026, None);
    for _ in 0..20 {
        app.step_simulation(0.1);
    }
    let snap = app.export_save_state("unit_test_slot");
    let json = snap.to_json_pretty().expect("serialize json");
    let parsed = canopyforge::persistence::SimulationSaveState::from_json_str(&json)
        .expect("deserialize json");
    assert_eq!(parsed.tiles.len(), snap.tiles.len());

    let db = SaveDatabase::open_in_memory().expect("in-memory sqlite");
    db.save_slot("slot_a", app.ecological_recovery_index(), &snap)
        .expect("save sqlite slot");
    let loaded = db
        .load_slot("slot_a")
        .expect("query sqlite slot")
        .expect("slot exists");
    assert_eq!(loaded.ledger.current_year, snap.ledger.current_year);
}
