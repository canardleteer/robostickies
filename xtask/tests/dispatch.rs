#[path = "../../tests/support/mod.rs"]
mod support;

use clap::Parser;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Output,
    time::Duration,
};
use support::CommandOutput;
use tokio::process::Command;

// Re-execution gives every task its own environment and isolated workspace.
#[test]
#[ignore = "subprocess task dispatcher"]
fn dispatch_fixture() {
    let root = std::env::var_os("FIXTURE_ROOT").unwrap();
    let args: Vec<String> = serde_json::from_str(&std::env::var("FIXTURE_ARGS").unwrap()).unwrap();
    let cli = xtask::Cli::try_parse_from(args).unwrap();
    if let Err(error) = xtask::run_at(Path::new(&root), cli) {
        panic!("{error:#}");
    }
}

async fn tool(directory: &Path) -> PathBuf {
    let executable = directory.join(format!("tool{}", std::env::consts::EXE_SUFFIX));
    let output = Command::new("rustc")
        .args(["--edition=2024", "--crate-name", "xtask_fixture"])
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tool.rs"))
        .arg("-o")
        .arg(&executable)
        .bounded_output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    executable
}

struct Fixture {
    root: tempfile::TempDir,
    tools: PathBuf,
}

impl Fixture {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let tools = root.path().join("tools with spaces");
        fs::create_dir(&tools).unwrap();
        let executable = tool(&tools).await;
        for name in [
            "cargo",
            "rustfmt",
            "cargo-clippy",
            "cargo-machete",
            "cargo-llvm-cov",
            "cargo-tarpaulin",
            "samply",
            "xdg-open",
            "open",
            "rundll32",
        ] {
            fs::copy(
                &executable,
                tools.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)),
            )
            .unwrap();
        }
        Self { root, tools }
    }

    fn program(&self, name: &str) -> PathBuf {
        self.tools
            .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--ignored", "--exact", "dispatch_fixture", "--nocapture"])
            .env("FIXTURE_ROOT", self.root.path())
            .env(
                "FIXTURE_ARGS",
                serde_json::to_string(&[&["xtask"][..], args].concat()).unwrap(),
            );
        self.environment(&mut command);
        command
    }

    fn environment(&self, command: &mut Command) {
        command
            .env("PATH", &self.tools)
            .env("CARGO", self.program("cargo"))
            .env("CARGO_PKG_NAME", "xtask")
            .env("FIXTURE_LOG", self.root.path().join("calls.log"));
    }

    fn log(&self) -> String {
        fs::read_to_string(self.root.path().join("calls.log")).unwrap()
    }

    async fn run(&self, args: &[&str]) -> Output {
        self.command(args).bounded_output().await.unwrap()
    }
}

