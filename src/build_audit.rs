use crate::utils::{
    available_cpus, get_cargo_config_path, get_cargo_toml_path, get_project_root, is_tool_installed,
};
use anyhow::{Context, Result};
use colored::*;
use std::fs;
use std::path::Path;
use std::process::Command;

pub struct AuditOptions {
    pub check_size: bool,
    pub check_rustflags: bool,
    pub check_features: bool,
    pub check_parallel: bool,
}

impl Default for AuditOptions {
    fn default() -> Self {
        Self {
            check_size: true,
            check_rustflags: true,
            check_features: true,
            check_parallel: true,
        }
    }
}

pub fn run(options: AuditOptions) -> Result<()> {
    println!("{}", "Comprehensive Build Audit...".bold().cyan());

    let root = get_project_root().context("Could not find project root")?;
    let mut issues = Vec::new();
    let mut passed = 0u32;

    if options.check_rustflags {
        println!("\n{}", "RUSTFLAGS Check:".bold());
        passed += check_rustflags_optimization(&root, &mut issues)?;
    }

    if options.check_features {
        println!("\n{}", "Dependency Feature Check:".bold());
        passed += check_dependency_features(&root, &mut issues)?;
    }

    if options.check_parallel {
        println!("\n{}", "Parallel Build Check:".bold());
        passed += check_parallel_build(&root, &mut issues)?;
    }

    if options.check_size {
        println!("\n{}", "Binary Size Check:".bold());
        if is_tool_installed("cargo-bloat") || is_tool_installed("cargo-size") {
            check_binary_size(&root)?;
            passed += 1;
        } else {
            println!("  Install cargo-bloat for binary size analysis: `cargo install cargo-bloat`");
            println!("  Install cargo-size for detailed size info: `cargo install cargo-size`");
            issues.push("Binary size analysis tools not installed (cargo-bloat)".into());
        }
    }

    println!("\n{}", "Audit Summary:".bold());
    println!("  {} checks passed", passed.to_string().green());
    if !issues.is_empty() {
        println!("  {} issues found:", issues.len().to_string().red());
        for issue in &issues {
            println!("    - {}", issue);
        }
    }

    Ok(())
}

fn check_rustflags_optimization(root: &Path, issues: &mut Vec<String>) -> Result<u32> {
    let mut score = 0u32;
    let config_path = get_cargo_config_path(root);

    let config_content = if config_path.exists() {
        Some(fs::read_to_string(config_path)?)
    } else {
        None
    };

    if let Some(ref content) = config_content {
        if content.contains("target-cpu") {
            println!("  ✔ target-cpu is set (good for release/CI builds)");
            score += 1;
        } else {
            println!("  ✖ target-cpu not set in rustflags");
            println!(
                "    → Add `-C target-cpu=native` to [target.*.rustflags] for CI/release builds"
            );
            issues.push("Set target-cpu=native in rustflags for better runtime performance".into());
        }

        if content.contains("link-arg=-fuse-ld=mold") || content.contains("link-arg=-fuse-ld=lld") {
            println!("  ✔ Fast linker enabled in rustflags");
            score += 1;
        } else {
            println!("  ✖ Fast linker not configured in rustflags");
            issues.push("Configure mold/lld linker via cargo accelerate linker".into());
        }
    } else {
        println!("  ✖ No .cargo/config.toml found — no rustflags configured");
        issues.push("No .cargo/config.toml — run cargo accelerate optimize".into());
    }

    if score == 0 {
        Ok(0)
    } else {
        Ok(score)
    }
}

fn check_dependency_features(root: &Path, issues: &mut Vec<String>) -> Result<u32> {
    let audit = crate::features::analyze_features(root)?;

    if audit.suggestions.is_empty() {
        println!("  ✔ No dependencies to analyze");
        return Ok(1);
    }

    let mut score = 0u32;
    let mut found_issues = false;

    for s in &audit.suggestions {
        if s.is_optimized {
            println!(
                "  ✔ '{}' features already optimized ({})",
                s.package_name.cyan(),
                s.recommended_features.join(", ")
            );
            score += 1;
        } else if !s.recommended_features.is_empty() {
            println!(
                "  ✖ '{}' uses {} default features — suggest: {} (saves ~{:.0}%)",
                s.package_name.cyan(),
                s.current_default_features.len(),
                s.recommended_features.join(", ").yellow(),
                s.estimated_savings_pct
            );
            issues.push(format!(
                "Disable unused default features on '{}': set default-features = false, features = [{}]",
                s.package_name,
                s.recommended_features.iter().map(|f| format!("\"{}\"", f)).collect::<Vec<_>>().join(", ")
            ));
            found_issues = true;
        }
    }

    if !found_issues {
        println!("  ✔ All dependencies are optimized");
        if score == 0 {
            score = 1;
        }
    }

    Ok(score)
}

