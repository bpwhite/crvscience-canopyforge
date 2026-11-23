# CanopyForge Scientific Methods & Mathematical Reference

*Version 1.0.0 — Copyright CRV Science Network — Licensed under `GPL-3.0-or-later`.*

## 1. Watershed Geometry & Seasonal VPD Forcing (`src/ecology.rs`)

CanopyForge simulates a `16 × 16` grid (`256` sectors at `3.0` world units pitch, representing a `256 ha` Oregon Coast Range / Western Cascades sub-basin) traversed from NE `(15, 2)` to SW `(0, 13)` by a meandering salmonid stream corridor.

### Seasonal Calendar & Vapor Pressure Deficit (VPD)

Each year spans `365` days divided equally across four `91.25`-day seasons (`Spring [0, 91.25)`, `Summer [91.25, 182.5)`, `Autumn [182.5, 273.75)`, `Winter [273.75, 365)`). Atmospheric Vapor Pressure Deficit (`kPa`) peaks squarely inside `Season::Summer` at day `d_peak = 145.0`:

```
phase       = ((d - 145.0) / 365.0) * 2 * pi
summer_bell = ((cos(phase) + 1.0) / 2.0) ^ 1.7
VPD(d)      = 0.52 + (VPD_summer_max(regime) - 0.52) * summer_bell   [kPa]
```

where `VPD_summer_max` is `1.35` (`CoolWetLaNina`), `1.95` (`NormalMaritime`), `2.75` (`WarmDrySummer`), or `3.65 kPa` (`SevereVpdDrought`).

## 2. Soil Moisture, Wildfire Risk & Offshore East-Wind Advection

### Hyporheic BDA Recharge & Moisture Balance

Neighboring riparian creek tiles (`dist <= 2`) contribute hyporheic water-table support scaled by Beaver Dam Analogue / Large Woody Debris maturity (`bda_lwd_level in [0, 1]`):

```
hydro_support = sum(min((0.25 + 0.85 * bda_lwd_level) / (1.15 * dist), 0.45))   clamped to [0, 0.55]
theta_target  = clamp(0.30 * precip(season, regime) + hydro_support + canopy_shade - 0.14 * max(VPD - 0.8, 0), 0.10, 0.96)
```

### Burn Risk & Offshore East-Wind (`-X`) Fire Percolation

Riparian reaches with `bda_lwd_level > 0.35` maintain persistent wet fire-refugia burn risk (`<= 0.25`). Upland sectors evaluate:

```
vpd_norm       = clamp((VPD - 0.6) / 3.2, 0, 1)
canopy_refugia = 0.28 * old_growth_maturity + 0.08 * mycorrhizal_index
burn_risk      = clamp(0.44 * ladder_fuels + 0.26 * vpd_norm + 0.34 * broom + 0.14 * blackberry
                       + 0.12 * ivy - 0.38 * soil_moisture - canopy_refugia - 0.55 * retardant, 0.02, 0.98)
```

During active wildfire spread across adjacent neighbors `(dx, dz)` (`dist <= 1.5`), fire percolation couples topographic upslope heating with an explicit Pacific Northwest offshore **east-wind** (`w = (-1, 0)`, blowing from `+X` toward `-X`):

```
slope_boost      = clamp(elev_target - elev_source, -0.2, 0.5) * 0.45
wind_align       = dx / dist                        (positive when source lies East of target)
east_wind_factor = max(1.0 + 0.48 * wind_align, 0.45)
fire_in          = sum((I_source / dist) * (1.0 + slope_boost) * east_wind_factor)
```

## 3. Non-Failure Compatibility & Success-Scaled Budgeting (`src/strategies.rs`)

All manual and autonomous crew interventions evaluate `evaluate_and_apply_action(action, tile, season, vpd, dry_run_only)` before modifying state or deducting grant funds:
- Compatible, un-saturated targets yield `EffectiveSuccess { summary, ecological_gain > 0 }`.
- Mismatched ecotypes, saturated stands, or high-VPD burn pauses (`VPD > 2.65 kPa` in `Summer`) yield `NoEffectNeutral { reason }` with zero ecological harm and `$0` budget deduction.

At year-end rollover, next year's stewardship grant scales monotonically with Ecological Recovery Index (`ERI = 0.68 * mean_tile_score + 0.32 * wildlife_score`), wildlife recovery, fire refugia integrity, and rollover reserve (`25%` capped at `$45,000`):

```
Grant_{y+1} = round_100(120,000 + 2,650 * ERI + 980 * WRS + 28,000 * (1 - mean_burn_risk) + rollover)
```

## 4. Wildlife Habitat Capacities & Spotted Owl Sub-Basin Calibration (`src/agents.rs`)

All six PNW wildlife populations evolve via coupled logistic recovery with regional corridor colonization when `K > N`:

```
dN/dt = r * N * (1 - N / K(tiles)) + 0.025 * max(K(tiles) - N, 0)
```

Because Northern Spotted Owl (*Strix occidentalis caurina*) breeding pairs occupy `~1,000–2,000 ha` home ranges in Oregon Coast Range / Cascades landscapes, carrying capacity in the `256 ha` focal watershed is calibrated as a **sub-basin habitat suitability index (`K in [1.5, 12.0]`)** rather than raw closed-basin pair packing:

```
K_owl = clamp(1.5 + 0.085 * og_habitat + 0.055 * snag_platforms + 0.022 * N_squirrel, 1.5, 12.0)
```

Visible 3D animal instances scale dynamically with `pop.count` and automatically prune surplus actors whenever populations decline.

## 5. Rendering & Tone Mapping Pipeline (`src/wgpu_renderer.rs`, `src/shaders/`)

- **HDR MSAA + Conditional Gamma Tonemapping**: Renders `4× MSAA` `Rgba16Float` PBR lighting with `2048×2048` 9-tap PCF sun shadows, passes ACES filmic tonemapping and subtle radial vignette, and applies `pow(rgb, 1/2.2)` **only** when `tonemap_params.apply_gamma > 0.5` (linear `Rgba8Unorm` headless captures), skipping duplicate software gamma on hardware sRGB swapchains (`swapchain_format.is_srgb()`).
- **Dynamic Environmental Uniforms & Creek Flow**: Modulates seasonal solar azimuth/elevation/color, VPD drought atmospheric haze (`sun_color_and_vpd.w`), wildfire smoke density, and `NE → SW` (`flow = -0.80 x + 0.60 z`) downstream creek ripples.
