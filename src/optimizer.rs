use crate::ci::CiOptions;
use crate::cli::CacheAction;
use crate::profile::{self, Scenario};
use crate::utils::{get_cargo_config_path, get_cargo_toml_path, get_project_root};
use anyhow::{Context, Result};
use colored::*;
use std::fs;
use toml_edit::DocumentMut;

pub fn run() -> Result<()> {
    println!(
        "{}",
        "Automatically Optimizing Project Configuration..."
            .bold()
            .cyan()
    );

    let root = get_project_root().context("Could not find project root")?;

    // 1. Apply scenario-aware dev profile by default
    println!(
        "\n{}",
        "Applying scenario-aware profiles...".bold().yellow()
    );
    let cargo_toml_path = get_cargo_toml_path(&root);
    if cargo_toml_path.exists() {
        profile::apply_profile(&Scenario::Dev, &cargo_toml_path)?;
        profile::apply_profile(&Scenario::Ci, &cargo_toml_path)?;
        profile::apply_profile(&Scenario::Release, &cargo_toml_path)?;
    }

    // 2. Optimize RUSTFLAGS with target-cpu=native
    println!("\nOptimizing RUSTFLAGS...");
    if let Err(e) = optimize_rustflags(&root) {
        println!("  {} Could not optimize RUSTFLAGS: {}", "⚠".yellow(), e);
    }

    // 3. Enable sccache if available
    println!("\nConfiguring Sccache...");
    if let Err(e) = crate::cache::run(Some(CacheAction::Enable)) {
        println!("  {} Could not configure sccache: {}", "⚠".yellow(), e);
    }

    // 4. Configure fast linker if available
    println!("\nConfiguring Linker...");
    if let Err(e) = crate::linker::run() {
        println!("  {} Could not configure fast linker: {}", "⚠".yellow(), e);
    }

    // 5. Generate CI workflow
    println!("\nGenerating CI Configuration...");
    if let Err(e) = crate::ci::run(CiOptions {
        enforce_policy: false,
        budget: None,
    }) {
        println!("  {} Could not generate CI workflow: {}", "⚠".yellow(), e);
    }

    // 6. Create policy template if missing
    let policy_path = root.join(".cargo-accelerate").join("policy.toml");
    if !policy_path.exists() {
        println!("\nCreating policy template...");
        let dir = root.join(".cargo-accelerate");
        fs::create_dir_all(&dir)?;
        fs::write(
            &policy_path,
            crate::policy::BuildPolicy::default_policy_content(),
        )?;
        println!(
            "  {} Policy template created at {}",
            "✔".green(),
            policy_path.display()
        );
    }

    println!(
        "\n{}",
        "✔ Project successfully optimized for blazing fast builds!"
            .bold()
            .green()
    );

    Ok(())
}

fn optimize_rustflags(root: &std::path::Path) -> Result<()> {
    let config_path = get_cargo_config_path(root);
    let config_dir = config_path.parent().context("Invalid config path")?;
    fs::create_dir_all(config_dir)?;

    let mut doc = if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        content.parse::<DocumentMut>()?
    } else {
        DocumentMut::new()
    };

    let target = format!("target.{}", std::env::consts::ARCH);
    let target_key = if cfg!(target_os = "linux") {
        format!("{}-unknown-linux-gnu", target)
    } else if cfg!(target_os = "macos") {
        format!("{}-apple-darwin", target)
    } else if cfg!(target_os = "windows") {
        format!("{}-pc-windows-msvc", target)
    } else {
        "target".to_string()
    };

    if !doc.contains_key(&target_key) {
        doc[&target_key] = toml_edit::table();
    }

    let has_target_cpu = doc[&target_key]
        .get("rustflags")
        .and_then(|f| f.as_array())
        .map(|arr| {
            arr.iter().any(|v| {
                v.as_str()
                    .map(|s| s.contains("target-cpu"))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);

    if !has_target_cpu {
        if let Some(tgt) = doc.get_mut(&target_key) {
            let existing = tgt
                .get("rustflags")
                .and_then(|f| f.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();

            let mut flags = existing;
            flags.push("-C".to_string());
            flags.push("target-cpu=native".to_string());

            tgt["rustflags"] = toml_edit::Array::from_iter(flags).into();
            println!(
                "  {} Added -C target-cpu=native for {}",
                "✔".green(),
                target_key
            );
        }
    } else {
        println!("  {} target-cpu=native already configured", "✔".green());
    }

    fs::write(&config_path, doc.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_optimize_rustflags_creates_config() {
        let dir = TempDir::new().unwrap();
        assert!(optimize_rustflags(dir.path()).is_ok());
        let config_path = get_cargo_config_path(dir.path());
        assert!(config_path.exists());
        let content = fs::read_to_string(config_path).unwrap();
        assert!(content.contains("target-cpu"));
    }

    #[test]
    fn test_optimize_rustflags_already_present() {
        let dir = TempDir::new().unwrap();
        let config_path = get_cargo_config_path(dir.path());
        let config_dir = config_path.parent().unwrap();
        fs::create_dir_all(config_dir).unwrap();
        fs::write(
            &config_path,
            "[target.x86_64-unknown-linux-gnu]\nrustflags = [\"-C\", \"target-cpu=native\"]\n",
        )
        .unwrap();
        assert!(optimize_rustflags(dir.path()).is_ok());
    }
}
