use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "cargo")]
#[command(bin_name = "cargo")]
pub enum CargoCli {
    Accelerate(Cli),
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Analyze the project and environment for optimization opportunities
    Doctor,
    /// Automatically optimize Cargo.toml and .cargo/config.toml
    Optimize,
    /// Benchmark build times before and after optimizations
    Benchmark {
        /// Measure incremental builds instead of full clean builds
        #[arg(long)]
        incremental: bool,
    },
    /// Manage sccache integration
    Cache {
        #[command(subcommand)]
        action: Option<CacheAction>,
    },
    /// Configure fast linkers (mold, lld)
    Linker,
    /// Analyze and optimize workspace structure
    Workspace,
    /// Analyze dependency impact on build times
    Deps,
    /// Audit dependency features and suggest minimal feature sets
    Features {
        /// Apply recommended minimal feature sets to Cargo.toml
        #[arg(long)]
        optimize: bool,
    },
    /// Generate optimized CI workflows
    Ci {
        /// Enforce performance policy checks in CI (fails on regression)
        #[arg(long)]
        enforce_policy: bool,
        /// Maximum allowed build time in seconds (for regression budget)
        #[arg(long)]
        budget: Option<f64>,
    },
    /// Enhanced watch mode
    Watch,
    /// Background daemon for warm builds and cache
    Daemon,
    /// Install missing optimization tools (sccache, mold, etc.)
    Install,
    /// Analyze build graph and find critical path bottlenecks
    Graph,
    /// Track and compare build performance over time
    Regression {
        /// Save a new baseline after measuring
        #[arg(long)]
        save: bool,
        /// Compare current build times against saved baseline
        #[arg(long)]
        compare: bool,
        /// Maximum allowed build time in seconds
        #[arg(long)]
        budget: Option<f64>,
        /// Percentage change threshold for regression warning (default: 10)
        #[arg(long, default_value = "10.0")]
        threshold: f64,
    },
    /// Enforce build-quality policies
    Policy {
        /// Apply policy recommendations automatically
        #[arg(long)]
        apply: bool,
    },
    /// Capture build trace phases and identify bottlenecks
    Trace {
        /// Export trace as JSON
        #[arg(long)]
        export_json: bool,
        /// Export trace as HTML report
        #[arg(long)]
        export_html: bool,
        /// Collect per-crate timing breakdown (runs cargo build --timings=json)
        #[arg(long)]
        collect_timings: bool,
    },
    /// Track and visualize build performance trends over time
    Trend,
    /// Query historical timing data collected during benchmark, regression, and trend runs
    Timings {
        /// Filter by command name (build, check, test, clippy)
        #[arg(long)]
        command: Option<String>,
        /// Show only the last N records
        #[arg(long)]
        last: Option<usize>,
    },
    /// Comprehensive build audit (rustflags, features, size, parallelism)
    Audit {
        /// Skip binary size check
        #[arg(long)]
        skip_size: bool,
        /// Skip rustflags optimization check
        #[arg(long)]
        skip_rustflags: bool,
        /// Skip dependency feature check
        #[arg(long)]
        skip_features: bool,
        /// Skip parallel build check
        #[arg(long)]
        skip_parallel: bool,
    },
    /// Interactive guided automation setup wizard
    Auto {
        /// Skip sccache configuration step
        #[arg(long)]
        skip_cache: bool,
        /// Skip fast linker configuration step
        #[arg(long)]
        skip_linker: bool,
        /// Skip profile tuning step
        #[arg(long)]
        skip_profile: bool,
        /// Skip CI workflow generation step
        #[arg(long)]
        skip_ci: bool,
        /// Skip policy generation step
        #[arg(long)]
        skip_policy: bool,
        /// Apply changes automatically (instead of preview-only)
        #[arg(long)]
        apply: bool,
        /// Run without any interactive prompts
        #[arg(long)]
        non_interactive: bool,
    },
    /// Generate and apply scenario-aware build profiles
    Profile {
        /// Which scenario to use (dev, test, ci, release)
        #[arg(value_parser = clap::value_parser!(ScenarioArg))]
        scenario: Option<ScenarioArg>,
    },
}

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum ScenarioArg {
    Dev,
    Test,
    Ci,
    Release,
}

impl From<ScenarioArg> for crate::profile::Scenario {
    fn from(value: ScenarioArg) -> Self {
        match value {
            ScenarioArg::Dev => crate::profile::Scenario::Dev,
            ScenarioArg::Test => crate::profile::Scenario::Test,
            ScenarioArg::Ci => crate::profile::Scenario::Ci,
            ScenarioArg::Release => crate::profile::Scenario::Release,
        }
    }
}

