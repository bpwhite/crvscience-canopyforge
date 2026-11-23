# CanopyForge User Guide

*Version 1.0.0 — Licensed under GNU General Public License v3.0 or later (`GPL-3.0-or-later`).*

## 1. Workstation Layout

CanopyForge centers a real-time `wgpu` 3D Pacific Northwest watershed diorama surrounded by three operational panels:

1. **Top Telemetry & Time Bar**: Displays `Year`, `Season (Day/365)`, remaining annual stewardship budget (`$`), `Recovery Index (ERI 0–100)`, active `VPD (kPa)` and climate regime, pause/speed controls (`1×`–`10×`), quick SQLite Save/Load, JSON Schema modal, and About (`F1`).
2. **Left Command Dock (`left_strategy_dock`)**:
   - **Set Strategy (Idle Crews)**: Enable/disable and prioritize 6 autonomous stewardship crews plus the emergency manual reserve slider (`0%–70%`).
   - **Manual Interventions**: Choose click behavior (`Click: Apply Tool` vs `Click: Inspect Only`), brush size (`1×1 Sector` vs `3×3 Watershed Brush`), and active tool (`Crosscut Thinning`, `Prescribed Burn`, `Remove Invasives`, `BDA & Large Wood`, `Native Underplant`, `Snag & Owl Dens`, `Suppress / Wet`).
   - **Climate & Fire Drills**: Trigger a `Dry Lightning Test` ignition on the highest-risk ridge sector or jump straight into `SevereVpdDrought` at peak summer drying (`Day 145`, `3.65 kPa`).
   - **SQLite Save Slots**: Enter a custom slot name, click `Save Slot`, and browse or load any saved watershed checkpoint listed from `canopyforge_saves.sqlite3`.
3. **Central 3D Viewport**:
   - Top bar toggles **Camera Presets (`1`–`4`)**, **Map Layers** (`Natural PBR Forest`, `Burn Risk Heatmap`, `Soil Moisture & BDA`, `Invasive Plant Cover`, `Old-Growth Maturity`), and **Click Mode** (`Apply Tool` vs `Inspect Only`).
   - When any analytical `MapOverlay` (`Burn Risk`, `Soil Moisture`, `Invasive Cover`, `Old-Growth Maturity`) is selected, a calibrated gradient colorbar legend appears in the bottom-left corner of the viewport.
4. **Right Ecology Inspector (`right_ecology_inspector`)**:
   - **PNW Wildlife Recovery**: Shows real-time population/index bars and `%/yr` recovery rates for all 6 indicator species. Click any species card to follow a live 3D animal with Preset `4`.
   - **Annual Audit & Budget History**: Plots year-over-year awarded grant budgets (amber bars) alongside Ecological Recovery Index trajectories (emerald line).
   - **Stand Sector Inspector & Field Crew Telemetry**: Displays per-sector metrics and crew status updates.

## 2. Inspect Mode vs. Apply Mode & Dry-Run Hover Previews

- **Live Dry-Run Hover Tooltip**: Moving the pointer across the 3D terrain highlights the hovered sector in emerald and opens a pointer tooltip showing:
  - Sector coordinates `(x, z)` and ecotype
  - Old-Growth %, Burn Risk %, and Invasive Cover %
  - Exact dry-run prediction for your selected tool and brush (`1×1` or `3×3`): `EffectiveSuccess (N tile(s)) | Cost: $X` or `NoEffectNeutral | Cost: $0` with ecological rationale.
- **Inspect Only Toggle**: Enable `Click: Inspect Only` to click around the watershed and inspect stands in the right panel without spending budget or modifying tiles.

## 3. Multi-Slot SQLite Persistence & JSON Portability

- **Named Slots**: Expand `SQLite Save Slots` in the left panel to save distinct runs (e.g., `pre_drought_yr4`, `post_bda_yr8`). Loading a slot deterministically restores all tiles, budgets, crews, climate regime, and re-seeds the `ChaCha8Rng` generator (`seed`).
- **JSON Schema Exchange**: Click `JSON Schema` in the top bar to inspect, copy, or paste portable `canopyforge.v1` JSON snapshots compatible with the CanopyForge web preview. While the JSON modal is open, viewport hotkeys (`WASD`, `Q/E`, `Space`, `1–4`, `[`/`]`) are safely suppressed.
