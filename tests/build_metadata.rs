#[path = "support/build_metadata.rs"]
mod fixture;
mod support;

use std::{fs, path::Path, process::Output, time::Duration};

use support::CommandOutput;
use tokio::process::Command;

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}

async fn git(root: &Path, args: &[&str]) -> String {
    success(
        Command::new("git")
            .current_dir(root)
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .bounded_output()
            .await
            .unwrap(),
    )
}

async fn checkout(root: &Path) -> String {
    git(root, &["init", "--initial-branch=main"]).await;
    fs::write(root.join("tracked.txt"), "original\n").unwrap();
    fs::write(root.join(".gitignore"), ".robostickies/private/\n").unwrap();
    git(root, &["add", "."]).await;
    git(root, &["commit", "-m", "Initial fixture"]).await;
    git(root, &["rev-parse", "HEAD"]).await
}

async fn rebuild(root: &Path, target: &Path) -> String {
    let mut child = Command::new(env!("CARGO"))
        .current_dir(root)
        .args(["run", "--offline", "--quiet"])
        .env("CARGO_TARGET_DIR", target)
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env_remove("LLVM_PROFILE_FILE")
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    success(
        support::collect(&mut child, None, Duration::from_secs(180))
            .await
            .unwrap(),
    )
}

#[tokio::test]
async fn incremental_cargo_builds_refresh_source_identity() {
    tokio::time::timeout(Duration::from_secs(300), async {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("package");
        let target = temp.path().join("target");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir(root.join("build_support")).unwrap();
        fs::write(
            root.join("Cargo.toml"),
            format!(
                r#"
[package]
name = "robostickies"
version = "{}"
edition = "2024"
rust-version = "1.90"
[build-dependencies]
built = {{ version = "=0.8.1", default-features = false, features = ["gix"] }}
serde_json = "1"
"#,
                env!("CARGO_PKG_VERSION")
            ),
        )
        .unwrap();
        let owner = Path::new(env!("CARGO_MANIFEST_DIR"));
        for file in ["Cargo.lock", "build.rs", "build_support/mod.rs"] {
            fs::copy(owner.join(file), root.join(file)).unwrap();
        }
        fs::write(
            root.join("src/main.rs"),
            r#"
mod source { include!(concat!(env!("OUT_DIR"), "/source.rs")); }
fn main() { println!("{} {}", source::COMMIT, source::SOURCE_STATE); }
"#,
        )
        .unwrap();
        // Resolve the small fixture manifest before committing its lockfile.
        assert_eq!(rebuild(&root, &target).await, "unknown unknown");
        let first = checkout(&root).await;
        assert_eq!(rebuild(&root, &target).await, format!("{first} clean"));
        fs::write(root.join("tracked.txt"), "unstaged change\n").unwrap();
        assert_eq!(rebuild(&root, &target).await, format!("{first} dirty"));
        git(&root, &["add", "tracked.txt"]).await;
        assert_eq!(rebuild(&root, &target).await, format!("{first} dirty"));
        git(&root, &["commit", "-m", "Incremental fixture"]).await;
        let second = git(&root, &["rev-parse", "HEAD"]).await;
        assert_ne!(first, second);
        assert_eq!(rebuild(&root, &target).await, format!("{second} clean"));
        fs::create_dir_all(root.join(".robostickies/private")).unwrap();
        fs::write(
            root.join(".robostickies/private/note.md"),
            "private fixture",
        )
        .unwrap();
        fs::write(root.join("untracked"), "fixture").unwrap();
        assert_eq!(rebuild(&root, &target).await, format!("{second} clean"));
        git(&root, &["checkout", "--detach", &first]).await;
        assert_eq!(rebuild(&root, &target).await, format!("{first} clean"));
    })
    .await
    .expect("incremental builds timed out");
}

#[tokio::test]
async fn version_flags_work_without_git_or_a_store_even_with_invalid_repository_options() {
    let root = tempfile::tempdir().unwrap();
    for args in [
        vec!["-V"],
        vec!["--version"],
        vec!["--repo", "/missing/repository", "--version"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_robostickies"))
            .current_dir(root.path())
            .env("PATH", "")
            .args(&args)
            .bounded_output()
            .await
            .unwrap();
        assert!(output.stderr.is_empty());
        let rendered = success(output);
        assert_eq!(
            rendered.lines().next().unwrap(),
            format!("robostickies {}", env!("CARGO_PKG_VERSION"))
        );
        if args == ["-V"] {
            assert_eq!(rendered.lines().count(), 1);
        } else {
            assert_eq!(rendered.lines().count(), 6);
            for label in [
                "Commit: ",
                "Source state: ",
                "Target: ",
                "Compiler: rustc ",
                "Build profile: ",
            ] {
                assert!(rendered.contains(label), "{rendered}");
            }
            assert!(!rendered.contains(env!("CARGO_MANIFEST_DIR")));
        }
        assert!(!root.path().join(".robostickies").exists());
    }
}

#[tokio::test]
async fn source_uses_only_own_checkout_and_tracks_staged_and_unstaged_changes() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let root = tempfile::tempdir().unwrap();
        let hash = checkout(root.path()).await;
        let (source, watches) = fixture::source(root.path()).await;
        assert_eq!(source.commit.as_deref(), Some(hash.as_str()));
        assert_eq!(source.state(), "clean");
        assert!(watches.contains(&root.path().join("tracked.txt")));
        assert!(watches.contains(&root.path().join(".git")));
        fs::create_dir_all(root.path().join(".robostickies/private")).unwrap();
        fs::write(
            root.path().join(".robostickies/private/note.md"),
            "private fixture",
        )
        .unwrap();
        fs::write(root.path().join("untracked"), "fixture").unwrap();
        assert_eq!(fixture::source(root.path()).await, (source, watches));
        fs::write(root.path().join("tracked.txt"), "changed\n").unwrap();
        assert_eq!(fixture::source(root.path()).await.0.state(), "dirty");
        git(root.path(), &["add", "tracked.txt"]).await;
        assert_eq!(fixture::source(root.path()).await.0.state(), "dirty");
        git(root.path(), &["commit", "-m", "Changed fixture"]).await;
        let next = git(root.path(), &["rev-parse", "HEAD"]).await;
        assert_ne!(next, hash);
        assert_eq!(fixture::source(root.path()).await.0.commit, Some(next));
        git(root.path(), &["checkout", "--detach", "HEAD~1"]).await;
        assert_eq!(fixture::source(root.path()).await.0.commit, Some(hash));
        assert_eq!(fixture::source(root.path()).await.0.state(), "clean");
        let consumer = root.path().join("nested-package");
        fs::create_dir(&consumer).unwrap();
        assert_eq!(
            fixture::source(&consumer).await,
            (fixture::Source::default(), vec![consumer.join(".git")])
        );
        git(root.path(), &["pack-refs", "--all"]).await;
        assert_eq!(fixture::source(root.path()).await.0.state(), "clean");
    })
    .await
    .expect("source metadata timed out");
}

