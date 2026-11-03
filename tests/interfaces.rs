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
        assert_eq!(info.server_info.as_ref().unwrap().name, "robostickies");
        assert_eq!(info.server_info.as_ref().unwrap().version, env!("CARGO_PKG_VERSION"));
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
                | "get_config" | "validate_store" | "rebuild_index" | "upgrade_store" | "version" => json!({}),
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
        for name in ["get_config", "validate_store", "rebuild_index", "version"] {
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
async fn startup_discovery_directs_agents_to_advice_independently_of_health() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let temp = tempfile::tempdir().unwrap();
        let client = start_mcp(temp.path()).await;
        let info = client.peer_info().unwrap();
        let instructions = info.instructions.as_deref().unwrap();
        assert!(
            instructions.contains("upgrade.requirement"),
            "Startup instructions must direct agents to the upgrade requirement"
        );
        assert!(instructions.contains("version"));
        let tools = client.list_all_tools().await.unwrap();
        for name in ["get_context", "get_config", "validate_store"] {
            let description = tools
                .iter()
                .find(|tool| tool.name == name)
                .unwrap()
                .description
                .as_deref()
                .unwrap();
            assert!(description.contains("upgrade"), "{name}: {description}");
        }
        call_mcp(&client, "init_store", json!({})).await;
        let config = temp.path().join(".robostickies/config.toml");
        let original = fs::read_to_string(&config).unwrap();
        for (source, requirement) in [("v1alpha1", "should"), ("v1alpha2", "none")] {
            fs::write(&config, original.replace("v1alpha2", source)).unwrap();
            let context = call_mcp(&client, "get_context", json!({})).await;
            let health = call_mcp(&client, "validate_store", json!({})).await;
            assert_eq!(health["healthy"], true);
            assert_eq!(health["diagnostics"], json!([]));
            assert_eq!(context["upgrade"]["requirement"], requirement);
            assert_eq!(health["upgrade"], context["upgrade"]);
            let guidance = context["guidance"].as_str().unwrap();
            for field in ["upgrade.requirement", "upgrade.message", "deferred"] {
                assert!(
                    guidance.contains(field),
                    "Missing {field} in startup guidance"
                );
            }
            assert!(context["agents_md"].as_str().unwrap().contains("upgrade"));
            let resource = resource_mcp(&client, "robostickies://guidance").await;
            assert_eq!(resource["guidance"], context["guidance"]);
        }
        client.cancel().await.unwrap();
    })
    .await
    .expect("startup advice discovery test timed out");
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
        assert!(!text.replace("\r\n", "").contains('\n'));
        assert_eq!(edited["body"], "Edited private fixture\r\n");
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
        // Exercise mixed writers within the bounded lock budget on slower hosts.
        for i in 0..3 {
            let root = temp.path().to_path_buf();
            writers.spawn(async move {
                let mut busy_retries = 0;
                loop {
                    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_robostickies"))
                        .kill_on_drop(true)
                        .arg("--repo")
                        .arg(&root)
                        .args([
                            "--no-user-config",
                            "--json",
                            "add",
                            &format!("Concurrent CLI fixture {i}"),
                        ])
                        .bounded_output()
                        .await
                        .unwrap();
                    // busy is returned before saving; successful cache warnings must not retry.
                    if output.status.code() == Some(75) && busy_retries < 2 {
                        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
                        assert_eq!(error["error"]["code"], "busy");
                        busy_retries += 1;
                        continue;
                    }
                    assert!(
                        output.status.success(),
                        "{}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                    break serde_json::from_slice::<Value>(&output.stdout).unwrap()["note"]["id"]
                        .clone();
                }
            });
        }
        let mut busy_retries = 0;
        let saved = loop {
            let result = client
                .call_tool(
                    CallToolRequestParams::new("create_note").with_arguments(
                        json!({"body":"Concurrent MCP fixture"})
                            .as_object()
                            .unwrap()
                            .clone(),
                    ),
                )
                .await
                .unwrap();
            if result.is_error == Some(true)
                && result
                    .structured_content
                    .as_ref()
                    .is_some_and(|value| value["error"]["code"] == "busy")
                && busy_retries < 2
            {
                busy_retries += 1;
                continue;
            }
            assert_ne!(result.is_error, Some(true), "{result:?}");
            break result.structured_content.unwrap();
        };
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
        assert_eq!(notes["total"], 4);
        let actual: std::collections::BTreeSet<_> = notes["notes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|note| note["id"].as_str().unwrap())
            .collect();
        assert_eq!(actual.len(), 4);
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

#[tokio::test]
async fn legacy_cli_and_fresh_mcp_upgrade_without_changing_note_envelopes() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let temp = tempfile::tempdir().unwrap();
        let client = start_mcp(temp.path()).await;
        assert_eq!(error_mcp(&client, "upgrade_store", json!({})).await["error"]["code"], "not_initialized");
        call_mcp(&client, "init_store", json!({})).await;
        assert_eq!(call_mcp(&client, "get_context", json!({})).await["data_version"], "v1alpha2");
        let config = temp.path().join(".robostickies/config.toml");
        let legacy = "\u{feff}# Preserve team settings\nformat_version = 1\n\n[tags]\ndefault_private = false\n";
        fs::write(&config, legacy).unwrap();
        let tools = client.list_all_tools().await.unwrap();
        let upgrade = tools.iter().find(|t| t.name == "upgrade_store").unwrap();
        assert!(upgrade.input_schema["properties"]["dry_run"].is_object());
        for name in ["get_context", "get_config", "validate_store"] {
            let value = call_mcp(&client, name, json!({})).await;
            assert_eq!(value["data_version"], "v1alpha1");
            assert_eq!(value["upgrade_available"], true);
        }
        assert_eq!(resource_mcp(&client, "robostickies://config").await["data_version"], "v1alpha1");
        let failed = command(temp.path()).args(["get", "invalid"]).bounded_output().await.unwrap();
        assert_eq!(failed.status.code(), Some(65));
        assert_eq!(serde_json::from_slice::<Value>(&failed.stderr).unwrap()["error"]["code"], "invalid_input");
        let mut human = Command::new(env!("CARGO_BIN_EXE_robostickies"));
        let output = human.args(["--repo", temp.path().to_str().unwrap(), "--no-user-config", "context", "--scope", "public"]).bounded_output().await.unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("robostickies upgrade"));
        assert!(!String::from_utf8_lossy(&output.stdout).contains("Notice:"));
        let output = command(temp.path()).args(["add", "Legacy write", "--private"]).bounded_output().await.unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("v1alpha1"));
        let saved: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(saved.as_object().unwrap().len(), 2);
        assert!(saved["note"].get("data_version").is_none());
        let id = saved["note"]["id"].as_str().unwrap();
        let path = temp.path().join(".robostickies").join(saved["note"]["path"].as_str().unwrap());
        let original = fs::read(&path).unwrap();
        let preview = call_mcp(&client, "upgrade_store", json!({"dry_run": true})).await;
        assert_eq!(preview["source_version"], "v1alpha1");
        assert_eq!(preview["target_version"], "v1alpha2");
        assert_eq!(preview["wrote"], false);
        assert_eq!(preview["dry_run"], true);
        assert_eq!(preview["snapshot"], Value::Null);
        assert_eq!(preview["upgrade"]["requirement"], "should");
        assert_eq!(fs::read_to_string(&config).unwrap(), legacy);
        let preview = cli(temp.path(), &["upgrade", "--dry-run"]).await;
        assert_eq!(preview["wrote"], false);
        assert_eq!(fs::read_to_string(&config).unwrap(), legacy);
        let second = start_mcp(temp.path()).await;
        let (first, second_result, third) = tokio::join!(
            call_mcp(&client, "upgrade_store", json!({})),
            call_mcp(&second, "upgrade_store", json!({})),
            cli(temp.path(), &["upgrade"]),
        );
        assert_eq!([first, second_result, third].iter().filter(|v| v["wrote"] == true).count(), 1);
        assert_eq!(fs::read(&path).unwrap(), original);
        assert_eq!(call_mcp(&client, "get_note", json!({"id":id})).await["revision"], saved["note"]["revision"]);
        for name in ["get_context", "get_config", "validate_store"] {
            let value = call_mcp(&second, name, json!({})).await;
            assert_eq!(value["data_version"], "v1alpha2");
            assert_eq!(value["upgrade_available"], false);
        }
        let output = command(temp.path()).args(["config", "show"]).bounded_output().await.unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        fs::write(&config, "format_version = 1\ndata_version = 'v9'\n").unwrap();
        assert_eq!(error_mcp(&client, "upgrade_store", json!({})).await["error"]["code"], "configuration");
        let output = command(temp.path()).arg("upgrade").bounded_output().await.unwrap();
        assert_eq!(output.status.code(), Some(78));
        assert_eq!(serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["code"], "configuration");
        assert_eq!(fs::read_to_string(&config).unwrap(), "format_version = 1\ndata_version = 'v9'\n");
        client.cancel().await.unwrap();
        second.cancel().await.unwrap();
    }).await.expect("migration interfaces timed out");
}

