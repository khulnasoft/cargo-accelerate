use crate::cli::CacheAction;
use crate::utils::{get_cargo_config_path, get_project_root, is_tool_installed};
use anyhow::{Context, Result};
use colored::*;
use std::fs;
use toml_edit::DocumentMut;

pub fn run(action: Option<CacheAction>) -> Result<()> {
    let action = action.unwrap_or(CacheAction::Status);

    match action {
        CacheAction::Enable => enable_cache()?,
        CacheAction::Disable => disable_cache()?,
        CacheAction::Status => show_status()?,
        CacheAction::Remote { enable } => configure_remote_cache(enable)?,
    }

    Ok(())
}

fn configure_remote_cache(enable: bool) -> Result<()> {
    let root = get_project_root().context("Could not find project root")?;
    let config_path = get_cargo_config_path(&root);

    if enable {
        println!(
            "{}",
            "Configuring Remote Cache via sccache-dist...".bold().cyan()
        );

        if !is_tool_installed("sccache") {
            println!(
                "  {} sccache must be installed first (run `cargo accelerate install`)",
                "✖".red()
            );
            return Ok(());
        }

        if !is_tool_installed("sccache-dist") {
            println!("  {} sccache-dist not found. Install with: `cargo install sccache --features=dist-client`", "⚠".yellow());
            println!("  Using local sccache as fallback for now.");
        }

        let mut doc = if config_path.exists() {
            let content = fs::read_to_string(&config_path)?;
            content.parse::<DocumentMut>()?
        } else {
            DocumentMut::new()
        };

        if !doc.contains_key("build") {
            doc["build"] = toml_edit::table();
        }
        doc["build"]["rustc-wrapper"] = toml_edit::value("sccache");

        if !doc.contains_key("env") {
            doc["env"] = toml_edit::table();
        }
        if let Some(env) = doc.get_mut("env") {
            if !env
                .as_table()
                .map(|t| t.contains_key("SCCACHE_ENDPOINT"))
                .unwrap_or(false)
            {
                println!(
                    "\n{}",
                    "  To configure remote cache, set these environment variables:".yellow()
                );
                println!("    SCCACHE_ENDPOINT=http://your-sccache-dist-server:10500");
                println!("    SCCACHE_BUCKET=your-cache-bucket");
                println!("    SCCACHE_REGION=auto");
                println!("    SCCACHE_SERVER_PORT=10501");
                println!("\n  Or add to your shell profile:\n");
                println!("    export SCCACHE_ENDPOINT=http://<server>:10500");
                println!("    export SCCACHE_BUCKET=<bucket-name>");
                println!("    export SCCACHE_REGION=auto");
            }
        }

        fs::write(&config_path, doc.to_string())?;
        println!(
            "  {} sccache configured with local cache ready for remote fallback.",
            "✔".green()
        );
    } else {
        println!(
            "{}",
            "Disabling remote cache configuration...".bold().cyan()
        );
        if config_path.exists() {
            let content = fs::read_to_string(&config_path)?;
            let doc = content.parse::<DocumentMut>()?;

            let has_env = doc.contains_key("env");
            let has_build = doc.contains_key("build");

            if has_env || has_build {
                let mut doc = content.parse::<DocumentMut>()?;
                if let Some(env) = doc.get_mut("env") {
                    if let Some(tbl) = env.as_table_mut() {
                        tbl.remove("SCCACHE_ENDPOINT");
                    }
                }
                fs::write(&config_path, doc.to_string())?;
            }
            println!("  {} Remote cache config cleared.", "✔".green());
        }
    }

    Ok(())
}

fn enable_cache() -> Result<()> {
    println!("{}", "Enabling sccache...".bold().cyan());

    if !is_tool_installed("sccache") {
        println!("{} sccache is not installed. Please install it first using `cargo accelerate install` or your package manager.", "✖".red());
        return Ok(());
    }

    let root = get_project_root().context("Could not find project root")?;
    let config_dir = root.join(".cargo");
    if !config_dir.exists() {
        fs::create_dir_all(&config_dir)?;
    }

    let config_path = get_cargo_config_path(&root);
    let mut doc = if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        content.parse::<DocumentMut>()?
    } else {
        DocumentMut::new()
    };

    // Ensure `build` table exists
    if !doc.contains_key("build") {
        doc["build"] = toml_edit::table();
    }

    doc["build"]["rustc-wrapper"] = toml_edit::value("sccache");

    fs::write(&config_path, doc.to_string())?;
    println!(
        "{} sccache has been successfully enabled as rustc-wrapper in `.cargo/config.toml`!",
        "✔".green()
    );

    Ok(())
}

