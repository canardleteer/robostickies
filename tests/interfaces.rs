mod support;

use support::CommandOutput;
use tokio::process::Command;

use std::{fs, path::Path, process::Stdio, time::Duration};

use rmcp::{
    RoleClient, ServiceExt,
    model::{CallToolRequestParams, ReadResourceRequestParams},
    service::RunningService,
    transport::TokioChildProcess,
};
use serde_json::{Value, json};

// Invoked only as a child of the process-runner and lock tests. No shell or
// installed executable is needed for deterministic failure-path fixtures.
#[test]
#[ignore = "subprocess fixture invoked explicitly by integration tests"]
fn subprocess_fixture() {
    use std::io::{Read, Write};
    let mode = std::env::var("ROBOSTICKIES_TEST_MODE").unwrap();
    let _lock = if mode == "lock" {
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(std::env::var_os("ROBOSTICKIES_TEST_LOCK").unwrap())
            .unwrap();
        file.lock().unwrap();
        Some(file)
    } else {
        None
    };
    println!("ROBOSTICKIES_CHILD_READY");
    std::io::stdout().flush().unwrap();
    match mode.as_str() {
        "hang" | "lock" => loop {
            std::thread::park();
        },
        "echo" => {
            std::io::stderr()
                .write_all(&vec![b'E'; 256 * 1024])
                .unwrap();
            let mut bytes = vec![];
            std::io::stdin().read_to_end(&mut bytes).unwrap();
            std::io::stdout().write_all(&bytes).unwrap();
        }
        "exit" => {
            eprintln!("fixture failure");
            std::process::exit(7);
        }
        _ => panic!("Unknown fixture mode"),
    }
}

