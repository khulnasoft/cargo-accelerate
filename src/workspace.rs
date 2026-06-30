use crate::utils::get_cached_metadata;
use anyhow::Result;
use colored::*;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use walkdir::WalkDir;

struct CrateMetrics {
    name: String,
    loc: usize,
    dep_count: usize,
    estimated_time: f64,
}

pub fn run() -> Result<()> {
    println!("{}", "Analyzing Workspace Structure...".bold().cyan());

    let metadata = get_cached_metadata()?;

    let members = &metadata.workspace_members;
    let total_crates = members.len();

    println!(
        "  Workspace contains {} crate(s):\n",
        total_crates.to_string().green()
    );

    let mut crate_metrics = Vec::new();

    for member in members {
        // Find the package corresponding to this member ID
        if let Some(package) = metadata.packages.iter().find(|p| p.id == *member) {
            let manifest_path = Path::new(&package.manifest_path);
            let crate_dir = manifest_path.parent().ok_or_else(|| {
                anyhow::anyhow!("Invalid manifest path: {}", package.manifest_path)
            })?;

            // Calculate lines of Rust code in this crate
            let loc = count_rust_loc(crate_dir)?;
            let dep_count = package.dependencies.len();

            // Estimate compile weight: 1.5s baseline + 0.005s per line of code + 0.2s per direct dependency
            let estimated_time = 1.5 + (loc as f64 * 0.005) + (dep_count as f64 * 0.2);

            crate_metrics.push(CrateMetrics {
                name: package.name.clone(),
                loc,
                dep_count,
                estimated_time,
            });
        }
    }

    // Sort by estimated compile time descending
    crate_metrics.sort_by(|a, b| b.estimated_time.partial_cmp(&a.estimated_time).unwrap());

    // Print summary table
    println!(
        "{:<25} {:<12} {:<15} {:<15}",
        "Crate Name".bold(),
        "Lines of Code".bold(),
        "Direct Deps".bold(),
        "Est. Compile Time".bold()
    );
    println!(
        "{}",
        "----------------------------------------------------------------------".cyan()
    );

    for m in &crate_metrics {
        let name_colored = if m.estimated_time > 15.0 {
            m.name.red()
        } else if m.estimated_time > 5.0 {
            m.name.yellow()
        } else {
            m.name.normal()
        };

        println!(
            "{:<25} {:<12} {:<15} {:.2}s",
            name_colored,
            m.loc.to_string().cyan(),
            m.dep_count.to_string().cyan(),
            m.estimated_time
        );
    }

    // Print analysis & suggestions
    if total_crates > 1 {
        println!("\n{}", "Slowest Bottleneck Crates:".bold().yellow());
        let slowest_count = std::cmp::min(3, crate_metrics.len());
        for i in 0..slowest_count {
            let m = &crate_metrics[i];
            println!(
                "  {}. {} (approx. {:.1}s, {} lines of code)",
                i + 1,
                m.name.bold().red(),
                m.estimated_time,
                m.loc
            );
        }

        println!("\n{}", "AI Architectural Suggestions:".bold().cyan());
        if let Some(slowest) = crate_metrics.first() {
            if slowest.loc > 5000 {
                println!(
                    "  - Crate '{}' is quite large ({} lines).",
                    slowest.name, slowest.loc
                );
                println!("    {} Splitting it into smaller sub-crates (e.g., '{}-core' and '{}-types') could reduce rebuild times by up to {}%!", 
                    "Suggestion:".green(), slowest.name, slowest.name, 25);
            } else {
                println!("  - Your crate sizes are well-balanced.");
            }
        }
    }

    Ok(())
}

fn count_rust_loc(dir: &Path) -> Result<usize> {
    let mut total_lines = 0;
    for entry in WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| {
            e.file_name() != "target" && e.file_name() != ".git"
        })
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() && path.extension().map(|s| s == "rs").unwrap_or(false) {
            let file = fs::File::open(path)?;
            let reader = BufReader::new(file);
            total_lines += reader.lines().count();
        }
    }
    Ok(total_lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_rs_file(dir: &TempDir, path: &str, content: &str) {
        let full = dir.path().join(path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(full, content).unwrap();
    }

    #[test]
    fn test_count_rust_loc_single_file() {
        let dir = TempDir::new().unwrap();
        create_rs_file(&dir, "src/lib.rs", "line1\nline2\nline3\n");
        assert_eq!(count_rust_loc(dir.path()).unwrap(), 3);
    }

    #[test]
    fn test_count_rust_loc_multiple_files() {
        let dir = TempDir::new().unwrap();
        create_rs_file(&dir, "src/lib.rs", "a\nb\n");
        create_rs_file(&dir, "src/main.rs", "x\ny\nz\n");
        assert_eq!(count_rust_loc(dir.path()).unwrap(), 5);
    }

    #[test]
    fn test_count_rust_loc_ignores_target() {
        let dir = TempDir::new().unwrap();
        create_rs_file(&dir, "src/lib.rs", "keep\n");
        create_rs_file(&dir, "target/debug/build/proc.rs", "ignore\nthis\n");
        assert_eq!(count_rust_loc(dir.path()).unwrap(), 1);
    }

    #[test]
    fn test_count_rust_loc_ignores_git() {
        let dir = TempDir::new().unwrap();
        create_rs_file(&dir, "src/lib.rs", "real\ncode\n");
        create_rs_file(&dir, ".git/hooks/pre-commit.rs", "hook\n");
        assert_eq!(count_rust_loc(dir.path()).unwrap(), 2);
    }

    #[test]
    fn test_count_rust_loc_empty_dir() {
        let dir = TempDir::new().unwrap();
        assert_eq!(count_rust_loc(dir.path()).unwrap(), 0);
    }

    #[test]
    fn test_count_rust_loc_non_rs_files_ignored() {
        let dir = TempDir::new().unwrap();
        create_rs_file(&dir, "src/lib.rs", "rust\nonly\n");
        fs::write(dir.path().join("README.md"), "not\nrust\n").unwrap();
        assert_eq!(count_rust_loc(dir.path()).unwrap(), 2);
    }

    #[test]
    fn test_count_rust_loc_subdirectories() {
        let dir = TempDir::new().unwrap();
        create_rs_file(&dir, "src/lib.rs", "1\n2\n");
        create_rs_file(&dir, "src/foo/bar.rs", "3\n4\n5\n");
        create_rs_file(&dir, "tests/integration.rs", "6\n");
        assert_eq!(count_rust_loc(dir.path()).unwrap(), 6);
    }
}
