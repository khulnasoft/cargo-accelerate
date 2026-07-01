mod auto;
mod benchmark;
mod build_audit;
mod cache;
mod ci;
mod cli;
mod daemon;
mod dependencies;
mod doctor;
mod features;
mod graph;
mod installer;
mod linker;
mod optimizer;
mod policy;
mod profile;
mod regression;
mod timings;
mod tracer;
mod trend;
mod utils;
mod watch;
mod workspace;

use clap::Parser;
use cli::{CargoCli, Commands};
use regression::CliOptions;

fn main() -> anyhow::Result<()> {
    let CargoCli::Accelerate(args) = CargoCli::parse();

    match args.command {
        Commands::Doctor => doctor::run()?,
        Commands::Optimize => optimizer::run()?,
        Commands::Benchmark { incremental } => benchmark::run(incremental)?,
        Commands::Cache { action } => cache::run(action)?,
        Commands::Linker => linker::run()?,
        Commands::Workspace => workspace::run()?,
        Commands::Deps => dependencies::run()?,
        Commands::Features { optimize } => {
            let opts = features::FeaturesOptions { optimize };
            features::run(opts)?;
        }
        Commands::Ci {
            enforce_policy,
            budget,
        } => ci::run(ci::CiOptions {
            enforce_policy,
            budget,
        })?,
        Commands::Watch => watch::run()?,
        Commands::Daemon => daemon::run()?,
        Commands::Install => installer::run()?,
        Commands::Graph => graph::run()?,
        Commands::Regression {
            save,
            compare,
            budget,
            threshold,
        } => {
            let opts = CliOptions {
                budget_secs: budget,
                threshold_pct: threshold,
                save_baseline: save,
                compare,
            };
            regression::run(opts)?;
        }
        Commands::Policy { apply: _apply } => {
            policy::run()?;
        }
        Commands::Trace {
            export_json,
            export_html,
            collect_timings,
        } => {
            let opts = tracer::TraceOptions {
                export_json,
                export_html,
                collect_per_crate: collect_timings,
            };
            tracer::run(opts)?;
        }
        Commands::Trend => {
            trend::run()?;
        }
        Commands::Timings {
            command,
            last,
        } => {
            let opts = timings::ListArgs {
                command,
                last,
            };
            timings::run_list(opts)?;
        }
        Commands::Audit {
            skip_size,
            skip_rustflags,
            skip_features,
            skip_parallel,
        } => {
            let opts = build_audit::AuditOptions {
                check_size: !skip_size,
                check_rustflags: !skip_rustflags,
                check_features: !skip_features,
                check_parallel: !skip_parallel,
            };
            build_audit::run(opts)?;
        }
        Commands::Auto {
            skip_cache,
            skip_linker,
            skip_profile,
            skip_ci,
            skip_policy,
            apply,
            non_interactive,
        } => {
            let config = auto::AutoConfig {
                skip_cache,
                skip_linker,
                skip_profile,
                skip_ci,
                skip_policy,
                apply,
                non_interactive,
            };
            auto::run(config)?;
        }
        Commands::Profile { scenario } => match scenario {
            Some(s) => {
                let scenario: profile::Scenario = s.into();
                let root = utils::get_project_root()?;
                let cargo_toml = utils::get_cargo_toml_path(&root);
                profile::apply_profile(&scenario, &cargo_toml)?;
            }
            None => {
                profile::run()?;
            }
        },
    }

    Ok(())
}