fn fixture_command(mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--ignored", "--exact", "subprocess_fixture", "--nocapture"])
        .env("ROBOSTICKIES_TEST_MODE", mode)
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

async fn wait_ready(child: &mut tokio::process::Child) {
    use tokio::io::AsyncBufReadExt;
    tokio::time::timeout(support::PROCESS_TIMEOUT, async {
        let mut reader = tokio::io::BufReader::new(child.stdout.as_mut().unwrap());
        loop {
            let mut line = String::new();
            assert_ne!(
                reader.read_line(&mut line).await.unwrap(),
                0,
                "Child exited before readiness"
            );
            if line.trim() == "ROBOSTICKIES_CHILD_READY" {
                break;
            }
        }
    })
    .await
    .expect("Subprocess readiness timed out");
}

#[tokio::test]
async fn subprocess_deadlines_reap_stalled_children_and_capture_all_streams() {
    let mut stalled = fixture_command("hang").spawn().unwrap();
    wait_ready(&mut stalled).await;
    let error = support::collect(&mut stalled, None, Duration::from_millis(100))
        .await
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(!stalled.try_wait().unwrap().unwrap().success());
    let mut echo = fixture_command("echo").spawn().unwrap();
    let input = vec![b'I'; 256 * 1024];
    let output = support::collect(&mut echo, Some(&input), support::PROCESS_TIMEOUT)
        .await
        .unwrap();
    assert!(output.status.success());
    assert!(
        output
            .stdout
            .windows(input.len())
            .any(|window| window == input)
    );
    assert_eq!(output.stderr, vec![b'E'; 256 * 1024]);
    let failed = fixture_command("exit").bounded_output().await.unwrap();
    assert_eq!(failed.status.code(), Some(7));
    assert!(
        String::from_utf8(failed.stderr)
            .unwrap()
            .contains("fixture failure")
    );
}

fn command(repo: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_robostickies"));
    command.args([
        "--repo",
        repo.to_str().unwrap(),
        "--no-user-config",
        "--json",
    ]);
    command.kill_on_drop(true);
    command
}

async fn init_git(directory: &Path) {
    let output = Command::new("git")
        .args(["init", "--quiet"])
        .current_dir(directory)
        .bounded_output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

async fn start_mcp_args(directory: &Path, args: &[&str]) -> RunningService<RoleClient, ()> {
    let mut process = tokio::process::Command::new(env!("CARGO_BIN_EXE_robostickies"));
    process
        .kill_on_drop(true)
        .args(args)
        .args(["--no-user-config", "mcp"])
        .current_dir(directory);
    ().serve(TokioChildProcess::new(process).unwrap())
        .await
        .unwrap()
}

async fn start_mcp(directory: &Path) -> RunningService<RoleClient, ()> {
    start_mcp_args(directory, &[]).await
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

async fn error_mcp(client: &RunningService<RoleClient, ()>, name: &str, arguments: Value) -> Value {
    let result = client
        .call_tool(
            CallToolRequestParams::new(name.to_owned())
                .with_arguments(arguments.as_object().unwrap().clone()),
        )
        .await
        .unwrap();
    assert_eq!(result.is_error, Some(true), "{result:?}");
    result.structured_content.unwrap()
}

async fn resource_mcp(client: &RunningService<RoleClient, ()>, uri: &str) -> Value {
    let result = client
        .read_resource(ReadResourceRequestParams::new(uri))
        .await
        .unwrap();
    let result = serde_json::to_value(result).unwrap();
    assert_eq!(result["contents"].as_array().unwrap().len(), 1);
    assert_eq!(result["contents"][0]["mimeType"], "application/json");
    serde_json::from_str(result["contents"][0]["text"].as_str().unwrap()).unwrap()
}

async fn cli(repo: &Path, args: &[&str]) -> Value {
    let output = command(repo).args(args).bounded_output().await.unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[tokio::test]
async fn cli_round_trip_stdin_search_archive_and_publish() {
    let temp = tempfile::tempdir().unwrap();
    cli(temp.path(), &["init"]).await;
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
    )
    .await;
    let mut child = command(temp.path())
        .args(["add", "-", "--tag", "papercut", "--tag", "private:local"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let output = support::collect(
        &mut child,
        Some(b"CLI stdin observation"),
        support::PROCESS_TIMEOUT,
    )
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let saved: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(saved["note"]["visibility"], "private");
    let id = saved["note"]["id"].as_str().unwrap();
    assert_eq!(
        cli(temp.path(), &["search", "STDIN", "--scope", "private"]).await["total"],
        1
    );
    assert_eq!(cli(temp.path(), &["review"]).await["total"], 0);
    let revision = saved["note"]["revision"].as_str().unwrap();
    let preview = cli(
        temp.path(),
        &["publish", id, "--dry-run", "--if-match", revision],
    )
    .await;
    assert_eq!(preview["dry_run"], true);
    assert_eq!(
        cli(temp.path(), &["list", "--scope", "public"]).await["total"],
        0
    );
    let published = cli(
        temp.path(),
        &[
            "publish",
            id,
            "Reviewed public CLI text",
            "--if-match",
            revision,
        ],
    )
    .await;
    let public_id = published["note"]["id"].as_str().unwrap();
    assert_eq!(published["note"]["tags"], json!(["papercut"]));
    let archived = cli(
        temp.path(),
        &[
            "archive",
            public_id,
            "--if-match",
            published["note"]["revision"].as_str().unwrap(),
        ],
    )
    .await;
    assert_eq!(cli(temp.path(), &["review"]).await["total"], 0);
    cli(
        temp.path(),
        &[
            "unarchive",
            public_id,
            "--if-match",
            archived["note"]["revision"].as_str().unwrap(),
        ],
    )
    .await;
    assert_eq!(cli(temp.path(), &["review"]).await["total"], 1);
    assert_eq!(cli(temp.path(), &["doctor"]).await["healthy"], true);
    let invalid = command(temp.path())
        .args(["get", "../../escape"])
        .bounded_output()
        .await
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
        process.kill_on_drop(true).args(["--repo", temp.path().to_str().unwrap(), "--no-user-config", "mcp"]);
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
        assert_eq!(cli(temp.path(), &["list", "--scope", "private"]).await["total"], 1);
        cli(temp.path(), &["add", "CLI alongside a running MCP server", "--tag", "papercut"]).await;
        let listed = client.call_tool(CallToolRequestParams::new("list_notes").with_arguments(json!({"scope": "all"}).as_object().unwrap().clone())).await.unwrap();
        assert_eq!(listed.structured_content.unwrap()["total"], 2);
        let review = client.call_tool(CallToolRequestParams::new("review_notes").with_arguments(json!({}).as_object().unwrap().clone())).await.unwrap();
        let public = review.structured_content.unwrap();
        assert_eq!(public["total"], 1);
        assert!(!serde_json::to_string(&public).unwrap().contains("MCP private observation"));
        let preview = client.call_tool(CallToolRequestParams::new("publish_note").with_arguments(json!({"id": id, "dry_run": true, "expected_revision": value["note"]["revision"]}).as_object().unwrap().clone())).await.unwrap();
        assert_eq!(preview.structured_content.unwrap()["dry_run"], true);
        let published = client.call_tool(CallToolRequestParams::new("publish_note").with_arguments(json!({"id": id, "body": "Reviewed public MCP text", "expected_revision": value["note"]["revision"]}).as_object().unwrap().clone())).await.unwrap();
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
async fn every_mcp_tool_rejects_unknown_arguments_before_running() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let temp = tempfile::tempdir().unwrap();
        let client = start_mcp(temp.path()).await;
        call_mcp(&client, "init_store", json!({})).await;
        let saved = call_mcp(
            &client,
            "create_note",
            json!({"body": "Original private fixture", "private": true}),
        )
        .await;
        let id = saved["note"]["id"].as_str().unwrap();
        let original = fs::read(
            temp.path()
                .join(".robostickies")
                .join(saved["note"]["path"].as_str().unwrap()),
        )
        .unwrap();
        let tools = client.list_all_tools().await.unwrap();
        let mut accepted = vec![];
        let mut open_schemas = vec![];
        for tool in &tools {
            let mut arguments = match tool.name.as_ref() {
                "get_note" => json!({"id": id}),
                "get_tag" => json!({"name": "papercut"}),
                "create_note" => json!({"body": "Unexpected note"}),
                "update_note" => json!({"id": id, "body": "Unexpected edit", "expected_revision": saved["note"]["revision"]}),
                "publish_note" => json!({"id": id, "dry_run": true, "expected_revision": saved["note"]["revision"]}),
                "create_tag" => json!({"name": "unexpected", "topic": "Unexpected tag"}),
                "get_context" | "init_store" | "list_notes" | "review_notes" | "list_tags"
                | "get_config" | "validate_store" | "rebuild_index" => json!({}),
                name => panic!("Add a valid input for the new tool {name}"),
            };
            arguments["unexpected_field"] = json!(true);
            let result = client
                .call_tool(
                    CallToolRequestParams::new(tool.name.clone())
                        .with_arguments(arguments.as_object().unwrap().clone()),
                )
                .await
                .unwrap();
            if result.is_error != Some(true) {
                accepted.push(tool.name.clone());
            }
            if tool.input_schema.get("additionalProperties") != Some(&json!(false)) {
                open_schemas.push(tool.name.clone());
            }
        }
        assert!(
            accepted.is_empty(),
            "Unknown fields accepted by {accepted:?}"
        );
        assert!(
            open_schemas.is_empty(),
            "Open input schemas: {open_schemas:?}"
        );
        assert_eq!(
            fs::read(
                temp.path()
                    .join(".robostickies")
                    .join(saved["note"]["path"].as_str().unwrap(),)
            )
            .unwrap(),
            original
        );
        assert!(
            !temp
                .path()
                .join(".robostickies/tags/unexpected.md")
                .exists()
        );
        assert_eq!(call_mcp(&client, "list_notes", json!({})).await["total"], 1);
        for name in ["get_config", "validate_store", "rebuild_index"] {
            let result = client
                .call_tool(CallToolRequestParams::new(name))
                .await
                .unwrap();
            assert_ne!(result.is_error, Some(true), "{name}: {result:?}");
        }
        client.cancel().await.unwrap();
    })
    .await
    .expect("MCP argument validation test timed out");
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
        init_git(&first_root).await;
        init_git(&second_root).await;

        let first = start_mcp(&first_cwd).await;
        let second = start_mcp(&second_cwd).await;
        for (client, root) in [(&first, &first_root), (&second, &second_root)] {
            let context = call_mcp(client, "get_context", json!({})).await;
            assert_eq!(
                context["repository_root"],
                root.canonicalize().unwrap().to_str().unwrap()
            );
            assert_eq!(context["initialized"], false);
        }
        call_mcp(&first, "init_store", json!({})).await;
        call_mcp(&second, "init_store", json!({})).await;
        assert!(first_root.join(".robostickies/config.toml").is_file());
        assert!(second_root.join(".robostickies/config.toml").is_file());

        init_git(&first_cwd).await;
        assert_eq!(
            call_mcp(&first, "get_context", json!({})).await["repository_root"],
            first_root.canonicalize().unwrap().to_str().unwrap()
        );
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
        assert_eq!(
            call_mcp(&client, "get_context", json!({})).await["repository_root"],
            cwd.canonicalize().unwrap().to_str().unwrap()
        );
        call_mcp(&client, "init_store", json!({})).await;
        assert_eq!(
            call_mcp(&client, "get_context", json!({"scope": "private"})).await["repository_root"],
            cwd.canonicalize().unwrap().to_str().unwrap()
        );
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

#[tokio::test]
async fn explicit_relative_repo_overrides_git_discovery() {
    let temp = tempfile::tempdir().unwrap();
    init_git(temp.path()).await;
    let selected = temp.path().join("selected");
    fs::create_dir(&selected).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_robostickies"))
        .args(["--no-user-config", "--repo", "selected", "init"])
        .current_dir(temp.path())
        .bounded_output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(selected.join(".robostickies/config.toml").is_file());
    assert!(!temp.path().join(".robostickies").exists());
}

#[tokio::test]
async fn context_and_all_resources_agree_before_and_after_initialization() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let temp = tempfile::tempdir().unwrap();
        init_git(temp.path()).await;
        let selected = temp.path().join("selected");
        fs::create_dir(&selected).unwrap();
        let root = selected.canonicalize().unwrap();
        let root = root.to_str().unwrap();
        let client = start_mcp_args(temp.path(), &["--repo", "./selected/../selected"]).await;
        let resources = client.list_all_resources().await.unwrap();
        assert_eq!(resources.len(), 4);

        for initialized in [false, true] {
            if initialized {
                call_mcp(&client, "init_store", json!({})).await;
                call_mcp(
                    &client,
                    "create_tag",
                    json!({
                        "name": "local", "topic": "PRIVATE_TOPIC_SENTINEL",
                        "body": "PRIVATE_GUIDANCE_SENTINEL", "read_on_start": true, "private": true
                    }),
                )
                .await;
            }
            let local = call_mcp(&client, "get_context", json!({})).await;
            assert_eq!(local["initialized"], initialized);
            assert_eq!(local["repository_root"], root);
            assert_eq!(resource_mcp(&client, "robostickies://context").await, local);
            let private = call_mcp(&client, "get_context", json!({"scope": "private"})).await;
            assert_eq!(private["repository_root"], root);
            let public = call_mcp(&client, "get_context", json!({"scope": "public"})).await;
            assert!(public.get("repository_root").is_none());
            assert!(public.get("configuration").is_none());
            let encoded = public.to_string();
            for sentinel in [root, "PRIVATE_TOPIC_SENTINEL", "PRIVATE_GUIDANCE_SENTINEL"] {
                assert!(!encoded.contains(sentinel));
            }
            assert_eq!(
                resource_mcp(&client, "robostickies://guidance").await,
                json!({
                    "guidance": public["guidance"], "agents_md": public["agents_md"]
                })
            );
            assert_eq!(
                resource_mcp(&client, "robostickies://config").await,
                call_mcp(&client, "get_config", json!({})).await
            );
            let tags = resource_mcp(&client, "robostickies://tags").await;
            if initialized {
                assert_eq!(tags, call_mcp(&client, "list_tags", json!({})).await);
                for name in ["resolved", "adopted"] {
                    let definition = call_mcp(&client, "get_tag", json!({"name": name})).await;
                    assert_eq!(definition["read_on_start"], true);
                    assert!(
                        local["startup_tags"]
                            .as_array()
                            .unwrap()
                            .contains(&definition)
                    );
                }
            } else {
                assert_eq!(tags["initialized"], false);
                assert_eq!(tags["tags"], json!([]));
                assert_eq!(
                    error_mcp(&client, "list_tags", json!({})).await["error"]["code"],
                    "not_initialized"
                );
            }
            assert_eq!(cli(&selected, &["context"]).await, local);
            for scope in ["all", "private", "public"] {
                let output = Command::new(env!("CARGO_BIN_EXE_robostickies"))
                    .args([
                        "--repo",
                        root,
                        "--no-user-config",
                        "context",
                        "--scope",
                        scope,
                    ])
                    .bounded_output()
                    .await
                    .unwrap();
                assert!(output.status.success());
                let text = String::from_utf8(output.stdout).unwrap();
                assert_eq!(text.contains(root), scope != "public");
            }
            let cli_public = cli(&selected, &["context", "--scope", "public"]).await;
            assert_eq!(cli_public, public);
        }
        assert!(
            call_mcp(&client, "init_store", json!({})).await["created"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(!temp.path().join(".robostickies").exists());
        client.cancel().await.unwrap();
    })
    .await
    .expect("context/resource test timed out");
}

#[tokio::test]
async fn mcp_edits_crlf_nested_frontmatter_and_publishes_a_clean_copy() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let temp = tempfile::tempdir().unwrap();
        let client = start_mcp(temp.path()).await;
        call_mcp(&client, "init_store", json!({})).await;
        let created = call_mcp(&client, "create_note", json!({
            "body": "Original private fixture", "private": true, "tags": ["paperclip"]
        })).await["note"].clone();
        let id = created["id"].as_str().unwrap();
        let directory = temp.path().join(".robostickies");
        let path = directory.join(created["path"].as_str().unwrap());
        let text = format!(
            "---\r\n# header comment\r\nid: {id}\r\ncreated_at: '{}'\r\nupdated_at: '{}'\r\ntags:\r\n  - paperclip # tag comment\r\nstatus: active\r\ncustom:\r\n  nested:\r\n    values: [one, two] # nested comment\r\n  message: |\r\n    first line\r\n    second line\r\n---\r\nOriginal private fixture\r\n",
            created["created_at"].as_str().unwrap(), created["updated_at"].as_str().unwrap()
        );
        fs::write(&path, text).unwrap();
        let source = call_mcp(&client, "get_note", json!({"id": id})).await;
        assert_eq!(source["metadata"]["custom"]["nested"]["values"], json!(["one", "two"]));
        let edited = call_mcp(&client, "update_note", json!({
            "id": id, "body": "Edited private fixture", "expected_revision": source["revision"]
        })).await["note"].clone();
        assert_eq!(edited["metadata"], source["metadata"]);
        assert_eq!(edited["created_at"], source["created_at"]);
        let bytes = fs::read(&path).unwrap();
        let text = String::from_utf8(bytes.clone()).unwrap();
        for comment in ["# header comment", "# tag comment", "# nested comment"] {
            assert!(text.contains(comment));
        }
        assert_eq!(error_mcp(&client, "update_note", json!({
            "id": id, "body": "Stale edit", "expected_revision": source["revision"]
        })).await["error"]["code"], "conflict");
        assert_eq!(fs::read(&path).unwrap(), bytes);
        for dry_run in [true, false] {
            let publication = call_mcp(&client, "publish_note", json!({
                "id": id, "body": "Reviewed public fixture", "dry_run": dry_run,
                "expected_revision": edited["revision"]
            })).await;
            let copy = &publication["note"];
            assert_ne!(copy["id"], id);
            assert_eq!(copy["visibility"], "public");
            assert_eq!(copy["metadata"], json!({}));
            assert_eq!(copy["tags"], json!(["paperclip"]));
            let target = directory.join(copy["path"].as_str().unwrap());
            assert_eq!(target.exists(), !dry_run);
            if !dry_run {
                let public = call_mcp(&client, "get_note", json!({"id": copy["id"]})).await;
                assert_eq!(public["body"].as_str().unwrap().trim(), "Reviewed public fixture");
                assert!(!fs::read_to_string(target).unwrap().contains("custom:"));
            }
            assert_eq!(fs::read(&path).unwrap(), bytes);
            assert_eq!(call_mcp(&client, "get_note", json!({"id": id})).await, edited);
        }
        client.cancel().await.unwrap();
    }).await.expect("MCP frontmatter test timed out");
}

