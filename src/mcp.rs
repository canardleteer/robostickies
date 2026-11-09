//! Direct rmcp server over stdio, with tools and resources sharing [`Store`].

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    schemars::{self, JsonSchema},
    service::RequestContext,
    tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};

use crate::{
    Store,
    model::{
        CreateNote, CreateTag, InitOptions, PublishNote, Query, ReviewQuery, Scope, UpdateNote,
        UpgradeOptions,
    },
    store::DISCOVERY_GUIDANCE,
};

/// An MCP service bound to one repository for its lifetime.
#[derive(Clone)]
pub struct StickyServer {
    store: Store,
    tool_router: ToolRouter<Self>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct NoteId {
    id: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TagName {
    name: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EmptyInput {}

#[derive(Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct Scoped {
    scope: Scope,
}

#[derive(Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
struct TagsQuery {
    scope: Scope,
    text: Option<String>,
}

impl StickyServer {
    /// Construct a service without changing the repository or requiring initialization.
    pub fn new(store: Store) -> Self {
        Self {
            store,
            tool_router: Self::tool_router(),
        }
    }

    async fn execute<T: Serialize + Send + 'static>(
        &self,
        operation: impl FnOnce(Store) -> crate::Result<T> + Send + 'static,
    ) -> CallToolResult {
        let store = self.store.clone();
        match blocking(store, operation).await {
            Ok(value) => match serde_json::to_value(value) {
                Ok(value) => CallToolResult::structured(value),
                Err(_) => CallToolResult::structured_error(
                    serde_json::json!({"error": {"code": "internal", "message": "Cannot encode tool result"}}),
                ),
            },
            Err(error) => CallToolResult::structured_error(serde_json::json!({"error": error})),
        }
    }
}

#[tool_router]
impl StickyServer {
    /// Identify this running process from compiled build data, without reading a store.
    /// Use this after installation: replacing the CLI does not refresh an existing MCP process.
    #[tool(output_schema = rmcp::handler::server::common::schema_for_output::<crate::build_info::BuildInfo>(), annotations(read_only_hint = true, open_world_hint = false))]
    async fn version(&self, Parameters(_): Parameters<EmptyInput>) -> CallToolResult {
        CallToolResult::structured(serde_json::json!(crate::build_info::version()))
    }

    /// Call this at session start. Inspect upgrade.requirement, upgrade.message,
    /// upgrade.deferred and warnings even when validation is healthy. Then read tag
    /// topics and startup guidance. Also returns configuration and an AGENTS.md snippet.
    #[tool(output_schema = rmcp::handler::server::common::schema_for_output::<crate::store::Context>(), annotations(read_only_hint = true, open_world_hint = false))]
    async fn get_context(&self, Parameters(input): Parameters<Scoped>) -> CallToolResult {
        self.execute(move |store| store.context(input.scope)).await
    }

    /// Scaffold a repository's .robostickies store. Existing notes and tag definitions
    /// are preserved. AGENTS.md integration is optional and preserves existing content.
    #[tool(output_schema = rmcp::handler::server::common::schema_for_output::<crate::model::InitResult>(), annotations(destructive_hint = false, idempotent_hint = true, open_world_hint = false))]
    async fn init_store(&self, Parameters(input): Parameters<InitOptions>) -> CallToolResult {
        self.execute(move |store| store.init(input)).await
    }

    /// Explicitly upgrade the shared store data version for both namespaces.
    /// Set dry_run=true to preview, defer=true to decline the optional target, or
    /// keep_snapshot=true to retain recovery data. Note values and customized guidance
    /// are preserved; repaired sources receive new revisions. Re-read before editing.
    #[tool(output_schema = rmcp::handler::server::common::schema_for_output::<crate::version::UpgradeResult>(), annotations(destructive_hint = false, idempotent_hint = true, open_world_hint = false))]
    async fn upgrade_store(&self, Parameters(input): Parameters<UpgradeOptions>) -> CallToolResult {
        self.execute(move |store| store.upgrade_with_options(input))
            .await
    }

    /// Record a brief observation. Prefer papercut for friction and paperclip for
    /// concrete wins; general notes are welcome. Any private:<tag> forces private
    /// storage. Set private=true to keep any note local; omit to derive from tags.
    /// Bodies are nonempty UTF-8 text; optional body_format is an advisory hint.
    #[tool(output_schema = rmcp::handler::server::common::schema_for_output::<crate::model::SavedNote>(), annotations(destructive_hint = false, open_world_hint = false))]
    async fn create_note(&self, Parameters(input): Parameters<CreateNote>) -> CallToolResult {
        self.execute(move |store| store.create_note(input)).await
    }

    /// Read a note by full UUID, including its revision for safe subsequent edits.
    #[tool(output_schema = rmcp::handler::server::common::schema_for_output::<crate::model::Note>(), annotations(read_only_hint = true, open_world_hint = false))]
    async fn get_note(&self, Parameters(input): Parameters<NoteId>) -> CallToolResult {
        self.execute(move |store| store.get_note(&input.id)).await
    }