async fn human(repo: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_robostickies"))
        .args(["--repo", repo.to_str().unwrap(), "--no-user-config"])
        .args(args)
        .bounded_output()
        .await
        .unwrap()
}

#[tokio::test]
async fn human_cli_renders_workflows_diagnostics_and_errors() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let text = human(root, &["context"]).await;
        assert!(text.status.success());
        assert!(String::from_utf8_lossy(&text.stdout).contains("Not initialized"));
        for args in [
            vec!["init", "--agents"],
            vec![
                "tags",
                "create",
                "tools",
                "--topic",
                "Tools",
                "--read-on-start",
            ],
            vec!["tags", "create", "ordinary", "--topic", "Ordinary"],
        ] {
            assert!(human(root, &args).await.status.success());
        }
        let input = root.join("input.md");
        fs::write(&input, "Human file body").unwrap();
        let output = human(
            root,
            &[
                "add",
                "--file",
                input.to_str().unwrap(),
                "--tag",
                "tools",
                "--public",
            ],
        )
        .await;
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("Tags: tools"));
        let public = cli(root, &["add", "Untagged public note"]).await;
        let local = cli(
            root,
            &["add", "Local note", "--private", "--tag", "paperclip"],
        )
        .await;
        let id = public["note"]["id"].as_str().unwrap();
        for args in [
            vec!["get", id],
            vec!["tags", "list"],
            vec!["tags", "get", "tools"],
            vec!["list", "--limit", "1"],
            vec!["search", "absent marker"],
            vec!["review"],
            vec!["context"],
            vec!["config", "show"],
            vec!["doctor"],
            vec!["reindex"],
        ] {
            let output = human(root, &args).await;
            assert!(
                output.status.success(),
                "{args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(!output.stdout.is_empty());
            if args[0] == "list" {
                assert!(String::from_utf8_lossy(&output.stdout).contains("Next cursor:"));
            }
            if args[0] == "get" {
                assert!(String::from_utf8_lossy(&output.stdout).contains("untagged"));
            }
        }
        let revision = public["note"]["revision"].as_str().unwrap();
        assert!(
            human(
                root,
                &[
                    "update",
                    id,
                    "Human edit",
                    "--clear-tags",
                    "--if-match",
                    revision
                ]
            )
            .await
            .status
            .success()
        );
        let updated = cli(root, &["get", id]).await;
        assert!(
            human(
                root,
                &[
                    "archive",
                    id,
                    "--if-match",
                    updated["revision"].as_str().unwrap()
                ]
            )
            .await
            .status
            .success()
        );
        let archived = cli(root, &["get", id]).await;
        assert!(
            human(
                root,
                &[
                    "unarchive",
                    id,
                    "--if-match",
                    archived["revision"].as_str().unwrap()
                ]
            )
            .await
            .status
            .success()
        );
        let local_id = local["note"]["id"].as_str().unwrap();
        let local_revision = local["note"]["revision"].as_str().unwrap();
        for dry in [true, false] {
            let mut args = vec![
                "publish",
                local_id,
                "Reviewed human copy",
                "--clear-tags",
                "--if-match",
                local_revision,
            ];
            if dry {
                args.push("--dry-run");
            }
            let output = human(root, &args).await;
            assert!(output.status.success());
            assert!(String::from_utf8_lossy(&output.stdout).contains(if dry {
                "Preview"
            } else {
                "Published public copy"
            }));
        }
        for (args, code) in [
            (vec!["add"], 65),
            (vec!["add", "--file", "missing.md"], 74),
            (vec!["get", "00000000-0000-0000-0000-000000000001"], 66),
        ] {
            let output = human(root, &args).await;
            assert_eq!(output.status.code(), Some(code));
            assert!(!output.stderr.is_empty());
        }
        let mut child = Command::new(env!("CARGO_BIN_EXE_robostickies"))
            .args([
                "--repo",
                root.to_str().unwrap(),
                "--no-user-config",
                "add",
                "--file",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let output = support::collect(
            &mut child,
            Some(b"File stdin body"),
            support::PROCESS_TIMEOUT,
        )
        .await
        .unwrap();
        assert!(output.status.success());
        fs::write(
            root.join(".robostickies/tags/broken.md"),
            "---\ntopic: ''\n---\n",
        )
        .unwrap();
        for args in [vec!["tags", "list"], vec!["context"], vec!["list"]] {
            let output = human(root, &args).await;
            assert!(output.status.success());
            assert!(String::from_utf8_lossy(&output.stdout).contains("broken.md"));
        }
        let output = human(root, &["doctor"]).await;
        assert_eq!(output.status.code(), Some(65));
        assert!(String::from_utf8_lossy(&output.stdout).contains("problems found"));
        fs::remove_file(root.join(".robostickies/tags/broken.md")).unwrap();
        fs::write(
            root.join(".robostickies/config.toml"),
            "format_version = 1\n",
        )
        .unwrap();
        for (args, expected) in [
            (vec!["upgrade", "--dry-run"], "dry run"),
            (vec!["upgrade"], "upgraded"),
            (vec!["upgrade"], "already current"),
        ] {
            let output = human(root, &args).await;
            assert!(output.status.success());
            assert!(String::from_utf8_lossy(&output.stdout).contains(expected));
        }
        let index = root.join(".robostickies/cache/index.redb");
        fs::remove_file(&index).unwrap();
        fs::create_dir(&index).unwrap();
        let output = human(root, &["add", "Saved with cache warning", "--private"]).await;
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("Warning:"));
    })
    .await
    .expect("human CLI workflows timed out");
}

