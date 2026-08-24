use crate::utils::{get_cached_metadata_with_deps, get_project_root};
use anyhow::{Context, Result};
use cargo_metadata::PackageId;
use colored::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::Path;

struct CrateNode {
    name: String,
    #[allow(dead_code)]
    loc: usize,
    dep_count: usize,
    estimated_time: f64,
    #[allow(dead_code)]
    depth: usize,
    /// Number of crates that depend on this crate
    fan_in: usize,
    /// Number of crates this crate depends on within the workspace
    fan_out: usize,
}

struct Edge {
    from: String,
    to: String,
    cost: f64,
}

pub fn run() -> Result<()> {
    println!("{}", "Analyzing Build Graph...".bold().cyan());

    let metadata = get_cached_metadata_with_deps()?;

    let mut all_crates = Vec::new();

    for package in &metadata.packages {
        let name = &package.name;
        let dep_count = package.dependencies.len();
        let estimated_time = estimate_crate_cost(name, dep_count);
        all_crates.push(CrateNode {
            name: name.to_string(),
            loc: 0,
            dep_count,
            estimated_time,
            depth: 0,
            fan_in: 0,
            fan_out: 0,
        });
    }

    all_crates.sort_by(|a, b| b.estimated_time.partial_cmp(&a.estimated_time).unwrap());

    let total_cost: f64 = all_crates.iter().map(|c| c.estimated_time).sum();

    println!(
        "\n{:<30} {:<12} {:<15}",
        "Crate".bold(),
        "Direct Deps".bold(),
        "Est. Cost (s)".bold()
    );
    println!(
        "{}",
        "--------------------------------------------------------------".cyan()
    );
    for c in &all_crates {
        let colored = if c.estimated_time > 10.0 {
            c.name.red()
        } else if c.estimated_time > 4.0 {
            c.name.yellow()
        } else {
            c.name.normal()
        };
        println!(
            "{:<30} {:<12} {:.1}s",
            colored,
            c.dep_count.to_string().cyan(),
            c.estimated_time
        );
    }

    println!("\n  Total estimated compile cost: {:.1}s", total_cost);

    let resolve = metadata.resolve.as_ref().context("No resolve graph")?;
    let edges = extract_edges(&resolve.nodes);

    if let Some(critical) = find_critical_path(&edges, &all_crates) {
        println!(
            "\n{}",
            "Critical Path (longest dependency chain):".bold().yellow()
        );
        for (i, name) in critical.iter().enumerate() {
            let prefix = if i == 0 {
                "  ┌ "
            } else if i == critical.len() - 1 {
                "  └ "
            } else {
                "  ├ "
            };
            println!("{}{}", prefix, name.bold());
        }
    }

    let high_cost: Vec<&CrateNode> = all_crates
        .iter()
        .filter(|c| c.estimated_time > 10.0)
        .collect();
    if !high_cost.is_empty() {
        println!("\n{}", "Split Recommendations:".bold().yellow());
        for c in &high_cost {
            println!(
                "  - '{}' ({:.1}s) is a heavy crate. Consider splitting into sub-crates.",
                c.name.bold().red(),
                c.estimated_time
            );
        }
    }

    update_fan_metrics(&mut all_crates, &edges);
    print_fan_analysis(&all_crates);
    print_partitioning_candidates(&all_crates, &edges);

    // Cross-crate split suggestions
    let root = get_project_root().context("Could not find project root")?;
    let split_report = suggest_crate_splits(metadata, &edges, &root)?;
    print_split_suggestions(&split_report);
    save_split_report(&root, &split_report)?;

    Ok(())
}