#[tokio::test]
async fn running_mcp_refreshes_sources_recovers_cache_and_shares_concurrent_writes() {
    tokio::time::timeout(Duration::from_secs(45), async {
        let temp = tempfile::tempdir().unwrap();
        let client = start_mcp(temp.path()).await;
        call_mcp(&client, "init_store", json!({})).await;
        let directory = temp.path().join(".robostickies");
        let source =
            call_mcp(&client, "create_note", json!({"body": "Before edit"})).await["note"].clone();
        let path = directory.join(source["path"].as_str().unwrap());
        fs::write(
            &path,
            fs::read_to_string(&path)
                .unwrap()
                .replace("Before edit", "Manual edit"),
        )
        .unwrap();
        assert_eq!(
            call_mcp(&client, "list_notes", json!({"text": "MANUAL EDIT"})).await["total"],
            1
        );
        let tag_path = directory.join("tags/paperclip.md");
        fs::write(
            &tag_path,
            fs::read_to_string(&tag_path).unwrap().replace(
                "What works well and is worth reusing.",
                "Manually updated topic.",
            ),
        )
        .unwrap();
        assert!(
            call_mcp(&client, "get_context", json!({})).await["tags"]
                .as_array()
                .unwrap()
                .iter()
                .any(|tag| tag["topic"] == "Manually updated topic.")
        );

        let public_before = call_mcp(&client, "review_notes", json!({})).await;
        fs::write(
            directory.join("private/notes/bad.md"),
            "PRIVATE_INVALID_SENTINEL",
        )
        .unwrap();
        let public_after = call_mcp(&client, "review_notes", json!({})).await;
        assert_eq!(public_after, public_before);
        assert!(
            call_mcp(&client, "list_notes", json!({"scope": "public"})).await["diagnostics"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            call_mcp(&client, "get_context", json!({"scope": "private"})).await["diagnostics"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            call_mcp(&client, "validate_store", json!({})).await["healthy"],
            false
        );
        assert_eq!(
            error_mcp(&client, "review_notes", json!({"scope": "private"})).await["error"]["code"],
            "invalid_document"
        );
        fs::write(directory.join("notes/bad.md"), "Invalid public source").unwrap();
        assert_eq!(
            error_mcp(&client, "review_notes", json!({})).await["error"]["code"],
            "invalid_document"
        );
        fs::remove_file(directory.join("notes/bad.md")).unwrap();
        fs::remove_file(directory.join("private/notes/bad.md")).unwrap();

        let cache = directory.join("cache/index.redb");
        for damage in ["missing", "corrupt", "incompatible"] {
            fs::remove_file(&cache).unwrap();
            match damage {
                "missing" => {}
                "corrupt" => fs::write(&cache, b"Broken cache").unwrap(),
                "incompatible" => {
                    let db = redb::Database::create(&cache).unwrap();
                    let tx = db.begin_write().unwrap();
                    tx.open_table(redb::TableDefinition::<&str, u64>::new("files_v1"))
                        .unwrap()
                        .insert("old-format", 1)
                        .unwrap();
                    tx.commit().unwrap();
                }
                _ => unreachable!(),
            }
            assert_eq!(call_mcp(&client, "list_notes", json!({})).await["total"], 1);
            assert!(cache.is_file());
        }
        let bytes = fs::read(&path).unwrap();
        assert_eq!(
            call_mcp(&client, "rebuild_index", json!({})).await["healthy"],
            true
        );
        assert_eq!(fs::read(&path).unwrap(), bytes);
        fs::remove_file(&cache).unwrap();
        fs::create_dir(&cache).unwrap();
        let saved = call_mcp(
            &client,
            "create_note",
            json!({"body": "Saved despite cache failure"}),
        )
        .await;
        assert!(!saved["warnings"].as_array().unwrap().is_empty());
        let id = &saved["note"]["id"];
        let saved_read = call_mcp(&client, "get_note", json!({"id": id})).await;
        assert_eq!(saved_read["id"], *id);
        let edited = call_mcp(
            &client,
            "update_note",
            json!({"id": id, "status": "archived", "expected_revision": saved_read["revision"]}),
        )
        .await;
        assert!(!edited["warnings"].as_array().unwrap().is_empty());
        assert_eq!(
            call_mcp(&client, "get_note", json!({"id": id})).await["status"],
            "archived"
        );
        fs::remove_dir(&cache).unwrap();
        assert_eq!(
            call_mcp(&client, "rebuild_index", json!({})).await["notes"],
            2
        );
        fs::remove_file(&path).unwrap();
        assert_eq!(
            call_mcp(&client, "list_notes", json!({"text": "manual edit"})).await["total"],
            0
        );

        let mut writers = tokio::task::JoinSet::new();
        for i in 0..6 {
            let root = temp.path().to_path_buf();
            writers.spawn(async move {
                let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_robostickies"))
                    .kill_on_drop(true)
                    .arg("--repo")
                    .arg(root)
                    .args([
                        "--no-user-config",
                        "--json",
                        "add",
                        &format!("Concurrent CLI fixture {i}"),
                    ])
                    .bounded_output()
                    .await
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                serde_json::from_slice::<Value>(&output.stdout).unwrap()["note"]["id"].clone()
            });
        }
        let saved = call_mcp(
            &client,
            "create_note",
            json!({"body": "Concurrent MCP fixture"}),
        )
        .await;
        let mut ids = vec![saved["note"]["id"].clone()];
        while let Some(result) = writers.join_next().await {
            ids.push(result.unwrap());
        }
        let notes = call_mcp(
            &client,
            "list_notes",
            json!({"text": "concurrent", "scope": "public"}),
        )
        .await;
        assert_eq!(notes["total"], 7);
        let actual: std::collections::BTreeSet<_> = notes["notes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|note| note["id"].as_str().unwrap())
            .collect();
        assert_eq!(actual.len(), 7);
        assert!(ids.iter().all(|id| actual.contains(id.as_str().unwrap())));
        assert_eq!(
            call_mcp(&client, "validate_store", json!({})).await["healthy"],
            true
        );
        client.cancel().await.unwrap();
    })
    .await
    .expect("MCP recovery/concurrency test timed out");
}

#[tokio::test]
async fn review_rejects_paging_filters_consistently_and_covers_internal_pages() {
    tokio::time::timeout(Duration::from_secs(45), async {
        let temp = tempfile::tempdir().unwrap();
        let client = start_mcp(temp.path()).await;
        call_mcp(&client, "init_store", json!({})).await;
        let tools = client.list_all_tools().await.unwrap();
        let schema = &tools.iter().find(|t| t.name == "review_notes").unwrap().input_schema;
        let fields: std::collections::BTreeSet<_> = schema["properties"].as_object().unwrap()
            .keys().map(String::as_str).collect();
        assert_eq!(fields, ["scope", "tags", "exclude_tags", "status", "text", "since", "until"].into());
        for arguments in [json!({"limit": 1}), json!({"cursor": null}), json!({"cursor": "next"})] {
            let result = client.call_tool(CallToolRequestParams::new("review_notes")
                .with_arguments(arguments.as_object().unwrap().clone())).await.unwrap();
            assert_eq!(result.is_error, Some(true));
        }
        let help = command(temp.path()).args(["review", "--help"]).bounded_output().await.unwrap();
        assert!(help.status.success());
        let help = String::from_utf8(help.stdout).unwrap();
        for flag in ["--limit", "--cursor"] {
            assert!(!help.contains(flag));
            let rejected = command(temp.path()).args(["review", flag, "1"]).bounded_output().await.unwrap();
            assert_eq!(rejected.status.code(), Some(2));
            assert!(rejected.stdout.is_empty());
        }

        let directory = temp.path().join(".robostickies");
        // Write a batch of authoritative fixtures once, avoiding an index refresh per file.
        for i in 0..1001 {
            let id = uuid::Uuid::now_v7();
            fs::write(directory.join(format!("notes/{id}.md")), format!(
                "---\nid: {id}\ncreated_at: '2026-01-02T00:00:00Z'\nupdated_at: '2026-01-02T00:00:00Z'\ntags: [papercut]\nstatus: active\n---\nBulk NEEDLE a.*[b] fixture {i}\n"
            )).unwrap();
        }
        let archived = call_mcp(&client, "create_note", json!({"body": "Archived needle", "tags": ["paperclip"]})).await;
        call_mcp(&client, "update_note", json!({"id": archived["note"]["id"], "status": "archived", "expected_revision": archived["note"]["revision"]})).await;
        call_mcp(&client, "create_note", json!({"body": "PRIVATE_NEEDLE_SENTINEL", "private": true, "tags": ["papercut"]})).await;
        let review = call_mcp(&client, "review_notes", json!({})).await;
        assert_eq!(review["total"], 1001);
        assert_eq!(review["notes"].as_array().unwrap().len(), 1001);
        assert_eq!(review, cli(temp.path(), &["review"]).await);
        assert_eq!(review, call_mcp(&client, "review_notes", json!({})).await);
        assert!(!review.to_string().contains("PRIVATE_NEEDLE_SENTINEL"));
        let ids: std::collections::BTreeSet<_> = review["notes"].as_array().unwrap().iter()
            .map(|note| note["id"].as_str().unwrap()).collect();
        assert_eq!(ids.len(), 1001);

        let queries: Vec<(Value, Vec<&str>, usize)> = vec![
            (json!({"tags": ["papercut"], "exclude_tags": ["paperclip"], "text": "needle A.*[B]", "since": "2026-01-02", "until": "2026-01-03"}),
                vec!["review", "--tag", "papercut", "--exclude-tag", "paperclip", "--text", "needle A.*[B]", "--since", "2026-01-02", "--until", "2026-01-03"], 1001),
            (json!({"tags": ["papercut", "paperclip"]}), vec!["review", "--tag", "papercut", "--tag", "paperclip"], 0),
            (json!({"until": "2026-01-02"}), vec!["review", "--until", "2026-01-02"], 0),
            (json!({"exclude_tags": ["papercut"]}), vec!["review", "--exclude-tag", "papercut"], 0),
            (json!({"status": "archived"}), vec!["review", "--status", "archived"], 1),
            (json!({"status": "all"}), vec!["review", "--status", "all"], 1002),
            (json!({"scope": "private"}), vec!["review", "--scope", "private"], 1),
            (json!({"scope": "all"}), vec!["review", "--scope", "all"], 1002),
        ];
        for (query, args, total) in queries {
            let review = call_mcp(&client, "review_notes", query).await;
            assert_eq!(review["total"], total);
            assert_eq!(review, cli(temp.path(), &args).await);
        }
        for query in [json!({"since": "bad-date"}), json!({"since": "2026-02-01", "until": "2026-01-01"})] {
            assert_eq!(error_mcp(&client, "review_notes", query).await["error"]["code"], "invalid_input");
        }
        let first = call_mcp(&client, "list_notes", json!({"limit": 1})).await;
        assert_eq!(first["notes"].as_array().unwrap().len(), 1);
        assert!(first["next_cursor"].is_string());
        let second = call_mcp(&client, "list_notes", json!({"limit": 1, "cursor": first["next_cursor"]})).await;
        assert_ne!(first["notes"][0]["id"], second["notes"][0]["id"]);
        assert_eq!(cli(temp.path(), &["search", "bulk", "--limit", "1"]).await["notes"].as_array().unwrap().len(), 1);
        client.cancel().await.unwrap();
    }).await.expect("complete review test timed out");
}

#[tokio::test]
async fn revision_inputs_are_required_and_rejected_requests_do_not_write() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let temp = tempfile::tempdir().unwrap();
        let client = start_mcp(temp.path()).await;
        call_mcp(&client, "init_store", json!({})).await;
        let source = call_mcp(
            &client,
            "create_note",
            json!({"body": "Original private fixture", "private": true}),
        )
        .await["note"]
            .clone();
        let id = source["id"].as_str().unwrap();
        let path = temp
            .path()
            .join(".robostickies")
            .join(source["path"].as_str().unwrap());
        let original = fs::read(&path).unwrap();
        let tools = client.list_all_tools().await.unwrap();
        for name in ["update_note", "publish_note"] {
            let schema = &tools.iter().find(|t| t.name == name).unwrap().input_schema;
            assert!(
                schema["required"]
                    .as_array()
                    .unwrap()
                    .contains(&json!("expected_revision"))
            );
            assert_eq!(schema["properties"]["expected_revision"]["type"], "string");
            for fields in [
                json!({}),
                json!({"expected_revision": null}),
                json!({"expected_revision": ""}),
                json!({"expected_revision": "stale"}),
            ] {
                for dry_run in [false, true] {
                    let mut args = fields.clone();
                    args["id"] = json!(id);
                    if name == "publish_note" {
                        args["dry_run"] = json!(dry_run);
                    } else {
                        args["body"] = json!("Rejected body");
                    }
                    let result = client
                        .call_tool(
                            CallToolRequestParams::new(name)
                                .with_arguments(args.as_object().unwrap().clone()),
                        )
                        .await
                        .unwrap();
                    assert_eq!(result.is_error, Some(true), "{name}: {result:?}");
                    if fields["expected_revision"] == "" {
                        assert_eq!(
                            result.structured_content.unwrap()["error"]["code"],
                            "invalid_input"
                        );
                    } else if fields["expected_revision"] == "stale" {
                        assert_eq!(
                            result.structured_content.unwrap()["error"]["code"],
                            "conflict"
                        );
                    }
                }
            }
        }
        for action in ["update", "archive", "unarchive", "publish"] {
            let missing = command(temp.path())
                .args([action, id])
                .bounded_output()
                .await
                .unwrap();
            assert_eq!(missing.status.code(), Some(2));
            let blank = command(temp.path())
                .args([action, id, "--if-match", ""])
                .bounded_output()
                .await
                .unwrap();
            assert_eq!(blank.status.code(), Some(65));
            assert_eq!(
                serde_json::from_slice::<Value>(&blank.stderr).unwrap()["error"]["code"],
                "invalid_input"
            );
            let stale = command(temp.path())
                .args([action, id, "--if-match", "stale"])
                .bounded_output()
                .await
                .unwrap();
            assert_eq!(stale.status.code(), Some(1));
            assert_eq!(
                serde_json::from_slice::<Value>(&stale.stderr).unwrap()["error"]["code"],
                "conflict"
            );
        }
        assert_eq!(fs::read(path).unwrap(), original);
        assert_eq!(
            fs::read_dir(temp.path().join(".robostickies/notes"))
                .unwrap()
                .count(),
            0
        );
        let edited = call_mcp(
            &client,
            "update_note",
            json!({
                "id": id, "body": "Intervening change", "expected_revision": source["revision"]
            }),
        )
        .await["note"]
            .clone();
        for dry_run in [false, true] {
            assert_eq!(
                error_mcp(
                    &client,
                    "publish_note",
                    json!({
                        "id": id, "dry_run": dry_run, "expected_revision": source["revision"]
                    })
                )
                .await["error"]["code"],
                "conflict"
            );
        }
        assert_eq!(
            call_mcp(&client, "get_note", json!({"id": id})).await,
            edited
        );
        assert_eq!(
            cli(temp.path(), &["list", "--scope", "public"]).await["total"],
            0
        );
        client.cancel().await.unwrap();
    })
    .await
    .expect("Revision boundary test timed out");
}