fn successful(output: &Output) {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[tokio::test]
async fn checks_dispatch_in_order_forward_features_and_stop_after_failures() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let f = Fixture::new().await;
        successful(&f.run(&["ci"]).await);
        assert_eq!(f.log().lines().collect::<Vec<_>>(), [
            "cargo\t--version", "rustfmt\t--version", "cargo\tfmt\t--all\t--\t--check",
            "cargo\tcheck\t--locked\t--workspace\t--all-targets\t--all-features",
            "cargo-clippy\t--version", "cargo\tclippy\t--locked\t--workspace\t--all-targets\t--all-features\t--\t-D\twarnings",
            "cargo-machete\t--version", "cargo\tmachete\t--skip-target-dir",
            "cargo\ttest\t--locked\t--workspace\t--all-features",
        ]);
        fs::remove_file(f.root.path().join("calls.log")).unwrap();
        successful(&f.run(&["check", "--only=test,check,test", "--no-default-features", "--features=alpha,beta"]).await);
        assert!(!f.log().contains("rustfmt"));
        assert!(!f.log().contains("cargo-clippy"));
        assert!(!f.log().contains("machete"));
        assert!(f.log().contains("--no-default-features\t--features\talpha,beta"));
        assert!(f.log().find("cargo\tcheck").unwrap() < f.log().find("cargo\ttest").unwrap());
        fs::remove_file(f.root.path().join("calls.log")).unwrap();
        let output = f.command(&["check"]).env("FIXTURE_FAIL", "fmt").bounded_output().await.unwrap();
        assert!(!output.status.success());
        assert!(!f.log().contains("cargo\tcheck"));
        assert!(String::from_utf8_lossy(&output.stderr).contains("failed with"));
        let output = f.run(&["check", "--exclude=fmt,check,clippy,machete,test"]).await;
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("selection is empty"));
        let output = f.command(&["check"]).env("FIXTURE_UNUSABLE", "cargo").bounded_output().await.unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("installed but unusable"));
        let output = f.command(&["check"]).env("CARGO", f.root.path().join("missing")).bounded_output().await.unwrap();
        assert!(String::from_utf8_lossy(&output.stderr).contains("not installed or not on PATH"));
        successful(&f.command(&["check", "--only=check"]).env_remove("CARGO").bounded_output().await.unwrap());
        successful(&f.command(&["check", "--only=check"]).env_remove("PATH").bounded_output().await.unwrap());
        let mut main = Command::new(env!("CARGO_BIN_EXE_xtask"));
        f.environment(&mut main);
        successful(&main.args(["check", "--only=fmt"]).bounded_output().await.unwrap());
    }).await.expect("check dispatch timed out");
}

#[tokio::test]
async fn machete_is_selected_without_cargo_flags_and_propagates_tool_failures() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let f = Fixture::new().await;
        let manifest = f.root.path().join("Cargo.toml");
        fs::write(&manifest, "# preserve manifests\n").unwrap();
        successful(
            &f.run(&[
                "check",
                "--only=test,machete,machete",
                "--features=alpha",
                "--no-default-features",
            ])
            .await,
        );
        assert_eq!(
            f.log().lines().collect::<Vec<_>>(),
            [
                "cargo\t--version",
                "cargo-machete\t--version",
                "cargo\tmachete\t--skip-target-dir",
                "cargo\ttest\t--locked\t--workspace\t--no-default-features\t--features\talpha",
            ]
        );
        for (variable, value, message) in [
            (
                "FIXTURE_UNUSABLE",
                "cargo-machete",
                "installed but unusable",
            ),
            ("FIXTURE_FINDINGS", "1", "unused dependency"),
            ("FIXTURE_FAIL", "machete", "failed with"),
        ] {
            fs::remove_file(f.root.path().join("calls.log")).unwrap();
            let output = f
                .command(&["check", "--only=machete,test"])
                .env(variable, value)
                .bounded_output()
                .await
                .unwrap();
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains(message));
            assert!(!f.log().contains("cargo\ttest"));
            assert_eq!(
                fs::read_to_string(&manifest).unwrap(),
                "# preserve manifests\n"
            );
        }
        fs::remove_file(f.program("cargo-machete")).unwrap();
        let output = f.run(&["check", "--only=machete"]).await;
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("cargo install --locked cargo-machete --version 0.9.2")
        );
        successful(&f.run(&["check", "--only=check"]).await);
        fs::create_dir(f.program("cargo-machete")).unwrap();
        let output = f.run(&["check", "--only=machete"]).await;
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("could not be launched"));
    })
    .await
    .expect("machete dispatch timed out");
}

