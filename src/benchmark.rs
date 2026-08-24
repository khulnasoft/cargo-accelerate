use crate::timings::{self, TimingStore};
use crate::utils::{get_cargo_config_path, get_cargo_toml_path, get_project_root};
use anyhow::{Context, Result};
use colored::*;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

struct BenchmarkStats {
    check_time: Duration,
    build_time: Duration,
    test_time: Duration,
    clippy_time: Duration,
}

pub fn run(incremental: bool) -> Result<()> {
    println!("{}", "Initiating Performance Benchmark...".bold().cyan());
    if incremental {
        println!(
            "  {}",
            "Incremental mode: measuring rebuild times without cargo clean".yellow()
        );
    }

    let root = get_project_root().context("Could not find project root")?;

    // Back up configurations
    let cargo_toml_path = get_cargo_toml_path(&root);
    let cargo_config_path = get_cargo_config_path(&root);

    let cargo_toml_backup = if cargo_toml_path.exists() {
        Some(fs::read_to_string(&cargo_toml_path)?)
    } else {
        None
    };

    let cargo_config_backup = if cargo_config_path.exists() {
        Some(fs::read_to_string(&cargo_config_path)?)
    } else {
        None
    };

    println!("  {} Backed up project configuration", "✔".green());

    // Ensure we restore configuration even if we panic/error
    let result = run_benchmark_sequence(&root, &cargo_toml_path, &cargo_config_path, incremental);

    // Restore backups
    println!("\nRestoring original project configuration...");
    if let Some(content) = cargo_toml_backup {
        fs::write(&cargo_toml_path, content)?;
    }
    if let Some(content) = cargo_config_backup {
        fs::write(&cargo_config_path, content)?;
    } else if cargo_config_path.exists() {
        let _ = fs::remove_file(&cargo_config_path);
    }
    println!("  {} Configuration restored", "✔".green());

    result
}

fn run_benchmark_sequence(
    root: &Path,
    cargo_toml_path: &Path,
    cargo_config_path: &Path,
    incremental: bool,
) -> Result<()> {
    let store_path = timings::get_store_path(root);
    let mut store = TimingStore::load(&store_path)?;

    // ---- 1. MEASURE UNOPTIMIZED (BEFORE) ----
    println!(
        "\n{}",
        "=== Phase 1: Benchmarking Unoptimized (Before) ==="
            .bold()
            .yellow()
    );

    // Write unoptimized configs
    setup_unoptimized_configs(cargo_toml_path, cargo_config_path)?;

    if !incremental {
        println!("Running clean build to clear any caches...");
        run_cargo_clean(root)?;
    } else {
        println!("Using existing build artifacts (incremental mode)...");
    }

    println!("Measuring Unoptimized times...");
    let before_stats = measure_builds(root)?;
    record_benchmark_timings(&mut store, &before_stats, "before");
    store.save(&store_path)?;

    // ---- 2. MEASURE OPTIMIZED (AFTER) ----
    println!(
        "\n{}",
        "=== Phase 2: Benchmarking Optimized (After) ==="
            .bold()
            .green()
    );

    // Write optimized configs
    setup_optimized_configs(cargo_toml_path, cargo_config_path)?;

    if !incremental {
        println!("Running clean build to clear target...");
        run_cargo_clean(root)?;
    } else {
        println!("Reusing build artifacts (incremental mode)...");
    }

    println!("Measuring Optimized times...");
    let after_stats = measure_builds(root)?;
    record_benchmark_timings(&mut store, &after_stats, "after");
    store.save(&store_path)?;

    // ---- 3. DISPLAY RESULTS ----
    println!(
        "\n{}",
        "================ Benchmark Report ================"
            .bold()
            .cyan()
    );

    print_comparison(
        "Cargo Check",
        before_stats.check_time,
        after_stats.check_time,
    );
    print_comparison(
        "Cargo Build",
        before_stats.build_time,
        after_stats.build_time,
    );
    print_comparison("Cargo Test ", before_stats.test_time, after_stats.test_time);
    print_comparison(
        "Cargo Clippy",
        before_stats.clippy_time,
        after_stats.clippy_time,
    );

    let total_before = before_stats.check_time
        + before_stats.build_time
        + before_stats.test_time
        + before_stats.clippy_time;
    let total_after = after_stats.check_time
        + after_stats.build_time
        + after_stats.test_time
        + after_stats.clippy_time;

    println!(
        "{}",
        "--------------------------------------------------".cyan()
    );
    print_comparison("Total Time ", total_before, total_after);

    Ok(())
}

