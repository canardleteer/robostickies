//! Synthetic interface baselines. Review each change for Rust/CLI/MCP compatibility.
mod support;

use std::{path::Path, time::Duration};

use rmcp::{ServiceExt, model::*, transport::TokioChildProcess};
use serde_json::{Value, json};
use support::CommandOutput;
use tokio::process::Command;

fn command(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_robostickies"));
    command
        .args(["--repo", root.to_str().unwrap(), "--no-user-config"])
        .env("COLUMNS", "100")
        .env("NO_COLOR", "1")
        .kill_on_drop(true);
    command
}

#[tokio::test]
async fn cli_help_contract() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let temp = tempfile::tempdir().unwrap();
        let mut help = String::new();
        for args in [
            vec![],
            vec!["add"],
            vec!["update"],
            vec!["tags"],
            vec!["tags", "create"],
            vec!["publish"],
            vec!["upgrade"],
        ] {
            let output = command(temp.path())
                .args(&args)
                .arg("--help")
                .bounded_output()
                .await
                .unwrap();
            assert!(output.status.success());
            assert!(output.stderr.is_empty());
            help.push_str(&format!(
                "$ robostickies {}--help\n{}\n",
                if args.is_empty() {
                    String::new()
                } else {
                    format!("{} ", args.join(" "))
                },
                String::from_utf8(output.stdout).unwrap()
            ));
        }
        // Clap pads undocumented argument rows; terminal padding is not a contract.
        let help = help
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n");
        insta::assert_snapshot!("cli_help", help);
    })
    .await
    .expect("help snapshot timed out");
}

#[tokio::test]
async fn mcp_discovery_contract() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let temp = tempfile::tempdir().unwrap();
        let mut process = command(temp.path());
        process.arg("mcp");
        let client = ().serve(TokioChildProcess::new(process).unwrap()).await.unwrap();
        let mut tools = client.list_all_tools().await.unwrap();
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        let mut resources = client.list_all_resources().await.unwrap();
        resources.sort_by(|a, b| a.uri.cmp(&b.uri));
        client.cancel().await.unwrap();
        // Full actual schemas and annotations, rather than a hand-maintained facsimile.
        insta::assert_json_snapshot!(
            "mcp_discovery",
            json!({"tools": tools, "resources": resources})
        );
    })
    .await
    .expect("MCP discovery snapshot timed out");
}

fn normalize_saved(mut value: Value) -> Value {
    let note = &mut value["note"];
    let id = note["id"].as_str().unwrap().to_owned();
    note["path"] = json!(note["path"].as_str().unwrap().replace(&id, "<uuid>"));
    note["id"] = json!("<uuid>");
    note["revision"] = json!("<revision>");
    note["created_at"] = json!("<timestamp>");
    note["updated_at"] = json!("<timestamp>");
    value
}

#[tokio::test]
async fn cli_output_and_error_contract() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let temp = tempfile::tempdir().unwrap();
        let init = command(temp.path())
            .arg("init")
            .bounded_output()
            .await
            .unwrap();
        assert!(init.status.success());
        let output = command(temp.path())
            .args([
                "--json",
                "add",
                "Synthetic contract example",
                "--private",
                "--body-format",
                "text",
            ])
            .bounded_output()
            .await
            .unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let saved = normalize_saved(serde_json::from_slice(&output.stdout).unwrap());
        let failed = command(temp.path())
            .args([
                "--json",
                "add",
                "Synthetic rejected example",
                "--body-format",
                " ",
            ])
            .bounded_output()
            .await
            .unwrap();
        assert_eq!(failed.status.code(), Some(65));
        assert!(failed.stdout.is_empty());
        let error: Value = serde_json::from_slice(&failed.stderr).unwrap();
        insta::assert_json_snapshot!(
            "cli_envelopes",
            json!({"saved": saved, "error": error, "error_exit": failed.status.code()})
        );
    })
    .await
    .expect("CLI envelope snapshot timed out");
}

#[tokio::test]
async fn mcp_output_and_error_contract() {
    tokio::time::timeout(Duration::from_secs(20), async {
        let temp = tempfile::tempdir().unwrap();
        let mut process = command(temp.path());
        process.arg("mcp");
        let client = ().serve(TokioChildProcess::new(process).unwrap()).await.unwrap();
        let init = client.call_tool(CallToolRequestParams::new("init_store")).await.unwrap();
        assert_ne!(init.is_error, Some(true));
        let saved = client.call_tool(CallToolRequestParams::new("create_note").with_arguments(json!({"body":"Synthetic contract example", "private":true, "body_format":"text"}).as_object().unwrap().clone())).await.unwrap();
        assert_ne!(saved.is_error, Some(true));
        let error = client.call_tool(CallToolRequestParams::new("create_note").with_arguments(json!({"body":"Synthetic rejected example", "body_format":" "}).as_object().unwrap().clone())).await.unwrap();
        assert_eq!(error.is_error, Some(true));
        client.cancel().await.unwrap();
        insta::assert_json_snapshot!("mcp_envelopes", json!({
            "saved": {"isError": saved.is_error, "structuredContent": normalize_saved(saved.structured_content.unwrap())},
            "error": error,
        }));
    }).await.expect("MCP envelope snapshot timed out");
}
