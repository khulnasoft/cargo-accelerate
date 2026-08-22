use crate::utils::{get_os, is_tool_installed};
use anyhow::{Context, Result};
use colored::*;
use std::process::Command;

pub fn run() -> Result<()> {
    println!("{}", "Running Cargo Accelerate Installer...".bold().cyan());

    let os = get_os();
    let preferred_linker = match os {
        "linux" => "mold",
        "macos" => "lld",
        "windows" => "lld-link",
        _ => "lld",
    };

    let tools = vec![
        (
            "cargo-binstall",
            "cargo-binstall",
            "Binary installer for cargo",
        ),
        ("sccache", "sccache", "Compiler cache"),
        (preferred_linker, preferred_linker, "Fast linker"),
        ("cargo-nextest", "cargo-nextest", "Fast test runner"),
        ("cargo-watch", "cargo-watch", "Watch daemon"),
        ("cargo-chef", "cargo-chef", "Docker caching helper"),
    ];

    let mut missing = Vec::new();

    for &(name, bin, desc) in &tools {
        // cargo-nextest binary is actually cargo-nextest
        let bin_name = if bin == "cargo-nextest" {
            "cargo-nextest"
        } else {
            bin
        };
        if is_tool_installed(bin_name) {
            println!("  {} {} is already installed ({})", "✔".green(), name, desc);
        } else {
            println!("  {} {} is missing ({})", "✖".red(), name, desc);
            missing.push((name, bin));
        }
    }

    if missing.is_empty() {
        println!(
            "\n{}",
            "✔ All optimization tools are already installed!"
                .bold()
                .green()
        );
        return Ok(());
    }

    println!("\nInstalling missing tools...");

    // First try to install cargo-binstall if missing, since it makes other installations extremely fast!
    let mut binstall_available = is_tool_installed("cargo-binstall");
    if !binstall_available && missing.iter().any(|&(n, _)| n == "cargo-binstall") {
        println!("Installing `cargo-binstall` to accelerate subsequent installations...");
        if install_tool_via_cargo("cargo-binstall").is_ok() {
            binstall_available = true;
            println!("  {} cargo-binstall installed successfully!", "✔".green());
        }
    }

    for (name, _bin) in missing {
        if name == "cargo-binstall" && binstall_available {
            continue; // Already handled
        }

        println!("\nInstalling {}...", name);

        let mut installed = false;

        // Try OS package managers first for system-level tools like mold or lld
        if name == "mold" || name == "lld" || name == "lld-link" {
            match os {
                "macos" => {
                    if is_tool_installed("brew") {
                        println!("  Attempting installation via Homebrew...");
                        let pkg = if name == "mold" { "mold" } else { "llvm" };
                        installed = run_command("brew", &["install", pkg]).is_ok();
                    }
                }
                "linux" => {
                    if is_tool_installed("apt-get") {
                        println!("  Attempting installation via apt-get...");
                        let pkg = if name == "mold" { "mold" } else { "lld" };
                        installed =
                            run_command("sudo", &["apt-get", "install", "-y", pkg, "clang"])
                                .is_ok();
                    }
                }
                _ => {}
            }
        }

        // Fallback or prefer cargo-binstall for cargo subcommands
        if !installed {
            if binstall_available {
                println!("  Attempting installation via cargo-binstall...");
                installed = run_command("cargo", &["binstall", "-y", name]).is_ok();
            }

            if !installed {
                println!("  Attempting installation via standard cargo install (this may take a few minutes)...");
                installed = install_tool_via_cargo(name).is_ok();
            }
        }

        if installed {
            println!("  {} {} installed successfully!", "✔".green(), name);
        } else {
            println!("  {} Failed to install {}.", "✖".red(), name);
        }
    }

    println!("\n{}", "✔ Installation phase complete!".bold().green());
    Ok(())
}

fn install_tool_via_cargo(name: &str) -> Result<()> {
    let mut args = vec!["install"];
    // cargo-nextest refuses to compile without --locked
    if name == "cargo-nextest" {
        args.push("--locked");
    }
    args.push(name);
    run_command("cargo", &args)
}

fn run_command(cmd: &str, args: &[&str]) -> Result<()> {
    let mut child = Command::new(cmd)
        .args(args)
        .spawn()
        .with_context(|| format!("Failed to start process {} with args {:?}", cmd, args))?;

    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("Command failed with exit code: {:?}", status.code())
    }
}
