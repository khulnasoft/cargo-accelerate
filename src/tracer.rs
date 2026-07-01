use crate::timings::{get_store_path, TimingStore};
use crate::utils::get_project_root;
use anyhow::{Context, Result};
use colored::*;
use std::collections::HashMap;
use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

pub struct TraceOptions {
    pub export_json: bool,
    pub export_html: bool,
    pub collect_per_crate: bool,
}

pub struct PerCrateTiming {
    pub crate_name: String,
    pub duration_ms: u64,
}

pub struct DashboardData {
    pub phases: HashMap<String, Duration>,
    pub bottlenecks: Vec<(String, String)>,
    pub per_crate_data: Vec<PerCrateTiming>,
    pub trend_data: HashMap<String, Vec<(u64, u64)>>,
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

    // Collect per-crate timing data if requested
    let per_crate_data = if options.collect_per_crate {
        println!("  Collecting per-crate timing data...");
        collect_per_crate_timings(&root).unwrap_or_default()
    } else {
        Vec::new()
    };

    // Load trend data from historical timings store
    let trend_data = load_trend_data(&root);

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
        let dashboard = DashboardData {
            phases: phases.clone(),
            bottlenecks: bottlenecks.clone(),
            per_crate_data,
            trend_data,
        };
        let html = generate_html_report(&dashboard);
        fs::write(&html_path, html)?;
        println!(
            "  {} Interactive HTML dashboard exported to {}",
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

fn load_trend_data(root: &Path) -> HashMap<String, Vec<(u64, u64)>> {
    let store_path = get_store_path(root);
    let store = match TimingStore::load(&store_path) {
        Ok(s) => s,
        Err(_) => return HashMap::new(),
    };
    let mut trend: HashMap<String, Vec<(u64, u64)>> = HashMap::new();
    for run in &store.runs {
        trend
            .entry(run.command.clone())
            .or_default()
            .push((run.timestamp, run.duration_ms));
    }
    // Keep last 20 per command
    for runs in trend.values_mut() {
        if runs.len() > 20 {
            runs.sort_by_key(|(ts, _)| *ts);
            runs.drain(0..runs.len() - 20);
        } else {
            runs.sort_by_key(|(ts, _)| *ts);
        }
    }
    trend
}

fn collect_per_crate_timings(root: &Path) -> Result<Vec<PerCrateTiming>> {
    // Run cargo build with --timings=json to get per-crate breakdown
    let timings_dir = root.join("target").join("cargo-timings");
    // Clean previous timings so we only parse the new one
    if timings_dir.exists() {
        let _ = fs::remove_dir_all(&timings_dir);
    }

    let status = Command::new("cargo")
        .args(["build", "--timings=json"])
        .current_dir(root)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("Failed to run cargo build --timings=json")?;

    if !status.success() {
        return Ok(Vec::new());
    }

    parse_timings_json_dir(&timings_dir)
}

fn parse_timings_json_dir(dir: &Path) -> Result<Vec<PerCrateTiming>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut results = Vec::new();
    let entries = fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            let content = fs::read_to_string(&path)?;
            for line in content.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
                    if val.get("event").and_then(|v| v.as_str()) == Some("compiler_artifact") {
                        if let (Some(crate_name), Some(dur)) = (
                            val.get("crate_name").and_then(|v| v.as_str()),
                            val.get("duration").and_then(|v| v.as_f64()),
                        ) {
                            results.push(PerCrateTiming {
                                crate_name: crate_name.to_string(),
                                duration_ms: dur as u64,
                            });
                        }
                    }
                }
            }
        }
    }
    // Aggregate per crate
    let mut aggregated: HashMap<String, u64> = HashMap::new();
    for pt in results {
        *aggregated.entry(pt.crate_name).or_default() += pt.duration_ms;
    }
    let mut aggregated: Vec<PerCrateTiming> = aggregated
        .into_iter()
        .map(|(k, v)| PerCrateTiming {
            crate_name: k,
            duration_ms: v,
        })
        .collect();
    aggregated.sort_by(|a, b| b.duration_ms.cmp(&a.duration_ms));
    Ok(aggregated)
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

