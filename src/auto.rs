use crate::utils::get_project_root;
use anyhow::{Context, Result};
use colored::*;
use std::process::Command;

#[derive(Default)]
pub struct AutoConfig {
    pub skip_cache: bool,
    pub skip_linker: bool,
    pub skip_profile: bool,
    pub skip_ci: bool,
    pub skip_policy: bool,
    pub apply: bool,
    pub non_interactive: bool,
}

pub fn run(config: AutoConfig) -> Result<()> {
    println!(
        "{}",
        "╔══════════════════════════════════════════╗".cyan().bold()
    );
    println!(
        "{}",
        "║   Cargo Accelerate — Guided Automation  ║".cyan().bold()
    );
    println!(
        "{}",
        "╚══════════════════════════════════════════╝".cyan().bold()
    );
    println!();

    let _root = get_project_root().context("Could not find project root")?;

    enum Step {
        Doctor,
        Cache,
        Linker,
        Profile,
        Ci,
        Policy,
    }

    let steps = [
        (Step::Doctor, "Doctor — System Health Check"),
        (Step::Cache, "Cache — sccache Configuration"),
        (Step::Linker, "Linker — Fast Linker Setup"),
        (Step::Profile, "Profile — Scenario Tuning"),
        (Step::Ci, "CI — Workflow Generation"),
        (Step::Policy, "Policy — Build Enforcement"),
    ];

    for (i, (step_kind, name)) in steps.iter().enumerate() {
        let step_num = i + 1;

        let skipped = match step_kind {
            Step::Cache if config.skip_cache => true,
            Step::Linker if config.skip_linker => true,
            Step::Profile if config.skip_profile => true,
            Step::Ci if config.skip_ci => true,
            Step::Policy if config.skip_policy => true,
            _ => false,
        };

        if skipped {
            println!(
                "{} {}. {} {}",
                "⏭".yellow().bold(),
                step_num,
                name,
                "(skipped)".dimmed()
            );
            continue;
        }

        println!("{} {}. {}", "▶".cyan().bold(), step_num, name.cyan().bold());

        if !config.non_interactive && !config.apply {
            let proceed = prompt_user("Proceed with this step?", true);
            if !proceed {
                println!("  {} Skipped.\n", "⏭".yellow());
                continue;
            }
        }

        let result = match step_kind {
            Step::Doctor => run_doctor_phase(),
            Step::Cache => run_cache_phase(&config),
            Step::Linker => run_linker_phase(&config),
            Step::Profile => run_profile_phase(&config),
            Step::Ci => run_ci_phase(&config),
            Step::Policy => run_policy_phase(&config),
        };

        match result {
            Ok(()) => println!("  {} Done.\n", "✔".green()),
            Err(e) => {
                println!("  {} Failed: {}\n", "✖".red().bold(), e);
                if !config.non_interactive {
                    let cont = prompt_user("Continue with remaining steps?", true);
                    if !cont {
                        println!("Aborting automation.");
                        return Ok(());
                    }
                }
            }
        }
    }

    println!("{}", "Automation complete!".bold().green());
    println!("Run `cargo accelerate test` to verify the setup.");
    Ok(())
}

fn prompt_user(prompt: &str, default: bool) -> bool {
    let default_str = if default { "Y/n" } else { "y/N" };
    print!("    {} [{}]: ", prompt, default_str);
    use std::io::Write;
    std::io::stdout().flush().ok();

    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();

    let input = input.trim().to_lowercase();
    match input.as_str() {
        "y" | "yes" => true,
        "n" | "no" => false,
        "" => default,
        _ => default,
    }
}

fn run_doctor_phase() -> Result<()> {
    let status = Command::new("cargo")
        .args(["accelerate", "doctor"])
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .context("Doctor phase failed")?;

    if !status.success() {
        anyhow::bail!("Doctor found issues");
    }
    Ok(())
}

fn run_cache_phase(config: &AutoConfig) -> Result<()> {
    if config.apply {
        let status = Command::new("cargo")
            .args(["accelerate", "cache", "enable"])
            .current_dir(get_project_root()?)
            .status()
            .context("Cache enable command failed")?;

        if !status.success() {
            anyhow::bail!("Cache enable failed");
        }
    } else {
        println!("  Checking sccache status...");
        if !crate::utils::is_tool_installed("sccache") {
            println!("  sccache not installed. Would install: `cargo install sccache`");
        } else {
            println!("  sccache is installed.");
        }
        println!("  Would run: `cargo accelerate cache enable`");
    }
    Ok(())
}

fn run_linker_phase(config: &AutoConfig) -> Result<()> {
    if config.apply {
        let status = Command::new("cargo")
            .args(["accelerate", "linker"])
            .current_dir(get_project_root()?)
            .status()
            .context("Linker command failed")?;

        if !status.success() {
            anyhow::bail!("Linker setup failed");
        }
    } else {
        println!("  Detecting available linkers...");
        for linker in &["mold", "lld", "lld-link"] {
            if crate::utils::is_tool_installed(linker) {
                println!("  {} found — would configure.", linker);
            }
        }
        println!("  Would run: `cargo accelerate linker`");
    }
    Ok(())
}

fn run_profile_phase(config: &AutoConfig) -> Result<()> {
    if config.apply {
        let status = Command::new("cargo")
            .args(["accelerate", "profile", "--apply", "release"])
            .current_dir(get_project_root()?)
            .status()
            .context("Profile command failed")?;

        if !status.success() {
            anyhow::bail!("Profile setup failed");
        }
    } else {
        println!("  Would generate optimized profile settings (dev, release, CI).");
        println!("  Would run: `cargo accelerate profile --apply release`");
    }
    Ok(())
}

fn run_ci_phase(config: &AutoConfig) -> Result<()> {
    if config.apply {
        let status = Command::new("cargo")
            .args(["accelerate", "ci"])
            .current_dir(get_project_root()?)
            .status()
            .context("CI command failed")?;

        if !status.success() {
            anyhow::bail!("CI workflow generation failed");
        }
    } else {
        println!("  Would generate CI workflow with sccache, mold/lld, and parallel jobs.");
        println!("  Would run: `cargo accelerate ci`");
    }
    Ok(())
}

fn run_policy_phase(config: &AutoConfig) -> Result<()> {
    if config.apply {
        let status = Command::new("cargo")
            .args(["accelerate", "policy"])
            .current_dir(get_project_root()?)
            .status()
            .context("Policy command failed")?;

        if !status.success() {
            anyhow::bail!("Policy setup failed");
        }
    } else {
        println!("  Would create .cargo-accelerate/policy.toml with recommended build settings.");
        println!("  Would run: `cargo accelerate policy`");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_config_default() {
        let config = AutoConfig::default();
        assert!(!config.skip_cache);
        assert!(!config.skip_linker);
        assert!(!config.skip_profile);
        assert!(!config.skip_ci);
        assert!(!config.skip_policy);
        assert!(!config.apply);
        assert!(!config.non_interactive);
    }

    #[test]
    fn test_auto_config_can_skip_all() {
        let config = AutoConfig {
            skip_cache: true,
            skip_linker: true,
            skip_profile: true,
            skip_ci: true,
            skip_policy: true,
            apply: true,
            non_interactive: true,
        };
        // Non-interactive targets don't need prompts
        assert!(config.skip_cache);
        assert!(config.apply);
    }

    #[test]
    fn test_prompt_default_yes() {
        assert!(prompt_user("test", true));
    }

    #[test]
    fn test_prompt_default_no() {
        assert!(!prompt_user("test", false));
    }
}