fn update_fan_metrics(crates: &mut [CrateNode], edges: &[Edge]) {
    let name_to_fan: Vec<(String, usize, usize)> = {
        let names: HashSet<&str> = crates.iter().map(|c| c.name.as_str()).collect();
        crates
            .iter()
            .map(|c| {
                let fan_in = edges
                    .iter()
                    .filter(|e| e.to == c.name && names.contains(e.from.as_str()))
                    .count();
                let fan_out = edges
                    .iter()
                    .filter(|e| e.from == c.name && names.contains(e.to.as_str()))
                    .count();
                (c.name.clone(), fan_in, fan_out)
            })
            .collect()
    };
    for c in crates.iter_mut() {
        if let Some((_, fi, fo)) = name_to_fan.iter().find(|(n, _, _)| *n == c.name) {
            c.fan_in = *fi;
            c.fan_out = *fo;
        }
    }
}

fn print_fan_analysis(crates: &[CrateNode]) {
    println!("\n{}", "Fan-In / Fan-Out Analysis:".bold().cyan());
    println!(
        "{:<30} {:<10} {:<10}",
        "Crate".bold(),
        "Fan-In".bold(),
        "Fan-Out".bold()
    );
    println!(
        "{}",
        "--------------------------------------------------------------".cyan()
    );
    for c in crates {
        let fan_in_str = if c.fan_in > 5 {
            c.fan_in.to_string().red()
        } else {
            c.fan_in.to_string().green()
        };
        let fan_out_str = if c.fan_out > 20 {
            c.fan_out.to_string().yellow()
        } else {
            c.fan_out.to_string().green()
        };
        println!("{:<30} {:<10} {:<10}", c.name, fan_in_str, fan_out_str);
    }
}

fn print_partitioning_candidates(crates: &[CrateNode], edges: &[Edge]) {
    let workspace_names: HashSet<&str> = crates.iter().map(|c| c.name.as_str()).collect();
    let internal_edges: Vec<&Edge> = edges
        .iter()
        .filter(|e| {
            workspace_names.contains(e.from.as_str()) && workspace_names.contains(e.to.as_str())
        })
        .collect();

    let mut component_edges: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in &internal_edges {
        component_edges
            .entry(edge.from.as_str())
            .or_default()
            .push(edge.to.as_str());
    }

    let mut high_own_deps: Vec<(&str, usize)> = crates
        .iter()
        .filter(|c| c.fan_out >= 5)
        .map(|c| (c.name.as_str(), c.fan_out))
        .collect();
    high_own_deps.sort_by_key(|x| std::cmp::Reverse(x.1));

    if !high_own_deps.is_empty() {
        println!("\n{}", "Workspace Partitioning Candidates:".bold().yellow());
        println!("  Crates with high internal dependency count may benefit from being extracted into separate crates or feature-gated:");
        for (name, count) in high_own_deps.iter().take(5) {
            println!(
                "    - {} ({} internal deps) — consider making optional or extracting",
                name.bold().red(),
                count
            );
        }
    }

    let mut is_utility: HashMap<&str, bool> = HashMap::new();
    for c in crates {
        is_utility.insert(c.name.as_str(), c.fan_in > 3 && c.fan_out < 3);
    }

    let utility: Vec<&CrateNode> = crates
        .iter()
        .filter(|c| *is_utility.get(c.name.as_str()).unwrap_or(&false))
        .collect();
    if !utility.is_empty() {
        println!(
            "\n{}",
            "High-Value Feature-Gate Candidates:".bold().yellow()
        );
        println!("  These utility crates are widely used — making them optional features can reduce CI compile times:");
        for c in utility.iter().take(3) {
            println!(
                "    - {} (fan-in: {}, fan-out: {})",
                c.name.bold().cyan(),
                c.fan_in,
                c.fan_out
            );
        }
    }
}

fn estimate_crate_cost(name: &str, dep_count: usize) -> f64 {
    let base = match name {
        "syn" => 14.5,
        "tokio" => 12.0,
        "regex" => 8.5,
        "serde" => 4.2,
        "clap" => 7.5,
        "hyper" => 9.5,
        "reqwest" => 11.0,
        "rand" => 3.8,
        "serde_json" => 3.2,
        "chrono" => 4.5,
        "axum" => 8.0,
        "diesel" => 13.0,
        "sqlx" => 15.0,
        _ => 1.0 + (dep_count as f64 * 0.3),
    };
    base + (dep_count as f64 * 0.15)
}

