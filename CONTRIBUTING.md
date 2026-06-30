# Contributing to Cargo Accelerate

Thank you for considering contributing! We welcome bug reports, feature requests, documentation improvements, and code changes.

## Getting Started

1. Fork the repository.
2. Clone your fork: `git clone https://github.com/<your-username>/cargo-accelerate.git`
3. Build: `cargo build`
4. Run tests: `cargo test`

## Development Guidelines

### Code Style

- Run `cargo fmt` before committing.
- Address all `cargo clippy` warnings.
- Follow existing patterns — the codebase uses `anyhow` for errors, `colored` for terminal output, and `toml_edit` for TOML manipulation.

### Adding a New Command

1. Define the command variant in `src/cli.rs`.
2. Add a `mod` declaration and handle the dispatch in `src/main.rs`.
3. Implement the logic in a new or existing module under `src/`.
4. Wire up the `run()` function with `anyhow::Result<()>`.
5. Add unit tests — see existing modules for patterns (tempfile-based tests for file operations).

### Testing

- Unit tests live in `#[cfg(test)] mod tests` blocks at the bottom of each source file.
- Run `cargo test` to execute all tests.
- For filesystem tests, use `tempfile::TempDir`.

## Pull Request Process

1. Ensure your code compiles without warnings (`cargo check`).
2. Run `cargo test` and confirm all tests pass.
3. Update the README if your change affects the CLI surface.
4. Open a PR with a clear title and description of the change.
5. A maintainer will review and merge once approved.

## Reporting Issues

- Use the GitHub issue tracker.
- Include the output of `cargo accelerate doctor` when reporting problems.
- Mention your OS and Rust version (`rustc --version`).