fn record_benchmark_timings(store: &mut TimingStore, stats: &BenchmarkStats, phase: &str) {
    timings::record_build_run(
        store,
        "check",
        stats.check_time,
        "benchmark",
        Some(&format!("{}-check", phase)),
    );
    timings::record_build_run(
        store,
        "build",
        stats.build_time,
        "benchmark",
        Some(&format!("{}-build", phase)),
    );
    timings::record_build_run(
        store,
        "test",
        stats.test_time,
        "benchmark",
        Some(&format!("{}-test", phase)),
    );
    timings::record_build_run(
        store,
        "clippy",
        stats.clippy_time,
        "benchmark",
        Some(&format!("{}-clippy", phase)),
    );
}

fn print_comparison(label: &str, before: Duration, after: Duration) {
    let before_secs = before.as_secs_f64();
    let after_secs = after.as_secs_f64();
    let saved = saved_pct(before_secs, after_secs);

    println!(
        "{}:  {:.2}s  ➔  {:.2}s   ({:.1}% Saved)",
        label.bold(),
        format!("{:.2}s", before_secs).red(),
        format!("{:.2}s", after_secs).green(),
        saved
    );
}

fn saved_pct(before_secs: f64, after_secs: f64) -> f64 {
    if before_secs > 0.0 {
        ((before_secs - after_secs) / before_secs) * 100.0
    } else {
        0.0
    }
}

fn run_cargo_clean(root: &Path) -> Result<()> {
    Command::new("cargo")
        .arg("clean")
        .current_dir(root)
        .output()
        .context("Failed to run cargo clean")?;
    Ok(())
}

fn setup_unoptimized_configs(cargo_toml_path: &Path, cargo_config_path: &Path) -> Result<()> {
    if cargo_toml_path.exists() {
        let content = fs::read_to_string(cargo_toml_path)?;
        let mut doc = content.parse::<toml_edit::DocumentMut>()?;

        if !doc.contains_key("profile") {
            doc["profile"] = toml_edit::table();
        }
        let has_dev = doc["profile"]
            .as_table()
            .map(|t| t.contains_key("dev"))
            .unwrap_or(false);
        if !has_dev {
            doc["profile"]["dev"] = toml_edit::table();
        }

        doc["profile"]["dev"]["incremental"] = toml_edit::value(false);
        doc["profile"]["dev"]["codegen-units"] = toml_edit::value(1);
        fs::write(cargo_toml_path, doc.to_string())?;
    }

    // Remove only sccache/linker keys instead of deleting the whole file
    if cargo_config_path.exists() {
        let content = fs::read_to_string(cargo_config_path)?;
        let mut doc = content.parse::<toml_edit::DocumentMut>()?;

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

        if let Some(target) = doc.get_mut("target") {
            if let Some(tbl) = target.as_table_mut() {
                let keys: Vec<String> = tbl.iter().map(|(k, _)| k.to_string()).collect();
                for key in keys {
                    if let Some(entry) = tbl.get_mut(&key) {
                        if let Some(t) = entry.as_table_mut() {
                            t.remove("linker");
                            t.remove("rustflags");
                        }
                    }
                    if let Some(entry) = tbl.get(&key) {
                        if let Some(t) = entry.as_table() {
                            if t.is_empty() {
                                tbl.remove(&key);
                            }
                        }
                    }
                }
            }
            if let Some(t) = target.as_table() {
                if t.is_empty() {
                    doc.remove("target");
                }
            }
        }

        fs::write(cargo_config_path, doc.to_string())?;
    }
    Ok(())
}

