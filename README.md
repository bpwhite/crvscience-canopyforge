# CanopyForge v1.0.0 — Pacific Northwest Old-Growth Ecosystem Recovery

**CanopyForge** is a native safe-Rust (`#![forbid(unsafe_code)]`) 3D isometric and zoomable Pacific Northwest (PNW) old-growth forest restoration "idle + active stewardship" simulation game modeled after the autonomous responder architecture of **SwarmForge**.

Instead of industrial assembly lines, you manage a **256-hectare (`16 × 16` sector) Oregon Coast Range / Western Cascades watershed**. Configure autonomous **"Set Strategy"** policies that dispatch specialized ecological field crews year-round, perform targeted **Manual Interventions**, weather **Vapor Pressure Deficit (VPD) droughts** and **simulated summer wildfires**, remove **invasive species**, and watch **6 native PNW wildlife populations** recover and roam across a hardware-accelerated `wgpu` 24.0 3D diorama.

## Core Ecological & Gameplay Systems

- **5 Grounded PNW Watershed Ecotypes**:
  - **Remnant Old-Growth Core**: 250–750 year *Pseudotsuga menziesii* ([USFS FEIS Douglas-fir](https://www.fs.usda.gov/database/feis/plants/tree/psemenm/all.html); [H.J. Andrews Old-Growth Characteristics](https://andrewsforest.oregonstate.edu/pubs/pdf/pub120.pdf)), *Thuja plicata* ([USFS FEIS Western Redcedar](https://www.fs.usda.gov/database/feis/plants/tree/thupli/all.html)), *Tsuga heterophylla* ([USFS FEIS Western Hemlock](https://www.fs.usda.gov/database/feis/plants/tree/tsuhet/all.html)), mossy nurse logs, and *Polystichum munitum* understory ([USFS FEIS Western Sword Fern](https://www.fs.usda.gov/database/feis/plants/fern/polmun/all.html)).
  - **Dense Second-Growth Plantation**: Overstocked even-aged conifer stand with continuous ladder fuels, closed shade, and high crown-fire hazard.
  - **Incised Salmonid Creek Corridor**: Wood-depleted stream channel ready for instream Large Woody Debris (LWD) and Beaver Dam Analogues (BDAs).
  - **Degraded Clearcut Edge**: Compacted soil colonized by *Rubus armeniacus* (Himalayan blackberry), *Cytisus scoparius* (Scotch broom), and creeping *Hedera helix* (English ivy).
  - **Upland Oak-Conifer Meadow Ridge**: Dry-mesic upper slope exposed to summer atmospheric drying (VPD), Scotch broom flammability, and lightning ignitions.

- **"Set Strategy" Autonomous Stewardship & Manual Interventions (Strict Non-Failure Guarantee)**:
  - **6 Autonomous Remediation Strategies** allocate your annual grant budget across mobile field crews (`Crosscut Sawyer Crew`, `Prescribed Burn Crew`, `Invasive Removal Crew`, `Riparian BDA Crew`, `Mycorrhizal Planting Crew`, `Wildlife Cavity Biologists`) plus overhead non-obstructing `Canopy LiDAR Survey Drones`:
    1. **Thinning from Below & Variable-Density Thinning** ([USFS PNW Thinning & Prescribed Fire](https://research.fs.usda.gov/treesearch/34869); [USFWS Crosscut Forest Thinning](https://www.fws.gov/project/forest-thinning-restore-fire-resilience))
    2. **Cultural & Prescribed Underburning** ([USFS R6 Fire Resiliency](https://www.fs.usda.gov/r06/planning/fire-resiliency); [USFS Thinning & Prescribed Fire Comparison](https://research.fs.usda.gov/download/treesearch/28573.pdf))
    3. **Invasive Root Grubbing & English Ivy Survival-Ringing** ([USFS Scotch Broom Management](https://research.fs.usda.gov/pnw/products/dataandtools/ecology-and-management-invasive-scotch-broom); [Seattle Forest Restoration Ivy Ringing](https://www.seattle.gov/trees/restoration/homes); [Skagit Fisheries Blackberry Removal](https://www.skagitfisheries.org/himalayan-blackberry-removal/))
    4. **Beaver Dam Analogues (BDAs) & Instream Large Woody Debris (LWD)** ([NOAA Fisheries BDA Coho Study](https://www.fisheries.noaa.gov/feature-story/start-beaver-dams-transform-parched-california-creeks-salmon-havens-new-research-finds); [Coast Coho Partnership LWD](https://coastcoho.org/large-woody-debris-lwd/))
    5. **Multi-Cohort Native Replanting & Mycorrhizal Nurse-Log Inoculation** ([USFS FEIS Western Redcedar](https://www.fs.usda.gov/database/feis/plants/tree/thupli/all.html))
    6. **Legacy Snag Cavities & Spotted Owl / Murrelet Platform Stewardship** ([USFWS Northern Spotted Owl Recovery Plan](https://ecos.fws.gov/docs/recovery_plan/NSO%20Final%20Rec%20Plan%20051408_1.pdf); [USFS Supplementary Flying Squirrel Dens](https://research.fs.usda.gov/treesearch/6108))
  - **Non-Failure Compatibility Invariant**: Remediation strategies and manual interventions **never fail or damage the ecosystem**. Applying an intervention to a mismatched ecotype or an already-restored tile resolves cleanly to `NoEffectNeutral` with explanatory ecological telemetry (and zero budget deduction on manual misclicks).

- **Annual Budget Scaling, Drought (VPD) & Wildfire Refugia**:
  - Years advance across `Spring → Summer → Autumn → Winter`. Each New Year's Watershed Audit evaluates your **Ecological Recovery Index (`ERI`, 0–100)** and scales next year's stewardship grant upward (`$120,000` base + ERI & biodiversity performance bonuses + up to 25% rollover reserve).
  - Summertime **Vapor Pressure Deficit (`VPD kPa`)** and multi-year drought waves dry surface fuels ([USFS H.J. Andrews VPD Drought Study](https://research.fs.usda.gov/treesearch/67961)), while BDA beaver ponds raise hyporheic water tables across neighboring stands and create natural **fire refugia** ([USFS PNW Fire Refugia](https://research.fs.usda.gov/download/treesearch/80467.pdf)).

- **6 Modelled & Visible 3D PNW Indicator Wildlife Species**:
  - **Northern Spotted Owl** (*Strix occidentalis caurina*) ([Center for Biological Diversity NSO](https://www.biologicaldiversity.org/species/birds/northern_spotted_owl/index.html))
  - **Oregon Coast Coho Salmon** (*Oncorhynchus kisutch*) ([NOAA Oregon Coast Coho Salmon](https://www.fisheries.noaa.gov/west-coast/endangered-species-conservation/oregon-coast-coho-salmon))
  - **North American Beaver** (*Castor canadensis*) ([NOAA Nature's Engineers Beaver & Coho](https://www.fisheries.noaa.gov/feature-story/working-natures-engineers-build-coho-salmon-habitat))
  - **Roosevelt Elk** (*Cervus canadensis roosevelti*) ([NPS Roosevelt Elk](https://www.nps.gov/articles/000/roosevelt-elk.htm))
  - **Marbled Murrelet** (*Brachyramphus marmoratus*) ([USFWS Marbled Murrelet](https://www.fws.gov/species/marbled-murrelet-brachyramphus-marmoratus))
  - **Northern Flying Squirrel** (*Glaucomys sabrinus*) ([NatureServe Glaucomys sabrinus](https://explorer.natureserve.org/Taxon/ELEMENT_GLOBAL.2.1010091/Glaucomys_sabrinus))

## Building, Running & Testing Locally

```bash
# Launch the 3D hardware-accelerated WGPU desktop game (zoomed-out isometric view by default)
cargo run --release

# Run headless multi-year ecological stewardship benchmark from CLI
cargo run --release -- simulate --years 12 --seed 42 --export-json audit_run.json

# Run all ecological invariant & persistence integration tests
cargo test --all-targets

# Render offscreen 1600x1000 WGPU PBR PNG screenshots (1=Full Iso, 2=Mid-Canopy, 3=Creek & BDA)
cargo run --release -- --headless-screenshot validation/headless_screenshot.png --screenshot-view 1
cargo run --release -- --headless-screenshot validation/headless_screenshot_2.png --screenshot-view 2
cargo run --release -- --headless-screenshot validation/headless_screenshot_3.png --screenshot-view 3
```

## Validation, Screenshots & Preprints

- **Automated Test Suite**: `cargo test --all-targets` (`8/8` checks passing across zero-harm stewardship compatibility, annual budget scaling, wildlife recovery, SQLite WAL round-trip, and articulated 3D fauna geometry).
- **Validation Artifacts**: See [validation/canopyforge_validation_summary.json](validation/canopyforge_validation_summary.json) and the three `1600×1000` headless WGPU captures under `validation/`.
- **Companion Preprints**: See [`Preprints/`](Preprints/README.md) (`APP-PREPRINT-23`, Zenodo Concept DOI [`10.5281/zenodo.24442091`](https://doi.org/10.5281/zenodo.24442091)) and citation metadata in [`CITATION.cff`](CITATION.cff).

## Camera & Input Controls

- **Scroll Wheel / HUD Zoom Presets (`1`–`4`)**: Smoothly transition from **1. Full Watershed Isometric View** (default) to **2. Mid-Canopy Stand Zoom**, **3. Salmon & Beaver Creek Close-Up**, and **4. Follow Wildlife / Understory Cam**.
- **Left Click**: Inspect tile / apply selected Manual Intervention (`1×1` or `3×3` brush).
- **Right Drag or `Q` / `E`**: Orbit camera azimuth around the watershed diorama.
- **Middle Drag or `W` `A` `S` `D`**: Pan across forest stands and stream reaches.
- **`Space`**: Toggle Pause / Resume; **`[` / `]`**: Adjust year simulation speed (`1×` to `10×`).

## License & Attribution

Licensed under the **GNU General Public License v3.0 or later (`GPL-3.0-or-later`)**. See [LICENSE](LICENSE) for details.

- **Repository**: <https://github.com/bpwhite/crvscience-canopyforge>
- **Zenodo Concept DOI**: <https://doi.org/10.5281/zenodo.24442091>
- **CRV Science Open-Source Apps**: <https://crvscience.com/apps>
- **Copyright**: Copyright CRV Science Network (<https://crvscience.com>)
