use crate::utils::{
    get_cargo_config_path, get_cargo_toml_path, get_os, get_project_root, is_tool_installed,
};
use anyhow::Result;
use colored::*;
use std::fs;
use std::path::Path;

pub fn run() -> Result<()> {
    println!("{}", "Running Cargo Accelerate Doctor...".bold().cyan());

    let root = match get_project_root() {
        Ok(r) => r,
        Err(_) => {
            println!(
                "{} No cargo project or workspace detected in the current directory.",
                "✖".red()
            );
            return Ok(());
        }
    };

    let mut missing_tools = Vec::new();
    let mut recommendations = Vec::new();

    // 1. Workspace detection
    let is_workspace = check_workspace(&root)?;
    if is_workspace {
        println!("{} Workspace detected", "✔".green());
    } else {
        println!("{} Single package project detected", "✔".green());
    }

    // 2. Incremental compilation detection
    let (incremental_enabled, codegen_units) = check_profiles(&root)?;
    if incremental_enabled {
        println!("{} Incremental compilation enabled", "✔".green());
    } else {
        println!(
            "{} Incremental compilation not explicitly configured (defaults to true in dev)",
            "⚠".yellow()
        );
        recommendations.push("Add `incremental = true` to [profile.dev] in Cargo.toml".to_string());
    }

    if let Some(cu) = codegen_units {
        if cu < 100 {
            println!(
                "{} codegen-units = {} (low codegen-units increases compile times in dev)",
                "⚠".yellow(),
                cu
            );
            recommendations.push(format!("Increase `codegen-units` to 256 in [profile.dev] to speed up dev builds (currently {})", cu));
        } else {
            println!(
                "{} codegen-units is optimized (currently {})",
                "✔".green(),
                cu
            );
        }
    } else {
        println!(
            "{} codegen-units not explicitly configured (defaults to 256 in dev)",
            "✔".green()
        );
    }

    // 3. Check sccache
    let sccache_installed = is_tool_installed("sccache");
    let sccache_configured = check_sccache_config(&root)?;
    if sccache_installed {
        if sccache_configured {
            println!("{} sccache installed and configured", "✔".green());
        } else {
            println!(
                "{} sccache installed but not configured in .cargo/config.toml",
                "⚠".yellow()
            );
            recommendations
                .push("Run `cargo accelerate cache enable` to configure sccache".to_string());
        }
    } else {
        println!("{} sccache missing", "✖".red());
        missing_tools.push("sccache".to_string());
    }

    // 4. Check Linker
    let os = get_os();
    let preferred_linker = match os {
        "linux" => "mold",
        "macos" => "lld",
        "windows" => "lld-link",
        _ => "lld",
    };

    let linker_installed = is_tool_installed(preferred_linker);
    let linker_configured = check_linker_config(&root, os)?;

    if linker_installed {
        if linker_configured {
            println!(
                "{} Fast linker ({}) configured",
                "✔".green(),
                preferred_linker
            );
        } else {
            println!(
                "{} Fast linker ({}) installed but not configured",
                "⚠".yellow(),
                preferred_linker
            );
            recommendations
                .push("Run `cargo accelerate linker` to configure the fast linker".to_string());
        }
    } else {
        println!("{} {} missing", "✖".red(), preferred_linker);
        missing_tools.push(preferred_linker.to_string());
    }

    // 5. Check cargo-nextest
    if is_tool_installed("cargo-nextest") {
        println!("{} cargo-nextest installed", "✔".green());
    } else {
        println!("{} cargo-nextest missing", "⚠".yellow());
        missing_tools.push("cargo-nextest".to_string());
    }

    // 6. Check policy file
    let policy_paths = vec![
        root.join(".cargo-accelerate").join("policy.toml"),
        root.join("accelerate-policy.toml"),
    ];
    if policy_paths.iter().any(|p| p.exists()) {
        println!("{} Build policy file found", "✔".green());
    } else {
        println!(
            "{} No build policy file found (run `cargo accelerate policy` to create one)",
            "⚠".yellow()
        );
    }

    // 7. Check baseline
    let baseline_path = root.join(".cargo-accelerate").join("baseline.json");
    if baseline_path.exists() {
        println!(
            "{} Build baseline found (run `cargo accelerate regression --compare` to check)",
            "✔".green()
        );
    } else {
        println!(
            "{} No build baseline (run `cargo accelerate regression --save` to create one)",
            "⚠".yellow()
        );
    }

    // Print Recommendations
    if !missing_tools.is_empty() || !recommendations.is_empty() {
        println!("\n{}", "Recommendation:".bold().underline().yellow());

        if !missing_tools.is_empty() {
            println!("\nInstall missing tools:");
            for tool in &missing_tools {
                match os {
                    "macos" => println!("  brew install {}", tool),
                    "linux" => {
                        if tool == "mold" {
                            println!("  sudo apt install mold clang");
                        } else {
                            println!("  cargo install {}", tool);
                        }
                    }
                    _ => println!("  cargo install {}", tool),
                }
            }
            println!("  (or run `cargo accelerate install` to auto-install missing ones)");
        }

        if !recommendations.is_empty() {
            println!("\nSuggested improvements:");
            for rec in &recommendations {
                println!("  - {}", rec);
            }
            println!(
                "  (or run `cargo accelerate optimize` to automatically apply recommendations)"
            );
        }
    } else {
        println!(
            "\n{}",
            "✔ Your environment is fully optimized!".bold().green()
        );
    }

    Ok(())
}

