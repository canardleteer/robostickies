use std::{fs, path::Path};

use anyhow::Result;

use crate::policy;
use crate::process::{CommandSpec, build_example, fresh_report, require, run as run_command};

const SAMPLY_GUIDANCE: &str = "Install it with `cargo install --locked samply`. Platform setup: https://github.com/mstange/samply#readme";

pub(crate) fn run(root: &Path, open: bool) -> Result<()> {
    require(
        root,
        "Samply",
        &CommandSpec::new("samply").arg("--version"),
        SAMPLY_GUIDANCE,
    )?;
    if cfg!(target_os = "linux")
        && let Ok(value) = fs::read_to_string("/proc/sys/kernel/perf_event_paranoid")
    {
        eprintln!(
            "kernel.perf_event_paranoid={}; if recording fails with a permission error, consult {SAMPLY_GUIDANCE}",
            value.trim()
        );
    }
    let executable = build_example(root, policy::PROFILE_PACKAGE, policy::PROFILE_EXAMPLE)?;
    let report = root.join(policy::PROFILE_REPORT);
    fresh_report(
        root,
        policy::PROFILE_REPORT,
        || run_command(root, &record_command(&executable, &report)),
        |path| {
            if open {
                run_command(root, &CommandSpec::new("samply").arg("load").arg(path))
            } else {
                Ok(())
            }
        },
    )
}

fn record_command(executable: &Path, report: &Path) -> CommandSpec {
    CommandSpec::new("samply")
        .args(["record", "--save-only", "--no-open", "--output"])
        .arg(report)
        .arg("--")
        .arg(executable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_uses_the_reported_artifact_as_one_argument() {
        let executable = Path::new("custom target/examples/profile_notes");
        let report = Path::new("target/profile/profile.json");
        assert_eq!(
            record_command(executable, report),
            CommandSpec::new("samply")
                .args(["record", "--save-only", "--no-open", "--output"])
                .arg(report)
                .arg("--")
                .arg(executable)
        );
    }
}