fn check_parallel_build(root: &Path, issues: &mut Vec<String>) -> Result<u32> {
    let cargo_toml = get_cargo_toml_path(root);
    if !cargo_toml.exists() {
        return Ok(0);
    }

    let content = fs::read_to_string(&cargo_toml)?;
    let parsed: toml::Value = toml::from_str(&content)?;

    let cpus = available_cpus();
    let suggested_dev_codegen = (cpus * 2).min(256) as i64;
    let overhead_threshold = (cpus * 4) as i64;

    let dev_codegen = parsed
        .get("profile")
        .and_then(|p| p.get("dev"))
        .and_then(|d| d.get("codegen-units"))
        .and_then(|c| c.as_integer());

    let mut score = 0u32;

    match dev_codegen {
        Some(cu) => {
            if cu > overhead_threshold {
                println!(
                    "  ✖ codegen-units = {} (exceeds {} × cpus = {} — thread scheduling overhead may negate benefits)",
                    cu, 4, overhead_threshold
                );
                issues.push(format!(
                    "Reduce codegen-units in [profile.dev] from {} to at most {} (4 × {} CPUs) to avoid excessive parallelism overhead",
                    cu, overhead_threshold, cpus
                ));
            } else if cu >= suggested_dev_codegen {
                println!(
                    "  ✔ codegen-units = {} (well-matched to {} CPU cores, ratio {:.1}×)",
                    cu,
                    cpus,
                    cu as f64 / cpus as f64
                );
                score += 1;
            } else {
                println!(
                    "  ✖ codegen-units = {} (low for {} CPU cores — may underutilize; suggest {} = {} × 2)",
                    cu, cpus, suggested_dev_codegen, cpus
                );
                issues.push(format!(
                    "Increase codegen-units in [profile.dev] from {} to {} (2× {} CPUs) to better utilize cores",
                    cu, suggested_dev_codegen, cpus
                ));
            }
        }
        None => {
            let default_cu = 256i64;
            if default_cu > overhead_threshold {
                println!(
                    "  ℹ  codegen-units defaults to 256 — {} CPU cores detected, suggest reducing to {} (4× {} CPUs) to avoid overhead",
                    cpus, overhead_threshold, cpus
                );
            } else {
                println!(
                    "  ✔ codegen-units defaults to 256 (well-matched to {} CPU cores)",
                    cpus
                );
                score += 1;
            }
        }
    }

    let cargo_config = get_cargo_config_path(root);
    if cargo_config.exists() {
        let config_content = fs::read_to_string(&cargo_config)?;
        if !config_content.contains("build.jobs") && cpus > 8 {
            println!(
                "  ℹ  More than 8 cores available — consider setting `build.jobs` in .cargo/config.toml"
            );
        }
    }

    println!(
        "  Available CPU cores: {} (suggested dev codegen-units: {})",
        cpus, suggested_dev_codegen
    );
    println!("  Parallelism ratio: codegen-units should be 1–2× CPUs for dev, 1 for CI/release");

    Ok(score)
}