#[tokio::test]
async fn broken_cli_stdout_and_mcp_startup_have_stable_errors() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let temp = tempfile::tempdir().unwrap();
        cli(temp.path(), &["init"]).await;
        let mut child = command(temp.path())
            .arg("context")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        drop(child.stdout.take());
        let output = support::collect(&mut child, None, support::PROCESS_TIMEOUT)
            .await
            .unwrap();
        assert_eq!(output.status.code(), Some(74));
        let failed = command(temp.path())
            .arg("mcp")
            .bounded_output()
            .await
            .unwrap();
        assert_eq!(failed.status.code(), Some(1));
        assert_eq!(
            serde_json::from_slice::<Value>(&failed.stderr).unwrap()["error"]["code"],
            "mcp"
        );
        let client = start_mcp(temp.path()).await;
        let unknown = client
            .read_resource(ReadResourceRequestParams::new("robostickies://missing"))
            .await
            .unwrap_err();
        assert!(unknown.to_string().contains("Unknown resource URI"));
        client.cancel().await.unwrap();
    })
    .await
    .expect("transport error tests timed out");
}

#[tokio::test]
async fn cli_configuration_failures_preserve_sources_across_layers() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        cli(root, &["init"]).await;
        let config = root.join(".robostickies/config.toml");
        let current = fs::read_to_string(&config).unwrap();
        let private = root.join(".robostickies/private/config.toml");
        let user = root.join("user.toml");
        for path in [&private, &user] {
            for text in [
                "format_version = 2\n",
                "[tags]\nprivate_patterns = ['[']\n",
                "data_version = 'v1alpha2'\n",
                "unexpected = 1\n",
            ] {
                fs::write(path, text).unwrap();
                let mut process = Command::new(env!("CARGO_BIN_EXE_robostickies"));
                process.args(["--repo", root.to_str().unwrap(), "--json"]);
                if path == &user {
                    process.args(["--user-config", user.to_str().unwrap()]);
                } else {
                    process.arg("--no-user-config");
                }
                let output = process
                    .args(["config", "show"])
                    .bounded_output()
                    .await
                    .unwrap();
                assert_eq!(output.status.code(), Some(78));
                assert_eq!(
                    serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["code"],
                    "configuration"
                );
                assert_eq!(fs::read_to_string(&config).unwrap(), current);
                assert_eq!(fs::read_to_string(path).unwrap(), text);
            }
            fs::remove_file(path).unwrap();
        }
        fs::remove_file(&config).unwrap();
        fs::create_dir(&config).unwrap();
        let output = command(root)
            .args(["config", "show"])
            .bounded_output()
            .await
            .unwrap();
        assert_eq!(output.status.code(), Some(78));
        fs::remove_dir(&config).unwrap();
        fs::write(config, current).unwrap();
        assert!(cli(root, &["doctor"]).await["healthy"].as_bool().unwrap());
    })
    .await
    .expect("configuration errors timed out");
}

