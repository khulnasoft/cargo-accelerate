use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_get_os_returns_valid() {
        let os = get_os();
        assert!(matches!(os, "linux" | "macos" | "windows" | "unknown"));
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
