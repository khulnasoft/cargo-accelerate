use crate::utils::{get_cached_metadata_for_root, get_cargo_toml_path, get_project_root};
use anyhow::{Context, Result};
use colored::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FeatureSuggestion {
    pub package_name: String,
    pub current_default_features: Vec<String>,
    pub recommended_features: Vec<String>,
    pub estimated_savings_pct: f64,
    pub is_optimized: bool,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct FeatureAudit {
    pub suggestions: Vec<FeatureSuggestion>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FeaturesConfig {
    pub suggestions: HashMap<String, Vec<String>>,
}

pub struct FeaturesOptions {
    pub optimize: bool,
}

fn known_feature_suggestions() -> &'static HashMap<&'static str, &'static [&'static str]> {
    static MAP: OnceLock<HashMap<&'static str, &'static [&'static str]>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut m = HashMap::new();
        m.insert("syn", &["derive"] as &[&str]);
        m.insert("tokio", &["rt-multi-thread"] as &[&str]);
        m.insert("clap", &["derive"] as &[&str]);
        m.insert("hyper", &["client", "http1"] as &[&str]);
        m.insert("reqwest", &["json"] as &[&str]);
        m.insert("chrono", &["serde"] as &[&str]);
        m.insert("rand", &["std"] as &[&str]);
        m.insert("serde", &["derive"] as &[&str]);
        m.insert("serde_json", &["std"] as &[&str]);
        m.insert("regex", &["std"] as &[&str]);
        m
    })
}

fn default_feature_count(suggested: &[&str], defaults: &[String]) -> usize {
    suggested
        .iter()
        .filter(|s| defaults.contains(&s.to_string()))
        .count()
}