fn check_binary_size(root: &Path) -> Result<()> {
    if is_tool_installed("cargo-bloat") {
        println!("  Running cargo bloat...");
        let output = Command::new("cargo")
            .args(["bloat", "--release"])
            .current_dir(root)
            .output();

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                for line in stdout.lines().take(20) {
                    println!("    {}", line);
                }
                if !out.status.success() {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    println!(
                        "    {} cargo bloat had issues: {}",
                        "⚠".yellow(),
                        stderr.lines().next().unwrap_or("")
                    );
                }
            }
            Err(_) => {
                println!("  Could not run cargo bloat");
            }
        }
    } else if is_tool_installed("cargo-size") {
        println!("  Running cargo size...");
        let output = Command::new("cargo")
            .args(["size", "--release"])
            .current_dir(root)
            .output();

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                for line in stdout.lines().take(20) {
                    println!("    {}", line);
                }
            }
            Err(_) => {
                println!("  Could not run cargo size");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_cargo_toml(dir: &TempDir, content: &str) {
        fs::write(dir.path().join("Cargo.toml"), content).unwrap();
    }

    fn create_workspace_with_path_dep(dir: &TempDir, manifest: &str, dep_name: &str) {
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("dep/src")).unwrap();
        fs::write(root.join("Cargo.toml"), manifest).unwrap();
        fs::write(
            root.join("dep/Cargo.toml"),
            format!(
                "[package]\nname = \"{}\"\nversion = \"1.0.0\"\n[features]\ndefault = [\"full\"]\nfull = []\nderive = []\n",
                dep_name
            ),
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn x() {}\n").unwrap();
        fs::write(root.join("dep/src/lib.rs"), "pub fn y() {}\n").unwrap();
    }

    fn create_cargo_config(dir: &TempDir, content: &str) {
        let config_dir = dir.path().join(".cargo");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("config.toml"), content).unwrap();
    }

    #[test]
    fn test_check_rustflags_missing_config() {
        let dir = TempDir::new().unwrap();
        let mut issues = Vec::new();
        let result = check_rustflags_optimization(dir.path(), &mut issues).unwrap();
        assert_eq!(result, 0);
        assert!(!issues.is_empty());
    }

    #[test]
    fn test_check_rustflags_with_config() {
        let dir = TempDir::new().unwrap();
        create_cargo_config(
            &dir,
            r#"
            [target.x86_64-unknown-linux-gnu]
            rustflags = ["-C", "target-cpu=native", "-C", "link-arg=-fuse-ld=mold"]
        "#,
        );
        let mut issues = Vec::new();
        let result = check_rustflags_optimization(dir.path(), &mut issues).unwrap();
        assert!(result > 0);
    }

    #[test]
    fn test_check_parallel_build_default() {
        let dir = TempDir::new().unwrap();
        create_cargo_toml(&dir, "[package]\nname = \"test\"\n");
        let mut issues = Vec::new();
        let result = check_parallel_build(dir.path(), &mut issues).unwrap();
        // Default codegen-units in dev is 256; if on <=64 CPUs, 256 should be fine
        let cpus = available_cpus();
        if cpus * 4 < 256 {
            // On machines with <64 CPUs, 256 CU may exceed overhead threshold
            assert_eq!(result, 0, "expected 0 with low CPU count");
        } else {
            assert_eq!(result, 1, "expected 1 on machines with sufficient CPUs");
        }
    }

    #[test]
    fn test_check_parallel_build_low_codegen() {
        let dir = TempDir::new().unwrap();
        create_cargo_toml(
            &dir,
            r#"
            [package]
            name = "test"
            [profile.dev]
            codegen-units = 1
        "#,
        );
        let mut issues = Vec::new();
        let result = check_parallel_build(dir.path(), &mut issues).unwrap();
        // codegen-units=1 is always too low for any multi-core machine
        assert_eq!(result, 0);
        assert!(!issues.is_empty());
    }

    #[test]
    fn test_check_parallel_build_high_codegen() {
        let dir = TempDir::new().unwrap();
        let cpus = available_cpus();
        let too_high = (cpus * 4 + 1) as i64;
        create_cargo_toml(
            &dir,
            &format!(
                r#"
            [package]
            name = "test"
            [profile.dev]
            codegen-units = {}
        "#,
                too_high
            ),
        );
        let mut issues = Vec::new();
        let result = check_parallel_build(dir.path(), &mut issues).unwrap();
        assert_eq!(result, 0);
        assert!(!issues.is_empty());
        let has_overhead_warning = issues.iter().any(|i| i.contains("4 ×"));
        assert!(
            has_overhead_warning,
            "expected overhead warning, got: {:?}",
            issues
        );
    }

    #[test]
    fn test_check_dependency_features_defaults() {
        let dir = TempDir::new().unwrap();
        create_workspace_with_path_dep(
            &dir,
            r#"
            [package]
            name = "test"
            [dependencies]
            syn = { path = "dep" }
        "#,
            "syn",
        );
        let mut issues = Vec::new();
        let result = check_dependency_features(dir.path(), &mut issues).unwrap();
        assert!(result == 0 || result == 1);
    }

    #[test]
    fn test_check_dependency_features_explicit_no_default() {
        let dir = TempDir::new().unwrap();
        create_workspace_with_path_dep(
            &dir,
            r#"
            [package]
            name = "test"
            [dependencies]
            syn = { path = "dep", default-features = false }
        "#,
            "syn",
        );
        let mut issues = Vec::new();
        let result = check_dependency_features(dir.path(), &mut issues).unwrap();
        assert!(result > 0);
    }

    #[test]
    fn test_audit_options_default() {
        let opts = AuditOptions::default();
        assert!(opts.check_size);
        assert!(opts.check_rustflags);
        assert!(opts.check_features);
        assert!(opts.check_parallel);
    }
}