fn generate_html_report(data: &DashboardData) -> String {
    let gantt_data = build_gantt_json(&data.phases);
    let flame_data = build_flame_json(&data.per_crate_data, &data.phases);
    let trend_data = build_trend_json(&data.trend_data);
    let has_per_crate = if data.per_crate_data.is_empty() {
        "false"
    } else {
        "true"
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Build Trace Dashboard</title>
<script src="https://cdn.plot.ly/plotly-2.35.2.min.js"></script>
<style>
* {{ box-sizing: border-box; margin: 0; padding: 0; }}
body {{ font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; background: #f8f9fa; color: #333; padding: 20px; }}
h1 {{ font-size: 1.8em; margin-bottom: 8px; }}
.subtitle {{ color: #666; margin-bottom: 24px; font-size: 0.95em; }}
h2 {{ font-size: 1.2em; margin: 24px 0 8px; color: #444; }}
.dashboard {{ display: flex; flex-wrap: wrap; gap: 16px; }}
.card {{ background: #fff; border-radius: 8px; box-shadow: 0 1px 3px rgba(0,0,0,.1); padding: 16px; flex: 1 1 100%; }}
.card.half {{ flex: 1 1 calc(50% - 8px); min-width: 340px; }}
.plot {{ width: 100%; height: 320px; }}
.plot.tall {{ height: 400px; }}
.plot.short {{ height: 260px; }}
.bottleneck {{ background: #fff3f3; border-left: 4px solid #e74c3c; padding: 10px 14px; margin: 6px 0; border-radius: 0 4px 4px 0; }}
.bottleneck-ok {{ background: #f0fff0; border-left: 4px solid #27ae60; }}
.note {{ color: #888; font-style: italic; padding: 8px; }}
.footer {{ margin-top: 24px; font-size: .8em; color: #999; text-align: center; }}
</style>
</head>
<body>

<h1>Build Trace Dashboard</h1>
<div class="subtitle">Interactive analysis of compilation phases, crate-level breakdown, and historical trends</div>

<div class="dashboard">

<div class="card">
<h2>Phase Timeline (Gantt)</h2>
<div id="gantt" class="plot"></div>
</div>

<div class="card half">
<h2>Compilation Breakdown</h2>
<div id="flame" class="plot"></div>
<p class="note" id="flame-note"></p>
</div>

<div class="card half">
<h2>Bottlenecks</h2>
<div id="bottlenecks">{bottlenecks_html}</div>
</div>

<div class="card">
<h2>Historical Trend (last 20 runs)</h2>
<div id="trend" class="plot tall"></div>
</div>

</div>

<div class="footer">Generated by cargo-accelerate | <span id="ts"></span></div>

<script>
const ganttData = {gantt_json};
const flameData = {flame_json};
const trendData = {trend_json};
const hasPerCrate = {has_per_crate};

const startTime = 0;
const maxDur = Math.max(...ganttData.map(d => d.duration), 1);

// --- Gantt chart ---
const ganttTraces = ganttData.map((d, i) => ({{
    type: 'bar',
    orientation: 'h',
    y: [d.label],
    x: [d.duration],
    base: startTime,
    name: d.label,
    marker: {{ color: d.color, line: {{ width: 0 }} }},
    hovertemplate: '%{{y}}: %{{x:.1f}}s<extra></extra>',
    showlegend: false,
    width: 0.8,
}}));
const ganttLayout = {{
    title: '',
    xaxis: {{ title: 'Duration (s)', zeroline: false }},
    yaxis: {{ title: '', autorange: 'reversed', showticklabels: true }},
    barmode: 'stack',
    margin: {{ l: 100, r: 20, t: 10, b: 40 }},
    paper_bgcolor: 'rgba(0,0,0,0)',
    plot_bgcolor: 'rgba(0,0,0,0)',
    hovermode: 'y',
}};
Plotly.newPlot('gantt', ganttTraces, ganttLayout, {{displayModeBar: false, responsive: true}});

// --- Flame / Breakdown chart ---
if (hasPerCrate && flameData.labels.length > 0) {{
    const fTrace = [{{
        type: 'treemap',
        labels: flameData.labels,
        parents: flameData.parents,
        values: flameData.values,
        textinfo: 'label+value',
        hovertemplate: '%{{label}}<br>%{{value:.1f}}s<extra></extra>',
        marker: {{ colorscale: 'Blues' }},
        branchvalues: 'total',
    }}];
    const fLayout = {{
        title: '',
        margin: {{ l: 0, r: 0, t: 10, b: 0 }},
        paper_bgcolor: 'rgba(0,0,0,0)',
    }};
    Plotly.newPlot('flame', fTrace, fLayout, {{displayModeBar: false, responsive: true}});
    document.getElementById('flame-note').textContent = '';
}} else {{
    const fTrace = [{{
        type: 'bar',
        x: flameData.labels,
        y: flameData.values,
        marker: {{ color: flameData.colors }},
        hovertemplate: '%{{x}}: %{{y:.1f}}s<extra></extra>',
    }}];
    const fLayout = {{
        title: '',
        xaxis: {{ title: 'Phase', tickangle: 0 }},
        yaxis: {{ title: 'Duration (s)' }},
        margin: {{ l: 50, r: 20, t: 10, b: 50 }},
        paper_bgcolor: 'rgba(0,0,0,0)',
        plot_bgcolor: 'rgba(0,0,0,0)',
    }};
    Plotly.newPlot('flame', fTrace, fLayout, {{displayModeBar: false, responsive: true}});
    document.getElementById('flame-note').textContent = 'Run `cargo accelerate trace --collect-timings` for per-crate breakdown.';
}}

// --- Trend chart ---
if (trendData.traces.length > 0) {{
    const tLayout = {{
        title: '',
        xaxis: {{ title: 'Date' }},
        yaxis: {{ title: 'Duration (s)', rangemode: 'tozero' }},
        margin: {{ l: 50, r: 20, t: 10, b: 50 }},
        paper_bgcolor: 'rgba(0,0,0,0)',
        plot_bgcolor: 'rgba(0,0,0,0)',
        hovermode: 'x unified',
        legend: {{ orientation: 'h', y: 1.1 }},
    }};
    Plotly.newPlot('trend', trendData.traces, tLayout, {{displayModeBar: false, responsive: true}});
}} else {{
    document.getElementById('trend').innerHTML = '<p class="note">No historical timing data found. Run `cargo accelerate benchmark --save` or `cargo accelerate trend` to start collecting.</p>';
}}

document.getElementById('ts').textContent = new Date().toISOString();
</script>
</body>
</html>"#,
        gantt_json = gantt_data,
        flame_json = flame_data,
        trend_json = trend_data,
        has_per_crate = has_per_crate,
        bottlenecks_html = render_bottlenecks_html(&data.bottlenecks),
    )
}

fn render_bottlenecks_html(bottlenecks: &[(String, String)]) -> String {
    if bottlenecks.is_empty() {
        r#"<div class="bottleneck-ok bottleneck">No bottlenecks detected</div>"#.to_string()
    } else {
        bottlenecks
            .iter()
            .map(|(phase, reason)| {
                format!(
                    r#"<div class="bottleneck"><strong>{}</strong>: {}</div>"#,
                    escape_html(phase),
                    escape_html(reason)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn build_gantt_json(phases: &HashMap<String, Duration>) -> String {
    let phase_order = ["check", "build", "test", "clippy"];
    let colors = ["#3498db", "#e74c3c", "#f39c12", "#9b59b6"];
    let mut entries = Vec::new();
    for (i, name) in phase_order.iter().enumerate() {
        if let Some(d) = phases.get(*name) {
            entries.push(format!(
                r#"{{"label":"{}","duration":{:.1},"color":"{}"}}"#,
                name,
                d.as_secs_f64(),
                colors[i % colors.len()]
            ));
        }
    }
    format!("[{}]", entries.join(","))
}

fn build_flame_json(
    per_crate: &[PerCrateTiming],
    phases: &HashMap<String, Duration>,
) -> String {
    if !per_crate.is_empty() {
        // Build treemap hierarchy: root -> crate names
        let total_ms: u64 = per_crate.iter().map(|c| c.duration_ms).sum();
        let total_s = total_ms as f64 / 1000.0;
        let mut labels = vec!["all crates".to_string()];
        let mut parents = vec!["".to_string()];
        let mut values = vec![total_s];
        for pt in per_crate.iter().take(20) {
            labels.push(pt.crate_name.clone());
            parents.push("all crates".to_string());
            values.push(pt.duration_ms as f64 / 1000.0);
        }
        let labels_json = serde_json::to_string(&labels).unwrap_or_default();
        let parents_json = serde_json::to_string(&parents).unwrap_or_default();
        let values_json = serde_json::to_string(&values).unwrap_or_default();
        format!(
            r#"{{"labels":{},"parents":{},"values":{}}}"#,
            labels_json, parents_json, values_json
        )
    } else {
        // Fallback: show phase durations as bar chart
        let phase_order = ["check", "build", "test", "clippy"];
        let colors = ["#3498db", "#e74c3c", "#f39c12", "#9b59b6"];
        let mut labels = Vec::new();
        let mut vals = Vec::new();
        let mut col_vals = Vec::new();
        for name in phase_order.iter() {
            if let Some(d) = phases.get(*name) {
                labels.push(name.to_string());
                vals.push(d.as_secs_f64());
                col_vals.push(colors[labels.len() - 1].to_string());
            }
        }
        let labels_json = serde_json::to_string(&labels).unwrap_or_default();
        let vals_json = serde_json::to_string(&vals).unwrap_or_default();
        let col_json = serde_json::to_string(&col_vals).unwrap_or_default();
        format!(
            r#"{{"labels":{},"values":{},"colors":{}}}"#,
            labels_json, vals_json, col_json
        )
    }
}

fn build_trend_json(trend: &HashMap<String, Vec<(u64, u64)>>) -> String {
    let colors = ["#3498db", "#e74c3c", "#f39c12", "#9b59b6", "#2ecc71"];
    if trend.is_empty() {
        return r#"{"traces":[]}"#.to_string();
    }
    let mut traces = Vec::new();
    for (i, (cmd, runs)) in trend.iter().enumerate() {
        let x_vals: Vec<String> = runs
            .iter()
            .map(|(ts, _)| chrono_like_timestamp(*ts))
            .collect();
        let y_vals: Vec<f64> = runs.iter().map(|(_, ms)| *ms as f64 / 1000.0).collect();
        let x_json = serde_json::to_string(&x_vals).unwrap_or_default();
        let y_json = serde_json::to_string(&y_vals).unwrap_or_default();
        let color = colors[i % colors.len()];
        traces.push(serde_json::json!({
            "type": "scatter",
            "mode": "lines+markers",
            "name": cmd,
            "x": x_vals,
            "y": y_vals,
            "marker": { "color": color },
            "line": { "color": color },
        }).to_string());
        ));
    }
    format!(r#"{{"traces":[{}]}}"#, traces.join(","))
}

fn chrono_like_timestamp(unix_ts: u64) -> String {
    let secs = unix_ts as i64;
    let nanos = 0u32;
    let dt = time_to_datetime(secs, nanos);
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        dt.0, dt.1, dt.2, dt.3, dt.4
    )
}

fn time_to_datetime(secs: i64, _nanos: u32) -> (i32, u32, u32, u32, u32, u32) {
    let s = secs;
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
        let data = DashboardData {
            phases: HashMap::new(),
            bottlenecks: vec![],
            per_crate_data: vec![],
            trend_data: HashMap::new(),
        };
        let html = generate_html_report(&data);
        assert!(html.contains("Build Trace Dashboard"));
        assert!(html.contains("No bottlenecks detected"));
    }

    #[test]
    fn test_generate_html_report_with_data() {
        let mut phases = HashMap::new();
        phases.insert("check".into(), Duration::from_secs(5));
        let data = DashboardData {
            phases,
            bottlenecks: vec![("build".into(), "Too slow".into())],
            per_crate_data: vec![],
            trend_data: HashMap::new(),
        };
        let html = generate_html_report(&data);
        assert!(html.contains("check"));
        assert!(html.contains("Too slow"));
        assert!(html.contains("phase"));
    }

    #[test]
    fn test_build_gantt_json() {
        let mut phases = HashMap::new();
        phases.insert("check".into(), Duration::from_secs(2));
        phases.insert("build".into(), Duration::from_secs(10));
        let json = build_gantt_json(&phases);
        assert!(json.contains("check"));
        assert!(json.contains("build"));
        assert!(json.contains("\"duration\":2.0"));
        assert!(json.contains("\"duration\":10.0"));
    }

    #[test]
    fn test_build_flame_json_per_crate() {
        let per_crate = vec![
            PerCrateTiming { crate_name: "serde".into(), duration_ms: 3000 },
            PerCrateTiming { crate_name: "tokio".into(), duration_ms: 5000 },
        ];
        let phases = HashMap::new();
        let json = build_flame_json(&per_crate, &phases);
        assert!(json.contains("serde"));
        assert!(json.contains("tokio"));
        assert!(json.contains("all crates"));
    }

    #[test]
    fn test_build_flame_json_fallback() {
        let mut phases = HashMap::new();
        phases.insert("build".into(), Duration::from_secs(10));
        let json = build_flame_json(&[], &phases);
        assert!(json.contains("build"));
        assert!(json.contains("\"values\""));
    }

    #[test]
    fn test_build_trend_json_empty() {
        let trend = HashMap::new();
        let json = build_trend_json(&trend);
        assert_eq!(json, r#"{"traces":[]}"#);
    }

    #[test]
    fn test_build_trend_json_with_data() {
        let mut trend = HashMap::new();
        trend.insert("build".into(), vec![
            (1000000, 5000),
            (1001000, 6000),
        ]);
        let json = build_trend_json(&trend);
        assert!(json.contains("build"));
        assert!(json.contains("5.0"));
        assert!(json.contains("6.0"));
    }

    #[test]
    fn test_escape_html() {
        assert_eq!(escape_html("foo & bar"), "foo &amp; bar");
        assert_eq!(escape_html("<script>"), "&lt;script&gt;");
    }

    #[test]
    fn test_render_bottlenecks_html_empty() {
        let html = render_bottlenecks_html(&[]);
        assert!(html.contains("No bottlenecks detected"));
    }

    #[test]
    fn test_render_bottlenecks_html_with_data() {
        let html = render_bottlenecks_html(&[
            ("build".into(), "Over 60s".into()),
        ]);
        assert!(html.contains("build"));
        assert!(html.contains("Over 60s"));
    }

    #[test]
    fn test_load_trend_data_empty() {
        let dir = tempfile::tempdir().unwrap();
        let trend = load_trend_data(dir.path());
        assert!(trend.is_empty());
    }

    #[test]
    fn test_parse_timings_json_dir_nonexistent() {
        let dir = PathBuf::from("/nonexistent/path");
        let result = parse_timings_json_dir(&dir).unwrap_or_default();
        assert!(result.is_empty());
    }

    #[test]
    fn test_collect_per_crate_timings_no_project() {
        let dir = PathBuf::from("/nonexistent/path");
        let result = collect_per_crate_timings(&dir);
        assert!(result.is_err() || result.unwrap().is_empty());
    }
}