pub fn analyze_features(root: &Path) -> Result<FeatureAudit> {
    let cargo_toml = get_cargo_toml_path(root);
    if !cargo_toml.exists() {
        return Ok(FeatureAudit::default());
    }

    let content = fs::read_to_string(&cargo_toml)?;
    let parsed: toml::Value = toml::from_str(&content)?;

    let direct_deps = parsed
        .get("dependencies")
        .and_then(|d| d.as_table())
        .map(|t| t.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>())
        .unwrap_or_default();

    if direct_deps.is_empty() {
        return Ok(FeatureAudit::default());
    }

    let metadata = get_cached_metadata_for_root(root)?;
    let known = known_feature_suggestions();

    let mut suggestions = Vec::new();

    for dep_name in &direct_deps {
        let package = metadata.packages.iter().find(|p| p.name == *dep_name);
        let package = match package {
            Some(p) => p,
            None => continue,
        };

        let has_default_features = parsed
            .get("dependencies")
            .and_then(|d| d.get(dep_name))
            .and_then(|d| d.get("default-features"))
            .and_then(|f| f.as_bool())
            .unwrap_or(true);

        let explicit_features: Vec<String> = parsed
            .get("dependencies")
            .and_then(|d| d.get(dep_name))
            .and_then(|d| d.get("features"))
            .and_then(|f| f.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let default_feature_list: Vec<String> =
            package.features.get("default").cloned().unwrap_or_default();

        let known_suggestion = known.get(dep_name.as_str());

        if let Some(suggested) = known_suggestion {
            let suggested_strs: Vec<String> = suggested.iter().map(|s| s.to_string()).collect();
            let already_optimized = !has_default_features;

            let current_defaults = if has_default_features {
                default_feature_list.clone()
            } else {
                explicit_features.clone()
            };

            let savings = if has_default_features && !default_feature_list.is_empty() {
                let overlap = default_feature_count(suggested, &default_feature_list);
                let total = default_feature_list.len();
                if total > 0 {
                    (1.0 - overlap as f64 / total as f64) * 100.0
                } else {
                    0.0
                }
            } else {
                0.0
            };

            suggestions.push(FeatureSuggestion {
                package_name: dep_name.clone(),
                current_default_features: current_defaults,
                recommended_features: suggested_strs,
                estimated_savings_pct: savings,
                is_optimized: already_optimized,
            });
        } else if !has_default_features || default_feature_list.is_empty() {
            let current_defaults = if has_default_features {
                default_feature_list.clone()
            } else {
                explicit_features.clone()
            };
            suggestions.push(FeatureSuggestion {
                package_name: dep_name.clone(),
                current_default_features: current_defaults,
                recommended_features: Vec::new(),
                estimated_savings_pct: 0.0,
                is_optimized: true,
            });
        }
    }

    suggestions.sort_by(|a, b| {
        b.estimated_savings_pct
            .partial_cmp(&a.estimated_savings_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(FeatureAudit { suggestions })
}

pub fn save_suggestions(root: &Path, audit: &FeatureAudit) -> Result<()> {
    let dir = root.join(".cargo-accelerate");
    fs::create_dir_all(&dir)?;
    let path = dir.join("features.toml");

    let mut config = FeaturesConfig {
        suggestions: HashMap::new(),
    };

    for s in &audit.suggestions {
        if !s.is_optimized && !s.recommended_features.is_empty() {
            config
                .suggestions
                .insert(s.package_name.clone(), s.recommended_features.clone());
        }
    }

    let toml_str = toml::to_string(&config)?;
    fs::write(&path, toml_str)?;
    println!("  {} Suggestions saved to {}", "✔".green(), path.display());
    Ok(())
}

pub fn optimize_dependencies(root: &Path, audit: &FeatureAudit) -> Result<()> {
    let cargo_toml = get_cargo_toml_path(root);
    if !cargo_toml.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&cargo_toml)?;
    let mut doc = content.parse::<toml_edit::DocumentMut>()?;

    let mut optimized_count = 0;

    for suggestion in &audit.suggestions {
        if suggestion.is_optimized || suggestion.recommended_features.is_empty() {
            continue;
        }

        let dep_name = &suggestion.package_name;

        if let Some(dep) = doc
            .get_mut("dependencies")
            .and_then(|d| d.as_table_mut())
            .and_then(|t| t.get_mut(dep_name))
        {
            fn make_features_array(features: &[String]) -> toml_edit::Array {
                let mut arr = toml_edit::Array::new();
                for f in features {
                    arr.push(f);
                }
                arr
            }

            if dep.is_str() {
                let version = dep.as_str().unwrap_or("").to_string();
                let mut inline = toml_edit::InlineTable::new();
                inline.insert("version", toml_edit::Value::from(version));
                inline.insert("default-features", toml_edit::Value::from(false));
                inline.insert(
                    "features",
                    toml_edit::Value::from(make_features_array(&suggestion.recommended_features)),
                );
                *dep = toml_edit::Item::Value(toml_edit::Value::InlineTable(inline));
                optimized_count += 1;
                continue;
            }

            if let Some(table) = dep.as_table_mut() {
                table.insert("default-features", toml_edit::value(false));
                table.insert(
                    "features",
                    toml_edit::value(make_features_array(&suggestion.recommended_features)),
                );
                optimized_count += 1;
            } else if let Some(inline) = dep.as_inline_table_mut() {
                inline.insert("default-features", toml_edit::Value::from(false));
                inline.insert(
                    "features",
                    toml_edit::Value::from(make_features_array(&suggestion.recommended_features)),
                );
                optimized_count += 1;
            }
        }
    }

    if optimized_count > 0 {
        fs::write(&cargo_toml, doc.to_string())?;
        println!(
            "  {} Optimized {} dependenc{} in Cargo.toml",
            "✔".green(),
            optimized_count,
            if optimized_count == 1 { "y" } else { "ies" }
        );
    } else {
        println!("  No dependencies to optimize.");
    }

    Ok(())
}

pub fn run(options: FeaturesOptions) -> Result<()> {
    println!("{}", "Dependency Feature Audit...".bold().cyan());

    let root = get_project_root().context("Could not find project root")?;
    let audit = analyze_features(&root)?;

    if audit.suggestions.is_empty() {
        println!("  No dependencies found to analyze.");
        return Ok(());
    }

    println!(
        "\n{:<20} {:<30} {:<25} {:<12}",
        "Dependency".bold(),
        "Current Default Features".bold(),
        "Recommended".bold(),
        "Saving".bold()
    );
    println!(
        "{}",
        "------------------------------------------------------------------------------".cyan()
    );

    let mut needs_optimization = 0;

    for s in &audit.suggestions {
        let name = if s.is_optimized {
            s.package_name.green()
        } else if s.estimated_savings_pct > 20.0 {
            s.package_name.red()
        } else if s.estimated_savings_pct > 5.0 {
            s.package_name.yellow()
        } else {
            s.package_name.normal()
        };

        let defaults_str = if s.current_default_features.is_empty() {
            "-".to_string()
        } else if s.current_default_features.len() <= 4 {
            s.current_default_features.join(", ")
        } else {
            format!("{} features", s.current_default_features.len())
        };

        let recommended_str = if s.recommended_features.is_empty() {
            "(already optimized)".green().to_string()
        } else if s.is_optimized {
            "✔ optimized".green().to_string()
        } else {
            s.recommended_features.join(", ").yellow().to_string()
        };

        let savings_str = if s.is_optimized {
            "✔".green().to_string()
        } else if s.estimated_savings_pct > 0.0 {
            format!("{:.0}%", s.estimated_savings_pct).red().to_string()
        } else {
            "-".dimmed().to_string()
        };

        if !s.is_optimized && !s.recommended_features.is_empty() {
            needs_optimization += 1;
        }

        println!(
            "{:<20} {:<30} {:<25} {:<12}",
            name, defaults_str, recommended_str, savings_str
        );
    }

    println!("\n{} dependencies analyzed", audit.suggestions.len());
    if needs_optimization > 0 {
        println!(
            "\n{} {} dependenc{} can be optimized. Run with --optimize to apply.",
            "⚠".yellow(),
            needs_optimization,
            if needs_optimization == 1 { "y" } else { "ies" }
        );
    } else {
        println!("\n{} All dependencies are already optimized!", "✔".green());
    }

    save_suggestions(&root, &audit)?;

    if options.optimize && needs_optimization > 0 {
        println!("\n{}", "Applying optimizations...".bold().yellow());
        optimize_dependencies(&root, &audit)?;
    } else if options.optimize {
        println!("  Nothing to optimize.");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_cargo_toml(dir: &TempDir, content: &str) {
        fs::write(dir.path().join("Cargo.toml"), content).unwrap();
    }

    #[test]
    fn test_known_feature_suggestions_contains_syn() {
        let known = known_feature_suggestions();
        assert!(known.contains_key("syn"));
        assert_eq!(known.get("syn").unwrap(), &["derive"]);
    }

    #[test]
    fn test_known_feature_suggestions_contains_tokio() {
        let known = known_feature_suggestions();
        assert!(known.contains_key("tokio"));
    }

    #[test]
    fn test_analyze_features_no_cargo_toml() {
        let dir = TempDir::new().unwrap();
        let audit = analyze_features(dir.path()).unwrap();
        assert_eq!(audit.suggestions.len(), 0);
    }

    #[test]
    fn test_analyze_features_empty_deps() {
        let dir = TempDir::new().unwrap();
        create_cargo_toml(&dir, "[package]\nname = \"test\"\n");
        let audit = analyze_features(dir.path()).unwrap();
        assert_eq!(audit.suggestions.len(), 0);
    }

    #[test]
    fn test_analyze_features_uses_root_metadata() {
        if !crate::utils::is_tool_installed("cargo") {
            return;
        }
        // Build a temp workspace whose dependency "tokio" is a LOCAL path dep
        // with a distinctive default feature, so a correct root-relative
        // metadata lookup is required for the suggestion to be accurate.
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("dep/src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"audit-test\"\nversion = \"0.1.0\"\n[dependencies]\ntokio = { path = \"dep\" }\n",
        )
        .unwrap();
        fs::write(
            root.join("dep/Cargo.toml"),
            "[package]\nname = \"tokio\"\nversion = \"9.9.9\"\n[features]\ndefault = [\"full\"]\nfull = []\nrt-multi-thread = []\n",
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn x() {}\n").unwrap();
        fs::write(root.join("dep/src/lib.rs"), "pub fn y() {}\n").unwrap();

        let audit = analyze_features(root).unwrap();
        let tokio = audit
            .suggestions
            .iter()
            .find(|s| s.package_name == "tokio")
            .expect("expected a suggestion for tokio");
        assert_eq!(tokio.current_default_features, vec!["full".to_string()]);
        assert_eq!(
            tokio.recommended_features,
            vec!["rt-multi-thread".to_string()]
        );
        assert!(!tokio.is_optimized);
    }

    #[test]
    fn test_feature_suggestion_serialization() {
        let s = FeatureSuggestion {
            package_name: "tokio".into(),
            current_default_features: vec!["rt".into(), "net".into(), "io-util".into()],
            recommended_features: vec!["rt-multi-thread".into()],
            estimated_savings_pct: 60.0,
            is_optimized: false,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("tokio"));
        assert!(json.contains("rt-multi-thread"));
    }

    #[test]
    fn test_feature_audit_default() {
        let audit = FeatureAudit::default();
        assert!(audit.suggestions.is_empty());
    }

    #[test]
    fn test_save_suggestions_creates_file() {
        let dir = TempDir::new().unwrap();
        let audit = FeatureAudit {
            suggestions: vec![FeatureSuggestion {
                package_name: "tokio".into(),
                current_default_features: vec!["full".into()],
                recommended_features: vec!["rt-multi-thread".into()],
                estimated_savings_pct: 70.0,
                is_optimized: false,
            }],
        };
        save_suggestions(dir.path(), &audit).unwrap();
        let path = dir.path().join(".cargo-accelerate/features.toml");
        assert!(path.exists());
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("tokio"));
    }

    #[test]
    fn test_optimize_dependencies_rewrites_cargo_toml() {
        let dir = TempDir::new().unwrap();
        create_cargo_toml(
            &dir,
            r#"
            [package]
            name = "test"
            [dependencies]
            tokio = "1.0"
        "#,
        );
        let audit = FeatureAudit {
            suggestions: vec![FeatureSuggestion {
                package_name: "tokio".into(),
                current_default_features: vec!["full".into()],
                recommended_features: vec!["rt-multi-thread".into()],
                estimated_savings_pct: 70.0,
                is_optimized: false,
            }],
        };
        optimize_dependencies(dir.path(), &audit).unwrap();
        let content = fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
        assert!(content.contains("default-features = false"));
        assert!(content.contains("rt-multi-thread"));
    }

    #[test]
    fn test_optimize_dependencies_skips_already_optimized() {
        let dir = TempDir::new().unwrap();
        create_cargo_toml(
            &dir,
            r#"
            [package]
            name = "test"
            [dependencies]
            tokio = { version = "1.0", default-features = false, features = ["rt-multi-thread"] }
        "#,
        );
        let original = fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
        let audit = FeatureAudit {
            suggestions: vec![FeatureSuggestion {
                package_name: "tokio".into(),
                current_default_features: vec![],
                recommended_features: vec!["rt-multi-thread".into()],
                estimated_savings_pct: 70.0,
                is_optimized: true,
            }],
        };
        optimize_dependencies(dir.path(), &audit).unwrap();
        let content = fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
        assert_eq!(content, original);
    }

    #[test]
    fn test_inline_table_optimization() {
        let dir = TempDir::new().unwrap();
        create_cargo_toml(
            &dir,
            r#"
            [package]
            name = "test"
            [dependencies]
            serde = { version = "1.0", features = ["derive"] }
        "#,
        );
        let audit = FeatureAudit {
            suggestions: vec![FeatureSuggestion {
                package_name: "serde".into(),
                current_default_features: vec!["std".into()],
                recommended_features: vec!["derive".into()],
                estimated_savings_pct: 30.0,
                is_optimized: false,
            }],
        };
        optimize_dependencies(dir.path(), &audit).unwrap();
        let content = fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
        assert!(content.contains("default-features = false"));
        assert!(content.contains("derive"));
    }
}
