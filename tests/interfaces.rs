use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

use rmcp::{
    RoleClient, ServiceExt,
    model::{CallToolRequestParams, ReadResourceRequestParams},
    service::RunningService,
    transport::TokioChildProcess,
};
use serde_json::{Value, json};

fn command(repo: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_robostickies"));
    command.args([
        "--repo",
        repo.to_str().unwrap(),
        "--no-user-config",
        "--json",
    ]);
    command
}

fn init_git(directory: &Path) {
    let output = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(directory)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

async fn start_mcp(directory: &Path) -> RunningService<RoleClient, ()> {
    let mut process = tokio::process::Command::new(env!("CARGO_BIN_EXE_robostickies"));
    process
        .args(["--no-user-config", "mcp"])
        .current_dir(directory);
    ().serve(TokioChildProcess::new(process).unwrap())
        .await
        .unwrap()
}

async fn call_mcp(client: &RunningService<RoleClient, ()>, name: &str, arguments: Value) -> Value {
    let result = client
        .call_tool(
            CallToolRequestParams::new(name.to_owned())
                .with_arguments(arguments.as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    assert_ne!(result.is_error, Some(true), "{result:?}");
    result.structured_content.unwrap()
}

fn cli(repo: &Path, args: &[&str]) -> Value {
    let output = command(repo).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn cli_round_trip_stdin_search_archive_and_publish() {
    let temp = tempfile::tempdir().unwrap();
    cli(temp.path(), &["init"]);
    cli(
        temp.path(),
        &[
            "tags",
            "create",
            "local",
            "--topic",
            "Local observations",
            "--private",
        ],
    );
    let mut child = command(temp.path())
        .args(["add", "-", "--tag", "papercut", "--tag", "private:local"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"CLI stdin observation")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let saved: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(saved["note"]["visibility"], "private");
    let id = saved["note"]["id"].as_str().unwrap();
    assert_eq!(
        cli(temp.path(), &["search", "STDIN", "--scope", "private"])["total"],
        1
    );
    assert_eq!(cli(temp.path(), &["review"])["total"], 0);
    let preview = cli(temp.path(), &["publish", id, "--dry-run"]);
    assert_eq!(preview["dry_run"], true);
    assert_eq!(cli(temp.path(), &["list", "--scope", "public"])["total"], 0);
    let published = cli(temp.path(), &["publish", id, "Reviewed public CLI text"]);
    let public_id = published["note"]["id"].as_str().unwrap();
    assert_eq!(published["note"]["tags"], json!(["papercut"]));
    cli(temp.path(), &["archive", public_id]);
    assert_eq!(cli(temp.path(), &["review"])["total"], 0);
    cli(temp.path(), &["unarchive", public_id]);
    assert_eq!(cli(temp.path(), &["review"])["total"], 1);
    assert_eq!(cli(temp.path(), &["doctor"])["healthy"], true);
    let invalid = command(temp.path())
        .args(["get", "../../escape"])
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(65));
    assert!(invalid.stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&invalid.stderr).unwrap()["error"]["code"],
        "invalid_input"
    );
}

#[tokio::test]
async fn real_stdio_mcp_discovery_mutation_errors_and_cli_interoperation() {
    tokio::time::timeout(Duration::from_secs(40), async {
        let temp = tempfile::tempdir().unwrap();
        let mut process = tokio::process::Command::new(env!("CARGO_BIN_EXE_robostickies"));
        process.args(["--repo", temp.path().to_str().unwrap(), "--no-user-config", "mcp"]);
        let client = ().serve(TokioChildProcess::new(process).unwrap()).await.unwrap();
        let info = client.peer_info().unwrap();
        assert!(info.instructions.as_ref().unwrap().contains("get_context"));
        let tools = client.list_all_tools().await.unwrap();
        for name in ["get_context", "create_note", "create_tag", "publish_note", "list_notes", "review_notes"] {
            assert!(tools.iter().any(|tool| tool.name == name), "missing tool {name}");
        }
        let create_schema = tools.iter().find(|t| t.name == "create_tag").unwrap();
        assert!(create_schema.input_schema.get("properties").unwrap().get("private").is_some());
        assert!(create_schema.output_schema.is_some());
        let before = client.call_tool(CallToolRequestParams::new("get_context").with_arguments(json!({}).as_object().unwrap().clone())).await.unwrap();
        assert_eq!(before.structured_content.unwrap()["initialized"], false);
        let resources = client.list_all_resources().await.unwrap();
        assert!(resources.iter().any(|r| r.uri == "robostickies://guidance"));
        let guidance = client.read_resource(ReadResourceRequestParams::new("robostickies://guidance")).await.unwrap();
        assert!(serde_json::to_string(&guidance).unwrap().contains("AGENTS.md"));
        let init = client.call_tool(CallToolRequestParams::new("init_store").with_arguments(json!({}).as_object().unwrap().clone())).await.unwrap();
        assert_ne!(init.is_error, Some(true));
        let tag = client.call_tool(CallToolRequestParams::new("create_tag").with_arguments(json!({"name": "local", "topic": "Personal observations", "private": true}).as_object().unwrap().clone())).await.unwrap();
        assert_eq!(tag.structured_content.unwrap()["name"], "private:local");
        let saved = client.call_tool(CallToolRequestParams::new("create_note").with_arguments(json!({"body": "MCP private observation", "tags": ["paperclip", "private:local"]}).as_object().unwrap().clone())).await.unwrap();
        let value = saved.structured_content.unwrap();
        assert_eq!(value["note"]["visibility"], "private");
        let id = value["note"]["id"].as_str().unwrap();
        // Keep the MCP service alive while independent CLI processes use the same redb index.
        assert_eq!(cli(temp.path(), &["list", "--scope", "private"])["total"], 1);
        cli(temp.path(), &["add", "CLI alongside a running MCP server", "--tag", "papercut"]);
        let listed = client.call_tool(CallToolRequestParams::new("list_notes").with_arguments(json!({"scope": "all"}).as_object().unwrap().clone())).await.unwrap();
        assert_eq!(listed.structured_content.unwrap()["total"], 2);
        let review = client.call_tool(CallToolRequestParams::new("review_notes").with_arguments(json!({}).as_object().unwrap().clone())).await.unwrap();
        let public = review.structured_content.unwrap();
        assert_eq!(public["total"], 1);
        assert!(!serde_json::to_string(&public).unwrap().contains("MCP private observation"));
        let preview = client.call_tool(CallToolRequestParams::new("publish_note").with_arguments(json!({"id": id, "dry_run": true}).as_object().unwrap().clone())).await.unwrap();
        assert_eq!(preview.structured_content.unwrap()["dry_run"], true);
        let published = client.call_tool(CallToolRequestParams::new("publish_note").with_arguments(json!({"id": id, "body": "Reviewed public MCP text"}).as_object().unwrap().clone())).await.unwrap();
        assert_eq!(published.structured_content.unwrap()["note"]["visibility"], "public");
        let invalid = client.call_tool(CallToolRequestParams::new("create_note").with_arguments(json!({"body": "bad", "tags": ["missing"]}).as_object().unwrap().clone())).await.unwrap();
        assert_eq!(invalid.is_error, Some(true));
        assert_eq!(invalid.structured_content.unwrap()["error"]["code"], "not_found");
        let schema_error = client.call_tool(CallToolRequestParams::new("create_note").with_arguments(json!({"body": 123}).as_object().unwrap().clone())).await.unwrap();
        assert_eq!(schema_error.is_error, Some(true));
        let unknown_field = client.call_tool(CallToolRequestParams::new("list_notes").with_arguments(json!({"scpoe": "public"}).as_object().unwrap().clone())).await.unwrap();
        assert_eq!(unknown_field.is_error, Some(true));
        client.cancel().await.unwrap();
    }).await.expect("MCP integration test timed out");
}

#[tokio::test]
async fn mcp_without_repo_binds_each_process_to_its_startup_git_root() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let temp = tempfile::tempdir().unwrap();
        let first_root = temp.path().join("first");
        let second_root = temp.path().join("second");
        let first_cwd = first_root.join("nested");
        let second_cwd = second_root.join("src");
        fs::create_dir_all(&first_cwd).unwrap();
        fs::create_dir_all(&second_cwd).unwrap();
        init_git(&first_root);
        init_git(&second_root);

        let first = start_mcp(&first_cwd).await;
        let second = start_mcp(&second_cwd).await;
        call_mcp(&first, "init_store", json!({})).await;
        call_mcp(&second, "init_store", json!({})).await;
        assert!(first_root.join(".robostickies/config.toml").is_file());
        assert!(second_root.join(".robostickies/config.toml").is_file());

        init_git(&first_cwd);
        let saved = call_mcp(
            &first,
            "create_note",
            json!({"body": "First repository observation"}),
        )
        .await;
        assert!(
            first_root
                .join(".robostickies")
                .join(saved["note"]["path"].as_str().unwrap())
                .is_file()
        );
        assert!(!first_cwd.join(".robostickies").exists());
        assert!(!second_cwd.join(".robostickies").exists());

        call_mcp(
            &second,
            "create_note",
            json!({"body": "Second repository observation"}),
        )
        .await;
        let first_notes = call_mcp(&first, "list_notes", json!({})).await;
        let second_notes = call_mcp(&second, "list_notes", json!({})).await;
        assert_eq!(first_notes["total"], 1);
        assert_eq!(second_notes["total"], 1);
        assert_eq!(
            first_notes["notes"][0]["body"].as_str().unwrap().trim(),
            "First repository observation"
        );
        assert_eq!(
            second_notes["notes"][0]["body"].as_str().unwrap().trim(),
            "Second repository observation"
        );
        first.cancel().await.unwrap();
        second.cancel().await.unwrap();
    })
    .await
    .expect("MCP directory binding test timed out");
}

#[tokio::test]
async fn mcp_without_repo_uses_its_startup_cwd_outside_git() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path().join("plain-directory");
        fs::create_dir(&cwd).unwrap();
        let client = start_mcp(&cwd).await;
        call_mcp(&client, "init_store", json!({})).await;
        let saved = call_mcp(
            &client,
            "create_note",
            json!({"body": "A note outside Git"}),
        )
        .await;
        assert!(cwd.join(".robostickies/config.toml").is_file());
        assert!(
            cwd.join(".robostickies")
                .join(saved["note"]["path"].as_str().unwrap())
                .is_file()
        );
        assert!(!temp.path().join(".robostickies").exists());
        client.cancel().await.unwrap();
    })
    .await
    .expect("MCP CWD fallback test timed out");
}

#[test]
fn explicit_relative_repo_overrides_git_discovery() {
    let temp = tempfile::tempdir().unwrap();
    init_git(temp.path());
    let selected = temp.path().join("selected");
    fs::create_dir(&selected).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_robostickies"))
        .args(["--no-user-config", "--repo", "selected", "init"])
        .current_dir(temp.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(selected.join(".robostickies/config.toml").is_file());
    assert!(!temp.path().join(".robostickies").exists());
}