    /// Edit only specified fields. Use status=archived for resolved/obsolete notes
    /// and status=active to restore them. Private notes remain private; publishing
    /// is separate. expected_revision is required. On conflict, re-read and reconcile
    /// the changes before retrying; there is no overwrite bypass.
    #[tool(output_schema = rmcp::handler::server::common::schema_for_output::<crate::model::SavedNote>(), annotations(destructive_hint = false, open_world_hint = false))]
    async fn update_note(&self, Parameters(input): Parameters<UpdateNote>) -> CallToolResult {
        self.execute(move |store| store.update_note(input)).await
    }

    /// Browse or search notes with visibility, tags, excluded tags, status, text,
    /// date bounds, and pagination. Text is a case-insensitive literal substring.
    /// Defaults to active notes across all visibility. Use scope=public for sharing.
    #[tool(output_schema = rmcp::handler::server::common::schema_for_output::<crate::model::NotePage>(), annotations(read_only_hint = true, open_world_hint = false))]
    async fn list_notes(&self, Parameters(input): Parameters<Query>) -> CallToolResult {
        self.execute(move |store| store.list_notes(input)).await
    }

    /// Prepare complete deterministic review material for summarization. Visibility
    /// defaults to public; private notes must be published first for public review.
    /// Explicit scope=private or all prepares a local review. Paging is internal.
    #[tool(output_schema = rmcp::handler::server::common::schema_for_output::<crate::model::Review>(), annotations(read_only_hint = true, open_world_hint = false))]
    async fn review_notes(&self, Parameters(input): Parameters<ReviewQuery>) -> CallToolResult {
        self.execute(move |store| store.review(input.into())).await
    }

    /// Define a tag using a bare filename slug, a short topic, and Markdown guidance.
    /// Set private=true/false to override configuration defaults and name patterns.
    /// Private tags have distinct private:<name> identities even if public names match.
    #[tool(output_schema = rmcp::handler::server::common::schema_for_output::<crate::model::Tag>(), annotations(destructive_hint = false, open_world_hint = false))]
    async fn create_tag(&self, Parameters(input): Parameters<CreateTag>) -> CallToolResult {
        self.execute(move |store| store.create_tag(input)).await
    }

    /// Discover defined tags and their short topics without loading every full body.
    /// Filter by visibility or a case-insensitive name/topic substring.
    #[tool(output_schema = rmcp::handler::server::common::schema_for_output::<crate::model::TagList>(), annotations(read_only_hint = true, open_world_hint = false))]
    async fn list_tags(&self, Parameters(input): Parameters<TagsQuery>) -> CallToolResult {
        self.execute(move |store| store.list_tags(input.scope, input.text.as_deref()))
            .await
    }

    /// Read complete guidance for a tag, using private:<name> for private definitions.
    #[tool(output_schema = rmcp::handler::server::common::schema_for_output::<crate::model::Tag>(), annotations(read_only_hint = true, open_world_hint = false))]
    async fn get_tag(&self, Parameters(input): Parameters<TagName>) -> CallToolResult {
        self.execute(move |store| store.get_tag(&input.name)).await
    }

    /// Copy a private note into public storage immediately, retaining the original.
    /// Private tags and additional metadata are omitted. Set dry_run=true for a
    /// preview, or provide replacement body/public tags. Review prose before sharing.
    /// expected_revision is required for both previews and copies; a changed source
    /// returns conflict without creating a copy.
    /// A body_format hint is public only when supplied explicitly for this copy.
    #[tool(output_schema = rmcp::handler::server::common::schema_for_output::<crate::model::Publication>(), annotations(destructive_hint = false, open_world_hint = false))]
    async fn publish_note(&self, Parameters(input): Parameters<PublishNote>) -> CallToolResult {
        self.execute(move |store| store.publish_note(input)).await
    }

    /// Inspect effective TOML configuration, privacy defaults, and their source files.
    /// Read upgrade.requirement/message/deferred and warnings for maintenance advice.
    #[tool(output_schema = rmcp::handler::server::common::schema_for_output::<crate::config::Configuration>(), annotations(read_only_hint = true, open_world_hint = false))]
    async fn get_config(&self, Parameters(_): Parameters<EmptyInput>) -> CallToolResult {
        self.execute(|store| store.configuration()).await
    }

    /// Diagnose invalid sources, unresolved tags, and Git tracking/exclusion problems.
    /// Healthy validation does not imply current data: inspect the separate upgrade advice.
    #[tool(output_schema = rmcp::handler::server::common::schema_for_output::<crate::model::Health>(), annotations(read_only_hint = true, open_world_hint = false))]
    async fn validate_store(&self, Parameters(_): Parameters<EmptyInput>) -> CallToolResult {
        self.execute(|store| store.doctor()).await
    }

