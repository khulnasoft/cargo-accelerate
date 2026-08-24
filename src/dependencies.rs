use crate::utils::get_cached_metadata_with_deps;
use anyhow::{Context, Result};
use cargo_metadata::{NodeDep, PackageId};
use colored::*;
use std::collections::{HashMap, HashSet};

struct DepMetrics {
    name: String,
    version: String,
    transitive_count: usize,
    estimated_time: f64,
}

pub fn run() -> Result<()> {
    println!(
        "{}",
        "Analyzing Dependency Impact on Compilation Times..."
            .bold()
            .cyan()
    );
    println!("  Retrieving complete dependency graph (including transitives)...");

    let metadata = get_cached_metadata_with_deps()?;

    // Map resolve node links to identify transitive chains
    let resolve = metadata
        .resolve
        .as_ref()
        .context("No dependency resolve graph found")?;
    let mut adjacency_list = HashMap::new();
    for node in &resolve.nodes {
        adjacency_list.insert(&node.id, &node.deps);
    }

    let mut metrics = Vec::new();

    // Analyze only external (non-workspace) crates
    for package in &metadata.packages {
        // Skip workspace members since we have cargo accelerate workspace for them
        if metadata.workspace_members.contains(&package.id) {
            continue;
        }

        let name = &package.name;

        let mut transitives = HashSet::new();
        get_transitive_deps(&package.id, &adjacency_list, &mut transitives);
        let transitive_count = transitives.len().saturating_sub(1);

        // Total estimated compile time takes into account the transitives compile cost too
        let estimated_time =
            base_weight_estimate(name, package.features.len()) + (transitives.len() as f64 * 0.15);

        metrics.push(DepMetrics {
            name: name.to_string(),
            version: package.version.to_string(),
            transitive_count,
            estimated_time,
        });
    }

    // Sort by estimated compile time descending
    metrics.sort_by(|a, b| b.estimated_time.partial_cmp(&a.estimated_time).unwrap());

    // Print summary
    println!(
        "\n{:<25} {:<10} {:<18} {:<15}",
        "Dependency Name".bold(),
        "Version".bold(),
        "Transitive Deps".bold(),
        "Est. Compile Time".bold()
    );
    println!(
        "{}",
        "-------------------------------------------------------------------------".cyan()
    );

    let max_display = std::cmp::min(20, metrics.len());
    for m in metrics.iter().take(max_display) {
        let name_colored = if m.estimated_time > 8.0 {
            m.name.red()
        } else if m.estimated_time > 3.0 {
            m.name.yellow()
        } else {
            m.name.normal()
        };

        println!(
            "{:<25} {:<10} {:<18} {:.1}s",
            name_colored,
            m.version.dimmed(),
            m.transitive_count.to_string().cyan(),
            m.estimated_time
        );
    }

    if metrics.len() > max_display {
        println!("  ... and {} other packages", metrics.len() - max_display);
    }

    // Print key insights
    println!("\n{}", "Key Dependency Insights:".bold().yellow());
    if let Some(top) = metrics.first() {
        println!(
            "  - {} is your heaviest dependency (approx. {:.1}s compilation impact).",
            top.name.bold().red(),
            top.estimated_time
        );
        if top.name == "syn" {
            println!("    {} If possible, verify if features like 'full' or 'derive' can be disabled to speed up 'syn' compilation.", "Tip:".green());
        }
    }

    Ok(())
}

fn get_transitive_deps<'a>(
    node_id: &'a PackageId,
    adjacency_list: &HashMap<&'a PackageId, &'a Vec<NodeDep>>,
    visited: &mut HashSet<&'a PackageId>,
) {
    if visited.contains(node_id) {
        return;
    }
    visited.insert(node_id);

    if let Some(deps) = adjacency_list.get(node_id) {
        for dep in *deps {
            get_transitive_deps(&dep.pkg, adjacency_list, visited);
        }
    }
}

