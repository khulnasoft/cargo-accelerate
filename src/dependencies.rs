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

        // Estimate compile footprint based on crate complexity and common crates
        let base_weight = match name.as_str() {
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
            _ => {
                // Heuristic for unknown crates
                let features_count = package.features.len() as f64;
                0.8 + (features_count * 0.1)
            }
        };

        // Total estimated compile time takes into account the transitives compile cost too
        let estimated_time = base_weight + (transitives.len() as f64 * 0.15);

        metrics.push(DepMetrics {
            name: name.clone(),
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
    for i in 0..max_display {
        let m = &metrics[i];
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
