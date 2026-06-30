use crate::utils::get_cached_metadata_with_deps;
use anyhow::{Context, Result};
use cargo_metadata::PackageId;
use colored::*;
use std::collections::{HashMap, HashSet, VecDeque};

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

    Ok(())
}

fn update_fan_metrics(crates: &mut Vec<CrateNode>, edges: &[Edge]) {
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
    high_own_deps.sort_by(|a, b| b.1.cmp(&a.1));

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
}