#[tokio::test]
async fn uninitialized_operations_have_the_same_cli_and_mcp_failure_contract() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let temp = tempfile::tempdir().unwrap();
        let client = start_mcp(temp.path()).await;
        let id = "00000000-0000-0000-0000-000000000001";
        for (name, input, args) in [
            (
                "create_note",
                json!({"body":"Rejected fixture", "private":true}),
                vec!["add", "Rejected fixture", "--private"],
            ),
            ("get_note", json!({"id":id}), vec!["get", id]),
            (
                "update_note",
                json!({"id":id,"expected_revision":"revision"}),
                vec!["update", id, "--if-match", "revision"],
            ),
            (
                "publish_note",
                json!({"id":id,"dry_run":true,"expected_revision":"revision"}),
                vec!["publish", id, "--dry-run", "--if-match", "revision"],
            ),
            ("list_notes", json!({}), vec!["list"]),
            ("review_notes", json!({}), vec!["review"]),
            (
                "create_tag",
                json!({"name":"tools","topic":"Tools","private":true}),
                vec!["tags", "create", "tools", "--topic", "Tools", "--private"],
            ),
            (
                "get_tag",
                json!({"name":"papercut"}),
                vec!["tags", "get", "papercut"],
            ),
            ("list_tags", json!({}), vec!["tags", "list"]),
            ("validate_store", json!({}), vec!["doctor"]),
            ("rebuild_index", json!({}), vec!["reindex"]),
            ("upgrade_store", json!({}), vec!["upgrade"]),
        ] {
            assert_eq!(
                error_mcp(&client, name, input).await["error"]["code"],
                "not_initialized"
            );
            let output = command(temp.path())
                .args(args)
                .bounded_output()
                .await
                .unwrap();
            assert_eq!(output.status.code(), Some(66));
            assert_eq!(
                serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["code"],
                "not_initialized"
            );
            assert!(!temp.path().join(".robostickies").exists());
        }
        fs::create_dir_all(temp.path().join(".robostickies/private")).unwrap();
        fs::write(
            temp.path().join(".robostickies/private/config.toml"),
            "PRIVATE_CONFIG_SENTINEL = [\n",
        )
        .unwrap();
        assert_eq!(
            call_mcp(&client, "get_context", json!({})).await["initialized"],
            false
        );
        assert_eq!(cli(temp.path(), &["context"]).await["initialized"], false);
        client.cancel().await.unwrap();
    })
    .await
    .expect("absent-store interfaces timed out");
}

