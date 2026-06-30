use crate::utils::{get_project_root, is_tool_installed};
use anyhow::{Context, Result};
use colored::*;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::channel;
use std::thread;
use std::time::{Duration, Instant};

pub fn run() -> Result<()> {
    println!("{}", "Starting Cargo Accelerate Daemon...".bold().cyan());
    let root = get_project_root().context("Could not find project root")?;
    println!("  Watching {}", root.display());

    if is_tool_installed("sccache") {
        println!("  sccache detected — background builds will warm the cache.");
    } else {
        println!("  sccache not detected. Run `cargo accelerate install` to enable cache warming.");
    }

    let (tx, rx) = channel();
    let mut watcher = RecommendedWatcher::new(
        move |res| {
            tx.send(res).expect("file watcher channel closed");
        },
        Config::default(),
    )
    .context("Failed to create file watcher")?;

    watcher
        .watch(&root, RecursiveMode::Recursive)
        .context("Failed to watch project directory")?;

    let debounce = Duration::from_millis(500);
    let mut pending_check = false;
    let mut last_event = Instant::now();

    println!("  Daemon is running. Press Ctrl+C to stop.");
    println!("  Changes to source files, Cargo.toml, and workspace manifests will trigger background cargo check.");

    // Warm cache in background thread so daemon starts watching immediately
    let warm_root = root.clone();
    thread::spawn(move || warm_cache_preload(&warm_root));

    loop {
        match rx.recv() {
            Ok(Ok(event)) => {
                if is_relevant_event(&event, &root) {
                    pending_check = true;
                    last_event = Instant::now();
                }
            }
            Ok(Err(err)) => {
                eprintln!("{} File watcher error: {}", "⚠".yellow(), err);
            }
            Err(_) => break,
        }

        while pending_check {
            let elapsed = last_event.elapsed();
            if elapsed < debounce {
                let timeout = debounce - elapsed;
                match rx.recv_timeout(timeout) {
                    Ok(Ok(event)) => {
                        if is_relevant_event(&event, &root) {
                            last_event = Instant::now();
                        }
                        continue;
                    }
                    Ok(Err(err)) => {
                        eprintln!("{} File watcher error: {}", "⚠".yellow(), err);
                        continue;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                }
            }

            pending_check = false;
            if let Err(err) = run_cargo_check(&root) {
                eprintln!("{} Background check failed: {}", "✖".red(), err);
            }
        }
    }

    Ok(())
}

fn warm_cache_preload(root: &PathBuf) {
    println!("\n{} Prewarming build cache...", "➤".cyan());
    let start = Instant::now();

    // Build high-impact targets to warm the cache: workspace check + key deps
    let status = Command::new("cargo")
        .args(&["check", "--workspace"])
        .current_dir(root)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    match status {
        Ok(s) if s.success() => {
            let elapsed = start.elapsed();
            println!(
                "{} Cache prewarmed in {:.1}s",
                "✔".green(),
                elapsed.as_secs_f32()
            );
        }
        _ => {
            println!(
                "{} Cache prewarm did not fully complete (background will continue)",
                "⚠".yellow()
            );
        }
    }
}

fn is_relevant_event(event: &Event, root: &PathBuf) -> bool {
    if !matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) {
        return false;
    }

    event.paths.iter().any(|path| {
        if let Ok(relative) = path.strip_prefix(root) {
            if relative.components().next().map(|c| c.as_os_str())
                == Some(std::ffi::OsStr::new("target"))
            {
                return false;
            }
            if relative.components().next().map(|c| c.as_os_str())
                == Some(std::ffi::OsStr::new(".git"))
            {
                return false;
            }
        }

        if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
            return matches!(extension, "rs" | "toml" | "lock" | "json");
        }

        if let Some(file_name) = path.file_name().and_then(|name| name.to_str()) {
            return file_name == "Cargo.toml" || file_name == "Cargo.lock";
        }

        false
    })
}

fn run_cargo_check(root: &PathBuf) -> Result<()> {
    let start = Instant::now();
    println!("\n{} Running cargo check...", "➤".cyan());

    let status = Command::new("cargo")
        .arg("check")
        .arg("--workspace")
        .current_dir(root)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("Failed to execute cargo check")?;

    let elapsed = start.elapsed();
    if status.success() {
        println!(
            "{} cargo check completed in {:.1}s",
            "✔".green(),
            elapsed.as_secs_f32()
        );
        Ok(())
    } else {
        anyhow::bail!("cargo check exited with status: {}", status);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_is_relevant_event_rs_file() {
        let root = PathBuf::from("/project");
        let event = Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Any,
            )),
            paths: vec![PathBuf::from("/project/src/lib.rs")],
            ..Default::default()
        };
        assert!(is_relevant_event(&event, &root));
    }

    #[test]
    fn test_is_relevant_event_ignores_target() {
        let root = PathBuf::from("/project");
        let event = Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Any,
            )),
            paths: vec![PathBuf::from("/project/target/debug/build")],
            ..Default::default()
        };
        assert!(!is_relevant_event(&event, &root));
    }

    #[test]
    fn test_is_relevant_event_ignores_git() {
        let root = PathBuf::from("/project");
        let event = Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Any,
            )),
            paths: vec![PathBuf::from("/project/.git/config")],
            ..Default::default()
        };
        assert!(!is_relevant_event(&event, &root));
    }

    #[test]
    fn test_is_relevant_event_non_matching_extension() {
        let root = PathBuf::from("/project");
        let event = Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Data(
                notify::event::DataChange::Any,
            )),
            paths: vec![PathBuf::from("/project/README.md")],
            ..Default::default()
        };
        assert!(!is_relevant_event(&event, &root));
    }

    #[test]
    fn test_is_relevant_event_cargo_toml() {
        let root = PathBuf::from("/project");
        let event = Event {
            kind: EventKind::Create(notify::event::CreateKind::File),
            paths: vec![PathBuf::from("/project/Cargo.toml")],
            ..Default::default()
        };
        assert!(is_relevant_event(&event, &root));
    }
}
