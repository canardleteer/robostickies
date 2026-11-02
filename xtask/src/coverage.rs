use std::path::Path;

use anyhow::{Context, Result};

use crate::process::{CommandSpec, fresh_report, open_report, require, run as run_command};
use crate::{CoverageArgs, CoverageEngine, FeatureArgs, policy};

fn prerequisite(engine: CoverageEngine) -> (CommandSpec, &'static str) {
    match engine {
        CoverageEngine::LlvmCov => (
            CommandSpec::new("cargo-llvm-cov").args(["llvm-cov", "--version"]),
            "Install it with `cargo install --locked cargo-llvm-cov`. LLVM tools: `rustup component add llvm-tools-preview`.",
        ),
        CoverageEngine::Tarpaulin => (
            CommandSpec::new("cargo-tarpaulin").args(["tarpaulin", "--version"]),
            "Install it with `cargo install --locked cargo-tarpaulin`.",
        ),
    }
}

pub(crate) fn run(root: &Path, args: &CoverageArgs, open: bool) -> Result<()> {
    let (probe, install) = prerequisite(args.engine);
    require(root, &probe.program.to_string_lossy(), &probe, install)?;
    fresh_report(
        root,
        report(args.engine),
        || {
            for command in commands(args.engine, &args.features) {
                run_command(root, &command)
                    .with_context(|| format!("coverage generation failed. {install}"))?;
            }
            Ok(())
        },
        |path| {
            if open {
                open_report(root, path)
            } else {
                Ok(())
            }
        },
    )
}

fn report(engine: CoverageEngine) -> &'static str {
    match engine {
        CoverageEngine::LlvmCov => policy::LLVM_COV_REPORT,
        CoverageEngine::Tarpaulin => policy::TARPAULIN_REPORT,
    }
}

fn commands(engine: CoverageEngine, features: &FeatureArgs) -> Vec<CommandSpec> {
    let mut command = CommandSpec::cargo().args([
        match engine {
            CoverageEngine::LlvmCov => "llvm-cov",
            CoverageEngine::Tarpaulin => "tarpaulin",
        },
        "--locked",
        "--workspace",
        "--all-targets",
    ]);
    command.args.extend(features.cargo_args());
    match engine {
        CoverageEngine::LlvmCov => vec![
            CommandSpec::cargo()
                .args(["llvm-cov", "clean", "--workspace"])
                .env("CARGO_LLVM_COV_SETUP", "no"),
            command
                .args([
                    "--include-build-script",
                    "--no-default-ignore-filename-regex",
                    "--ignore-filename-regex",
                    policy::COVERAGE_IGNORES,
                ])
                .args(["--html", "--output-dir", "target/coverage/llvm-cov"])
                .env("CARGO_LLVM_COV_SETUP", "no"),
        ],
        CoverageEngine::Tarpaulin => {
            vec![command.args(["--out", "Html", "--output-dir", "target/coverage/tarpaulin"])]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_probes_follow_cargo_extension_calling_conventions() {
        for (engine, executable, subcommand) in [
            (CoverageEngine::LlvmCov, "cargo-llvm-cov", "llvm-cov"),
            (CoverageEngine::Tarpaulin, "cargo-tarpaulin", "tarpaulin"),
        ] {
            let (probe, install) = prerequisite(engine);
            assert_eq!(
                probe,
                CommandSpec::new(executable).args([subcommand, "--version"])
            );
            assert!(install.contains(&format!("cargo install --locked {executable}")));
        }
    }

    #[test]
    fn engines_preserve_feature_selection_and_separate_outputs() {
        assert_ne!(
            report(CoverageEngine::LlvmCov),
            report(CoverageEngine::Tarpaulin)
        );
        let features = FeatureArgs {
            features: vec!["alpha".into()],
            no_default_features: true,
            ..FeatureArgs::default()
        };
        for engine in [CoverageEngine::LlvmCov, CoverageEngine::Tarpaulin] {
            let commands = commands(engine, &features);
            let generated = commands.last().unwrap();
            for expected in [
                "--locked",
                "--workspace",
                "--all-targets",
                "--no-default-features",
                "--features",
                "alpha",
            ] {
                assert!(generated.args.contains(&expected.into()));
            }
            assert!(!generated.args.contains(&"--all-features".into()));
            assert!(!generated.args.contains(&"--open".into()));
            let output = match engine {
                CoverageEngine::LlvmCov => "target/coverage/llvm-cov",
                CoverageEngine::Tarpaulin => "target/coverage/tarpaulin",
            };
            assert_eq!(generated.args.last().unwrap(), output);
        }
        assert_eq!(
            commands(CoverageEngine::LlvmCov, &FeatureArgs::default())[0],
            CommandSpec::cargo()
                .args(["llvm-cov", "clean", "--workspace"])
                .env("CARGO_LLVM_COV_SETUP", "no")
        );
    }
}