#[tokio::test]
async fn cache_table_faults_and_missing_definitions_refresh_through_live_mcp() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let temp = tempfile::tempdir().unwrap();
        let client = start_mcp(temp.path()).await;
        call_mcp(&client, "init_store", json!({})).await;
        let saved = call_mcp(
            &client,
            "create_note",
            json!({"body":"MCP recovery fixture", "tags":["papercut"], "private":true}),
        )
        .await;
        let root = temp.path().join(".robostickies");
        let source = root.join(saved["note"]["path"].as_str().unwrap());
        let original = fs::read(&source).unwrap();
        for table in [
            "metadata_v1",
            "files_v1",
            "notes_v1",
            "tags_v1",
            "dates_v1",
            "filters_v1",
        ] {
            let db = redb::Database::open(root.join("cache/index.redb")).unwrap();
            let tx = db.begin_write().unwrap();
            if table == "filters_v1" {
                tx.delete_multimap_table(redb::MultimapTableDefinition::<&str, &str>::new(table))
                    .unwrap();
            } else if table == "metadata_v1" {
                tx.delete_table(redb::TableDefinition::<&str, u64>::new(table))
                    .unwrap();
            } else if table == "dates_v1" {
                tx.delete_table(redb::TableDefinition::<&str, &str>::new(table))
                    .unwrap();
            } else {
                tx.delete_table(redb::TableDefinition::<&str, &[u8]>::new(table))
                    .unwrap();
            }
            tx.open_table(redb::TableDefinition::<u64, u64>::new(table))
                .unwrap()
                .insert(1, 1)
                .unwrap();
            tx.commit().unwrap();
            drop(db);
            assert_eq!(call_mcp(&client, "list_notes", json!({})).await["total"], 1);
            assert_eq!(fs::read(&source).unwrap(), original);
        }
        for (table, key, tool) in [
            (
                "notes_v1",
                saved["note"]["id"].as_str().unwrap(),
                "list_notes",
            ),
            ("tags_v1", "papercut", "list_tags"),
        ] {
            let db = redb::Database::open(root.join("cache/index.redb")).unwrap();
            let tx = db.begin_write().unwrap();
            tx.open_table(redb::TableDefinition::<&str, &[u8]>::new(table))
                .unwrap()
                .insert(key, b"invalid json".as_slice())
                .unwrap();
            tx.commit().unwrap();
            drop(db);
            assert_eq!(
                error_mcp(&client, tool, json!({})).await["error"]["code"],
                "index"
            );
            assert_eq!(
                error_mcp(&client, "validate_store", json!({})).await["error"]["code"],
                "index"
            );
            if table == "tags_v1" {
                assert_eq!(
                    error_mcp(&client, "get_context", json!({})).await["error"]["code"],
                    "index"
                );
            }
            assert_eq!(
                call_mcp(&client, "rebuild_index", json!({})).await["healthy"],
                true
            );
            assert_eq!(fs::read(&source).unwrap(), original);
        }
        let db = redb::Database::open(root.join("cache/index.redb")).unwrap();
        let tx = db.begin_write().unwrap();
        tx.open_table(redb::TableDefinition::<&str, &str>::new("dates_v1"))
            .unwrap()
            .insert(
                "2026-01-01T00:00:00.000000000Z/00000000-0000-0000-0000-000000000001",
                "00000000-0000-0000-0000-000000000001",
            )
            .unwrap();
        tx.commit().unwrap();
        drop(db);
        assert_eq!(
            call_mcp(&client, "list_notes", json!({"status":"all"})).await["total"],
            1
        );
        fs::remove_file(root.join("tags/papercut.md")).unwrap();
        let local = call_mcp(&client, "list_notes", json!({})).await;
        assert!(
            local["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["code"] == "unknown_tag")
        );
        assert_eq!(
            call_mcp(&client, "list_notes", json!({"scope":"public"})).await["diagnostics"],
            json!([])
        );
        client.cancel().await.unwrap();
    })
    .await
    .expect("MCP cache faults timed out");
}

#[tokio::test]
async fn mcp_git_probes_have_closed_input_and_captured_output() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let temp = tempfile::tempdir().unwrap();
        cli(temp.path(), &["init"]).await;
        let tools = temp.path().join("tools with spaces");
        fs::create_dir(&tools).unwrap();
        let git = tools.join(format!("git{}", std::env::consts::EXE_SUFFIX));
        let compiled = Command::new("rustc")
            .args(["--edition=2024", "--crate-name", "git_probe_fixture"])
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/git.rs"))
            .arg("-o")
            .arg(git)
            .bounded_output()
            .await
            .unwrap();
        assert!(
            compiled.status.success(),
            "{}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let path = std::env::join_paths(std::iter::once(tools).chain(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        )))
        .unwrap();
        let mut process = command(temp.path());
        process
            .arg("mcp")
            .env("PATH", path)
            .current_dir(temp.path());
        let client = ().serve(TokioChildProcess::new(process).unwrap()).await.unwrap();
        let report = tokio::time::timeout(
            support::PROCESS_TIMEOUT,
            client.call_tool(
                CallToolRequestParams::new("validate_store")
                    .with_arguments(json!({}).as_object().unwrap().clone()),
            ),
        )
        .await;
        tokio::time::timeout(Duration::from_secs(2), client.cancel())
            .await
            .unwrap()
            .unwrap();
        let report = report
            .expect("Git probe waited for protocol input")
            .unwrap();
        assert_ne!(report.is_error, Some(true));
        assert_eq!(report.structured_content.unwrap()["healthy"], true);
    })
    .await
    .expect("MCP Git probe test timed out");
}

#[tokio::test]
async fn doctor_identifies_missing_private_exclusion_while_cache_remains_ignored() {
    tokio::time::timeout(Duration::from_secs(60), async {
        let temp = tempfile::tempdir().unwrap();
        init_git(temp.path()).await;
        cli(temp.path(), &["init"]).await;
        let ignore = temp.path().join(".robostickies/.gitignore");
        fs::write(&ignore, "/cache/\n").unwrap();
        let client = start_mcp(temp.path()).await;
        let report = call_mcp(&client, "validate_store", json!({})).await;
        assert_eq!(report["healthy"], false);
        assert!(
            report["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["code"] == "missing_ignore")
        );
        let output = command(temp.path())
            .arg("doctor")
            .bounded_output()
            .await
            .unwrap();
        assert_eq!(output.status.code(), Some(65));
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap()["healthy"],
            false
        );
        call_mcp(&client, "init_store", json!({})).await;
        assert_eq!(
            call_mcp(&client, "validate_store", json!({})).await["healthy"],
            true
        );
        client.cancel().await.unwrap();
    })
    .await
    .expect("doctor exclusion test timed out");
}

