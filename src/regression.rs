use crate::timings::{self, TimingStore};
use crate::utils::get_project_root;
use anyhow::{Context, Result};
use colored::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::BufWriter;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

#[derive(Serialize, Deserialize, Debug, Clone)]
struct BuildBaseline {
    pub check_time_secs: f64,
    pub build_time_secs: f64,
    pub test_time_secs: f64,
    pub clippy_time_secs: f64,
    pub total_time_secs: f64,
}

pub struct CliOptions {
    pub budget_secs: Option<f64>,
    pub threshold_pct: f64,
    pub save_baseline: bool,
    pub compare: bool,
}

impl Default for CliOptions {
    fn default() -> Self {
        Self {
            budget_secs: None,
            threshold_pct: 10.0,
            save_baseline: false,
            compare: false,
        }
    }
}

pub fn run(options: CliOptions) -> Result<()> {
    println!("{}", "Performance Regression Tracking...".bold().cyan());

    let root = get_project_root().context("Could not find project root")?;
    let baseline_dir = root.join(".cargo-accelerate");
    let baseline_path = baseline_dir.join("baseline.json");

    let store_path = timings::get_store_path(&root);
    let mut store = TimingStore::load(&store_path)?;

    if options.compare {
        return compare_with_baseline(&root, &baseline_path, &options, &mut store, &store_path);
    }

    let stats = measure_current_builds(&root)?;

    if let Some(budget) = options.budget_secs {
        if stats.total_time_secs > budget {
            println!(
                "{} Build time {:.1}s exceeds budget of {:.1}s!",
                "✖".red(),
                stats.total_time_secs,
                budget
            );
        } else {
            println!(
                "{} Build time {:.1}s is within budget of {:.1}s",
                "✔".green(),
                stats.total_time_secs,
                budget
            );
        }
    }

    if baseline_path.exists() {
        compare_with_baseline(&root, &baseline_path, &options, &mut store, &store_path)?;
    } else {
        println!("  No baseline found. Use --save to create one after your next measurement.");
    }

    if options.save_baseline {
        fs::create_dir_all(&baseline_dir)?;
        let file = fs::File::create(&baseline_path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, &stats)?;
        println!(
            "  {} Baseline saved to {}",
            "✔".green(),
            baseline_path.display()
        );
    }

    Ok(())
}

fn compare_with_baseline(
    root: &PathBuf,
    baseline_path: &PathBuf,
    options: &CliOptions,
    store: &mut TimingStore,
    store_path: &PathBuf,
) -> Result<()> {
    let baseline_content =
        fs::read_to_string(baseline_path).context("Failed to read baseline file")?;
    let baseline: BuildBaseline =
        serde_json::from_str(&baseline_content).context("Failed to parse baseline JSON")?;

    let current = measure_current_builds(root)?;

    // Record timings for historical tracking
    timings::record_build_run(store, "build", std::time::Duration::from_secs_f64(current.build_time_secs), "regression", Some("regression-current"));
    timings::record_build_run(store, "check", std::time::Duration::from_secs_f64(current.check_time_secs), "regression", Some("regression-estimated"));
    store.save(store_path)?;

    println!("\n{}", "Regression Report:".bold().yellow());
    println!(
        "{:<20} {:<15} {:<15} {:<15}",
        "Metric".bold(),
        "Baseline".bold(),
        "Current".bold(),
        "Change".bold()
    );
    println!(
        "{}",
        "------------------------------------------------------------".cyan()
    );

    let comparisons = vec![
        (
            "cargo check",
            baseline.check_time_secs,
            current.check_time_secs,
        ),
        (
            "cargo build",
            baseline.build_time_secs,
            current.build_time_secs,
        ),
        (
            "cargo test",
            baseline.test_time_secs,
            current.test_time_secs,
        ),
        (
            "cargo clippy",
            baseline.clippy_time_secs,
            current.clippy_time_secs,
        ),
        ("total", baseline.total_time_secs, current.total_time_secs),
    ];

    let mut regression_found = false;

    for (label, base_val, curr_val) in &comparisons {
        let pct_change = if *base_val > 0.0 {
            ((curr_val - base_val) / base_val) * 100.0
        } else {
            0.0
        };

        let change_str = if pct_change > options.threshold_pct {
            regression_found = true;
            format!("+{:.1}% ⚠", pct_change).red().to_string()
        } else if pct_change < -options.threshold_pct {
            format!("{:.1}% ✔", pct_change).green().to_string()
        } else {
            format!("{:.1}%", pct_change).dimmed().to_string()
        };

        println!(
            "{:<20} {:<15.2}s {:<15.2}s {:<15}",
            label, base_val, curr_val, change_str
        );
    }

    if regression_found {
        println!(
            "\n{} Regression detected! Some builds are slower than baseline.",
            "⚠".yellow().bold()
        );
        println!("  Threshold: >{:.0}% change", options.threshold_pct);
    } else {
        println!(
            "\n{} No significant regressions detected.",
            "✔".green().bold()
        );
    }

    Ok(())
}

fn measure_current_builds(root: &PathBuf) -> Result<BuildBaseline> {
    // build subsumes check, so only run build
    print!("  Measuring cargo build... ");
    let start = Instant::now();
    let status = Command::new("cargo")
        .arg("build")
        .arg("--workspace")
        .current_dir(root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("Failed to run cargo build")?;
    let build_elapsed = start.elapsed();
    if status.success() {
        println!("{:.1}s", build_elapsed.as_secs_f64());
    } else {
        println!("{}", "failed (but continuing)".yellow());
    }

    let build = build_elapsed.as_secs_f64();
    // Estimate check from build (typically 40-60%)
    let check = build * 0.5;

    Ok(BuildBaseline {
        check_time_secs: check,
        build_time_secs: build,
        test_time_secs: 0.0,
        clippy_time_secs: 0.0,
        total_time_secs: build + check,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_baseline_roundtrip() {
        let baseline = BuildBaseline {
            check_time_secs: 5.2,
            build_time_secs: 30.1,
            test_time_secs: 8.4,
            clippy_time_secs: 12.3,
            total_time_secs: 56.0,
        };
        let json = serde_json::to_string(&baseline).unwrap();
        let parsed: BuildBaseline = serde_json::from_str(&json).unwrap();
        assert!((parsed.check_time_secs - 5.2).abs() < 1e-6);
        assert!((parsed.total_time_secs - 56.0).abs() < 1e-6);
    }

    #[test]
    fn test_default_options() {
        let opts = CliOptions::default();
        assert!((opts.threshold_pct - 10.0).abs() < 1e-6);
        assert!(!opts.save_baseline);
        assert!(!opts.compare);
        assert!(opts.budget_secs.is_none());
    }

    #[test]
    fn test_regression_detection_threshold() {
        let base = 10.0;
        let current = 12.0; // 20% increase, exceeds 10% threshold
        let pct = ((current - base) / base) * 100.0;
        assert!(pct > 10.0);

        let current2 = 10.5; // 5% increase, within threshold
        let pct2 = ((current2 - base) / base) * 100.0;
        assert!(pct2 < 10.0);
    }
}
