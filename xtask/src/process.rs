use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use anyhow::{Context, Result, ensure};
use serde_json::Value;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CommandSpec {
    pub(crate) program: OsString,
    pub(crate) args: Vec<OsString>,
    env: Vec<(OsString, OsString)>,
    env_remove: Vec<OsString>,
}

impl CommandSpec {
    pub(crate) fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            env: Vec::new(),
            env_remove: Vec::new(),
        }
    }

    pub(crate) fn cargo() -> Self {
        Self::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
    }

    pub(crate) fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub(crate) fn args(mut self, args: impl IntoIterator<Item = impl Into<OsString>>) -> Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub(crate) fn env(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    pub(crate) fn env_remove(mut self, key: impl Into<OsString>) -> Self {
        self.env_remove.push(key.into());
        self
    }

    fn command(&self, root: &Path) -> Command {
        let mut command = Command::new(&self.program);
        if let Some(path) = std::env::var_os("PATH") {
            command.env("PATH", path);
        }
        command
            .args(&self.args)
            .envs(self.env.iter().cloned())
            .current_dir(root);
        for key in &self.env_remove {
            command.env_remove(key);
        }
        command
    }

    fn display(&self) -> String {
        std::iter::once(self.program.as_os_str())
            .chain(self.args.iter().map(OsString::as_os_str))
            .map(|arg| format!("{:?}", arg.to_string_lossy()))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

pub(crate) fn run(root: &Path, spec: &CommandSpec) -> Result<()> {
    eprintln!("+ {}", spec.display());
    let status = spec
        .command(root)
        .status()
        .with_context(|| format!("starting {}", spec.display()))?;
    ensure!(status.success(), "{} failed with {status}", spec.display());
    Ok(())
}

pub(crate) fn require(root: &Path, label: &str, spec: &CommandSpec, guidance: &str) -> Result<()> {
    match spec.command(root).output() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            anyhow::bail!("{label} is not installed or not on PATH. {guidance}");
        }
        Err(error) => anyhow::bail!("{label} could not be launched: {error}. {guidance}"),
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => {
            let diagnostic = concise_output(&output.stdout, &output.stderr);
            anyhow::bail!(
                "{label} is installed but unusable ({}): {diagnostic}. {guidance}",
                output.status
            );
        }
    }
}

fn concise_output(stdout: &[u8], stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(if stderr.is_empty() { stdout } else { stderr });
    let diagnostic = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if diagnostic.is_empty() {
        "no diagnostic output".into()
    } else {
        diagnostic.chars().take(400).collect()
    }
}

/// Remove the old report before generation so opening can never use stale data.
pub(crate) fn fresh_report(
    root: &Path,
    relative: &str,
    generate: impl FnOnce() -> Result<()>,
    open: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    let report = root.join(relative);
    fs::create_dir_all(report.parent().context("report path has no parent")?)
        .with_context(|| format!("creating report directory for {}", report.display()))?;
    match fs::remove_file(&report) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error).with_context(|| format!("removing old report {}", report.display()));
        }
    }
    if let Err(error) = generate() {
        let _ = fs::remove_file(&report);
        return Err(error);
    }
    ensure!(
        report.is_file(),
        "generator succeeded but no report exists at {}",
        report.display()
    );
    let report = report
        .canonicalize()
        .with_context(|| format!("resolving report {}", report.display()))?;
    eprintln!("Report: {}", report.display());
    open(&report)
}

pub(crate) fn open_report(root: &Path, report: &Path) -> Result<()> {
    let command = if cfg!(target_os = "macos") {
        CommandSpec::new("open").arg(report)
    } else if cfg!(target_os = "windows") {
        CommandSpec::new("rundll32").args([
            OsStr::new("url.dll,FileProtocolHandler"),
            report.as_os_str(),
        ])
    } else {
        CommandSpec::new("xdg-open").arg(report)
    };
    run(root, &command).with_context(|| {
        format!(
            "could not open the report; open {} manually or configure your system browser",
            report.display()
        )
    })
}

pub(crate) fn build_example(root: &Path, package: &str, example: &str) -> Result<PathBuf> {
    let identity = CommandSpec::cargo()
        .args(["pkgid", "--locked", "--package", package])
        .command(root)
        .output()
        .context("resolving Cargo package identity")?;
    ensure!(
        identity.status.success(),
        "Cargo could not resolve package {package}: {}",
        concise_output(&identity.stdout, &identity.stderr)
    );
    let package_id =
        String::from_utf8(identity.stdout).context("Cargo package identity was not UTF-8")?;
    let build = CommandSpec::cargo().args([
        "build",
        "--locked",
        "--package",
        package,
        "--example",
        example,
        "--profile",
        "profiling",
        "--message-format=json-render-diagnostics",
    ]);
    eprintln!("+ {}", build.display());
    // output() waits and reaps the child before parsing, including malformed JSON.
    let output = build
        .command(root)
        .stderr(Stdio::inherit())
        .output()
        .context("building profiling example")?;
    let mut executable = None;
    for line in output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let message: Value = serde_json::from_slice(line).context("parsing Cargo JSON message")?;
        if message["reason"] == "compiler-message"
            && let Some(rendered) = message["message"]["rendered"].as_str()
        {
            eprint!("{rendered}");
        }
        if artifact_matches(&message, package_id.trim(), example) {
            executable = message["executable"].as_str().map(PathBuf::from);
        }
    }
    ensure!(
        output.status.success(),
        "profiling example build failed with {}",
        output.status
    );
    executable.context("Cargo did not report the requested profiling example executable")
}