#[tokio::test]
async fn source_supports_shallow_clones_linked_worktrees_and_unborn_or_broken_git() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let root = tempfile::tempdir().unwrap();
        let owner = root.path().join("owner");
        fs::create_dir(&owner).unwrap();
        git(&owner, &["init", "--initial-branch=main"]).await;
        assert_eq!(fixture::source(&owner).await.0.state(), "unknown");
        let hash = checkout(&owner).await;
        let linked = root.path().join("linked");
        git(
            &owner,
            &["worktree", "add", "--detach", linked.to_str().unwrap()],
        )
        .await;
        let (source, watches) = fixture::source(&linked).await;
        assert_eq!(source.commit.as_deref(), Some(hash.as_str()));
        assert_eq!(source.state(), "clean");
        assert!(
            watches
                .iter()
                .any(|path| path.canonicalize().unwrap()
                    == owner.join(".git").canonicalize().unwrap())
        );
        assert!(
            watches
                .iter()
                .any(|path| path.ends_with("worktrees/linked"))
        );
        fs::write(linked.join("tracked.txt"), "linked change\n").unwrap();
        assert_eq!(fixture::source(&linked).await.0.state(), "dirty");
        assert_eq!(fixture::source(&owner).await.0.state(), "clean");
        let shallow = root.path().join("shallow");
        let path = owner.to_str().unwrap().replace('\\', "/");
        let url = format!("file:///{}", path.trim_start_matches('/'));
        git(
            root.path(),
            &["clone", "--depth=1", &url, shallow.to_str().unwrap()],
        )
        .await;
        assert_eq!(fixture::source(&shallow).await.0, source);
        for length in [12, 128] {
            fs::write(owner.join(".git/index"), vec![0_u8; length]).unwrap();
            let (broken, watches) = fixture::source(&owner).await;
            assert_eq!(broken.commit.as_deref(), Some(hash.as_str()));
            assert_eq!(broken.state(), "unknown");
            assert!(watches.contains(&owner.join(".git")));
        }
        fs::write(owner.join(".git/HEAD"), "malformed\n").unwrap();
        assert_eq!(fixture::source(&owner).await.0.commit, None);
    })
    .await
    .expect("Git layouts timed out");
}

#[tokio::test]
async fn packaged_metadata_takes_precedence_and_rejects_missing_or_malformed_identity() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let root = tempfile::tempdir().unwrap();
        checkout(root.path()).await;
        let package = root.path().join("package");
        fs::create_dir(&package).unwrap();
        assert_eq!(fixture::source(&package).await.0.state(), "unknown");
        let vcs = package.join(".cargo_vcs_info.json");
        let hash = "1234567890123456789012345678901234567890";
        for dirty in [None, Some(false), Some(true)] {
            let mut value = serde_json::json!({"git":{"sha1":hash},"path_in_vcs":""});
            if let Some(dirty) = dirty {
                value["git"]["dirty"] = dirty.into();
            }
            fs::write(&vcs, value.to_string()).unwrap();
            let (source, watches) = fixture::source(&package).await;
            assert_eq!(source.commit.as_deref(), Some(hash));
            assert_eq!(source.dirty, Some(dirty.unwrap_or(false)));
            assert_eq!(watches, vec![vcs.clone()]);
        }
        for body in [
            "broken",
            "{}",
            r#"{"git":{"sha1":42}}"#,
            r#"{"git":{"sha1":"short"}}"#,
            r#"{"git":{"sha1":"zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"}}"#,
            r#"{"git":{"sha1":"1234567890123456789012345678901234567890","dirty":"no"}}"#,
        ] {
            fs::write(&vcs, body).unwrap();
            assert_eq!(
                fixture::source(&package).await.0,
                fixture::Source::default()
            );
        }
        fs::remove_file(&vcs).unwrap();
        fs::create_dir(&vcs).unwrap();
        assert_eq!(fixture::source(&package).await.0.state(), "unknown");
        // Even a checkout must not replace malformed packaged provenance.
        fs::write(root.path().join(".cargo_vcs_info.json"), "broken").unwrap();
        assert_eq!(fixture::source(root.path()).await.0.state(), "unknown");
        assert!(
            !fixture::generate(&package, &root.path().join("missing-output"))
                .await
                .status
                .success()
        );
    })
    .await
    .expect("packaged metadata timed out");
}