fn check_workspace(root: &Path) -> Result<bool> {
    let cargo_toml_path = get_cargo_toml_path(root);
    if !cargo_toml_path.exists() {
        return Ok(false);
    }
    let content = fs::read_to_string(cargo_toml_path)?;
    let parsed: toml::Value = toml::from_str(&content)?;

    if let Some(workspace) = parsed.get("workspace") {
        if workspace.get("members").is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn check_profiles(root: &Path) -> Result<(bool, Option<i64>)> {
    let cargo_toml_path = get_cargo_toml_path(root);
    if !cargo_toml_path.exists() {
        return Ok((false, None));
    }
    let content = fs::read_to_string(cargo_toml_path)?;
    let parsed: toml::Value = toml::from_str(&content)?;

    let mut incremental = true;
    let mut codegen_units = None;

    if let Some(profile) = parsed.get("profile") {
        if let Some(dev) = profile.get("dev") {
            if let Some(inc) = dev.get("incremental") {
                if let Some(b) = inc.as_bool() {
                    incremental = b;
                }
            }
            if let Some(cu) = dev.get("codegen-units") {
                if let Some(i) = cu.as_integer() {
                    codegen_units = Some(i);
                }
            }
        }
    }

    Ok((incremental, codegen_units))
}

fn check_sccache_config(root: &Path) -> Result<bool> {
    let config_path = get_cargo_config_path(root);
    if !config_path.exists() {
        return Ok(false);
    }
    let content = fs::read_to_string(config_path)?;
    let parsed: toml::Value = toml::from_str(&content)?;

    if let Some(build) = parsed.get("build") {
        if let Some(wrapper) = build.get("rustc-wrapper") {
            if let Some(s) = wrapper.as_str() {
                if s.contains("sccache") {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

fn check_linker_config(root: &Path, os: &str) -> Result<bool> {
    let config_path = get_cargo_config_path(root);
    if !config_path.exists() {
        return Ok(false);
    }
    let content = fs::read_to_string(config_path)?;
    let parsed: toml::Value = toml::from_str(&content)?;

    if let Some(target) = parsed.get("target") {
        if let Some(tbl) = target.as_table() {
            for (_key, val) in tbl {
                if let Some(linker) = val.get("linker") {
                    if let Some(l_str) = linker.as_str() {
                        if os == "linux" && (l_str.contains("clang") || l_str.contains("mold")) {
                            return Ok(true);
                        }
                    }
                }
                if let Some(rustflags) = val.get("rustflags") {
                    if let Some(arr) = rustflags.as_array() {
                        let flags: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
                        for flag in flags {
                            if flag.contains("fuse-ld=mold") || flag.contains("fuse-ld=lld") {
                                return Ok(true);
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(false)
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
    fn test_check_workspace_with_members() {
        let dir = TempDir::new().unwrap();
        create_cargo_toml(
            &dir,
            r#"
            [workspace]
            members = ["crate-a", "crate-b"]
        "#,
        );
        assert!(check_workspace(dir.path()).unwrap());
    }

    #[test]
    fn test_check_workspace_single_package() {
        let dir = TempDir::new().unwrap();
        create_cargo_toml(
            &dir,
            r#"
            [package]
            name = "my-crate"
        "#,
        );
        assert!(!check_workspace(dir.path()).unwrap());
    }

    #[test]
    fn test_check_workspace_no_cargo_toml() {
        let dir = TempDir::new().unwrap();
        assert!(!check_workspace(dir.path()).unwrap());
    }

    #[test]
    fn test_check_profiles_defaults() {
        let dir = TempDir::new().unwrap();
        create_cargo_toml(
            &dir,
            r#"
            [package]
            name = "my-crate"
        "#,
        );
        let (incremental, codegen_units) = check_profiles(dir.path()).unwrap();
        assert!(incremental);
        assert_eq!(codegen_units, None);
    }

    #[test]
    fn test_check_profiles_explicit() {
        let dir = TempDir::new().unwrap();
        create_cargo_toml(
            &dir,
            r#"
            [package]
            name = "my-crate"
            [profile.dev]
            incremental = false
            codegen-units = 1
        "#,
        );
        let (incremental, codegen_units) = check_profiles(dir.path()).unwrap();
        assert!(!incremental);
        assert_eq!(codegen_units, Some(1));
    }

    #[test]
    fn test_check_profiles_partial() {
        let dir = TempDir::new().unwrap();
        create_cargo_toml(
            &dir,
            r#"
            [package]
            name = "my-crate"
            [profile.dev]
            codegen-units = 128
        "#,
        );
        let (incremental, codegen_units) = check_profiles(dir.path()).unwrap();
        assert!(incremental);
        assert_eq!(codegen_units, Some(128));
    }

    #[test]
    fn test_check_sccache_config_enabled() {
        let dir = TempDir::new().unwrap();
        create_cargo_config(
            &dir,
            r#"
            [build]
            rustc-wrapper = "sccache"
        "#,
        );
        assert!(check_sccache_config(dir.path()).unwrap());
    }

    #[test]
    fn test_check_sccache_config_disabled() {
        let dir = TempDir::new().unwrap();
        create_cargo_config(
            &dir,
            r#"
            [build]
            rustc-wrapper = "/usr/bin/rustc"
        "#,
        );
        assert!(!check_sccache_config(dir.path()).unwrap());
    }

    #[test]
    fn test_check_sccache_config_missing() {
        let dir = TempDir::new().unwrap();
        assert!(!check_sccache_config(dir.path()).unwrap());
    }

    #[test]
    fn test_check_linker_config_mold_linux() {
        let dir = TempDir::new().unwrap();
        create_cargo_config(
            &dir,
            r#"
            [target.x86_64-unknown-linux-gnu]
            linker = "clang"
            rustflags = ["-C", "link-arg=-fuse-ld=mold"]
        "#,
        );
        assert!(check_linker_config(dir.path(), "linux").unwrap());
    }

    #[test]
    fn test_check_linker_config_lld_macos() {
        let dir = TempDir::new().unwrap();
        create_cargo_config(
            &dir,
            r#"
            [target.x86_64-apple-darwin]
            rustflags = ["-C", "link-arg=-fuse-ld=lld"]
        "#,
        );
        assert!(check_linker_config(dir.path(), "macos").unwrap());
    }

    #[test]
    fn test_check_linker_config_not_configured() {
        let dir = TempDir::new().unwrap();
        create_cargo_config(
            &dir,
            r#"
            [build]
            rustc-wrapper = "sccache"
        "#,
        );
        assert!(!check_linker_config(dir.path(), "linux").unwrap());
    }

    #[test]
    fn test_check_linker_config_no_config_file() {
        let dir = TempDir::new().unwrap();
        assert!(!check_linker_config(dir.path(), "linux").unwrap());
    }
}