#[tokio::test]
async fn public_reads_do_not_export_or_depend_on_private_configuration_errors() {
    tokio::time::timeout(Duration::from_secs(60), async {
        for layer in ["private", "user"] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path();
            cli(root, &["init"]).await;
            cli(root, &["add", "Public review fixture", "--public"]).await;
            cli(root, &["add", "PRIVATE_CONFIG_SENTINEL", "--private"]).await;
            let config = if layer == "private" {
                root.join(".robostickies/private/config.toml")
            } else {
                root.join("user.toml")
            };
            let mut process = Command::new(env!("CARGO_BIN_EXE_robostickies"));
            process.kill_on_drop(true).current_dir(root);
            if layer == "user" {
                process.args(["--user-config", config.to_str().unwrap()]);
            } else {
                process.arg("--no-user-config");
            }
            process.arg("mcp");
            let client = ().serve(TokioChildProcess::new(process).unwrap()).await.unwrap();
            for text in [
                "data_version = 'PRIVATE_CONFIG_SENTINEL'\n",
                "[tags]\nprivate_patterns = ['PRIVATE_CONFIG_SENTINEL[']\n",
                "PRIVATE_CONFIG_SENTINEL = [\n",
            ] {
                fs::write(&config, text).unwrap();
                assert_eq!(
                    error_mcp(&client, "get_config", json!({})).await["error"]["code"],
                    "configuration"
                );
                for (tool, input, args) in [
                    (
                        "get_context",
                        json!({"scope":"public"}),
                        vec!["context", "--scope", "public"],
                    ),
                    (
                        "list_tags",
                        json!({"scope":"public"}),
                        vec!["tags", "list", "--scope", "public"],
                    ),
                    (
                        "list_notes",
                        json!({"scope":"public"}),
                        vec!["list", "--scope", "public"],
                    ),
                    ("review_notes", json!({}), vec!["review"]),
                ] {
                    let result = call_mcp(&client, tool, input).await;
                    assert!(!result.to_string().contains("PRIVATE_CONFIG_SENTINEL"));
                    if let Some(total) = result.get("total") {
                        assert_eq!(total, 1);
                    }
                    let mut command = Command::new(env!("CARGO_BIN_EXE_robostickies"));
                    command.args(["--repo", root.to_str().unwrap(), "--json"]);
                    if layer == "user" {
                        command.args(["--user-config", config.to_str().unwrap()]);
                    } else {
                        command.arg("--no-user-config");
                    }
                    let output = command.args(args).bounded_output().await.unwrap();
                    assert!(
                        output.status.success(),
                        "{}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                    assert!(
                        !String::from_utf8_lossy(&output.stdout)
                            .contains("PRIVATE_CONFIG_SENTINEL")
                    );
                    assert!(
                        !String::from_utf8_lossy(&output.stderr)
                            .contains("PRIVATE_CONFIG_SENTINEL")
                    );
                }
                assert_eq!(fs::read_to_string(&config).unwrap(), text);
            }
            client.cancel().await.unwrap();
        }
    })
    .await
    .expect("public configuration isolation timed out");
}

#[cfg(unix)]
#[tokio::test]
async fn filesystem_failures_keep_cli_error_json_and_source_files_intact() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    tokio::time::timeout(Duration::from_secs(30), async {
        let temp = tempfile::tempdir().unwrap();
        let permissions = fs::metadata(temp.path()).unwrap().permissions();
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o555)).unwrap();
        let output = command(temp.path())
            .arg("init")
            .bounded_output()
            .await
            .unwrap();
        fs::set_permissions(temp.path(), permissions).unwrap();
        assert_eq!(output.status.code(), Some(74));
        let store = robostickies::Store::with_user_config(temp.path(), None).unwrap();
        fs::create_dir_all(store.directory().join(".gitignore")).unwrap();
        let output = command(temp.path())
            .arg("init")
            .bounded_output()
            .await
            .unwrap();
        assert_eq!(output.status.code(), Some(74));
        assert!(!store.directory().join("config.toml").exists());
        fs::remove_dir(store.directory().join(".gitignore")).unwrap();
        cli(temp.path(), &["init"]).await;
        let agents = temp.path().join("AGENTS.md");
        let broken = "<!-- BEGIN robostickies -->\nKeep this incomplete block.\n";
        fs::write(&agents, broken).unwrap();
        let output = command(temp.path())
            .args(["init", "--agents"])
            .bounded_output()
            .await
            .unwrap();
        assert_eq!(output.status.code(), Some(78));
        assert_eq!(fs::read_to_string(&agents).unwrap(), broken);
        let lock = store.directory().join("cache/store.lock");
        let permissions = fs::metadata(&lock).unwrap().permissions();
        fs::set_permissions(&lock, fs::Permissions::from_mode(0o444)).unwrap();
        let output = command(temp.path())
            .arg("doctor")
            .bounded_output()
            .await
            .unwrap();
        fs::set_permissions(&lock, permissions).unwrap();
        assert_eq!(output.status.code(), Some(74));
        let outside = tempfile::NamedTempFile::new().unwrap();
        fs::write(outside.path(), "Outside fixture").unwrap();
        symlink(
            outside.path(),
            store.directory().join("cache/index.failed.redb"),
        )
        .unwrap();
        let output = command(temp.path())
            .arg("list")
            .bounded_output()
            .await
            .unwrap();
        assert_eq!(output.status.code(), Some(74));
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["code"],
            "io"
        );
        assert_eq!(
            fs::read_to_string(outside.path()).unwrap(),
            "Outside fixture"
        );
        let linked = tempfile::tempdir().unwrap();
        let store = robostickies::Store::with_user_config(linked.path(), None).unwrap();
        fs::create_dir(store.directory()).unwrap();
        symlink(outside.path(), store.directory().join("config.toml")).unwrap();
        assert_eq!(
            store
                .init(robostickies::model::InitOptions::default())
                .unwrap_err()
                .code,
            "io"
        );
        assert_eq!(
            fs::read_to_string(outside.path()).unwrap(),
            "Outside fixture"
        );
    })
    .await
    .expect("filesystem failure tests timed out");
}