#[derive(Subcommand)]
pub enum CacheAction {
    Enable,
    Disable,
    Status,
    /// Configure remote cache via sccache-dist
    Remote {
        #[arg(long)]
        enable: bool,
    },
    /// Validate remote cache connectivity
    ValidateRemote {
        /// Print actual env var values and raw command output (may contain secrets)
        #[arg(long)]
        show_values: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_parse_doctor() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "doctor"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => assert!(matches!(args.command, Commands::Doctor)),
        }
    }

    #[test]
    fn test_parse_optimize() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "optimize"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => assert!(matches!(args.command, Commands::Optimize)),
        }
    }

    #[test]
    fn test_parse_graph() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "graph"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => assert!(matches!(args.command, Commands::Graph)),
        }
    }

    #[test]
    fn test_parse_regression() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "regression"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => {
                assert!(matches!(args.command, Commands::Regression { .. }))
            }
        }
    }

    #[test]
    fn test_parse_regression_with_save() {
        let cli =
            CargoCli::try_parse_from(["cargo", "accelerate", "regression", "--save"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => match &args.command {
                Commands::Regression { save, .. } => assert!(*save),
                _ => panic!("expected Regression"),
            },
        }
    }

    #[test]
    fn test_parse_regression_with_budget() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "regression", "--budget", "60"])
            .unwrap();
        match cli {
            CargoCli::Accelerate(args) => match &args.command {
                Commands::Regression { budget, .. } => assert_eq!(*budget, Some(60.0)),
                _ => panic!("expected Regression"),
            },
        }
    }

    #[test]
    fn test_parse_policy() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "policy"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => assert!(matches!(args.command, Commands::Policy { .. })),
        }
    }

    #[test]
    fn test_parse_policy_with_apply() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "policy", "--apply"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => match &args.command {
                Commands::Policy { apply } => assert!(*apply),
                _ => panic!("expected Policy"),
            },
        }
    }

    #[test]
    fn test_parse_profile() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "profile"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => assert!(matches!(args.command, Commands::Profile { .. })),
        }
    }

    #[test]
    fn test_parse_profile_dev() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "profile", "dev"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => match &args.command {
                Commands::Profile { scenario } => assert!(scenario.is_some()),
                _ => panic!("expected Profile"),
            },
        }
    }

    #[test]
    fn test_parse_benchmark() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "benchmark"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => {
                assert!(matches!(args.command, Commands::Benchmark { .. }))
            }
        }
    }

    #[test]
    fn test_parse_benchmark_incremental() {
        let cli =
            CargoCli::try_parse_from(["cargo", "accelerate", "benchmark", "--incremental"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => match &args.command {
                Commands::Benchmark { incremental } => assert!(*incremental),
                _ => panic!("expected Benchmark"),
            },
        }
    }

    #[test]
    fn test_parse_cache_enable() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "cache", "enable"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => match &args.command {
                Commands::Cache { action } => {
                    assert!(matches!(action, Some(CacheAction::Enable)));
                }
                _ => panic!("expected Cache command"),
            },
        }
    }

    #[test]
    fn test_parse_cache_disable() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "cache", "disable"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => match &args.command {
                Commands::Cache { action } => {
                    assert!(matches!(action, Some(CacheAction::Disable)));
                }
                _ => panic!("expected Cache command"),
            },
        }
    }

    #[test]
    fn test_parse_cache_validate_remote() {
        let cli =
            CargoCli::try_parse_from(["cargo", "accelerate", "cache", "validate-remote"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => match &args.command {
                Commands::Cache { action } => {
                    assert!(matches!(
                        action,
                        Some(CacheAction::ValidateRemote { show_values: false })
                    ));
                }
                _ => panic!("expected Cache command"),
            },
        }
    }

    #[test]
    fn test_parse_cache_validate_remote_show_values() {
        let cli = CargoCli::try_parse_from([
            "cargo",
            "accelerate",
            "cache",
            "validate-remote",
            "--show-values",
        ])
        .unwrap();
        match cli {
            CargoCli::Accelerate(args) => match &args.command {
                Commands::Cache { action } => {
                    assert!(matches!(
                        action,
                        Some(CacheAction::ValidateRemote { show_values: true })
                    ));
                }
                _ => panic!("expected Cache command"),
            },
        }
    }

    #[test]
    fn test_parse_cache_defaults_to_none() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "cache"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => match &args.command {
                Commands::Cache { action } => {
                    assert!(action.is_none());
                }
                _ => panic!("expected Cache command"),
            },
        }
    }

    #[test]
    fn test_parse_linker() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "linker"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => assert!(matches!(args.command, Commands::Linker)),
        }
    }

    #[test]
    fn test_parse_workspace() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "workspace"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => assert!(matches!(args.command, Commands::Workspace)),
        }
    }

    #[test]
    fn test_parse_deps() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "deps"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => assert!(matches!(args.command, Commands::Deps)),
        }
    }

    #[test]
    fn test_parse_ci() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "ci"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => {
                assert!(matches!(
                    args.command,
                    Commands::Ci {
                        enforce_policy: _,
                        budget: _
                    }
                ));
                match &args.command {
                    Commands::Ci {
                        enforce_policy,
                        budget,
                    } => {
                        assert!(!enforce_policy);
                        assert!(budget.is_none());
                    }
                    _ => unreachable!(),
                }
            }
        }
    }

    #[test]
    fn test_parse_ci_with_enforce_policy() {
        let cli =
            CargoCli::try_parse_from(["cargo", "accelerate", "ci", "--enforce-policy"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => match &args.command {
                Commands::Ci {
                    enforce_policy,
                    budget,
                } => {
                    assert!(*enforce_policy);
                    assert!(budget.is_none());
                }
                _ => panic!("expected Ci"),
            },
        }
    }

    #[test]
    fn test_parse_ci_with_budget() {
        let cli =
            CargoCli::try_parse_from(["cargo", "accelerate", "ci", "--budget", "120"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => match &args.command {
                Commands::Ci {
                    enforce_policy,
                    budget,
                } => {
                    assert!(!enforce_policy);
                    assert_eq!(*budget, Some(120.0));
                }
                _ => panic!("expected Ci"),
            },
        }
    }

    #[test]
    fn test_parse_ci_with_both_flags() {
        let cli = CargoCli::try_parse_from([
            "cargo",
            "accelerate",
            "ci",
            "--enforce-policy",
            "--budget",
            "300",
        ])
        .unwrap();
        match cli {
            CargoCli::Accelerate(args) => match &args.command {
                Commands::Ci {
                    enforce_policy,
                    budget,
                } => {
                    assert!(*enforce_policy);
                    assert_eq!(*budget, Some(300.0));
                }
                _ => panic!("expected Ci"),
            },
        }
    }

    #[test]
    fn test_parse_watch() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "watch"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => assert!(matches!(args.command, Commands::Watch)),
        }
    }

    #[test]
    fn test_parse_install() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "install"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => assert!(matches!(args.command, Commands::Install)),
        }
    }

    #[test]
    fn test_parse_trace() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "trace"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => assert!(matches!(args.command, Commands::Trace { .. })),
        }
    }

    #[test]
    fn test_parse_trace_with_flags() {
        let cli = CargoCli::try_parse_from([
            "cargo",
            "accelerate",
            "trace",
            "--export-json",
            "--export-html",
        ])
        .unwrap();
        match cli {
            CargoCli::Accelerate(args) => match &args.command {
                Commands::Trace {
                    export_json,
                    export_html,
                    collect_timings,
                } => {
                    assert!(*export_json);
                    assert!(*export_html);
                    assert!(!*collect_timings);
                }
                _ => panic!("expected Trace"),
            },
        }
    }

    #[test]
    fn test_parse_trend() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "trend"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => assert!(matches!(args.command, Commands::Trend)),
        }
    }

    #[test]
    fn test_parse_audit() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "audit"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => assert!(matches!(args.command, Commands::Audit { .. })),
        }
    }

    #[test]
    fn test_parse_audit_with_skip() {
        let cli = CargoCli::try_parse_from([
            "cargo",
            "accelerate",
            "audit",
            "--skip-size",
            "--skip-features",
        ])
        .unwrap();
        match cli {
            CargoCli::Accelerate(args) => match &args.command {
                Commands::Audit {
                    skip_size,
                    skip_features,
                    ..
                } => {
                    assert!(*skip_size);
                    assert!(*skip_features);
                }
                _ => panic!("expected Audit"),
            },
        }
    }

    #[test]
    fn test_parse_auto() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "auto"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => assert!(matches!(args.command, Commands::Auto { .. })),
        }
    }

    #[test]
    fn test_parse_auto_with_flags() {
        let cli = CargoCli::try_parse_from([
            "cargo",
            "accelerate",
            "auto",
            "--apply",
            "--non-interactive",
        ])
        .unwrap();
        match cli {
            CargoCli::Accelerate(args) => match &args.command {
                Commands::Auto {
                    apply,
                    non_interactive,
                    ..
                } => {
                    assert!(*apply);
                    assert!(*non_interactive);
                }
                _ => panic!("expected Auto"),
            },
        }
    }

    #[test]
    fn test_parse_features() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "features"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => {
                assert!(matches!(args.command, Commands::Features { .. }))
            }
        }
    }

    #[test]
    fn test_parse_features_with_optimize() {
        let cli =
            CargoCli::try_parse_from(["cargo", "accelerate", "features", "--optimize"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => match &args.command {
                Commands::Features { optimize } => assert!(*optimize),
                _ => panic!("expected Features"),
            },
        }
    }

    #[test]
    fn test_parse_cache_remote_enable() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "cache", "remote", "--enable"])
            .unwrap();
        match cli {
            CargoCli::Accelerate(args) => match &args.command {
                Commands::Cache { action } => {
                    assert!(matches!(action, Some(CacheAction::Remote { enable: true })));
                }
                _ => panic!("expected Cache command"),
            },
        }
    }

    #[test]
    fn test_parse_cache_remote_without_enable() {
        // Without --enable, it defaults to false
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "cache", "remote"]).unwrap();
        match cli {
            CargoCli::Accelerate(args) => match &args.command {
                Commands::Cache { action } => {
                    assert!(matches!(
                        action,
                        Some(CacheAction::Remote { enable: false })
                    ));
                }
                _ => panic!("expected Cache command"),
            },
        }
    }

    #[test]
    fn test_parse_unknown_command_fails() {
        let cli = CargoCli::try_parse_from(["cargo", "accelerate", "nonexistent"]);
        assert!(cli.is_err());
    }
}
