//! Layered TOML configuration and platform-standard user configuration lookup.

use std::{
    fs,
    path::{Path, PathBuf},
};

use directories::BaseDirs;
use globset::Glob;
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

use crate::{
    Error, Result,
    version::{self, DataVersion},
};

/// Tag-creation defaults. Explicit creation arguments always take precedence.
#[derive(Debug, Default, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct TagDefaults {
    pub default_private: bool,
    pub private_patterns: Vec<String>,
}

impl TagDefaults {
    pub(crate) fn is_private(&self, name: &str, explicit: Option<bool>) -> Result<bool> {
        if let Some(value) = explicit {
            return Ok(value);
        }
        for pattern in &self.private_patterns {
            if Glob::new(pattern)
                .map_err(|e| {
                    Error::new(
                        "configuration",
                        format!("Invalid tag privacy pattern {pattern:?}: {e}"),
                    )
                })?
                .compile_matcher()
                .is_match(name)
            {
                return Ok(true);
            }
        }
        Ok(self.default_private)
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct TagLayer {
    default_private: Option<bool>,
    private_patterns: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Layer {
    format_version: Option<u32>,
    data_version: Option<String>,
    tags: TagLayer,
}

/// Fully resolved configuration, with sources in increasing precedence order.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Configuration {
    pub format_version: u32,
    /// Shared store version; user and private configuration cannot override it.
    pub data_version: DataVersion,
    /// Derived from the shared version; legacy stores remain readable and writable.
    pub upgrade_available: bool,
    pub tags: TagDefaults,
    pub sources: Vec<String>,
    pub user_config_path: Option<String>,
}

/// Platform-standard config location, including XDG_CONFIG_HOME support on Linux.
pub fn user_config_path() -> Option<PathBuf> {
    BaseDirs::new().map(|dirs| dirs.config_dir().join("robostickies.toml"))
}

pub(crate) fn load(root: &Path, user: Option<&Path>) -> Result<Configuration> {
    load_layers(root, user, true)
}

pub(crate) fn load_shared(root: &Path) -> Result<Configuration> {
    load_layers(root, None, false)
}

fn load_layers(root: &Path, user: Option<&Path>, include_private: bool) -> Result<Configuration> {
    let mut config = Configuration {
        format_version: 1,
        data_version: version::LEGACY,
        upgrade_available: false,
        tags: TagDefaults::default(),
        sources: vec![],
        user_config_path: user.map(|p| p.display().to_string()),
    };
    let shared = root.join("config.toml");
    let mut paths = vec![shared.clone()];
    if let Some(path) = user {
        paths.push(path.to_path_buf());
    }
    if include_private {
        paths.push(root.join("private/config.toml"));
    }
    for path in paths {
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(Error::new(
                    "configuration",
                    format!("Cannot read {}: {e}", path.display()),
                ));
            }
        };
        let layer: Layer = toml::from_str(&text).map_err(|e| {
            Error::new(
                "configuration",
                format!("Invalid configuration {}: {e}", path.display()),
            )
        })?;
        if path == shared {
            if layer.format_version != Some(1) {
                return Err(Error::new(
                    "configuration",
                    "Store config.toml must contain format_version = 1",
                ));
            }
            config.data_version = layer
                .data_version
                .as_deref()
                .map(str::parse)
                .transpose()?
                .unwrap_or(version::LEGACY);
            config.upgrade_available = config.data_version.upgrade_available();
        } else if layer.data_version.is_some() {
            return Err(Error::new(
                "configuration",
                format!(
                    "data_version is allowed only in the shared store config.toml; remove it from {}",
                    path.display()
                ),
            ));
        }
        if let Some(version) = layer.format_version
            && version != 1
        {
            return Err(Error::new(
                "configuration",
                format!(
                    "Unsupported format_version {version} in {}; expected 1",
                    path.display()
                ),
            ));
        }
        if let Some(value) = layer.tags.default_private {
            config.tags.default_private = value;
        }
        if let Some(value) = layer.tags.private_patterns {
            config.tags.private_patterns = value;
        }
        config.sources.push(path.display().to_string());
    }
    for pattern in &config.tags.private_patterns {
        Glob::new(pattern).map_err(|e| {
            Error::new(
                "configuration",
                format!("Invalid tag privacy pattern {pattern:?}: {e}"),
            )
        })?;
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privacy_patterns_validate_and_explicit_choices_override_them() {
        let defaults = TagDefaults {
            default_private: false,
            private_patterns: vec!["local-*".into()],
        };
        assert!(defaults.is_private("local-test", None).unwrap());
        assert!(!defaults.is_private("other", None).unwrap());
        assert!(!defaults.is_private("local-test", Some(false)).unwrap());
        let broken = TagDefaults {
            default_private: false,
            private_patterns: vec!["[".into()],
        };
        assert_eq!(
            broken.is_private("test", None).unwrap_err().code,
            "configuration"
        );
        assert!(broken.is_private("test", Some(true)).unwrap());
    }

    #[test]
    fn layers_report_read_syntax_and_pattern_failures() {
        let temp = tempfile::tempdir().unwrap();
        let user = temp.path().join("user.toml");
        let shared = temp.path().join("config.toml");
        assert!(load(temp.path(), None).unwrap().sources.is_empty());
        fs::create_dir(&shared).unwrap();
        assert_eq!(load(temp.path(), None).unwrap_err().code, "configuration");
        fs::remove_dir(&shared).unwrap();
        fs::write(&shared, "format_version = 1\n").unwrap();
        for text in [
            "format_version = 2\n",
            "[tags]\nprivate_patterns = ['[']\n",
            "unknown = true\n",
            "[tags]\nunknown = true\n",
        ] {
            fs::write(&user, text).unwrap();
            assert_eq!(
                load(temp.path(), Some(&user)).unwrap_err().code,
                "configuration"
            );
        }
        assert!(user_config_path().is_some());
    }
}
