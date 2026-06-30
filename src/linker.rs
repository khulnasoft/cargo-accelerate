use crate::utils::{get_cargo_config_path, get_os, get_project_root, is_tool_installed};
use anyhow::{Context, Result};
use colored::*;
use std::fs;
use toml_edit::{Array, DocumentMut};

pub fn run() -> Result<()> {
    println!("{}", "Configuring Fast Linker...".bold().cyan());

    let os = get_os();
    let root = get_project_root().context("Could not find project root")?;
    let config_dir = root.join(".cargo");
    if !config_dir.exists() {
        fs::create_dir_all(&config_dir)?;
    }

    let config_path = get_cargo_config_path(&root);
    let mut doc = if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        content.parse::<DocumentMut>()?
    } else {
        DocumentMut::new()
    };

    if !doc.contains_key("target") {
        doc["target"] = toml_edit::table();
    }

    match os {
        "linux" => {
            if is_tool_installed("mold") {
                println!("  mold linker detected. Configuring for Linux...");
                configure_linux_linker(&mut doc)?;
                fs::write(&config_path, doc.to_string())?;
                println!(
                    "{} Linux targets configured to use mold linker successfully!",
                    "✔".green()
                );
            } else if is_tool_installed("lld") {
                println!("  lld linker detected. Configuring for Linux...");
                configure_lld_linker(
                    &mut doc,
                    &["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"],
                )?;
                fs::write(&config_path, doc.to_string())?;
                println!(
                    "{} Linux targets configured to use lld linker successfully!",
                    "✔".green()
                );
            } else {
                println!("{} Neither mold nor lld is installed. Please install mold or lld to optimize link times.", "⚠".yellow());
                println!(
                    "  To install mold: `sudo apt install mold clang` or `brew install mold`."
                );
            }
        }
        "macos" => {
            if is_tool_installed("lld") {
                println!("  lld linker detected. Configuring for macOS...");
                configure_lld_linker(&mut doc, &["x86_64-apple-darwin", "aarch64-apple-darwin"])?;
                fs::write(&config_path, doc.to_string())?;
                println!(
                    "{} macOS targets configured to use lld linker successfully!",
                    "✔".green()
                );
            } else {
                println!(
                    "{} lld is not installed. Please install lld to optimize link times on macOS.",
                    "⚠".yellow()
                );
                println!("  To install lld: `brew install llvm` or `brew install lld`.");
            }
        }
        "windows" => {
            if is_tool_installed("lld-link") || is_tool_installed("lld") {
                println!("  lld linker detected. Configuring for Windows...");
                configure_windows_linker(&mut doc)?;
                fs::write(&config_path, doc.to_string())?;
                println!(
                    "{} Windows targets configured to use lld-link successfully!",
                    "✔".green()
                );
            } else {
                println!("{} lld-link/lld is not installed. Please install llvm to optimize link times on Windows.", "⚠".yellow());
                println!("  To install lld on Windows: `scoop install llvm` or `choco install x64-msc-llvm`.");
            }
        }
        _ => {
            println!(
                "{} Unsupported OS for automatic linker configuration.",
                "✖".red()
            );
        }
    }

    Ok(())
}

fn configure_linux_linker(doc: &mut DocumentMut) -> Result<()> {
    // Configure mold for standard linux targets
    let targets = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"];
    for target in targets {
        if doc["target"].get(target).is_none() {
            doc["target"][target] = toml_edit::table();
        }

        doc["target"][target]["linker"] = toml_edit::value("clang");

        let mut arr = Array::new();
        arr.push("-C");
        arr.push("link-arg=-fuse-ld=mold");
        doc["target"][target]["rustflags"] = toml_edit::value(arr);
    }
    Ok(())
}

fn configure_lld_linker(doc: &mut DocumentMut, targets: &[&str]) -> Result<()> {
    for target in targets {
        if doc["target"].get(target).is_none() {
            doc["target"][target] = toml_edit::table();
        }

        let mut arr = Array::new();
        arr.push("-C");
        arr.push("link-arg=-fuse-ld=lld");
        doc["target"][target]["rustflags"] = toml_edit::value(arr);
    }
    Ok(())
}

fn configure_windows_linker(doc: &mut DocumentMut) -> Result<()> {
    let target = "x86_64-pc-windows-msvc";
    if doc["target"].get(target).is_none() {
        doc["target"][target] = toml_edit::table();
    }

    let mut arr = Array::new();
    arr.push("-C");
    arr.push("link-arg=-fuse-ld=lld-link");
    doc["target"][target]["rustflags"] = toml_edit::value(arr);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use toml_edit::DocumentMut;

    fn new_doc() -> DocumentMut {
        let mut doc = DocumentMut::new();
        doc["target"] = toml_edit::table();
        doc
    }

    #[test]
    fn test_configure_linux_linker_sets_mold() {
        let mut doc = new_doc();
        configure_linux_linker(&mut doc).unwrap();

        for target in ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"] {
            let tbl = doc["target"][target].as_table().unwrap();
            assert_eq!(tbl["linker"].as_str(), Some("clang"));
            let flags = tbl["rustflags"].as_array().unwrap();
            let joined: String = flags
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            assert!(
                joined.contains("mold"),
                "expected mold in rustflags for {}",
                target
            );
        }
    }

    #[test]
    fn test_configure_lld_linker() {
        let mut doc = new_doc();
        let targets = ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"];
        configure_lld_linker(&mut doc, &targets).unwrap();

        for target in &targets {
            let tbl = doc["target"][*target].as_table().unwrap();
            let flags = tbl["rustflags"].as_array().unwrap();
            let joined: String = flags
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            assert!(
                joined.contains("lld"),
                "expected lld in rustflags for {}",
                target
            );
        }
    }

    #[test]
    fn test_configure_windows_linker() {
        let mut doc = new_doc();
        configure_windows_linker(&mut doc).unwrap();

        let tbl = doc["target"]["x86_64-pc-windows-msvc"].as_table().unwrap();
        let flags = tbl["rustflags"].as_array().unwrap();
        let joined: String = flags
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(joined.contains("lld-link"));
    }

    #[test]
    fn test_configure_linux_linker_creates_target_table() {
        let mut doc = DocumentMut::new();
        doc["target"] = toml_edit::table();
        configure_linux_linker(&mut doc).unwrap();
        assert!(doc["target"]["x86_64-unknown-linux-gnu"].is_table());
    }

    #[test]
    fn test_configure_lld_linker_does_not_set_linker() {
        let mut doc = new_doc();
        configure_lld_linker(&mut doc, &["x86_64-unknown-linux-gnu"]).unwrap();
        let tbl = doc["target"]["x86_64-unknown-linux-gnu"]
            .as_table()
            .unwrap();
        assert!(
            tbl.get("linker").is_none(),
            "lld config should not set linker field"
        );
    }
}
