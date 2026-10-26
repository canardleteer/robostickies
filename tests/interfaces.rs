use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ReadResourceRequestParams},
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