fn artifact_matches(message: &Value, package_id: &str, example: &str) -> bool {
    message["reason"] == "compiler-artifact"
        && message["package_id"] == package_id
        && message["target"]["name"] == example
        && message["target"]["kind"] == serde_json::json!(["example"])
        && message["executable"].is_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::Cell;

    pub(super) fn no_report() -> Result<()> {
        Ok(())
    }

    pub(super) fn rejecting_viewer(path: &Path) -> Result<()> {
        assert_eq!(fs::read_to_string(path)?, "fresh");
        anyhow::bail!("viewer unavailable")
    }

    #[test]
    fn probes_distinguish_missing_and_unusable_tools() {
        let root = tempfile::tempdir().unwrap();
        let missing = require(
            root.path(),
            "fixture",
            &CommandSpec::new(root.path().join("missing")),
            "Install the fixture.",
        )
        .unwrap_err()
        .to_string();
        assert!(missing.contains("not installed or not on PATH"));
        assert!(missing.contains("Install the fixture."));
        let unusable = require(
            root.path(),
            "Cargo",
            &CommandSpec::cargo().arg("__xtask_invalid_subcommand__"),
            "Repair the tool.",
        )
        .unwrap_err()
        .to_string();
        assert!(unusable.contains("installed but unusable"));
        assert!(unusable.contains("__xtask_invalid_subcommand__"));
        assert!(unusable.contains("Repair the tool."));
        assert_eq!(concise_output(b"", b""), "no diagnostic output");
        assert_eq!(
            concise_output(b"stdout", b"loader\n failed"),
            "loader failed"
        );
    }

    #[test]
    fn artifact_identity_includes_package_name_kind_and_executable() {
        let message = json!({"reason": "compiler-artifact", "package_id": "path+file:///repo#robostickies@0.1.0", "target": {"name": "profile_notes", "kind": ["example"]}, "executable": "/custom-target/profile_notes"});
        let package = "path+file:///repo#robostickies@0.1.0";
        assert!(artifact_matches(&message, package, "profile_notes"));
        assert!(!artifact_matches(
            &message,
            "another-package",
            "profile_notes"
        ));
        assert!(!artifact_matches(&message, package, "another-target"));
        for (pointer, value) in [
            ("/reason", json!("build-script-executed")),
            ("/target/kind", json!(["bin"])),
            ("/executable", Value::Null),
        ] {
            let mut wrong = message.clone();
            *wrong.pointer_mut(pointer).unwrap() = value;
            assert!(!artifact_matches(&wrong, package, "profile_notes"));
        }
    }

    #[test]
    fn report_is_regenerated_before_opening_and_retained_on_viewer_failure() {
        let root = tempfile::tempdir().unwrap();
        let report = root.path().join("report.html");
        fs::write(&report, "stale").unwrap();
        let generated = Cell::new(false);
        let error = fresh_report(
            root.path(),
            "report.html",
            || {
                assert!(!report.exists());
                fs::write(&report, "fresh")?;
                generated.set(true);
                Ok(())
            },
            rejecting_viewer,
        )
        .unwrap_err();
        assert!(error.to_string().contains("viewer unavailable"));
        assert!(generated.get());
        assert!(report.is_file());
    }

    #[test]
    fn failed_or_missing_generation_never_opens_a_stale_report() {
        let root = tempfile::tempdir().unwrap();
        let report = root.path().join("report.json");
        for partial in [false, true] {
            fs::write(&report, "stale").unwrap();
            let result = fresh_report(
                root.path(),
                "report.json",
                || {
                    if partial {
                        fs::write(&report, "incomplete")?;
                        anyhow::bail!("generation failed");
                    }
                    no_report()
                },
                rejecting_viewer,
            );
            assert!(result.unwrap_err().to_string().contains(if partial {
                "generation failed"
            } else {
                "no report exists"
            }));
            assert!(!report.exists());
        }
    }
}

#[cfg(test)]
mod failure_tests {
    use super::*;

    #[test]
    fn starting_errors_and_report_parent_failures_include_the_operation() {
        let root = tempfile::tempdir().unwrap();
        let failed = run(root.path(), &CommandSpec::new(root.path().join("missing"))).unwrap_err();
        assert!(failed.to_string().contains("starting"));
        fs::write(root.path().join("blocked"), "keep").unwrap();
        let failed = fresh_report(
            root.path(),
            "blocked/report.html",
            tests::no_report,
            tests::rejecting_viewer,
        )
        .unwrap_err();
        assert!(failed.to_string().contains("creating report directory"));
        assert_eq!(
            fs::read_to_string(root.path().join("blocked")).unwrap(),
            "keep"
        );
        assert!(
            require(
                root.path(),
                "directory",
                &CommandSpec::new(root.path()),
                "Choose an executable."
            )
            .unwrap_err()
            .to_string()
            .contains("could not be launched")
        );
        assert_eq!(concise_output(b"  stdout\nmessage ", b""), "stdout message");
        assert_eq!(concise_output("x".repeat(500).as_bytes(), b"").len(), 400);
    }
}
