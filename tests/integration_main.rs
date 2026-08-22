use std::process::Command;

fn bin() -> Command {
    // The binary is a cargo subcommand wrapper: argv starts with "accelerate".
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cargo-accelerate"));
    cmd.arg("accelerate");
    cmd
}

#[test]
fn help_exits_successfully_and_lists_subcommands() {
    let output = bin().arg("--help").output().expect("failed to run binary");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for cmd in [
        "doctor",
        "optimize",
        "benchmark",
        "cache",
        "linker",
        "workspace",
        "deps",
        "features",
        "ci",
        "watch",
        "daemon",
        "install",
        "graph",
        "regression",
        "policy",
        "trace",
        "trend",
        "timings",
        "stats",
        "audit",
        "auto",
        "profile",
    ] {
        assert!(
            stdout.contains(cmd),
            "--help output missing subcommand: {cmd}"
        );
    }
}

#[test]
fn unknown_command_fails_with_nonzero_exit() {
    let output = bin()
        .arg("nonexistent")
        .output()
        .expect("failed to run binary");
    assert!(!output.status.success());
}

#[test]
fn every_subcommand_help_renders_without_panic() {
    let subcommands = [
        "doctor",
        "optimize",
        "benchmark",
        "cache",
        "linker",
        "workspace",
        "deps",
        "features",
        "ci",
        "watch",
        "daemon",
        "install",
        "graph",
        "regression",
        "policy",
        "trace",
        "trend",
        "timings",
        "stats",
        "audit",
        "auto",
        "profile",
    ];
    for cmd in subcommands {
        let output = bin()
            .args([cmd, "--help"])
            .output()
            .unwrap_or_else(|e| panic!("failed to run {cmd} --help: {e}"));
        assert!(output.status.success(), "{cmd} --help exited with failure");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Usage"),
            "{cmd} --help did not render usage"
        );
    }
}

#[test]
fn version_flag_prints_version() {
    let output = bin()
        .arg("--version")
        .output()
        .expect("failed to run binary");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")));
}
