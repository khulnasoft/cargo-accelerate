use crate::utils::get_project_root;
use anyhow::{Context, Result};
use colored::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::BufWriter;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BuildRecord {
    pub timestamp: u64,
    pub check_time_secs: f64,
    pub build_time_secs: f64,
    pub total_time_secs: f64,
    pub rustc_version: String,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct TrendHistory {
    pub records: Vec<BuildRecord>,
}

impl TrendHistory {
    pub fn load(path: &PathBuf) -> Result<Self> {
        if path.exists() {
            let content = fs::read_to_string(path)?;
            Ok(serde_json::from_str(&content).unwrap_or_default())
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self, path: &PathBuf) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = fs::File::create(path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, self)?;
        Ok(())
    }

    pub fn add_record(&mut self, record: BuildRecord) {
        self.records.push(record);
        if self.records.len() > 30 {
            self.records.remove(0);
        }
    }

    #[allow(dead_code)]
    pub fn latest(&self) -> Option<&BuildRecord> {
        self.records.last()
    }

    pub fn median_time(&self) -> Option<f64> {
        let mut times: Vec<f64> = self.records.iter().map(|r| r.total_time_secs).collect();
        if times.is_empty() {
            return None;
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        Some(times[times.len() / 2])
    }

    pub fn trend_direction(&self) -> Option<String> {
        if self.records.len() < 3 {
            return None;
        }
        let recent = self.records.last()?.total_time_secs;
        let older = self.records[self.records.len() - 3].total_time_secs;
        let change = ((recent - older) / older) * 100.0;
        if change > 5.0 {
            Some("degrading".into())
        } else if change < -5.0 {
            Some("improving".into())
        } else {
            Some("stable".into())
        }
    }
}

pub fn run() -> Result<()> {
    println!("{}", "Performance Trend Tracking...".bold().cyan());

    let root = get_project_root().context("Could not find project root")?;
    let trend_dir = root.join(".cargo-accelerate");
    let trend_path = trend_dir.join("trends.json");

    let mut history = TrendHistory::load(&trend_path)?;

    println!("  Measuring current build times...");
    let check_time = measure_cmd(&root, "check")?;
    let build_time = measure_cmd(&root, "build")?;

    let rustc_version = get_rustc_version();

    let record = BuildRecord {
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        check_time_secs: check_time.as_secs_f64(),
        build_time_secs: build_time.as_secs_f64(),
        total_time_secs: (check_time + build_time).as_secs_f64(),
        rustc_version,
    };

    history.add_record(record);
    history.save(&trend_path)?;

    print_trend_report(&history);

    Ok(())
}

fn measure_cmd(root: &PathBuf, cmd: &str) -> Result<Duration> {
    let start = Instant::now();
    Command::new("cargo")
        .arg(cmd)
        .arg("--workspace")
        .current_dir(root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context(format!("Failed to run cargo {}", cmd))?;
    Ok(start.elapsed())
}

fn get_rustc_version() -> String {
    let output = Command::new("rustc").arg("--version").output().ok();
    match output {
        Some(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        None => "unknown".into(),
    }
}

fn print_trend_report(history: &TrendHistory) {
    if history.records.is_empty() {
        println!("  No trend data yet. Run `cargo accelerate trend` a few times to build history.");
        return;
    }

    println!("\n{}", "Trend Report:".bold());
    println!(
        "{:<8} {:<12} {:<12} {:<12}",
        "Run".bold(),
        "Check (s)".bold(),
        "Build (s)".bold(),
        "Total (s)".bold()
    );
    println!(
        "{}",
        "--------------------------------------------------".cyan()
    );

    let start = history.records.len().saturating_sub(10);
    for i in start..history.records.len() {
        let r = &history.records[i];
        let label = if i == history.records.len() - 1 {
            "latest".yellow().to_string()
        } else {
            format!("#{}", i + 1)
        };
        println!(
            "{:<8} {:<12.1} {:<12.1} {:<12.1}",
            label, r.check_time_secs, r.build_time_secs, r.total_time_secs
        );
    }

    if let Some(median) = history.median_time() {
        println!(
            "{}",
            "--------------------------------------------------".cyan()
        );
        println!("{:<8} {:<12.1}", "Median".bold(), median);
    }

    if let Some(trend) = history.trend_direction() {
        match trend.as_str() {
            "degrading" => println!(
                "\n{} Build times are degrading — investigate recent changes!",
                "⚠".yellow().bold()
            ),
            "improving" => println!(
                "\n{} Build times are improving — keep up the good work!",
                "✔".green().bold()
            ),
            _ => println!("\n{} Build times are stable.", "✓".green().bold()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trend_history_default() {
        let history = TrendHistory::default();
        assert!(history.records.is_empty());
        assert!(history.median_time().is_none());
    }

    #[test]
    fn test_trend_history_add_record() {
        let mut history = TrendHistory::default();
        history.add_record(BuildRecord {
            timestamp: 1000,
            check_time_secs: 5.0,
            build_time_secs: 20.0,
            total_time_secs: 25.0,
            rustc_version: "rustc 1.70".into(),
        });
        assert_eq!(history.records.len(), 1);
        assert_eq!(history.latest().unwrap().total_time_secs, 25.0);
    }

    #[test]
    fn test_trend_history_caps_at_30() {
        let mut history = TrendHistory::default();
        for i in 0..35 {
            history.add_record(BuildRecord {
                timestamp: i,
                check_time_secs: 0.0,
                build_time_secs: 0.0,
                total_time_secs: i as f64,
                rustc_version: "test".into(),
            });
        }
        assert_eq!(history.records.len(), 30);
        assert_eq!(history.records.last().unwrap().total_time_secs, 34.0);
    }

    #[test]
    fn test_trend_median_odd() {
        let mut history = TrendHistory::default();
        for i in 0..5 {
            history.add_record(BuildRecord {
                timestamp: i,
                check_time_secs: 0.0,
                build_time_secs: 0.0,
                total_time_secs: (i * 10) as f64,
                rustc_version: "test".into(),
            });
        }
        assert!((history.median_time().unwrap() - 20.0).abs() < 1e-6);
    }

    #[test]
    fn test_trend_direction_stable() {
        let mut history = TrendHistory::default();
        for i in 0..5 {
            history.add_record(BuildRecord {
                timestamp: i,
                check_time_secs: 0.0,
                build_time_secs: 0.0,
                total_time_secs: 30.0,
                rustc_version: "test".into(),
            });
        }
        assert_eq!(history.trend_direction().unwrap(), "stable");
    }

    #[test]
    fn test_trend_direction_degrading() {
        let mut history = TrendHistory::default();
        let values = vec![20.0, 22.0, 25.0, 28.0, 32.0];
        for (i, v) in values.iter().enumerate() {
            history.add_record(BuildRecord {
                timestamp: i as u64,
                check_time_secs: 0.0,
                build_time_secs: 0.0,
                total_time_secs: *v,
                rustc_version: "test".into(),
            });
        }
        assert_eq!(history.trend_direction().unwrap(), "degrading");
    }

    #[test]
    fn test_trend_insufficient_data() {
        let mut history = TrendHistory::default();
        history.add_record(BuildRecord {
            timestamp: 0,
            check_time_secs: 0.0,
            build_time_secs: 0.0,
            total_time_secs: 10.0,
            rustc_version: "test".into(),
        });
        history.add_record(BuildRecord {
            timestamp: 1,
            check_time_secs: 0.0,
            build_time_secs: 0.0,
            total_time_secs: 15.0,
            rustc_version: "test".into(),
        });
        assert!(history.trend_direction().is_none());
    }

    #[test]
    fn test_build_record_roundtrip() {
        let record = BuildRecord {
            timestamp: 12345,
            check_time_secs: 5.2,
            build_time_secs: 30.1,
            total_time_secs: 35.3,
            rustc_version: "rustc 1.70.0".into(),
        };
        let json = serde_json::to_string(&record).unwrap();
        let parsed: BuildRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.timestamp, 12345);
        assert!((parsed.total_time_secs - 35.3).abs() < 1e-6);
    }
}
