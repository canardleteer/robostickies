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
    model::{CreateNote, CreateTag, InitOptions, PublishNote, Query, Scope, UpdateNote},
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
        let result = tokio::task::spawn_blocking(move || operation(store)).await;
        match result {
            Ok(Ok(value)) => match serde_json::to_value(value) {
                Ok(value) => CallToolResult::structured(value),
                Err(_) => CallToolResult::structured_error(
                    serde_json::json!({"error": {"code": "internal", "message": "Cannot encode tool result"}}),
                ),
            },
            Ok(Err(error)) => CallToolResult::structured_error(serde_json::json!({"error": error})),
            Err(_) => CallToolResult::structured_error(
                serde_json::json!({"error": {"code": "internal", "message": "Repository operation could not complete"}}),
            ),
        }
    }
}

#[tool_router]
impl StickyServer {
    /// Call this at session start to read tag topics and startup guidance.
    /// It also returns effective configuration and an optional AGENTS.md paragraph.
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

    /// Record a brief observation. Prefer papercut for friction and paperclip for
    /// concrete wins; general notes are welcome. Any private:<tag> forces private
    /// storage. Set private=true to keep any note local; omit to derive from tags.
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
    /// is separate. Supply expected_revision to reject stale edits.
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
    /// Explicit scope=private or all prepares a local review. Cursor is not accepted.
    #[tool(output_schema = rmcp::handler::server::common::schema_for_output::<crate::model::Review>(), annotations(read_only_hint = true, open_world_hint = false))]
    async fn review_notes(&self, Parameters(input): Parameters<Query>) -> CallToolResult {
        self.execute(move |store| store.review(input)).await
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
    #[tool(output_schema = rmcp::handler::server::common::schema_for_output::<crate::model::Publication>(), annotations(destructive_hint = false, open_world_hint = false))]
    async fn publish_note(&self, Parameters(input): Parameters<PublishNote>) -> CallToolResult {
        self.execute(move |store| store.publish_note(input)).await
    }

    /// Inspect effective TOML configuration, privacy defaults, and their source files.
    #[tool(output_schema = rmcp::handler::server::common::schema_for_output::<crate::config::Configuration>(), annotations(read_only_hint = true, open_world_hint = false))]
    async fn get_config(&self, Parameters(_): Parameters<EmptyInput>) -> CallToolResult {
        self.execute(|store| store.configuration()).await
    }

    /// Diagnose invalid sources, unresolved tags, and Git tracking/exclusion problems.
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
        let text = tokio::task::spawn_blocking(move || -> crate::Result<String> {
            let value = match request_uri.as_str() {
                "robostickies://context" => serde_json::to_value(store.context(Scope::All)?),
                "robostickies://guidance" => serde_json::to_value(serde_json::json!({"guidance": store.context(Scope::Public)?.guidance, "agents_md": store.agents_snippet()})),
                "robostickies://tags" => {
                    if store.context(Scope::Public)?.initialized { serde_json::to_value(store.list_tags(Scope::All, None)?) }
                    else { serde_json::to_value(serde_json::json!({"tags": [], "initialized": false, "guidance": DISCOVERY_GUIDANCE})) }
                },
                "robostickies://config" => serde_json::to_value(store.configuration()?),
                _ => return Err(crate::Error::new("not_found", "Unknown resource URI; list resources to discover supported URIs")),
            }.map_err(|_| crate::Error::new("internal", "Cannot encode resource"))?;
            serde_json::to_string_pretty(&value).map_err(|_| crate::Error::new("internal", "Cannot encode resource"))
        }).await.map_err(|_| McpError::internal_error("Resource operation could not complete", None))?
            .map_err(|error| McpError::invalid_params(error.message, Some(serde_json::json!({"code": error.code}))))?;
        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(text, uri).with_mime_type("application/json"),
        ])
        .into())
    }
}

/// Serve one repository over stdio until EOF, cancellation, or process interruption.
/// The caller is responsible for initializing logging on stderr.
pub async fn serve(store: Store) -> crate::Result<()> {
    let service = StickyServer::new(store)
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| crate::Error::new("mcp", format!("MCP startup failed: {e}")))?;
    tokio::select! {
        result = service.waiting() => { result.map_err(|e| crate::Error::new("mcp", format!("MCP service failed: {e}")))?; },
        result = tokio::signal::ctrl_c() => { result.map_err(crate::Error::from)?; },
    }
    Ok(())
}