#[tokio::test]
async fn public_cli_and_mcp_queries_ignore_local_duplicate_id_conflicts() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let temp = tempfile::tempdir().unwrap();
        let client = start_mcp(temp.path()).await;
        call_mcp(&client, "init_store", json!({})).await;
        let source = call_mcp(
            &client,
            "create_note",
            json!({"body": "Public duplicate fixture"}),
        )
        .await["note"]
            .clone();
        let id = source["id"].as_str().unwrap();
        let before_review = call_mcp(&client, "review_notes", json!({})).await;
        let before_list = call_mcp(&client, "list_notes", json!({"scope": "public"})).await;
        let directory = temp.path().join(".robostickies");
        let path = directory.join(source["path"].as_str().unwrap());
        let original = fs::read_to_string(&path).unwrap();
        let duplicate = directory.join(format!("private/notes/{id}.md"));
        fs::write(
            &duplicate,
            original.replace("Public duplicate fixture", "PRIVATE_DUPLICATE_SENTINEL"),
        )
        .unwrap();
        assert_eq!(
            call_mcp(&client, "review_notes", json!({})).await,
            before_review
        );
        assert_eq!(
            call_mcp(&client, "list_notes", json!({"scope": "public"})).await,
            before_list
        );
        assert_eq!(cli(temp.path(), &["review"]).await, before_review);
        assert_eq!(
            cli(temp.path(), &["list", "--scope", "public"]).await,
            before_list
        );
        assert_eq!(
            error_mcp(&client, "get_note", json!({"id": id})).await["error"]["code"],
            "conflict"
        );
        for scope in ["all", "private"] {
            assert_eq!(
                error_mcp(&client, "review_notes", json!({"scope": scope})).await["error"]["code"],
                "invalid_document"
            );
        }
        let public_context = call_mcp(&client, "get_context", json!({"scope": "public"})).await;
        assert!(public_context["diagnostics"].as_array().unwrap().is_empty());
        assert!(
            !public_context
                .to_string()
                .contains("PRIVATE_DUPLICATE_SENTINEL")
        );
        assert_eq!(fs::read_to_string(path).unwrap(), original);
        fs::remove_file(duplicate).unwrap();
        assert_eq!(
            call_mcp(&client, "get_note", json!({"id": id})).await,
            source
        );
        assert_eq!(
            call_mcp(&client, "review_notes", json!({})).await,
            before_review
        );
        client.cancel().await.unwrap();
    })
    .await
    .expect("Duplicate namespace protocol test timed out");
}

