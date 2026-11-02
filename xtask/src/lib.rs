//! Portable, synchronous development tasks for robostickies.

#![forbid(unsafe_code)]

mod check;
mod coverage;
mod policy;
mod process;
mod profile;

use std::{ffi::OsString, path::Path};

use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};

/// Parse repository checks, coverage generation, and local profiling commands.
#[derive(Debug, Parser)]
#[command(name = "cargo xtask", bin_name = "cargo xtask", about)]
pub struct Cli {
    #[command(subcommand)]
    task: Task,
}

#[derive(Debug, Subcommand)]
enum Task {
    /// Run all checks or select a subset in canonical order.
    #[command(visible_alias = "ci")]
    Check(CheckArgs),
    /// Generate coverage and optionally open the fresh report.
    Coverage {
        #[command(flatten)]
        selection: CoverageArgs,
        /// Open the newly generated report.
        #[arg(long)]
        open: bool,
    },
    /// Generate a fresh coverage report and open it.
    CoverageOpen(CoverageArgs),
    /// Record the synthetic notes workload with Samply.
    Profile {
        /// Open the newly recorded profile.
        #[arg(long)]
        open: bool,
    },
    /// Record a fresh profile and open it.
    ProfileOpen,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum CheckStep {
    Fmt,
    Check,
    Clippy,
    Test,
}

#[derive(Debug, Args)]
struct CheckArgs {
    /// Run only these comma-separated checks.
    #[arg(long, value_delimiter = ',', num_args = 1.., conflicts_with = "exclude")]
    only: Vec<CheckStep>,
    /// Run all checks except these comma-separated checks.
    #[arg(long, value_delimiter = ',', num_args = 1.., conflicts_with = "only")]
    exclude: Vec<CheckStep>,
    #[command(flatten)]
    features: FeatureArgs,
}

#[derive(Clone, Debug, Default, Args)]
struct FeatureArgs {
    /// Enable all features (the default if no feature options are supplied).
    #[arg(long, conflicts_with_all = ["features", "no_default_features"])]
    all_features: bool,
    /// Enable Cargo features, separated by commas or spaces.
    #[arg(long, value_delimiter = ',', num_args = 1.., value_parser = nonempty_feature)]
    features: Vec<String>,
    /// Disable default features; may be combined with --features.
    #[arg(long)]
    no_default_features: bool,
}

impl FeatureArgs {
    fn cargo_args(&self) -> Vec<OsString> {
        if self.all_features || (self.features.is_empty() && !self.no_default_features) {
            return vec!["--all-features".into()];
        }
        let mut args = Vec::new();
        if self.no_default_features {
            args.push("--no-default-features".into());
        }
        if !self.features.is_empty() {
            args.extend(["--features".into(), self.features.join(",").into()]);
        }
        args
    }
}

fn nonempty_feature(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        Err("feature names cannot be empty".into())
    } else {
        Ok(value.into())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
enum CoverageEngine {
    #[default]
    LlvmCov,
    Tarpaulin,
}

#[derive(Debug, Args)]
struct CoverageArgs {
    /// Coverage engine to use.
    #[arg(long, value_enum, default_value_t)]
    engine: CoverageEngine,
    #[command(flatten)]
    features: FeatureArgs,
}

/// Execute a parsed task from this checkout's workspace root.
///
/// Checks launch Cargo subprocesses. Coverage and profiling replace their local
/// reports under `target/`; opening a report starts the configured system viewer
/// or Samply's local server. Profiling uses only an isolated synthetic store.
///
/// # Errors
///
/// Returns an error for invalid selections, missing or unusable tools, failed
/// subprocesses, report I/O, or a successful generator that omits its report.
pub fn run(cli: Cli) -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask is a direct workspace member");
    match cli.task {
        Task::Check(args) => check::run(root, &args),
        Task::Coverage { selection, open } => coverage::run(root, &selection, open),
        Task::CoverageOpen(selection) => coverage::run(root, &selection, true),
        Task::Profile { open } => profile::run(root, open),
        Task::ProfileOpen => profile::run(root, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_builds_and_ci_is_visible() {
        let mut command = Cli::command();
        command.clone().debug_assert();
        assert!(command.render_long_help().to_string().contains("ci"));
        for name in ["check", "ci"] {
            let cli = Cli::try_parse_from(["xtask", name, "--only=test,fmt"]).unwrap();
            let Task::Check(args) = cli.task else {
                panic!("expected check")
            };
            assert_eq!(args.only, [CheckStep::Test, CheckStep::Fmt]);
        }
    }

    #[test]
    fn invalid_selections_fail_at_parsing() {
        for args in [
            vec!["--only="],
            vec!["--exclude="],
            vec!["--only=fmt,"],
            vec!["--only=unknown"],
            vec!["--only=all"],
            vec!["--only=fmt", "--exclude=test"],
        ] {
            assert!(Cli::try_parse_from(["xtask", "check"].into_iter().chain(args)).is_err());
        }
    }

    #[test]
    fn features_preserve_cargo_semantics_on_checks_and_coverage() {
        for task in ["check", "coverage", "coverage-open"] {
            for (flags, expected) in [
                (vec![], vec!["--all-features"]),
                (vec!["--all-features"], vec!["--all-features"]),
                (vec!["--no-default-features"], vec!["--no-default-features"]),
                (
                    vec!["--features=alpha,beta"],
                    vec!["--features", "alpha,beta"],
                ),
                (
                    vec!["--no-default-features", "--features", "alpha", "beta"],
                    vec!["--no-default-features", "--features", "alpha,beta"],
                ),
            ] {
                let cli = Cli::try_parse_from(["xtask", task].into_iter().chain(flags)).unwrap();
                let features = match cli.task {
                    Task::Check(args) => args.features,
                    Task::Coverage { selection, .. } | Task::CoverageOpen(selection) => {
                        selection.features
                    }
                    _ => unreachable!(),
                };
                assert_eq!(
                    features.cargo_args(),
                    expected.iter().map(OsString::from).collect::<Vec<_>>()
                );
            }
            for flags in [
                vec!["--all-features", "--features=alpha"],
                vec!["--all-features", "--no-default-features"],
                vec!["--features="],
                vec!["--features=alpha,"],
                vec!["--features", " "],
            ] {
                assert!(Cli::try_parse_from(["xtask", task].into_iter().chain(flags)).is_err());
            }
        }
    }

    #[test]
    fn report_commands_accept_both_opening_forms() {
        for task in ["coverage", "coverage-open"] {
            let mut args = vec!["xtask", task, "--engine=tarpaulin", "--no-default-features"];
            if task == "coverage" {
                args.push("--open");
            }
            let cli = Cli::try_parse_from(args).unwrap();
            let selection = match cli.task {
                Task::Coverage {
                    selection,
                    open: true,
                }
                | Task::CoverageOpen(selection) => selection,
                _ => panic!("expected coverage opening"),
            };
            assert_eq!(selection.engine, CoverageEngine::Tarpaulin);
            assert!(selection.features.no_default_features);
        }
        assert!(matches!(
            Cli::try_parse_from(["xtask", "profile", "--open"])
                .unwrap()
                .task,
            Task::Profile { open: true }
        ));
        assert!(matches!(
            Cli::try_parse_from(["xtask", "profile-open"]).unwrap().task,
            Task::ProfileOpen
        ));
        assert!(Cli::try_parse_from(["xtask", "image"]).is_err());
        assert!(Cli::try_parse_from(["xtask", "mcp-test"]).is_err());
    }
}