fn extract_edges(nodes: &[cargo_metadata::Node]) -> Vec<Edge> {
    let mut edges = Vec::new();
    for node in nodes {
        let from_name = extract_pkg_name(&node.id);
        for dep in &node.deps {
            let to_name = extract_pkg_name(&dep.pkg);
            edges.push(Edge {
                from: from_name.clone(),
                to: to_name,
                cost: 0.5,
            });
        }
    }
    edges
}

fn extract_pkg_name(id: &PackageId) -> String {
    extract_pkg_name_from_str(&id.repr)
}

fn find_critical_path(edges: &[Edge], crates: &[CrateNode]) -> Option<Vec<String>> {
    let crate_names: HashSet<&str> = crates.iter().map(|c| c.name.as_str()).collect();
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut in_degree: HashMap<&str, usize> = HashMap::new();

    for c in &crate_names {
        in_degree.entry(c).or_insert(0);
        adj.entry(c).or_default();
    }

    for edge in edges {
        if crate_names.contains(edge.from.as_str()) && crate_names.contains(edge.to.as_str()) {
            adj.entry(&edge.from).or_default().push(&edge.to);
            *in_degree.entry(&edge.to).or_insert(0) += 1;
        }
    }

    let mut dist: HashMap<&str, f64> = HashMap::new();
    let mut parent: HashMap<&str, Option<&str>> = HashMap::new();

    let mut queue: VecDeque<&str> = VecDeque::new();
    for (&name, &deg) in &in_degree {
        if deg == 0 {
            queue.push_back(name);
            dist.insert(name, 0.0);
            parent.insert(name, None);
        }
    }

    while let Some(node) = queue.pop_front() {
        let node_dist = *dist.get(node).unwrap_or(&0.0);
        if let Some(neighbors) = adj.get(node) {
            for &next in neighbors {
                let edge_cost = edges
                    .iter()
                    .find(|e| e.from == node && e.to == next)
                    .map(|e| e.cost)
                    .unwrap_or(0.5);

                let crate_cost = crates
                    .iter()
                    .find(|c| c.name == next)
                    .map(|c| c.estimated_time)
                    .unwrap_or(1.0);

                let new_dist = node_dist + edge_cost + crate_cost;
                let entry = dist.entry(next).or_insert(0.0);
                if new_dist > *entry {
                    *entry = new_dist;
                    parent.insert(next, Some(node));
                }

                if let Some(deg) = in_degree.get_mut(next) {
                    *deg = deg.saturating_sub(1);
                    if *deg == 0 {
                        queue.push_back(next);
                    }
                }
            }
        }
    }

    let (end_node, _) = dist
        .iter()
        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))?;

    let mut path = Vec::new();
    let mut current = Some(*end_node);
    while let Some(node) = current {
        path.push(node.to_string());
        current = parent.get(node).copied().flatten();
    }
    path.reverse();

    if path.len() > 1 {
        Some(path)
    } else {
        None
    }
}

fn extract_pkg_name_from_str(s: &str) -> String {
    if let Some(name_end) = s.find(' ') {
        s[..name_end].to_string()
    } else {
        s.to_string()
    }
}

