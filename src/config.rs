//! Layered TOML configuration and platform-standard user configuration lookup.

use std::{
    fs,
    path::{Path, PathBuf},
};

use directories::BaseDirs;
use globset::Glob;
use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

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
    tags: TagLayer,
}

/// Fully resolved configuration, with sources in increasing precedence order.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Configuration {
    pub format_version: u32,
    pub tags: TagDefaults,
    pub sources: Vec<String>,
    pub user_config_path: Option<String>,
}

/// Platform-standard config location, including XDG_CONFIG_HOME support on Linux.
pub fn user_config_path() -> Option<PathBuf> {
    BaseDirs::new().map(|dirs| dirs.config_dir().join("robostickies.toml"))
}

pub(crate) fn load(root: &Path, user: Option<&Path>) -> Result<Configuration> {
    let mut config = Configuration {
        format_version: 1,
        tags: TagDefaults::default(),
        sources: vec![],
        user_config_path: user.map(|p| p.display().to_string()),
    };
    let mut paths = vec![root.join("config.toml")];
    if let Some(path) = user {
        paths.push(path.to_path_buf());
    }
    paths.push(root.join("private/config.toml"));
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
