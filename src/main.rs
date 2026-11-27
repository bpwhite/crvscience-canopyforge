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

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![deny(unsafe_code)]

use canopyforge::app::{render_headless_screenshot, CanopyForgeApp};
use canopyforge::export_century_scorecard_files;
use canopyforge::icon::app_egui_icon;
use clap::{Parser, Subcommand};
use std::fs;
use std::path::PathBuf;

#[cfg(windows)]
#[allow(unsafe_code)]
fn attach_parent_console_if_cli_args() {
    if std::env::args_os().len() > 1 {
        unsafe {
            let _ = windows_sys::Win32::System::Console::AttachConsole(u32::MAX);
        }
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "canopyforge",
    version = "1.0.0",
    about = "Native safe-Rust 3D isometric PNW old-growth ecosystem recovery idle & stewardship game"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Deterministic RNG watershed generation seed
    #[arg(long, global = true, default_value_t = 2026)]
    seed: u64,

    /// Render an offscreen 1600x1000 RGBA PNG screenshot without opening a GUI window
    #[arg(long)]
    headless_screenshot: Option<PathBuf>,

    /// Select camera preset view (1=Full Watershed Iso, 2=Mid-Canopy Stand, 3=Creek & BDA Pool)
    #[arg(long, default_value_t = 1)]
    screenshot_view: u8,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Launch the hardware-accelerated WGPU 3D desktop game (default)
    Gui,
    /// Run headless multi-year stewardship audit simulation for testing/benchmarking
    Simulate {
        #[arg(long, default_value_t = 8)]
        years: u32,
        #[arg(long)]
        export_json: Option<String>,
    },
    /// Run 100-year watershed scenario comparison (Passive vs Balanced vs Severe Drought) and export JSON/CSV scorecard
    Scorecard {
        #[arg(long, default_value_t = 100)]
        years: u32,
        #[arg(long, default_value = "exports")]
        out_dir: PathBuf,
    },
}

fn main() -> eframe::Result<()> {
    #[cfg(windows)]
    attach_parent_console_if_cli_args();

    let cli = Cli::parse();

    if let Some(out_path) = &cli.headless_screenshot {
        if let Err(err) = render_headless_screenshot(out_path, cli.screenshot_view) {
            eprintln!("Headless screenshot error: {err}");
            std::process::exit(1);
        }
        println!(
            "Rendered offscreen 1600x1000 screenshot (view {}) -> {}",
            cli.screenshot_view,
            out_path.display()
        );
        return Ok(());
    }

    match cli.command.unwrap_or(Commands::Gui) {
        Commands::Gui => {
            let native_options = eframe::NativeOptions {
                viewport: eframe::egui::ViewportBuilder::default()
                    .with_title("CanopyForge v1.0.0 — PNW Old-Growth Ecosystem Recovery")
                    .with_icon(app_egui_icon())
                    .with_inner_size([1440.0, 900.0])
                    .with_min_inner_size([1024.0, 680.0]),
                renderer: eframe::Renderer::Wgpu,
                ..Default::default()
            };
            let seed = cli.seed;
            eframe::run_native(
                "canopyforge",
                native_options,
                Box::new(move |cc| Ok(Box::new(CanopyForgeApp::init_with_eframe(cc, seed)))),
            )
        }
        Commands::Simulate { years, export_json } => {
            let mut app = CanopyForgeApp::new_simulation(cli.seed, None);
            app.sim_speed_mult = 10.0;
            let target_year = 1 + years;
            while app.ledger.current_year < target_year {
                app.step_simulation(0.1);
            }
            println!(
                "Completed {} simulated PNW watershed years | ERI: {:.2}/100 | Next Grant: ${:.0}",
                years,
                app.ecological_recovery_index(),
                app.ledger.annual_grant
            );
            for pop in &app.populations {
                println!(
                    "  - {:<26} ({:<30}): {:>6.1} / K {:>6.1} {} ({:+.1}%/yr)",
                    pop.species.common_name(),
                    pop.species.scientific_name(),
                    pop.count,
                    pop.carrying_capacity,
                    pop.species.unit_label(),
                    pop.annual_recovery_rate_pct
                );
            }
            if let Some(out_path) = export_json {
                if let Ok(json) = app.export_save_state("cli_benchmark").to_json_pretty() {
                    let _ = fs::write(&out_path, json);
                    println!("Exported JSON save snapshot to {}", out_path);
                }
            }
            Ok(())
        }
        Commands::Scorecard { years, out_dir } => {
            match export_century_scorecard_files(&out_dir, cli.seed, years) {
                Ok((jp, cp, sc)) => {
                    println!(
                        "Completed {}-year watershed scorecard (seed {}): Active vs Passive ERI delta {:+.2}, Burn Risk delta {:+.4}",
                        sc.horizon_years,
                        sc.seed,
                        sc.eri_advantage_active_vs_passive,
                        sc.burn_risk_reduction_active_vs_passive
                    );
                    for row in &sc.scenarios {
                        println!(
                            "  - {:<38} | ERI {:.2} -> {:.2} ({:+.2}) | Burn {:.1}% | Owl {:.1}",
                            row.scenario_label,
                            row.initial_eri,
                            row.final_eri,
                            row.eri_delta,
                            row.final_burn_risk * 100.0,
                            row.owl_habitat_index
                        );
                    }
                    println!("Wrote JSON -> {} and CSV -> {}", jp.display(), cp.display());
                }
                Err(err) => {
                    eprintln!("Scorecard export error: {err}");
                    std::process::exit(1);
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses_headless_screenshot_and_view() {
        let parsed = Cli::try_parse_from([
            "canopyforge",
            "--headless-screenshot",
            "validation/headless_screenshot_2.png",
            "--screenshot-view",
            "2",
        ])
        .expect("clap parses headless screenshot arguments");
        assert_eq!(
            parsed.headless_screenshot,
            Some(PathBuf::from("validation/headless_screenshot_2.png"))
        );
        assert_eq!(parsed.screenshot_view, 2);
    }

    #[test]
    fn cli_parses_simulate_subcommand_with_trailing_global_seed() {
        let parsed = Cli::try_parse_from(["canopyforge", "simulate", "--years", "4", "--seed", "42"])
            .expect("clap parses simulate with trailing --seed");
        assert_eq!(parsed.seed, 42);
        match parsed.command {
            Some(Commands::Simulate { years, .. }) => assert_eq!(years, 4),
            other => panic!("Expected Simulate command, got {:?}", other),
        }
    }

    #[test]
    fn cli_parses_scorecard_subcommand_with_years_and_out_dir() {
        let parsed = Cli::try_parse_from([
            "canopyforge",
            "scorecard",
            "--years",
            "100",
            "--out-dir",
            "custom_exports",
            "--seed",
            "99",
        ])
        .expect("clap parses scorecard subcommand");
        assert_eq!(parsed.seed, 99);
        match parsed.command {
            Some(Commands::Scorecard { years, out_dir }) => {
                assert_eq!(years, 100);
                assert_eq!(out_dir, PathBuf::from("custom_exports"));
            }
            other => panic!("Expected Scorecard command, got {:?}", other),
        }
    }

    #[test]
    fn cli_scorecard_defaults_to_100_years_and_exports_dir() {
        let parsed = Cli::try_parse_from(["canopyforge", "scorecard"])
            .expect("clap parses scorecard defaults");
        match parsed.command {
            Some(Commands::Scorecard { years, out_dir }) => {
                assert_eq!(years, 100);
                assert_eq!(out_dir, PathBuf::from("exports"));
            }
            other => panic!("Expected Scorecard command, got {:?}", other),
        }
    }
}
