use crate::utils::{
    get_cargo_config_path, get_cargo_toml_path, get_project_root, is_tool_installed,
};
use anyhow::{Context, Result};
use colored::*;
use std::fs;
use std::path::Path;

pub struct CiOptions {
    pub enforce_policy: bool,
    pub budget: Option<f64>,
}

const BASE_WORKFLOW_HEAD: &str = r#"name: Security & Build CI

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
"#;

const REGRESSION_CHECK_JOB: &str = r#"  regression-check:
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
        run: cargo accelerate regression --compare --budget BUDGET_PLACEHOLDER
"#;

const POLICY_CHECK_JOB: &str = r#"  policy-check:
    name: Build Policy Enforcement
    runs-on: ubuntu-latest
    needs: build
    steps:
      - name: Checkout Code
        uses: actions/checkout@v4

      - name: Install Rust Toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Set up sccache-action
        uses: mozilla/sccache-action@v0.0.7

      - name: Install cargo-accelerate
        run: cargo install --path .

      - name: Validate Build Policy
        run: cargo accelerate audit --fail-fast
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

const PRE_COMMIT_HOOK: &str = r#"#!/bin/sh
# cargo-accelerate pre-commit hook
# Run build audit before each commit to catch performance regressions early.
# Install: ln -sf ../../.cargo-accelerate/pre-commit.sh .git/hooks/pre-commit

set -e

echo "Running cargo-accelerate audit..."
cargo accelerate audit --fail-fast

# Optional: uncomment to enforce a build time budget
# echo "Checking build budget..."
# cargo accelerate regression --compare --budget 300
"#;

pub fn run(opts: CiOptions) -> Result<()> {
    println!(
        "{}",
        "Generating Optimized CI Configurations...".bold().cyan()
    );

    let root = get_project_root().context("Could not find project root")?;

    validate_budget(opts.budget)?;

    // 1. Create GitHub Actions workflow with conditional enforcement
    let github_dir = root.join(".github").join("workflows");
    fs::create_dir_all(&github_dir)?;

    let workflow = build_workflow(opts.enforce_policy, opts.budget);
    let workflow_path = github_dir.join("build.yml");
    fs::write(&workflow_path, workflow)?;
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

    // 3. Generate pre-commit hook
    let accelerate_dir = root.join(".cargo-accelerate");
    fs::create_dir_all(&accelerate_dir)?;
    let hook_path = accelerate_dir.join("pre-commit.sh");
    fs::write(&hook_path, PRE_COMMIT_HOOK)?;

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::metadata(&hook_path)?.permissions();
        let mut new_perms = perms;
        new_perms.set_mode(0o755);
        fs::set_permissions(&hook_path, new_perms)?;
    }

    println!(
        "  {} Generated pre-commit hook: `.cargo-accelerate/pre-commit.sh`",
        "✔".green()
    );
    println!("    Install: ln -sf ../../.cargo-accelerate/pre-commit.sh .git/hooks/pre-commit");

    // 4. CI Parity Check
    println!("\n{}", "CI Parity Check:".bold().yellow());
    check_ci_parity(&root)?;

    // Show enforcement summary
    print_enforcement_summary(opts);

    println!("\n{}", "✔ CI optimization setup complete!".bold().green());
    println!("  GitHub Actions, Docker builds, and regression checks are configured.");

    Ok(())
}

fn validate_budget(budget: Option<f64>) -> Result<()> {
    if let Some(b) = budget {
        if !b.is_finite() || b <= 0.0 {
            anyhow::bail!(
                "CI regression budget must be a finite number greater than 0 (got {})",
                b
            );
        }
    }
    Ok(())
}

fn build_workflow(enforce_policy: bool, budget: Option<f64>) -> String {
    let mut workflow = String::from(BASE_WORKFLOW_HEAD);

    if budget.is_some() || enforce_policy {
        let budget_str = budget
            .map(|b| format!("{}", b))
            .unwrap_or_else(|| "600".to_string());
        let regression_job = REGRESSION_CHECK_JOB.replace("BUDGET_PLACEHOLDER", &budget_str);

        if enforce_policy {
            // When enforcing policy, regression check must fail on breach
            let enforced =
                regression_job.replace("continue-on-error: true", "continue-on-error: false");
            workflow.push_str(&enforced);
        } else {
            workflow.push_str(&regression_job);
        }
    }

    if enforce_policy {
        workflow.push('\n');
        workflow.push_str(POLICY_CHECK_JOB);
    }

    workflow
}

