# Cargo Accelerate

[![CI](https://github.com/khulnasoft/cargo-accelerate/actions/workflows/build.yml/badge.svg)](https://github.com/khulnasoft/cargo-accelerate/actions/workflows/build.yml)
[![Crates.io](https://img.shields.io/crates/v/cargo-accelerate.svg)](https://crates.io/crates/cargo-accelerate)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange)](https://www.rust-lang.org)

Zero-config CLI tool that diagnoses and optimizes Rust project compile times.

```bash
cargo install cargo-accelerate
cargo accelerate doctor
```

## Quick Start

```bash
# Diagnose your project
cargo accelerate doctor

# Auto-apply every optimization
cargo accelerate optimize

# Measure the improvement
cargo accelerate benchmark
```

---

## Commands

### `doctor` — Diagnose bottlenecks

Analyzes your project and environment for build speed issues:

```bash
cargo accelerate doctor
```

Checks for:
- Workspace vs single-package structure
- Incremental compilation and `codegen-units` settings
- `sccache` installation and configuration
- Fast linker (`mold` / `lld`) installation and configuration
- `cargo-nextest` availability

Output shows each check result and prints recommendations for anything missing or suboptimal.

---

### `optimize` — Auto-apply optimizations

Surgically writes optimal settings to `Cargo.toml` and `.cargo/config.toml`:

```bash
cargo accelerate optimize
```

Applies:
- Scenario-aware profiles (dev, ci, release) — tuned for fast iteration, CI throughput, and release performance
- RUSTFLAGS optimization — `-C target-cpu=native` for native CPU instruction support in CI/release builds
- Enables `sccache` as `rustc-wrapper` (if installed)
- Configures fast linker (`mold` / `lld`) per-platform (if installed)
- CI workflow generation (`.github/workflows/build.yml`)
- Policy template (`.cargo-accelerate/policy.toml`)

Uses `toml_edit` AST parsing — preserves comments, formatting, and unrelated config.

---

### `benchmark` — Measure before/after compile times

Compares build performance with and without optimizations:

```bash
cargo accelerate benchmark
```

Flow:
1. Backs up `Cargo.toml` and `.cargo/config.toml`
2. Measures unoptimized times (no incremental, codegen-units=1, no sccache/linker)
3. Measures optimized times (incremental, codegen-units=256, sccache, fast linker)
4. Restores original configuration (always — even on error)
5. Prints comparison report

Output:
```
================ Benchmark Report ================
Cargo Check:  9.20s  ➔  2.10s   (77.2% Saved)
Cargo Build:  92.50s  ➔  26.80s  (71.0% Saved)
Cargo Test :  12.40s  ➔  4.10s   (66.9% Saved)
Cargo Clippy: 14.10s  ➔  3.20s   (77.3% Saved)
--------------------------------------------------
Total Time :  128.20s ➔  36.20s  (71.8% Saved)
```

---

### `cache` — Manage sccache

```bash
cargo accelerate cache status    # Show sccache configuration and stats
cargo accelerate cache enable    # Set sccache as rustc-wrapper in config
cargo accelerate cache disable   # Remove sccache wrapper from config
```

Without a subcommand, defaults to `status`.

Remote cache via `sccache-dist`:

```bash
cargo accelerate cache remote --enable     # Enable remote cache config
cargo accelerate cache remote              # Disable remote cache config
```

---

### `linker` — Configure fast linker

Detects OS and configures the fastest available linker:

```bash
cargo accelerate linker
```

| OS      | Preferred linker | Fallback |
|---------|-----------------|----------|
| Linux   | `mold`          | `lld`    |
| macOS   | `lld`           | —        |
| Windows | `lld-link`      | `lld`    |

Writes target-specific `rustflags` to `.cargo/config.toml`.

---

### `workspace` — Analyze crate architecture

Scans workspace members and estimates compile weights:

```bash
cargo accelerate workspace
```

Output:
```
Crate Name            Lines of Code  Direct Deps     Est. Compile Time
----------------------------------------------------------------------
backend               8542           12               45.21s
shared-types          3201           5                17.51s
cli                   1502           8                10.01s
```

Highlights crates with >5000 LoC as candidates for splitting.

---

### `deps` — Find heavyweight dependencies

Analyzes the full transitive dependency graph, sorted by estimated compile impact:

```bash
cargo accelerate deps
```

Output:
```
Dependency Name         Version    Transitive Deps    Est. Compile Time
-------------------------------------------------------------------------
syn                     2.0.118    3                  15.1s
tokio                   1.52.3     18                 14.7s
clap                    4.6.1      20                 10.7s
...
```

---

### `ci` — Generate CI workflows

Creates optimized GitHub Actions and Docker build configs:

```bash
cargo accelerate ci
```

Generates:
- `.github/workflows/build.yml` — `sccache-action`, `Swatinem/rust-cache`, `cargo-nextest`, clippy with `-D warnings`
- `Dockerfile` — cargo-chef multi-stage build with `rust-1.84`

---

### `watch` — Continuous check/test/clippy

```bash
cargo accelerate watch
```

Runs `cargo check && cargo test && cargo clippy` on every file change (requires `cargo-watch`).

---

### `install` — Install optimization tools

Auto-installs missing build tooling:

```bash
cargo accelerate install
```

Attempts installation via `cargo-binstall` (fast), falling back to `cargo install` (slow). Installs: `sccache`, `mold`/`lld`, `cargo-nextest`, `cargo-watch`, `cargo-chef`.

---

### `graph` — Analyze build graph and critical path

Maps the crate dependency graph and identifies the critical path (longest compile chain):

```bash
cargo accelerate graph
```

Features:
- **Critical path** — the longest dependency chain that gates build time
- **Fan-in / fan-out analysis** — identifies widely-used utility crates and crates with high internal dependency counts
- **Workspace partitioning candidates** — flags crates that could be extracted or made optional to reduce CI compile times
- **Split recommendations** — highlights crates with estimated compile cost >10s

---

### `regression` — Track build performance regressions

Measures build times and compares against a saved baseline:

```bash
cargo accelerate regression                        # Run measurement
cargo accelerate regression --save                  # Save a new baseline
cargo accelerate regression --compare               # Compare against baseline
cargo accelerate regression --budget 300            # Fail if build exceeds 300s
cargo accelerate regression --compare --threshold 5 # Warn on >5% regression
```

Saves baseline to `.cargo-accelerate/baseline.json`. The `--budget` flag enforces a hard cap on total build time.

---

### `policy` — Enforce build-quality policies

Creates and validates a project-level build policy file:

```bash
cargo accelerate policy
```

Generates `.cargo-accelerate/policy.toml` with defaults. On subsequent runs, validates the project against profile, cache, linker, and CI quality standards.

---

### `profile` — Generate scenario-aware build profiles

Preview or apply optimized profiles for different scenarios:

```bash
cargo accelerate profile              # Preview all profiles
cargo accelerate profile dev          # Apply dev profile (fast iteration)
cargo accelerate profile test         # Apply test profile
cargo accelerate profile ci           # Apply CI profile (optimized for CI)
cargo accelerate profile release      # Apply release profile (max runtime perf)
```

| Scenario | Incremental | Codegen Units | Opt Level | LTO |
|----------|-------------|---------------|-----------|-----|
| dev      | yes         | 256           | 0         | no  |
| test     | yes         | 256           | 0         | no  |
| ci       | no          | 1             | 2         | yes |
| release  | no          | 1             | 3         | yes |

---

### `trace` — Capture build phases and identify bottlenecks

Measures each build phase (check, build, test, clippy) and identifies the slowest:

```bash
cargo accelerate trace                     # Run and display report
cargo accelerate trace --export-json       # Export as JSON for CI tooling
cargo accelerate trace --export-html       # Generate interactive HTML report
```

Output flags phases >30s in red and >10s in yellow. Provides recommendations for builds exceeding time thresholds.

---

### `trend` — Track build performance over time

Periodically measures build times and visualizes the trend across recent runs:

```bash
cargo accelerate trend
```

Stores up to 30 records in `.cargo-accelerate/trends.json`. Detects and reports degradation (>5% increase over 3 runs) or improvement. Shows last 10 runs in a table with median time.

---

### `audit` — Comprehensive build health check

Evaluates four dimensions of build configuration:

```bash
cargo accelerate audit                             # Full audit
cargo accelerate audit --skip-size                 # Skip binary size check
cargo accelerate audit --skip-rustflags            # Skip RUSTFLAGS check
cargo accelerate audit --skip-features             # Skip dependency features check
cargo accelerate audit --skip-parallel             # Skip parallelism check
```

Checks:
- **RUSTFLAGS** — is `target-cpu=native` set? Is a fast linker configured?
- **Dependency features** — are heavy crates (`syn`, `tokio`, etc.) using default features unnecessarily?
- **Parallel build** — is `codegen-units` high enough? Are CPU cores underutilized?
- **Binary size** — runs `cargo-bloat` or `cargo-size` if installed (degradates gracefully)

---

### `auto` — Interactive guided automation wizard

Walks through the full optimization pipeline step by step:

```bash
cargo accelerate auto                              # Interactive, preview-only
cargo accelerate auto --apply                      # Apply changes automatically
cargo accelerate auto --apply --non-interactive    # Fully automated, no prompts
cargo accelerate auto --skip-cache                 # Skip sccache step
cargo accelerate auto --skip-linker --skip-ci      # Skip specific steps
```

Steps: Doctor → Cache → Linker → Profile → CI → Policy. Each step prompts before execution in interactive mode. Failed steps can be skipped without aborting.

---

### `cache remote` — Remote cache orchestration

Configure or disable remote caching via `sccache-dist`:

```bash
cargo accelerate cache remote --enable    # Enable remote cache config
cargo accelerate cache remote             # Disable remote cache config
```

Prints environment variable setup instructions for connecting to a `sccache-dist` server. Degrades gracefully if `sccache-dist` is not installed.

---

## Features

| Command | Purpose |
|---------|---------|
| `doctor` | Diagnose bottlenecks in project & environment |
| `optimize` | Auto-apply all optimizations |
| `benchmark` | Measure before/after compile times |
| `audit` | Comprehensive build health check |
| `cache` | Manage sccache (local & remote) |
| `linker` | Configure mold/lld fast linker |
| `workspace` | Analyze crate architecture |
| `deps` | Find heavyweight dependencies |
| `graph` | Analyze build graph & critical path |
| `profile` | Generate scenario-aware profiles |
| `trace` | Capture build phases & bottlenecks |
| `trend` | Track performance over time |
| `regression` | Guard against performance regressions |
| `policy` | Enforce build-quality policies |
| `ci` | Generate optimized CI workflows |
| `watch` | Continuous check/test/clippy |
| `daemon` | Background cache warming |
| `auto` | Interactive guided setup |
| `install` | Install optimization tools |

## Installation

### From crates.io (recommended)
```bash
cargo install cargo-accelerate
```

### From source
```bash
git clone https://github.com/khulnasoft/cargo-accelerate.git
cd cargo-accelerate
cargo install --path .
```

### Using the installer script
```bash
curl -fsSL https://raw.githubusercontent.com/khulnasoft/cargo-accelerate/main/install.sh | bash
# or locally:
./install.sh
```

### Post-install
```bash
# Verify it works
cargo accelerate doctor

# Install companion tools
cargo accelerate install
```

---

## Project Status

Active development. The goal is to become the standard `cargo` companion for
build-performance management — similar to what `cargo-audit` does for security,
but for compile times.

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT — see [LICENSE](LICENSE).