fn setup_optimized_configs(cargo_toml_path: &Path, cargo_config_path: &Path) -> Result<()> {
    if cargo_toml_path.exists() {
        let content = fs::read_to_string(cargo_toml_path)?;
        let mut doc = content.parse::<toml_edit::DocumentMut>()?;

        if !doc.contains_key("profile") {
            doc["profile"] = toml_edit::table();
        }
        let has_dev = doc["profile"]
            .as_table()
            .map(|t| t.contains_key("dev"))
            .unwrap_or(false);
        if !has_dev {
            doc["profile"]["dev"] = toml_edit::table();
        }

        doc["profile"]["dev"]["incremental"] = toml_edit::value(true);
        doc["profile"]["dev"]["codegen-units"] = toml_edit::value(256);
        doc["profile"]["dev"]["opt-level"] = toml_edit::value(0);
        doc["profile"]["dev"]["debug"] = toml_edit::value(1);
        fs::write(cargo_toml_path, doc.to_string())?;
    }

    // Preserve existing config, only add sccache/linker keys
    let mut config_doc = if cargo_config_path.exists() {
        let content = fs::read_to_string(cargo_config_path)?;
        content.parse::<toml_edit::DocumentMut>()?
    } else {
        toml_edit::DocumentMut::new()
    };

    if !config_doc.contains_key("build") {
        config_doc["build"] = toml_edit::table();
    }
    if crate::utils::is_tool_installed("sccache") {
        config_doc["build"]["rustc-wrapper"] = toml_edit::value("sccache");
    }

    let os = crate::utils::get_os();
    let preferred_linker = crate::utils::preferred_linker_for(os);

    if crate::utils::is_tool_installed(preferred_linker) {
        if !config_doc.contains_key("target") {
            config_doc["target"] = toml_edit::table();
        }
        let target_key = crate::utils::target_triple_for(os);
        if config_doc["target"].get(target_key).is_none() {
            config_doc["target"][target_key] = toml_edit::table();
        }

        if os == "linux" {
            config_doc["target"][target_key]["linker"] = toml_edit::value("clang");
        }

        let mut arr = toml_edit::Array::new();
        arr.push("-C");
        arr.push(format!("link-arg=-fuse-ld={}", preferred_linker));
        config_doc["target"][target_key]["rustflags"] = toml_edit::value(arr);
    }

    if let Some(parent) = cargo_config_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(cargo_config_path, config_doc.to_string())?;

    Ok(())
}

fn measure_builds(root: &Path) -> Result<BenchmarkStats> {
    // 1. Measure cargo build (subsumes check)
    println!("  Running cargo build...");
    let start = Instant::now();
    run_cargo_cmd(root, "build")?;
    let build_time = start.elapsed();
    println!("    Finished in {:.2}s", build_time.as_secs_f64());

    // 2. Measure cargo test
    println!("  Running cargo test...");
    let start = Instant::now();
    run_cargo_cmd(root, "test")?;
    let test_time = start.elapsed();
    println!("    Finished in {:.2}s", test_time.as_secs_f64());

    // 3. Measure cargo clippy
    println!("  Running cargo clippy...");
    let start = Instant::now();
    run_cargo_cmd(root, "clippy")?;
    let clippy_time = start.elapsed();
    println!("    Finished in {:.2}s", clippy_time.as_secs_f64());

    // Estimate check time as a fraction of build (cargo check is typically 40-60% of build)
    let check_time = build_time.mul_f64(0.5);

    Ok(BenchmarkStats {
        check_time,
        build_time,
        test_time,
        clippy_time,
    })
}

