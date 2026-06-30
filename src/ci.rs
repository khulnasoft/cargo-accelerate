use crate::utils::{
    get_cargo_config_path, get_cargo_toml_path, get_project_root, is_tool_installed,
};
use anyhow::{Context, Result};
use colored::*;
use std::fs;

const WORKFLOW_TEMPLATE: &str = r#"name: Security & Build CI

on:
  push:
    branches: [ main, master ]
  pull_request:
    branches: [ main, master ]

env:
  CARGO_TERM_COLOR: always
  SCCACHE_GHA_ENABLED: "true"
  RUSTC_WRAPPER: "sccache"

jobs:
  build:
    name: Build, Lint, and Test
    runs-on: ubuntu-latest

    steps:
      - name: Checkout Code
        uses: actions/checkout@v4

      - name: Install Rust Toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt

      - name: Set up sccache-action
        uses: mozilla/sccache-action@v0.0.7

      - name: Enable Rust Cache (Swatinem/rust-cache)
        uses: Swatinem/rust-cache@v2
        with:
          key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}

      - name: Install cargo-nextest
        uses: taiki-e/install-action@nextest

      - name: Check Formatting
        run: cargo fmt --all -- --check

      - name: Run Clippy
        run: cargo clippy --all-targets --all-features -- -D warnings

      - name: Build Workspace
        run: cargo build --all-targets --all-features

      - name: Run Tests (Nextest)
        run: cargo nextest run --all-features

  regression-check:
    name: Performance Regression Check
    runs-on: ubuntu-latest
    needs: build
    continue-on-error: true
    steps:
      - name: Checkout Code
        uses: actions/checkout@v4

      - name: Install Rust Toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Set up sccache-action
        uses: mozilla/sccache-action@v0.0.7

      - name: Install cargo-accelerate
        run: cargo install --path .

      - name: Run Regression Check
        run: cargo accelerate regression --compare --budget 600
"#;

const DOCKERFILE_TEMPLATE: &str = r#"# Cargo-chef Dockerfile recipe for optimized multi-stage Docker builds
FROM lukemathwalker/cargo-chef:latest-rust-1.84 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder 
COPY --from=planner /app/recipe.json recipe.json
# Build dependencies - this layer is cached unless dependencies change
RUN cargo chef cook --release --recipe-path recipe.json
# Build application
COPY . .
RUN cargo build --release

# Run-time Stage
FROM debian:bookworm-slim
WORKDIR /app
COPY --from=builder /app/target/release/{{app_name}} /usr/local/bin/app
CMD ["app"]
"#;

pub fn run() -> Result<()> {
    println!(
        "{}",
        "Generating Optimized CI Configurations...".bold().cyan()
    );

    let root = get_project_root().context("Could not find project root")?;

    // 1. Create GitHub Actions workflow
    let github_dir = root.join(".github").join("workflows");
    fs::create_dir_all(&github_dir)?;

    let workflow_path = github_dir.join("build.yml");
    fs::write(&workflow_path, WORKFLOW_TEMPLATE)?;
    println!(
        "  {} Generated GitHub Actions CI workflow: `.github/workflows/build.yml`",
        "✔".green()
    );

    // 2. Generate Cargo Chef Dockerfile
    let cargo_toml_path = root.join("Cargo.toml");
    let app_name = if cargo_toml_path.exists() {
        let content = fs::read_to_string(&cargo_toml_path)?;
        let parsed: toml::Value = toml::from_str(&content)?;
        parsed
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("app")
            .to_string()
    } else {
        "app".to_string()
    };

    let dockerfile_content = DOCKERFILE_TEMPLATE.replace("{{app_name}}", &app_name);
    let dockerfile_path = root.join("Dockerfile");
    fs::write(&dockerfile_path, dockerfile_content)?;
    println!(
        "  {} Generated cargo-chef optimized Dockerfile: `Dockerfile`",
        "✔".green()
    );

    // 3. CI Parity Check
    println!("\n{}", "CI Parity Check:".bold().yellow());
    check_ci_parity(&root)?;

    println!("\n{}", "✔ CI optimization setup complete!".bold().green());
    println!("  GitHub Actions, Docker builds, and regression checks are configured.");

    Ok(())
}

fn check_ci_parity(root: &std::path::Path) -> Result<()> {
    let mut parity_issues = Vec::new();

    // Check sccache
    let config_path = get_cargo_config_path(root);
    if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        let parsed: toml::Value = toml::from_str(&content)?;
        let has_sccache = parsed
            .get("build")
            .and_then(|b| b.get("rustc-wrapper"))
            .and_then(|w| w.as_str())
            .map(|s| s.contains("sccache"))
            .unwrap_or(false);
        if !has_sccache {
            parity_issues.push(
                "sccache is configured in CI but not locally. Run `cargo accelerate cache enable`.",
            );
        }

        let has_fast_linker = parsed
            .get("target")
            .and_then(|t| t.as_table())
            .map(|tbl| {
                tbl.values().any(|v| {
                    v.get("rustflags")
                        .and_then(|f| f.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|v| v.as_str())
                                .any(|s| s.contains("fuse-ld=mold") || s.contains("fuse-ld=lld"))
                        })
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        if !has_fast_linker {
            parity_issues
                .push("Fast linker (mold/lld) suggested for CI. Run `cargo accelerate linker`.");
        }
    } else {
        parity_issues
            .push("No .cargo/config.toml found. CI uses sccache but local config is missing.");
    }

    // Check profile parity
    let cargo_toml_path = get_cargo_toml_path(root);
    if cargo_toml_path.exists() {
        let content = fs::read_to_string(&cargo_toml_path)?;
        let parsed: toml::Value = toml::from_str(&content)?;
        let has_lto = parsed
            .get("profile")
            .and_then(|p| p.get("release"))
            .and_then(|r| r.get("lto"))
            .is_some();
        if !has_lto {
            parity_issues
                .push("Release profile LTO not configured. CI release builds may be slower.");
        }
    }

    // Check tooling
    if !is_tool_installed("cargo-nextest") {
        parity_issues.push("cargo-nextest is used in CI but not installed locally. Run `cargo accelerate install`.");
    }

    if parity_issues.is_empty() {
        println!(
            "  {} Local configuration is in parity with CI settings.",
            "✔".green()
        );
    } else {
        println!("  {} CI parity issues found:", "⚠".yellow());
        for issue in &parity_issues {
            println!("    - {}", issue);
        }
    }

    Ok(())
}
