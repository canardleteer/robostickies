use std::{
    fs,
    path::{Path, PathBuf},
    process::Output,
};
use tokio::process::Command;

use super::support::CommandOutput;

mod built {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Source {
    pub commit: Option<String>,
    pub dirty: Option<bool>,
}

impl Source {
    pub fn state(&self) -> &'static str {
        match self.dirty {
            Some(true) => "dirty",
            Some(false) => "clean",
            None => "unknown",
        }
    }
}

pub async fn generate(root: &Path, output: &Path) -> Output {
    Command::new(env!("ROBOSTICKIES_BUILD_SCRIPT"))
        .env("CARGO_MANIFEST_DIR", root)
        .env("OUT_DIR", output)
        .envs([
            ("TARGET", built::TARGET),
            ("HOST", built::HOST),
            ("PROFILE", built::PROFILE),
            ("RUSTC", built::RUSTC),
            ("RUSTDOC", built::RUSTDOC),
            ("OPT_LEVEL", built::OPT_LEVEL),
            ("CARGO_CFG_TARGET_ARCH", built::CFG_TARGET_ARCH),
            ("CARGO_CFG_TARGET_ENDIAN", built::CFG_ENDIAN),
            ("CARGO_CFG_TARGET_ENV", built::CFG_ENV),
            ("CARGO_CFG_TARGET_FAMILY", built::CFG_FAMILY),
            ("CARGO_CFG_TARGET_OS", built::CFG_OS),
            ("CARGO_CFG_TARGET_POINTER_WIDTH", built::CFG_POINTER_WIDTH),
        ])
        .env("NUM_JOBS", built::NUM_JOBS.to_string())
        .env("DEBUG", built::DEBUG.to_string())
        .bounded_output()
        .await
        .unwrap()
}

pub async fn source(root: &Path) -> (Source, Vec<PathBuf>) {
    let output = tempfile::tempdir().unwrap();
    let result = generate(root, output.path()).await;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let source = fs::read_to_string(output.path().join("source.rs")).unwrap();
    let mut values = source.lines().map(|line| {
        serde_json::from_str::<String>(line.split_once(" = ").unwrap().1.trim_end_matches(';'))
            .unwrap()
    });
    let commit = values.next().unwrap();
    let state = values.next().unwrap();
    let source = Source {
        commit: (commit != "unknown").then_some(commit),
        dirty: match state.as_str() {
            "clean" => Some(false),
            "dirty" => Some(true),
            "unknown" => None,
            _ => panic!("unexpected source state: {state}"),
        },
    };
    let watches = String::from_utf8(result.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| line.strip_prefix("cargo:rerun-if-changed="))
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .collect();
    (source, watches)
}