#[tokio::test]
async fn compiled_mcp_identity_and_body_hints_interoperate_with_the_cli() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let temp = tempfile::tempdir().unwrap();
        let client = start_mcp(temp.path()).await;
        let identity = call_mcp(&client, "version", json!({})).await;
        assert_eq!(identity, serde_json::to_value(robostickies::build_info::version()).unwrap());
        assert!(!temp.path().join(".robostickies").exists());
        let uninitialized = call_mcp(&client, "get_config", json!({})).await;
        assert_eq!(uninitialized["upgrade"]["requirement"], "none");
        assert_eq!(uninitialized["upgrade"]["can_upgrade"], false);
        assert_eq!(resource_mcp(&client, "robostickies://config").await, uninitialized);
        let long = command(temp.path()).arg("--version").bounded_output().await.unwrap();
        let long = String::from_utf8(long.stdout).unwrap();
        for (key, label) in [("commit", "Commit"), ("source_state", "Source state"), ("target", "Target"), ("compiler", "Compiler"), ("profile", "Build profile")] {
            assert!(long.contains(&format!("{label}: {}", identity[key].as_str().unwrap())));
        }
        call_mcp(&client, "init_store", json!({})).await;
        let created = cli(temp.path(), &["add", "key: value", "--private", "--body-format", "yaml"]).await;
        let id = created["note"]["id"].as_str().unwrap();
        let read = call_mcp(&client, "get_note", json!({"id": id})).await;
        assert_eq!(read["metadata"]["body_format"], "yaml");
        let edited = call_mcp(&client, "update_note", json!({"id": id, "expected_revision": read["revision"], "body_format":"custom/type"})).await;
        let before = fs::read(temp.path().join(".robostickies").join(edited["note"]["path"].as_str().unwrap())).unwrap();
        assert_eq!(error_mcp(&client, "update_note", json!({"id":id, "expected_revision":edited["note"]["revision"], "body_format":"text", "clear_body_format":true})).await["error"]["code"], "invalid_input");
        let conflict = command(temp.path()).args(["update", id, "--if-match", edited["note"]["revision"].as_str().unwrap(), "--body-format", "text", "--clear-body-format"]).bounded_output().await.unwrap();
        assert_eq!(conflict.status.code(), Some(2));
        assert_eq!(fs::read(temp.path().join(".robostickies").join(edited["note"]["path"].as_str().unwrap())).unwrap(), before);
        for (hint, expected) in [(json!({}), Value::Null), (json!({"body_format":"text"}), json!("text"))] {
            let mut args = json!({"id":id, "expected_revision":edited["note"]["revision"], "dry_run":true});
            args.as_object_mut().unwrap().extend(hint.as_object().unwrap().clone());
            assert_eq!(call_mcp(&client, "publish_note", args).await["note"]["metadata"].get("body_format").cloned().unwrap_or_default(), expected);
        }
        let cleared = cli(temp.path(), &["update", id, "--if-match", edited["note"]["revision"].as_str().unwrap(), "--clear-body-format"]).await;
        assert!(cleared["note"]["metadata"].as_object().unwrap().is_empty());
        let config = temp.path().join(".robostickies/config.toml");
        fs::write(config, "format_version = 1\ndata_version = 'future'\n").unwrap();
        assert_eq!(call_mcp(&client, "version", json!({})).await, identity);
        client.cancel().await.unwrap();
    }).await.expect("identity/hint interoperation timed out");
}