    /// Rebuild the disposable Rust-native redb index from current Markdown sources.
    /// Source files and Git history remain intact.
    #[tool(output_schema = rmcp::handler::server::common::schema_for_output::<crate::model::Health>(), annotations(destructive_hint = false, idempotent_hint = true, open_world_hint = false))]
    async fn rebuild_index(&self, Parameters(_): Parameters<EmptyInput>) -> CallToolResult {
        self.execute(|store| store.reindex()).await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for StickyServer {
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::default();
        info.server_info = Implementation::new("robostickies", env!("CARGO_PKG_VERSION"));
        info.instructions = Some(DISCOVERY_GUIDANCE.into());
        info.capabilities = ServerCapabilities::builder()
            .enable_tools()
            .enable_resources()
            .build();
        info
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListResourcesResult, McpError> {
        let resources = [
            (
                "robostickies://context",
                "Session context",
                "Tag topics, startup guidance, and optional AGENTS.md paragraph",
            ),
            (
                "robostickies://guidance",
                "Full guidance",
                "How to record, discover, keep private, and publish notes",
            ),
            (
                "robostickies://tags",
                "Tag catalog",
                "Available qualified tag names and short topics",
            ),
            (
                "robostickies://config",
                "Effective configuration",
                "Local configuration defaults and their sources",
            ),
        ]
        .into_iter()
        .map(|(uri, name, description)| {
            Resource::new(uri, name)
                .with_description(description)
                .with_mime_type("application/json")
        })
        .collect();
        Ok(ListResourcesResult {
            resources,
            ..Default::default()
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ReadResourceResponse, McpError> {
        let uri = request.uri;
        let request_uri = uri.clone();
        let store = self.store.clone();
        let text = blocking(store, move |store| -> crate::Result<String> {
            match request_uri.as_str() {
                "robostickies://context" => resource_json(store.context(Scope::All)?),
                "robostickies://guidance" => resource_json(serde_json::json!({"guidance": store.context(Scope::Public)?.guidance, "agents_md": store.agents_snippet()})),
                "robostickies://tags" => {
                    if store.context(Scope::Public)?.initialized { resource_json(store.list_tags(Scope::All, None)?) }
                    else { resource_json(serde_json::json!({"tags": [], "initialized": false, "guidance": DISCOVERY_GUIDANCE})) }
                },
                "robostickies://config" => resource_json(store.configuration()?),
                _ => Err(crate::Error::new("not_found", "Unknown resource URI; list resources to discover supported URIs")),
            }
        }).await.map_err(resource_error)?;
        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(text, uri).with_mime_type("application/json"),
        ])
        .into())
    }
}

async fn blocking<T: Send + 'static>(
    store: Store,
    operation: impl FnOnce(Store) -> crate::Result<T> + Send + 'static,
) -> crate::Result<T> {
    tokio::task::spawn_blocking(move || operation(store))
        .await
        .map_err(|_| crate::Error::new("internal", "Repository operation could not complete"))?
}

fn resource_json(value: impl Serialize) -> crate::Result<String> {
    serde_json::to_string_pretty(&value)
        .map_err(|_| crate::Error::new("internal", "Cannot encode resource"))
}

fn resource_error(error: crate::Error) -> McpError {
    if error.code == "internal" {
        McpError::internal_error(error.message, None)
    } else {
        McpError::invalid_params(error.message, Some(serde_json::json!({"code": error.code})))
    }
}

fn transport_error(error: impl std::fmt::Display) -> crate::Error {
    crate::Error::new("mcp", format!("MCP transport failed: {error}"))
}

/// Serve one repository over stdio until EOF, cancellation, or process interruption.
/// The caller is responsible for initializing logging on stderr.
pub async fn serve(store: Store) -> crate::Result<()> {
    let service = StickyServer::new(store)
        .serve(rmcp::transport::stdio())
        .await
        .map_err(transport_error)?;
    tokio::select! {
        result = service.waiting() => { result.map_err(transport_error)?; },
        result = tokio::signal::ctrl_c() => { result.map_err(crate::Error::from)?; },
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fails;
    impl Serialize for Fails {
        fn serialize<S: serde::Serializer>(&self, _: S) -> std::result::Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("fixture encoding failure"))
        }
    }

    #[tokio::test]
    async fn failed_encoding_and_worker_panics_return_structured_errors() {
        let temp = tempfile::tempdir().unwrap();
        let server = StickyServer::new(Store::with_user_config(temp.path(), None).unwrap());
        let failed = server.execute(|_| Ok(Fails)).await;
        assert_eq!(failed.is_error, Some(true));
        assert_eq!(
            failed.structured_content.unwrap()["error"]["code"],
            "internal"
        );
        fn panic_worker(_: Store) -> crate::Result<()> {
            panic!("fixture worker failure")
        }
        let panicked = server.execute(panic_worker).await;
        assert_eq!(panicked.is_error, Some(true));
        assert_eq!(
            panicked.structured_content.unwrap()["error"]["message"],
            "Repository operation could not complete"
        );
        assert_eq!(resource_json(Fails).unwrap_err().code, "internal");
        let internal = resource_error(crate::Error::new("internal", "worker failed"));
        assert_eq!(internal.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
        let invalid = resource_error(crate::Error::new("not_found", "unknown URI"));
        assert_eq!(invalid.code, rmcp::model::ErrorCode::INVALID_PARAMS);
        assert_eq!(transport_error("closed stream").code, "mcp");
    }
}
