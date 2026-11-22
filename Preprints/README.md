# Preprints for CanopyForge (`crvscience-canopyforge`)

**Author:** Bryan P. White (ORCID: [0000-0003-4191-3511](https://orcid.org/0000-0003-4191-3511))  
**Hub:** https://crvscience.com/apps

This directory contains the self-contained LaTeX source (`.tex`) and single-column PDF (`.pdf`) editions for both:

1. **Companion Application Preprint (`APP-PREPRINT-23`)**
   - **Title:** CanopyForge: 3D Isometric Pacific Northwest Old-Growth Watershed Recovery and Ecological Stewardship Simulator in Rust
   - **Files:** `canopyforge-3d-pnw-old-growth-recovery.tex`, `canopyforge-3d-pnw-old-growth-recovery.pdf`
   - **Canonical Zenodo DOI:** https://doi.org/10.5281/zenodo.24442091
   - **Figshare Mirror:** https://figshare.com/articles/preprint/CanopyForge_3D_Isometric_Pacific_Northwest_Old-Growth_Watershed_Recovery_and_Ecological_Stewardship_Simulator_in_Rust/35084815
   - **Web Preprint:** https://crvscience.com/apps/preprints/canopyforge-3d-pnw-old-growth-recovery

2. **CRV Science Public Apps Network Overview (`APP-PREPRINT-01`)**
   - **Title:** The CRV Science App Network: Twenty-Two Open-Source Rust Programs for Science, Data, and Simulation
   - **Files:** `crv-science-public-apps-network.tex`, `crv-science-public-apps-network.pdf`
   - **Canonical Zenodo DOI:** https://doi.org/10.5281/zenodo.24222398
   - **Figshare Mirror:** https://figshare.com/articles/preprint/The_CRV_Science_App_Network_Seventeen_Open-Source_Rust_Programs_for_Science_Data_and_Simulation/35013736
   - **Web Preprint:** https://crvscience.com/apps/preprints/crv-science-public-apps-network

## Compiling Standalone LaTeX

Both `.tex` files inline their bibliography so each compiles standalone with standard `pdflatex`:

```bash
pdflatex canopyforge-3d-pnw-old-growth-recovery.tex
pdflatex crv-science-public-apps-network.tex
```