// ── Cross-Crate Split Suggestions ──

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SplitSuggestion {
    pub crate_name: String,
    pub loc: usize,
    pub dep_count: usize,
    pub estimated_compile_secs: f64,
    pub proposed_modules: Vec<String>,
    pub estimated_savings_pct: f64,
    pub reason: String,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct SplitReport {
    pub suggestions: Vec<SplitSuggestion>,
}

fn find_workspace_member_dirs(metadata: &cargo_metadata::Metadata) -> Vec<(&str, &Path)> {
    metadata
        .packages
        .iter()
        .filter(|p| metadata.workspace_members.contains(&p.id))
        .filter_map(|p| {
            let manifest = Path::new(&p.manifest_path);
            let dir = manifest.parent()?;
            Some((p.name.as_str(), dir))
        })
        .collect()
}

fn suggest_crate_splits(
    metadata: &cargo_metadata::Metadata,
    edges: &[Edge],
    _root: &Path,
) -> Result<SplitReport> {
    let workspace_names: HashSet<&str> = metadata
        .packages
        .iter()
        .filter(|p| metadata.workspace_members.contains(&p.id))
        .map(|p| p.name.as_str())
        .collect();

    let member_dirs = find_workspace_member_dirs(metadata);
    let mut suggestions = Vec::new();

    for (name, dir) in &member_dirs {
        let loc = crate::workspace::count_rust_loc(dir).unwrap_or(0);
        let package = metadata
            .packages
            .iter()
            .find(|p| p.name.as_str() == *name)
            .unwrap();

        let dep_count = package.dependencies.len();
        let external_dep_count = package
            .dependencies
            .iter()
            .filter(|d| !workspace_names.contains(d.name.as_str()))
            .count();

        // Estimate compile time: 1.5s baseline + 0.005s/LoC + 0.2s/dep
        let estimated = 1.5 + (loc as f64 * 0.005) + (dep_count as f64 * 0.2);

        // Determine if this crate is a split candidate:
        //  - "wide": >1000 LoC with low external-dep density (<1 ext dep per 200 lines)
        //  - Or: fan_in > 3 (many dependents) — good utility extraction candidate
        //  - Or: simply >3000 LoC (always worth considering a split)
        let fan_in = edges
            .iter()
            .filter(|e| e.to == *name && workspace_names.contains(e.from.as_str()))
            .count();
        let fan_out = edges
            .iter()
            .filter(|e| e.from == *name && workspace_names.contains(e.to.as_str()))
            .count();

        let external_dep_ratio = if loc > 0 {
            external_dep_count as f64 / loc as f64 * 1000.0
        } else {
            0.0
        };

        let is_large = loc > 3000;
        let is_wide = loc > 1000 && external_dep_ratio < 5.0;
        let is_utility_hub = fan_in > 3 && loc > 800;

        if !is_large && !is_wide && !is_utility_hub {
            continue;
        }

        let proposed_modules = propose_modules(name, loc, fan_in, fan_out, external_dep_count);
        let savings = if loc > 3000 {
            30.0
        } else if is_wide {
            // Wide crates with few external deps benefit from splitting into domain modules
            20.0
        } else {
            // Utility crates benefit from being extracted into a shared crate
            15.0
        };

        let reason = if is_large {
            format!(
                "Large crate ({} LoC) — splitting into {} would reduce per-change rebuild scope",
                loc,
                proposed_modules.join(", ")
            )
        } else if is_wide {
            format!("Wide crate ({} LoC, {} external deps) — low external dependency density ({:.1}/kLoC) suggests self-contained logic that can be modularized", loc, external_dep_count, external_dep_ratio)
        } else {
            format!("Utility hub (fan-in: {}, {} LoC) — extracting shared types/logic reduces recompilation of dependents", fan_in, loc)
        };

        suggestions.push(SplitSuggestion {
            crate_name: name.to_string(),
            loc,
            dep_count,
            estimated_compile_secs: estimated,
            proposed_modules,
            estimated_savings_pct: savings,
            reason,
        });
    }

    suggestions.sort_by(|a, b| {
        b.estimated_savings_pct
            .partial_cmp(&a.estimated_savings_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(SplitReport { suggestions })
}

fn propose_modules(
    name: &str,
    _loc: usize,
    fan_in: usize,
    fan_out: usize,
    _external_dep_count: usize,
) -> Vec<String> {
    let mut modules = Vec::new();

    if fan_in > 3 {
        modules.push(format!("{}-core", name));
        modules.push(format!("{}-types", name));
    }
    if fan_out > 10 {
        modules.push(format!("{}-utils", name));
    }
    if fan_in <= 3 && fan_out <= 10 {
        modules.push(format!("{}-core", name));
        modules.push(format!("{}-macros", name));
    }

    if modules.is_empty() {
        modules.push(format!("{}-core", name));
    }

    modules
}

fn save_split_report(root: &Path, report: &SplitReport) -> Result<()> {
    let dir = root.join(".cargo-accelerate");
    fs::create_dir_all(&dir)?;
    let path = dir.join("splits.toml");

    let toml_str = toml::to_string(report)?;
    fs::write(&path, toml_str)?;
    println!(
        "  {} Split suggestions saved to {}",
        "✔".green(),
        path.display()
    );
    Ok(())
}

fn print_split_suggestions(report: &SplitReport) {
    if report.suggestions.is_empty() {
        println!(
            "\n{} No crates identified as split candidates.",
            "✔".green()
        );
        return;
    }

    println!(
        "\n{}",
        "Cross-Crate Optimization Suggestions:".bold().yellow()
    );
    println!("  The following workspace members may benefit from splitting:");

    for s in &report.suggestions {
        println!();
        println!(
            "  {:<25} {:>6} LoC, {} deps, ~{:.1}s compile",
            s.crate_name.bold().red(),
            s.loc,
            s.dep_count,
            s.estimated_compile_secs
        );
        println!("  ├ Reason: {}", s.reason);
        println!(
            "  ├ Proposed: split into {}",
            s.proposed_modules.join(", ").cyan()
        );
        println!(
            "  └ Estimated savings: ~{:.0}% on rebuilds of affected modules",
            s.estimated_savings_pct
        );
        println!("    → See .cargo-accelerate/splits.toml for suggested Cargo.toml changes");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_estimate_crate_cost_known() {
        assert!(estimate_crate_cost("syn", 3) > 10.0);
    }

    #[test]
    fn test_estimate_crate_cost_unknown() {
        let cost = estimate_crate_cost("my-crate", 5);
        assert!(cost > 0.0);
    }

    #[test]
    fn test_extract_pkg_name_from_str_with_version() {
        assert_eq!(
            extract_pkg_name_from_str("syn 2.0.0 (path+file:///tmp)"),
            "syn"
        );
    }

    #[test]
    fn test_extract_pkg_name_from_str_plain() {
        assert_eq!(extract_pkg_name_from_str("my-crate"), "my-crate");
    }

    #[test]
    fn test_find_critical_path_empty() {
        let edges = vec![];
        let crates = vec![];
        assert!(find_critical_path(&edges, &crates).is_none());
    }

    #[test]
    fn test_find_critical_path_single() {
        let edges = vec![];
        let crates = vec![CrateNode {
            name: "a".into(),
            loc: 0,
            dep_count: 0,
            estimated_time: 1.0,
            depth: 0,
            fan_in: 0,
            fan_out: 0,
        }];
        assert!(find_critical_path(&edges, &crates).is_none());
    }

    #[test]
    fn test_update_fan_metrics_no_edges() {
        let mut crates = vec![CrateNode {
            name: "a".into(),
            loc: 0,
            dep_count: 0,
            estimated_time: 1.0,
            depth: 0,
            fan_in: 0,
            fan_out: 0,
        }];
        update_fan_metrics(&mut crates, &[]);
        assert_eq!(crates[0].fan_in, 0);
        assert_eq!(crates[0].fan_out, 0);
    }

    #[test]
    fn test_update_fan_metrics_with_edges() {
        let mut crates = vec![
            CrateNode {
                name: "a".into(),
                loc: 0,
                dep_count: 1,
                estimated_time: 1.0,
                depth: 0,
                fan_in: 0,
                fan_out: 0,
            },
            CrateNode {
                name: "b".into(),
                loc: 0,
                dep_count: 0,
                estimated_time: 1.0,
                depth: 0,
                fan_in: 0,
                fan_out: 0,
            },
        ];
        let edges = vec![Edge {
            from: "a".into(),
            to: "b".into(),
            cost: 0.5,
        }];
        update_fan_metrics(&mut crates, &edges);
        assert_eq!(crates[0].fan_in, 0);
        assert_eq!(crates[0].fan_out, 1);
        assert_eq!(crates[1].fan_in, 1);
        assert_eq!(crates[1].fan_out, 0);
    }

    #[test]
    fn test_estimate_crate_cost_zero_deps() {
        let cost = estimate_crate_cost("my-crate", 0);
        assert!((cost - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_propose_modules_utility_hub() {
        let modules = propose_modules("serde-ext", 2000, 6, 3, 2);
        assert!(modules.contains(&"serde-ext-core".to_string()));
        assert!(modules.contains(&"serde-ext-types".to_string()));
    }

    #[test]
    fn test_propose_modules_large_fan_out() {
        let modules = propose_modules("utils", 5000, 2, 15, 1);
        assert!(modules.contains(&"utils-utils".to_string()));
    }

    #[test]
    fn test_propose_modules_fallback() {
        let modules = propose_modules("tiny-crate", 500, 0, 0, 0);
        assert!(modules.contains(&"tiny-crate-core".to_string()));
    }

    #[test]
    fn test_split_suggestion_ordering() {
        let mut report = SplitReport {
            suggestions: vec![
                SplitSuggestion {
                    crate_name: "A".into(),
                    loc: 5000,
                    dep_count: 3,
                    estimated_compile_secs: 30.0,
                    proposed_modules: vec!["a-core".into()],
                    estimated_savings_pct: 30.0,
                    reason: "Large crate".into(),
                },
                SplitSuggestion {
                    crate_name: "B".into(),
                    loc: 1500,
                    dep_count: 1,
                    estimated_compile_secs: 10.0,
                    proposed_modules: vec!["b-core".into()],
                    estimated_savings_pct: 20.0,
                    reason: "Wide crate".into(),
                },
            ],
        };
        report.suggestions.sort_by(|a, b| {
            b.estimated_savings_pct
                .partial_cmp(&a.estimated_savings_pct)
                .unwrap()
        });
        assert_eq!(report.suggestions[0].crate_name, "A");
        assert_eq!(report.suggestions[1].crate_name, "B");
    }

    #[test]
    fn test_split_report_serde_roundtrip() {
        let report = SplitReport {
            suggestions: vec![SplitSuggestion {
                crate_name: "foo".into(),
                loc: 2000,
                dep_count: 2,
                estimated_compile_secs: 15.0,
                proposed_modules: vec!["foo-core".into(), "foo-macros".into()],
                estimated_savings_pct: 25.0,
                reason: "Wide crate".into(),
            }],
        };
        let toml_str = toml::to_string(&report).unwrap();
        let deserialized: SplitReport = toml::from_str(&toml_str).unwrap();
        assert_eq!(deserialized.suggestions.len(), 1);
        assert_eq!(deserialized.suggestions[0].crate_name, "foo");
        assert_eq!(
            deserialized.suggestions[0].proposed_modules,
            vec!["foo-core", "foo-macros"]
        );
        assert!((deserialized.suggestions[0].estimated_savings_pct - 25.0).abs() < 1e-9);
    }

    #[test]
    fn test_extract_pkg_name_from_str_with_space() {
        assert_eq!(extract_pkg_name_from_str("foo 0.1.0 (path+..."), "foo");
        assert_eq!(extract_pkg_name_from_str("bar"), "bar");
    }
}
