use crate::utils::get_project_root;
use anyhow::{Context, Result};
use colored::*;
use std::collections::HashMap;
use std::fs;
use std::io::BufWriter;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

pub struct TraceOptions {
    pub export_json: bool,
    pub export_html: bool,
}

pub fn run(options: TraceOptions) -> Result<()> {
    println!("{}", "Build Trace & Bottleneck Analysis...".bold().cyan());

    let root = get_project_root().context("Could not find project root")?;
    let trace_dir = root.join(".cargo-accelerate").join("traces");
    fs::create_dir_all(&trace_dir)?;

    println!("  Capturing build phases...");
    let phases = measure_phases(&root)?;

    print_phase_report(&phases);

    let bottlenecks = identify_bottlenecks(&phases);
    if !bottlenecks.is_empty() {
        println!("\n{}", "Bottlenecks:".bold().yellow());
        for (phase, reason) in &bottlenecks {
            println!("  - {}: {}", phase.bold().red(), reason);
        }
    }

    if options.export_json {
        let json_path = trace_dir.join(format!("trace_{}.json", chrono_now()));
        let file = fs::File::create(&json_path)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, &phases)?;
        println!(
            "  {} JSON trace exported to {}",
            "✔".green(),
            json_path.display()
        );
    }

    if options.export_html {
        let html_path = trace_dir.join(format!("trace_{}.html", chrono_now()));
        let html = generate_html_report(&phases, &bottlenecks);
        fs::write(&html_path, html)?;
        println!(
            "  {} HTML report exported to {}",
            "✔".green(),
            html_path.display()
        );
    }

    println!("\n{}", "Recommendations:".bold().yellow());
    let total_parallel = phases
        .get("check")
        .or(phases.get("build"))
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    if total_parallel > 30.0 {
        println!("  - Build time >30s: consider increasing `codegen-units` or using `-j` flag");
    }
    if total_parallel > 120.0 {
        println!("  - Build time >120s: consider splitting large crates or using sccache");
    }

    Ok(())
}

fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    secs.to_string()
}

fn measure_phases(root: &PathBuf) -> Result<HashMap<String, Duration>> {
    let mut phases = HashMap::new();

    // build subsumes check, so only run build
    for cmd in &["build", "test", "clippy"] {
        print!("  Phase: cargo {}... ", cmd);
        let start = Instant::now();
        let status = Command::new("cargo")
            .arg(cmd)
            .arg("--workspace")
            .current_dir(root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .context(format!("Failed to run cargo {}", cmd))?;
        let elapsed = start.elapsed();
        phases.insert(cmd.to_string(), elapsed);
        if status.success() {
            println!("{:.1}s", elapsed.as_secs_f64());
        } else {
            println!("{}", "failed".yellow());
        }
    }

    // Estimate check time from build (typically 40-60% of build)
    if let Some(build) = phases.get("build") {
        let check_est = build.mul_f64(0.5);
        phases.insert("check".into(), check_est);
    }

    Ok(phases)
}

fn print_phase_report(phases: &HashMap<String, Duration>) {
    println!("\n{}", "Phase Report:".bold());
    println!("{:<15} {:<15}", "Phase".bold(), "Duration".bold());
    println!("{}", "-------------------------------".cyan());

    let mut sorted: Vec<_> = phases.iter().collect();
    sorted.sort_by(|a, b| b.1.partial_cmp(a.1).unwrap_or(std::cmp::Ordering::Equal));

    for (phase, duration) in &sorted {
        let dur_str = format!("{:.1}s", duration.as_secs_f64());
        let colored = if **duration > Duration::from_secs(30) {
            dur_str.red()
        } else if **duration > Duration::from_secs(10) {
            dur_str.yellow()
        } else {
            dur_str.green()
        };
        println!("{:<15} {}", phase, colored);
    }

    let total: Duration = phases.values().sum();
    println!("{}", "-------------------------------".cyan());
    println!("{:<15} {:.1}s", "Total".bold(), total.as_secs_f64());
}

fn identify_bottlenecks(phases: &HashMap<String, Duration>) -> Vec<(String, String)> {
    let mut bottlenecks = Vec::new();

    if let Some(d) = phases.get("build") {
        if *d > Duration::from_secs(60) {
            bottlenecks.push((
                "build".into(),
                format!(
                    "{:.0}s — consider sccache, splitting crates, or increasing codegen-units",
                    d.as_secs_f64()
                ),
            ));
        }
    }
    if let Some(d) = phases.get("clippy") {
        if *d > Duration::from_secs(30) {
            bottlenecks.push(("clippy".into(), format!("{:.0}s — consider running clippy only on changed files or using clippy --fix selectively", d.as_secs_f64())));
        }
    }

    bottlenecks
}

fn generate_html_report(
    phases: &HashMap<String, Duration>,
    bottlenecks: &[(String, String)],
) -> String {
    let rows: String = phases
        .iter()
        .map(|(phase, dur)| {
            format!(
                "<tr><td>{}</td><td>{:.1}s</td></tr>",
                phase,
                dur.as_secs_f64()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let bottleneck_rows: String = if bottlenecks.is_empty() {
        "<tr><td colspan='2'>None detected</td></tr>".into()
    } else {
        bottlenecks
            .iter()
            .map(|(phase, reason)| format!("<tr><td>{}</td><td>{}</td></tr>", phase, reason))
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        r#"<!DOCTYPE html>
<html>
<head><title>Build Trace Report</title>
<style>
body {{ font-family: monospace; margin: 20px; }}
table {{ border-collapse: collapse; width: 100%; }}
th, td {{ text-align: left; padding: 8px; border-bottom: 1px solid #ddd; }}
th {{ background-color: #f2f2f2; }}
tr:hover {{ background-color: #f5f5f5; }}
.warn {{ color: #e67e22; }}
.good {{ color: #27ae60; }}
.bad {{ color: #e74c3c; }}
</style></head>
<body>
<h1>Build Trace Report</h1>
<h2>Phase Durations</h2>
<table><tr><th>Phase</th><th>Duration</th></tr>
{rows}
</table>
<h2>Bottlenecks</h2>
<table><tr><th>Phase</th><th>Issue</th></tr>
{bottleneck_rows}
</table>
</body>
</html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_identify_bottlenecks_empty() {
        let phases = HashMap::new();
        assert!(identify_bottlenecks(&phases).is_empty());
    }

    #[test]
    fn test_identify_bottlenecks_fast_build() {
        let mut phases = HashMap::new();
        phases.insert("build".into(), Duration::from_secs(10));
        assert!(identify_bottlenecks(&phases).is_empty());
    }

    #[test]
    fn test_identify_bottlenecks_slow_build() {
        let mut phases = HashMap::new();
        phases.insert("build".into(), Duration::from_secs(120));
        let b = identify_bottlenecks(&phases);
        assert!(!b.is_empty());
        assert_eq!(b[0].0, "build");
    }

    #[test]
    fn test_generate_html_report_empty() {
        let phases = HashMap::new();
        let html = generate_html_report(&phases, &[]);
        assert!(html.contains("Build Trace Report"));
        assert!(html.contains("None detected"));
    }

    #[test]
    fn test_generate_html_report_with_data() {
        let mut phases = HashMap::new();
        phases.insert("check".into(), Duration::from_secs(5));
        let bottlenecks = vec![("build".into(), "Too slow".into())];
        let html = generate_html_report(&phases, &bottlenecks);
        assert!(html.contains("check"));
        assert!(html.contains("Too slow"));
    }
}
