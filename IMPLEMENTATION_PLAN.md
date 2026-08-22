# Implementation Plan: Improvements, Gaps & Code Coverage

> Status: Proposed · Baseline: v0.1.0 · 9,116 LoC · 196 unit tests, all passing

---

## 1. Current State Audit

### 1.1 Test Coverage by Module

| Module | LoC | Fns | Tests | Coverage Risk |
|---|---|---|---|---|
| benchmark.rs | 392 | 9 | **0** | 🔴 None |
| dependencies.rs | 156 | 2 | **0** | 🔴 None |
| installer.rs | 147 | 3 | **0** | 🔴 None |
| main.rs | 145 | 1 | **0** | 🔴 None (dispatch untested) |
| watch.rs | 65 | 1 | **0** | 🔴 None |
| auto.rs | 300 | 13 | 4 | 🟠 Thin (~31%) |
| regression.rs | 261 | 7 | 3 | 🟠 Thin (~43%) |
| linker.rs | 234 | 10 | 5 | 🟡 ~50% |
| profile.rs | 284 | 13 | 7 | 🟡 ~54% |
| workspace.rs | 208 | 9 | 7 | 🟢 OK |
| build_audit.rs | 454 | 17 | 8 | 🟢 OK |
| trend.rs | 322 | 18 | 8 | 🟢 OK |
| cache.rs | 595 | 20 | 9 | 🟢 OK |
| ci.rs | 449 | 15 | 10 | 🟢 OK |
| features.rs | 557 | 19 | 11 | 🟢 OK |
| policy.rs | 561 | 23 | 12 | 🟢 OK |
| doctor.rs | 523 | 20 | 13 | 🟢 OK |
| graph.rs | 773 | 29 | 15 | 🟢 OK |
| tracer.rs | 726 | 30 | 16 | 🟢 OK |
| timings.rs | 679 | 41 | 17 | 🟢 OK |
| cli.rs | 686 | 39 | 38 | 🟢 Strong |
| utils.rs | 169 | 16 | 6 | 🟡 ~37% |

**Structural gaps:**
- No `tests/` directory — zero integration tests; all coverage is intra-module unit tests.
- No coverage measurement tooling (`cargo-llvm-cov` / `tarpaulin`) locally or in CI.
- CI (`.github/workflows/build.yml`) runs build/test only — no lint gate, no coverage report.

### 1.2 Functional Gap: Timings Data Recorded but Never Surfaced

`timings.rs` implements a full query/aggregation API — `query_by_profile`,
`query_by_branch`, `query_by_label`, `query_since`, `median_duration_ms`,
`avg_duration_ms`, `total_count` — but **none are called outside tests**
(this is the existing `dead_code` build warning). Data flows *into*
`.cargo-accelerate/timings.json` from `trend`/`regression`/`benchmark`, but no
command reads it back out for analysis. This is wasted infrastructure and the
highest-value quick win in this plan.

### 1.3 Code Quality Debt

| Issue | Location | Severity |
|---|---|---|
| Dead-code warning on 7 `TimingStore` methods | src/timings.rs:63–116 | HIGH (fix via §3.1) |
| No `--dry-run` on mutating commands (`optimize`, `profile`, `linker`) | optimizer.rs, profile.rs | HIGH |
| Error paths of `run()` commands untested (I/O failures, missing tools) | all modules | MEDIUM |
| `main.rs` dispatch has no test harness | main.rs:145 LoC | MEDIUM |
| No clippy/rustfmt enforcement | CI | LOW |

---

## 2. Phase 1 — Coverage Infrastructure (Week 1)

Goal: make coverage measurable and visible before writing new tests.

- [ ] **P1.1** Add `cargo-llvm-cov` workflow:
      ```bash
      cargo install cargo-llvm-cov
      cargo llvm-cov --html --output-dir target/coverage   # local
      cargo llvm-cov --lcov --output-path lcov.info        # CI
      ```
- [ ] **P1.2** Extend `.github/workflows/build.yml`:
      - Job: `cargo clippy -- -D warnings` + `cargo fmt --check`
      - Job: `cargo llvm-cov` → upload `lcov.info` to Codecov / artifact
      - Fail PRs dropping overall line coverage below a floor (start at measured baseline, ratchet up)
- [ ] **P1.3** Record baseline numbers in this file after first run (fill table §1.1 with real %).
- [ ] **P1.4** Add `tests/integration_main.rs`: assert `--help`, unknown command exit codes,
      and each subcommand's `--help` renders without panic (covers main.rs dispatch).

**Exit criteria:** coverage report published per PR; baseline % documented; dispatch smoke-tested.

## 3. Phase 2 — Wire Up Dead Code & Quick Wins (Week 1–2)

