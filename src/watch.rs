use crate::utils::is_tool_installed;
use anyhow::{Context, Result};
use colored::*;
use std::process::Command;
use std::time::Instant;

pub fn run() -> Result<()> {
    println!("{}", "Starting Enhanced Watch Mode...".bold().cyan());

    if !is_tool_installed("cargo-watch") {
        println!("{} `cargo-watch` is not installed.", "✖".red());
        println!("  Please install it first by running `cargo accelerate install`.");
        return Ok(());
    }

    // Warm the cache before watching
    println!("  {} Prewarming build cache...", "➤".cyan());
    let warm_start = Instant::now();
    let warmup = Command::new("cargo")
        .args(["check", "--workspace"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    match warmup {
        Ok(s) if s.success() => {
            println!(
                "  {} Cache prewarmed in {:.1}s",
                "✔".green(),
                warm_start.elapsed().as_secs_f32()
            );
        }
        _ => {
            println!(
                "  {} Cache prewarm did not fully complete (continuing)",
                "⚠".yellow()
            );
        }
    }

    println!(
        "  Pipeline configured: {} ➔ {} ➔ {}",
        "cargo check".yellow(),
        "cargo test".blue(),
        "cargo clippy".green()
    );
    println!("  Watching for file changes... (Press Ctrl+C to stop)\n");

    let status = Command::new("cargo")
        .args(pipeline_args())
        .spawn()
        .context("Failed to run cargo watch")?
        .wait()?;

    if !status.success() {
        anyhow::bail!("cargo watch exited with non-zero status");
    }

    Ok(())
}

/// Arguments passed to `cargo watch`: quiet, clear, running the
/// check → test → clippy pipeline on every change.
fn pipeline_args() -> [&'static str; 5] {
    [
        "watch",
        "-q",
        "-c",
        "-s",
        "cargo check && cargo test && cargo clippy",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_args_shape() {
        let args = pipeline_args();
        assert_eq!(args[0], "watch");
        assert!(args.contains(&"-q"));
        assert!(args.contains(&"-c"));
        assert_eq!(
            *args.last().unwrap(),
            "cargo check && cargo test && cargo clippy"
        );
    }
}
