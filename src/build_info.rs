//! Version details captured during compilation, without runtime repository access.

use std::sync::OnceLock;

mod generated {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
    include!(concat!(env!("OUT_DIR"), "/source.rs"));
}

pub(crate) fn long_version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| {
        format!(
            "{}\nCommit: {}\nSource state: {}\nTarget: {}\nCompiler: {}\nBuild profile: {}",
            env!("CARGO_PKG_VERSION"),
            generated::COMMIT,
            generated::SOURCE_STATE,
            generated::TARGET,
            generated::RUSTC_VERSION,
            generated::PROFILE,
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