fn disable_cache() -> Result<()> {
    println!("{}", "Disabling sccache...".bold().cyan());

    let root = get_project_root().context("Could not find project root")?;
    let config_path = get_cargo_config_path(&root);

    if !config_path.exists() {
        println!(
            "{} No cargo config found, sccache is not enabled.",
            "✔".green()
        );
        return Ok(());
    }

    let content = fs::read_to_string(&config_path)?;
    let mut doc = content.parse::<DocumentMut>()?;

    if let Some(build) = doc.get_mut("build") {
        if let Some(tbl) = build.as_table_mut() {
            tbl.remove("rustc-wrapper");
        }
    }

    // Clean up empty build table if necessary
    if let Some(build) = doc.get("build") {
        if let Some(tbl) = build.as_table() {
            if tbl.is_empty() {
                doc.remove("build");
            }
        }
    }

    fs::write(&config_path, doc.to_string())?;
    println!(
        "{} sccache wrapper removed from `.cargo/config.toml`.",
        "✔".green()
    );

    Ok(())
}

fn show_status() -> Result<()> {
    println!("{}", "sccache status:".bold().cyan());

    let root = get_project_root().context("Could not find project root")?;
    let config_path = get_cargo_config_path(&root);

    let mut configured = false;
    if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        let doc = content.parse::<DocumentMut>()?;
        if let Some(build) = doc.get("build") {
            if let Some(wrapper) = build.get("rustc-wrapper") {
                if wrapper
                    .as_str()
                    .map(|s| s.contains("sccache"))
                    .unwrap_or(false)
                {
                    configured = true;
                }
            }
        }
    }

    if configured {
        println!("  Configured in .cargo/config.toml: {}", "Yes".green());
    } else {
        println!("  Configured in .cargo/config.toml: {}", "No".red());
    }

    if is_tool_installed("sccache") {
        println!("  sccache executable: {}", "Available".green());
        println!("\nSccache Stats:");

        // Run sccache --show-stats
        let output = std::process::Command::new("sccache")
            .arg("--show-stats")
            .output();

        match output {
            Ok(out) => {
                let stats = String::from_utf8_lossy(&out.stdout);
                for line in stats.lines() {
                    println!("  {}", line.trim());
                }
            }
            Err(_) => {
                println!("  Could not execute sccache --show-stats");
            }
        }
    } else {
        println!("  sccache executable: {}", "Not found".red());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    struct TestEnv {
        #[allow(dead_code)]
        dir: TempDir,
        config_path: std::path::PathBuf,
    }

    impl TestEnv {
        fn new() -> Self {
            let dir = TempDir::new().unwrap();
            let config_dir = dir.path().join(".cargo");
            fs::create_dir_all(&config_dir).unwrap();
            let config_path = config_dir.join("config.toml");
            TestEnv { dir, config_path }
        }

        fn write_config(&self, content: &str) {
            fs::write(&self.config_path, content).unwrap();
        }

        fn read_config(&self) -> String {
            fs::read_to_string(&self.config_path).unwrap()
        }
    }

    #[test]
    fn test_enable_cache_creates_config() {
        let env = TestEnv::new();
        let doc = &mut DocumentMut::new();
        let has_build = doc.contains_key("build");
        if !has_build {
            doc["build"] = toml_edit::table();
        }
        doc["build"]["rustc-wrapper"] = toml_edit::value("sccache");
        fs::write(&env.config_path, doc.to_string()).unwrap();

        let content = env.read_config();
        let parsed = content.parse::<DocumentMut>().unwrap();
        assert_eq!(parsed["build"]["rustc-wrapper"].as_str(), Some("sccache"));
    }

    #[test]
    fn test_disable_cache_removes_wrapper() {
        let env = TestEnv::new();
        env.write_config("[build]\nrustc-wrapper = \"sccache\"\n");

        let content = env.read_config();
        let mut doc = content.parse::<DocumentMut>().unwrap();
        if let Some(build) = doc.get_mut("build") {
            if let Some(tbl) = build.as_table_mut() {
                tbl.remove("rustc-wrapper");
            }
        }
        if let Some(build) = doc.get("build") {
            if let Some(tbl) = build.as_table() {
                if tbl.is_empty() {
                    doc.remove("build");
                }
            }
        }
        fs::write(&env.config_path, doc.to_string()).unwrap();

        let result = env.read_config();
        assert!(
            !result.contains("sccache"),
            "config should not contain sccache: {}",
            result
        );
    }

    #[test]
    fn test_disable_cache_no_config() {
        // Should not error when no config exists
        let doc = DocumentMut::new();
        let result = doc.to_string();
        assert!(result.is_empty());
    }

    #[test]
    fn test_enable_cache_adds_to_existing_config() {
        let env = TestEnv::new();
        env.write_config("[registries]\nmy-registry = { index = \"https://example.com\" }\n");

        let content = env.read_config();
        let mut doc = content.parse::<DocumentMut>().unwrap();
        if !doc.contains_key("build") {
            doc["build"] = toml_edit::table();
        }
        doc["build"]["rustc-wrapper"] = toml_edit::value("sccache");
        fs::write(&env.config_path, doc.to_string()).unwrap();

        let result = env.read_config();
        assert!(result.contains("sccache"));
        assert!(result.contains("registries"));
        assert!(result.contains("example.com"));
    }
}
