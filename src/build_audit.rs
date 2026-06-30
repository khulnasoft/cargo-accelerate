use crate::utils::{
    get_cargo_config_path, get_cargo_toml_path, get_project_root, is_tool_installed,
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
    let cargo_toml = get_cargo_toml_path(root);
    if !cargo_toml.exists() {
        return Ok(0);
    }

    let content = fs::read_to_string(&cargo_toml)?;
    let parsed: toml::Value = toml::from_str(&content)?;

    let mut score = 0u32;

    if let Some(deps) = parsed.get("dependencies").and_then(|d| d.as_table()) {
        let heavy_defaults = ["syn", "tokio", "serde", "clap", "hyper", "reqwest"];
        for dep_name in heavy_defaults {
            if let Some(dep) = deps.get(dep_name) {
                let has_default_features = dep
                    .get("default-features")
                    .and_then(|f| f.as_bool())
                    .unwrap_or(true);

                if has_default_features {
                    println!(
                        "  ✖ '{}' uses default features — consider disabling unused features",
                        dep_name.cyan()
                    );
                    issues.push(format!(
                        "Disable unused default features on '{}' to reduce compile time",
                        dep_name
                    ));
                } else {
                    println!("  ✔ '{}' has default-features = false", dep_name.cyan());
                    score += 1;
                }
            }
        }
    }

    if score == 0 {
        // No heavy deps or all need attention
        if issues.is_empty() {
            println!("  ✔ No heavy dependencies with default features detected");
            Ok(1)
        } else {
            Ok(0)
        }
    } else {
        Ok(score)
    }
}

fn check_parallel_build(root: &Path, issues: &mut Vec<String>) -> Result<u32> {
    let cargo_toml = get_cargo_toml_path(root);
    if !cargo_toml.exists() {
        return Ok(0);
    }

    let content = fs::read_to_string(&cargo_toml)?;
    let parsed: toml::Value = toml::from_str(&content)?;

    let dev_codegen = parsed
        .get("profile")
        .and_then(|p| p.get("dev"))
        .and_then(|d| d.get("codegen-units"))
        .and_then(|c| c.as_integer());

    let mut score = 0u32;

    match dev_codegen {
        Some(cu) if cu >= 128 => {
            println!("  ✔ codegen-units = {} (good parallelization in dev)", cu);
            score += 1;
        }
        Some(cu) => {
            println!(
                "  ✖ codegen-units = {} (low — may underutilize CPU cores)",
                cu
            );
            issues.push(format!(
                "Increase codegen-units to 256 in [profile.dev] (currently {})",
                cu
            ));
        }
        None => {
            println!("  ✔ codegen-units not set (defaults to 256 in dev)");
            score += 1;
        }
    }

    let available = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);

    println!("  Available CPU cores: {}", available);

    let cargo_config = get_cargo_config_path(root);
    if cargo_config.exists() {
        let config_content = fs::read_to_string(&cargo_config)?;
        if !config_content.contains("build.jobs") && available > 8 {
            println!(
                "  ℹ  More than 8 cores available — consider setting `build.jobs` in config.toml"
            );
        }
    }

    Ok(if score == 0 { 0 } else { score })
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
        assert_eq!(result, 1);
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
        assert_eq!(result, 0);
        assert!(!issues.is_empty());
    }

    #[test]
    fn test_check_dependency_features_defaults() {
        let dir = TempDir::new().unwrap();
        create_cargo_toml(
            &dir,
            r#"
            [package]
            name = "test"
            [dependencies]
            syn = "2.0"
            tokio = { version = "1.0", features = ["rt"] }
        "#,
        );
        let mut issues = Vec::new();
        let result = check_dependency_features(dir.path(), &mut issues).unwrap();
        assert!(result == 0 || result == 1);
    }

    #[test]
    fn test_check_dependency_features_explicit_no_default() {
        let dir = TempDir::new().unwrap();
        create_cargo_toml(
            &dir,
            r#"
            [package]
            name = "test"
            [dependencies]
            syn = { version = "2.0", default-features = false }
        "#,
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