#[tokio::test]
async fn upgrade_deferral_notices_and_snapshot_privacy_cross_cli_and_mcp() {
    tokio::time::timeout(Duration::from_secs(30), async {
        let temp = tempfile::tempdir().unwrap();
        init_git(temp.path()).await;
        cli(temp.path(), &["init"]).await;
        let config = temp.path().join(".robostickies/config.toml");
        fs::write(&config, "format_version = 1\n").unwrap();
        let client = start_mcp(temp.path()).await;
        let public = call_mcp(&client, "get_context", json!({"scope":"public"})).await;
        let declined = cli(temp.path(), &["upgrade", "--defer"]).await;
        assert_eq!(declined["deferred"], true);
        assert!(declined["snapshot"].is_null());
        assert_eq!(
            call_mcp(&client, "get_context", json!({"scope":"public"})).await,
            public
        );
        assert_eq!(
            resource_mcp(&client, "robostickies://config").await["upgrade"]["deferred"],
            true
        );
        let status = command(temp.path())
            .arg("config")
            .arg("show")
            .bounded_output()
            .await
            .unwrap();
        assert!(status.status.success() && status.stderr.is_empty());
        let shared = command(temp.path())
            .args(["context", "--scope", "public"])
            .bounded_output()
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&shared.stderr).contains("SHOULD"));
        let preference = temp
            .path()
            .join(".robostickies/private/upgrade-preferences.toml");
        fs::write(&preference, "malformed").unwrap();
        let warned = command(temp.path())
            .args(["config", "show"])
            .bounded_output()
            .await
            .unwrap();
        assert!(warned.status.success());
        assert!(String::from_utf8_lossy(&warned.stderr).contains("recommendations remain enabled"));
        assert_eq!(
            call_mcp(&client, "get_context", json!({"scope":"public"})).await,
            public
        );
        // Exclusion checks fail closed, even though the root cache remains usable.
        let ignore = temp.path().join(".robostickies/.gitignore");
        fs::write(&ignore, "/cache/\n").unwrap();
        let rejected = error_mcp(&client, "upgrade_store", json!({})).await;
        assert_eq!(rejected["error"]["code"], "privacy");
        assert_eq!(fs::read_to_string(&config).unwrap(), "format_version = 1\n");
        assert!(!temp.path().join(".robostickies/private/snapshots").exists());
        fs::write(&ignore, "/private/\n/cache/\n").unwrap();
        let kept = command(temp.path())
            .args(["upgrade", "--keep-snapshot"])
            .bounded_output()
            .await
            .unwrap();
        assert!(
            kept.status.success(),
            "{}",
            String::from_utf8_lossy(&kept.stderr)
        );
        let result: Value = serde_json::from_slice(&kept.stdout).unwrap();
        assert_eq!(result["wrote"], true);
        assert_eq!(result["snapshot"]["retained"], true);
        assert!(String::from_utf8_lossy(&kept.stderr).contains("Snapshot:"));
        let retained = temp
            .path()
            .join(result["snapshot"]["path"].as_str().unwrap());
        assert_eq!(
            fs::read_to_string(retained.join("config.toml")).unwrap(),
            "format_version = 1\n"
        );
        let ignored = Command::new("git")
            .current_dir(temp.path())
            .args(["check-ignore", "--no-index", "-q", "--"])
            .arg(retained)
            .bounded_output()
            .await
            .unwrap();
        assert!(ignored.status.success());
        client.cancel().await.unwrap();
    })
    .await
    .expect("upgrade interface verification timed out");
}

#[cfg(unix)]
#[tokio::test]
async fn fifo_preferences_warn_without_blocking_and_config_sources_are_rejected() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let temp = tempfile::tempdir().unwrap();
        let client = start_mcp(temp.path()).await;
        call_mcp(&client, "init_store", json!({})).await;
        let directory = temp.path().join(".robostickies");
        let config = directory.join("config.toml");
        let original = "format_version = 1\n";
        fs::write(&config, original).unwrap();
        let public_before = call_mcp(&client, "get_context", json!({"scope":"public"})).await;
        let preference = directory.join("private/upgrade-preferences.toml");
        let created = Command::new("mkfifo")
            .arg(&preference)
            .bounded_output()
            .await
            .unwrap();
        assert!(created.status.success());
        let inspected = call_mcp(&client, "get_config", json!({})).await;
        assert_eq!(inspected["warnings"].as_array().unwrap().len(), 1);
        assert_eq!(inspected["upgrade"]["requirement"], "should");
        assert_eq!(inspected["upgrade"]["deferred"], false);
        assert_eq!(
            call_mcp(&client, "get_context", json!({"scope":"public"})).await,
            public_before
        );
        let output = command(temp.path())
            .args(["config", "show"])
            .bounded_output()
            .await
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("recommendations remain enabled"));
        let failed = error_mcp(&client, "upgrade_store", json!({})).await;
        assert_eq!(failed["error"]["code"], "io");
        assert_eq!(fs::read_to_string(&config).unwrap(), original);
        assert!(!directory.join("private/snapshots").exists());
        fs::remove_file(preference).unwrap();
        // Shared configuration cannot wait for a FIFO writer either.
        fs::remove_file(&config).unwrap();
        let created = Command::new("mkfifo")
            .arg(&config)
            .bounded_output()
            .await
            .unwrap();
        assert!(created.status.success());
        assert_eq!(
            error_mcp(&client, "get_config", json!({})).await["error"]["code"],
            "configuration"
        );
        assert_eq!(
            error_mcp(
                &client,
                "create_note",
                json!({"body":"must not be created"})
            )
            .await["error"]["code"],
            "configuration"
        );
        fs::remove_file(&config).unwrap();
        fs::write(&config, original).unwrap();
        assert_eq!(
            call_mcp(&client, "validate_store", json!({})).await["healthy"],
            true
        );
        client.cancel().await.unwrap();
    })
    .await
    .expect("FIFO configuration checks timed out");
}
