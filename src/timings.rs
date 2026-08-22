use anyhow::{Context, Result};
use colored::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BuildRun {
    pub id: u64,
    pub timestamp: u64,
    pub command: String,
    pub duration_ms: u64,
    pub profile: String,
    pub branch: String,
    pub commit_hash: String,
    pub label: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct TimingStore {
    pub runs: Vec<BuildRun>,
    next_id: u64,
}

impl TimingStore {
    pub fn load(path: &Path) -> Result<Self> {
        if path.exists() {
            let content = fs::read_to_string(path)?;
            Ok(serde_json::from_str(&content).unwrap_or_default())
        } else {
            Ok(Self::default())
        }
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = fs::File::create(path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, self)?;
        Ok(())
    }

    pub fn record(&mut self, run: BuildRun) {
        self.next_id = self.next_id.max(run.id.saturating_add(1));
        self.runs.push(run);
    }

    fn allocate_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn query_by_command(&self, command: &str) -> Vec<&BuildRun> {
        self.runs.iter().filter(|r| r.command == command).collect()
    }

    pub fn query_by_profile(&self, profile: &str) -> Vec<&BuildRun> {
        self.runs.iter().filter(|r| r.profile == profile).collect()
    }

    pub fn query_by_branch(&self, branch: &str) -> Vec<&BuildRun> {
        self.runs.iter().filter(|r| r.branch == branch).collect()
    }

    pub fn query_by_label(&self, label: &str) -> Vec<&BuildRun> {
        self.runs
            .iter()
            .filter(|r| r.label.as_deref() == Some(label))
            .collect()
    }

    pub fn query_since(&self, since_unix_ts: u64) -> Vec<&BuildRun> {
        self.runs
            .iter()
            .filter(|r| r.timestamp >= since_unix_ts)
            .collect()
    }

    pub fn latest_n(&self, n: usize) -> Vec<&BuildRun> {
        self.runs.iter().rev().take(n).collect()
    }

    pub fn median_duration_ms(&self, command: &str) -> Option<f64> {
        let mut times: Vec<f64> = self
            .runs
            .iter()
            .filter(|r| r.command == command)
            .map(|r| r.duration_ms as f64)
            .collect();
        if times.is_empty() {
            return None;
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        Some(times[times.len() / 2])
    }

    pub fn avg_duration_ms(&self, command: &str) -> Option<f64> {
        let times: Vec<f64> = self
            .runs
            .iter()
            .filter(|r| r.command == command)
            .map(|r| r.duration_ms as f64)
            .collect();
        if times.is_empty() {
            return None;
        }
        Some(times.iter().sum::<f64>() / times.len() as f64)
    }

    pub fn total_count(&self) -> usize {
        self.runs.len()
    }
}

pub fn get_git_branch(root: &Path) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(root)
        .output()
        .ok();
    match output {
        Some(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        None => "unknown".into(),
    }
}

pub fn get_git_commit_hash(root: &Path) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .ok();
    match output {
        Some(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        None => "unknown".into(),
    }
}

pub fn get_current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn record_build_run(
    store: &mut TimingStore,
    command: &str,
    duration: Duration,
    profile: &str,
    label: Option<&str>,
) {
    let root = crate::utils::get_project_root().ok();
    let branch = root
        .as_ref()
        .map(|r| get_git_branch(r))
        .unwrap_or_else(|| "unknown".into());
    let commit_hash = root
        .as_ref()
        .map(|r| get_git_commit_hash(r))
        .unwrap_or_else(|| "unknown".into());

    let id = store.allocate_id();
    let run = BuildRun {
        id,
        timestamp: get_current_timestamp(),
        command: command.to_string(),
        duration_ms: duration.as_millis() as u64,
        profile: profile.to_string(),
        branch,
        commit_hash,
        label: label.map(|s| s.to_string()),
    };
    store.record(run);
}

pub fn get_store_path(root: &Path) -> PathBuf {
    root.join(".cargo-accelerate").join("timings.json")
}

pub fn run_list(args: ListArgs) -> Result<()> {
    let root = crate::utils::get_project_root().context("Could not find project root")?;
    let store_path = get_store_path(&root);
    let store = TimingStore::load(&store_path)?;

    let runs = if let Some(cmd) = args.command {
        store.query_by_command(&cmd)
    } else if let Some(n) = args.last {
        store.latest_n(n)
    } else {
        store.latest_n(20)
    };

    if runs.is_empty() {
        println!("  No timing records found.");
        return Ok(());
    }

    println!(
        "{:<4} {:<20} {:<10} {:<12} {:<12} {:<10} {:<10}",
        "ID".bold(),
        "Timestamp".bold(),
        "Command".bold(),
        "Duration".bold(),
        "Profile".bold(),
        "Branch".bold(),
        "Label".bold()
    );
    println!("{}", "-".repeat(90).cyan());

    for run in runs {
        let ts = chrono_like_timestamp(run.timestamp);
        let duration = format_duration(run.duration_ms);
        let label = run.label.as_deref().unwrap_or("-");
        let branch = run.branch.chars().take(10).collect::<String>();
        println!(
            "{:<4} {:<20} {:<10} {:<12} {:<12} {:<10} {:<10}",
            run.id, ts, run.command, duration, run.profile, branch, label
            &run.branch
        };
        println!(
            "{:<4} {:<20} {:<10} {:<12} {:<12} {:<10} {:<10}",
            run.id, ts, run.command, duration, run.profile, branch, label
        );
    }

    println!("\n{} records total", store.runs.len());
    Ok(())
}

pub struct ListArgs {
    pub command: Option<String>,
    pub last: Option<usize>,
}

fn chrono_like_timestamp(unix_ts: u64) -> String {
    let secs = unix_ts as i64;
    let nanos = 0u32;
    let dt = time_to_datetime(secs, nanos);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        dt.0, dt.1, dt.2, dt.3, dt.4, dt.5
    )
}

fn time_to_datetime(secs: i64, _nanos: u32) -> (i32, u32, u32, u32, u32, u32) {
    let s = secs;
    // days since epoch
    let mut days = s / 86400;
    let n = if s < 0 {
        days -= 1;
        let rem = s % 86400;
        if rem < 0 { rem + 86400 } else { rem }
    } else {
        s % 86400
    };
    let hours = (n / 3600) as u32;
    let minutes = ((n % 3600) / 60) as u32;
    let seconds = (n % 60) as u32;

    // year/month/day from days since 1970-01-01
    let mut y = 1970i32;
    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        y += 1;
    }
    let leap = is_leap(y);
    let month_days: [i64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 0u32;
    for (i, &md) in month_days.iter().enumerate() {
        if days < md {
            m = (i + 1) as u32;
            break;
        }
        days -= md;
    }
    let d = (days + 1) as u32;

    (y, m, d, hours, minutes, seconds)
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn format_duration(ms: u64) -> String {
    if ms >= 60_000 {
        format!("{:.1}m", ms as f64 / 60_000.0)
    } else if ms >= 1_000 {
        format!("{:.1}s", ms as f64 / 1_000.0)
    } else {
        format!("{}ms", ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_timing_store_default() {
        let store = TimingStore::default();
        assert_eq!(store.runs.len(), 0);
        assert_eq!(store.next_id, 0);
    }

    #[test]
    fn test_record_and_allocate_id() {
        let mut store = TimingStore::default();
        let run = BuildRun {
            id: store.allocate_id(),
            timestamp: 1000,
            command: "build".into(),
            duration_ms: 5000,
            profile: "dev".into(),
            branch: "main".into(),
            commit_hash: "abc123".into(),
            label: None,
        };
        store.record(run);
        assert_eq!(store.runs.len(), 1);
        assert_eq!(store.next_id, 1);
        assert_eq!(store.runs[0].id, 0);

        let run2 = BuildRun {
            id: store.allocate_id(),
            timestamp: 2000,
            command: "test".into(),
            duration_ms: 3000,
            profile: "ci".into(),
            branch: "main".into(),
            commit_hash: "abc123".into(),
            label: Some("baseline".into()),
        };
        store.record(run2);
        assert_eq!(store.runs.len(), 2);
        assert_eq!(store.next_id, 2);
        assert_eq!(store.runs[1].id, 1);
    }

    #[test]
    fn test_query_by_command() {
        let mut store = TimingStore::default();
        for i in 0..5 {
            let run = BuildRun {
                id: i,
                timestamp: 1000 + i,
                command: if i % 2 == 0 { "build".into() } else { "test".into() },
                duration_ms: 1000 * (i + 1),
                profile: "dev".into(),
                branch: "main".into(),
                commit_hash: "abc".into(),
                label: None,
            };
            store.record(run);
        }
        assert_eq!(store.query_by_command("build").len(), 3);
        assert_eq!(store.query_by_command("test").len(), 2);
        assert_eq!(store.query_by_command("check").len(), 0);
    }

    #[test]
    fn test_query_by_profile() {
        let mut store = TimingStore::default();
        store.record(BuildRun {
            id: 0,
            timestamp: 1000,
            command: "build".into(),
            duration_ms: 5000,
            profile: "dev".into(),
            branch: "main".into(),
            commit_hash: "abc".into(),
            label: None,
        });
        store.record(BuildRun {
            id: 1,
            timestamp: 1001,
            command: "build".into(),
            duration_ms: 8000,
            profile: "release".into(),
            branch: "main".into(),
            commit_hash: "abc".into(),
            label: None,
        });
        assert_eq!(store.query_by_profile("dev").len(), 1);
        assert_eq!(store.query_by_profile("release").len(), 1);
    }

    #[test]
    fn test_query_by_branch() {
        let mut store = TimingStore::default();
        store.record(BuildRun {
            id: 0,
            timestamp: 1000,
            command: "build".into(),
            duration_ms: 5000,
            profile: "dev".into(),
            branch: "main".into(),
            commit_hash: "abc".into(),
            label: None,
        });
        store.record(BuildRun {
            id: 1,
            timestamp: 1001,
            command: "build".into(),
            duration_ms: 8000,
            profile: "dev".into(),
            branch: "feature".into(),
            commit_hash: "def".into(),
            label: None,
        });
        assert_eq!(store.query_by_branch("main").len(), 1);
        assert_eq!(store.query_by_branch("feature").len(), 1);
        assert_eq!(store.query_by_branch("other").len(), 0);
    }

    #[test]
    fn test_query_by_label() {
        let mut store = TimingStore::default();
        store.record(BuildRun {
            id: 0,
            timestamp: 1000,
            command: "build".into(),
            duration_ms: 5000,
            profile: "dev".into(),
            branch: "main".into(),
            commit_hash: "abc".into(),
            label: Some("baseline".into()),
        });
        store.record(BuildRun {
            id: 1,
            timestamp: 1001,
            command: "build".into(),
            duration_ms: 4000,
            profile: "dev".into(),
            branch: "main".into(),
            commit_hash: "abc".into(),
            label: Some("after-optimize".into()),
        });
        assert_eq!(store.query_by_label("baseline").len(), 1);
        assert_eq!(store.query_by_label("after-optimize").len(), 1);
        assert_eq!(store.query_by_label("nonexistent").len(), 0);
    }

    #[test]
    fn test_query_since() {
        let mut store = TimingStore::default();
        for i in 0..5 {
            store.record(BuildRun {
                id: i,
                timestamp: 1000 + i * 10,
                command: "build".into(),
                duration_ms: 1000,
                profile: "dev".into(),
                branch: "main".into(),
                commit_hash: "abc".into(),
                label: None,
            });
        }
        assert_eq!(store.query_since(1025).len(), 2);
        assert_eq!(store.query_since(1050).len(), 0);
        assert_eq!(store.query_since(1000).len(), 5);
    }

    #[test]
    fn test_latest_n() {
        let mut store = TimingStore::default();
        for i in 0..10 {
            store.record(BuildRun {
                id: i,
                timestamp: 1000 + i,
                command: "build".into(),
                duration_ms: 1000,
                profile: "dev".into(),
                branch: "main".into(),
                commit_hash: "abc".into(),
                label: None,
            });
        }
        let latest = store.latest_n(3);
        assert_eq!(latest.len(), 3);
        assert_eq!(latest[0].id, 9);
        assert_eq!(latest[1].id, 8);
        assert_eq!(latest[2].id, 7);
    }

    #[test]
    fn test_median_duration_ms() {
        let mut store = TimingStore::default();
        for i in 0..5 {
            store.record(BuildRun {
                id: i,
                timestamp: 1000,
                command: "build".into(),
                duration_ms: 1000 * (i + 1),
                profile: "dev".into(),
                branch: "main".into(),
                commit_hash: "abc".into(),
                label: None,
            });
        }
        let median = store.median_duration_ms("build");
        assert!(median.is_some());
        assert!((median.unwrap() - 3000.0).abs() < 1.0);

        assert!(store.median_duration_ms("test").is_none());
    }

    #[test]
    fn test_avg_duration_ms() {
        let mut store = TimingStore::default();
        store.record(BuildRun {
            id: 0,
            timestamp: 1000,
            command: "build".into(),
            duration_ms: 2000,
            profile: "dev".into(),
            branch: "main".into(),
            commit_hash: "abc".into(),
            label: None,
        });
        store.record(BuildRun {
            id: 1,
            timestamp: 1001,
            command: "build".into(),
            duration_ms: 4000,
            profile: "dev".into(),
            branch: "main".into(),
            commit_hash: "abc".into(),
            label: None,
        });
        let avg = store.avg_duration_ms("build");
        assert!(avg.is_some());
        assert!((avg.unwrap() - 3000.0).abs() < 1.0);

        assert!(store.avg_duration_ms("test").is_none());
    }

    #[test]
    fn test_total_count() {
        let mut store = TimingStore::default();
        assert_eq!(store.total_count(), 0);
        for i in 0..5 {
            store.record(BuildRun {
                id: i,
                timestamp: 1000,
                command: "build".into(),
                duration_ms: 1000,
                profile: "dev".into(),
                branch: "main".into(),
                commit_hash: "abc".into(),
                label: None,
            });
        }
        assert_eq!(store.total_count(), 5);
    }

    #[test]
    fn test_store_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timings.json");

        let mut store = TimingStore::default();
        for i in 0..3 {
            let run = BuildRun {
                id: store.allocate_id(),
                timestamp: 1000 + i,
                command: "build".into(),
                duration_ms: 1000 * (i + 1),
                profile: "dev".into(),
                branch: "main".into(),
                commit_hash: format!("abc{}", i),
                label: None,
            };
            store.record(run);
        }
        store.save(&path).unwrap();

        let loaded = TimingStore::load(&path).unwrap();
        assert_eq!(loaded.runs.len(), 3);
        assert_eq!(loaded.next_id, 3);
        assert_eq!(loaded.runs[0].duration_ms, 1000);
        assert_eq!(loaded.runs[2].duration_ms, 3000);
    }

    #[test]
    fn test_load_nonexistent_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        let store = TimingStore::load(&path).unwrap();
        assert_eq!(store.runs.len(), 0);
        assert_eq!(store.next_id, 0);
    }

    #[test]
    fn test_record_build_run_helper() {
        let mut store = TimingStore::default();
        let duration = Duration::from_millis(5500);

        record_build_run(&mut store, "build", duration, "release", Some("test-run"));
        assert_eq!(store.runs.len(), 1);
        assert_eq!(store.runs[0].command, "build");
        assert_eq!(store.runs[0].duration_ms, 5500);
        assert_eq!(store.runs[0].profile, "release");
        assert_eq!(store.runs[0].label.as_deref(), Some("test-run"));

        record_build_run(&mut store, "check", Duration::from_millis(1200), "dev", None);
        assert_eq!(store.runs.len(), 2);
        assert_eq!(store.runs[1].command, "check");
        assert_eq!(store.runs[1].label, None);
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(500), "500ms");
        assert_eq!(format_duration(1500), "1.5s");
        assert_eq!(format_duration(120000), "2.0m");
    }

    #[test]
    fn test_time_to_datetime() {
        // 1970-01-01 00:00:00
        let dt = time_to_datetime(0, 0);
        assert_eq!(dt, (1970, 1, 1, 0, 0, 0));

        // 2024-01-15 11:50:45 UTC
        let ts = 1705319445;
        let dt = time_to_datetime(ts, 0);
        assert_eq!(dt, (2024, 1, 15, 11, 50, 45));
    }

    #[test]
    fn test_is_leap() {
        assert!(is_leap(2000));
        assert!(!is_leap(1900));
        assert!(!is_leap(2023));
        assert!(is_leap(2024));
    }
}