#[tokio::test]
async fn two_mcp_servers_and_cli_cannot_overwrite_each_others_edits() {
    tokio::time::timeout(Duration::from_secs(40), async {
        let temp = tempfile::tempdir().unwrap();
        let first = start_mcp(temp.path()).await;
        let second = start_mcp(temp.path()).await;
        call_mcp(&first, "init_store", json!({})).await;
        for initial_status in ["active", "archived"] {
            let mut source = call_mcp(&first, "create_note", json!({"body": "Shared original"})).await["note"].clone();
            if initial_status == "archived" {
                source = call_mcp(&first, "update_note", json!({"id": source["id"], "status": "archived", "expected_revision": source["revision"]})).await["note"].clone();
            }
            let id = source["id"].as_str().unwrap();
            assert_eq!(call_mcp(&second, "get_note", json!({"id": id})).await, source);
            assert_eq!(cli(temp.path(), &["get", id]).await, source);
            let next_status = if initial_status == "active" { "archived" } else { "active" };
            let barrier = tokio::sync::Barrier::new(3);
            let status_writer = async {
                barrier.wait().await;
                first.call_tool(CallToolRequestParams::new("update_note").with_arguments(json!({
                    "id": id, "status": next_status, "tags": ["paperclip"], "expected_revision": source["revision"]
                }).as_object().unwrap().clone())).await.unwrap()
            };
            let privacy_writer = async {
                barrier.wait().await;
                second.call_tool(CallToolRequestParams::new("update_note").with_arguments(json!({
                    "id": id, "private": true, "expected_revision": source["revision"]
                }).as_object().unwrap().clone())).await.unwrap()
            };
            let cli_writer = async {
                barrier.wait().await;
                command(temp.path()).args(["update", id, "CLI replacement", "--if-match", source["revision"].as_str().unwrap()])
                    .bounded_output().await.unwrap()
            };
            let (status_result, privacy_result, cli_result) = tokio::join!(status_writer, privacy_writer, cli_writer);
            let winners = [status_result.is_error != Some(true), privacy_result.is_error != Some(true), cli_result.status.success()];
            assert_eq!(winners.into_iter().filter(|won| *won).count(), 1);
            for result in [&status_result, &privacy_result] {
                if result.is_error == Some(true) {
                    assert_eq!(result.structured_content.as_ref().unwrap()["error"]["code"], "conflict");
                }
            }
            if !cli_result.status.success() {
                assert_eq!(serde_json::from_slice::<Value>(&cli_result.stderr).unwrap()["error"]["code"], "conflict");
            }
            let current = call_mcp(&first, "get_note", json!({"id": id})).await;
            assert_eq!(current["body"].as_str().unwrap().trim(), if winners[2] { "CLI replacement" } else { "Shared original" });
            assert_eq!(current["status"], if winners[0] { next_status } else { initial_status });
            assert_eq!(current["tags"], if winners[0] { json!(["paperclip"]) } else { json!([]) });
            assert_eq!(current["visibility"], if winners[1] { "private" } else { "public" });
            let directory = temp.path().join(".robostickies");
            assert_eq!(directory.join(format!("notes/{id}.md")).exists(), !winners[1]);
            assert_eq!(directory.join(format!("private/notes/{id}.md")).exists(), winners[1]);
        }
        let request = || CallToolRequestParams::new("create_tag").with_arguments(json!({"name": "same", "topic": "Competing definition"}).as_object().unwrap().clone());
        let (a, b) = tokio::join!(first.call_tool(request()), second.call_tool(request()));
        let a = a.unwrap(); let b = b.unwrap();
        assert_ne!(a.is_error == Some(true), b.is_error == Some(true));
        let loser = if a.is_error == Some(true) { a } else { b };
        assert_eq!(loser.structured_content.unwrap()["error"]["code"], "conflict");
        assert_eq!(call_mcp(&first, "validate_store", json!({})).await["healthy"], true);
        first.cancel().await.unwrap(); second.cancel().await.unwrap();
    }).await.expect("Shared edit race test timed out");
}

