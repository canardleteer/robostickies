use std::path::Path;

use anyhow::{Result, ensure};

use crate::process::{CommandSpec, require, run as run_command};
use crate::{CheckArgs, CheckStep, FeatureArgs, policy};

fn select_steps(only: &[CheckStep], exclude: &[CheckStep]) -> Result<Vec<CheckStep>> {
    let selected: Vec<_> = policy::CHECK_ORDER
        .into_iter()
        .filter(|step| {
            if only.is_empty() {
                !exclude.contains(step)
            } else {
                only.contains(step)
            }
        })
        .collect();
    ensure!(
        !selected.is_empty(),
        "the check selection is empty; select at least one step"
    );
    Ok(selected)
}

pub(crate) fn run(root: &Path, args: &CheckArgs) -> Result<()> {
    let steps = select_steps(&args.only, &args.exclude)?;
    require(
        root,
        "Cargo",
        &CommandSpec::cargo().arg("--version"),
        "Install a Rust toolchain from https://rustup.rs/.",
    )?;
    for step in steps {
        match step {
            CheckStep::Fmt => require(
                root,
                "rustfmt",
                &CommandSpec::new("rustfmt").arg("--version"),
                "Install it with `rustup component add rustfmt`.",
            )?,
            CheckStep::Clippy => require(
                root,
                "Clippy",
                &CommandSpec::new("cargo-clippy").arg("--version"),
                "Install it with `rustup component add clippy`.",
            )?,
            CheckStep::Machete => require(
                root,
                "cargo-machete",
                &CommandSpec::new("cargo-machete")
                    .arg("--version")
                    .env_remove("CARGO"),
                "Install it with `cargo install --locked cargo-machete --version 0.9.2`.",
            )?,
            _ => {}
        }
        run_command(root, &command_for(step, &args.features))?;
    }
    Ok(())
}

fn command_for(step: CheckStep, features: &FeatureArgs) -> CommandSpec {
    let subcommand = match step {
        CheckStep::Fmt => return CommandSpec::cargo().args(["fmt", "--all", "--", "--check"]),
        CheckStep::Machete => {
            // 0.9.2 uses CARGO_PKG_NAME to distinguish direct and Cargo invocation.
            // cargo run supplies xtask's value, which would misclassify this child.
            return CommandSpec::cargo()
                .args(["machete", "--skip-target-dir"])
                .env_remove("CARGO_PKG_NAME");
        }
        CheckStep::Check => "check",
        CheckStep::Clippy => "clippy",
        CheckStep::Test => "test",
    };
    let mut command = CommandSpec::cargo().args([subcommand, "--locked", "--workspace"]);
    if step != CheckStep::Test {
        command = command.arg("--all-targets");
    }
    command.args.extend(features.cargo_args());
    if step == CheckStep::Clippy {
        command = command.args(["--", "-D", "warnings"]);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_deduplicates_and_retains_canonical_order() {
        assert_eq!(select_steps(&[], &[]).unwrap(), policy::CHECK_ORDER);
        assert_eq!(
            select_steps(&[CheckStep::Test, CheckStep::Fmt, CheckStep::Test], &[]).unwrap(),
            [CheckStep::Fmt, CheckStep::Test]
        );
        assert_eq!(
            select_steps(&[], &[CheckStep::Clippy]).unwrap(),
            [
                CheckStep::Fmt,
                CheckStep::Check,
                CheckStep::Machete,
                CheckStep::Test
            ]
        );
        assert!(select_steps(&[], &policy::CHECK_ORDER).is_err());
    }

    #[test]
    fn commands_preserve_workspace_flags_and_doctests() {
        let features = FeatureArgs::default();
        assert_eq!(
            command_for(CheckStep::Fmt, &features),
            CommandSpec::cargo().args(["fmt", "--all", "--", "--check"])
        );
        for features in [
            features.clone(),
            FeatureArgs {
                features: vec!["alpha".into()],
                no_default_features: true,
                ..FeatureArgs::default()
            },
        ] {
            assert_eq!(
                command_for(CheckStep::Machete, &features),
                CommandSpec::cargo()
                    .args(["machete", "--skip-target-dir"])
                    .env_remove("CARGO_PKG_NAME")
            );
        }
        assert_eq!(
            command_for(CheckStep::Clippy, &features),
            CommandSpec::cargo().args([
                "clippy",
                "--locked",
                "--workspace",
                "--all-targets",
                "--all-features",
                "--",
                "-D",
                "warnings"
            ])
        );
        assert_eq!(
            command_for(CheckStep::Test, &features),
            CommandSpec::cargo().args(["test", "--locked", "--workspace", "--all-features"])
        );
        assert_eq!(
            command_for(
                CheckStep::Check,
                &FeatureArgs {
                    no_default_features: true,
                    ..FeatureArgs::default()
                }
            ),
            CommandSpec::cargo().args([
                "check",
                "--locked",
                "--workspace",
                "--all-targets",
                "--no-default-features"
            ])
        );
    }
}
