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
    #[arg(long, default_value_t = 2026)]
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
                    "  - {:<26} ({:<30}): {:>6.1} / K {:>6.1} ({:+.1}%/yr)",
                    pop.species.common_name(),
                    pop.species.scientific_name(),
                    pop.count,
                    pop.carrying_capacity,
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
}
