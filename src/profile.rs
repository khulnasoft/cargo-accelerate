use crate::utils::available_cpus;
use anyhow::Result;
use colored::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Scenario {
    Dev,
    Test,
    Ci,
    Release,
}

impl std::fmt::Display for Scenario {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Scenario::Dev => write!(f, "dev"),
            Scenario::Test => write!(f, "test"),
            Scenario::Ci => write!(f, "ci"),
            Scenario::Release => write!(f, "release"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProfileSettings {
    pub incremental: Option<bool>,
    pub codegen_units: Option<i64>,
    pub opt_level: Option<i64>,
    pub debug: Option<i64>,
    pub lto: Option<bool>,
    pub strip: Option<bool>,
    pub overflow_checks: Option<bool>,
}

impl ProfileSettings {
    pub fn for_scenario(scenario: &Scenario) -> Self {
        let cpus = available_cpus();
        match scenario {
            Scenario::Dev => Self {
                incremental: Some(true),
                codegen_units: Some((cpus * 2).min(256) as i64),
                opt_level: Some(0),
                debug: Some(1),
                lto: Some(false),
                strip: Some(false),
                overflow_checks: Some(true),
            },
            Scenario::Test => Self {
                incremental: Some(true),
                codegen_units: Some((cpus * 2).min(256) as i64),
                opt_level: Some(0),
                debug: Some(2),
                lto: Some(false),
                strip: Some(false),
                overflow_checks: Some(true),
            },
            Scenario::Ci => Self {
                incremental: Some(false),
                codegen_units: Some(1),
                opt_level: Some(2),
                debug: Some(1),
                lto: Some(true),
                strip: Some(false),
                overflow_checks: Some(true),
            },
            Scenario::Release => Self {
                incremental: Some(false),
                codegen_units: Some(1),
                opt_level: Some(3),
                debug: Some(0),
                lto: Some(true),
                strip: Some(true),
                overflow_checks: Some(false),
            },
        }
    }

    pub fn to_toml_table(&self) -> toml_edit::Table {
        let mut table = toml_edit::Table::new();
        if let Some(v) = self.incremental {
            table["incremental"] = toml_edit::value(v);
        }
        if let Some(v) = self.codegen_units {
            table["codegen-units"] = toml_edit::value(v);
        }
        if let Some(v) = self.opt_level {
            table["opt-level"] = toml_edit::value(v);
        }
        if let Some(v) = self.debug {
            table["debug"] = toml_edit::value(v);
        }
        if let Some(v) = self.lto {
            table["lto"] = toml_edit::value(v);
        }
        if let Some(v) = self.strip {
            table["strip"] = toml_edit::value(v);
        }
        if let Some(v) = self.overflow_checks {
            table["overflow-checks"] = toml_edit::value(v);
        }
        table
    }
}

pub fn run() -> Result<()> {
    println!("{}", "Scenario-Aware Profile Generator...".bold().cyan());
    println!();
    println!(
        "{:<15} {:<12} {:<12} {:<10} {:<10} {:<10}",
        "Scenario".bold(),
        "Incremental".bold(),
        "Codegen Units".bold(),
        "Opt Level".bold(),
        "LTO".bold(),
        "Strip".bold()
    );
    println!(
        "{}",
        "------------------------------------------------------------------".cyan()
    );

    for scenario in &[
        Scenario::Dev,
        Scenario::Test,
        Scenario::Ci,
        Scenario::Release,
    ] {
        let s = ProfileSettings::for_scenario(scenario);
        println!(
            "{:<15} {:<12} {:<12} {:<10} {:<10} {:<10}",
            format!("{}", scenario).cyan(),
            yes_no(s.incremental),
            s.codegen_units
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".into()),
            s.opt_level
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".into()),
            yes_no(s.lto),
            yes_no(s.strip),
        );
    }

    println!("\n{}", "Recommended usage:".bold().yellow());
    println!("  cargo accelerate profile dev      — Fast local iteration");
    println!("  cargo accelerate profile test     — Fast test execution");
    println!("  cargo accelerate profile ci       — Optimized CI builds");
    println!("  cargo accelerate profile release  — Maximum runtime performance");
    println!("\nRun `cargo accelerate optimize --scenario <name>` to apply.");

    Ok(())
}

fn yes_no(v: Option<bool>) -> String {
    match v {
        Some(true) => "yes".green().to_string(),
        Some(false) => "no".red().to_string(),
        None => "-".dimmed().to_string(),
    }
}

pub fn apply_profile(scenario: &Scenario, cargo_toml_path: &std::path::Path) -> Result<()> {
    let content = std::fs::read_to_string(cargo_toml_path)?;
    let mut doc = content.parse::<toml_edit::DocumentMut>()?;

    if !doc.contains_key("profile") {
        doc["profile"] = toml_edit::table();
    }

    let profile_name = scenario.to_string();
    if !doc["profile"]
        .as_table()
        .map(|t| t.contains_key(&profile_name))
        .unwrap_or(false)
    {
        doc["profile"][&profile_name] = toml_edit::table();
    }

    let settings = ProfileSettings::for_scenario(scenario);
    let table = settings.to_toml_table();

    for (key, value) in table.iter() {
        doc["profile"][&profile_name][key] = value.clone();
    }

    std::fs::write(cargo_toml_path, doc.to_string())?;
    println!(
        "  {} Applied {} profile to Cargo.toml",
        "✔".green(),
        profile_name.cyan()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_dev_profile_settings() {
        let cpus = available_cpus();
        let expected_cu = (cpus * 2).min(256) as i64;
        let s = ProfileSettings::for_scenario(&Scenario::Dev);
        assert_eq!(s.incremental, Some(true));
        assert_eq!(s.codegen_units, Some(expected_cu));
        assert_eq!(s.opt_level, Some(0));
    }

    #[test]
    fn test_release_profile_settings() {
        let s = ProfileSettings::for_scenario(&Scenario::Release);
        assert_eq!(s.opt_level, Some(3));
        assert_eq!(s.lto, Some(true));
        assert_eq!(s.strip, Some(true));
    }

    #[test]
    fn test_ci_profile_settings() {
        let s = ProfileSettings::for_scenario(&Scenario::Ci);
        assert_eq!(s.incremental, Some(false));
        assert_eq!(s.codegen_units, Some(1));
        assert_eq!(s.opt_level, Some(2));
    }

    #[test]
    fn test_profile_to_toml_table() {
        let cpus = available_cpus();
        let expected_cu = (cpus * 2).min(256) as i64;
        let s = ProfileSettings::for_scenario(&Scenario::Dev);
        let table = s.to_toml_table();
        assert_eq!(table["incremental"].as_bool(), Some(true));
        assert_eq!(table["codegen-units"].as_integer(), Some(expected_cu));
    }

    #[test]
    fn test_apply_profile_creates_profile() {
        let cpus = available_cpus();
        let expected_cu = (cpus * 2).min(256) as i64;
        use std::fs;
        let dir = TempDir::new().unwrap();
        let cargo_toml = dir.path().join("Cargo.toml");
        fs::write(&cargo_toml, "[package]\nname = \"test\"\n").unwrap();
        apply_profile(&Scenario::Dev, &cargo_toml).unwrap();
        let content = fs::read_to_string(&cargo_toml).unwrap();
        let doc = content.parse::<toml_edit::DocumentMut>().unwrap();
        assert_eq!(doc["profile"]["dev"]["incremental"].as_bool(), Some(true));
        assert_eq!(
            doc["profile"]["dev"]["codegen-units"].as_integer(),
            Some(expected_cu)
        );
    }

    #[test]
    fn test_apply_profile_preserves_existing() {
        use std::fs;
        let dir = TempDir::new().unwrap();
        let cargo_toml = dir.path().join("Cargo.toml");
        fs::write(
            &cargo_toml,
            r#"
            [package]
            name = "test"
            [profile.release]
            opt-level = 3
        "#,
        )
        .unwrap();
        apply_profile(&Scenario::Ci, &cargo_toml).unwrap();
        let content = fs::read_to_string(&cargo_toml).unwrap();
        let doc = content.parse::<toml_edit::DocumentMut>().unwrap();
        assert_eq!(doc["profile"]["release"]["opt-level"].as_integer(), Some(3));
        assert_eq!(doc["profile"]["ci"]["incremental"].as_bool(), Some(false));
    }

    #[test]
    fn test_display_scenario() {
        assert_eq!(format!("{}", Scenario::Dev), "dev");
        assert_eq!(format!("{}", Scenario::Ci), "ci");
        assert_eq!(format!("{}", Scenario::Release), "release");
    }
}
