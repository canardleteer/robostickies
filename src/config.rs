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
    version::{self, DataVersion, UpgradeAdvice},
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
    pub upgrade: UpgradeAdvice,
    /// Local advisory warnings. These never appear in shared/public configuration.
    pub warnings: Vec<String>,
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
        format_version: version::FORMAT_VERSION,
        data_version: version::LEGACY,
        upgrade_available: false,
        upgrade: UpgradeAdvice::new(version::LEGACY, None),
        warnings: vec![],
        tags: TagDefaults::default(),
        sources: vec![],
        user_config_path: user.map(|p| p.display().to_string()),
    };
    let shared = root.join("config.toml");
    let mut initialized = false;
    let mut paths = vec![shared.clone()];
    if let Some(path) = user {
        paths.push(path.to_path_buf());
    }
    if include_private {
        paths.push(root.join("private/config.toml"));
    }
    for path in paths {
        let text = match fs::metadata(&path).and_then(|metadata| {
            if !metadata.is_file() {
                return Err(std::io::Error::other(
                    "Configuration sources must be regular files",
                ));
            }
            fs::read_to_string(&path)
        }) {
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
            initialized = true;
            if layer.format_version.is_none() {
                return Err(Error::new(
                    "configuration",
                    format!(
                        "Store config.toml must contain format_version = {}",
                        version::FORMAT_VERSION
                    ),
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
            && version != version::FORMAT_VERSION
        {
            return Err(Error::new(
                "configuration",
                format!(
                    "Unsupported format_version {version} in {}; expected {}. Use compatible tooling for newer formats; do not downgrade the version marker",
                    path.display(),
                    version::FORMAT_VERSION
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
    let deferred = if include_private {
        match read_preferences(root) {
            Ok(value) => value,
            Err(()) => {
                config.warnings.push("Invalid or unreadable local upgrade preferences; recommendations remain enabled.".into());
                None
            }
        }
    } else {
        None
    };
    config.upgrade = if initialized {
        UpgradeAdvice::new(config.data_version, deferred.as_deref())
    } else {
        UpgradeAdvice::uninitialized()
    };
    Ok(config)
}

pub(crate) const PREFERENCES: &str = "private/upgrade-preferences.toml";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UpgradePreferences {
    deferred_target: String,
}

fn read_preferences(root: &Path) -> std::result::Result<Option<String>, ()> {
    crate::document::safe_path(root, Path::new(PREFERENCES)).map_err(|_| ())?;
    let path = root.join(PREFERENCES);
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(()),
    };
    if !metadata.is_file() {
        return Err(());
    }
    let text = fs::read_to_string(path).map_err(|_| ())?;
    let preferences: UpgradePreferences = toml::from_str(&text).map_err(|_| ())?;
    if preferences.deferred_target.trim().is_empty() {
        return Err(());
    }
    Ok(Some(preferences.deferred_target))
}

/// Validate a transformed shared configuration before its atomic replacement.
pub(crate) fn validate_migration(text: &str, target: &str) -> Result<()> {
    let layer: Layer = toml::from_str(text)
        .map_err(|_| Error::new("configuration", "Migration produced invalid configuration"))?;
    if layer.format_version != Some(version::FORMAT_VERSION)
        || layer.data_version.as_deref() != Some(target)
    {
        return Err(Error::new(
            "configuration",
            "Migration did not produce its declared target",
        ));
    }
    target.parse::<DataVersion>()?;
    for pattern in layer.tags.private_patterns.unwrap_or_default() {
        Glob::new(&pattern).map_err(|_| {
            Error::new("configuration", "Migration produced an invalid tag pattern")
        })?;
    }
    Ok(())
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

    #[test]
    fn future_configuration_formats_require_compatible_tooling_without_rewriting() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.toml");
        let text = "format_version = 2\ndata_version = 'v1alpha2'\n";
        fs::write(&path, text).unwrap();
        let error = load_shared(temp.path()).unwrap_err();
        assert_eq!(error.code, "configuration");
        assert!(error.message.contains("compatible tooling"));
        assert!(error.message.contains("do not downgrade"));
        assert_eq!(fs::read_to_string(path).unwrap(), text);
    }

    #[test]
    fn transformed_configuration_must_validate_its_declared_target() {
        for text in [
            "not toml",
            "format_version = 2\ndata_version = 'v1alpha2'\n",
            "format_version = 1\ndata_version = 'v1alpha1'\n",
            "format_version = 1\ndata_version = 'v1alpha2'\n[tags]\nprivate_patterns = ['[']\n",
        ] {
            assert_eq!(
                validate_migration(text, version::CURRENT.as_str())
                    .unwrap_err()
                    .code,
                "configuration"
            );
        }
        assert_eq!(
            validate_migration("format_version = 1\ndata_version = 'future'\n", "future")
                .unwrap_err()
                .code,
            "configuration"
        );
        assert!(
            validate_migration(
                "format_version = 1\ndata_version = 'v1alpha4'\n",
                version::CURRENT.as_str()
            )
            .is_ok()
        );
    }
}