#[tokio::test]
async fn coverage_generates_before_opening_and_preserves_or_cleans_reports_on_failure() {
    tokio::time::timeout(Duration::from_secs(60), async {
        for (engine, path) in [
            ("llvm-cov", "target/coverage/llvm-cov/html/index.html"),
            (
                "tarpaulin",
                "target/coverage/tarpaulin/tarpaulin-report.html",
            ),
        ] {
            let f = Fixture::new().await;
            for (task, flags) in [
                ("coverage", vec![]),
                ("coverage", vec!["--open"]),
                ("coverage-open", vec![]),
            ] {
                let args = [
                    vec![task, "--engine", engine, "--no-default-features"],
                    flags,
                ]
                .concat();
                successful(&f.run(&args).await);
                assert_eq!(
                    fs::read_to_string(f.root.path().join(path)).unwrap(),
                    "fresh fixture report"
                );
            }
            let log = f.log();
            assert!(log.contains("--no-default-features"));
            assert!(log.contains("--version"));
            let viewer = if cfg!(target_os = "windows") {
                "rundll32"
            } else if cfg!(target_os = "macos") {
                "open"
            } else {
                "xdg-open"
            };
            assert!(log.rfind("--output-dir").unwrap() < log.rfind(viewer).unwrap());
            let output = f
                .command(&["coverage-open", "--engine", engine])
                .env("FIXTURE_FAIL", viewer)
                .bounded_output()
                .await
                .unwrap();
            assert!(!output.status.success());
            assert!(f.root.path().join(path).is_file());
            assert!(String::from_utf8_lossy(&output.stderr).contains("manually"));
            let output = f
                .command(&["coverage", "--engine", engine])
                .env("FIXTURE_FAIL", engine)
                .bounded_output()
                .await
                .unwrap();
            assert!(!output.status.success());
            assert!(!f.root.path().join(path).exists());
            fs::write(f.root.path().join(path), "stale").unwrap();
            let output = f
                .command(&["coverage", "--engine", engine])
                .env("FIXTURE_NO_REPORT", "1")
                .bounded_output()
                .await
                .unwrap();
            assert!(!output.status.success());
            assert!(!f.root.path().join(path).exists());
            assert!(String::from_utf8_lossy(&output.stderr).contains("no report exists"));
            fs::create_dir(f.root.path().join(path)).unwrap();
            let output = f.run(&["coverage", "--engine", engine]).await;
            assert!(!output.status.success());
            assert!(String::from_utf8_lossy(&output.stderr).contains("removing old report"));
        }
    })
    .await
    .expect("coverage dispatch timed out");
}

#[tokio::test]
async fn profiles_select_exact_artifacts_and_handle_build_recording_and_viewer_failures() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let f = Fixture::new().await;
        let path = f.root.path().join("target/profile/profile.json");
        for args in [
            vec!["profile"],
            vec!["profile", "--open"],
            vec!["profile-open"],
        ] {
            successful(&f.run(&args).await);
            assert_eq!(fs::read_to_string(&path).unwrap(), "fresh fixture report");
        }
        let log = f.log();
        assert!(log.contains("artifacts with spaces"));
        assert!(log.rfind("samply\trecord").unwrap() < log.rfind("samply\tload").unwrap());
        for (variable, mode, expected) in [
            ("FIXTURE_PKGID", "failed", "could not resolve package"),
            ("FIXTURE_PKGID", "non-utf8", "was not UTF-8"),
            ("FIXTURE_BUILD", "malformed", "parsing Cargo JSON"),
            ("FIXTURE_BUILD", "failed", "build failed"),
            ("FIXTURE_BUILD", "missing", "did not report"),
        ] {
            let output = f
                .command(&["profile"])
                .env(variable, mode)
                .bounded_output()
                .await
                .unwrap();
            assert!(!output.status.success());
            assert!(
                String::from_utf8_lossy(&output.stderr).contains(expected),
                "{}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
        let output = f
            .command(&["profile"])
            .env("FIXTURE_FAIL", "record")
            .bounded_output()
            .await
            .unwrap();
        assert!(!output.status.success());
        assert!(!path.exists());
        let output = f
            .command(&["profile-open"])
            .env("FIXTURE_FAIL", "load")
            .bounded_output()
            .await
            .unwrap();
        assert!(!output.status.success());
        assert!(path.is_file());
        fs::remove_file(f.program("samply")).unwrap();
        let output = f.run(&["profile"]).await;
        assert!(String::from_utf8_lossy(&output.stderr).contains("cargo install --locked samply"));
    })
    .await
    .expect("profile dispatch timed out");
}
