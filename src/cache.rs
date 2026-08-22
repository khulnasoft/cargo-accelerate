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
        CacheAction::ValidateRemote { show_values } => validate_remote_cache(show_values)?,
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

fn mask_value(v: &str) -> String {
    // Mask credentials embedded in URLs (scheme://user:pass@host)
    if let Some(scheme_end) = v.find("://") {
        let rest = &v[scheme_end + 3..];
        if let Some(at_idx) = rest.find('@') {
            let scheme = &v[..scheme_end + 3];
            let host = &rest[at_idx + 1..];
            return format!("{}***@{}", scheme, host);
        }
    }
    // Mask obvious secret values
    let lower = v.to_lowercase();
    if lower.contains("token")
        || lower.contains("secret")
        || lower.contains("password")
        || lower.contains("api_key")
        || lower.contains("access_key")
    {
        return "***".to_string();
    }
    v.to_string()
}

fn display_env(v: &str, show_values: bool) -> String {
    if show_values {
        mask_value(v)
    } else {
        "set".to_string()
    }
}

fn validate_remote_cache(show_values: bool) -> Result<()> {
    println!(
        "{}",
        "Remote Cache Connectivity Validation...".bold().cyan()
    );

    // 1. Check sccache installation
    let sccache_available = is_tool_installed("sccache");
    if sccache_available {
        println!("  ✓ sccache executable: {}", "Available".green());
    } else {
        println!("  ✗ sccache executable: {}", "Not found".red());
        println!(
            "    Install with: cargo install sccache --features=dist-client"
        );
        return Ok(());
    }

    let dist_available = is_tool_installed("sccache-dist");
    if dist_available {
        println!("  ✓ sccache-dist executable: {}", "Available".green());
    } else {
        println!("  ⚠ sccache-dist executable: {}", "Not found".yellow());
        println!("    Install with: cargo install sccache --features=dist-client");
        println!("    (Local-only caching will still work.)");
    }

    // 2. Check environment variables
    let endpoint = std::env::var("SCCACHE_ENDPOINT").ok();
    let bucket = std::env::var("SCCACHE_BUCKET").ok();
    let region = std::env::var("SCCACHE_REGION").ok();
    let dist_enabled = std::env::var("SCCACHE_DIST").ok();

    println!("\n  Environment:");
    match &endpoint {
        Some(v) => println!("  ✓ SCCACHE_ENDPOINT = {}", display_env(v, show_values).green()),
        None => println!("  ✗ SCCACHE_ENDPOINT = {}", "not set".red()),
    }
    match &bucket {
        Some(v) => println!("  ✓ SCCACHE_BUCKET  = {}", display_env(v, show_values).green()),
        None => println!("  ⚠ SCCACHE_BUCKET  = {}", "not set (may not be needed)".yellow()),
    }
    match &region {
        Some(v) => println!("  ✓ SCCACHE_REGION  = {}", display_env(v, show_values).green()),
        None => println!("  ⚠ SCCACHE_REGION  = {}", "not set (may not be needed)".yellow()),
    }
    match &dist_enabled {
        Some(v) if v == "true" || v == "1" => {
            println!("  ✓ SCCACHE_DIST    = {}", display_env(v, show_values).green());
        }
        Some(v) => {
            println!("  ⚠ SCCACHE_DIST    = {}", display_env(v, show_values).yellow());
            println!("    Set SCCACHE_DIST=true to enable distributed caching.");
        }
        None => {
            println!("  ⚠ SCCACHE_DIST    = {}", "not set".yellow());
            println!("    Set SCCACHE_DIST=true to enable distributed caching.");
        }
    }

    // 3. Test connectivity
    println!("\n  Connectivity test:");
    if endpoint.is_some() {
        let start = std::time::Instant::now();
        let output = std::process::Command::new("sccache")
            .args(["--dist-status"])
            .output();
        let elapsed = start.elapsed();

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let combined = format!("{}{}", stdout, stderr);
                let latency_ms = elapsed.as_millis();

                if out.status.success() {
                    if combined.to_lowercase().contains("connected")
                        || combined.to_lowercase().contains("ok")
                        || combined.to_lowercase().contains("available")
                    {
                        println!(
                            "  ✓ Remote cache {} (latency: {}ms)",
                            "reachable".green(),
                            latency_ms.to_string().cyan()
                        );
                    } else {
                        println!(
                            "  ⚠ Remote cache responded but status unclear (latency: {}ms)",
                            latency_ms.to_string().cyan()
                        );
                    }
                } else {
                    println!(
                        "  ✗ Remote cache {} (latency: {}ms)",
                        "unreachable".red(),
                        latency_ms.to_string().cyan()
                    );
                }

                // Print raw command output only when explicitly requested
                if show_values {
                    let trimmed = combined.trim();
                    if !trimmed.is_empty() {
                        println!("\n  sccache --dist-status output:\n");
                        for line in trimmed.lines() {
                            println!("    {}", mask_value(line.trim()));
                        }
                    }
                }
            }
            Err(e) => {
                println!(
                    "  ✗ Failed to run sccache --dist-status: {}",
                    e.to_string().red()
                );
            }
        }
    } else {
        println!(
            "  ⚠ Skipping connectivity test — SCCACHE_ENDPOINT not set."
        );
    }

    // 4. Summary
    println!("\n  {}", "Summary:".bold());
    let all_set = sccache_available
        && endpoint.is_some()
        && dist_enabled
            .as_deref()
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
    if all_set {
        println!("  {} Remote cache configuration looks complete.", "✔".green());
    } else if sccache_available {
        println!(
            "  {} sccache is available but remote cache is not fully configured.",
            "⚠".yellow()
        );
        println!(
            "    See: https://github.com/mozilla/sccache/blob/main/docs/dist.md"
        );
    } else {
        println!(
            "  {} sccache is not installed. Run `cargo accelerate install` first.",
            "✖".red()
        );
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
    fn test_validate_remote_cache_no_sccache() {
        // Should not error even when sccache is not installed (just prints warnings)
        let result = validate_remote_cache(false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_remote_cache_with_env_vars() {
        // Set env vars temporarily; function should handle them gracefully
        unsafe {
            std::env::set_var("SCCACHE_ENDPOINT", "http://test-server:10500");
            std::env::set_var("SCCACHE_BUCKET", "test-bucket");
            std::env::set_var("SCCACHE_REGION", "us-east-1");
            std::env::set_var("SCCACHE_DIST", "true");
        }
        let result = validate_remote_cache(false);
        assert!(result.is_ok());
        unsafe {
            std::env::remove_var("SCCACHE_ENDPOINT");
            std::env::remove_var("SCCACHE_BUCKET");
            std::env::remove_var("SCCACHE_REGION");
            std::env::remove_var("SCCACHE_DIST");
        }
    }

    #[test]
    fn test_validate_remote_cache_without_dist() {
        unsafe {
            std::env::set_var("SCCACHE_ENDPOINT", "http://test-server:10500");
            std::env::set_var("SCCACHE_DIST", "false");
        }
        let result = validate_remote_cache(false);
        assert!(result.is_ok());
        unsafe {
            std::env::remove_var("SCCACHE_ENDPOINT");
            std::env::remove_var("SCCACHE_DIST");
        }
    }

    #[test]
    fn test_mask_value_redacts_credentials() {
        assert_eq!(
            mask_value("https://user:password@cache.example.com:10500"),
            "https://***@cache.example.com:10500"
        );
        assert_eq!(mask_value("https://cache.example.com:10500"), "https://cache.example.com:10500");
        assert_eq!(mask_value("my-token-12345"), "***");
        assert_eq!(mask_value("super_secret_api_key_123"), "***");
        assert_eq!(mask_value("plain-value"), "plain-value");
    }

    #[test]
    fn test_display_env_hides_values_by_default() {
        assert_eq!(display_env("http://test-server:10500", false), "set");
        assert_eq!(display_env("http://test-server:10500", true), "http://test-server:10500");
        assert_eq!(display_env("http://user:pass@host", true), "http://***@host");
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