/// Estimate compile footprint (seconds) based on crate complexity and known
/// heavyweights; unknown crates fall back to a feature-count heuristic.
fn base_weight_estimate(name: &str, features_count: usize) -> f64 {
    match name {
        "syn" => 14.5,
        "tokio" => 12.0,
        "regex" => 8.5,
        "serde" => 4.2,
        "serde_derive" => 5.8,
        "clap" => 7.5,
        "clap_builder" => 6.2,
        "hyper" => 9.5,
        "reqwest" => 11.0,
        "rand" => 3.8,
        "serde_json" => 3.2,
        "chrono" => 4.5,
        "axum" => 8.0,
        "diesel" => 13.0,
        "sqlx" => 15.0,
        // Heuristic for unknown crates
        _ => 0.8 + (features_count as f64 * 0.1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cargo_metadata::PackageId;

    fn pid(s: &str) -> PackageId {
        PackageId {
            repr: s.to_string(),
        }
    }

    fn dep(name: &str) -> NodeDep {
        // NodeDep is #[non_exhaustive], so build it via deserialization.
        serde_json::from_value(serde_json::json!({ "name": name, "pkg": name })).unwrap()
    }

    /// Build an adjacency list from `(node, [deps])` tuples.
    fn graph(edges: &[(&str, Vec<&str>)]) -> HashMap<PackageId, Vec<NodeDep>> {
        let mut map = HashMap::new();
        for (name, deps) in edges {
            let nodes = deps.iter().map(|d| dep(d)).collect::<Vec<_>>();
            map.insert(pid(name), nodes);
        }
        map
    }

    fn collect(graph: &HashMap<PackageId, Vec<NodeDep>>, start: &str) -> HashSet<String> {
        let adjacency: HashMap<&PackageId, &Vec<NodeDep>> = graph.iter().collect();
        let mut visited = HashSet::new();
        let start = pid(start);
        get_transitive_deps(&start, &adjacency, &mut visited);
        visited.into_iter().map(|p| p.repr.clone()).collect()
    }

    #[test]
    fn test_transitive_chain() {
        // a -> b -> c
        let g = graph(&[("a", vec!["b"]), ("b", vec!["c"]), ("c", vec![])]);
        let seen = collect(&g, "a");
        assert_eq!(seen.len(), 3);
        assert!(seen.contains("a") && seen.contains("b") && seen.contains("c"));
    }

    #[test]
    fn test_transitive_diamond_deduplicates() {
        // a -> b, a -> c, b -> d, c -> d : d must be counted once
        let g = graph(&[
            ("a", vec!["b", "c"]),
            ("b", vec!["d"]),
            ("c", vec!["d"]),
            ("d", vec![]),
        ]);
        assert_eq!(collect(&g, "a").len(), 4);
    }

    #[test]
    fn test_transitive_cycle_terminates() {
        // a <-> b cycle plus c hanging off b
        let g = graph(&[("a", vec!["b"]), ("b", vec!["a", "c"]), ("c", vec![])]);
        assert_eq!(collect(&g, "a").len(), 3);
    }

    #[test]
    fn test_transitive_self_loop_and_leaf() {
        let g = graph(&[("solo", vec![]), ("selfy", vec!["selfy"])]);
        assert_eq!(collect(&g, "solo").len(), 1);
        assert_eq!(collect(&g, "selfy").len(), 1);
    }

    #[test]
    fn test_transitive_missing_node_is_empty() {
        let g = graph(&[("a", vec![])]);
        assert_eq!(collect(&g, "not-in-graph").len(), 1);
    }

    #[test]
    fn test_base_weight_known_heavyweights() {
        assert_eq!(base_weight_estimate("syn", 0), 14.5);
        assert_eq!(base_weight_estimate("sqlx", 99), 15.0);
        assert_eq!(base_weight_estimate("tokio", 0), 12.0);
        assert_eq!(base_weight_estimate("serde_json", 5), 3.2);
    }

    #[test]
    fn test_base_weight_unknown_uses_feature_heuristic() {
        assert_eq!(base_weight_estimate("obscure-crate", 0), 0.8);
        assert_eq!(base_weight_estimate("obscure-crate", 10), 1.8);
    }
}
