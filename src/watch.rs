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
        .args([
            "watch",
            "-q",
            "-c",
            "-s",
            "cargo check && cargo test && cargo clippy",
        ])
        .spawn()
        .context("Failed to run cargo watch")?
        .wait()?;

    if !status.success() {
        anyhow::bail!("cargo watch exited with non-zero status");
    }

    Ok(())
}
