//! Version details captured during compilation, without runtime repository access.

use std::sync::OnceLock;

use rmcp::schemars::{self, JsonSchema};
use serde::Serialize;

mod generated {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
    include!(concat!(env!("OUT_DIR"), "/source.rs"));
}

/// Identity of the running artifact, captured at compilation.
/// Reading this value performs no filesystem or repository operations.
#[derive(Debug, Clone, Copy, Serialize, JsonSchema)]
pub struct BuildInfo {
    /// Compiled package name.
    pub name: &'static str,
    /// Compiled package SemVer.
    pub version: &'static str,
    /// Full source commit, or unknown when provenance is unavailable.
    pub commit: &'static str,
    /// Tracked source state: clean, dirty, or unknown.
    pub source_state: &'static str,
    /// Rust target triple.
    pub target: &'static str,
    /// Compiler version used to build this artifact.
    pub compiler: &'static str,
    /// Cargo build profile.
    pub profile: &'static str,
}

/// Return the compiled identity, even outside an initialized store.
pub const fn version() -> BuildInfo {
    BuildInfo {
        name: env!("CARGO_PKG_NAME"),
        version: env!("CARGO_PKG_VERSION"),
        commit: generated::COMMIT,
        source_state: generated::SOURCE_STATE,
        target: generated::TARGET,
        compiler: generated::RUSTC_VERSION,
        profile: generated::PROFILE,
    }
}

pub(crate) fn long_version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| {
        let build = version();
        format!(
            "{}\nCommit: {}\nSource state: {}\nTarget: {}\nCompiler: {}\nBuild profile: {}",
            build.version,
            build.commit,
            build.source_state,
            build.target,
            build.compiler,
            build.profile,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_contains_only_the_selected_build_details() {
        let rendered = long_version();
        assert_eq!(rendered, long_version());
        let lines: Vec<_> = rendered.lines().collect();
        assert_eq!(lines.len(), 6);
        assert_eq!(lines[0], env!("CARGO_PKG_VERSION"));
        assert_eq!(lines[1], format!("Commit: {}", generated::COMMIT));
        assert_eq!(
            lines[2],
            format!("Source state: {}", generated::SOURCE_STATE)
        );
        assert_eq!(lines[3], format!("Target: {}", generated::TARGET));
        assert_eq!(lines[4], format!("Compiler: {}", generated::RUSTC_VERSION));
        assert_eq!(lines[5], format!("Build profile: {}", generated::PROFILE));
    }
}