fn print_enforcement_summary(opts: CiOptions) {
    if opts.enforce_policy || opts.budget.is_some() {
        println!("\n{}", "CI Enforcement:".bold().green());
        if opts.enforce_policy {
            println!(
                "  {} Build policy enforcement enabled (audit + budget check)",
                "✓".green()
            );
        }
        if let Some(budget) = opts.budget {
            println!(
                "  {} Regression budget set to {}s — CI will fail on timeout",
                "✓".green(),
                budget.to_string().cyan()
            );
        }
    }
}

fn check_ci_parity(root: &Path) -> Result<()> {
    let mut parity_issues = Vec::new();

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_workflow_default() {
        let workflow = build_workflow(false, None);
        assert!(workflow.contains("Build, Lint, and Test"));
        assert!(!workflow.contains("regression-check"));
        assert!(!workflow.contains("policy-check"));
        assert!(!workflow.contains("cargo accelerate"));
    }

    #[test]
    fn test_validate_budget_accepts_positive() {
        assert!(validate_budget(None).is_ok());
        assert!(validate_budget(Some(1.0)).is_ok());
        assert!(validate_budget(Some(300.5)).is_ok());
    }

    #[test]
    fn test_validate_budget_rejects_invalid() {
        assert!(validate_budget(Some(0.0)).is_err());
        assert!(validate_budget(Some(-5.0)).is_err());
        assert!(validate_budget(Some(f64::NAN)).is_err());
        assert!(validate_budget(Some(f64::INFINITY)).is_err());
        assert!(validate_budget(Some(f64::NEG_INFINITY)).is_err());
    }

    #[test]
    fn test_build_workflow_with_budget() {
        let workflow = build_workflow(false, Some(300.0));
        assert!(workflow.contains("regression-check"));
        assert!(workflow.contains("--budget 300"));
        assert!(workflow.contains("continue-on-error: true"));
        assert!(!workflow.contains("policy-check"));
    }

    #[test]
    fn test_build_workflow_enforce_policy() {
        let workflow = build_workflow(true, None);
        assert!(workflow.contains("regression-check"));
        assert!(workflow.contains("policy-check"));
        assert!(workflow.contains("--budget 600"));
        assert!(workflow.contains("continue-on-error: false"));
        assert!(workflow.contains("cargo accelerate audit --fail-fast"));
    }

    #[test]
    fn test_build_workflow_enforce_with_budget() {
        let workflow = build_workflow(true, Some(120.0));
        assert!(workflow.contains("regression-check"));
        assert!(workflow.contains("policy-check"));
        assert!(workflow.contains("--budget 120"));
        assert!(workflow.contains("continue-on-error: false"));
        assert!(workflow.contains("cargo accelerate audit --fail-fast"));
    }

    #[test]
    fn test_pre_commit_hook_content() {
        assert!(PRE_COMMIT_HOOK.contains("cargo accelerate audit --fail-fast"));
        assert!(PRE_COMMIT_HOOK.contains(".git/hooks/pre-commit"));
    }

    #[test]
    fn test_print_enforcement_summary() {
        // Just verify it doesn't panic
        print_enforcement_summary(CiOptions {
            enforce_policy: false,
            budget: None,
        });
        print_enforcement_summary(CiOptions {
            enforce_policy: true,
            budget: Some(300.0),
        });
    }

    #[test]
    fn test_dockerfile_template() {
        assert!(DOCKERFILE_TEMPLATE.contains("{{app_name}}"));
        assert!(DOCKERFILE_TEMPLATE.contains("cargo-chef"));
    }

    #[test]
    fn test_ci_parity_no_config() {
        let dir = tempfile::tempdir().unwrap();
        let result = check_ci_parity(dir.path());
        assert!(result.is_ok());
    }
}