- [ ] **P2.1 New `cargo accelerate stats` command** consuming the orphaned `TimingStore` API:
      - `stats [--since <ts>] [--branch <b>] [--profile <p>] [--label <l>]`
      - Output per-command count/median/avg (uses exactly the 7 dead methods → removes warning)
      - Unit tests mirror timings.rs tests; CLI parse tests in cli.rs pattern
- [ ] **P2.2** `trend.rs`: replace ad-hoc history math with `median_duration_ms` from store.
- [ ] **P2.3** Fix remaining warnings; set `#![deny(warnings)]`-equivalent via CI clippy `-D warnings`.

**Exit criteria:** zero build warnings; `stats` shipped with ≥12 new tests; dead-code warning gone.

## 3. Phase 3 — Cover the Untested Modules (Weeks 2–4)

Target: every module ≥70% line coverage. Testable-without-cargo logic extracted into pure functions where needed.

- [ ] **P3.1 benchmark.rs (biggest gap, 392 LoC):**
      - Extract pure helpers: config generation in `setup_*_configs`, `print_comparison` delta math, `BenchmarkStats` aggregation → unit test those
      - Test `record_benchmark_timings` writes correct store entries (tempfile)
      - Integration test running full `run()` against a tiny fixture crate in `target/tmp`
- [ ] **P3.2 dependencies.rs:** transitive-dep scoring against a fixture `Cargo.toml` + metadata JSON snapshot (no network).
- [ ] **P3.3 installer.rs:** inject fake runner instead of `run_command` shelling out; assert tool→command mapping and skip-if-installed logic.
- [ ] **P3.4 watch.rs:** extract command-vector construction; test arg assembly for check/test/clippy sequences.
- [ ] **P3.5 auto.rs (thin):** add tests for wizard answer→config mapping branches (currently 4/~13 fns covered).
- [ ] **P3.6 regression.rs (thin):** budget-violation exit paths, save-vs-compare modes, malformed baseline file.
- [ ] **P3.7 utils.rs:** remaining helpers (`get_os` variants via `cfg` fixtures, tool lookup fallbacks with `PATH` manipulation in temp dirs).

## 4. Phase 4 — Robustness & UX Improvements (Weeks 4–6)

- [ ] **P4.1** `--dry-run` flag on `optimize`, `profile apply`, `linker configure`, `features optimize`:
      print unified diff of would-be changes; no writes. Each gets diff-formatting tests.
- [ ] **P4.2** Error-path hardening: every `run()` handles corrupt/missing `.cargo-accelerate/` state gracefully (tested via corrupted-fixture tests).
- [ ] **P4.3** Snapshot-test generated artifacts (CI YAML, Dockerfile, HTML report) with `insta` to catch template regressions.
- [ ] **P4.4** Property tests for TOML round-trips (`toml_edit` preserve-comments guarantee) using `proptest`.

## 5. Tracking

| Metric | Now | Phase 2 Exit | Phase 3 Exit | Phase 4 Exit |
|---|---|---|---|---|
| Modules w/ zero tests | 5 | **4** (main covered via integration tests) | 0 | 0 |
| Unit tests | 196 | **209** ✅ | ~280 | ~320 |
| Integration tests | 0 | **4** ✅ | 6+ | 8+ |
| Line coverage (measured) | TBD (P1.3) | baseline | ≥70% all modules | ≥80% overall |
| Build warnings | 1 | **0** ✅ | 0 | 0 |

### Progress Log

- **2026-08-22 — Phases 1–2 executed:**
  - CI: added `coverage` job (cargo-llvm-cov → lcov + Codecov); fmt/clippy gates already existed and now pass
    (`cargo clippy --all-targets --all-features -- -D warnings` clean; `cargo fmt --check` clean)
  - Fixed all pre-existing clippy violations across 13 files (derivable Default, sort_by_key,
    needless borrows, collapsible ifs, vec!→array, elided lifetimes, etc.) and rustfmt'd the repo
  - `tests/integration_main.rs`: dispatch smoke tests (--help lists all subcommands, per-subcommand
    help renders, unknown command exits non-zero, --version)
  - New `cargo accelerate stats` command wires up the orphaned `TimingStore` API
    (`query_by_{branch,profile,label}`, `query_since`, `total_count`, `median_duration_ms`,
    `avg_duration_ms`) with filters `--since` (7d/24h/90m or unix ts), `--branch`, `--profile`,
    `--label`; dead-code warning eliminated. Median now interpolates for even-length samples.
  - Remaining: P1.3 coverage baseline (needs one CI run or local `cargo llvm-cov` install), Phase 3 modules

Suggested labels for tracking: `coverage`, `tech-debt`, `phase-1`…`phase-4`.