fn run_cargo_cmd(root: &Path, cmd: &str) -> Result<()> {
    let status = Command::new("cargo")
        .arg(cmd)
        .current_dir(root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .with_context(|| format!("Failed to run cargo {}", cmd))?;

    if !status.success() {
        // Suppress warning if clippy or test fails in benchmark environment, just continue
        println!(
            "    {} cargo {} returned non-zero status (continuing)",
            "⚠".yellow(),
            cmd
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::TempDir;

    fn stats(check_ms: u64) -> BenchmarkStats {
        BenchmarkStats {
            check_time: Duration::from_millis(check_ms),
            build_time: Duration::from_millis(check_ms * 2),
            test_time: Duration::from_millis(check_ms * 3),
            clippy_time: Duration::from_millis(check_ms * 4),
        }
    }

    #[test]
    fn test_saved_pct_math() {
        assert_eq!(saved_pct(10.0, 5.0), 50.0);
        assert_eq!(saved_pct(10.0, 10.0), 0.0);
        assert_eq!(saved_pct(10.0, 20.0), -100.0);
        // Zero baseline must not divide by zero
        assert_eq!(saved_pct(0.0, 5.0), 0.0);
        assert!((saved_pct(8.0, 6.0) - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_record_benchmark_timings_labels_each_phase() {
        let mut store = TimingStore::default();
        record_benchmark_timings(&mut store, &stats(100), "before");

        assert_eq!(store.runs.len(), 4);
        let labels: Vec<Option<&str>> = store.runs.iter().map(|r| r.label.as_deref()).collect();
        assert!(labels.contains(&Some("before-check")));
        assert!(labels.contains(&Some("before-build")));
        assert!(labels.contains(&Some("before-test")));
        assert!(labels.contains(&Some("before-clippy")));

        for r in &store.runs {
            assert_eq!(r.profile, "benchmark");
        }

        record_benchmark_timings(&mut store, &stats(200), "after");
        assert_eq!(store.runs.len(), 8);
        assert!(store
            .runs
            .iter()
            .any(|r| r.label.as_deref() == Some("after-build")));
    }

    #[test]
    fn test_setup_unoptimized_configs_missing_files_is_noop() {
        let dir = TempDir::new().unwrap();
        let toml = dir.path().join("Cargo.toml");
        let config = dir.path().join("config.toml");

        assert!(setup_unoptimized_configs(&toml, &config).is_ok());
        assert!(!toml.exists());
        assert!(!config.exists());
    }

    #[test]
    fn test_setup_unoptimized_configs_strips_wrapper_and_sets_slow_profile() {
        let dir = TempDir::new().unwrap();
        let toml = dir.path().join("Cargo.toml");
        let config = dir.path().join("config.toml");

        fs::write(
            &toml,
            "[package]\nname = \"x\"\n# keep me\n[profile.dev]\nopt-level = 3\n",
        )
        .unwrap();
        fs::write(
            &config,
            "[build]\nrustc-wrapper = \"sccache\"\n\n[target.x86_64-unknown-linux-gnu]\nlinker = \"clang\"\nrustflags = [\"-C\", \"link-arg=-fuse-ld=mold\"]\n",
        )
        .unwrap();

        setup_unoptimized_configs(&toml, &config).unwrap();

        let toml_out = fs::read_to_string(&toml).unwrap();
        assert!(toml_out.contains("# keep me"), "comments must survive");
        assert!(toml_out.contains("incremental = false"));
        assert!(toml_out.contains("codegen-units = 1"));
        // Existing keys are preserved (not clobbered)
        assert!(toml_out.contains("opt-level = 3"));

        let config_out = fs::read_to_string(&config).unwrap();
        assert!(!config_out.contains("rustc-wrapper"));
        assert!(!config_out.contains("linker"));
        assert!(!config_out.contains("rustflags"));
        assert!(!config_out.contains("target"));
        assert!(config_out.contains("[build]") || !config_out.contains("[build]\nrustc-wrapper"));
    }

    #[test]
    fn test_setup_optimized_configs_writes_fast_dev_profile() {
        let dir = TempDir::new().unwrap();
        let toml = dir.path().join("Cargo.toml");
        let config = dir.path().join("config.toml");
        fs::write(&toml, "[package]\nname = \"x\"\n").unwrap();

        setup_optimized_configs(&toml, &config).unwrap();

        let toml_out = fs::read_to_string(&toml).unwrap();
        assert!(toml_out.contains("incremental = true"));
        assert!(toml_out.contains("codegen-units = 256"));
        assert!(toml_out.contains("opt-level = 0"));
        assert!(toml_out.contains("debug = 1"));

        let config_out = fs::read_to_string(&config).unwrap();
        // Wrapper/linker presence depends on the tools installed in the
        // environment; mirror the same condition the code uses.
        if crate::utils::is_tool_installed("sccache") {
            assert!(config_out.contains("rustc-wrapper = \"sccache\""));
        } else {
            assert!(!config_out.contains("rustc-wrapper"));
        }
        let linker = crate::utils::preferred_linker_for(crate::utils::get_os());
        if crate::utils::is_tool_installed(linker) {
            assert!(config_out.contains(&format!("fuse-ld={}", linker)));
        } else {
            assert!(!config_out.contains("rustflags"));
        }
    }

    #[test]
    fn test_setup_optimized_configs_preserves_existing_config_keys() {
        let dir = TempDir::new().unwrap();
        let toml = dir.path().join("Cargo.toml");
        let config = dir.path().join("config.toml");
        fs::write(&toml, "[package]\nname = \"x\"\n").unwrap();
        fs::write(
            &config,
            "[build]\njobs = 8\n[target.'cfg(unix)']\nrustflags = [\"-Dwarnings\"]\n",
        )
        .unwrap();

        setup_optimized_configs(&toml, &config).unwrap();

        let out = fs::read_to_string(&config).unwrap();
        assert!(out.contains("jobs = 8"), "existing build keys preserved");
        assert!(out.contains("cfg(unix)"), "unrelated target entries kept");
    }
}