#[tokio::test]
async fn lock_contention_is_bounded_and_process_exit_releases_the_lock() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let temp = tempfile::tempdir().unwrap();
        cli(temp.path(), &["init"]).await;
        let client = start_mcp(temp.path()).await;
        let lock_path = temp.path().join(".robostickies/cache/store.lock");
        let mut holder = fixture_command("lock")
            .env("ROBOSTICKIES_TEST_LOCK", &lock_path)
            .spawn()
            .unwrap();
        wait_ready(&mut holder).await;
        let mut blocked_cli = command(temp.path());
        blocked_cli.args(["add", "Blocked CLI write"]);
        let (mcp, cli_result) = tokio::join!(
            error_mcp(&client, "create_note", json!({"body": "Blocked MCP write"})),
            blocked_cli.bounded_output()
        );
        assert_eq!(mcp["error"]["code"], "busy");
        let cli_result = cli_result.unwrap();
        assert_eq!(cli_result.status.code(), Some(75));
        assert_eq!(
            fs::read_dir(temp.path().join(".robostickies/notes"))
                .unwrap()
                .count(),
            0
        );
        tokio::time::timeout(Duration::from_secs(2), holder.kill())
            .await
            .unwrap()
            .unwrap();
        assert!(holder.try_wait().unwrap().is_some());
        assert!(lock_path.is_file());
        call_mcp(
            &client,
            "create_note",
            json!({"body": "MCP after lock release"}),
        )
        .await;
        cli(temp.path(), &["add", "CLI after lock release"]).await;
        assert_eq!(call_mcp(&client, "list_notes", json!({})).await["total"], 2);
        client.cancel().await.unwrap();
    })
    .await
    .expect("Lock contention test timed out");
}
