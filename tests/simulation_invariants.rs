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
use canopyforge::app::{CanopyForgeApp, CanopyPaletteAction};
use canopyforge::ecology::{build_initial_watershed, Ecotype, Season};
use canopyforge::persistence::SaveDatabase;
use canopyforge::scorecard::{
    evaluate_century_restoration_scorecard, export_century_scorecard_files,
    format_century_scorecard_csv, CenturyRestorationScorecard,
};
use canopyforge::strategies::{
    evaluate_and_apply_action, BudgetLedger, CompatibilityOutcome, RemediationAction,
};
use eframe::egui;
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

#[test]
fn century_scorecard_projects_100_years_and_shows_active_stewardship_advantage() {
    let sc = evaluate_century_restoration_scorecard(2026, 100);
    assert_eq!(sc.schema_version, "canopyforge_century_scorecard_v1");
    assert_eq!(sc.horizon_years, 100);
    assert_eq!(sc.scenarios.len(), 3);
    let passive = &sc.scenarios[0];
    let active = &sc.scenarios[1];
    assert_eq!(passive.decadal_eri_trajectory.len(), 10);
    assert_eq!(active.decadal_eri_trajectory.len(), 10);
    assert!(
        active.final_eri > passive.final_eri,
        "100-year active stewardship ERI ({}) must exceed passive ({})",
        active.final_eri,
        passive.final_eri
    );
    assert!(sc.eri_advantage_active_vs_passive > 0.0);
}

#[test]
fn century_scorecard_json_and_csv_export_roundtrip_are_valid() {
    let out_dir = std::env::temp_dir().join(format!(
        "canopyforge_scorecard_test_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&out_dir);

    let (jp, cp, sc) = export_century_scorecard_files(&out_dir, 42, 40)
        .expect("scorecard export succeeds");
    assert!(jp.exists());
    assert!(cp.exists());

    let parsed: CenturyRestorationScorecard =
        serde_json::from_str(&std::fs::read_to_string(&jp).expect("read json"))
            .expect("parse json scorecard");
    assert_eq!(parsed.horizon_years, 40);
    assert_eq!(parsed.scenarios.len(), sc.scenarios.len());

    let csv = format_century_scorecard_csv(&parsed);
    let lines: Vec<&str> = csv.lines().collect();
    assert_eq!(lines.len(), 4, "header + 3 scenarios");
    assert!(lines[0].starts_with("scenario_code,scenario_label,seed"));

    let _ = std::fs::remove_dir_all(&out_dir);
}

#[test]
fn century_scorecard_severe_drought_retains_higher_burn_risk_than_normal() {
    let sc = evaluate_century_restoration_scorecard(2026, 30);
    let balanced = &sc.scenarios[1];
    let drought = &sc.scenarios[2];
    assert!(
        drought.final_burn_risk >= balanced.final_burn_risk,
        "Severe VPD drought burn risk ({}) should be >= balanced ({})",
        drought.final_burn_risk,
        balanced.final_burn_risk
    );
}

#[test]
fn ctrl_k_command_palette_opens_filters_and_escapes_in_headless_ui() {
    let ctx = egui::Context::default();
    let mut app = CanopyForgeApp::new_simulation(2026, None);
    let ctrl_k = egui::Event::Key {
        key: egui::Key::K,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::CTRL,
    };
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            modifiers: egui::Modifiers::CTRL,
            events: vec![ctrl_k],
            ..Default::default()
        },
        |c| app.ui_step(c, true),
    );
    assert!(app.show_command_palette);

    app.command_palette_query = "Salmonid".to_string();
    let filtered = app.filtered_palette_commands();
    assert!(!filtered.is_empty());
    assert!(filtered[0].0.contains("Salmonid Creek"));

    let esc = egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            events: vec![esc],
            ..Default::default()
        },
        |c| app.ui_step(c, true),
    );
    assert!(!app.show_command_palette);
}

#[test]
fn ctrl_k_respects_text_focus_guard_when_json_or_text_edit_focused() {
    let ctx = egui::Context::default();
    let mut app = CanopyForgeApp::new_simulation(2026, None);
    app.json_modal_open = true;
    let ctrl_k = egui::Event::Key {
        key: egui::Key::K,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::CTRL,
    };
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            modifiers: egui::Modifiers::CTRL,
            events: vec![ctrl_k],
            ..Default::default()
        },
        |c| app.ui_step(c, true),
    );
    assert!(!app.show_command_palette);
}

#[test]
fn palette_actions_jump_ecotype_select_tool_switch_season_and_export_scorecard() {
    let mut app = CanopyForgeApp::new_simulation(2026, None);
    app.execute_palette_action(CanopyPaletteAction::JumpEcotype(Ecotype::RiparianSalmonCreek));
    let (sx, sz) = app.selected_tile.expect("tile selected");
    assert_eq!(app.tiles[sz * 16 + sx].ecotype, Ecotype::RiparianSalmonCreek);

    app.execute_palette_action(CanopyPaletteAction::SelectTool(RemediationAction::InstallBdaAndLwd));
    assert_eq!(app.selected_manual_tool, RemediationAction::InstallBdaAndLwd);

    app.execute_palette_action(CanopyPaletteAction::JumpSeasonDay(145));
    assert_eq!(Season::from_day_of_year(app.day_of_year), Season::Summer);

    app.execute_palette_action(CanopyPaletteAction::ToggleAuditModal);
    assert!(app.show_audit_modal);

    let tmp_dir = std::env::temp_dir().join(format!(
        "canopyforge_palette_export_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp_dir);
    app.export_dir_input = tmp_dir.display().to_string();
    app.execute_palette_action(CanopyPaletteAction::ExportCenturyScorecard);
    assert!(app.last_scorecard.is_some());
    assert!(tmp_dir.join("canopyforge_100yr_scorecard.json").exists());
    assert!(tmp_dir.join("canopyforge_100yr_scorecard.csv").exists());
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn palette_enter_executes_selected_command() {
    let ctx = egui::Context::default();
    let mut app = CanopyForgeApp::new_simulation(2026, None);
    app.show_command_palette = true;
    app.command_palette_query = "Summer VPD".to_string();
    let enter = egui::Event::Key {
        key: egui::Key::Enter,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 800.0),
            )),
            events: vec![enter],
            ..Default::default()
        },
        |c| app.ui_step(c, true),
    );
    assert!(!app.show_command_palette);
    assert!((app.day_of_year - 145.0).abs() < 1e-4);
}

#[test]
fn century_scorecard_is_deterministic_for_identical_seed() {
    let a = evaluate_century_restoration_scorecard(777, 25);
    let b = evaluate_century_restoration_scorecard(777, 25);
    assert_eq!(a.scenarios.len(), b.scenarios.len());
    for (ra, rb) in a.scenarios.iter().zip(b.scenarios.iter()) {
        assert!((ra.final_eri - rb.final_eri).abs() < 1e-6);
        assert!((ra.final_burn_risk - rb.final_burn_risk).abs() < 1e-6);
    }
}
