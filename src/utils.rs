use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use which::which;

static PROJECT_ROOT: OnceLock<PathBuf> = OnceLock::new();

pub fn is_tool_installed(tool: &str) -> bool {
    which(tool).is_ok()
}

pub fn get_project_root() -> Result<PathBuf> {
    if let Some(root) = PROJECT_ROOT.get() {
        return Ok(root.clone());
    }
    let metadata = cargo_metadata::MetadataCommand::new().no_deps().exec()?;
    let root: PathBuf = metadata.workspace_root.into();
    let _ = PROJECT_ROOT.set(root.clone());
    Ok(root)
}

pub fn get_cached_metadata() -> Result<&'static cargo_metadata::Metadata> {
    static METADATA: OnceLock<cargo_metadata::Metadata> = OnceLock::new();
    if let Some(m) = METADATA.get() {
        return Ok(m);
    }
    let metadata = cargo_metadata::MetadataCommand::new().no_deps().exec()?;
    Ok(METADATA.get_or_init(|| metadata))
}

pub fn get_cached_metadata_with_deps() -> Result<&'static cargo_metadata::Metadata> {
    static METADATA_DEPS: OnceLock<cargo_metadata::Metadata> = OnceLock::new();
    if let Some(m) = METADATA_DEPS.get() {
        return Ok(m);
    }
    let metadata = cargo_metadata::MetadataCommand::new().exec()?;
    Ok(METADATA_DEPS.get_or_init(|| metadata))
}

/// Returns workspace metadata for the given root, running `cargo metadata` in
/// that directory and caching the result keyed by root path.
pub fn get_cached_metadata_for_root(root: &Path) -> Result<&'static cargo_metadata::Metadata> {
    static METADATA_BY_ROOT: OnceLock<Mutex<HashMap<PathBuf, &'static cargo_metadata::Metadata>>> =
        OnceLock::new();
    let map = METADATA_BY_ROOT.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = map.lock().expect("metadata cache lock poisoned");
    let key = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if let Some(m) = guard.get(&key) {
        return Ok(m);
    }
    let metadata = cargo_metadata::MetadataCommand::new()
        .current_dir(&key)
        .exec()?;
    let metadata: &'static cargo_metadata::Metadata = Box::leak(Box::new(metadata));
    guard.insert(key, metadata);
    Ok(metadata)
}

pub fn get_cargo_config_path(root: &Path) -> PathBuf {
    root.join(".cargo").join("config.toml")
}

pub fn get_cargo_toml_path(root: &Path) -> PathBuf {
    root.join("Cargo.toml")
}

pub fn get_os() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "unknown"
    }
}

pub fn available_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn create_workspace(dir: &TempDir, pkg_name: &str, dep_name: &str) {
        let root = dir.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("dep/src")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            format!(
                "[package]\nname = \"{}\"\nversion = \"0.1.0\"\n[dependencies]\n{} = {{ path = \"dep\" }}\n",
                pkg_name, dep_name
            ),
        )
        .unwrap();
        fs::write(
            root.join("dep/Cargo.toml"),
            format!("[package]\nname = \"{}\"\nversion = \"1.0.0\"\n", dep_name),
        )
        .unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn x() {}\n").unwrap();
        fs::write(root.join("dep/src/lib.rs"), "pub fn y() {}\n").unwrap();
    }

    #[test]
    fn test_get_cached_metadata_keyed_by_root() {
        if !is_tool_installed("cargo") {
            return;
        }
        let dir_a = TempDir::new().unwrap();
        create_workspace(&dir_a, "workspace-a", "dep-a");
        let dir_b = TempDir::new().unwrap();
        create_workspace(&dir_b, "workspace-b", "dep-b");

        let meta_a = get_cached_metadata_for_root(dir_a.path()).unwrap();
        let meta_b = get_cached_metadata_for_root(dir_b.path()).unwrap();

        assert!(meta_a.packages.iter().any(|p| p.name == "workspace-a"));
        assert!(meta_b.packages.iter().any(|p| p.name == "workspace-b"));
        assert!(
            !meta_b.packages.iter().any(|p| p.name == "workspace-a"),
            "metadata must be keyed by root; root B returned root A's metadata"
        );
    }

    #[test]
    fn test_get_os_returns_valid() {
        let os = get_os();
        assert!(matches!(os, "linux" | "macos" | "windows" | "unknown"));
    }

    #[test]
    fn test_available_cpus_returns_positive() {
        let cpus = available_cpus();
        assert!(cpus >= 1);
        // Sanity check: unlikely to have more than 1024 CPUs
        assert!(cpus < 1024);
    }

    #[test]
    fn test_get_cargo_config_path() {
        let path = get_cargo_config_path(Path::new("/project"));
        assert_eq!(path, Path::new("/project/.cargo/config.toml"));
    }

    #[test]
    fn test_get_cargo_toml_path() {
        let path = get_cargo_toml_path(Path::new("/project"));
        assert_eq!(path, Path::new("/project/Cargo.toml"));
    }

    #[test]
    fn test_get_cargo_config_path_relative() {
        let path = get_cargo_config_path(Path::new("my-project"));
        assert_eq!(path, Path::new("my-project/.cargo/config.toml"));
    }
}
