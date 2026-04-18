# HSD Data Process

This is an open-source project for processing HSD format data from Himawari satellites. The project uses Rust to implement an efficient data processing pipeline, including data correction, Rayleigh atmospheric correction, Lanczos scaling, and output in TIFF image format. Currently, this project is very unstable, use with caution.

## Project Structure
```
hsd_data_process/
├── Cargo.toml
├── README.md
├── src/
│   ├── main.rs
│   ├── lib.rs
│   ├── processer/
│   │   ├── mod.rs
│   │   ├── processer.rs
│   │   └── rayleigh_correction.rs
│   ├── reader/
│   │   ├── mod.rs
│   │   ├── hsd_organizer.rs
│   │   └── raw_hsd_reader.rs
│   └── writer/
│       ├── mod.rs
│       ├── writer.rs
│       └── writer_testing.rs
├── proj_precompute/
│   ├── main.py
│   ├── pyproject.toml
│   └── README.md
├── lut_binary/
├── 02/
└── target/
```

## Features
- Efficient data processing: High-performance parallel processing using Rust
- Multi-band support: Supports multiple visible and infrared bands from Himawari satellites
- Atmospheric correction: Built-in Rayleigh atmospheric scattering correction algorithm
- Geometric correction: Supports satellite geometric parameter calculation and correction
- Image output: Generates standard TIFF format true-color images
- Precomputation optimization: Python scripts precompute geometric data to accelerate processing

## Dependencies
### Rust Dependencies
- Rust 1.70+ (supports 2024 edition)
- System dependencies: Requires bzip2 static linking support

### Python Dependencies (Precomputation only)
- Python 3.12+ (using [uv](https://github.com/astral-sh/uv) package manager)
- numpy
- pyproj
- tqdm

## Installation
1. Clone the project:
   ```bash
   git clone 
   cd hsd_data_process
   ```

2. Install Rust dependencies:
   ```bash
   cargo build --release
   ```

3. Install Python dependencies:
   ```bash
   cd proj_precompute
   uv sync
   ```

## Usage
1. Precompute geometric data
   First, run the Python script to generate geometric data files:
   ```bash
   uv run main.py
   ```
   This will generate geometric data files for Himawari-9 satellite, including latitude, longitude, solar zenith angle, etc.

2. Prepare data
   Place HSD data files in the `02` directory, ensuring the file structure matches the expected format.
   (i.e., all 160 FLDK files for the same time block containing B01-B16)

3. Run data processing
   ```bash
   cd ..
   cargo run --release
   ```
   The program will automatically process all time-series data and generate corresponding TIFF image files.

## Data Processing Pipeline
1. Data reading: Parse HSD file format, extract raw observation data
2. Correction processing:
   - Data calibration correction
   - Rayleigh atmospheric correction
   - Geometric parameter calculation
3. Image synthesis:
   - Multi-band data fusion
   - Color space conversion (linear sRGB)
   - Lanczos interpolation scaling
4. Output: Generate full-disk true-color TIFF images