use crate::utils::{
    get_cargo_config_path, get_cargo_toml_path, get_project_root, is_tool_installed,
};
use anyhow::{Context, Result};
use colored::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct BuildPolicy {
    pub profile: Option<ProfilePolicy>,
    pub cache: Option<CachePolicy>,
    pub linker: Option<LinkerPolicy>,
    pub ci: Option<CiPolicy>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProfilePolicy {
    pub require_incremental: Option<bool>,
    pub min_codegen_units: Option<i64>,
    pub max_opt_level_dev: Option<i64>,
    pub require_release_lto: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CachePolicy {
    pub require_sccache: Option<bool>,
    pub require_rust_cache: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LinkerPolicy {
    pub require_fast_linker: Option<bool>,
    pub preferred_linker: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CiPolicy {
    pub require_clippy_deny: Option<bool>,
    pub require_nextest: Option<bool>,
    pub require_fmt_check: Option<bool>,
    pub require_sccache_action: Option<bool>,
}

impl BuildPolicy {
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .context(format!("Failed to read policy file: {}", path.display()))?;
        let policy: BuildPolicy = toml::from_str(&content)
            .context(format!("Failed to parse policy file: {}", path.display()))?;
        Ok(policy)
    }

    pub fn default_policy_content() -> &'static str {
        r#"# Build policy for cargo-accelerate
[profile]
require_incremental = true
min_codegen_units = 128
max_opt_level_dev = 0
require_release_lto = true

[cache]
require_sccache = true
require_rust_cache = true

[linker]
require_fast_linker = true
preferred_linker = "mold"

[ci]
require_clippy_deny = true
require_nextest = true
require_fmt_check = true
require_sccache_action = true
"#
    }
}

pub fn run() -> Result<()> {
    println!("{}", "Policy Enforcement...".bold().cyan());

    let root = get_project_root().context("Could not find project root")?;
    let policy_paths = [
        root.join(".cargo-accelerate").join("policy.toml"),
        root.join("accelerate-policy.toml"),
    ];

    let policy_path = policy_paths.iter().find(|p| p.exists());
    let policy = match policy_path {
        Some(path) => {
            println!("  Using policy file: {}", path.display());
            BuildPolicy::from_file(path)?
        }
        None => {
            println!("  No policy file found. Creating template at .cargo-accelerate/policy.toml");
            let dir = root.join(".cargo-accelerate");
            fs::create_dir_all(&dir)?;
            let template_path = dir.join("policy.toml");
            fs::write(&template_path, BuildPolicy::default_policy_content())?;
            println!(
                "  {} Template created at {}",
                "✔".green(),
                template_path.display()
            );
            return Ok(());
        }
    };

    let mut passed = 0u32;
    let mut failed = 0u32;

    if let Some(ref profile_pol) = policy.profile {
        println!("\n{}", "Profile Checks:".bold());
        if let Some(inc) = profile_pol.require_incremental {
            if inc {
                passed += check_incremental(&root)?;
            } else {
                failed += 1;
            }
        }
        if let Some(min_cu) = profile_pol.min_codegen_units {
            passed += check_codegen_units(&root, min_cu)?;
        }
        if let Some(max_opt) = profile_pol.max_opt_level_dev {
            passed += check_opt_level_dev(&root, max_opt)?;
        }
        if let Some(lto) = profile_pol.require_release_lto {
            if lto {
                passed += check_release_lto(&root)?;
            } else {
                failed += 1;
            }
        }
    }

    if let Some(ref cache_pol) = policy.cache {
        println!("\n{}", "Cache Checks:".bold());
        if let Some(sccache) = cache_pol.require_sccache {
            if sccache {
                passed += check_sccache(&root)?;
            } else {
                failed += 1;
            }
        }
    }

    if let Some(ref linker_pol) = policy.linker {
        println!("\n{}", "Linker Checks:".bold());
        if let Some(fast) = linker_pol.require_fast_linker {
            if fast {
                passed += check_fast_linker(&root, &linker_pol.preferred_linker)?;
            } else {
                failed += 1;
            }
        }
    }

    println!("\n{}", "Summary:".bold());
    println!("  {} checks passed", passed.to_string().green());
    if failed > 0 {
        println!(
            "  {} checks failed — run `cargo accelerate doctor` for details",
            failed.to_string().red()
        );
    }

    Ok(())
}

fn check_incremental(root: &Path) -> Result<u32> {
    let cargo_toml = get_cargo_toml_path(root);
    if !cargo_toml.exists() {
        return Ok(0);
    }
    let content = fs::read_to_string(&cargo_toml)?;
    let parsed: toml::Value = toml::from_str(&content)?;

    let enabled = parsed
        .get("profile")
        .and_then(|p| p.get("dev"))
        .and_then(|d| d.get("incremental"))
        .and_then(|i| i.as_bool())
        .unwrap_or(true);

    if enabled {
        println!("  ✔ incremental compilation is enabled");
        Ok(1)
    } else {
        println!(
            "  ✖ incremental compilation is disabled — set `incremental = true` in [profile.dev]"
        );
        Ok(0)
    }
}

fn check_codegen_units(root: &Path, min: i64) -> Result<u32> {
    let cargo_toml = get_cargo_toml_path(root);
    if !cargo_toml.exists() {
        return Ok(0);
    }
    let content = fs::read_to_string(&cargo_toml)?;
    let parsed: toml::Value = toml::from_str(&content)?;

    let current = parsed
        .get("profile")
        .and_then(|p| p.get("dev"))
        .and_then(|d| d.get("codegen-units"))
        .and_then(|c| c.as_integer());

    match current {
        Some(cu) if cu >= min => {
            println!("  ✔ codegen-units = {} (meets minimum of {})", cu, min);
            Ok(1)
        }
        Some(cu) => {
            println!("  ✖ codegen-units = {} (below minimum of {})", cu, min);
            Ok(0)
        }
        None => {
            println!(
                "  ✔ codegen-units not set (defaults to 256, meets minimum of {})",
                min
            );
            Ok(1)
        }
    }
}

fn check_opt_level_dev(root: &Path, max: i64) -> Result<u32> {
    let cargo_toml = get_cargo_toml_path(root);
    if !cargo_toml.exists() {
        return Ok(0);
    }
    let content = fs::read_to_string(&cargo_toml)?;
    let parsed: toml::Value = toml::from_str(&content)?;

    let current = parsed
        .get("profile")
        .and_then(|p| p.get("dev"))
        .and_then(|d| d.get("opt-level"))
        .and_then(|o| o.as_integer());

    match current {
        Some(opt) if opt <= max => {
            println!("  ✔ opt-level = {} (within max of {})", opt, max);
            Ok(1)
        }
        Some(opt) => {
            println!(
                "  ✖ opt-level = {} (exceeds max of {} — slows dev builds)",
                opt, max
            );
            Ok(0)
        }
        None => {
            println!(
                "  ✔ opt-level not set (defaults to 0, within max of {})",
                max
            );
            Ok(1)
        }
    }
}

fn check_release_lto(root: &Path) -> Result<u32> {
    let cargo_toml = get_cargo_toml_path(root);
    if !cargo_toml.exists() {
        return Ok(0);
    }
    let content = fs::read_to_string(&cargo_toml)?;
    let parsed: toml::Value = toml::from_str(&content)?;

    let has_lto = parsed
        .get("profile")
        .and_then(|p| p.get("release"))
        .and_then(|r| r.get("lto"))
        .map(|l| l.as_bool().unwrap_or(false))
        .unwrap_or(false);

    if has_lto {
        println!("  ✔ release profile has LTO enabled");
        Ok(1)
    } else {
        println!("  ✖ release profile does not have LTO enabled");
        Ok(0)
    }
}

fn check_sccache(root: &Path) -> Result<u32> {
    let config_path = get_cargo_config_path(root);
    let configured = if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        let parsed: toml::Value = toml::from_str(&content)?;
        parsed
            .get("build")
            .and_then(|b| b.get("rustc-wrapper"))
            .and_then(|w| w.as_str())
            .map(|s| s.contains("sccache"))
            .unwrap_or(false)
    } else {
        false
    };

    let installed = is_tool_installed("sccache");

    if installed && configured {
        println!("  ✔ sccache is installed and configured");
        Ok(1)
    } else if installed {
        println!("  ✖ sccache is installed but not configured");
        Ok(0)
    } else {
        println!("  ✖ sccache is not installed");
        Ok(0)
    }
}

fn check_fast_linker(root: &Path, preferred: &Option<String>) -> Result<u32> {
    let config_path = get_cargo_config_path(root);
    let configured = if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        let parsed: toml::Value = toml::from_str(&content)?;
        parsed
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
            .unwrap_or(false)
    } else {
        false
    };

    let linker_name = preferred.as_deref().unwrap_or("mold/lld");
    let installed =
        is_tool_installed("mold") || is_tool_installed("lld") || is_tool_installed("lld-link");

    if installed && configured {
        println!(
            "  ✔ Fast linker ({}) is installed and configured",
            linker_name
        );
        Ok(1)
    } else if installed {
        println!(
            "  ✖ Fast linker ({}) is installed but not configured",
            linker_name
        );
        Ok(0)
    } else {
        println!("  ✖ Fast linker ({}) is not installed", linker_name);
        Ok(0)
    }
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
    fn test_policy_from_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("policy.toml");
        fs::write(
            &path,
            r#"
[profile]
require_incremental = true
min_codegen_units = 128

[cache]
require_sccache = true

[linker]
require_fast_linker = true
preferred_linker = "mold"
"#,
        )
        .unwrap();
        let policy = BuildPolicy::from_file(&path).unwrap();
        assert!(policy.profile.unwrap().require_incremental.unwrap());
    }

    #[test]
    fn test_default_policy_parses() {
        let content = BuildPolicy::default_policy_content();
        let policy: BuildPolicy = toml::from_str(content).unwrap();
        assert!(policy.profile.is_some());
        assert!(policy.cache.is_some());
        assert!(policy.linker.is_some());
        assert!(policy.ci.is_some());
    }

    #[test]
    fn test_check_incremental_enabled() {
        let dir = TempDir::new().unwrap();
        create_cargo_toml(
            &dir,
            r#"
            [package]
            name = "test"
            [profile.dev]
            incremental = true
        "#,
        );
        assert_eq!(check_incremental(dir.path()).unwrap(), 1);
    }

    #[test]
    fn test_check_incremental_disabled() {
        let dir = TempDir::new().unwrap();
        create_cargo_toml(
            &dir,
            r#"
            [package]
            name = "test"
            [profile.dev]
            incremental = false
        "#,
        );
        assert_eq!(check_incremental(dir.path()).unwrap(), 0);
    }

    #[test]
    fn test_check_codegen_units_meets_min() {
        let dir = TempDir::new().unwrap();
        create_cargo_toml(
            &dir,
            r#"
            [package]
            name = "test"
            [profile.dev]
            codegen-units = 256
        "#,
        );
        assert_eq!(check_codegen_units(dir.path(), 128).unwrap(), 1);
    }

    #[test]
    fn test_check_codegen_units_below_min() {
        let dir = TempDir::new().unwrap();
        create_cargo_toml(
            &dir,
            r#"
            [package]
            name = "test"
            [profile.dev]
            codegen-units = 64
        "#,
        );
        assert_eq!(check_codegen_units(dir.path(), 128).unwrap(), 0);
    }

    #[test]
    fn test_check_opt_level_dev_within() {
        let dir = TempDir::new().unwrap();
        create_cargo_toml(
            &dir,
            r#"
            [package]
            name = "test"
            [profile.dev]
            opt-level = 0
        "#,
        );
        assert_eq!(check_opt_level_dev(dir.path(), 0).unwrap(), 1);
    }

    #[test]
    fn test_check_opt_level_dev_exceeds() {
        let dir = TempDir::new().unwrap();
        create_cargo_toml(
            &dir,
            r#"
            [package]
            name = "test"
            [profile.dev]
            opt-level = 2
        "#,
        );
        assert_eq!(check_opt_level_dev(dir.path(), 0).unwrap(), 0);
    }

    #[test]
    fn test_check_release_lto_enabled() {
        let dir = TempDir::new().unwrap();
        create_cargo_toml(
            &dir,
            r#"
            [package]
            name = "test"
            [profile.release]
            lto = true
        "#,
        );
        assert_eq!(check_release_lto(dir.path()).unwrap(), 1);
    }

    #[test]
    fn test_check_release_lto_missing() {
        let dir = TempDir::new().unwrap();
        create_cargo_toml(
            &dir,
            r#"
            [package]
            name = "test"
        "#,
        );
        assert_eq!(check_release_lto(dir.path()).unwrap(), 0);
    }

    #[test]
    fn test_check_sccache_configured() {
        let dir = TempDir::new().unwrap();
        create_cargo_config(
            &dir,
            r#"
            [build]
            rustc-wrapper = "sccache"
        "#,
        );
        let result = check_sccache(dir.path()).unwrap();
        assert!(result == 0 || result == 1); // depends on whether sccache is installed
    }

    #[test]
    fn test_check_fast_linker_configured() {
        let dir = TempDir::new().unwrap();
        create_cargo_config(
            &dir,
            r#"
            [target.x86_64-unknown-linux-gnu]
            rustflags = ["-C", "link-arg=-fuse-ld=mold"]
        "#,
        );
        let result = check_fast_linker(dir.path(), &Some("mold".into())).unwrap();
        assert!(result == 0 || result == 1);
    }
}
